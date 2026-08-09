use std::{collections::HashMap, convert::TryFrom};

use oxc::ast::ast::BindingPatternKind;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn function_parameters(
    function: &oxc::ast::ast::Function<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    let mut parameters = HashMap::new();
    for (slot, parameter) in function.params.items.iter().enumerate() {
        let BindingPatternKind::BindingIdentifier(identifier) = &parameter.pattern.kind else {
            return Err(vec!["Unsupported function parameter pattern".to_string()]);
        };
        let slot =
            u16::try_from(slot).map_err(|_| vec!["Too many function parameters".to_string()])?;
        parameters.insert(identifier.name.to_string(), slot);
    }
    let count = u16::try_from(function.params.items.len())
        .map_err(|_| vec!["Too many function parameters".to_string()])?;
    Ok((parameters, count))
}

pub(crate) fn reduce_expression(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let body = function.body.as_ref()?;
    let (mut parameters, parameter_count) = function_parameters(function).ok()?;
    let captures = locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1));
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    parameters.insert(
        "arguments".to_string(),
        captures.saturating_add(parameter_count),
    );
    parameters.extend(locals.iter().map(|(name, slot)| (name.clone(), *slot)));
    let body_ops = crate::reduce::reduce_statements_with_locals(
        &body.statements,
        facts,
        parameters,
        parameter_count,
    )
    .ok()?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunction {
        dst: register,
        body: body_ops,
        params: parameter_count,
        captures,
    });
    Some(register)
}

pub(crate) fn make(
    body: &[Op],
    params: u16,
    captures: Vec<crate::value::Value>,
) -> crate::value::Value {
    crate::value::Value::Function(std::rc::Rc::new(crate::value::FunctionValue {
        body: body.to_vec(),
        params,
        captures: std::rc::Rc::new(captures),
        properties: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
    }))
}

pub(crate) fn write(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    body: &[Op],
    params: u16,
    captures: u16,
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
    let Op::MakeFunction {
        dst,
        body,
        params,
        captures,
    } = op
    else {
        return;
    };
    write(registers, *dst, body, *params, *captures);
}

pub(crate) fn execute(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let original_arguments = arguments.to_vec();
    let mut parameters = arguments.to_vec();
    parameters.resize(usize::from(function.params), crate::value::Value::Undefined);
    parameters.truncate(usize::from(function.params));
    let mut registers = function.captures.as_ref().clone();
    registers.extend(parameters);
    crate::execute::write_value(
        &mut registers,
        function.captures.len() as u16 + function.params,
        crate::value::Value::Array(std::rc::Rc::new(original_arguments)),
    );
    registers.resize(
        registers.len().saturating_add(32),
        crate::value::Value::Undefined,
    );
    crate::execute::execute_with_registers(&function.body, registers)
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
        crate::value::Value::Function(function) => execute(function, &combined),
        crate::value::Value::BoundFunction(next) => execute_bound(next, &combined),
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
        crate::value::Value::Function(function) => execute(function, arguments),
        crate::value::Value::BoundFunction(bound) => execute_bound(bound, arguments),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn function_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::FunctionCall => {
            let receiver = receiver.ok_or(crate::execute::VmError::NotCallable)?;
            let this = arguments
                .first()
                .cloned()
                .unwrap_or(crate::value::Value::Undefined);
            execute_target(receiver, &this, &arguments[1..])
        }
        crate::ops::Builtin::FunctionBind => {
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
        crate::ops::Builtin::ArrayJoin => Ok(crate::builtins::array_join(receiver, arguments)),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}
