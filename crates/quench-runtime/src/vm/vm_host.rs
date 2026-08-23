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
        return Err(crate::value::error::throw_type_error(&format!(
            "value is not callable [host capability id={} receiver_variant={}]",
            kind_id(kind),
            value_variant(capability_receiver),
        )));
    };
    let descriptor = HostCapabilityRef {
        realm: capability.realm(),
        kind,
    };
    if !host_capability_permitted(descriptor, kind) {
        // Legacy Require handles are frequently retained by a bound function
        // across realm transitions.  The active realm's host is authoritative
        // for Require dispatch; do not reject the handle merely because its
        // original realm differs.
        let legacy_require = matches!(kind, HostCapabilityKind::Custom(1))
            && CURRENT_CONTEXT.with(|context| context.borrow().as_ref().is_some_and(|context| context.host_handle().is_some()));
        if !legacy_require {
            return Err(crate::value::error::throw_type_error(&format!(
                "value is not callable [host capability id={} permission denied]",
                kind_id(kind),
            )));
        }
    }
    let result = dispatch_host_capability(descriptor, kind, capability, receiver, arguments);
    result
}
fn kind_id(kind: HostCapabilityKind) -> u32 {
    match kind {
        HostCapabilityKind::GetGlobal => 0,
        HostCapabilityKind::CreateRealm => 1,
        HostCapabilityKind::EvalScript => 2,
        HostCapabilityKind::DetachArrayBuffer => 3,
        HostCapabilityKind::IsHTMLDDA => 4,
        HostCapabilityKind::Custom(id) => 0x10000 | u32::from(id),
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

fn host_capability_permitted(descriptor: HostCapabilityRef, kind: HostCapabilityKind) -> bool {
    CURRENT_CONTEXT.with(|context| {
        context.borrow().as_ref().is_some_and(|context| {
            context.permits(descriptor)
                || (kind == HostCapabilityKind::EvalScript
                    && realm::context(descriptor.realm).is_some())
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
        HostCapabilityKind::IsHTMLDDA => Ok(Value::Null),
        HostCapabilityKind::EvalScript => run_eval_in_capability_realm(capability, arguments),
        HostCapabilityKind::Custom(_) => {
            let global = matches!(kind, HostCapabilityKind::Custom(1));
            let result = if global {
                let global_value = current_global_object();
                host_custom_call(descriptor, Some(&global_value), arguments)
            } else {
                host_custom_call(descriptor, receiver, arguments)
            };
            return result;
        }
    }
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
    let host = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .and_then(VmContext::host_handle)
    }).or_else(|| crate::vm::realm::context(descriptor.realm).and_then(|context| context.host_handle()));
    host.map(|host| {
        match host.call(descriptor, receiver, arguments) {
            Err(VmError::NotCallable) if descriptor.kind == HostCapabilityKind::Custom(11) => {
                host.construct(descriptor, arguments)
            }
            result => result,
        }
    })
        .unwrap_or_else(|| Err(VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::TypeError,
            &[Value::String(format!(
                "value is not callable [host capability id={} realm={:?} receiver_variant={} host_missing]",
                kind_id(descriptor.kind), descriptor.realm, value_variant(receiver)
            ))],
        ))))
}
/// Invoke a legacy Custom(1) capability directly on the active host.  Bound
/// `require` handles must preserve their concatenated arguments while using
/// the current global object as the JavaScript receiver.
pub(crate) fn execute_legacy_require_direct(
    capability: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Value::HostCapability(capability) = capability else {
        return Err(crate::value::error::throw_type_error("require capability missing"));
    };
    let host = CURRENT_CONTEXT
        .with(|context| context.borrow().as_ref().and_then(VmContext::host_handle))
        .or_else(|| realm::context(capability.realm()).and_then(|context| context.host_handle()))
        .ok_or(VmError::NotCallable)?;
    let global = current_global_object();
    host.call(
        HostCapabilityRef {
            realm: capability.realm(),
            kind: HostCapabilityKind::Custom(1),
        },
        Some(&global),
        arguments,
    )
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
        ("Number", Builtin::Number),
        ("String", Builtin::String),
        ("Boolean", Builtin::Boolean),
        ("Symbol", Builtin::Symbol),
        ("TypeError", Builtin::TypeError),
    ] {
        properties.push((name.to_string(), realm::intrinsic(realm, builtin)?));
    }
    Some(Rc::new(crate::value::ObjectData::new(properties)))
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
        Value::Builtin(crate::ops::Builtin::HostCapability(crate::ops::HostCapabilityKind::EvalScript)),
        &Value::HostCapability(Rc::clone(&token)),
    );
    create_realm_object(global, eval_script, Value::HostCapability(token), creation_realm)
}

fn create_realm_object(
    global: Value,
    eval_script: Value,
    token: Value,
    creation_realm: Value,
) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("global".to_string(), global),
        ("evalScript".to_string(), eval_script),
        ("\0realm".to_string(), token),
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
        context.emit_output(&text);
    }
    Ok(Value::Undefined)
}
