use crate::{
    facts::ProgramDb,
    ops::Op,
    value::{ObjectData, Value},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
pub(crate) fn reduce(
    expression: &oxc::ast::ast::NewExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let callee =
        crate::reduce::reduce_expression(&expression.callee, ops, facts, next_register, locals)?;
    let (args, spreads) = crate::reduce::reduce_expressions::calls_reduce::reduce_arguments(
        &expression.arguments,
        ops,
        facts,
        next_register,
        locals,
    )?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Construct {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}
pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), crate::execute::VmError> {
    let Op::Construct {
        dst,
        callee,
        args,
        spreads,
    } = op
    else {
        return Err(crate::execute::VmError::NotCallable);
    };
    let arguments = collect_construct_arguments(registers, args, spreads)?;
    let target = crate::execute::read_register(registers, *callee)?;
    let value = construct_value(&target, &arguments)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
fn collect_construct_arguments(
    registers: &[Value],
    args: &[u16],
    spreads: &[bool],
) -> Result<Vec<Value>, crate::execute::VmError> {
    let mut arguments = Vec::new();
    for (index, spread) in args.iter().zip(spreads) {
        let value = crate::execute::read_register(registers, *index)?;
        if *spread {
            arguments.extend(crate::collections::iterator::collect_iterable(value)?);
        } else {
            arguments.push(value);
        }
    }
    Ok(arguments)
}
pub(crate) fn construct_value(
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, target, arguments)
}
pub(crate) fn construct_value_with_new_target(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, new_target, arguments)
}
pub(crate) fn construct_super(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, new_target, arguments)
}
fn construct_with_new_target(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match target {
        Value::Builtin(builtin) => {
            construct_builtin_target(*builtin, target, new_target, arguments)
        }
        Value::Function(function) => construct_function(function, new_target, arguments),
        Value::BoundFunction(bound) => construct_bound(bound, target, new_target, arguments),
        _ => Err(crate::vm::not_callable()),
    }
}

fn construct_builtin_target(
    builtin: crate::ops::Builtin,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if builtin == crate::ops::Builtin::SharedArrayBuffer
        && !crate::builtins::same_value(Some(target), Some(new_target))
    {
        return construct_shared_array_buffer_with_target(builtin, target, new_target, arguments);
    }
    let prototype = if builtin == crate::ops::Builtin::ArrayBuffer
        && !crate::builtins::same_value(Some(target), Some(new_target))
    {
        Some(crate::execute::get_property_result(
            new_target,
            "prototype",
        )?)
    } else {
        None
    };
    let value = construct_builtin(builtin, arguments)?;
    let value = if let Some(prototype) = prototype {
        apply_new_target_prototype(value, target, new_target, prototype)?
    } else {
        with_new_target_prototype(value, target, new_target)?
    };
    validate_data_view(&value)?;
    Ok(value)
}

fn construct_shared_array_buffer_with_target(
    builtin: crate::ops::Builtin,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    validate_shared_array_buffer_length(arguments)?;
    let prototype = crate::execute::get_property_result(new_target, "prototype")?;
    let value = construct_builtin(builtin, arguments)?;
    let prototype = if crate::value::is_object(&prototype) {
        Some(prototype)
    } else {
        realm_default_prototype(target, new_target)
    };
    Ok(prototype.map_or(value.clone(), |prototype| {
        crate::builtins::set_property(value, "\0prototype", prototype)
    }))
}

fn validate_data_view(value: &Value) -> Result<(), crate::execute::VmError> {
    let Value::DataView(view) = value else {
        return Ok(());
    };
    if *view.buffer.detached.borrow() {
        return Err(type_error("Cannot use a detached ArrayBuffer"));
    }
    if view.is_out_of_bounds() {
        return Err(range_error("Invalid DataView byte length"));
    }
    Ok(())
}

fn validate_shared_array_buffer_length(arguments: &[Value]) -> Result<(), crate::execute::VmError> {
    let length = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let length = to_index(length)?;
    let Some(options) = arguments
        .get(1)
        .filter(|value| crate::value::is_object(value))
    else {
        return Ok(());
    };
    let maximum = crate::execute::get_property_result(options, "maxByteLength")?;
    if matches!(maximum, Value::Undefined) {
        return Ok(());
    }
    if to_index(crate::conversion::to_number(&maximum)?)? < length {
        return Err(range_error("maxByteLength is smaller than byteLength"));
    }
    Ok(())
}
fn with_new_target_prototype(
    value: Value,
    target: &Value,
    new_target: &Value,
) -> Result<Value, crate::execute::VmError> {
    if crate::builtins::same_value(Some(target), Some(new_target)) {
        return Ok(value);
    }
    let prototype = crate::execute::get_property_result(new_target, "prototype")?;
    apply_new_target_prototype(value, target, new_target, prototype)
}

fn apply_new_target_prototype(
    value: Value,
    target: &Value,
    new_target: &Value,
    prototype: Value,
) -> Result<Value, crate::execute::VmError> {
    let prototype = if crate::value::is_object(&prototype) {
        Some(prototype)
    } else {
        realm_default_prototype(target, new_target)
    };
    Ok(prototype.map_or(value.clone(), |prototype| {
        let value = crate::builtins::set_property(value, "\0prototype", prototype);
        drop_shadowed_error_constructor(value)
    }))
}

fn drop_shadowed_error_constructor(mut value: Value) -> Value {
    let Value::Object(data) = &mut value else {
        return value;
    };
    let data = std::rc::Rc::make_mut(data);
    let is_error = data
        .properties
        .iter()
        .any(|(key, _)| key == crate::builtins::ERROR_SLOT);
    if is_error {
        data.properties.retain(|(key, _)| key != "constructor");
    }
    value
}
include!("construct_realm.rs");
fn construct_bound(
    bound: &crate::value::BoundFunctionValue,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    let same = crate::builtins::same_value(Some(target), Some(new_target));
    let new_target = if same {
        bound.target.clone()
    } else {
        new_target.to_owned()
    };
    let value = construct_bound_target(bound, &bound.target, &new_target, &combined)?;
    if let Value::HostCapability(capability) = &bound.receiver {
        let value = crate::builtins::set_property(
            value,
            "\0realm",
            Value::HostCapability(capability.clone()),
        );
        return Ok(crate::builtins::set_property(
            value,
            "\0creation_realm",
            Value::HostCapability(capability.clone()),
        ));
    }
    Ok(value)
}

fn construct_bound_target(
    bound: &crate::value::BoundFunctionValue,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match target {
        Value::Builtin(builtin) => {
            if crate::builtin_meta::constructor_name(*builtin).is_none() {
                return Err(crate::vm::not_callable());
            }
            let value = construct_bound_in_realm(bound, *builtin, arguments)?;
            with_new_target_prototype(value, &Value::Builtin(*builtin), new_target)
        }
        Value::Function(function) => construct_function(function, new_target, arguments),
        Value::BoundFunction(next) => construct_bound(
            next,
            &Value::BoundFunction(std::rc::Rc::clone(next)),
            new_target,
            arguments,
        ),
        _ => Err(crate::vm::not_callable()),
    }
}

fn construct_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if is_error_builtin(builtin) {
        return construct_error(&builtin, arguments);
    }
    if let Some(result) = crate::functions_dynamic::construct_builtin(builtin, arguments) {
        return result;
    }
    if let Some(result) = construct_typed_builtin(builtin, arguments) {
        return result;
    }
    construct_builtin_match(builtin, arguments)
}

include!("construct_builtins.rs");
fn construct_function(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if !crate::functions::is_constructible(function) {
        return Err(crate::vm::not_callable());
    }
    if is_default_derived_constructor(function) {
        let super_constructor = derived_constructor(function)?;
        return construct_with_new_target(&super_constructor, target, arguments);
    }
    if derived_constructor(function).is_ok() {
        let _context = crate::super_scope::Guard::install(function, &Value::Undefined);
        let (result, final_this) =
            crate::functions::execute_construct(function, &Value::Undefined, target, arguments)?;
        if crate::value::is_object(&result) {
            return Ok(result);
        }
        if crate::value::is_object(&final_this) {
            return Ok(final_this);
        }
        return Err(crate::value::error::throw_reference_error(
            "Derived constructor did not initialize this",
        ));
    }
    let object = initialize_instance_fields(function, constructor_receiver(target))?;
    let (result, final_this) =
        crate::functions::execute_construct(function, &object, target, arguments)?;
    if crate::value::is_object(&result) {
        Ok(result)
    } else if crate::value::is_object(&final_this) {
        Ok(final_this)
    } else {
        Ok(object)
    }
}

pub(crate) fn initialize_instance_fields(
    function: &crate::value::FunctionValue,
    receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    initialize_instance_fields_impl(function, receiver)
}
include!("construct_instance_fields.rs");
pub(crate) fn derived_constructor(
    function: &crate::value::FunctionValue,
) -> Result<Value, crate::execute::VmError> {
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
        .ok_or_else(|| crate::value::error::throw_reference_error("super is unavailable"))
}

include!("construct_tail.rs");
