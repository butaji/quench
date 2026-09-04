//! VM op execution dispatch (call, builtin tail, unary, binary).
use crate::intl::tolocale::parse_num::{parse_float, parse_int};
use crate::ops::HostCapabilityKind;
use crate::value::Value;

use crate::vm::VmError;

thread_local! {
    static CALL_STACK: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static CALL_STACK_SOURCES: std::cell::RefCell<Vec<Option<String>>> = const { std::cell::RefCell::new(Vec::new()) };
    static CALL_CONTINUATION_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct CallContinuationDepthGuard;

impl CallContinuationDepthGuard {
    fn enter() -> Result<Self, VmError> {
        const MAX_CALL_CONTINUATION_DEPTH: usize = 2048;
        let overflow = CALL_CONTINUATION_DEPTH.with(|depth| {
            let current = depth.get();
            if current >= MAX_CALL_CONTINUATION_DEPTH {
                true
            } else {
                depth.set(current + 1);
                false
            }
        });
        if overflow {
            return Err(crate::value::error::throw_range_error(
                "Maximum call stack size exceeded",
            ));
        }
        Ok(Self)
    }
}

impl Drop for CallContinuationDepthGuard {
    fn drop(&mut self) {
        CALL_CONTINUATION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

pub(crate) fn call_stack_frames() -> Vec<String> {
    CALL_STACK.with(|stack| stack.borrow().clone())
}

pub(crate) fn reset_call_stack() {
    CALL_STACK.with(|stack| stack.borrow_mut().clear());
    CALL_STACK_SOURCES.with(|stack| stack.borrow_mut().clear());
}

fn function_source_name(value: &Value) -> Option<String> {
    match crate::execute::get_property(value, "\0quench:source_name") {
        Value::String(name) => Some(name),
        _ => None,
    }
}

pub(crate) fn current_call_stack_source_names() -> Vec<Option<String>> {
    CALL_STACK_SOURCES.with(|stack| stack.borrow().clone())
}

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
    // Specialized function bodies are complete synchronous operations.  Finish
    // them while the caller frame is still live instead of allocating a call
    // continuation and moving its register file out and back for the same
    // result.  A miss falls through to the ordinary continuation path, which
    // preserves all dynamic call/throw/suspend semantics.
    if let Value::Function(function) = &callee_value {
        if let Some(value) = crate::functions::try_execute_specialized(
            function,
            &receiver_value,
            &arguments,
        )? {
            super::write_value(registers, dst, value);
            return Ok(crate::completion::Completion::Normal);
        }
    }
    Ok(take_call_continuation(
        registers,
        dst,
        callee_value,
        receiver_value,
        arguments,
    ))
}

pub(crate) fn take_call_continuation(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    callee: Value,
    receiver: Value,
    arguments: crate::completion::CallArguments,
) -> crate::completion::Completion {
    crate::completion::Completion::Call(crate::completion::CallContinuation {
        callee,
        receiver,
        arguments,
        caller_code: crate::identity::CodeId(0),
        caller_pc: 0,
        caller_registers: std::mem::take(registers),
        caller_environment: crate::identity::EnvironmentRef(0),
        destination,
        guards: crate::completion::ContinuationGuards::default(),
    })
}

fn peel_binding_cell(mut value: Value) -> Value {
    if !matches!(&value, Value::BindingCell(_)) {
        return value;
    }
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
    let _depth_guard = CallContinuationDepthGuard::enter()?;
    stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
        execute_call_continuation_inner(registers, continuation)
    })
}

fn execute_call_continuation_inner(
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
    enum StartedCall {
        Active(ActiveCall),
        Fallback(crate::completion::CallContinuation),
        Error(VmError, crate::completion::CallContinuation),
    }
    fn start(
        continuation: crate::completion::CallContinuation,
    ) -> StartedCall {
        let Value::Function(function) = &continuation.callee else {
            return StartedCall::Fallback(continuation);
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
            return StartedCall::Error(error, continuation);
        }
        // Async and generator functions must go through the ordinary invocation
        // path: it creates the Promise/generator wrapper and performs the
        // corresponding completion setup. Inlining their raw ops would return
        // the body value directly and skip that observable protocol.
        if function.is_async || matches!(function.kind, crate::ops::FunctionKind::Generator) {
            return StartedCall::Fallback(continuation);
        }
        // Functions created inside a `with` scope carry a dynamic object
        // environment. The optimized continuation path has no per-frame
        // scope guard, so use the ordinary invocation path for these
        // closures to restore the captured object lookup semantics.
        if !function.with_captures.is_empty() {
            return StartedCall::Fallback(continuation);
        }
        // The packed continuation has no lexical private-environment guard.
        // Class-scoped functions therefore use the ordinary invocation path,
        // which installs the captured private-name map before executing code.
        if function.private_environment.has_names() {
            return StartedCall::Fallback(continuation);
        }
        if crate::with_scope::is_active() {
            return StartedCall::Fallback(continuation);
        }
        let receiver = crate::vm::bare_call_receiver(function, &continuation.receiver);
        let (callee_registers, environment) =
            crate::functions::build_registers(function, &receiver, &continuation.arguments);
        StartedCall::Active(ActiveCall {
            code: function.code.clone(),
            continuation,
            registers: callee_registers,
            environment,
            pc: 0,
        })
    }
    if let Value::Function(function) = &continuation.callee {
        if let Some(value) = crate::functions::try_execute_specialized(
            function,
            &continuation.receiver,
            &continuation.arguments,
        )? {
            *registers = continuation.caller_registers;
            super::write_value(registers, continuation.destination, value);
            return Ok(());
        }
    }
    let mut stack: Vec<ActiveCall> = Vec::new();
    let mut current = match start(continuation) {
        StartedCall::Active(active) => active,
        StartedCall::Fallback(continuation) => {
            let value = invoke_with_receiver(
                &continuation.callee,
                &continuation.receiver,
                &continuation.arguments,
            )?;
            *registers = continuation.caller_registers;
            super::write_value(registers, continuation.destination, value);
            return Ok(());
        }
        StartedCall::Error(error, continuation) => {
            *registers = continuation.caller_registers;
            return Err(error);
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
                if let crate::execute::VmError::Thrown(value) = error {
                    let thrown = value;
                    loop {
                        let view = current.code.code().ok_or(VmError::MissingReturn)?;
                        if let Some((handler, slot)) = view.catch_at(current.pc) {
                            if let Some(slot) = slot {
                                super::write_value(&mut current.registers, slot, thrown.clone());
                                crate::locals::write(slot, thrown.clone());
                            }
                            current.pc = handler;
                            break;
                        }
                        let Some(parent) = stack.pop() else {
                            return Err(crate::execute::VmError::Thrown(thrown));
                        };
                        current = parent;
                    }
                    continue;
                }
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
        if let crate::completion::Completion::Throw(value) = completion {
            let thrown = value;
            loop {
                let view = current.code.code().ok_or(VmError::MissingReturn)?;
                if let Some((handler, slot)) = view.catch_at(current.pc) {
                    if let Some(slot) = slot {
                        super::write_value(&mut current.registers, slot, thrown.clone());
                        crate::locals::write(slot, thrown.clone());
                    }
                    current.pc = handler;
                    break;
                }
                let Some(parent) = stack.pop() else {
                    decorate_thrown(&thrown, &current, &stack);
                    *registers = current.continuation.caller_registers.clone();
                    return Err(crate::execute::VmError::Thrown(thrown));
                };
                current = parent;
            }
            continue;
        }
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
                current = match start(nested) {
                    StartedCall::Active(active) => active,
                    StartedCall::Fallback(nested) => {
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
                    StartedCall::Error(error, _) => {
                        let parent = stack.pop().expect("caller frame just pushed");
                        *registers = parent.registers;
                        return Err(error);
                    }
                };
                None
            }
            crate::completion::Completion::TailCall(request) => {
                // A reducer may promote the final call in a function body to a
                // tail call.  Treat it as a frame replacement, not as an
                // unconsumed completion: otherwise nested assert.throws sees an
                // internal EvalError instead of the callback's own error value.
                let parent = current.continuation;
                let tail = crate::completion::CallContinuation {
                    callee: request.callee,
                    receiver: request.receiver,
                    arguments: request.arguments,
                    caller_code: parent.caller_code,
                    caller_pc: parent.caller_pc,
                    caller_registers: parent.caller_registers,
                    caller_environment: parent.caller_environment,
                    destination: parent.destination,
                    guards: parent.guards,
                };
                current = match start(tail) {
                    StartedCall::Active(active) => active,
                    StartedCall::Fallback(tail) => {
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
                    StartedCall::Error(error, tail) => {
                        if let Some(parent) = stack.last() {
                            *registers = parent.registers.clone();
                        } else {
                            *registers = tail.caller_registers;
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
) -> Result<crate::completion::CallArguments, VmError> {
    let mut arguments = crate::completion::CallArguments::with_capacity(args.len());
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
    arguments: &mut crate::completion::CallArguments,
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
            let receiver = crate::functions::bound_this_for_call(bound)
                .unwrap_or_else(|| receiver.clone());
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&bound.receiver),
                Some(&receiver),
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
