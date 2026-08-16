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
            .is_some_and(|context| {
                context.permits(descriptor)
                    || (kind == HostCapabilityKind::EvalScript
                        && realm::context(capability.realm()).is_some())
            })
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
        HostCapabilityKind::EvalScript => realm::with_realm(capability.realm(), || {
            run_eval_script(arguments)
        })
        .ok_or(VmError::NotCallable)?,
    }
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
    let Some(constructor) = realm::intrinsic(realm, Builtin::TypeError) else {
        return Value::Undefined;
    };
    let Some(object_prototype) = realm::intrinsic(realm, Builtin::ObjectPrototype) else {
        return Value::Undefined;
    };
    let properties = Rc::new(crate::value::ObjectData::new(vec![
        ("TypeError".to_string(), constructor),
        ("\0prototype".to_string(), object_prototype),
        ("\0realm".to_string(), Value::HostCapability(Rc::clone(&token))),
    ]));
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
        context.emit_output(&text);
    }
    Ok(Value::Undefined)
}
