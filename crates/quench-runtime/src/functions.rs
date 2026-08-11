use crate::{
    facts::ProgramDb,
    ops::{FunctionKind, FunctionStrictness, Op},
};
use std::collections::HashMap;
include!("functions_properties.rs");
const NEW_TARGET: &str = "\0new_target";
pub(crate) const FUNCTION_SELF: &str = "\0function_self";
#[derive(Clone, Copy)]
pub(crate) struct FunctionMetadata {
    pub(crate) kind: FunctionKind,
    pub(crate) length: u16,
    pub(crate) strictness: FunctionStrictness,
    pub(crate) is_async: bool,
    pub(crate) mapped_arguments: bool,
}
pub(super) fn function_parameters(
    function: &oxc::ast::ast::Function<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    crate::function_parameters::bindings(&function.params)
}
pub(crate) fn reduce_body(
    body: &oxc::ast::ast::FunctionBody<'_>,
    formal: &oxc::ast::ast::FormalParameters<'_>,
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
    metadata: FunctionMetadata,
) -> Result<Vec<Op>, Vec<String>> {
    let lexical_receiver = matches!(metadata.kind, FunctionKind::Arrow);
    let rest = rest_slot(
        &parameters,
        parameter_count,
        captures,
        lexical_receiver,
        false,
    );
    let minimum = captures
        .saturating_add(parameter_count)
        .saturating_add(reserved_slots(lexical_receiver, rest));
    let next_slot = crate::reduce_support::register_base(&parameters).max(minimum);
    let tail_calls = tail_calls_enabled(metadata.strictness, metadata.kind, metadata.is_async);
    let inherited = enter_function(facts, metadata.strictness, tail_calls);
    let prefix = crate::function_parameters::prefix(formal, facts, &parameters, captures, true);
    let reduced = reduce_body_statements(&body.statements, formal, facts, &parameters, next_slot)?;
    (facts.strict, facts.in_function, facts.tail_calls) = inherited;
    let mut prefix =
        prefix.ok_or_else(|| vec!["Unsupported function parameter initialization".to_string()])?;
    prefix.extend((!prefix.is_empty()).then_some(Op::ParameterEnd));
    prefix.extend(reduced);
    Ok(bind_rest(prefix, rest, parameter_count, captures))
}

fn enter_function(
    facts: &mut ProgramDb,
    strictness: FunctionStrictness,
    tail_calls: bool,
) -> (bool, bool, bool) {
    let inherited = (facts.strict, facts.in_function, facts.tail_calls);
    facts.strict = matches!(strictness, FunctionStrictness::Strict);
    facts.in_function = true;
    facts.tail_calls = tail_calls;
    inherited
}
fn rest_slot(
    parameters: &HashMap<String, u16>,
    params: u16,
    captures: u16,
    lexical_receiver: bool,
    reserve_self: bool,
) -> Option<u16> {
    let slot = captures
        .saturating_add(params)
        .saturating_add(if lexical_receiver {
            2
        } else {
            3 + u16::from(reserve_self)
        });
    parameters.values().copied().find(|value| *value == slot)
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
    (if lexical_receiver { 2_u16 } else { 3_u16 }).saturating_add(u16::from(rest.is_some()))
}
fn bind_rest(mut body: Vec<Op>, rest: Option<u16>, params: u16, captures: u16) -> Vec<Op> {
    let Some(slot) = rest else { return body };
    let arguments = captures.saturating_add(params);
    let mut prefix = vec![
        Op::LoadLocal {
            dst: 0,
            slot: arguments,
        },
        Op::Const {
            dst: 1,
            value: crate::ops::Constant::Number(f64::from(params)),
        },
        Op::CallMethod {
            dst: 2,
            object: 0,
            key: "slice".to_string(),
            callee: None,
            args: vec![1],
        },
        Op::StoreLocal { slot, src: 2 },
    ];
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
