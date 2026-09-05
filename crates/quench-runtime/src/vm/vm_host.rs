pub(crate) fn execute_host_capability(
    kind: HostCapabilityKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    execute_host_capability_with_receiver(kind, receiver, receiver, arguments)
}

pub(crate) fn execute_host_capability_with_receiver(
    kind: HostCapabilityKind,
    capability_receiver: Option<&Value>,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::HostCapability(capability)) = capability_receiver else {
        return Err(VmError::NotCallable);
    };
    let descriptor = HostCapabilityRef {
        realm: capability.realm(),
        kind,
    };
    if !host_capability_permitted(descriptor, kind) {
        return Err(VmError::NotCallable);
    }
    dispatch_host_capability(descriptor, kind, capability, receiver, arguments)
}

fn host_capability_permitted(descriptor: HostCapabilityRef, kind: HostCapabilityKind) -> bool {
    CURRENT_CONTEXT.with(|context| {
        let current = context.borrow();
        current.as_ref().is_some_and(|context| {
            context.permits(descriptor)
                || (kind == HostCapabilityKind::EvalScript
                    && realm::context(descriptor.realm).is_some())
                || (matches!(kind, HostCapabilityKind::Custom(_))
                    && realm::context(descriptor.realm)
                        .is_some_and(|realm| realm.host_handle().is_some()))
        })
    })
}

fn dispatch_host_capability(
    descriptor: HostCapabilityRef,
    kind: HostCapabilityKind,
    capability: &Rc<crate::value::HostCapabilityValue>,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    match kind {
        HostCapabilityKind::GetGlobal if arguments.is_empty() => current_global_value(),
        HostCapabilityKind::GetGlobal => Err(type_error("getGlobal expects no arguments")),
        HostCapabilityKind::CreateRealm if arguments.is_empty() => Ok(create_realm_value()),
        HostCapabilityKind::CreateRealm => Err(type_error("createRealm expects no arguments")),
        HostCapabilityKind::DetachArrayBuffer => vm_ops::detach_array_buffer(arguments),
        HostCapabilityKind::EvalScript => run_eval_in_capability_realm(capability, arguments),
        HostCapabilityKind::Agent => Err(type_error("agent is not callable")),
        HostCapabilityKind::AgentStart => agent_start(arguments),
        HostCapabilityKind::AgentBroadcast => agent_broadcast(arguments),
        HostCapabilityKind::AgentReport => agent_report(arguments),
        HostCapabilityKind::AgentGetReport => Ok(agent_get_report()),
        HostCapabilityKind::AgentLeaving => Ok(Value::Undefined),
        HostCapabilityKind::AgentReceiveBroadcast => agent_receive_broadcast(arguments),
        HostCapabilityKind::AgentSleep
        | HostCapabilityKind::AgentTryYield
        | HostCapabilityKind::AgentTrySleep => agent_sleep(kind, arguments),
        HostCapabilityKind::AgentSetTimeout => agent_set_timeout(arguments),
        HostCapabilityKind::AgentMonotonicNow => Ok(agent_monotonic_now()),
        HostCapabilityKind::IsHTMLDDA => Ok(Value::Null),
        HostCapabilityKind::PromiseHook => Err(VmError::NotCallable),
        HostCapabilityKind::Custom(_) => host_custom_call(descriptor, receiver, arguments),
    }
}

fn agent_start(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(_)) = arguments.first() else {
        return Err(type_error("agent.start expects a string"));
    };
    crate::atomics::begin_agent_callback();
    let result = run_eval_script(arguments);
    crate::atomics::end_agent_callback();
    result
}

fn agent_receive_broadcast(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments
        .first()
        .ok_or_else(|| type_error("agent.receiveBroadcast expects a callback"))?;
    if !crate::conversion::is_callable(callback) {
        return Err(type_error("agent.receiveBroadcast expects a callback"));
    }
    AGENT_CALLBACKS.with(|callbacks| callbacks.borrow_mut().push(callback.clone()));
    Ok(Value::Undefined)
}

fn agent_broadcast(arguments: &[Value]) -> Result<Value, VmError> {
    let buffer = arguments.first().cloned().unwrap_or(Value::Undefined);
    let callbacks = AGENT_CALLBACKS.with(|callbacks| callbacks.borrow().clone());
    for callback in callbacks {
        crate::atomics::begin_agent_callback();
        let result = crate::functions::execute_target(
            &callback,
            &Value::Undefined,
            std::slice::from_ref(&buffer),
        );
        crate::atomics::end_agent_callback();
        result?;
    }
    Ok(Value::Undefined)
}

fn agent_report(arguments: &[Value]) -> Result<Value, VmError> {
    let value = match arguments.first().cloned().unwrap_or(Value::Undefined) {
        Value::Number(value) => Value::String(
            crate::conversion::to_string(&Value::Number(value)).unwrap_or_default(),
        ),
        Value::BigInt(value) => Value::String(value),
        value => value,
    };
    let associates = matches!(&value,
        Value::String(value)
            if value == "ok"
                || value == "timed-out"
                || value == "not-equal"
                || value.ends_with(" ok")
                || value.ends_with(" timed-out")
                || value.ends_with(" not-equal"));
    let index = AGENT_REPORTS.with(|reports| {
        let mut reports = reports.borrow_mut();
        reports.push(value);
        AGENT_REPORT_CONSUMED.with(|consumed| consumed.borrow_mut().push(false));
        reports.len() - 1
    });
    crate::atomics::register_agent_report(index, associates);
    Ok(Value::Undefined)
}

fn agent_get_report() -> Value {
    run_due_agent_timers();
    AGENT_REPORTS.with(|reports| {
        let reports = reports.borrow();
        let consumed = AGENT_REPORT_CONSUMED.with(|consumed| consumed.borrow().clone());
        if let Some(index) = (0..reports.len()).find(|&index| {
            !consumed.get(index).copied().unwrap_or(false)
                && crate::atomics::agent_report_ready(index)
        }) {
            let value = reports[index].clone();
            AGENT_REPORT_CONSUMED.with(|consumed| {
                if let Some(consumed) = consumed.borrow_mut().get_mut(index) {
                    *consumed = true;
                }
            });
            value
        } else {
            Value::Undefined
        }
    })
}

fn agent_sleep(kind: HostCapabilityKind, arguments: &[Value]) -> Result<Value, VmError> {
    let delay = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if delay.is_finite() && delay > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(delay / 1_000.0));
    }
    let _ = kind;
    run_due_agent_timers();
    AGENT_REPORTS.with(|reports| {
        crate::atomics::expire_agent_waiters(&mut reports.borrow_mut());
    });
    Ok(Value::Undefined)
}

fn agent_set_timeout(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments
        .first()
        .cloned()
        .ok_or_else(|| type_error("agent.setTimeout expects a callback"))?;
    if !crate::conversion::is_callable(&callback) {
        return Err(type_error("agent.setTimeout expects a callback"));
    }
    let delay = crate::conversion::to_number(arguments.get(1).unwrap_or(&Value::Undefined))?;
    let delay = if delay.is_finite() && delay > 0.0 {
        delay
    } else {
        0.0
    };
    AGENT_TIMERS.with(|timers| {
        timers.borrow_mut().push(AgentTimer {
            deadline: std::time::Instant::now()
                + std::time::Duration::from_secs_f64(delay / 1_000.0),
            callback,
        });
    });
    Ok(Value::Undefined)
}

fn run_due_agent_timers() {
    let now = std::time::Instant::now();
    let due = AGENT_TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        let mut due = Vec::new();
        let mut pending = Vec::with_capacity(timers.len());
        for timer in timers.drain(..) {
            if timer.deadline <= now {
                due.push(timer.callback);
            } else {
                pending.push(timer);
            }
        }
        *timers = pending;
        due
    });
    for callback in due {
        crate::atomics::begin_agent_callback();
        let _ = crate::functions::execute_target(&callback, &Value::Undefined, &[]);
        crate::atomics::end_agent_callback();
    }
}

fn agent_monotonic_now() -> Value {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let value =
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1_000.0
            + crate::atomics::agent_time_bias();
    Value::Number(value)
}

fn run_eval_in_capability_realm(
    capability: &Rc<crate::value::HostCapabilityValue>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    realm::with_realm(capability.realm(), || run_eval_script(arguments))
        .unwrap_or_else(|| run_eval_script(arguments))
}

fn host_custom_call(
    descriptor: HostCapabilityRef,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let host =
        CURRENT_CONTEXT.with(|context| context.borrow().as_ref().and_then(|rc| rc.host_handle()));
    host.map(|host| host.call(descriptor, receiver, arguments))
        .unwrap_or(Err(VmError::NotCallable))
}

fn run_eval_script(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Err(type_error("evalScript expects a string argument"));
    };
    let Value::String(source) = value else {
        return Err(type_error("evalScript expects a string argument"));
    };
    let realm = CURRENT_CONTEXT
        .with(|context| context.borrow().as_ref().map(|rc| rc.realm()))
        .and_then(|id| realm::context(id).is_some().then_some(id));
    let program = match crate::reduce::reduce_statements::reduce_global_script_source(source) {
        Ok(program) => program,
        Err(errors) => {
            return Err(VmError::Thrown(crate::builtins::error(
                crate::ops::Builtin::SyntaxError,
                &[Value::String(errors.join("; "))],
            )));
        }
    };
    match realm {
        Some(realm) => execute_indirect_eval_in_realm(realm, program.code()),
        None => execute_indirect_eval(program.code()),
    }
}

pub(crate) fn create_shadow_realm_value() -> Value {
    create_realm_value()
}

fn realm_global_object(
    realm: crate::ops::RealmId,
    token: &Rc<crate::value::HostCapabilityValue>,
) -> Option<Rc<crate::value::ObjectData>> {
    let mut properties = vec![
        (
            "\0prototype".to_string(),
            realm::intrinsic(realm, Builtin::ObjectPrototype)?,
        ),
        (
            "\0realm".to_string(),
            Value::HostCapability(Rc::clone(token)),
        ),
    ];
    for (name, builtin) in [
        ("eval", Builtin::Eval),
        ("Object", Builtin::Object),
        ("Function", Builtin::Function),
        ("Iterator", Builtin::Iterator),
        ("DisposableStack", Builtin::DisposableStack),
        ("Number", Builtin::Number),
        ("String", Builtin::String),
        ("RegExp", Builtin::RegExp),
        ("Reflect", Builtin::Reflect),
        ("Boolean", Builtin::Boolean),
        ("Symbol", Builtin::Symbol),
        ("ArrayBuffer", Builtin::ArrayBuffer),
        ("SharedArrayBuffer", Builtin::SharedArrayBuffer),
        ("DataView", Builtin::DataView),
        ("Float64Array", Builtin::Float64Array),
        ("Float32Array", Builtin::Float32Array),
        ("Int8Array", Builtin::Int8Array),
        ("Int16Array", Builtin::Int16Array),
        ("Int32Array", Builtin::Int32Array),
        ("Uint8Array", Builtin::Uint8Array),
        ("Uint8ClampedArray", Builtin::Uint8ClampedArray),
        ("Uint16Array", Builtin::Uint16Array),
        ("Uint32Array", Builtin::Uint32Array),
        ("BigInt64Array", Builtin::BigInt64Array),
        ("BigUint64Array", Builtin::BigUint64Array),
        ("parseFloat", Builtin::ParseFloat),
        ("parseInt", Builtin::ParseInt),
        ("TypeError", Builtin::TypeError),
    ] {
        properties.push((name.to_string(), realm::intrinsic(realm, builtin)?));
    }
    Some(Rc::new(crate::value::ObjectData::new(properties)))
}

fn create_realm_value() -> Value {
    let parent = CURRENT_CONTEXT
        .with(|context| context.borrow().clone())
        .unwrap_or_else(|| Rc::new(VmContext::default()));
    let realm = realm::create(&parent);
    let Some(context) = realm::context(realm) else {
        return Value::Undefined;
    };
    let Some(token) = realm::token(context.realm()) else {
        return Value::Undefined;
    };
    let creation_realm = realm::token(crate::vm::current_context_or_default().realm())
        .map(Value::HostCapability)
        .unwrap_or(Value::Undefined);
    let Some(properties) = realm_global_object(realm, &token) else {
        return Value::Undefined;
    };
    if realm::id_for_token(&token).is_none() || !realm::register_global(&token, properties) {
        return Value::Undefined;
    }
    let global = realm::global(realm).unwrap_or(Value::Undefined);
    let global = match &global {
        Value::Object(object) => {
            let alias =
                crate::value::ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(object))));
            realm::register_global_alias(realm, &alias);
            Value::ObjectAlias(alias)
        }
        _ => global,
    };
    let eval_script = crate::vm::bind_receiver_property(
        Value::Builtin(Builtin::HostCapability(HostCapabilityKind::EvalScript)),
        &Value::HostCapability(Rc::clone(&token)),
    );
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("global".to_string(), global),
        ("evalScript".to_string(), eval_script),
        ("\0realm".to_string(), Value::HostCapability(token)),
        ("\0creation_realm".to_string(), creation_realm),
    ])))
}

fn current_global_value() -> Result<Value, VmError> {
    let global = current_global_object();
    (!matches!(global, Value::Undefined))
        .then_some(global)
        .ok_or_else(|| VmError::EvalError("Global object is unavailable".to_string()))
}

fn execute_print(arguments: &[Value]) -> Result<Value, VmError> {
    let text = arguments
        .iter()
        .map(|value| to_string(Some(value)))
        .collect::<Vec<_>>()
        .join(" ");
    let context = CURRENT_CONTEXT.with(|current| current.borrow().clone());
    if let Some(context) = context {
        // `print` is the shell-style compatibility builtin and owns its
        // record delimiter. Node stream writes remain raw chunks.
        context.emit_output(&format!("{text}\n"));
    }
    Ok(Value::Undefined)
}
thread_local! {
    static AGENT_CALLBACKS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static AGENT_REPORTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static AGENT_REPORT_CONSUMED: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    static AGENT_TIMERS: RefCell<Vec<AgentTimer>> = const { RefCell::new(Vec::new()) };
}

struct AgentTimer {
    deadline: std::time::Instant,
    callback: Value,
}

pub(crate) fn reset_agent_state() {
    AGENT_CALLBACKS.with(|callbacks| callbacks.borrow_mut().clear());
    AGENT_REPORTS.with(|reports| reports.borrow_mut().clear());
    AGENT_REPORT_CONSUMED.with(|consumed| consumed.borrow_mut().clear());
    AGENT_TIMERS.with(|timers| timers.borrow_mut().clear());
    crate::atomics::reset_agent_state();
}
