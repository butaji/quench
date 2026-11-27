//! The call sites of a source text, read by the front end: for each call, the callee text
//! V8's `CallPrinter` renders for it, and the source expression it spans. The runtime freezes
//! them once per source (`vm::call_printer`), or reads one function's text on a diagnostic
//! path. What leaves this module is plain data (`CallSiteDescription`); nothing outside the
//! front end parses.

use oxc_ast::ast::{
    ArrayExpressionElement, CallExpression, ChainElement, Comment, Expression, MemberExpression,
    NewExpression, SimpleAssignmentTarget,
};
use oxc_ast_visit::Visit;
use oxc_parser::{ParseOptions, Parser, Token, Kind, config::TokensParserConfig};
use oxc_span::SourceType;
use std::collections::BTreeMap;

/// What `CallPrinter` prints for an operand it does not render.
const INTERMEDIATE_VALUE: &str = "(intermediate value)";

/// A nested call's arguments.
const CALL_ARGUMENTS: &str = "(...)";

const CALL_SOURCE_GOALS: [SourceType; 2] = [SourceType::script(), SourceType::mjs()];

/// The linker names of this syntax front end's crates. The link audit refuses an execution image
/// that retains any of them.
pub(crate) const SYNTAX_FRONTEND_SYMBOLS: &[&str] = &["oxc_parser::", "oxc_semantic::"];

/// The call sites of `source`, read from its text: the first goal (script, then module) that
/// parses cleanly gives them all; a text that parses under neither keeps the sites the earlier
/// goal found.
pub(crate) fn source_call_sites(source: &crate::bytecode::ProgramSource) -> Vec<crate::bytecode::CallSiteDescription> {
    let mut found = CallSites::new();
    for goal in CALL_SOURCE_GOALS.map(|goal| goal.with_typescript(source.typescript)) {
        match parse_call_sites(source.text.as_ref(), goal, found) {
            Ok((next, valid)) => { found = next; if valid { break; } }
            Err(previous) => { found = previous; break; }
        }
    }
    found.into_values().collect()
}

/// The calls V8 positions at an offset, by offset and kind: a call of a name at the name, a
/// non-optional call of a named property at the property name, a construction at `new`, any
/// other call at its argument list's `(`.
type CallSites = BTreeMap<(u32, bool), crate::bytecode::CallSiteDescription>;

fn parse_call_sites(source: &str, goal: SourceType, found: CallSites) -> Result<(CallSites, bool), CallSites> {
    // Without the front end no source is parsed: the call sites stay as found.
    if !cfg!(feature = "source") {
        return Err(found);
    }
    // A data script's literal holds no call, and its tree takes several times its text: it
    // is parsed blank, as its compilation parses it (`data_literal`).
    let located = (!goal.is_typescript()).then(|| crate::compile::data_literal::locate(source)).flatten();
    match located {
        Some(literal) => parse_call_sites_in(source, &literal.substituted, Some(literal.start), goal, found),
        None => parse_call_sites_in(source, source, None, goal, found),
    }
}

/// `parse_call_sites` of `source` parsed as `text`, which blanks the data literal at `blank`;
/// a literal that is not a top-level value is parsed as it is.
fn parse_call_sites_in(source: &str, text: &str, blank: Option<u32>, goal: SourceType, found: CallSites) -> Result<(CallSites, bool), CallSites> {
    // The parse arena compiles share: its pages go back when it is handed back, and a
    // diagnostic's parse leaves none of the malloc heap behind.
    let allocator = crate::compile::lazy_bodies::sized_arena(text);
    let options = ParseOptions { allow_return_outside_function: true, ..ParseOptions::default() };
    let parsed = Parser::new(&allocator, text, goal).with_options(options).with_config(TokensParserConfig).parse();
    if blank.is_some_and(|start| !crate::compile::data_literal::is_top_level_value(&parsed.program, start)) {
        drop(parsed);
        return parse_call_sites_in(source, source, None, goal, found);
    }
    if crate::compile::stack::validate_parsed(&parsed).is_err() { return Err(found); }
    let mut finder = CallFinder { source, comments: &parsed.program.comments, found };
    finder.visit_program(&parsed.program);
    let mut found = finder.found;
    freeze_expressions(source, &parsed.tokens, &mut found);
    Ok((found, parsed.diagnostics.is_empty()))
}

/// Acorn's `name` label includes contextual keywords, `await`, `yield` and `enum`.
fn expression_name(kind: Kind) -> bool {
    kind.is_identifier() || matches!(kind, Kind::Await | Kind::Yield | Kind::Enum)
}

fn member_access(kind: Kind) -> bool {
    matches!(kind, Kind::Dot | Kind::QuestionDot | Kind::LBrack | Kind::RBrack)
}

/// Node 26's `getFirstExpression`: remember a simple member prefix before the
/// call position, then stop at the first balanced argument list on that line.
/// OXC owns tokenization; line prefixes and closing pairs are processed once,
/// even when thousands of calls share a minified line.
fn freeze_expressions(source: &str, tokens: &[Token], sites: &mut CallSites) {
    let starts: Vec<_> = crate::bytecode::source_line_starts(source).collect();
    let mut sites = sites.values_mut().peekable();
    let mut closes = Vec::new();
    let mut pending = Vec::new();
    let mut endings = Vec::new();
    let mut semicolons = Vec::new();
    while let Some(site) = sites.peek() {
        let line = starts.partition_point(|start| *start <= site.offset as usize).checked_sub(1).expect("source begins at zero");
        let start = starts[line];
        let text = source[start..starts.get(line + 1).copied().unwrap_or(source.len())]
            .trim_end_matches(['\r', '\n', '\u{2028}', '\u{2029}']);
        let end = start + text.len();
        let first = tokens.partition_point(|token| (token.start() as usize) < start);
        let last = tokens.partition_point(|token| (token.start() as usize) < end);
        let tokens = &tokens[first..last];
        closes.clear();
        closes.resize(tokens.len(), None);
        pending.clear();
        for (index, token) in tokens.iter().enumerate() {
            match token.kind() {
                Kind::LParen => pending.push(index),
                Kind::RParen => if let Some(open) = pending.pop() { closes[open] = Some(token.end()); },
                _ => {},
            }
        }
        endings.clear();
        endings.resize(tokens.len() + 1, None);
        semicolons.clear();
        semicolons.resize(tokens.len() + 1, None);
        for index in (0..tokens.len()).rev() {
            endings[index] = if tokens[index].kind() == Kind::LParen { closes[index] } else { endings[index + 1] };
            semicolons[index] = if tokens[index].kind() == Kind::Semicolon { Some(tokens[index].start()) } else { semicolons[index + 1] };
        }
        let mut cursor = 0;
        let mut previous_name = None;
        let mut member_start = None;
        while sites.peek().is_some_and(|site| (site.offset as usize) < starts.get(line + 1).copied().unwrap_or(source.len() + 1)) {
            let site = sites.next().expect("line has a site");
            while cursor < tokens.len() && tokens[cursor].start() < site.offset {
                let token = tokens[cursor];
                let kind = token.kind();
                cursor += 1;
                if kind == Kind::Semicolon { member_start = None; continue; }
                if member_access(kind) && previous_name.is_some() {
                    member_start = member_start.or(previous_name);
                } else if !member_access(kind) && !expression_name(kind) && kind != Kind::Str && !kind.is_number() {
                    member_start = None;
                }
                previous_name = expression_name(kind).then_some(token.start());
            }
            let finish = endings[cursor].unwrap_or(end as u32).min(end as u32);
            // Node's semicolon terminator converts its token object to zero;
            // that yields no source expression, rather than the following statement.
            site.expression = if semicolons[cursor].is_some_and(|semicolon| semicolon < finish) { None }
                else {
                    let begin = member_start.unwrap_or(site.offset);
                    (begin < finish).then_some(crate::bytecode::SourceSpan { start: begin, end: finish })
                };
        }
    }
}

struct CallFinder<'s> {
    source: &'s str,
    comments: &'s [Comment],
    found: CallSites,
}

impl CallFinder<'_> {
    fn call_position(&self, call: &CallExpression<'_>) -> u32 {
        crate::compile::source_syntax::call_offset(call, self.source, self.comments)
    }

    fn observe(&mut self, offset: u32, construct: bool, callee: &Expression<'_>) {
        self.found.entry((offset, construct)).or_insert_with(|| crate::bytecode::CallSiteDescription {
            offset, construct, callee: find(callee, true), expression: None,
        });
    }
}

impl<'a> Visit<'a> for CallFinder<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let offset = self.call_position(call);
        self.observe(offset, false, &call.callee);
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, new: &NewExpression<'a>) {
        self.observe(new.span.start, true, &new.callee);
        oxc_ast_visit::walk::walk_new_expression(self, new);
    }
}

/// `CallPrinter::Find` once the call is found: `expression` printed, or the intermediate
/// value when it prints nothing or is not printed.
fn find(expression: &Expression<'_>, print: bool) -> String {
    let printed = if print { render(expression) } else { String::new() };
    if printed.is_empty() { INTERMEDIATE_VALUE.to_owned() } else { printed }
}

/// `CallPrinter`'s rendering of `expression`; empty when it prints nothing.
fn render(expression: &Expression<'_>) -> String {
    let expression = expression.get_inner_expression();
    if let Some(member) = expression.as_member_expression() {
        return render_member(member);
    }
    match expression {
        Expression::Identifier(identifier) => identifier.name.to_string(),
        Expression::ThisExpression(_) => "this".into(),
        Expression::Super(_) => "super".into(),
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => format!("{}{CALL_ARGUMENTS}", find(&call.callee, true)),
            ChainElement::TSNonNullExpression(inner) => render(&inner.expression),
            element => element.as_member_expression().map_or_else(String::new, render_member),
        },
        Expression::CallExpression(call) => format!("{}{CALL_ARGUMENTS}", find(&call.callee, true)),
        // A nested construction's callee is not printed.
        Expression::NewExpression(_) => INTERMEDIATE_VALUE.into(),
        Expression::SequenceExpression(sequence) => {
            let parts: Vec<String> = sequence.expressions.iter().map(|part| find(part, true)).collect();
            format!("({})", parts.join(" , "))
        }
        Expression::BinaryExpression(binary) => {
            format!("({} {} {})", find(&binary.left, true), binary.operator.as_str(), find(&binary.right, true))
        }
        Expression::LogicalExpression(logical) => {
            format!("({} {} {})", find(&logical.left, true), logical.operator.as_str(), find(&logical.right, true))
        }
        Expression::UnaryExpression(unary) => {
            let operator = unary.operator.as_str();
            let space = if operator.chars().all(char::is_alphabetic) { " " } else { "" };
            format!("({operator}{space}{})", find(&unary.argument, true))
        }
        Expression::UpdateExpression(update) => {
            let target = render_simple_target(&update.argument);
            let operator = update.operator.as_str();
            if update.prefix { format!("({operator}{target})") } else { format!("({target}{operator})") }
        }
        Expression::StringLiteral(literal) => format!("\"{}\"", literal.value),
        Expression::NumericLiteral(literal) => crate::number_to_string::format(literal.value),
        Expression::BooleanLiteral(literal) => literal.value.to_string(),
        Expression::NullLiteral(_) => "null".into(),
        Expression::ArrayExpression(array) => {
            let parts: Vec<String> = array
                .elements
                .iter()
                .map(|element| match element {
                    ArrayExpressionElement::Elision(_) => String::new(),
                    ArrayExpressionElement::SpreadElement(spread) => format!("(...{})", find(&spread.argument, true)),
                    element => element.as_expression().map_or_else(String::new, |value| find(value, true)),
                })
                .collect();
            format!("[{}]", parts.join(","))
        }
        // Each operand is visited without printing (`CallPrinter::VisitConditional`).
        Expression::ConditionalExpression(_) => INTERMEDIATE_VALUE.repeat(3),
        // The target prints; the value is not visited for printing.
        Expression::AssignmentExpression(assignment) => assignment
            .left
            .as_simple_assignment_target()
            .map_or_else(String::new, render_simple_target),
        Expression::RegExpLiteral(literal) => format!("/{}/{}", literal.regex.pattern.text, literal.regex.flags),
        // A template without substitutions is a string literal to V8; one with them prints
        // its substitutions.
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked)
            .map_or_else(String::new, |cooked| format!("\"{cooked}\"")),
        Expression::TemplateLiteral(template) => {
            template.expressions.iter().map(|expression| find(expression, true)).collect()
        }
        Expression::ObjectExpression(object) => {
            format!("{{{}}}", INTERMEDIATE_VALUE.repeat(object.properties.len()))
        }
        _ => String::new(),
    }
}

/// A member: `object.name` for a name or string key, `object[key]` for any other key.
fn render_member(member: &MemberExpression<'_>) -> String {
    let dot = |optional: bool| if optional { "?." } else { "." };
    match member {
        MemberExpression::StaticMemberExpression(member) => {
            format!("{}{}{}", find(&member.object, true), dot(member.optional), member.property.name)
        }
        MemberExpression::ComputedMemberExpression(member) => match &member.expression {
            Expression::StringLiteral(key) => {
                format!("{}{}{}", find(&member.object, true), dot(member.optional), key.value)
            }
            key => format!(
                "{}{}[{}]",
                find(&member.object, true),
                if member.optional { "?." } else { "" },
                find(key, true)
            ),
        },
        MemberExpression::PrivateFieldExpression(member) => {
            let open = if member.optional { "?.[" } else { "[" };
            format!("{}{open}#{}]", find(&member.object, true), member.field.name)
        }
    }
}

/// An update's target, printed as a member or name.
fn render_simple_target(target: &SimpleAssignmentTarget<'_>) -> String {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => identifier.name.to_string(),
        target => target.as_member_expression().map_or_else(|| INTERMEDIATE_VALUE.to_owned(), render_member),
    }
}

/// The call sites of the throwing function's text, else of the top level at `offset`, parsed
/// now (`source_routes`).
pub(crate) fn unfrozen_call_sites(source: &crate::bytecode::ProgramSource, span: Option<crate::bytecode::SourceSpan>, offset: u32) -> Option<Vec<crate::bytecode::CallSiteDescription>> {
    span.and_then(|span| function_call_sites(source, span)).or_else(|| top_level_call_sites(source, offset))
}

/// The call sites of the function whose text is `span` of `source`, parsed alone at its
/// place: every other byte is blank (lines kept), and the function sits in parentheses, so
/// its offsets are the source's. `None` when it does not parse as an expression.
fn function_call_sites(source: &crate::bytecode::ProgramSource, span: crate::bytecode::SourceSpan) -> Option<Vec<crate::bytecode::CallSiteDescription>> {
    let text: &str = source.text.as_ref();
    let (start, end) = (span.start as usize, span.end as usize);
    if source.base != 0 || start == 0 || (start == 1 && end == text.len()) {
        return None;
    }
    let function = text.get(start..end)?;
    let blank = |part: &str| part.bytes().map(|byte| if byte == b'\n' { '\n' } else { ' ' }).collect::<String>();
    let padded = format!("{}({function})", blank(text.get(..start - 1)?));
    let goal = SourceType::script().with_typescript(source.typescript);
    // Only the callee is read, never the expression text, so the parse takes no tokens, and
    // the bodies of the functions inside it are released as they parse: the call is the
    // function's own (an inner function's call raises in its own frame). jsdom-d3-startup
    // threw and caught twice per iteration in a 2.4 MB module function, whose whole tree with
    // tokens took 39 MB of the run's peak each time.
    let allocator = crate::compile::lazy_bodies::sized_arena(&padded);
    let options = ParseOptions { allow_return_outside_function: true, lazy_function_bodies: Some(start as u32 + 1), ..ParseOptions::default() };
    let parsed = Parser::new(&allocator, &padded, goal).with_options(options).parse();
    if crate::compile::stack::validate_parsed(&parsed).is_err() || !parsed.diagnostics.is_empty() {
        return None;
    }
    let mut finder = CallFinder { source: &padded, comments: &parsed.program.comments, found: CallSites::new() };
    finder.visit_program(&parsed.program);
    Some(finder.found.into_values().collect())
}

/// The call sites of the top-level statement of `source` that holds `offset`: the source's
/// statements are parsed a run at a time, each run released once read
/// (`oxc_parser::StatementStream`), with every function body released as it parses
/// (`ParseOptions::lazy_function_bodies`) and without tokens, so an error a script's top level
/// raises and catches costs a run of its statements, not its top level's tree, its bodies
/// recognized without a tree (mandreel: a
/// caught top-level call took 29 MB at the run's peak to parse its 5 MB top level). A call
/// inside a released body is not listed.
fn top_level_call_sites(source: &crate::bytecode::ProgramSource, offset: u32) -> Option<Vec<crate::bytecode::CallSiteDescription>> {
    let text: &str = source.text.as_ref();
    if source.base != 0 {
        return None;
    }
    let allocator = oxc_allocator::Allocator::with_capacity(STREAM_ARENA);
    // Each body is recognized, not built: built then released, a 2.7 MB body took 33 MB.
    let mut accept = |_: oxc_parser::LazyBody<'_, '_>| true;
    // SAFETY: `accept` lives through the stream, which alone calls it.
    let check = unsafe { oxc_parser::LazyBodyCheck::new(&mut accept) };
    let options = ParseOptions {
        allow_return_outside_function: true,
        lazy_function_bodies: Some(0),
        lazy_recognize: true,
        lazy_body_check: Some(check),
        ..ParseOptions::default()
    };
    let mut stream = Parser::new(&allocator, text, SourceType::script().with_typescript(source.typescript)).with_options(options).statement_stream();
    let mut found = None;
    while found.is_none() {
        let step = stream.next(RUN_STATEMENTS, RUN_BYTES, &mut |_| {}, &mut |statements, comments| {
            let holding = statements.iter().filter(|statement| oxc_span::GetSpan::span(*statement).contains_inclusive(oxc_span::Span::empty(offset)));
            let mut finder = CallFinder { source: text, comments, found: CallSites::new() };
            for statement in holding {
                finder.visit_statement(statement);
            }
            if !finder.found.is_empty() {
                found = Some(finder.found.into_values().collect());
            }
        });
        if step != oxc_parser::StreamStep::More {
            break;
        }
    }
    found
}

/// The statements, and the arena bytes, of one run of `top_level_call_sites`, and its arena.
const RUN_STATEMENTS: usize = 4096;
const RUN_BYTES: usize = 1 << 20;
const STREAM_ARENA: usize = 2 << 20;
