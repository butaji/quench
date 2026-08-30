//! VM op execution dispatch (call, builtin tail, unary, binary).
use crate::intl::tolocale::parse_num::{parse_float, parse_int};
use crate::ops::HostCapabilityKind;
use crate::value::Value;

use crate::vm::VmError;

pub fn execute_call(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    callee: u16,
    receiver: Option<u16>,
    args: &[u16],
    spreads: &[bool],
) -> Result<crate::completion::Completion, VmError> {
    let raw_callee = super::read_register(registers, callee)?;
    let arguments = collect_call_arguments(registers, args, spreads)?;
    let callee_value = peel_binding_cell(raw_callee);
    let receiver_value = match receiver {
        Some(receiver) => {
            let value = super::read_register(registers, receiver)?;
            peel_binding_cell(value)
        }
        None => crate::with_scope::receiver_for_callable(&callee_value)
            .map(peel_binding_cell)
            .unwrap_or(Value::Undefined),
    };
    Ok(crate::completion::Completion::Call(
        crate::completion::CallContinuation {
            callee: callee_value,
            receiver: receiver_value,
            arguments,
            caller_code: crate::identity::CodeId(0),
            caller_pc: 0,
            caller_registers: std::mem::take(registers),
            caller_environment: crate::identity::EnvironmentRef(0),
            destination: dst,
            guards: crate::completion::ContinuationGuards::default(),
        },
    ))
}

fn peel_binding_cell(mut value: Value) -> Value {
    let mut seen = std::collections::HashSet::new();
    loop {
        let Value::BindingCell(cell) = value else {
            return value;
        };
        if !seen.insert(std::rc::Rc::as_ptr(&cell)) {
            return Value::BindingCell(cell);
        }
        value = cell.load();
    }
}

pub fn execute_call_continuation(
    registers: &mut crate::register_file::RegisterFile,
    continuation: crate::completion::CallContinuation,
) -> Result<(), VmError> {
    struct ActiveCall {
        continuation: crate::completion::CallContinuation,
        code: crate::machine::FunctionCode,
        registers: crate::register_file::RegisterFile,
        environment: std::rc::Rc<crate::environment::Environment>,
        pc: usize,
    }
    fn start(
        continuation: crate::completion::CallContinuation,
    ) -> Result<Option<ActiveCall>, VmError> {
        let Value::Function(function) = &continuation.callee else {
            return Ok(None);
        };
        if crate::functions::is_class_constructor(function) {
            let error =
                crate::vm::with_realm(crate::construct::function_realm_id(function), || {
                    crate::value::error::throw_type_error(
                        "Class constructor cannot be invoked without 'new'",
                    )
                })
                .unwrap_or_else(|| {
                    crate::value::error::throw_type_error(
                        "Class constructor cannot be invoked without 'new'",
                    )
                });
            return Err(error);
        }
        // Async and generator functions must go through the ordinary invocation
        // path: it creates the Promise/generator wrapper and performs the
        // corresponding completion setup. Inlining their raw ops would return
        // the body value directly and skip that observable protocol.
        if function.is_async || matches!(function.kind, crate::ops::FunctionKind::Generator) {
            return Ok(None);
        }
        // Functions created inside a `with` scope carry a dynamic object
        // environment. The optimized continuation path has no per-frame
        // scope guard, so use the ordinary invocation path for these
        // closures to restore the captured object lookup semantics.
        if !function.with_captures.is_empty() {
            return Ok(None);
        }
        if crate::with_scope::is_active() {
            return Ok(None);
        }
        let receiver = crate::vm::bare_call_receiver(function, &continuation.receiver);
        let (callee_registers, environment) =
            crate::functions::build_registers(function, &receiver, &continuation.arguments);
        Ok(Some(ActiveCall {
            code: function.code.clone(),
            continuation,
            registers: callee_registers,
            environment,
            pc: 0,
        }))
    }
    if let Value::Function(function) = &continuation.callee {
        if crate::functions::is_shape_kernel_candidate(function) {
            let receiver = crate::vm::bare_call_receiver(function, &continuation.receiver);
            if let Some(value) =
                crate::functions::execute_shape_kernel(function, &receiver, &continuation.arguments)
            {
                *registers = continuation.caller_registers;
                super::write_value(registers, continuation.destination, value);
                return Ok(());
            }
        }
        let receiver = crate::vm::bare_call_receiver(function, &continuation.receiver);
        if let Some(result) =
            crate::functions::execute_proven_leaf(function, &receiver, &continuation.arguments)
        {
            let value = result?;
            *registers = continuation.caller_registers;
            super::write_value(registers, continuation.destination, value);
            return Ok(());
        }
    }
    let mut stack: Vec<ActiveCall> = Vec::new();
    let mut current = match start(continuation.clone())? {
        Some(active) => active,
        None => {
            let value = invoke_with_receiver(
                &continuation.callee,
                &continuation.receiver,
                &continuation.arguments,
            )?;
            *registers = continuation.caller_registers;
            super::write_value(registers, continuation.destination, value);
            return Ok(());
        }
    };
    let context = crate::vm::current_context_or_default();
    let value = loop {
        let code = current.code.code().ok_or(VmError::MissingReturn)?;
        let (completion, next) = match crate::vm::execute_code_from(
            code,
            current.pc,
            &mut current.registers,
            &context,
            current.environment.clone(),
        ) {
            Ok(step) => step,
            Err(error) => {
                // Calls move the caller's register file into the continuation.
                // On an abrupt completion, restore the nearest suspended caller
                // before returning so an enclosing try/assert.throws can observe
                // the original thrown value instead of an empty register set.
                if let Some(parent) = stack.last() {
                    *registers = parent.continuation.caller_registers.clone();
                } else {
                    *registers = current.continuation.caller_registers.clone();
                }
                return Err(error);
            }
        };
        current.pc = next;
        let result = match completion {
            crate::completion::Completion::Normal => Some(Value::Undefined),
            crate::completion::Completion::Return(value) => Some(value),
            crate::completion::Completion::Call(mut nested) => {
                // `execute_call` moves the caller frame's registers into the
                // continuation. Restore them on the suspended parent before
                // pushing it, so nested results are written into the live
                // parent frame rather than an empty register vector.
                current.registers = std::mem::take(&mut nested.caller_registers);
                stack.push(current);
                current = match start(nested.clone())? {
                    Some(active) => active,
                    None => {
                        let value = match invoke_with_receiver(
                            &nested.callee,
                            &nested.receiver,
                            &nested.arguments,
                        ) {
                            Ok(value) => value,
                            Err(error) => {
                                // A native nested call can throw before a child frame
                                let parent = stack.pop().expect("caller frame just pushed");
                                *registers = parent.registers;
                                return Err(error);
                            }
                        };
                        let mut parent = stack.pop().expect("caller frame just pushed");
                        super::write_value(&mut parent.registers, nested.destination, value);
                        parent
                    }
                };
                None
            }
            crate::completion::Completion::TailCall(request) => {
                // A reducer may promote the final call in a function body to a
                // tail call.  Treat it as a frame replacement, not as an
                // unconsumed completion: otherwise nested assert.throws sees an
                // internal EvalError instead of the callback's own error value.
                let tail = crate::completion::CallContinuation {
                    callee: request.callee,
                    receiver: request.receiver,
                    arguments: request.arguments,
                    caller_code: current.continuation.caller_code,
                    caller_pc: current.continuation.caller_pc,
                    caller_registers: current.continuation.caller_registers.clone(),
                    caller_environment: current.continuation.caller_environment.clone(),
                    destination: current.continuation.destination,
                    guards: current.continuation.guards,
                };
                current = match start(tail.clone()) {
                    Ok(Some(active)) => active,
                    Ok(None) => {
                        let value = match invoke_with_receiver(
                            &tail.callee,
                            &tail.receiver,
                            &tail.arguments,
                        ) {
                            Ok(value) => value,
                            Err(error) => {
                                if let Some(parent) = stack.last() {
                                    *registers = parent.registers.clone();
                                } else {
                                    *registers = tail.caller_registers.clone();
                                }
                                return Err(error);
                            }
                        };
                        *registers = tail.caller_registers;
                        super::write_value(registers, tail.destination, value);
                        return Ok(());
                    }
                    Err(error) => {
                        if let Some(parent) = stack.last() {
                            *registers = parent.registers.clone();
                        } else {
                            *registers = tail.caller_registers.clone();
                        }
                        return Err(error);
                    }
                };
                continue;
            }
            other => match crate::vm::completion_result(other) {
                Ok(value) => Some(value),
                Err(error) => {
                    if let Some(parent) = stack.last() {
                        *registers = parent.registers.clone();
                    } else {
                        *registers = current.continuation.caller_registers.clone();
                    }
                    return Err(error);
                }
            },
        };
        let Some(value) = result else { continue };
        if let Some(mut parent) = stack.pop() {
            super::write_value(
                &mut parent.registers,
                current.continuation.destination,
                value,
            );
            current = parent;
        } else {
            break value;
        }
    };
    *registers = current.continuation.caller_registers;
    super::write_value(registers, current.continuation.destination, value);
    Ok(())
}
pub fn execute_optional_call(
    registers: &mut crate::register_file::RegisterFile,
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
    registers: &crate::register_file::RegisterFile,
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
    registers: &mut crate::register_file::RegisterFile,
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
pub fn execute_await(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    src: u16,
) -> Result<(), VmError> {
    let value = super::read_register(registers, src)?;
    let value = crate::promise::promise_resolve(std::slice::from_ref(&value));
    match value {
        Value::Promise(promise) => {
            let state = promise.state.borrow().clone();
            match state {
                crate::value::PromiseState::Fulfilled(value) => {
                    super::write_value(registers, dst, value);
                    if crate::module_bindings::fulfilled_await_defers() {
                        crate::module_bindings::mark_await_advanced(true);
                        return Err(VmError::Suspended(promise));
                    }
                    Ok(())
                }
                crate::value::PromiseState::Rejected(reason) => Err(VmError::Thrown(reason)),
                crate::value::PromiseState::Pending => {
                    if crate::module_bindings::fulfilled_await_defers() {
                        crate::module_bindings::mark_await_advanced(false);
                        return Err(VmError::Suspended(promise));
                    }
                    Err(VmError::Suspended(promise))
                }
            }
        }
        value => {
            super::write_value(registers, dst, value);
            Ok(())
        }
    }
}

pub(crate) fn collect_call_arguments(
    registers: &crate::register_file::RegisterFile,
    args: &[u16],
    spreads: &[bool],
) -> Result<Vec<Value>, VmError> {
    let mut arguments = Vec::with_capacity(args.len());
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
        return crate::vm::not_callable();
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
    match callee_value {
        Value::Function(_) => {
            crate::functions::execute_target_with_receiver(callee_value, receiver, arguments)
                .map(|(value, _)| value)
        }
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(crate::ops::Builtin::HostCapability(_))
            ) =>
        {
            let Value::Builtin(crate::ops::Builtin::HostCapability(kind)) = bound.target else {
                unreachable!()
            };
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&bound.receiver),
                Some(receiver),
                &combined,
            )
        }
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, arguments),
        Value::Builtin(builtin) if crate::conversion::is_callable(callee_value) => {
            let receiver = if matches!(receiver, Value::Undefined) {
                crate::super_scope::current_receiver().unwrap_or_else(|| receiver.clone())
            } else {
                receiver.clone()
            };
            super::execute_builtin_with_receiver(*builtin, arguments, Some(&receiver))
        }
        Value::HostCapability(capability) => crate::vm::execute_host_capability_with_receiver(
            capability.descriptor.kind,
            Some(callee_value),
            Some(receiver),
            arguments,
        ),
        Value::Proxy(proxy) if crate::conversion::is_callable(callee_value) => {
            crate::proxy::proxy_apply(callee_value, receiver, arguments)
        }
        _ => Err(super::not_callable()),
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
    Err(VmError::EvalError(format!(
        "unimplemented builtin: {builtin:?}"
    )))
}

fn tail_object_dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    use crate::ops::Builtin;
    let result = match builtin {
        Builtin::ObjectIs => {
            let first = arguments.first().unwrap_or(&Value::Undefined);
            let second = arguments.get(1).unwrap_or(&Value::Undefined);
            Ok(Value::Boolean(crate::builtins::same_value(
                Some(first),
                Some(second),
            )))
        }
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
        Builtin::TypedArray => "TypedArray is not directly constructible",
        Builtin::Float64Array
        | Builtin::Float32Array
        | Builtin::Int8Array
        | Builtin::Int16Array
        | Builtin::Int32Array
        | Builtin::Uint8Array
        | Builtin::Uint8ClampedArray
        | Builtin::Uint16Array
        | Builtin::Uint32Array
        | Builtin::BigInt64Array
        | Builtin::BigUint64Array => "Constructor requires 'new'",
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
