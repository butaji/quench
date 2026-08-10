use std::{collections::HashMap, convert::TryFrom};

use oxc::ast::ast::BindingPatternKind;

use crate::{
    facts::ProgramDb,
    ops::{FunctionKind, Op},
};

pub(crate) fn function_parameters(
    function: &oxc::ast::ast::Function<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    parameters(&function.params)
}

fn parameters(
    formal: &oxc::ast::ast::FormalParameters<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    let mut parameters = HashMap::new();
    for (slot, parameter) in formal.items.iter().enumerate() {
        let BindingPatternKind::BindingIdentifier(identifier) = &parameter.pattern.kind else {
            return Err(vec!["Unsupported function parameter pattern".to_string()]);
        };
        let slot =
            u16::try_from(slot).map_err(|_| vec!["Too many function parameters".to_string()])?;
        parameters.insert(identifier.name.to_string(), slot);
    }
    let count = u16::try_from(formal.items.len())
        .map_err(|_| vec!["Too many function parameters".to_string()])?;
    Ok((parameters, count))
}

pub(crate) fn reduce_body(
    body: &oxc::ast::ast::FunctionBody<'_>,
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
) -> Result<Vec<Op>, Vec<String>> {
    crate::reduce::reduce_statements_with_locals(
        &body.statements,
        facts,
        parameters,
        captures.saturating_add(parameter_count).saturating_add(2),
    )
}

fn capture_count(locals: &HashMap<String, u16>) -> u16 {
    locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1))
}

fn extend_function_parameters(
    mut parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
    locals: &HashMap<String, u16>,
) -> HashMap<String, u16> {
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    parameters.insert(
        "arguments".to_string(),
        captures.saturating_add(parameter_count),
    );
    parameters.insert(
        "this".to_string(),
        captures.saturating_add(parameter_count).saturating_add(1),
    );
    parameters.extend(locals.iter().map(|(name, slot)| (name.clone(), *slot)));
    parameters
}

fn reduce_function_ops(
    statements: &[oxc::ast::ast::Statement<'_>],
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    locals: &HashMap<String, u16>,
) -> Option<(Vec<Op>, u16)> {
    let captures = capture_count(locals);
    let parameters = extend_function_parameters(parameters, parameter_count, captures, locals);
    let local_count = captures.saturating_add(parameter_count).saturating_add(2);
    let body_ops =
        crate::reduce::reduce_statements_with_locals(statements, facts, parameters, local_count)
            .ok()?;
    Some((body_ops, captures))
}

fn emit_function_op(
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    body: Vec<Op>,
    params: u16,
    captures: u16,
    kind: FunctionKind,
) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunctionWithKind {
        dst: register,
        body,
        params,
        captures,
        kind,
    });
    register
}

pub(crate) fn reduce_expression(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let body = function.body.as_ref()?;
    let (parameters, parameter_count) = function_parameters(function).ok()?;
    let (body_ops, captures) =
        reduce_function_ops(&body.statements, facts, parameters, parameter_count, locals)?;
    Some(emit_function_op(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionKind::Ordinary,
    ))
}

pub(crate) fn reduce_arrow(
    function: &oxc::ast::ast::ArrowFunctionExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (parameters, parameter_count) = parameters(&function.params).ok()?;
    let (body_ops, captures) = reduce_function_ops(
        &function.body.statements,
        facts,
        parameters,
        parameter_count,
        locals,
    )?;
    Some(emit_function_op(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionKind::Arrow,
    ))
}

pub(crate) fn make(
    body: &[Op],
    params: u16,
    captures: Vec<crate::value::Value>,
) -> crate::value::Value {
    let value = crate::value::Value::Function(std::rc::Rc::new(crate::value::FunctionValue {
        body: body.to_vec(),
        params,
        captures: std::rc::Rc::new(std::cell::RefCell::new(captures)),
        properties: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
    }));
    let prototype = crate::value::Value::Object(std::rc::Rc::new(vec![(
        "constructor".to_string(),
        value.clone(),
    )]));
    if let crate::value::Value::Function(ref function) = value {
        function
            .properties
            .borrow_mut()
            .push(("prototype".to_string(), prototype));
    }
    value
}

pub(crate) fn write(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    body: &[Op],
    params: u16,
    captures: u16,
    _kind: FunctionKind,
) {
    let mut values = registers
        .iter()
        .take(usize::from(captures))
        .cloned()
        .collect::<Vec<_>>();
    values.resize(usize::from(captures), crate::value::Value::Undefined);
    crate::execute::write_value(registers, dst, make(body, params, values));
}

pub(crate) fn write_op(registers: &mut Vec<crate::value::Value>, op: &Op) {
    match op {
        Op::MakeFunction {
            dst,
            body,
            params,
            captures,
        } => write(
            registers,
            *dst,
            body,
            *params,
            *captures,
            FunctionKind::Ordinary,
        ),
        Op::MakeFunctionWithKind {
            dst,
            body,
            params,
            captures,
            kind,
        } => write(registers, *dst, body, *params, *captures, *kind),
        _ => {}
    }
}

fn build_registers(
    function: &crate::value::FunctionValue,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Vec<crate::value::Value> {
    let original_arguments = arguments.to_vec();
    let mut parameters = arguments.to_vec();
    parameters.resize(usize::from(function.params), crate::value::Value::Undefined);
    parameters.truncate(usize::from(function.params));
    let mut registers = function.captures.borrow().clone();
    registers.extend(parameters);
    let base = function.captures.borrow().len() as u16;
    crate::execute::write_value(
        &mut registers,
        base + function.params,
        crate::value::Value::Array(std::rc::Rc::new(original_arguments)),
    );
    crate::execute::write_value(
        &mut registers,
        base + function.params + 1,
        this_value.clone(),
    );
    registers.resize(
        registers.len().saturating_add(32),
        crate::value::Value::Undefined,
    );
    registers
}

pub(crate) fn execute(
    function: &crate::value::FunctionValue,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let registers = build_registers(function, this_value, arguments);
    crate::execute::execute_with_registers(&function.body, registers)
}

/// Execute a constructor, returning both its result and the object bound to
/// `this` after it ran (so `this.message = ...` mutations are preserved).
pub(crate) fn execute_construct(
    function: &crate::value::FunctionValue,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let this_slot = function.captures.borrow().len() as u16 + function.params + 1;
    let mut registers = build_registers(function, this_value, arguments);
    let result = crate::execute::execute_in_place(&function.body, &mut registers)?;
    let final_this = crate::execute::read_register(&registers, this_slot)
        .unwrap_or(crate::value::Value::Undefined);
    Ok((result, final_this))
}

pub(crate) fn execute_bound(
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    match &bound.target {
        crate::value::Value::Builtin(builtin) => crate::execute::execute_builtin_with_receiver(
            *builtin,
            &combined,
            Some(&bound.receiver),
        ),
        crate::value::Value::Function(function) => execute(function, &bound.receiver, &combined),
        crate::value::Value::BoundFunction(next) => execute_bound(next, &combined),
        crate::value::Value::Proxy(_) => {
            crate::proxy::proxy_apply(&bound.target, &bound.receiver, &combined)
        }
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn execute_target(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match target {
        crate::value::Value::Builtin(builtin) => {
            crate::execute::execute_builtin_with_receiver(*builtin, arguments, Some(receiver))
        }
        crate::value::Value::Function(function) => execute(function, receiver, arguments),
        crate::value::Value::BoundFunction(bound) => execute_bound(bound, arguments),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn execute_function_call(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = receiver.ok_or(crate::execute::VmError::NotCallable)?;
    let this = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    execute_target(receiver, &this, arguments.get(1..).unwrap_or_default())
}

fn bind_function_target(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if !matches!(
        receiver,
        Some(
            crate::value::Value::Builtin(_)
                | crate::value::Value::Function(_)
                | crate::value::Value::BoundFunction(_)
                | crate::value::Value::Proxy(_)
        )
    ) {
        return Err(crate::execute::VmError::NotCallable);
    }
    let target = arguments
        .first()
        .ok_or(crate::execute::VmError::NotCallable)?;
    Ok(crate::value::Value::BoundFunction(std::rc::Rc::new(
        crate::value::BoundFunctionValue {
            target: receiver.cloned().unwrap_or(crate::value::Value::Undefined),
            receiver: target.clone(),
            arguments: arguments[1..].to_vec(),
        },
    )))
}

pub(crate) fn function_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::FunctionCall => execute_function_call(receiver, arguments),
        crate::ops::Builtin::FunctionBind => bind_function_target(receiver, arguments),
        crate::ops::Builtin::ArrayJoin => Ok(crate::builtins::array_join(receiver, arguments)),
        crate::ops::Builtin::ArrayPush => Ok(crate::builtins::array_push(receiver, arguments)),
        crate::ops::Builtin::ObjectPropertyIsEnumerable => Ok(
            crate::builtins::object::object_property_is_enumerable(receiver, arguments),
        ),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}
