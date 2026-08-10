//! VM op execution dispatch (call, builtin tail, unary, binary).
use crate::intl::tolocale::value::{parse_float, parse_int, to_string};
use crate::value::Value;

use crate::vm::VmError;

pub fn execute_call(
    registers: &mut Vec<Value>,
    dst: u16,
    callee: u16,
    args: &[u16],
    spreads: &[bool],
) -> Result<(), VmError> {
    let arguments = collect_call_arguments(registers, args, spreads)?;
    let callee_value = super::read_register(registers, callee)?;
    let value = invoke_callee(&callee_value, &arguments)?;
    super::write_value(registers, dst, value);
    Ok(())
}

fn collect_call_arguments(
    registers: &[Value],
    args: &[u16],
    spreads: &[bool],
) -> Result<Vec<Value>, VmError> {
    let mut arguments = Vec::new();
    for (i, index) in args.iter().enumerate() {
        push_argument_value(
            &mut arguments,
            super::read_register(registers, *index)?,
            spreads.get(i) == Some(&true),
        );
    }
    Ok(arguments)
}

fn push_argument_value(arguments: &mut Vec<Value>, value: Value, is_spread: bool) {
    if is_spread {
        if let Value::Array(values) = value {
            arguments.extend(values.iter().cloned());
        } else {
            arguments.push(value);
        }
        return;
    }
    arguments.push(value);
}

fn invoke_callee(callee_value: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    match callee_value {
        Value::Function(body) => {
            crate::functions::execute(body, &crate::value::Value::Undefined, arguments)
        }
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, arguments),
        Value::Builtin(builtin) => super::execute_builtin_with_receiver(*builtin, arguments, None),
        Value::Proxy(proxy) => execute_proxy_call(callee_value, proxy, arguments),
        _ => Err(VmError::NotCallable),
    }
}

fn execute_proxy_call(
    callee_value: &Value,
    proxy: &crate::value::ProxyValue,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if *proxy.revoked.borrow() {
        return Err(VmError::EvalError("Cannot call revoked proxy".to_string()));
    }
    let this_arg = arguments.first().cloned().unwrap_or(Value::Undefined);
    crate::proxy::proxy_apply(callee_value, &this_arg, &arguments[1..])
}

pub fn execute_builtin_tail(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    use crate::ops::Builtin;
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    if crate::math::is_builtin(builtin) {
        return crate::math::execute(builtin, arguments);
    }
    if is_proxy_or_reflect(builtin) {
        return crate::proxy::builtin(builtin, arguments);
    }
    Ok(match builtin {
        Builtin::ObjectIs => Value::Boolean(crate::builtins::same_value(
            arguments.first(),
            arguments.get(1),
        )),
        Builtin::ObjectIsExtensible => {
            crate::proxy::proxy_is_extensible(arguments.first().ok_or(VmError::NotCallable)?)?
        }
        Builtin::ObjectKeys => crate::builtins::keys(arguments.first()),
        Builtin::ObjectHasOwnProperty | Builtin::ObjectGetOwnPropertyDescriptor => {
            crate::builtins::object::object_special(builtin, receiver, arguments)
        }
        Builtin::ParseFloat => Value::Number(parse_float(arguments.first())),
        Builtin::ParseInt => Value::Number(parse_int(arguments)),
        Builtin::String => Value::String(to_string(arguments.first())),
        Builtin::Unescape => crate::builtins::unescape(arguments.first()),
        Builtin::MathPow => crate::builtins::math_pow(arguments),
        _ => Value::Undefined,
    })
}

fn early_dispatch(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    crate::strings::execute_builtin(builtin, receiver, arguments)
        .or_else(|| crate::intl::execute(builtin, arguments, receiver))
        .or_else(|| crate::collections::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::regexp::execute_builtin(builtin, receiver, arguments))
}

fn is_proxy_or_reflect(builtin: crate::ops::Builtin) -> bool {
    use crate::ops::Builtin;
    matches!(
        builtin,
        Builtin::Proxy
            | Builtin::ProxyRevocable
            | Builtin::ReflectGet
            | Builtin::ReflectSet
            | Builtin::ReflectHas
            | Builtin::ReflectDeleteProperty
            | Builtin::ReflectGetPrototypeOf
            | Builtin::ReflectSetPrototypeOf
            | Builtin::ReflectIsExtensible
            | Builtin::ReflectPreventExtensions
            | Builtin::ReflectGetOwnPropertyDescriptor
            | Builtin::ReflectDefineProperty
            | Builtin::ReflectOwnKeys
            | Builtin::ReflectApply
            | Builtin::ReflectConstruct
    )
}
