use oxc_ast::ast::{ArrowFunctionExpression, FormalParameterKind, FormalParameters, Function};
use oxc_ast::ast::{BindingPattern, Program, Statement, VariableDeclarationKind};
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashSet;
use std::borrow::Cow;

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

pub(super) fn block_early_error(program: &Program<'_>) -> Option<String> {
    validate_nested(&program.body)
}

pub(super) fn regexp_early_error(program: &Program<'_>) -> Option<String> {
    let mut validator = RegExpEarlyErrors(None);
    validator.visit_program(program);
    validator.0
}

struct RegExpEarlyErrors(Option<String>);

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
            self.0 = Some(error);
            return;
        }
        if let Err(error) = super::regexp::validate_pattern(pattern, flags) {
            self.0 = Some(error);
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

struct DirectEvalParameterFinder(bool);

impl<'a> Visit<'a> for DirectEvalParameterFinder {
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if matches!(&call.callee, oxc_ast::ast::Expression::Identifier(id) if id.name == "eval") {
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
}

impl<'a> Visit<'a> for ParameterEarlyErrors {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let own_strict = function.body.as_ref().is_some_and(|body| {
            body.directives
                .iter()
                .any(|directive| directive.directive == "use strict")
        });
        self.validate_function(&function.params, own_strict);
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

fn validate_nested(statements: &[Statement<'_>]) -> Option<String> {
    for statement in statements {
        match statement {
            Statement::BlockStatement(block) => {
                if let Some(error) = validate_block(&block.body) {
                    return Some(error);
                }
            }
            Statement::IfStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.consequent) {
                    return Some(error);
                }
                if let Some(alternate) = &statement.alternate
                    && let Some(error) = validate_loop_body(alternate)
                {
                    return Some(error);
                }
            }
            Statement::WhileStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::DoWhileStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::ForStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::ForInStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::ForOfStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(body) = &function.body
                    && let Some(error) = validate_nested(&body.statements)
                {
                    return Some(error);
                }
            }
            Statement::TryStatement(statement) => {
                if let Some(error) = validate_nested(&statement.block.body) {
                    return Some(error);
                }
                if let Some(handler) = &statement.handler
                    && let Some(error) = validate_block(&handler.body.body)
                {
                    return Some(error);
                }
                if let Some(finalizer) = &statement.finalizer
                    && let Some(error) = validate_block(&finalizer.body)
                {
                    return Some(error);
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_loop_body(body: &Statement<'_>) -> Option<String> {
    if matches!(body, Statement::FunctionDeclaration(_)) {
        return Some("SyntaxError: function declaration is not a loop body".into());
    }
    validate_nested(std::slice::from_ref(body))
}

fn validate_block(statements: &[Statement<'_>]) -> Option<String> {
    let mut lexical = FxHashSet::default();
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
                if let Some(identifier) = &function.id
                    && !lexical.insert(identifier.name.to_string())
                {
                    return Some("SyntaxError: duplicate lexical declaration".into());
                }
            }
            _ => {}
        }
    }
    collect_nested_vars(statements, &mut variables);
    if lexical.iter().any(|name| variables.contains(name)) {
        return Some("SyntaxError: block lexical declaration conflicts with var".into());
    }
    validate_nested(statements)
}

pub(super) fn collect_var_names(statements: &[Statement<'_>]) -> Vec<String> {
    let mut names = FxHashSet::default();
    collect_nested_vars(statements, &mut names);
    let mut names = names.into_iter().collect::<Vec<_>>();
    names.sort();
    names
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

pub(super) fn strict_arguments_early_error(source: &str) -> bool {
    let masked = mask_literals_and_comments(source);
    let bytes = masked.as_bytes();
    let mut index = 0;
    while index + 9 <= bytes.len() {
        if bytes[index..].starts_with(b"arguments")
            && (index == 0 || !is_identifier_byte(bytes[index - 1]))
            && (index + 9 == bytes.len() || !is_identifier_byte(bytes[index + 9]))
        {
            let mut cursor = index + 9;
            while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                cursor += 1;
            }
            if matches!(
                bytes.get(cursor),
                Some(b'=') | Some(b'+') | Some(b'-') | Some(b'*') | Some(b'/')
            ) || source[index.saturating_sub(7)..index].contains("delete")
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

pub(super) fn strict_eval_early_error(source: &str) -> bool {
    let masked = mask_literals_and_comments(source);
    let bytes = masked.as_bytes();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if bytes[index..].starts_with(b"eval")
            && (index == 0 || !is_identifier_byte(bytes[index - 1]))
            && (index + 4 == bytes.len() || !is_identifier_byte(bytes[index + 4]))
        {
            let mut cursor = index + 4;
            while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                cursor += 1;
            }
            if matches!(bytes.get(cursor), Some(b'=' | b'+' | b'-' | b'*' | b'/')) {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn is_identifier_byte(byte: u8) -> bool {
    !byte.is_ascii() || byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

fn mask_literals_and_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[index] == b'\\' && index + 1 < bytes.len() {
                masked[index] = b' ';
                masked[index + 1] = b' ';
                index += 2;
                continue;
            }
            if bytes[index] == delimiter {
                quote = None;
            } else if !matches!(bytes[index], b'\n' | b'\r') {
                masked[index] = b' ';
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            quote = Some(bytes[index]);
            masked[index] = b' ';
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            masked[index] = b' ';
            masked[index + 1] = b' ';
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                masked[index] = b' ';
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            masked[index] = b' ';
            masked[index + 1] = b' ';
            index += 2;
            while index + 1 < bytes.len() {
                if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                    masked[index] = b' ';
                    masked[index + 1] = b' ';
                    index += 2;
                    break;
                }
                if !matches!(bytes[index], b'\n' | b'\r') {
                    masked[index] = b' ';
                }
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    String::from_utf8(masked).unwrap_or_default()
}
