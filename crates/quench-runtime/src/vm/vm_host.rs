use std::sync::OnceLock;
use std::time::Instant;

thread_local! {
    static AGENT_CALLBACKS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static AGENT_REPORTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn execute_host_capability(
    kind: HostCapabilityKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::HostCapability(capability)) = receiver else {
        return Err(VmError::NotCallable);
    };
    let descriptor = HostCapabilityRef {
        realm: capability.realm(),
        kind,
    };
    let permitted = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.permits(descriptor))
    });
    if !permitted {
        return Err(VmError::NotCallable);
    }
    match kind {
        HostCapabilityKind::GetGlobal if arguments.is_empty() => current_global_value(),
        HostCapabilityKind::GetGlobal => Err(type_error("getGlobal expects no arguments")),
        HostCapabilityKind::CreateRealm if arguments.is_empty() => Ok(create_realm_value()),
        HostCapabilityKind::CreateRealm => Err(type_error("createRealm expects no arguments")),
        HostCapabilityKind::DetachArrayBuffer => vm_ops::detach_array_buffer(arguments),
        HostCapabilityKind::EvalScript => run_eval_script(arguments),
        HostCapabilityKind::Agent => Err(type_error("$262.agent is not callable")),
        HostCapabilityKind::AgentStart => agent_start(arguments),
        HostCapabilityKind::AgentBroadcast => agent_broadcast(arguments),
        HostCapabilityKind::AgentReport => agent_report(arguments),
        HostCapabilityKind::AgentGetReport => Ok(agent_get_report()),
        HostCapabilityKind::AgentLeaving => Ok(Value::Undefined),
        HostCapabilityKind::AgentReceiveBroadcast => agent_receive_broadcast(arguments),
        HostCapabilityKind::AgentSleep
        | HostCapabilityKind::AgentTryYield
        | HostCapabilityKind::AgentTrySleep => agent_sleep(arguments),
        HostCapabilityKind::AgentSetTimeout => Ok(Value::Undefined),
        HostCapabilityKind::AgentMonotonicNow => Ok(agent_monotonic_now()),
    }
}

fn agent_sleep(arguments: &[Value]) -> Result<Value, VmError> {
    let delay = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if delay.is_finite() && delay > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(delay / 1_000.0));
    }
    crate::atomics::expire_async_waiters();
    Ok(Value::Undefined)
}

fn agent_monotonic_now() -> Value {
    static START: OnceLock<Instant> = OnceLock::new();
    let elapsed = START.get_or_init(Instant::now).elapsed();
    Value::Number(elapsed.as_secs_f64() * 1_000.0)
}

fn agent_start(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(_)) = arguments.first() else {
        return Err(type_error("$262.agent.start expects a string"));
    };
    run_eval_script(arguments)
}

fn agent_receive_broadcast(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments
        .first()
        .ok_or_else(|| type_error("$262.agent.receiveBroadcast expects a callback"))?;
    if !crate::conversion::is_callable(callback) {
        return Err(type_error("$262.agent.receiveBroadcast expects a callback"));
    }
    AGENT_CALLBACKS.with(|callbacks| callbacks.borrow_mut().push(callback.clone()));
    Ok(Value::Undefined)
}

fn agent_broadcast(arguments: &[Value]) -> Result<Value, VmError> {
    let buffer = arguments.first().cloned().unwrap_or(Value::Undefined);
    let callbacks = AGENT_CALLBACKS.with(|callbacks| callbacks.borrow().clone());
    for callback in callbacks {
        crate::atomics::begin_agent_callback();
        crate::functions::execute_target(&callback, &Value::Undefined, std::slice::from_ref(&buffer))?;
        crate::atomics::end_agent_callback();
    }
    Ok(Value::Undefined)
}

fn agent_report(arguments: &[Value]) -> Result<Value, VmError> {
    let value = match arguments.first().cloned().unwrap_or(Value::Undefined) {
        Value::BigInt(value) => Value::String(value),
        Value::Number(value) => Value::String(crate::conversion::to_string(&Value::Number(value))?),
        value => value,
    };
    crate::atomics::record_agent_report(value);
    AGENT_REPORTS.with(|reports| reports.borrow_mut().push(Value::Undefined));
    Ok(Value::Undefined)
}

fn agent_get_report() -> Value {
    let _ = AGENT_REPORTS.with(|reports| reports.borrow_mut().pop());
    crate::atomics::take_agent_report()
}

fn run_eval_script(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Err(type_error("evalScript expects a string argument"));
    };
    let Value::String(source) = value else {
        return Err(type_error("evalScript expects a string argument"));
    };
    let realm = CURRENT_CONTEXT
        .with(|context| context.borrow().as_ref().map(VmContext::realm))
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
        Some(realm) => execute_indirect_eval_in_realm(realm, program.ops()),
        None => execute_indirect_eval(program.ops()),
    }
}

fn create_realm_value() -> Value {
    let parent = CURRENT_CONTEXT
        .with(|context| context.borrow().clone())
        .unwrap_or_default();
    let realm = realm::create(&parent);
    let Some(context) = realm::context(realm) else {
        return Value::Undefined;
    };
    let Some(token) = realm::token(context.realm()) else {
        return Value::Undefined;
    };
    let Some(constructor) = realm::intrinsic(realm, Builtin::TypeError) else {
        return Value::Undefined;
    };
    let properties = Rc::new(crate::value::ObjectData::new(vec![(
        "TypeError".to_string(),
        constructor,
    )]));
    if realm::id_for_token(&token).is_none() || !realm::register_global(&token, properties) {
        return Value::Undefined;
    }
    let global = realm::global(realm).unwrap_or(Value::Undefined);
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![(
        "global".to_string(),
        global,
    )])))
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
        context.emit_output(&text);
    }
    Ok(Value::Undefined)
}
