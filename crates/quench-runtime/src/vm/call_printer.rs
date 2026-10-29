//! The callee a failed call names in its TypeError, as V8's `CallPrinter` renders it
//! ("o.a.b is not a function", "(0 , f) is not a function"): the call at the throwing
//! position is found in the program's source on the error path, so no call site carries a
//! description while code runs.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, CallExpression, ChainElement, Expression, MemberExpression,
    NewExpression, SimpleAssignmentTarget,
};
use oxc_ast_visit::Visit;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType};

use super::{Host, Vm};
use crate::bytecode::{Op, ResidualProgram, WideInstruction};
use crate::heap::Cell;
use crate::value::Value;

/// What `CallPrinter` prints for an operand it does not render.
const INTERMEDIATE_VALUE: &str = "(intermediate value)";

/// A nested call's arguments.
const CALL_ARGUMENTS: &str = "(...)";

/// The message templates (V8 `kCalledNonCallable`, `kNotConstructor`).
const NOT_A_FUNCTION: &str = " is not a function";
const NOT_A_CONSTRUCTOR: &str = " is not a constructor";

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
        let callable = if construct { self.is_constructable(program, value) } else { self.is_function(value) };
        if callable {
            return None;
        }
        let template = if construct { NOT_A_CONSTRUCTOR } else { NOT_A_FUNCTION };
        let rendered = program
            .source_position(function as u32, pc)
            .and_then(|position| {
                let source = program.source.as_ref()?;
                let line = source.source_line(position.line)?;
                let line_start = line.as_ptr() as usize - source.text.as_ptr() as usize;
                let column = utf16_column_byte(line, position.column.checked_sub(1)?)?;
                render_call_site(&source.text, line_start + column, construct)
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
        Some(match instruction.op() {
            Op::Construct if construct && !instruction.is_super_construct() => {
                Callee::Value(register(instruction.register_b())?)
            }
            Op::Call | Op::CallArray | Op::CallSpread | Op::CallForwardingArguments if !construct => {
                Callee::Value(register(instruction.register_b())?)
            }
            Op::CallMethod if !construct => Callee::Method {
                receiver: register(instruction.register_b())?,
                atom: program.method_sites.get(instruction.method_site_index())?.atom,
            },
            _ => return None,
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

/// The byte offset of UTF-16 column `column` (zero-based) in `line`.
fn utf16_column_byte(line: &str, column: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in line.char_indices() {
        if units == column {
            return Some(byte);
        }
        units += character.len_utf16() as u32;
    }
    (units == column).then_some(line.len())
}

/// The callee of the call (or construction) at byte `offset` of `source`, rendered.
fn render_call_site(source: &str, offset: usize, construct: bool) -> Option<String> {
    let allocator = Allocator::with_capacity(source.len());
    let options = ParseOptions { allow_return_outside_function: true, ..ParseOptions::default() };
    let offset = u32::try_from(offset).ok()?;
    for source_type in [SourceType::script(), SourceType::mjs()] {
        let parsed = Parser::new(&allocator, source, source_type).with_options(options).parse();
        if crate::compile::stack::validate_parsed(&parsed).is_err() {
            return None;
        }
        let mut finder = CallFinder { source, offset, construct, found: None };
        finder.visit_program(&parsed.program);
        if finder.found.is_some() {
            return finder.found;
        }
        if parsed.diagnostics.is_empty() {
            return None;
        }
    }
    None
}

/// Finds the call V8 positions at `offset`: a call of a name at the name, a non-optional
/// call of a named property at the property name, a construction at `new`, any other call
/// at its argument list's `(`.
struct CallFinder<'s> {
    source: &'s str,
    offset: u32,
    construct: bool,
    found: Option<String>,
}

impl CallFinder<'_> {
    fn call_position(&self, call: &CallExpression<'_>) -> Option<u32> {
        match &call.callee {
            Expression::Identifier(name) => Some(name.span.start),
            Expression::StaticMemberExpression(member) if !call.optional => Some(member.property.span.start),
            callee => {
                let end = callee.span().end as usize;
                self.source.get(end..)?.find('(').map(|index| (end + index) as u32)
            }
        }
    }
}

impl<'a> Visit<'a> for CallFinder<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.found.is_some() {
            return;
        }
        if !self.construct && self.call_position(call) == Some(self.offset) {
            self.found = Some(find(&call.callee, true));
            return;
        }
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, new: &NewExpression<'a>) {
        if self.found.is_some() {
            return;
        }
        if self.construct && new.span.start == self.offset {
            self.found = Some(find(&new.callee, true));
            return;
        }
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
    if let Some(member) = expression.as_member_expression() {
        return render_member(member);
    }
    match expression {
        Expression::Identifier(identifier) => identifier.name.to_string(),
        Expression::ThisExpression(_) => "this".into(),
        Expression::Super(_) => "super".into(),
        Expression::ParenthesizedExpression(inner) => render(&inner.expression),
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
