use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, BindingPattern, CallExpression, Expression,
    FormalParameterKind, FormalParameters, Function, Program, SimpleAssignmentTarget, Statement,
    UnaryExpression, UpdateExpression, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashSet;
use std::borrow::Cow;

/// Labels preserve the enclosing declaration scope of an Annex B function.
pub(super) fn statement_without_labels<'a, 's>(
    mut statement: &'s Statement<'a>,
) -> &'s Statement<'a> {
    while let Statement::LabeledStatement(labelled) = statement {
        statement = &labelled.body;
    }
    statement
}

pub(super) fn normalize_hashbang(source: &str) -> Cow<'_, str> {
    if !source.starts_with("#!") {
        return Cow::Borrowed(source);
    }
    let end = source
        .find(['\n', '\r', '\u{2028}', '\u{2029}'])
        .unwrap_or(source.len());
    let mut normalized = source.to_owned();
    normalized.replace_range(0..end, &" ".repeat(end));
    Cow::Owned(normalized)
}

/// Eval may inherit new.target from a function, but never its Return grammar.
pub(super) fn eval_return_outside_function(program: &Program<'_>) -> Option<oxc_span::Span> {
    struct Returns(Option<oxc_span::Span>);
    impl<'a> Visit<'a> for Returns {
        fn visit_return_statement(&mut self, statement: &oxc_ast::ast::ReturnStatement<'a>) {
            self.0.get_or_insert(statement.span);
        }
        fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    }
    let mut returns = Returns(None);
    returns.visit_program(program);
    returns.0
}

pub(super) fn normalize_dynamic_function_body(source: &str) -> Cow<'_, str> {
    if dynamic_body_is_strict(source) {
        return Cow::Borrowed(source);
    }
    let mut normalized = source.as_bytes().to_vec();
    let mut lexical = DynamicBodyLexicalState::default();
    let mut line_start = true;
    let mut cursor = 0;
    let mut changed = false;
    while cursor < normalized.len() {
        if line_start && lexical.in_code() {
            let mut comment = cursor;
            while matches!(normalized.get(comment), Some(b' ' | b'\t')) {
                comment += 1;
            }
            if normalized.get(comment..comment.saturating_add(3)) == Some(b"-->" as &[u8]) {
                normalized[comment..comment + 3].copy_from_slice(b"// ");
                changed = true;
                cursor = comment + 3;
                line_start = false;
                continue;
            }
        }
        let byte = normalized[cursor];
        lexical.advance(&normalized, &mut cursor);
        line_start = matches!(byte, b'\n' | b'\r') || line_start && matches!(byte, b' ' | b'\t');
    }
    if changed {
        String::from_utf8(normalized).map_or(Cow::Borrowed(source), Cow::Owned)
    } else {
        Cow::Borrowed(source)
    }
}

pub(super) fn normalize_dynamic_function_parameters(source: &str) -> Cow<'_, str> {
    let original = source;
    let source = original.strip_prefix("<!--").unwrap_or(original);
    let mut changed = source.len() != original.len();
    let mut first_line = true;
    let mut lines = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !first_line && trimmed.starts_with("-->") {
            changed = true;
            lines.push(&line[..line.len() - trimmed.len()]);
        } else {
            lines.push(line);
        }
        first_line = false;
    }
    if changed {
        Cow::Owned(lines.join("\n"))
    } else {
        Cow::Borrowed(source)
    }
}

fn dynamic_body_is_strict(source: &str) -> bool {
    let source = source.trim_start();
    ["'use strict'", "\"use strict\""]
        .iter()
        .any(|directive| source.starts_with(directive))
}

#[derive(Default)]
struct DynamicBodyLexicalState {
    quote: Option<u8>,
    template: bool,
    line_comment: bool,
    block_comment: bool,
    escaped: bool,
}

impl DynamicBodyLexicalState {
    fn in_code(&self) -> bool {
        self.quote.is_none() && !self.template && !self.line_comment && !self.block_comment
    }

    fn advance(&mut self, source: &[u8], cursor: &mut usize) {
        let byte = source[*cursor];
        let next = source.get(*cursor + 1).copied();
        *cursor += 1;
        if self.line_comment {
            self.line_comment = !matches!(byte, b'\n' | b'\r');
        } else if self.block_comment {
            if byte == b'*' && next == Some(b'/') {
                self.block_comment = false;
                *cursor += 1;
            }
        } else if self.escaped {
            self.escaped = false;
        } else if byte == b'\\' {
            self.escaped = true;
        } else if let Some(quote) = self.quote {
            if byte == quote {
                self.quote = None;
            } else if matches!(byte, b'\n' | b'\r') {
                self.quote = None;
            }
        } else if self.template {
            if byte == b'`' {
                self.template = false;
            }
        } else {
            match (byte, next) {
                (b'/', Some(b'/')) => {
                    self.line_comment = true;
                    *cursor += 1;
                }
                (b'/', Some(b'*')) => {
                    self.block_comment = true;
                    *cursor += 1;
                }
                (b'\'' | b'\"', _) => self.quote = Some(byte),
                (b'`', _) => self.template = true,
                _ => {}
            }
        }
    }
}

pub(super) fn block_early_error(program: &Program<'_>, strict: bool) -> Option<String> {
    validate_nested(&program.body, strict)
}

pub(super) enum RegExpEarlyError {
    Syntax(String),
    StackExhausted,
}

pub(super) fn regexp_early_error(program: &Program<'_>) -> Option<RegExpEarlyError> {
    let mut validator = RegExpEarlyErrors(None);
    validator.visit_program(program);
    validator.0
}

struct RegExpEarlyErrors(Option<RegExpEarlyError>);

impl<'a> Visit<'a> for RegExpEarlyErrors {
    fn visit_reg_exp_literal(&mut self, literal: &oxc_ast::ast::RegExpLiteral<'a>) {
        let Some(raw) = literal.raw.as_ref() else {
            return;
        };
        let text = raw.as_str();
        let Some(separator) = text.rfind('/') else {
            return;
        };
        let pattern = &text[1..separator];
        let flags = &text[separator + 1..];
        if let Err(error) = validate_modifier_groups(pattern) {
            self.0 = Some(RegExpEarlyError::Syntax(error));
            return;
        }
        if let Err(error) = super::regexp::validate_pattern(pattern, flags) {
            self.0 = Some(if quench_regexp::is_stack_exhaustion_message(&error) {
                RegExpEarlyError::StackExhausted
            } else {
                RegExpEarlyError::Syntax(error)
            });
        }
    }
}

fn validate_modifier_groups(pattern: &str) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] != b'(' || bytes[index + 1] != b'?' {
            index += 1;
            continue;
        }
        let start = index + 2;
        match bytes.get(start).copied() {
            Some(b'=' | b'!') => index = start + 1,
            Some(b':' | b'>') => index = start,
            Some(b'<') => {
                index = pattern[start..]
                    .find('>')
                    .map_or(start + 1, |close| start + close + 1);
            }
            Some(_) => index = validate_modifier_group(pattern, start)?,
            None => return Ok(()),
        }
    }
    Ok(())
}

fn validate_modifier_group(pattern: &str, start: usize) -> Result<usize, String> {
    let bytes = pattern.as_bytes();
    let (enable, mut cursor) = read_modifier_flags(bytes, start)?;
    let disable = if bytes.get(cursor) == Some(&b'-') {
        cursor += 1;
        let (flags, after) = read_modifier_flags(bytes, cursor)?;
        cursor = after;
        flags
    } else {
        String::new()
    };
    if bytes.get(cursor) != Some(&b':')
        || enable.is_empty() && disable.is_empty()
        || !valid_modifier_flags(&enable, enable.is_empty())
        || !valid_modifier_flags(&disable, disable.is_empty())
        || enable.chars().any(|flag| disable.contains(flag))
    {
        return Err("SyntaxError: invalid regular expression modifiers".into());
    }
    Ok(cursor)
}

fn read_modifier_flags(bytes: &[u8], start: usize) -> Result<(String, usize), String> {
    let mut end = start;
    while end < bytes.len() && !matches!(bytes[end], b':' | b'-' | b')') {
        end += 1;
    }
    let flags = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| "SyntaxError: invalid regular expression modifiers".to_owned())?;
    Ok((flags.to_owned(), end))
}

fn valid_modifier_flags(flags: &str, allow_empty: bool) -> bool {
    if flags.is_empty() {
        return allow_empty;
    }
    let mut seen = FxHashSet::default();
    flags
        .chars()
        .all(|flag| matches!(flag, 'i' | 'm' | 's') && seen.insert(flag))
}

pub(super) fn strict_binding_early_error(
    program: &Program<'_>,
    inherited_strict: bool,
) -> Option<String> {
    let strict = inherited_strict
        || program
            .directives
            .iter()
            .any(|directive| directive.directive == "use strict");
    validate_strict_statements(&program.body, strict)
}

pub(super) fn strict_octal_numeric_early_error(program: &Program<'_>) -> Option<String> {
    let mut validator = StrictOctalNumericEarlyError(None);
    validator.visit_program(program);
    validator.0
}

struct StrictOctalNumericEarlyError(Option<String>);

impl<'a> Visit<'a> for StrictOctalNumericEarlyError {
    fn visit_numeric_literal(&mut self, literal: &oxc_ast::ast::NumericLiteral<'a>) {
        let Some(raw) = literal.raw.as_ref().map(|raw| raw.as_str()) else {
            return;
        };
        let bytes = raw.as_bytes();
        if bytes.len() > 1
            && bytes[0] == b'0'
            && bytes[1].is_ascii_digit()
            && bytes.iter().all(u8::is_ascii_digit)
        {
            self.0 = Some("SyntaxError: legacy octal literal is not allowed in strict mode".into());
        }
    }
}

pub(super) fn parameter_early_error(
    program: &Program<'_>,
    inherited_strict: bool,
) -> Option<String> {
    let mut validator = ParameterEarlyErrors {
        strict: inherited_strict,
        in_parameters: false,
        async_parameters: false,
        yield_parameters_forbidden: false,
        error: None,
    };
    validator.visit_program(program);
    validator.error
}

pub(super) fn parameters_contain_direct_eval(params: &FormalParameters<'_>) -> bool {
    let mut finder = DirectEvalParameterFinder(false);
    finder.visit_formal_parameters(params);
    finder.0
}

pub(super) fn is_direct_eval_call(call: &CallExpression<'_>) -> bool {
    !call.optional
        && matches!(call.callee.without_parentheses(), Expression::Identifier(id) if id.name == "eval")
}

struct DirectEvalParameterFinder(bool);

impl<'a> Visit<'a> for DirectEvalParameterFinder {
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if is_direct_eval_call(call) {
            self.0 = true;
        } else {
            walk::walk_call_expression(self, call);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

struct ParameterEarlyErrors {
    strict: bool,
    in_parameters: bool,
    async_parameters: bool,
    yield_parameters_forbidden: bool,
    error: Option<String>,
}

impl ParameterEarlyErrors {
    fn validate_function(&mut self, params: &FormalParameters<'_>, own_strict: bool) {
        let strict = self.strict || own_strict;
        let mut names = Vec::new();
        let simple = params.rest.is_none()
            && params.items.iter().all(|item| {
                item.initializer.is_none()
                    && matches!(item.pattern, BindingPattern::BindingIdentifier(_))
            });
        for item in &params.items {
            collect_pattern_names(&item.pattern, &mut names);
        }
        if let Some(rest) = &params.rest {
            collect_pattern_names(&rest.rest.argument, &mut names);
        }
        let duplicate = {
            let mut seen = FxHashSet::default();
            names.iter().any(|name| !seen.insert(name.clone()))
        };
        if duplicate
            && (strict
                || !simple
                || matches!(
                    params.kind,
                    FormalParameterKind::UniqueFormalParameters
                        | FormalParameterKind::ArrowFormalParameters
                ))
        {
            self.error = Some("SyntaxError: duplicate formal parameter".into());
            return;
        }
        if strict && names.iter().any(|name| strict_reserved(name)) {
            self.error = Some("SyntaxError: strict-reserved formal parameter".into());
            return;
        }
        if own_strict && !simple {
            self.error = Some(
                "SyntaxError: use strict directive is not allowed with non-simple parameters"
                    .into(),
            );
        }
    }

    fn validate_parameter_body(&mut self, params: &FormalParameters<'_>, body: &[Statement<'_>]) {
        let mut parameters = Vec::new();
        for item in &params.items {
            collect_pattern_names(&item.pattern, &mut parameters);
        }
        if let Some(rest) = &params.rest {
            collect_pattern_names(&rest.rest.argument, &mut parameters);
        }
        let parameters: FxHashSet<_> = parameters.into_iter().collect();
        let mut lexical = Vec::new();
        for statement in body {
            match statement {
                Statement::VariableDeclaration(declaration)
                    if declaration.kind != VariableDeclarationKind::Var =>
                {
                    for item in &declaration.declarations {
                        collect_pattern_names(&item.id, &mut lexical);
                    }
                }
                Statement::ClassDeclaration(class) => {
                    if let Some(name) = &class.id {
                        lexical.push(name.name.to_string());
                    }
                }
                _ => {}
            }
        }
        if lexical.iter().any(|name| parameters.contains(name)) {
            self.error = Some("SyntaxError: lexical declaration conflicts with parameter".into());
        }
    }
}

impl<'a> Visit<'a> for ParameterEarlyErrors {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let own_strict = function.body.as_ref().is_some_and(|body| {
            body.directives
                .iter()
                .any(|directive| directive.directive == "use strict")
        });
        self.validate_function(&function.params, own_strict);
        if let Some(body) = &function.body {
            self.validate_parameter_body(&function.params, &body.statements);
        }
        let previous = self.strict;
        let previous_parameters = self.in_parameters;
        let previous_async = self.async_parameters;
        let previous_yield = self.yield_parameters_forbidden;
        self.strict |= own_strict;
        self.in_parameters = false;
        self.async_parameters = function.r#async;
        self.yield_parameters_forbidden = function.generator;
        walk::walk_function(self, function, flags);
        self.strict = previous;
        self.in_parameters = previous_parameters;
        self.async_parameters = previous_async;
        self.yield_parameters_forbidden = previous_yield;
    }

    fn visit_arrow_function_expression(&mut self, function: &ArrowFunctionExpression<'a>) {
        let own_strict = match &function.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => body
                .directives
                .iter()
                .any(|directive| directive.directive == "use strict"),
            _ => false,
        };
        self.validate_function(&function.params, own_strict);
        if let oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) = &function.body {
            self.validate_parameter_body(&function.params, &body.statements);
        }
        let previous = self.strict;
        let previous_parameters = self.in_parameters;
        let previous_async = self.async_parameters;
        let previous_yield = self.yield_parameters_forbidden;
        self.strict |= own_strict;
        self.in_parameters = false;
        self.async_parameters = function.r#async;
        self.yield_parameters_forbidden = false;
        walk::walk_arrow_function_expression(self, function);
        self.strict = previous;
        self.in_parameters = previous_parameters;
        self.async_parameters = previous_async;
        self.yield_parameters_forbidden = previous_yield;
    }

    fn visit_formal_parameters(&mut self, params: &FormalParameters<'a>) {
        let previous = self.in_parameters;
        self.in_parameters = true;
        walk::walk_formal_parameters(self, params);
        self.in_parameters = previous;
    }

    fn visit_identifier_reference(&mut self, identifier: &oxc_ast::ast::IdentifierReference<'a>) {
        let name = identifier.name.as_str();
        if name == "yield" && (self.strict || self.in_parameters && self.yield_parameters_forbidden)
        {
            self.error = Some("SyntaxError: yield identifier is not allowed here".into());
        } else if name == "await" && self.in_parameters && self.async_parameters {
            self.error =
                Some("SyntaxError: await identifier is not allowed in async parameters".into());
        }
        walk::walk_identifier_reference(self, identifier);
    }
}

fn validate_strict_statements(
    statements: &[Statement<'_>],
    inherited_strict: bool,
) -> Option<String> {
    for statement in statements {
        match statement {
            Statement::WithStatement(_) if inherited_strict => {
                return Some("SyntaxError: with statement is not allowed in strict mode".into());
            }
            Statement::FunctionDeclaration(function)
                if inherited_strict
                    && function
                        .id
                        .as_ref()
                        .is_some_and(|identifier| strict_reserved(identifier.name.as_str())) =>
            {
                return Some("SyntaxError: strict-reserved function name".into());
            }
            Statement::VariableDeclaration(declaration) if inherited_strict => {
                let mut names = Vec::new();
                for item in &declaration.declarations {
                    collect_pattern_names(&item.id, &mut names);
                }
                if names.iter().any(|name| strict_reserved(name)) {
                    return Some("SyntaxError: strict-reserved binding identifier".into());
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(body) = &function.body {
                    let strict = inherited_strict
                        || body
                            .directives
                            .iter()
                            .any(|directive| directive.directive == "use strict");
                    if let Some(error) = validate_strict_statements(&body.statements, strict) {
                        return Some(error);
                    }
                }
            }
            Statement::TryStatement(statement) => {
                if let Some(error) =
                    validate_strict_statements(&statement.block.body, inherited_strict)
                {
                    return Some(error);
                }
                if let Some(handler) = &statement.handler {
                    if inherited_strict && let Some(parameter) = &handler.param {
                        let mut names = Vec::new();
                        collect_pattern_names(&parameter.pattern, &mut names);
                        if names.iter().any(|name| strict_reserved(name)) {
                            return Some(
                                "SyntaxError: strict-reserved catch binding identifier".into(),
                            );
                        }
                    }
                    if let Some(error) =
                        validate_strict_statements(&handler.body.body, inherited_strict)
                    {
                        return Some(error);
                    }
                }
                if let Some(finalizer) = &statement.finalizer
                    && let Some(error) =
                        validate_strict_statements(&finalizer.body, inherited_strict)
                {
                    return Some(error);
                }
            }
            Statement::ExpressionStatement(statement) => {
                if let Some(error) =
                    validate_strict_expression(&statement.expression, inherited_strict)
                {
                    return Some(error);
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_strict_expression(
    expression: &oxc_ast::ast::Expression<'_>,
    inherited_strict: bool,
) -> Option<String> {
    use oxc_ast::ast::Expression;
    match expression {
        Expression::FunctionExpression(function) => function.body.as_ref().and_then(|body| {
            let strict = inherited_strict
                || body
                    .directives
                    .iter()
                    .any(|directive| directive.directive == "use strict");
            validate_strict_statements(&body.statements, strict)
        }),
        Expression::ParenthesizedExpression(expression) => {
            validate_strict_expression(&expression.expression, inherited_strict)
        }
        Expression::CallExpression(call) => {
            if let Some(error) = validate_strict_expression(&call.callee, inherited_strict) {
                return Some(error);
            }
            for argument in &call.arguments {
                if let Some(expression) = argument.as_expression()
                    && let Some(error) = validate_strict_expression(expression, inherited_strict)
                {
                    return Some(error);
                }
            }
            None
        }
        _ => None,
    }
}

fn strict_reserved(name: &str) -> bool {
    matches!(
        name,
        "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
            | "eval"
            | "arguments"
    )
}

fn validate_nested(statements: &[Statement<'_>], strict: bool) -> Option<String> {
    for statement in statements {
        match statement {
            Statement::BlockStatement(block) => {
                if let Some(error) = validate_block(&block.body, strict) {
                    return Some(error);
                }
            }
            Statement::IfStatement(statement) => {
                if let Some(error) =
                    validate_nested(std::slice::from_ref(&statement.consequent), strict)
                {
                    return Some(error);
                }
                if let Some(alternate) = &statement.alternate
                    && let Some(error) = validate_nested(std::slice::from_ref(alternate), strict)
                {
                    return Some(error);
                }
            }
            Statement::WhileStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body, strict) {
                    return Some(error);
                }
            }
            Statement::DoWhileStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body, strict) {
                    return Some(error);
                }
            }
            Statement::ForStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body, strict) {
                    return Some(error);
                }
            }
            Statement::ForInStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body, strict) {
                    return Some(error);
                }
            }
            Statement::ForOfStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body, strict) {
                    return Some(error);
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(body) = &function.body {
                    let strict = strict
                        || body
                            .directives
                            .iter()
                            .any(|directive| directive.directive == "use strict");
                    if let Some(error) = validate_nested(&body.statements, strict) {
                        return Some(error);
                    }
                }
            }
            Statement::TryStatement(statement) => {
                if let Some(error) = validate_nested(&statement.block.body, strict) {
                    return Some(error);
                }
                if let Some(handler) = &statement.handler
                    && let Some(error) = validate_block(&handler.body.body, strict)
                {
                    return Some(error);
                }
                if let Some(finalizer) = &statement.finalizer
                    && let Some(error) = validate_block(&finalizer.body, strict)
                {
                    return Some(error);
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_loop_body(body: &Statement<'_>, strict: bool) -> Option<String> {
    if matches!(body, Statement::FunctionDeclaration(_)) {
        return Some("SyntaxError: function declaration is not a loop body".into());
    }
    validate_nested(std::slice::from_ref(body), strict)
}

fn validate_block(statements: &[Statement<'_>], strict: bool) -> Option<String> {
    let mut lexical = FxHashSet::default();
    let mut functions = FxHashSet::default();
    let mut variables = FxHashSet::default();
    for statement in statements {
        match statement {
            Statement::VariableDeclaration(declaration)
                if declaration.kind == VariableDeclarationKind::Var =>
            {
                for item in &declaration.declarations {
                    collect_pattern_names(&item.id, &mut variables);
                }
            }
            Statement::VariableDeclaration(declaration) => {
                for item in &declaration.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&item.id, &mut names);
                    for name in names {
                        if !lexical.insert(name.clone()) {
                            return Some("SyntaxError: duplicate lexical declaration".into());
                        }
                    }
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(identifier) = &class.id
                    && !lexical.insert(identifier.name.to_string())
                {
                    return Some("SyntaxError: duplicate lexical declaration".into());
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id {
                    let name = identifier.name.to_string();
                    if !lexical.insert(name.clone()) && (strict || !functions.contains(&name)) {
                        return Some("SyntaxError: duplicate lexical declaration".into());
                    }
                    functions.insert(name);
                }
            }
            _ => {}
        }
    }
    collect_nested_vars(statements, &mut variables);
    if lexical.iter().any(|name| variables.contains(name)) {
        return Some("SyntaxError: block lexical declaration conflicts with var".into());
    }
    validate_nested(statements, strict)
}

pub(super) fn collect_var_names(statements: &[Statement<'_>]) -> Vec<String> {
    let mut names = FxHashSet::default();
    collect_nested_vars(statements, &mut names);
    let mut names = names.into_iter().collect::<Vec<_>>();
    names.sort();
    names
}

pub(super) fn annex_b_lexical_collisions(statements: &[Statement<'_>]) -> FxHashSet<u32> {
    let visible = lexical_names(statements);
    let mut collisions = FxHashSet::default();
    collect_annex_b_collisions(statements, &visible, &mut collisions);
    collisions
}

pub(super) fn annex_b_function_names(statements: &[Statement<'_>]) -> Vec<(u32, String)> {
    statements
        .iter()
        .flat_map(annex_b_function_names_in)
        .collect()
}

fn annex_b_function_names_in(statement: &Statement<'_>) -> Vec<(u32, String)> {
    match statement {
        Statement::FunctionDeclaration(function) => annex_b_function_name(function)
            .map(|name| (function.span.start, name))
            .into_iter()
            .collect(),
        Statement::BlockStatement(block) => annex_b_function_names(&block.body),
        Statement::IfStatement(statement) => {
            let mut names = annex_b_function_names(std::slice::from_ref(&statement.consequent));
            if let Some(alternate) = &statement.alternate {
                names.extend(annex_b_function_names(std::slice::from_ref(alternate)));
            }
            names
        }
        Statement::SwitchStatement(statement) => statement
            .cases
            .iter()
            .flat_map(|case| annex_b_function_names(&case.consequent))
            .collect(),
        Statement::LabeledStatement(statement) => {
            annex_b_function_names(std::slice::from_ref(&statement.body))
        }
        Statement::WhileStatement(statement) => {
            annex_b_function_names(std::slice::from_ref(&statement.body))
        }
        Statement::DoWhileStatement(statement) => {
            annex_b_function_names(std::slice::from_ref(&statement.body))
        }
        Statement::TryStatement(statement) => {
            let mut names = annex_b_function_names(&statement.block.body);
            if let Some(handler) = &statement.handler {
                names.extend(annex_b_function_names(&handler.body.body));
            }
            if let Some(finalizer) = &statement.finalizer {
                names.extend(annex_b_function_names(&finalizer.body));
            }
            names
        }
        _ => Vec::new(),
    }
}

pub(super) fn annex_b_function_eligible(function: &oxc_ast::ast::Function<'_>) -> bool {
    !function.r#async && !function.generator && function.id.is_some()
}

fn annex_b_function_name(function: &oxc_ast::ast::Function<'_>) -> Option<String> {
    annex_b_function_eligible(function)
        .then(|| {
            function
                .id
                .as_ref()
                .expect("eligible Annex B function has a name")
        })
        .map(|identifier| identifier.name.to_string())
}

fn block_function_names(statements: &[Statement<'_>]) -> Vec<String> {
    statements
        .iter()
        .filter_map(|statement| match statement_without_labels(statement) {
            Statement::FunctionDeclaration(function) => annex_b_function_name(function),
            _ => None,
        })
        .collect()
}

fn lexical_names(statements: &[Statement<'_>]) -> Vec<String> {
    let mut names = FxHashSet::default();
    for statement in statements {
        match statement {
            Statement::VariableDeclaration(declaration)
                if declaration.kind != VariableDeclarationKind::Var =>
            {
                for item in &declaration.declarations {
                    collect_pattern_names(&item.id, &mut names);
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(identifier) = &class.id {
                    names.insert(identifier.name.to_string());
                }
            }
            _ => {}
        }
    }
    names.into_iter().collect()
}

fn collect_annex_b_collisions(
    statements: &[Statement<'_>],
    visible: &[String],
    collisions: &mut FxHashSet<u32>,
) {
    for statement in statements {
        match statement {
            Statement::BlockStatement(block) => {
                collect_block_collisions(&block.body, visible, collisions);
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id
                    && visible.iter().any(|name| name == identifier.name.as_str())
                {
                    collisions.insert(function.span.start);
                }
            }
            Statement::IfStatement(statement) => {
                collect_annex_b_one(&statement.consequent, visible, collisions);
                if let Some(alternate) = &statement.alternate {
                    collect_annex_b_one(alternate, visible, collisions);
                }
            }
            Statement::ForStatement(statement) => {
                let mut nested = visible.to_vec();
                if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(declaration)) =
                    &statement.init
                    && declaration.kind != VariableDeclarationKind::Var
                {
                    for item in &declaration.declarations {
                        collect_pattern_names(&item.id, &mut nested);
                    }
                }
                collect_annex_b_one(&statement.body, &nested, collisions);
            }
            Statement::ForInStatement(statement) => {
                collect_loop_collisions(&statement.left, &statement.body, visible, collisions);
            }
            Statement::ForOfStatement(statement) => {
                collect_loop_collisions(&statement.left, &statement.body, visible, collisions);
            }
            Statement::LabeledStatement(statement) => {
                collect_annex_b_one(&statement.body, visible, collisions);
            }
            Statement::WhileStatement(statement) => {
                collect_annex_b_one(&statement.body, visible, collisions);
            }
            Statement::DoWhileStatement(statement) => {
                collect_annex_b_one(&statement.body, visible, collisions);
            }
            Statement::SwitchStatement(statement) => {
                let mut own_lexicals = Vec::new();
                for case in &statement.cases {
                    own_lexicals.extend(lexical_names(&case.consequent));
                }
                let mut direct_functions = Vec::new();
                for case in &statement.cases {
                    direct_functions.extend(block_function_names(&case.consequent));
                }
                let mut function_visible = visible.to_vec();
                function_visible.extend(own_lexicals);
                let mut nested_visible = function_visible.clone();
                nested_visible.extend(direct_functions);
                for case in &statement.cases {
                    for nested_statement in &case.consequent {
                        let nested_statement = statement_without_labels(nested_statement);
                        if let Statement::FunctionDeclaration(function) = nested_statement {
                            if let Some(identifier) = &function.id
                                && function_visible
                                    .iter()
                                    .any(|name| name == identifier.name.as_str())
                            {
                                collisions.insert(function.span.start);
                            }
                        } else {
                            collect_annex_b_one(nested_statement, &nested_visible, collisions);
                        }
                    }
                }
            }
            Statement::TryStatement(statement) => {
                collect_block_collisions(&statement.block.body, visible, collisions);
                if let Some(handler) = &statement.handler {
                    let mut nested = visible.to_vec();
                    if let Some(parameter) = &handler.param
                        && !matches!(parameter.pattern, BindingPattern::BindingIdentifier(_))
                    {
                        collect_pattern_names(&parameter.pattern, &mut nested);
                    }
                    collect_block_collisions(&handler.body.body, &nested, collisions);
                }
                if let Some(finalizer) = &statement.finalizer {
                    collect_block_collisions(&finalizer.body, visible, collisions);
                }
            }
            _ => {}
        }
    }
}

fn collect_block_collisions(
    statements: &[Statement<'_>],
    visible: &[String],
    collisions: &mut FxHashSet<u32>,
) {
    let mut function_visible = visible.to_vec();
    function_visible.extend(lexical_names(statements));
    let mut nested_visible = function_visible.clone();
    nested_visible.extend(block_function_names(statements));
    for statement in statements {
        let statement = statement_without_labels(statement);
        if let Statement::FunctionDeclaration(function) = statement {
            if let Some(identifier) = &function.id
                && function_visible
                    .iter()
                    .any(|name| name == identifier.name.as_str())
            {
                collisions.insert(function.span.start);
            }
        } else {
            collect_annex_b_one(statement, &nested_visible, collisions);
        }
    }
}

fn collect_annex_b_one(
    statement: &Statement<'_>,
    visible: &[String],
    collisions: &mut FxHashSet<u32>,
) {
    collect_annex_b_collisions(std::slice::from_ref(statement), visible, collisions);
}

fn collect_loop_collisions(
    left: &oxc_ast::ast::ForStatementLeft<'_>,
    body: &Statement<'_>,
    visible: &[String],
    collisions: &mut FxHashSet<u32>,
) {
    let mut nested = visible.to_vec();
    if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left
        && declaration.kind != VariableDeclarationKind::Var
    {
        for item in &declaration.declarations {
            collect_pattern_names(&item.id, &mut nested);
        }
    }
    collect_annex_b_one(body, &nested, collisions);
}

fn collect_nested_vars(statements: &[Statement<'_>], names: &mut FxHashSet<String>) {
    for statement in statements {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                collect_var_declaration(declaration, names)
            }
            Statement::BlockStatement(block) => collect_nested_vars(&block.body, names),
            Statement::WithStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.body), names)
            }
            Statement::IfStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.consequent), names);
                if let Some(alternate) = &statement.alternate {
                    collect_nested_vars(std::slice::from_ref(alternate), names);
                }
            }
            Statement::ForStatement(statement) => {
                if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(declaration)) =
                    &statement.init
                {
                    collect_var_declaration(declaration, names);
                }
                collect_nested_vars(std::slice::from_ref(&statement.body), names);
            }
            Statement::ForInStatement(statement) => {
                if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(declaration) =
                    &statement.left
                {
                    collect_var_declaration(declaration, names);
                }
                collect_nested_vars(std::slice::from_ref(&statement.body), names);
            }
            Statement::ForOfStatement(statement) => {
                if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(declaration) =
                    &statement.left
                {
                    collect_var_declaration(declaration, names);
                }
                collect_nested_vars(std::slice::from_ref(&statement.body), names);
            }
            Statement::WhileStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.body), names)
            }
            Statement::DoWhileStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.body), names)
            }
            Statement::TryStatement(statement) => {
                collect_nested_vars(&statement.block.body, names);
                if let Some(handler) = &statement.handler {
                    collect_nested_vars(&handler.body.body, names);
                }
                if let Some(finalizer) = &statement.finalizer {
                    collect_nested_vars(&finalizer.body, names);
                }
            }
            Statement::SwitchStatement(statement) => {
                for case in &statement.cases {
                    collect_nested_vars(&case.consequent, names);
                }
            }
            Statement::LabeledStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.body), names)
            }
            _ => {}
        }
    }
}

fn collect_var_declaration(
    declaration: &oxc_ast::ast::VariableDeclaration<'_>,
    names: &mut FxHashSet<String>,
) {
    if declaration.kind != VariableDeclarationKind::Var {
        return;
    }
    for item in &declaration.declarations {
        let mut declared = Vec::new();
        collect_pattern_names(&item.id, &mut declared);
        names.extend(declared);
    }
}

pub(super) fn collect_pattern_names(pattern: &BindingPattern<'_>, names: &mut impl Extend<String>) {
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => {
            names.extend(std::iter::once(identifier.name.to_string()));
        }
        BindingPattern::AssignmentPattern(pattern) => collect_pattern_names(&pattern.left, names),
        BindingPattern::ArrayPattern(pattern) => {
            for element in pattern.elements.iter().flatten() {
                collect_pattern_names(element, names);
            }
        }
        BindingPattern::ObjectPattern(pattern) => {
            for property in &pattern.properties {
                collect_pattern_names(&property.value, names);
            }
        }
    }
}

pub(super) fn strict_restricted_assignment_early_error(
    program: &Program<'_>,
    strict: bool,
) -> Option<&'static str> {
    let mut validator = StrictRestrictedAssignmentEarlyError {
        strict,
        found: None,
    };
    validator.visit_program(program);
    validator.found
}

struct StrictRestrictedAssignmentEarlyError {
    strict: bool,
    found: Option<&'static str>,
}

impl<'a> Visit<'a> for StrictRestrictedAssignmentEarlyError {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let own_strict = function.body.as_ref().is_some_and(|body| {
            body.directives
                .iter()
                .any(|directive| directive.directive == "use strict")
        });
        let previous = self.strict;
        self.strict |= own_strict || flags.is_strict_mode();
        walk::walk_function(self, function, flags);
        self.strict = previous;
    }

    fn visit_arrow_function_expression(&mut self, function: &ArrowFunctionExpression<'a>) {
        let own_strict = match &function.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => body
                .directives
                .iter()
                .any(|directive| directive.directive == "use strict"),
            _ => false,
        };
        let previous = self.strict;
        self.strict |= own_strict;
        walk::walk_arrow_function_expression(self, function);
        self.strict = previous;
    }

    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'a>) {
        if self.strict
            && let Some(name) = expression
                .left
                .as_simple_assignment_target()
                .and_then(Self::restricted_target)
        {
            self.found.get_or_insert(name);
        }
        walk::walk_assignment_expression(self, expression);
    }

    fn visit_update_expression(&mut self, expression: &UpdateExpression<'a>) {
        if self.strict && let Some(name) = Self::restricted_target(&expression.argument) {
            self.found.get_or_insert(name);
        }
        walk::walk_update_expression(self, expression);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if self.strict
            && expression.operator == oxc_syntax::operator::UnaryOperator::Delete
            && let Expression::Identifier(identifier) = &expression.argument
            && let Some(name) = Self::restricted_name(identifier.name.as_str())
        {
            self.found.get_or_insert(name);
        }
        walk::walk_unary_expression(self, expression);
    }
}

impl StrictRestrictedAssignmentEarlyError {
    fn restricted_target(target: &SimpleAssignmentTarget<'_>) -> Option<&'static str> {
        let SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) = target else {
            return None;
        };
        Self::restricted_name(identifier.name.as_str())
    }

    fn restricted_name(name: &str) -> Option<&'static str> {
        match name {
            "eval" => Some("eval"),
            "arguments" => Some("arguments"),
            _ => None,
        }
    }
}
