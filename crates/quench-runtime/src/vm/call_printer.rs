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
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use super::{Host, Vm};
use crate::bytecode::{Op, ResidualProgram, WideInstruction};
use crate::heap::Cell;
use crate::value::Value;
#[cfg(feature = "aot")]
use std::collections::{BTreeMap, BTreeSet};

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
    pub(super) fn name_non_callable_callee(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        function: usize,
        pc: u32,
        error: super::JsError,
    ) -> super::JsError {
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
    pub(super) fn non_callable_call_site_message(
        &self,
        program: &ResidualProgram,
        frame: usize,
        function: usize,
        pc: u32,
        construct: bool,
    ) -> Option<String> {
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
                if let Some(sites) = source.call_sites.get() {
                    return sites.binary_search_by_key(&(offset, construct), |site| (site.offset, site.construct))
                        .ok().map(|index| sites[index].callee.clone());
                }
                crate::DYNAMIC_EXECUTION_ENABLED.then(|| render_call_site(&source.text, source.typescript, offset as usize, construct)).flatten()
            })
            .unwrap_or_else(|| self.default_call_site(value));
        Some(rendered + template)
    }

    fn call_instruction_callee(
        &self,
        program: &ResidualProgram,
        frame: usize,
        instruction: WideInstruction,
        construct: bool,
    ) -> Option<Callee> {
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

/// Resolve only descriptions whose positions the bytecode can observe. One
/// parse per source goal replaces reparsing the whole source for every call.
#[cfg(feature = "aot")]
pub(crate) fn freeze_call_sites(program: &ResidualProgram) {
    let Some(source) = &program.source else { return };
    source.call_sites.get_or_init(|| {
        let mut positions = BTreeSet::new();
        for function in 0..program.functions.len() {
            for (pc, instruction) in program.function(function).instructions().enumerate() {
                if let Some(construct) = call_kind(instruction)
                    && let Some(position) = program.source_position(function as u32, pc as u32)
                { positions.insert((position.line, position.column, construct)); }
            }
        }
        // Sorted columns share one scan of each line. Repeated scans would be
        // quadratic for minified modules whose calls all occupy one long line.
        let mut positions = positions.into_iter().peekable();
        let mut requested = BTreeSet::new();
        while let Some(&(line_number, _, _)) = positions.peek() {
            let line = source.source_line(line_number);
            let mut characters = line.unwrap_or("").char_indices().peekable();
            let mut units = 0;
            while positions.peek().is_some_and(|position| position.0 == line_number) {
                let (_, column, construct) = positions.next().unwrap();
                let Some(line) = line else { continue };
                let Some(column) = column.checked_sub(1) else { continue };
                while units < column {
                    let Some((_, character)) = characters.next() else { break };
                    units += character.len_utf16() as u32;
                }
                if units == column {
                    let byte = characters.peek().map_or(line.len(), |(byte, _)| *byte);
                    let line_start = line.as_ptr() as usize - source.text.as_ptr() as usize;
                    if let Ok(offset) = u32::try_from(line_start + byte) { requested.insert((offset, construct)); }
                }
            }
        }
        let mut search = CallSearch::All { requested, found: BTreeMap::new() };
        for goal in CALL_SOURCE_GOALS.map(|goal| goal.with_typescript(source.typescript)) {
            if search.complete() { break; }
            match parse_call_sites(source.text.as_ref(), goal, search) {
                Ok((next, valid)) => { search = next; if valid { break; } }
                Err(previous) => { search = previous; break; }
            }
        }
        let CallSearch::All { found, .. } = search else { unreachable!("all-call search changes shape") };
        found.into_iter().map(|((offset, construct), callee)| crate::bytecode::CallSiteDescription { offset, construct, callee })
            .collect::<Vec<_>>().into()
    });
}

/// The callee of the call (or construction) at byte `offset` of `source`, rendered.
fn render_call_site(source: &str, typescript: bool, offset: usize, construct: bool) -> Option<String> {
    let offset = u32::try_from(offset).ok()?;
    for source_type in CALL_SOURCE_GOALS.map(|goal| goal.with_typescript(typescript)) {
        let (search, valid) = parse_call_sites(source, source_type,
            CallSearch::One { offset, construct, found: None }).ok()?;
        let found = match search {
            CallSearch::One { found, .. } => found,
            #[cfg(feature = "aot")]
            CallSearch::All { .. } => unreachable!("single call search changes shape"),
        };
        if found.is_some() { return found; }
        if valid { return None; }
    }
    None
}

/// Finds the call V8 positions at `offset`: a call of a name at the name, a non-optional
/// call of a named property at the property name, a construction at `new`, any other call
/// at its argument list's `(`.
enum CallSearch {
    One { offset: u32, construct: bool, found: Option<String> },
    #[cfg(feature = "aot")]
    All { requested: BTreeSet<(u32, bool)>, found: BTreeMap<(u32, bool), String> },
}

impl CallSearch {
    fn complete(&self) -> bool {
        match self {
            Self::One { found, .. } => found.is_some(),
            #[cfg(feature = "aot")]
            Self::All { requested, found } => found.len() == requested.len(),
        }
    }

    fn observe(&mut self, offset: u32, construct: bool, callee: &Expression<'_>) {
        match self {
            Self::One { offset: target, construct: target_kind, found } if (*target, *target_kind) == (offset, construct) => {
                *found = Some(find(callee, true));
            }
            #[cfg(feature = "aot")]
            Self::All { requested, found } if requested.contains(&(offset, construct)) => {
                found.entry((offset, construct)).or_insert_with(|| find(callee, true));
            }
            _ => (),
        }
    }
}

fn parse_call_sites(source: &str, goal: SourceType, search: CallSearch) -> Result<(CallSearch, bool), CallSearch> {
    let allocator = Allocator::with_capacity(source.len());
    let options = ParseOptions { allow_return_outside_function: true, ..ParseOptions::default() };
    let parsed = Parser::new(&allocator, source, goal).with_options(options).parse();
    if crate::compile::stack::validate_parsed(&parsed).is_err() { return Err(search); }
    let mut finder = CallFinder { source, comments: &parsed.program.comments, search };
    finder.visit_program(&parsed.program);
    Ok((finder.search, parsed.diagnostics.is_empty()))
}

struct CallFinder<'s> {
    source: &'s str,
    comments: &'s [Comment],
    search: CallSearch,
}

impl CallFinder<'_> {
    fn call_position(&self, call: &CallExpression<'_>) -> u32 {
        crate::compile::source_syntax::call_offset(call, self.source, self.comments)
    }
}

impl<'a> Visit<'a> for CallFinder<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.search.complete() {
            return;
        }
        let offset = self.call_position(call);
        self.search.observe(offset, false, &call.callee);
        if self.search.complete() { return; }
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, new: &NewExpression<'a>) {
        if self.search.complete() {
            return;
        }
        self.search.observe(new.span.start, true, &new.callee);
        if self.search.complete() { return; }
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
    fn frozen_descriptions_match_dynamic_search_and_survive_both_encodings() {
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
                source.text = crate::bytecode::SourceText::Shared(body.into());
                source.base = prefix.len() as u32;
                (source.line_offset, source.column_offset) = origin.unwrap();
            }
            let source = program.source.as_ref().unwrap();
            let mut expected = BTreeMap::new();
            for function in 0..program.function_count() {
                for (pc, instruction) in program.function(function).instructions().enumerate() {
                    if let Some(construct) = call_kind(instruction)
                        && let Some(offset) = program.source_position(function as u32, pc as u32).and_then(|position| source.byte_offset(position))
                        && let Some(callee) = render_call_site(&source.text, source.typescript, offset as usize, construct)
                    { expected.insert((offset, construct), callee); }
                }
            }
            program.freeze_native_diagnostics();
            let actual: BTreeMap<_, _> = source.call_sites.get().unwrap().iter()
                .map(|site| ((site.offset, site.construct), site.callee.clone())).collect();
            assert!(!actual.is_empty(), "typescript={typescript} retained={retained:?}");
            assert_eq!(actual, expected, "typescript={typescript} retained={retained:?}");
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
                .map(|(offset, callee)| crate::bytecode::CallSiteDescription { offset, construct: false, callee: callee.into() })
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
