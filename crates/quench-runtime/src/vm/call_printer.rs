//! The callee a failed call names in its TypeError, as V8's `CallPrinter` renders it
//! ("o.a.b is not a function", "(0 , f) is not a function"): the call at the throwing
//! position is found in source on the dynamic error path. Native images freeze the same
//! descriptions before publication, so their error path needs no syntax frontend.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, CallExpression, ChainElement, Comment, Expression, MemberExpression,
    NewExpression, SimpleAssignmentTarget,
};
use oxc_ast_visit::Visit;
use oxc_parser::{ParseOptions, Parser, Token, Kind, config::TokensParserConfig};
use oxc_span::SourceType;

use super::{Host, Vm};
use crate::bytecode::{Op, ResidualProgram, WideInstruction};
use crate::heap::Cell;
use crate::value::Value;
use std::collections::BTreeMap;
mod function_sites;

/// What `CallPrinter` prints for an operand it does not render.
const INTERMEDIATE_VALUE: &str = "(intermediate value)";

/// A nested call's arguments.
const CALL_ARGUMENTS: &str = "(...)";

/// The message templates (V8 `kCalledNonCallable`, `kNotConstructor`).
const NOT_A_FUNCTION: &str = " is not a function";
const NOT_A_CONSTRUCTOR: &str = " is not a constructor";
const CALL_SOURCE_GOALS: [SourceType; 2] = [SourceType::script(), SourceType::mjs()];

/// The messages the call paths give a non-callable callee before its call site is known.
pub(super) const NOT_CALLABLE_MESSAGE: &str = "value is not callable";
pub(super) const NOT_CONSTRUCTOR_MESSAGE: &str = "value is not a constructor";

/// How a call instruction names its callee.
enum Callee {
    /// The callee is this value.
    Value(Value),
    /// The callee is the receiver's `atom` property, read without running guest code.
    Method { receiver: Value, atom: crate::bytecode::Atom },
}

impl<H: Host> Vm<H> {
    /// `error` with the callee named when it is the call paths' failure to call or construct
    /// the callee of the call at `pc` (`NOT_CALLABLE_MESSAGE`, `NOT_CONSTRUCTOR_MESSAGE`):
    /// the TypeError V8 throws there. Any other error is returned as it is.
    pub(super) fn name_non_callable_callee(&mut self, program: &ResidualProgram, frame: usize, function: usize, pc: u32, error: super::JsError) -> super::JsError {
        let construct = if error.has_message(NOT_CALLABLE_MESSAGE) {
            false
        } else if error.has_message(NOT_CONSTRUCTOR_MESSAGE) {
            true
        } else {
            return error;
        };
        match self.non_callable_call_site_message(program, frame, function, pc, construct) {
            Some(message) => self.type_error(program, message),
            None => error,
        }
    }

    /// The message for calling (or, with `construct`, constructing) a non-callable callee at
    /// `pc` of `function`, the instruction `frame` runs: `None` when that instruction is not a
    /// call whose own callee is not callable, so the failure is from inside the call.
    pub(super) fn non_callable_call_site_message(&self, program: &ResidualProgram, frame: usize, function: usize, pc: u32, construct: bool) -> Option<String> {
        let instruction = program.function(function).instructions().nth(pc as usize)?;
        let callee = self.call_instruction_callee(program, frame, instruction, construct)?;
        let value = match callee {
            Callee::Value(value) => Some(value),
            Callee::Method { receiver, atom } => self.data_property_value_without_getters(receiver, atom),
        };
        // An accessor or proxy hides the callee: the failure may be anywhere, so keep the
        // generic message.
        let value = value?;
        let callable = if construct { self.is_constructable(value) } else { self.is_function(value) };
        if callable {
            return None;
        }
        let template = if construct { NOT_A_CONSTRUCTOR } else { NOT_A_FUNCTION };
        let rendered = program
            .source_position(function as u32, pc)
            .and_then(|position| {
                let source = program.source.as_ref()?;
                let offset = source.byte_offset(position)?;
                let span = program.functions[function].header().source_span;
                function_sites::call_site_callee(source, span, (offset, construct))
            })
            .unwrap_or_else(|| self.default_call_site(value));
        Some(rendered + template)
    }

    fn call_instruction_callee(&self, program: &ResidualProgram, frame: usize, instruction: WideInstruction, construct: bool) -> Option<Callee> {
        let register = |index: u16| self.frames.get(frame)?.registers.get(usize::from(index)).copied();
        if call_kind(instruction)? != construct { return None; }
        Some(match instruction.op() {
            Op::CallMethod => Callee::Method {
                receiver: register(instruction.register_b())?,
                atom: program.method_sites.get(instruction.method_site_index())?.atom,
            },
            _ => Callee::Value(register(instruction.register_b())?),
        })
    }

    /// `receiver[atom]` when every object on its prototype chain answers it with a data
    /// property or nothing (undefined); `None` when an accessor or proxy would run code.
    fn data_property_value_without_getters(&self, receiver: Value, atom: crate::bytecode::Atom) -> Option<Value> {
        let key = crate::vm::PropertyKey::string(atom);
        let mut holder = self.primitive_prototype(receiver).unwrap_or(receiver);
        loop {
            if holder.is_null() || holder.is_undefined() {
                return Some(Value::UNDEFINED);
            }
            if matches!(self.heap.get(holder), Some(Cell::Proxy { .. })) {
                return None;
            }
            if let Some(attributes) = self.property_attributes(holder, key) {
                return (!attributes.accessor).then(|| self.own_property(holder, atom)).flatten();
            }
            holder = match self.object_data(holder) {
                Some(object) => object.proto(),
                None => return Some(Value::UNDEFINED),
            };
        }
    }

    /// V8's `BuildDefaultCallSite`: the callee's type and, for a primitive, its value.
    fn default_call_site(&self, value: Value) -> String {
        let kind = super::coercion::typeof_text(self.typeof_kind(value));
        if let Some(text) = self.heap.string(value) {
            return format!("{kind} \"{text}\"");
        }
        if value.is_null() {
            return format!("{kind} null");
        }
        if let Some(flag) = value.as_bool() {
            return format!("{kind} {flag}");
        }
        if let Some(number) = value.as_number() {
            return format!("{kind} {}", crate::number_to_string::format(number));
        }
        kind.to_owned()
    }
}

fn call_kind(instruction: WideInstruction) -> Option<bool> {
    match instruction.op() {
        Op::Construct if !instruction.is_super_construct() => Some(true),
        Op::Call | Op::CallArray | Op::CallSpread | Op::CallForwardingArguments | Op::CallMethod => Some(false),
        _ => None,
    }
}

/// Freeze source-derived descriptions without opening or compiling lazy function bodies.
/// A native-only build freezes them as it compiles; a dynamic one on their first read
/// (`frozen_call_sites`) or before writing the program, so a load that raises no error
/// parses its source once.
pub(crate) fn freeze_call_sites(program: &ResidualProgram) {
    if let Some(source) = &program.source {
        freeze_source_call_sites(source);
    }
}

/// The call-site descriptions of `source`: frozen ones, or, in a dynamic build, those its
/// text gives now. Native-only execution never reconstructs syntax on a diagnostic path.
fn frozen_call_sites(source: &crate::bytecode::ProgramSource) -> Option<&[crate::bytecode::CallSiteDescription]> {
    if crate::DYNAMIC_EXECUTION_ENABLED {
        freeze_source_call_sites(source);
    }
    source.call_sites.get().map(|sites| &sites[..])
}

pub(crate) fn freeze_source_call_sites(source: &crate::bytecode::ProgramSource) {
    source.call_sites.get_or_init(|| {
        let mut found = CallSites::new();
        for goal in CALL_SOURCE_GOALS.map(|goal| goal.with_typescript(source.typescript)) {
            match parse_call_sites(source.text.as_ref(), goal, found) {
                Ok((next, valid)) => { found = next; if valid { break; } }
                Err(previous) => { found = previous; break; }
            }
        }
        found.into_values().collect::<Vec<_>>().into()
    });
}

/// The admitted source expression at an exact call position. Native-only execution
/// consumes frozen facts; it never reconstructs syntax on a diagnostic path.
pub(super) fn source_expression(program: &ResidualProgram, line: u32, column: u32) -> Option<String> {
    let source = program.source.as_ref()?;
    let offset = source.byte_offset(crate::bytecode::SourcePosition { pc: 0, line, column })?;
    let sites = frozen_call_sites(source)?;
    let index = sites.binary_search_by_key(&offset, |site| site.offset).ok()?;
    let span = sites[index].expression?;
    source.text.get(span.start as usize..span.end as usize).map(str::to_owned)
}

/// The calls V8 positions at an offset, by offset and kind: a call of a name at the name, a
/// non-optional call of a named property at the property name, a construction at `new`, any
/// other call at its argument list's `(`.
type CallSites = BTreeMap<(u32, bool), crate::bytecode::CallSiteDescription>;

fn parse_call_sites(source: &str, goal: SourceType, found: CallSites) -> Result<(CallSites, bool), CallSites> {
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
    let allocator = Allocator::with_capacity(text.len());
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

#[cfg(all(test, feature = "aot"))]
mod tests {
    use super::*;

    #[test]
    fn assertion_expression_projection_follows_node_token_boundaries() {
        for (text, position, expected) in [
            ("assert(false)", "assert", "assert(false)"),
            ("assert.ok(false)", "ok(", "assert.ok(false)"),
            ("assert['ok'](false)", "(false)", "assert['ok'](false)"),
            ("assert?.ok(false)", "ok(", "assert?.ok(false)"),
            ("(0, assert)(false)", "(false)", "(false)"),
            ("assert[(() => 'ok')()](false)", "(false)", "(false)"),
            ("assert.ok(\n  false\n)", "ok(", "assert.ok("),
            ("const text='😀'; assert.ok(text === '')", "ok(", "assert.ok(text === '')"),
            ("new assert(false)", "new assert", "new assert(false)"),
            ("assert.ok(/[(;)]/.test(''))", "ok(", "assert.ok(/[(;)]/.test(''))"),
        ] {
            let program = crate::Engine::specialize(text, "assertion-source.js").unwrap();
            freeze_call_sites(&program);
            let source = program.source.as_ref().unwrap();
            let site = source.call_sites.get().expect("facts are frozen from source").iter()
                .find(|site| site.offset as usize == text.find(position).unwrap())
                .expect("assertion call");
            let actual = site.expression.map(|span| &source.text[span.start as usize..span.end as usize]);
            assert_eq!(actual, Some(expected), "{text}");
        }
        let program = crate::Engine::specialize("assert.ok((()=>{return false;})())", "semicolon-expression.js").unwrap();
        freeze_call_sites(&program);
        let sites = program.source.as_ref().unwrap().call_sites.get().unwrap();
        assert!(sites.iter().find(|site| site.callee == "assert.ok").unwrap().expression.is_none(),
            "Node's semicolon terminator yields an empty expression");
    }

    #[test]
    fn diagnostics_keep_lazy_records_and_native_lookup_never_parses() {
        let mut program = crate::Engine::specialize(
            "function cold(value) { assert.ok(value); }", "cold-diagnostics.js").unwrap();
        let released: Vec<_> = program.functions.iter().map(crate::bytecode::FunctionSlot::is_released).collect();
        assert!(released.iter().any(|released| *released), "the fixture must keep a cold record");
        // A dynamic build defers the facts to their first read; a native-only one admits them.
        assert_eq!(program.source.as_ref().unwrap().call_sites.get().is_some(), !crate::DYNAMIC_EXECUTION_ENABLED);
        program.source.as_mut().unwrap().call_sites.take();
        freeze_call_sites(&program);
        assert_eq!(program.functions.iter().map(crate::bytecode::FunctionSlot::is_released).collect::<Vec<_>>(), released);
        let source = program.source.as_ref().unwrap();
        let sites = source.call_sites.get().unwrap();
        let owner = sites.as_ptr();
        let column = "function cold(value) { assert.".len() as u32 + 1;
        for _ in 0..2 {
            assert_eq!(source_expression(&program, 1, column).as_deref(), Some("assert.ok(value)"));
            assert_eq!(program.source.as_ref().unwrap().call_sites.get().unwrap().as_ptr(), owner);
        }
        program.source.as_mut().unwrap().call_sites.take();
        if crate::DYNAMIC_EXECUTION_ENABLED {
            assert_eq!(source_expression(&program, 1, column).as_deref(), Some("assert.ok(value)"), "a dynamic lookup derives them");
        } else {
            assert_eq!(source_expression(&program, 1, column), None, "lookup never derives missing facts");
            assert!(program.source.as_ref().unwrap().call_sites.get().is_none());
        }
    }

    #[test]
    fn frozen_expression_ranges_are_validated_by_binary_and_snapshot_readers() {
        use crate::bytecode::SourceSpan;
        let text = "const face='😀'; call();\nnext();";
        let offset = text.find("call").unwrap() as u32;
        let emoji = text.find('😀').unwrap() as u32;
        for span in [
            SourceSpan { start: offset + 1, end: offset + 6 },
            SourceSpan { start: 0, end: offset - 1 },
            SourceSpan { start: offset + 6, end: offset },
            SourceSpan { start: emoji + 1, end: offset + 6 },
            SourceSpan { start: offset, end: u32::MAX },
            SourceSpan { start: offset, end: text.len() as u32 },
        ] {
            let mut program = crate::Engine::specialize(text, "invalid-expression.js").unwrap();
            program.freeze_native_diagnostics();
            let source = program.source.as_mut().unwrap();
            let mut sites = source.call_sites.take().unwrap().to_vec();
            sites.iter_mut().find(|site| site.offset == offset).unwrap().expression = Some(span);
            source.call_sites = std::cell::OnceCell::from(std::sync::Arc::from(sites));
            let mut writer = crate::snapshot::Writer::new();
            source.write_snapshot(&mut writer);
            let bytes = writer.finish();
            let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
            assert!(crate::bytecode::ProgramSource::read_snapshot(&mut reader).is_err(), "{span:?}");
            let decoded = crate::ResidualProgram::decode_trusted(std::rc::Rc::new(program.encode_trusted())).unwrap();
            assert!(decoded.validate().unwrap_err().contains("call-site expression"), "{span:?}");
        }
    }

    #[test]
    fn frozen_descriptions_cover_every_call_and_survive_both_encodings() {
        let body = "const marker = '😀'; function test() { missing(); new Missing(); object.deep.method(); (0, object.method)(); object['key'](); optional?.(); }";
        for (text, typescript, retained) in [
            (body.to_owned(), false, None),
            (format!("const typed: number = 1; {body}"), true, None),
            (format!("(function () {{\n  {body}\n}})"), false, Some(("(function () {\n  ", body))),
        ] {
            let origin = retained.map(|(prefix, _)| crate::wrapper_origin(prefix).unwrap());
            let mut program = crate::Engine::specialize_host_script(&text, "calls.js", &crate::compile::NoAtomPrefix,
                crate::HostScriptOptions { origin, typescript, keep_records: true }, crate::compile::ReleasedRecords::Encoded).unwrap();
            if let Some((prefix, body)) = retained {
                let source = program.source.as_mut().unwrap();
                source.retain_body(crate::bytecode::SourceText::Shared(body.into()), prefix.len() as u32);
                (source.line_offset, source.column_offset) = origin.unwrap();
            }
            let source = program.source.as_ref().unwrap();
            let mut calls = std::collections::BTreeSet::new();
            for function in 0..program.function_count() {
                for (pc, instruction) in program.function(function).instructions().enumerate() {
                    if let Some(construct) = call_kind(instruction)
                        && let Some(offset) = program.source_position(function as u32, pc as u32).and_then(|position| source.byte_offset(position))
                    { calls.insert((offset, construct)); }
                }
            }
            program.freeze_native_diagnostics();
            let actual: BTreeMap<_, _> = source.call_sites.get().unwrap().iter()
                .map(|site| ((site.offset, site.construct), site.callee.clone())).collect();
            assert!(!calls.is_empty(), "typescript={typescript} retained={retained:?}");
            assert!(calls.iter().all(|call| actual.contains_key(call)), "typescript={typescript} retained={retained:?}");
            for callee in ["missing", "Missing", "object.deep.method"] {
                assert!(actual.values().any(|rendered| rendered == callee), "{callee}: typescript={typescript} retained={retained:?}");
            }
            let decoded = crate::ResidualProgram::decode_trusted(std::rc::Rc::new(program.encode_trusted())).unwrap();
            assert_eq!(decoded.source.as_ref().unwrap().call_sites.get(), source.call_sites.get());
            let mut writer = crate::snapshot::Writer::new();
            source.write_snapshot(&mut writer);
            let bytes = writer.finish();
            let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
            let restored = crate::bytecode::ProgramSource::read_snapshot(&mut reader).unwrap();
            reader.finish().unwrap();
            assert_eq!(restored.call_sites.get(), source.call_sites.get());
        }
    }

    #[test]
    fn invalid_frozen_descriptions_are_rejected_and_empty_facts_are_complete() {
        let program = crate::Engine::specialize("42;", "empty.js").unwrap();
        program.freeze_native_diagnostics();
        assert!(program.source.as_ref().unwrap().call_sites.get().unwrap().is_empty());
        for sites in [
            vec![(0, "first"), (0, "duplicate")],
            vec![(2, "later"), (0, "earlier")],
            vec![(u32::MAX, "outside")],
            vec![(0, "")],
        ] {
            let mut invalid = crate::Engine::specialize("42;", "invalid.js").unwrap();
            invalid.source.as_mut().unwrap().call_sites = std::cell::OnceCell::from(std::sync::Arc::from(sites.into_iter()
                .map(|(offset, callee)| crate::bytecode::CallSiteDescription { offset, construct: false, callee: callee.into(), expression: None })
                .collect::<Vec<_>>()));
            let decoded = crate::ResidualProgram::decode_trusted(std::rc::Rc::new(invalid.encode_trusted())).unwrap();
            assert!(decoded.validate().unwrap_err().contains("frozen call-site"));
        }
    }

    #[test]
    fn typescript_assertions_and_type_arguments_preserve_callee_identity() {
        let program = crate::Engine::specialize_typescript(
            "const f = 0 as unknown as {(): unknown; new(): unknown}; (f as any)(); f!(); f?.<() => void>(); new (f as any)();",
            "typed-calls.cts",
        ).unwrap();
        program.freeze_native_diagnostics();
        let sites = program.source.as_ref().unwrap().call_sites.get().unwrap();
        assert_eq!(sites.len(), 4);
        assert!(sites.iter().all(|site| site.callee == "f"), "{sites:?}");
    }
}
