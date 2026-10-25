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
pub(super) fn class_eval_references_arguments(statements: &[Statement<'_>]) -> bool {
    struct Arguments(bool);
    impl<'a> Visit<'a> for Arguments {
        fn visit_identifier_reference(
            &mut self,
            identifier: &oxc_ast::ast::IdentifierReference<'a>,
        ) {
            self.0 |= identifier.name == "arguments";
        }
        // Ordinary functions introduce their own arguments binding; arrows inherit it.
        fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    }
    let mut arguments = Arguments(false);
    for statement in statements {
        arguments.visit_statement(statement);
    }
    arguments.0
}

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
    if dynamic_body_is_strict(source) || !has_line_leading_html_close(source) {
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

/// Whether any `-->` follows only spaces and tabs on its line: the one place an HTML-like
/// closing comment can begin, so a body without such a candidate needs no lexical scan.
fn has_line_leading_html_close(source: &str) -> bool {
    source.match_indices("-->").any(|(start, _)| {
        source.as_bytes()[..start]
            .iter()
            .rev()
            .find(|byte| !matches!(byte, b' ' | b'\t'))
            .is_none_or(|byte| matches!(byte, b'\n' | b'\r'))
    })
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
        if let Err(error) = validate_modifier_groups(pattern, flags.contains('v')) {
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

/// Validate `(?flags-flags:...)` groups. A character class is atoms, never groups:
/// `[(?-i)]` is a class of `(`, `?`-`i` and `)`. Classes nest only under the `v` flag.
fn validate_modifier_groups(pattern: &str, nested_classes: bool) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    let mut class_depth = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if class_depth > 0 {
            match bytes[index] {
                b']' => class_depth -= 1,
                b'[' if nested_classes => class_depth += 1,
                _ => {}
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'[' {
            class_depth = 1;
            index += 1;
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

/// Whether a function with these parameters and body can observe a binding of `name` its
/// own scope would hold: a reference anywhere in them, or a direct eval, which can name it
/// at run time; nested functions included, which see the binding too.
pub(super) fn function_may_reference(
    params: &FormalParameters<'_>,
    body: &[Statement<'_>],
    name: &str,
) -> bool {
    struct References<'n> {
        name: &'n str,
        found: bool,
    }
    impl<'a> Visit<'a> for References<'_> {
        fn visit_identifier_reference(
            &mut self,
            identifier: &oxc_ast::ast::IdentifierReference<'a>,
        ) {
            self.found |= identifier.name == self.name;
        }
        fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
            self.found |= is_direct_eval_call(call);
            walk::walk_call_expression(self, call);
        }
    }
    let mut references = References { name, found: false };
    references.visit_formal_parameters(params);
    for statement in body {
        references.visit_statement(statement);
    }
    references.found
}

/// Whether a parameter list's patterns and defaults evaluate without reaching its parameter
/// scope: no nested function or class, which would capture its bindings, no direct eval,
/// which could name them, no `arguments`, and no reference to a binding of the list itself,
/// whose dead zone a reference could reach (`bound` names them). Each argument's hidden slot
/// then holds a value only the list's own bindings read, as a register can.
pub(super) fn parameters_reach_no_scope(params: &FormalParameters<'_>, bound: &[String]) -> bool {
    struct Reach<'n> {
        bound: &'n [String],
        reaches: bool,
    }
    impl<'a> Visit<'a> for Reach<'_> {
        fn visit_identifier_reference(&mut self, identifier: &oxc_ast::ast::IdentifierReference<'a>) {
            self.reaches |= identifier.name == "arguments" || self.bound.iter().any(|name| identifier.name == name.as_str());
        }
        fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
            self.reaches |= is_direct_eval_call(call);
            walk::walk_call_expression(self, call);
        }
        fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {
            self.reaches = true;
        }
        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {
            self.reaches = true;
        }
        fn visit_class(&mut self, _: &oxc_ast::ast::Class<'a>) {
            self.reaches = true;
        }
    }
    let mut reach = Reach { bound, reaches: false };
    reach.visit_formal_parameters(params);
    !reach.reaches
}

/// Whether evaluating `expression` makes a function or class, whose closure captures the
/// activation that runs it.
pub(super) fn expression_makes_function(expression: &Expression<'_>) -> bool {
    struct Makes(bool);
    impl<'a> Visit<'a> for Makes {
        fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {
            self.0 = true;
        }
        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {
            self.0 = true;
        }
        fn visit_class(&mut self, _: &oxc_ast::ast::Class<'a>) {
            self.0 = true;
        }
    }
    let mut makes = Makes(false);
    makes.visit_expression(expression);
    makes.0
}

pub(super) fn parameters_contain_direct_eval(params: &FormalParameters<'_>) -> bool {
    let mut finder = DirectEvalFinder(false);
    finder.visit_formal_parameters(params);
    finder.0
}

pub(super) fn body_contains_direct_eval(body: &[Statement<'_>]) -> bool {
    let mut finder = DirectEvalFinder(false);
    for statement in body {
        finder.visit_statement(statement);
    }
    finder.0
}

pub(super) fn expression_contains_direct_eval(expression: &Expression<'_>) -> bool {
    let mut finder = DirectEvalFinder(false);
    finder.visit_expression(expression);
    finder.0
}

/// How a body reaches its own `arguments` binding, ordered so the widest use found wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ArgumentsUse {
    None,
    /// Only `arguments.length` and `arguments[key]` reads: no call through, write to,
    /// `delete` of or escape of the object.
    Reads,
    /// Such reads, and the object itself as a value (`f(arguments)`): what receives it may
    /// write its elements, but no call through or write to it in the body.
    Passes,
    /// Any other reference.
    Escapes,
}

/// Whether `expression` names `arguments` itself.
fn names_arguments(expression: &Expression<'_>) -> bool {
    matches!(expression.without_parentheses(), Expression::Identifier(identifier) if identifier.name == ARGUMENTS)
}

const LENGTH_NAME: &str = "length";

pub(super) struct FixedRegisterLocalFacts {
    /// Promotion is sound but for the body's `arguments` use, which `arguments` states.
    pub(super) safe: bool,
    /// `safe` but for the functions and classes the body makes (`closures`).
    body_safe: bool,
    pub(super) arguments: ArgumentsUse,
    /// Where the body first passes `arguments` as a value (`ArgumentsUse::Passes`), when no
    /// loop repeats it and no destructuring evaluates a default or key after its source, so
    /// that source order is evaluation order around it.
    first_pass: Option<u32>,
    /// Each name's last reference in source order.
    last_references: rustc_hash::FxHashMap<String, u32>,
    /// Every name the body declares or assigns, at any depth: a parameter among them may be
    /// rebound.
    pub(super) bound_names: FxHashSet<String>,
    /// Names written somewhere other than at statement level (see `statement_level_write`).
    /// A register-resident local loads as its register itself, so it stays sound only while
    /// no write can land between a read and that read's consumer within one statement.
    pub(super) nested_written_names: FxHashSet<String>,
    /// What the functions and classes the body makes may do to its bindings
    /// (`ClosureReach`); `None` when it makes none.
    closures: Option<ClosureReach>,
}

/// What functions and classes nested in a body may reach of its bindings, read without
/// resolving their own scopes: every name any of them assigns, and whether one may observe
/// the body's `arguments` binding itself (an arrow naming `arguments`, which is the
/// body's own) or bind dynamically (a direct eval).
#[derive(Default)]
struct ClosureReach {
    assigned: FxHashSet<String>,
    observes_arguments: bool,
    /// How many ordinary (non-arrow) functions enclose the visit: inside one, `arguments`
    /// is that function's own.
    ordinary_depth: u32,
}

impl<'a> Visit<'a> for ClosureReach {
    fn visit_identifier_reference(&mut self, identifier: &oxc_ast::ast::IdentifierReference<'a>) {
        self.observes_arguments |= self.ordinary_depth == 0 && identifier.name == ARGUMENTS;
    }

    fn visit_simple_assignment_target(&mut self, target: &SimpleAssignmentTarget<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) = target {
            self.assigned.insert(identifier.name.to_string());
        }
        walk::walk_simple_assignment_target(self, target);
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        property: &oxc_ast::ast::AssignmentTargetPropertyIdentifier<'a>,
    ) {
        self.assigned.insert(property.binding.name.to_string());
        walk::walk_assignment_target_property_identifier(self, property);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.observes_arguments |= is_direct_eval_call(call);
        walk::walk_call_expression(self, call);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.ordinary_depth += 1;
        walk::walk_function(self, function, flags);
        self.ordinary_depth -= 1;
    }
}

pub(super) fn fixed_register_local_facts(
    body: &[Statement<'_>],
    expression: Option<&Expression<'_>>,
) -> FixedRegisterLocalFacts {
    struct Facts {
        safe: bool,
        arguments: ArgumentsUse,
        bound_names: FxHashSet<String>,
        nested_written_names: FxHashSet<String>,
        first_pass: Option<u32>,
        /// A pass inside a loop, or a destructuring pattern, leaves `first_pass` unordered.
        unordered: bool,
        loops: u32,
        last_references: rustc_hash::FxHashMap<String, u32>,
        closures: Option<ClosureReach>,
    }

    impl Facts {
        /// A function or class the body makes: its name, when it declares one, binds in the
        /// body, and what it reaches of the body's bindings joins `closures`.
        fn make_closure(&mut self, name: Option<&oxc_ast::ast::BindingIdentifier<'_>>, visit: impl FnOnce(&mut ClosureReach)) {
            if let Some(name) = name {
                self.bound_names.insert(name.name.to_string());
            }
            visit(self.closures.get_or_insert_with(ClosureReach::default));
        }

        /// Visits `x = e`, `x op= e` or `x++` standing as a whole statement or for-update:
        /// the write to `x` follows every read in it, so only the operands are visited. A
        /// chain `x = y = e` of plain assignments is the same: each link writes after `e`.
        /// So is each part of a statement-level comma sequence, conditional or logical
        /// expression whose value nothing reads: every earlier part's reads are consumed
        /// before the next part runs (minifiers fold statements into these forms).
        fn statement_level_write<'a>(&mut self, expression: &Expression<'a>) -> bool {
            let statement_level = |facts: &mut Self, part: &Expression<'a>| {
                if !facts.statement_level_write(part) {
                    facts.visit_expression(part);
                }
            };
            match expression {
                Expression::ParenthesizedExpression(parenthesized) => {
                    statement_level(self, &parenthesized.expression);
                    return true;
                }
                Expression::SequenceExpression(sequence) => {
                    for part in &sequence.expressions {
                        statement_level(self, part);
                    }
                    return true;
                }
                Expression::ConditionalExpression(conditional) => {
                    self.visit_expression(&conditional.test);
                    statement_level(self, &conditional.consequent);
                    statement_level(self, &conditional.alternate);
                    return true;
                }
                Expression::LogicalExpression(logical) => {
                    self.visit_expression(&logical.left);
                    statement_level(self, &logical.right);
                    return true;
                }
                _ => {}
            }
            let target = match expression {
                Expression::AssignmentExpression(assignment) => match &assignment.left {
                    oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) => {
                        let chained = assignment.operator
                            == oxc_syntax::operator::AssignmentOperator::Assign
                            && matches!(
                                &assignment.right,
                                Expression::AssignmentExpression(link)
                                    if link.operator
                                        == oxc_syntax::operator::AssignmentOperator::Assign
                            )
                            && self.statement_level_write(&assignment.right);
                        if !chained {
                            self.visit_expression(&assignment.right);
                        }
                        target
                    }
                    _ => return false,
                },
                Expression::UpdateExpression(update) => match &update.argument {
                    SimpleAssignmentTarget::AssignmentTargetIdentifier(target) => target,
                    _ => return false,
                },
                _ => return false,
            };
            self.safe &= target.name != ARGUMENTS;
            self.bound_names.insert(target.name.to_string());
            true
        }

        fn use_arguments(&mut self, kind: ArgumentsUse) {
            self.arguments = self.arguments.max(kind);
        }
    }

    impl<'a> Visit<'a> for Facts {
        fn visit_identifier_reference(
            &mut self,
            identifier: &oxc_ast::ast::IdentifierReference<'a>,
        ) {
            if identifier.name == ARGUMENTS {
                self.use_arguments(ArgumentsUse::Passes);
                self.unordered |= self.loops != 0;
                self.first_pass.get_or_insert(identifier.span.start);
            } else {
                self.last_references.insert(identifier.name.to_string(), identifier.span.start);
            }
        }

        fn visit_binding_identifier(&mut self, identifier: &oxc_ast::ast::BindingIdentifier<'a>) {
            self.bound_names.insert(identifier.name.to_string());
        }

        fn visit_static_member_expression(&mut self, member: &oxc_ast::ast::StaticMemberExpression<'a>) {
            if names_arguments(&member.object) && member.property.name == LENGTH_NAME {
                self.use_arguments(ArgumentsUse::Reads);
            } else {
                walk::walk_static_member_expression(self, member);
            }
        }

        fn visit_computed_member_expression(&mut self, member: &oxc_ast::ast::ComputedMemberExpression<'a>) {
            if names_arguments(&member.object) {
                self.use_arguments(ArgumentsUse::Reads);
                self.visit_expression(&member.expression);
            } else {
                walk::walk_computed_member_expression(self, member);
            }
        }

        // A member of `arguments` as a callee or tag passes the object as the receiver.
        fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
            if call.callee.without_parentheses().as_member_expression().is_some_and(|member| names_arguments(member.object())) {
                self.use_arguments(ArgumentsUse::Escapes);
            }
            walk::walk_call_expression(self, call);
        }

        fn visit_tagged_template_expression(&mut self, tagged: &oxc_ast::ast::TaggedTemplateExpression<'a>) {
            if tagged.tag.without_parentheses().as_member_expression().is_some_and(|member| names_arguments(member.object())) {
                self.use_arguments(ArgumentsUse::Escapes);
            }
            walk::walk_tagged_template_expression(self, tagged);
        }

        fn visit_simple_assignment_target(&mut self, target: &SimpleAssignmentTarget<'a>) {
            if let SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) = target {
                self.nested_written_names
                    .insert(identifier.name.to_string());
                self.bound_names.insert(identifier.name.to_string());
            }
            if target.as_member_expression().is_some_and(|member| names_arguments(member.object())) {
                self.use_arguments(ArgumentsUse::Escapes);
            }
            walk::walk_simple_assignment_target(self, target);
        }

        fn visit_assignment_target_property_identifier(
            &mut self,
            property: &oxc_ast::ast::AssignmentTargetPropertyIdentifier<'a>,
        ) {
            self.nested_written_names
                .insert(property.binding.name.to_string());
            self.bound_names.insert(property.binding.name.to_string());
            walk::walk_assignment_target_property_identifier(self, property);
        }

        fn visit_expression_statement(
            &mut self,
            statement: &oxc_ast::ast::ExpressionStatement<'a>,
        ) {
            if !self.statement_level_write(&statement.expression) {
                walk::walk_expression_statement(self, statement);
            }
        }

        fn visit_for_statement(&mut self, statement: &oxc_ast::ast::ForStatement<'a>) {
            if let Some(init) = &statement.init {
                self.visit_for_statement_init(init);
            }
            self.loops += 1;
            if let Some(test) = &statement.test {
                self.visit_expression(test);
            }
            if let Some(update) = &statement.update
                && !self.statement_level_write(update)
            {
                self.visit_expression(update);
            }
            self.visit_statement(&statement.body);
            self.loops -= 1;
        }

        fn visit_with_statement(&mut self, _: &oxc_ast::ast::WithStatement<'a>) {
            self.safe = false;
        }

        fn visit_while_statement(&mut self, statement: &oxc_ast::ast::WhileStatement<'a>) {
            self.loops += 1;
            walk::walk_while_statement(self, statement);
            self.loops -= 1;
        }

        fn visit_do_while_statement(&mut self, statement: &oxc_ast::ast::DoWhileStatement<'a>) {
            self.loops += 1;
            walk::walk_do_while_statement(self, statement);
            self.loops -= 1;
        }

        fn visit_for_in_statement(&mut self, statement: &oxc_ast::ast::ForInStatement<'a>) {
            self.loops += 1;
            walk::walk_for_in_statement(self, statement);
            self.loops -= 1;
        }

        fn visit_for_of_statement(&mut self, statement: &oxc_ast::ast::ForOfStatement<'a>) {
            self.loops += 1;
            walk::walk_for_of_statement(self, statement);
            self.loops -= 1;
        }

        // A destructuring target or binding evaluates its defaults and computed keys after
        // the value it takes apart.
        fn visit_array_assignment_target(&mut self, target: &oxc_ast::ast::ArrayAssignmentTarget<'a>) {
            self.unordered = true;
            walk::walk_array_assignment_target(self, target);
        }

        fn visit_object_assignment_target(&mut self, target: &oxc_ast::ast::ObjectAssignmentTarget<'a>) {
            self.unordered = true;
            walk::walk_object_assignment_target(self, target);
        }

        fn visit_array_pattern(&mut self, pattern: &oxc_ast::ast::ArrayPattern<'a>) {
            self.unordered = true;
            walk::walk_array_pattern(self, pattern);
        }

        fn visit_object_pattern(&mut self, pattern: &oxc_ast::ast::ObjectPattern<'a>) {
            self.unordered = true;
            walk::walk_object_pattern(self, pattern);
        }

        fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
            // Only `delete name` reaches a binding, and `delete arguments[k]` unmaps a
            // parameter from a mapped `arguments`; deleting any other member (`delete o[k]`)
            // leaves every local where it was.
            if expression.operator == oxc_syntax::operator::UnaryOperator::Delete {
                let reaches_binding = match expression.argument.without_parentheses() {
                    Expression::Identifier(_) => true,
                    expression => expression.as_member_expression().is_none_or(|member| {
                        matches!(member.object().without_parentheses(), Expression::Identifier(object) if object.name == "arguments")
                    }),
                };
                if reaches_binding {
                    self.safe = false;
                }
            }
            walk::walk_unary_expression(self, expression);
        }

        fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
            self.make_closure(function.id.as_ref(), |reach| reach.visit_function(function, flags));
        }

        fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
            self.make_closure(None, |reach| reach.visit_arrow_function_expression(arrow));
        }

        fn visit_class(&mut self, class: &oxc_ast::ast::Class<'a>) {
            self.make_closure(class.id.as_ref(), |reach| reach.visit_class(class));
        }
    }

    let mut facts = Facts {
        safe: true,
        arguments: ArgumentsUse::None,
        bound_names: FxHashSet::default(),
        nested_written_names: FxHashSet::default(),
        first_pass: None,
        unordered: false,
        loops: 0,
        last_references: rustc_hash::FxHashMap::default(),
        closures: None,
    };
    for statement in body {
        facts.visit_statement(statement);
    }
    if let Some(expression) = expression {
        facts.visit_expression(expression);
    }
    FixedRegisterLocalFacts {
        // Promotion keeps every local of a body that makes no function or class.
        safe: facts.safe && facts.closures.is_none(),
        body_safe: facts.safe,
        arguments: facts.arguments,
        first_pass: facts.first_pass.filter(|_| !facts.unordered),
        last_references: facts.last_references,
        bound_names: facts.bound_names,
        nested_written_names: facts.nested_written_names,
        closures: facts.closures,
    }
}

impl FixedRegisterLocalFacts {
    /// Whether a sloppy body that rebinds no parameter cannot observe its `arguments`
    /// mapping: it only reads the object while no function it makes assigns a parameter,
    /// or, making none, it passes the object on after its last reference to any parameter,
    /// so what writes the object's elements finds no parameter read after it.
    pub(super) fn arguments_mapping_unobserved(&self, mut parameters: impl Iterator<Item = impl AsRef<str>>) -> bool {
        if !self.body_safe {
            return false;
        }
        match self.arguments {
            // Only a write to a parameter could make a read differ from the value passed:
            // the functions the body makes may not assign one, nor observe the binding.
            ArgumentsUse::Reads => self.closures.as_ref().is_none_or(|reach| {
                !reach.observes_arguments && parameters.all(|parameter| !reach.assigned.contains(parameter.as_ref()))
            }),
            // A function the body makes may read a parameter at any later time.
            ArgumentsUse::Passes => self.closures.is_none() && self.first_pass.is_some_and(|pass| {
                parameters.into_iter().all(|parameter| {
                    self.last_references.get(parameter.as_ref()).is_none_or(|&reference| reference < pass)
                })
            }),
            ArgumentsUse::None | ArgumentsUse::Escapes => false,
        }
    }
}

pub(super) fn is_direct_eval_call(call: &CallExpression<'_>) -> bool {
    !call.optional
        && matches!(call.callee.without_parentheses(), Expression::Identifier(id) if id.name == "eval")
}

struct DirectEvalFinder(bool);

/// Whether a body (not its nested functions) may give the functions nested in it binding
/// sites to resolve through: a direct eval, a `with` statement, or `delete` of a name.
pub(super) fn body_may_bind_dynamically(
    body: &[Statement<'_>],
    expression: Option<&Expression<'_>>,
) -> bool {
    let mut finder = DynamicBindingFinder(false);
    for statement in body {
        finder.visit_statement(statement);
    }
    if let Some(expression) = expression {
        finder.visit_expression(expression);
    }
    finder.0
}

struct DynamicBindingFinder(bool);

impl<'a> Visit<'a> for DynamicBindingFinder {
    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if is_direct_eval_call(call) {
            self.0 = true;
        } else {
            walk::walk_call_expression(self, call);
        }
    }

    fn visit_with_statement(&mut self, _: &oxc_ast::ast::WithStatement<'a>) {
        self.0 = true;
    }

    fn visit_unary_expression(&mut self, expression: &oxc_ast::ast::UnaryExpression<'a>) {
        if expression.operator == oxc_syntax::operator::UnaryOperator::Delete
            && matches!(expression.argument.without_parentheses(), Expression::Identifier(_))
        {
            self.0 = true;
        } else {
            walk::walk_unary_expression(self, expression);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

impl<'a> Visit<'a> for DirectEvalFinder {
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
    #[derive(Default)]
    struct Names(Vec<(u32, String)>);
    impl<'a> Visit<'a> for Names {
        fn visit_function(&mut self, function: &Function<'a>, _: ScopeFlags) {
            if function.r#type == oxc_ast::ast::FunctionType::FunctionDeclaration
                && let Some(name) = annex_b_function_name(function)
            {
                self.0.push((function.span.start, name));
            }
            // Nested functions own their declarations, not this variable scope.
        }
        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
        fn visit_class(&mut self, _: &oxc_ast::ast::Class<'a>) {}
    }
    let mut names = Names::default();
    for statement in statements {
        names.visit_statement(statement);
    }
    names.0
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
            Statement::WithStatement(statement) => {
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
        if self.strict
            && let Some(name) = Self::restricted_target(&expression.argument)
        {
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

#[cfg(test)]
mod regexp_modifier_tests {
    use super::validate_modifier_groups;

    #[test]
    fn classes_hold_atoms_not_modifier_groups() {
        for pattern in [r"^[(?-i)y]$", r"[a(?-i)]", r"[\](?-i)]", r"(?i:a)[(?-m)]"] {
            assert!(validate_modifier_groups(pattern, false).is_ok(), "{pattern}");
        }
        assert!(validate_modifier_groups(r"[[a]](?-i)", true).is_err());
        assert!(validate_modifier_groups(r"[a](?-i)", false).is_err());
        assert!(validate_modifier_groups(r"(?ii:a)", false).is_err());
    }
}

/// The name `x` of a function body's `var x = arguments;` alias whose every use reads
/// `x.length` or `x[key]`, so each read may read the `arguments` binding itself: the
/// declaration is a top-level statement before every use, it is `x`'s only binding, and
/// nothing writes `x` or `arguments`. The body makes no closure, enters no `with` and calls
/// no direct eval, which could reach either binding another way.
pub(super) fn arguments_alias<'a>(
    params: Option<&FormalParameters<'a>>,
    body: &[Statement<'a>],
) -> Option<String> {
    let (declared_at, alias) = body.iter().find_map(|statement| {
        let Statement::VariableDeclaration(declaration) = statement else {
            return None;
        };
        let [declarator] = declaration.declarations.as_slice() else {
            return None;
        };
        match (&declarator.id, &declarator.init) {
            (BindingPattern::BindingIdentifier(name), Some(Expression::Identifier(init)))
                if declaration.kind == VariableDeclarationKind::Var
                    && init.name == ARGUMENTS
                    && name.name != ARGUMENTS =>
            {
                Some((declarator.span.end, name.name.as_str()))
            }
            _ => None,
        }
    })?;
    let mut uses = AliasUses { alias, declared_at, bindings: 0, references: 0, reads: 0, safe: true };
    if let Some(params) = params {
        uses.visit_formal_parameters(params);
    }
    for statement in body {
        uses.visit_statement(statement);
    }
    (uses.safe && uses.bindings == 1 && uses.references == uses.reads).then(|| alias.to_owned())
}

const ARGUMENTS: &str = "arguments";

struct AliasUses<'s> {
    alias: &'s str,
    declared_at: u32,
    bindings: usize,
    references: usize,
    reads: usize,
    safe: bool,
}

impl AliasUses<'_> {
    /// Whether `expression` is `alias.length` or `alias[key]`, outside an optional chain.
    fn alias_read(&self, expression: &Expression<'_>) -> bool {
        let object = match expression {
            Expression::StaticMemberExpression(member) if !member.optional => &member.object,
            Expression::ComputedMemberExpression(member) if !member.optional => &member.object,
            _ => return false,
        };
        matches!(object, Expression::Identifier(name) if name.name == self.alias)
    }
}

impl<'a> Visit<'a> for AliasUses<'_> {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        if self.alias_read(expression) {
            self.reads += 1;
        }
        walk::walk_expression(self, expression);
    }

    fn visit_identifier_reference(&mut self, identifier: &oxc_ast::ast::IdentifierReference<'a>) {
        if identifier.name == self.alias {
            self.references += 1;
            self.safe &= identifier.span.start > self.declared_at;
        }
    }

    fn visit_binding_identifier(&mut self, identifier: &oxc_ast::ast::BindingIdentifier<'a>) {
        self.bindings += usize::from(identifier.name == self.alias);
        self.safe &= identifier.name != ARGUMENTS;
    }

    fn visit_simple_assignment_target(&mut self, target: &SimpleAssignmentTarget<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(name) = target {
            self.safe &= name.name != self.alias && name.name != ARGUMENTS;
        }
        walk::walk_simple_assignment_target(self, target);
    }

    // A read as a callee binds `this` to the object, and `delete` writes it.
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.safe &= !self.alias_read(&call.callee) && !is_direct_eval_call(call);
        walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, new: &oxc_ast::ast::NewExpression<'a>) {
        self.safe &= !self.alias_read(&new.callee);
        walk::walk_new_expression(self, new);
    }

    fn visit_tagged_template_expression(
        &mut self,
        tagged: &oxc_ast::ast::TaggedTemplateExpression<'a>,
    ) {
        self.safe &= !self.alias_read(&tagged.tag);
        walk::walk_tagged_template_expression(self, tagged);
    }

    fn visit_unary_expression(&mut self, unary: &UnaryExpression<'a>) {
        self.safe &= unary.operator != oxc_syntax::operator::UnaryOperator::Delete
            || !self.alias_read(&unary.argument);
        walk::walk_unary_expression(self, unary);
    }

    fn visit_with_statement(&mut self, _: &oxc_ast::ast::WithStatement<'a>) {
        self.safe = false;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {
        self.safe = false;
    }

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {
        self.safe = false;
    }

    fn visit_class(&mut self, _: &oxc_ast::ast::Class<'a>) {
        self.safe = false;
    }
}
