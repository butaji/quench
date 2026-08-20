include!("vm_generator_step.rs");
include!("vm_completion_step.rs");

fn run_ops(ops: &[Op], registers: &mut Vec<Value>, context: &VmContext) -> Result<Value, VmError> {
    completion_result(run_ops_completion(ops, registers, context)?)
}

fn run_ops_completion(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    Ok(run_ops_completion_step(ops, registers, context)?.completion)
}

pub(crate) fn execute_ops_from(
    ops: &[Op],
    start: usize,
    registers: &mut Vec<Value>,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_ops_completion_step_from(ops, start, registers, context)?;
    Ok((step.completion, step.next))
}

fn run_ops_completion_step(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    run_ops_completion_step_from(ops, 0, registers, context)
}

fn run_ops_completion_step_from(
    ops: &[Op],
    start: usize,
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    for (index, op) in ops.iter().enumerate().skip(start) {
        let result = match run_op(registers, op, context) {
            Ok(result) => result,
            Err(error) => {
                crate::vm::flush_global_declaration_batch(registers);
                return error_completion(error).map(|completion| CompletionStep {
                    completion,
                    next: index + 1,
                });
            }
        };
        match result {
            None | Some(crate::completion::Completion::Normal) => {}
            Some(completion) => {
                crate::vm::flush_global_declaration_batch(registers);
                return Ok(CompletionStep {
                    completion,
                    next: index + 1,
                });
            }
        }
    }
    crate::vm::flush_global_declaration_batch(registers);
    Ok(CompletionStep {
        completion: crate::completion::Completion::Normal,
        next: ops.len(),
    })
}

fn error_completion(error: VmError) -> Result<crate::completion::Completion, VmError> {
    crate::completion::Completion::from_vm_error(error)
}

pub(crate) fn completion_result(
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    completion.into_vm_error()
}

struct GlobalObjectGuard {
    previous: Option<ObjectProperties>,
    restore: bool,
    realm: Option<RealmId>,
}
include!("vm_global.rs");

pub(crate) fn bare_call_receiver(
    function: &crate::value::FunctionValue,
    this_value: &Value,
) -> Value {
    if matches!(function.kind, FunctionKind::Ordinary)
        && matches!(function.strictness, FunctionStrictness::Sloppy)
    {
        let realm = function
            .properties
            .borrow()
            .iter()
            .find_map(|(key, value)| {
                (key == "\0realm")
                    .then(|| crate::vm::realm_id_for_intrinsic_receiver(Some(value)))
                    .flatten()
            })
            .or_else(|| crate::vm::realm_id_for_global_value(&function.captures.get(0)));
        let global = realm
            .and_then(|realm| crate::vm::with_realm(realm, || Some(crate::vm::current_global_object())))
            .flatten()
            .unwrap_or_else(|| function.captures.get(0));
        return to_object_value_in_realm(this_value, &global);
    }
    this_value.clone()
}

fn to_object_value_in_realm(this_value: &Value, global: &Value) -> Value {
    let Some(realm) = crate::vm::realm_id_for_global_value(global) else {
        return to_object_value(this_value);
    };
    crate::vm::with_realm(realm, || to_object_value(this_value))
        .unwrap_or_else(|| to_object_value(this_value))
}

fn to_object_value(this_value: &Value) -> Value {
    match this_value {
        Value::Object(_)
        | Value::Array(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Builtin(_)
        | Value::ObjectAlias(_)
        | Value::Proxy(_)
        | Value::Promise(_)
        | Value::Map(_)
        | Value::Set(_)
        | Value::ArrayBuffer(_)
        | Value::DataView(_)
        | Value::Float32Array(_)
        | Value::Float64Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_)
        | Value::Uint32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Iterator(_)
        | Value::Generator(_)
        | Value::HostCapability(_) => this_value.clone(),
        Value::Number(_) => boxed_primitive(this_value, crate::ops::Builtin::Number),
        Value::Boolean(_) => boxed_primitive(this_value, crate::ops::Builtin::Boolean),
        Value::String(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::StringUnits(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::BigInt(_) => boxed_primitive(this_value, crate::ops::Builtin::BigInt),
        Value::Null | Value::Undefined | Value::BindingCell(_) => this_value.clone(),
    }
}

fn boxed_primitive(value: &Value, constructor: crate::ops::Builtin) -> Value {
    let mut properties = vec![("_value".to_string(), value.clone())];
    if constructor != Builtin::Number {
        properties.push(("constructor".to_string(), Value::Builtin(constructor)));
    }
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
}

pub fn execute_builtin_with_receiver(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = stateful_builtin(builtin, receiver, arguments) {
        return result;
    }
    if builtin == Builtin::Print {
        return execute_print(arguments);
    }
    if is_object_special(builtin) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if let Some(result) = define_builtin(builtin, arguments) {
        return result;
    }
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    if is_data_view_builtin(builtin) {
        return execute_data_view_builtin(builtin, receiver, arguments);
    }
    if is_shared_array_buffer_builtin(builtin) {
        return execute_shared_array_buffer_builtin(builtin, receiver, arguments);
    }
    if let Builtin::HostCapability(kind) = builtin {
        return vm_ops::execute_host_capability(kind, receiver, arguments);
    }
    match builtin {
        _ if is_function_builtin(builtin) => {
            crate::functions::function_builtin(builtin, receiver, arguments)
        }
        _ if is_simple_builtin(builtin) => execute_simple_builtin(builtin, arguments, receiver),
        _ => vm_ops::execute_builtin_tail(builtin, arguments, receiver),
    }
}

fn is_shared_array_buffer_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ArrayBufferByteLengthGetter
            | Builtin::ArrayBufferDetachedGetter
            | Builtin::ArrayBufferImmutableGetter
            | Builtin::ArrayBufferMaxByteLengthGetter
            | Builtin::ArrayBufferResizableGetter
            | Builtin::SharedArrayBufferByteLengthGetter
            | Builtin::SharedArrayBufferGrow
            | Builtin::ArrayBufferSlice
            | Builtin::SharedArrayBufferSlice
            | Builtin::SharedArrayBufferGrowableGetter
            | Builtin::SharedArrayBufferMaxByteLengthGetter
    )
}

fn define_builtin(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::ObjectDefineProperty => Some(crate::builtins::define_property(arguments)),
        Builtin::ObjectDefineProperties => Some(crate::builtins::define_properties(arguments)),
        _ => None,
    }
}

fn stateful_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::GeneratorNext => Some(crate::generator::next(receiver, arguments)),
        Builtin::AsyncGeneratorNext => Some(crate::generator::async_next(receiver, arguments)),
        Builtin::GeneratorReturn => Some(crate::generator::return_(receiver, arguments)),
        Builtin::AsyncGeneratorReturn => Some(crate::generator::async_return(receiver, arguments)),
        Builtin::GeneratorThrow => Some(crate::generator::throw(receiver, arguments)),
        Builtin::AsyncGeneratorThrow => Some(crate::generator::async_throw(receiver, arguments)),
        Builtin::AsyncIteratorDispose => Some(crate::generator::async_dispose(receiver)),
        Builtin::AsyncIteratorDisposeFulfilled => Some(Ok(Value::Undefined)),
        Builtin::ProxyRevoke => Some(crate::proxy::revoke(receiver)),
        Builtin::Math => Some(Err(not_callable())),
        builtin @ (Builtin::AtomicsAdd
        | Builtin::AtomicsAnd
        | Builtin::AtomicsOr
        | Builtin::AtomicsSub
        | Builtin::AtomicsXor
        | Builtin::AtomicsCompareExchange) => {
            Some(crate::atomics::execute(builtin, receiver, arguments))
        }
        Builtin::AtomicsIsLockFree => Some(crate::atomics::is_lock_free(arguments)),
        Builtin::AtomicsNotify => Some(crate::atomics::notify(arguments)),
        Builtin::AtomicsWait => Some(crate::atomics::wait(arguments)),
        Builtin::AtomicsLoad | Builtin::AtomicsStore => {
            Some(crate::atomics::load_store(builtin, arguments))
        }
        Builtin::AtomicsExchange => Some(crate::atomics::exchange(arguments)),
        Builtin::AtomicsWaitAsync => Some(crate::atomics::wait_async(arguments)),
        Builtin::AtomicsPause => Some(Ok(Value::Undefined)),
        _ => None,
    }
}

fn is_object_special(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ObjectHasOwnProperty
            | Builtin::ObjectHasOwn
            | Builtin::ObjectGetOwnPropertyDescriptor
            | Builtin::ObjectGetOwnPropertyDescriptors
            | Builtin::ObjectGetOwnPropertyNames
            | Builtin::ObjectGetOwnPropertySymbols
            | Builtin::ObjectKeys
            | Builtin::ObjectValues
            | Builtin::ObjectEntries
            | Builtin::ObjectAssign
            | Builtin::ObjectFromEntries
            | Builtin::ObjectGroupBy
            | Builtin::ObjectCreate
            | Builtin::ObjectSetPrototypeOf
            | Builtin::ObjectPropertyIsEnumerable
            | Builtin::ObjectPrototypeIsPrototypeOf
            | Builtin::ObjectPrototypeDefineGetter
            | Builtin::ObjectPrototypeDefineSetter
            | Builtin::ObjectPrototypeLookupGetter
            | Builtin::ObjectPrototypeLookupSetter
    )
}

include!("vm_host.rs");
include!("vm_boolean_value.rs");
include!("vm_builtins.rs");
include!("vm_properties.rs");
include!("vm_dispatch.rs");
