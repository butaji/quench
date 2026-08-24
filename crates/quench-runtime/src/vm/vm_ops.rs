//! VM op execution dispatch (call, builtin tail, unary, binary).
use crate::intl::tolocale::parse_num::{parse_float, parse_int};
use crate::ops::HostCapabilityKind;
use crate::value::Value;

use crate::vm::VmError;

pub fn execute_call(
    registers: &mut Vec<Value>,
    dst: u16,
    callee: u16,
    receiver: Option<u16>,
    args: &[u16],
    spreads: &[bool],
) -> Result<(), VmError> {
    let arguments = collect_call_arguments(registers, args, spreads)?;
    let callee_value = super::read_register(registers, callee)?;
    let value = match receiver {
        Some(receiver) => {
            let this_value = super::read_register(registers, receiver)?;
            invoke_with_receiver(&callee_value, &this_value, &arguments)?
        }
        None => match crate::with_scope::receiver_for_callable(&callee_value) {
            Some(this_value) => invoke_with_receiver(&callee_value, &this_value, &arguments)?,
            None => invoke_callee(&callee_value, &arguments)?,
        },
    };
    propagate_object_mutation(registers, &callee_value, args, &arguments, &value);
    super::write_value(registers, dst, value);
    Ok(())
}

pub fn execute_optional_call(
    registers: &mut Vec<Value>,
    dst: u16,
    callee: u16,
    receiver: Option<u16>,
    guard_receiver: bool,
    args: &[u16],
    spreads: &[bool],
) -> Result<(), VmError> {
    let callee_value = super::read_register(registers, callee)?;
    let this_value = receiver
        .map(|index| super::read_register(registers, index))
        .transpose()?;
    let receiver_nullish = guard_receiver
        && this_value
            .as_ref()
            .is_some_and(|value| matches!(value, Value::Null | Value::Undefined));
    let callee_nullish =
        receiver.is_none() && matches!(callee_value, Value::Null | Value::Undefined);
    if receiver_nullish || callee_nullish {
        super::write_value(registers, dst, Value::Undefined);
        return Ok(());
    }
    let arguments = collect_call_arguments(registers, args, spreads)?;
    let value = invoke_with_receiver(
        &callee_value,
        this_value.as_ref().unwrap_or(&Value::Undefined),
        &arguments,
    )?;
    super::write_value(registers, dst, value);
    Ok(())
}

pub fn prepare_tail_call(
    registers: &[Value],
    callee: u16,
    args: &[u16],
    spreads: &[bool],
) -> Result<crate::completion::TailCallRequest, VmError> {
    let arguments = collect_call_arguments(registers, args, spreads)?;
    Ok(crate::completion::TailCallRequest {
        callee: super::read_register(registers, callee)?,
        receiver: Value::Undefined,
        arguments,
    })
}

fn propagate_object_mutation(
    registers: &mut Vec<Value>,
    callee: &Value,
    args: &[u16],
    arguments: &[Value],
    result: &Value,
) {
    if !matches!(
        callee,
        Value::Builtin(crate::ops::Builtin::ObjectDefineProperty)
    ) {
        return;
    }
    let target = arguments.first().unwrap_or(&Value::Undefined);
    crate::properties::propagate_updated_object(registers, args.first().copied(), target, result);
}

/// Resolve an await operand, suspending only for a pending Promise.
pub fn execute_await(registers: &mut Vec<Value>, dst: u16, src: u16) -> Result<(), VmError> {
    let value = super::read_register(registers, src)?;
    let value = crate::promise::promise_resolve(std::slice::from_ref(&value));
    match value {
        Value::Promise(promise) => {
            let state = promise.state.borrow().clone();
            match state {
                crate::value::PromiseState::Fulfilled(value) => {
                    super::write_value(registers, dst, value);
                    Ok(())
                }
                crate::value::PromiseState::Rejected(reason) => Err(VmError::Thrown(reason)),
                crate::value::PromiseState::Pending => {
                    execute_pending_await(registers, dst, promise)
                }
            }
        }
        value => {
            super::write_value(registers, dst, value);
            Ok(())
        }
    }
}

fn execute_pending_await(
    registers: &mut Vec<Value>,
    dst: u16,
    promise: std::rc::Rc<crate::value::PromiseData>,
) -> Result<(), VmError> {
    if crate::module_bindings::fulfilled_await_defers() {
        crate::module_bindings::mark_await_advanced(false);
        return Err(VmError::Suspended(promise));
    }
    crate::promise::drain_microtasks_all();
    let state = promise.state.borrow().clone();
    match state {
        crate::value::PromiseState::Fulfilled(value) => {
            super::write_value(registers, dst, value);
            Ok(())
        }
        crate::value::PromiseState::Rejected(reason) => Err(VmError::Thrown(reason)),
        crate::value::PromiseState::Pending => Err(VmError::Suspended(promise)),
    }
}

pub(crate) fn collect_call_arguments(
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
        )
        .map_err(map_not_callable)?;
    }
    Ok(arguments)
}

fn push_argument_value(
    arguments: &mut Vec<Value>,
    value: Value,
    is_spread: bool,
) -> Result<(), VmError> {
    if is_spread {
        arguments.extend(
            crate::collections::iterator::collect_iterable(value).map_err(map_not_callable)?,
        );
        return Ok(());
    }
    arguments.push(value);
    Ok(())
}

fn map_not_callable(error: crate::execute::VmError) -> crate::execute::VmError {
    if matches!(error, crate::execute::VmError::NotCallable) {
        return VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::TypeError,
            &[Value::String("value is not callable [VM-MAP-NOT-CALLABLE]".to_string())],
        ));
    }
    error
}

fn invoke_callee(callee_value: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    invoke_with_receiver(callee_value, &Value::Undefined, arguments)
}

fn invoke_with_receiver(
    callee_value: &Value,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let receiver_value = if matches!(receiver, Value::Undefined) {
        crate::with_scope::receiver_for_callable(callee_value)
            .unwrap_or_else(|| receiver.clone())
    } else {
        receiver.clone()
    };
    let receiver = &receiver_value;
    // Dispatch host capabilities directly; callback values may be BindingCells,
    // but ordinary callbacks must not be re-entered through their receiver.
    if let Value::BoundFunction(bound) = callee_value {
        if let Value::HostCapability(capability) = &bound.target {
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            if capability.descriptor.kind == crate::ops::HostCapabilityKind::Custom(1) {
                return crate::vm::execute_legacy_require_direct(&bound.target, &combined);
            }
            let receiver = if capability.descriptor.kind == crate::ops::HostCapabilityKind::Custom(1) {
                super::current_global_object()
            } else {
                bound.receiver.clone()
            };
            return crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(&bound.target),
                Some(&receiver),
                &combined,
            );
        }
    }
    if let Value::BoundFunction(bound) = callee_value {
        if matches!(
            bound.target,
            Value::Builtin(crate::ops::Builtin::HostCapability(
                crate::ops::HostCapabilityKind::Custom(1)
            ))
        ) {
            let capability = bound_capability(bound)?;
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            return crate::vm::execute_legacy_require_direct(&capability, &combined);
        }
    }
    if let Value::BoundFunction(bound) = callee_value {
        if let Value::Builtin(crate::ops::Builtin::HostCapability(kind)) = bound.target {
            let capability = bound_capability(bound)?;
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            return crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&capability),
                Some(&bound.receiver),
                &combined,
            );
        }
    }
    let callee_detail = match callee_value {
        Value::BoundFunction(bound) => match &bound.target {
            Value::Builtin(crate::ops::Builtin::HostCapability(kind)) => {
                format!("bound_target=host_capability:{}", host_kind_id(*kind))
            }
            other => format!("bound_target_variant={}", value_variant(Some(other))),
        },
        Value::Builtin(crate::ops::Builtin::HostCapability(kind)) => {
            format!("capability={}", host_kind_id(*kind))
        }
        _ => String::new(),
    };
    crate::vm::set_not_callable_context(format!(
        "caller=vm_ops::invoke_with_receiver callee_variant={} receiver_variant={} {callee_detail}",
        value_variant(Some(callee_value)),
        value_variant(Some(receiver)),
    ));
    match callee_value {
        Value::BindingCell(cell) => {
            let target = cell.borrow().clone();
            invoke_with_receiver(&target, receiver, arguments)
        }
        Value::Builtin(crate::ops::Builtin::HostCapability(kind)) => {
            let Some(capability_receiver) = super::realm_token(super::current_context_or_default().realm()) else {
                return Err(VmError::Thrown(crate::builtins::error(
                    crate::ops::Builtin::TypeError,
                    &[Value::String(format!("value is not callable [host capability id={} missing realm]", host_kind_id(*kind)))],
                )));
            };
            let js_receiver = if matches!(kind, crate::ops::HostCapabilityKind::Custom(1))
                && matches!(receiver, Value::Undefined)
            {
                super::current_global_object()
            } else {
                receiver.clone()
            };
            crate::vm::execute_host_capability_with_receiver(
                *kind,
                Some(&capability_receiver),
                Some(&js_receiver),
                arguments,
            )
        }
        Value::HostCapability(capability) => {
            crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(callee_value),
                Some(receiver),
                arguments,
            )
        }
        Value::BoundFunction(bound)
            if matches!(
                &bound.target,
                Value::HostCapability(capability)
                    if matches!(capability.descriptor.kind, crate::ops::HostCapabilityKind::Custom(1))
            ) =>
        {
            let capability_receiver = super::realm_token(super::current_context_or_default().realm())
                .or_else(|| match &bound.target {
                    Value::HostCapability(capability) => Some(Value::HostCapability(capability.clone())),
                    _ => None,
                })
                .ok_or(crate::execute::VmError::NotCallable)?;
            let js_receiver = super::current_global_object();
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            crate::vm::execute_legacy_require_direct(&bound.target, &combined)
        }
        Value::BoundFunction(bound)
            if matches!(bound.target, Value::HostCapability(_)) =>
        {
            let Value::HostCapability(capability) = &bound.target else {
                unreachable!()
            };
            let capability_receiver = Value::HostCapability(capability.clone());
            let bound_receiver = if matches!(&bound.receiver, Value::Undefined)
                && matches!(capability.descriptor.kind, crate::ops::HostCapabilityKind::Custom(1))
            {
                super::current_global_object()
            } else if matches!(&bound.receiver, Value::Undefined) {
                capability_receiver.clone()
            } else {
                bound.receiver.clone()
            };
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(&capability_receiver),
                Some(&bound_receiver),
                &combined,
            )
        }
        Value::Builtin(crate::ops::Builtin::DateNow) => {
            super::execute_builtin_with_receiver(crate::ops::Builtin::DateNow, arguments, Some(receiver))
        }
        // `require` is exposed as a bound host capability.  Keep this path
        // explicit: the host handle is the current realm token, while an
        // unbound JS receiver is the current realm global object.
        Value::BoundFunction(bound)
            if matches!(
                &bound.target,
                Value::Builtin(crate::ops::Builtin::HostCapability(
                    crate::ops::HostCapabilityKind::Custom(1)
                ))
            ) =>
        {
            let capability_receiver = super::realm_token(bound.realm)
                .or_else(|| super::realm_token(super::current_context_or_default().realm()))
                .ok_or(crate::execute::VmError::NotCallable)?;
            let js_receiver = super::current_global_object();
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            let capability = super::realm_token(bound.realm)
                .ok_or(crate::execute::VmError::NotCallable)?;
            crate::vm::execute_legacy_require_direct(&capability, &combined)
        }
        Value::BoundFunction(bound)
            if matches!(&bound.target, Value::Builtin(crate::ops::Builtin::HostCapability(_))) =>
        {
            let Value::Builtin(crate::ops::Builtin::HostCapability(kind)) = &bound.target else {
                unreachable!()
            };
            let capability_receiver = match &bound.receiver {
                Value::HostCapability(capability) => Value::HostCapability(capability.clone()),
                Value::BindingCell(cell) => cell.borrow().clone(),
                Value::Undefined => super::realm_token(bound.realm)
                    .or_else(|| super::realm_token(super::current_context_or_default().realm()))
                    .ok_or(crate::execute::VmError::NotCallable)?,
                value => value.clone(),
            };
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            let js_receiver = if matches!(kind, crate::ops::HostCapabilityKind::Custom(1)) {
                super::current_global_object()
            } else {
                receiver.clone()
            };
            crate::vm::execute_host_capability_with_receiver(
                *kind,
                Some(&capability_receiver),
                Some(&js_receiver),
                &combined,
            )
        }
        Value::Function(function) => {
            crate::functions::execute_in_function_realm(function, receiver, arguments)
        }
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, arguments),
        Value::Builtin(builtin) if crate::conversion::is_callable(callee_value) => {
            super::execute_builtin_with_receiver(*builtin, arguments, Some(receiver))
        }
        Value::Proxy(proxy) if crate::conversion::is_callable(callee_value) => {
            crate::proxy::proxy_apply(callee_value, receiver, arguments)
        }
        _ => {
            let callee_variant = value_variant(Some(callee_value));
            let receiver_variant = value_variant(Some(receiver));
            let message = format!(
                "value is not callable (callee_variant={callee_variant} receiver_variant={receiver_variant})"
            );
            Err(VmError::Thrown(crate::builtins::error(
                crate::ops::Builtin::TypeError,
                &[Value::String(message)],
            )))
        }
    }
}

fn bound_capability(bound: &crate::value::BoundFunctionValue) -> Result<Value, VmError> {
    match &bound.receiver {
        Value::HostCapability(_) => Ok(bound.receiver.clone()),
        _ => super::realm_token(bound.realm).ok_or(VmError::NotCallable),
    }
}
fn host_kind_id(kind: crate::ops::HostCapabilityKind) -> u32 {
    match kind {
        crate::ops::HostCapabilityKind::GetGlobal => 0,
        crate::ops::HostCapabilityKind::CreateRealm => 1,
        crate::ops::HostCapabilityKind::EvalScript => 2,
        crate::ops::HostCapabilityKind::DetachArrayBuffer => 3,
        crate::ops::HostCapabilityKind::IsHTMLDDA => 4,
        crate::ops::HostCapabilityKind::Custom(id) => 0x10000 | u32::from(id),
    }
}
fn value_variant(value: Option<&Value>) -> u32 {
    match value {
        None => 0,
        Some(Value::Undefined) => 1,
        Some(Value::Null) => 2,
        Some(Value::Boolean(_)) => 3,
        Some(Value::Number(_)) => 4,
        Some(Value::String(_)) => 5,
        Some(Value::HostCapability(_)) => 6,
        Some(Value::Builtin(_)) => 7,
        Some(Value::Function(_)) => 8,
        Some(Value::BoundFunction(_)) => 9,
        Some(Value::Proxy(_)) => 10,
        Some(_) => 11,
    }
}

pub fn execute_builtin_tail(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    execute_builtin_mid(builtin, arguments, receiver)
}

fn execute_builtin_mid(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    use crate::ops::Builtin;
    if crate::math::is_builtin(builtin) {
        return crate::math::execute(builtin, arguments);
    }
    if is_proxy_or_reflect(builtin) {
        return crate::proxy::builtin(builtin, arguments);
    }
    if builtin == Builtin::RegExp {
        return crate::construct::construct_value(&Value::Builtin(Builtin::RegExp), arguments);
    }
    tail_dispatch(builtin, arguments, receiver)
}

fn tail_dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = tail_object_dispatch(builtin, arguments, receiver) {
        return result;
    }
    if let Some(result) = tail_conversion_dispatch(builtin, arguments) {
        return result;
    }
    if let Some(result) = tail_constructor_dispatch(builtin, arguments) {
        return result;
    }
    Ok(Value::Undefined)
}

fn tail_object_dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    use crate::ops::Builtin;
    let result = match builtin {
        Builtin::ObjectIs => Ok(Value::Boolean(crate::builtins::same_value(
            arguments.first(),
            arguments.get(1),
        ))),
        Builtin::ObjectIsExtensible => crate::properties::is_extensible_value(arguments.first()),
        Builtin::ObjectIsFrozen => crate::properties::integrity_level(arguments.first(), true),
        Builtin::ObjectIsSealed => crate::properties::integrity_level(arguments.first(), false),
        Builtin::ObjectFreeze => crate::properties::integrity_apply(arguments.first(), true),
        Builtin::ObjectSeal => crate::properties::integrity_apply(arguments.first(), false),
        Builtin::ObjectPreventExtensions => {
            crate::properties::prevent_extensions(arguments.first())
        }
        Builtin::ObjectGetPrototypeOf => {
            crate::builtins::object::get_prototype_of(arguments.first())
        }
        Builtin::ObjectHasOwnProperty | Builtin::ObjectGetOwnPropertyDescriptor => Ok(
            crate::builtins::object::object_special(builtin, receiver, arguments),
        ),
        _ => return None,
    };
    Some(result)
}

fn tail_conversion_dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use crate::ops::Builtin;
    let result = match builtin {
        Builtin::ParseFloat => parse_float(arguments.first()).map(Value::Number),
        Builtin::ParseInt => parse_int(arguments).map(Value::Number),
        Builtin::String => match arguments.first() {
            Some(value) => crate::conversion::to_string_explicit(value).map(Value::String),
            None => Ok(Value::String(String::new())),
        },
        Builtin::Unescape => crate::builtins::unescape(arguments.first()),
        Builtin::MathPow => crate::builtins::math_pow(arguments),
        _ => return None,
    };
    Some(result)
}

fn tail_constructor_dispatch(
    builtin: crate::ops::Builtin,
    _arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use crate::ops::Builtin;
    let message = match builtin {
        Builtin::ArrayBuffer => "Constructor ArrayBuffer requires 'new'",
        Builtin::DataView => "Constructor DataView requires 'new'",
        Builtin::SharedArrayBuffer => "Constructor SharedArrayBuffer requires 'new'",
        Builtin::WeakRef => "Constructor WeakRef requires 'new'",
        _ => return None,
    };
    Some(Err(crate::value::error::throw_type_error(message)))
}

pub fn execute_host_capability(
    kind: HostCapabilityKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    super::execute_host_capability(kind, receiver, arguments)
}

pub fn detach_array_buffer(arguments: &[Value]) -> Result<Value, VmError> {
    match arguments.first() {
        Some(Value::ArrayBuffer(buffer)) => {
            buffer.detach();
            Ok(Value::Undefined)
        }
        _ => Err(super::type_error(
            "detachArrayBuffer requires an ArrayBuffer",
        )),
    }
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
