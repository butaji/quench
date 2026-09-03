use crate::{
    facts::ProgramDb,
    ops::{FunctionKind, FunctionStrictness, Op},
};
use std::collections::HashMap;
include!("functions_properties.rs");
const NEW_TARGET: &str = "\0new_target";
pub(crate) const FUNCTION_SELF: &str = "\0function_self";
pub(crate) const FUNCTION_NAME_IMMUTABLE: &str = "\0function_name_immutable";
#[derive(Clone)]
pub(crate) struct FunctionMetadata {
    pub(crate) kind: FunctionKind,
    pub(crate) length: u16,
    pub(crate) strictness: FunctionStrictness,
    pub(crate) is_async: bool,
    pub(crate) mapped_arguments: bool,
    pub(crate) direct_constructor: std::rc::Rc<[crate::facts::DirectConstructorField]>,
    pub(crate) composed_constructor: std::rc::Rc<[crate::facts::ComposedConstructorStep]>,
}
include!("functions_direct_constructor_fact.rs");
pub(super) fn function_parameters(
    function: &oxc::ast::ast::Function<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    crate::function_parameters::bindings(&function.params)
}
fn enter_function(
    facts: &mut ProgramDb,
    strictness: FunctionStrictness,
    tail_calls: bool,
) -> (bool, bool, bool, u16) {
    let inherited = (
        facts.strict,
        facts.in_function,
        facts.tail_calls,
        facts.function_dynamic_scope_floor,
    );
    facts.strict = matches!(strictness, FunctionStrictness::Strict);
    facts.in_function = true;
    facts.tail_calls = tail_calls;
    facts.function_dynamic_scope_floor = facts.dynamic_scope_depth;
    inherited
}
fn rest_slot(parameters: &HashMap<String, u16>) -> Option<u16> {
    parameters.get("\0rest").copied()
}

pub(crate) fn ordered_function_declarations_first<'a>(
    statements: &[oxc::ast::ast::Statement<'a>],
) -> Vec<usize> {
    let mut ordered = Vec::with_capacity(statements.len());
    for (index, statement) in statements.iter().enumerate() {
        if matches!(statement, oxc::ast::ast::Statement::FunctionDeclaration(_)) {
            ordered.push(index);
        }
    }
    for (index, statement) in statements.iter().enumerate() {
        if !matches!(statement, oxc::ast::ast::Statement::FunctionDeclaration(_)) {
            ordered.push(index);
        }
    }
    ordered
}

fn reserved_slots(lexical_receiver: bool, rest: Option<u16>) -> u16 {
    // arguments, this, new.target, and the optional function-self slot
    // that declarations install via FUNCTION_SELF.
    (if lexical_receiver { 2_u16 } else { 4_u16 }).saturating_add(u16::from(rest.is_some()))
}
pub(crate) fn execute_make_rest(
    slot: u16,
    arguments: u16,
    skip: u16,
) -> Result<(), crate::execute::VmError> {
    let arguments = crate::locals::current().get(arguments);
    crate::locals::current().set(slot, rest_array(&arguments, skip));
    Ok(())
}

fn rest_array(arguments: &crate::value::Value, skip: u16) -> crate::value::Value {
    let skip = usize::from(skip);
    let crate::value::Value::Array(values) = arguments else {
        return crate::value::Value::array(Vec::new());
    };
    let length = values.logical_len();
    let mut tail = Vec::with_capacity(length.saturating_sub(skip));
    for index in skip..length {
        tail.push(
            values
                .get_index(index)
                .unwrap_or(crate::value::Value::Undefined),
        );
    }
    crate::value::Value::array(tail)
}

fn bind_rest(mut body: Vec<Op>, rest: Option<u16>, params: u16, captures: u16) -> Vec<Op> {
    let Some(slot) = rest else { return body };
    let mut prefix = vec![Op::MakeRest {
        slot,
        arguments: captures.saturating_add(params),
        skip: params,
    }];
    prefix.append(&mut body);
    prefix
}
fn capture_count(locals: &HashMap<String, u16>) -> u16 {
    crate::reduce_support::register_base(locals)
}
pub(super) fn function_bindings(
    mut parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
    locals: &HashMap<String, u16>,
    lexical_receiver: bool,
) -> HashMap<String, u16> {
    if !lexical_receiver {
        let shifted = parameter_count.saturating_add(2);
        parameters
            .values_mut()
            .filter(|slot| **slot >= shifted)
            .for_each(|slot| *slot = slot.saturating_add(1));
    }
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    let mut bindings = locals.clone();
    bindings.retain(|name, _| name != "\0rest");
    if !lexical_receiver {
        let base = captures.saturating_add(parameter_count);
        for (name, offset) in [("arguments", 0), ("this", 1), (NEW_TARGET, 2)] {
            bindings.insert(name.to_string(), base.saturating_add(offset));
        }
    }
    bindings.extend(parameters);
    bindings
}
include!("functions_reduction.rs");

include!("functions_tail.rs");

include!("functions_arguments.rs");
include!("functions_receiver.rs");
