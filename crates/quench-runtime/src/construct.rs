use crate::{
    facts::ProgramDb,
    ops::Op,
    value::{ObjectData, Value},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
const MAX_HOST_BUFFER_BYTES: usize = 256 * 1024 * 1024;
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
    let result = match target {
        Value::Builtin(builtin) => {
            let prefetched = prefetch_buffer_prototype(*builtin, target, new_target)?;
            let value = construct_builtin(*builtin, arguments)?;
            let value = with_new_target_prototype_cached(value, target, new_target, prefetched)?;
            if let Value::DataView(view) = &value {
                if *view.buffer.detached.borrow() {
                    return Err(type_error("Cannot use a detached ArrayBuffer"));
                }
                if view.is_out_of_bounds() {
                    return Err(range_error("Invalid DataView byte length"));
                }
            }
            Ok(value)
        }
        Value::Function(function) => construct_function(function, new_target, arguments),
        Value::BoundFunction(bound) => construct_bound(bound, target, new_target, arguments),
        _ => Err(crate::vm::not_callable()),
    };
    result
}
fn prefetch_buffer_prototype(
    builtin: crate::ops::Builtin,
    target: &Value,
    new_target: &Value,
) -> Result<Option<Value>, crate::execute::VmError> {
    let buffer = matches!(
        builtin,
        crate::ops::Builtin::ArrayBuffer | crate::ops::Builtin::SharedArrayBuffer
    );
    if buffer && !crate::builtins::same_value(Some(target), Some(new_target)) {
        return crate::execute::get_property_result(new_target, "prototype").map(Some);
    }
    Ok(None)
}
fn with_new_target_prototype(
    value: Value,
    target: &Value,
    new_target: &Value,
) -> Result<Value, crate::execute::VmError> {
    with_new_target_prototype_cached(value, target, new_target, None)
}

fn with_new_target_prototype_cached(
    value: Value,
    target: &Value,
    new_target: &Value,
    prefetched: Option<Value>,
) -> Result<Value, crate::execute::VmError> {
    if crate::builtins::same_value(Some(target), Some(new_target)) {
        return Ok(value);
    }
    let prototype = prefetched
        .map(Ok)
        .unwrap_or_else(|| crate::execute::get_property_result(new_target, "prototype"))?;
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
    let value = match &bound.target {
        Value::Builtin(builtin) => {
            if crate::builtin_meta::constructor_name(*builtin).is_none() {
                return Err(crate::vm::not_callable());
            }
            let value = construct_bound_in_realm(bound, *builtin, &combined)?;
            install_dynamic_global(bound, &value);
            with_new_target_prototype(value, &Value::Builtin(*builtin), &new_target)?
        }
        Value::Function(function) => construct_function(function, &new_target, &combined)?,
        Value::BoundFunction(next) => construct_bound(
            next,
            &Value::BoundFunction(std::rc::Rc::clone(next)),
            &new_target,
            &combined,
        )?,
        _ => return Err(crate::vm::not_callable()),
    };
    Ok(if let Value::HostCapability(capability) = &bound.receiver {
        crate::builtins::set_property(value, "\0realm", Value::HostCapability(capability.clone()))
    } else {
        value
    })
}

fn install_dynamic_global(bound: &crate::value::BoundFunctionValue, value: &Value) {
    let Value::Function(function) = value else {
        return;
    };
    let dynamic = function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0dynamic_function");
    if !dynamic {
        return;
    }
    if let Some(Some(global)) =
        crate::vm::with_realm(bound.realm, || Some(crate::vm::current_global_object()))
    {
        function.captures.set(0, global);
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
    match builtin {
        crate::ops::Builtin::Array => Ok(crate::builtins::array(arguments)),
        crate::ops::Builtin::ArrayBuffer => construct_array_buffer(arguments),
        crate::ops::Builtin::SharedArrayBuffer => construct_shared_array_buffer(arguments),
        crate::ops::Builtin::DataView => construct_data_view(arguments),
        crate::ops::Builtin::Object => Ok(crate::builtins::object(arguments)),
        crate::ops::Builtin::Number => construct_number(arguments),
        crate::ops::Builtin::Boolean => construct_boolean(arguments),
        crate::ops::Builtin::String => construct_string(arguments),
        crate::ops::Builtin::Promise => construct_promise(arguments),
        crate::ops::Builtin::Proxy => crate::proxy::proxy_new(arguments),
        crate::ops::Builtin::Map => crate::collections::map::map_new(arguments),
        crate::ops::Builtin::Set => crate::collections::set::set_new(arguments),
        crate::ops::Builtin::WeakMap => crate::collections::map::weak_map_new(arguments),
        crate::ops::Builtin::WeakSet => crate::collections::set::weak_set_new(arguments),
        crate::ops::Builtin::WeakRef => construct_weak_ref(arguments),
        crate::ops::Builtin::Date => crate::date::execute(builtin, None, arguments)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        crate::ops::Builtin::DisposableStack => crate::disposable_stack::construct(),
        crate::ops::Builtin::AsyncDisposableStack => crate::disposable_stack::construct_async(),
        crate::ops::Builtin::AbstractModuleSource => Err(crate::value::error::throw_type_error(
            "AbstractModuleSource is abstract",
        )),
        crate::ops::Builtin::FinalizationRegistry => {
            crate::finalization_registry::construct(arguments)
        }
        crate::ops::Builtin::RegExp => construct_regexp(arguments),
        _ if crate::intl::is_constructor(builtin) => crate::intl::execute(builtin, arguments, None)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        _ => Err(crate::vm::not_callable()),
    }
}

include!("construct_more.rs");
