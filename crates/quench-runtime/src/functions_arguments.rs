fn emit_function_expression(
    ops: &mut Vec<Op>,
    next: &mut u16,
    body: Vec<Op>,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
    declared_name: Option<&str>,
) -> u16 {
    let function = emit_function_op(ops, next, body, params, captures, metadata);
    if let Some(name) = declared_name {
        ops.push(Op::SetFunctionName {
            function,
            name: name.to_string(),
        });
        let marker = *next;
        *next = next.saturating_add(1);
        ops.push(Op::Const {
            dst: marker,
            value: crate::ops::Constant::Boolean(true),
        });
        ops.push(Op::SetProperty {
            object: function,
            key: FUNCTION_SELF.to_string(),
            src: marker,
            strict: true,
        });
        ops.push(Op::SetProperty {
            object: function,
            key: crate::functions::FUNCTION_NAME_IMMUTABLE.to_string(),
            src: marker,
            strict: true,
        });
    }
    function
}

pub(crate) fn build_registers(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> (
    Vec<crate::value::Value>,
    std::rc::Rc<crate::environment::Environment>,
) {
    let original_arguments = arguments.to_vec();
    let mut parameters = arguments.to_vec();
    parameters.resize(usize::from(function.params), crate::value::Value::Undefined);
    parameters.truncate(usize::from(function.params));
    parameters.push(crate::value::Value::Undefined);
    parameters.push(this_value.clone());
    let named = function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == FUNCTION_SELF);
    if !matches!(function.kind, FunctionKind::Arrow) {
        parameters.push(crate::value::Value::Undefined);
        if named {
            parameters.push(crate::value::Value::Function(std::rc::Rc::clone(function)));
        }
    }
    let environment = crate::environment::Environment::in_place_child(&function.captures, parameters);
    let arguments_slot = function.captures.len() as u16 + function.params;
    if matches!(function.kind, FunctionKind::Arrow) {
        let arguments = arguments_object(function, original_arguments, &environment);
        environment.set(arguments_slot, arguments);
    } else {
        let arguments = arguments_object(function, original_arguments, &environment);
        environment.set(arguments_slot, arguments);
        mark_arguments_immutable(function, &environment, arguments_slot);
    }
    let register_count = function.ops().len().max(32);
    (vec![crate::value::Value::Undefined; register_count], environment)
}

fn mark_arguments_immutable(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    environment: &std::rc::Rc<crate::environment::Environment>,
    arguments_slot: u16,
) {
    if function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == crate::functions::FUNCTION_NAME_IMMUTABLE)
    {
        environment.mark_immutable_slot(arguments_slot.saturating_add(3));
    }
}

/// Execute a constructor and return its result plus the final `this` value.
pub(crate) fn execute_construct(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    new_target: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let captures = function.captures.len() as u16;
    let (mut registers, environment) = build_registers(function, this_value, arguments);
    let this_slot = captures.saturating_add(function.params).saturating_add(1);
    let new_target_slot = this_slot.saturating_add(1);
    environment.set(new_target_slot, new_target.clone());
    if is_derived_constructor(function) {
        environment.mark_uninitialized(this_slot);
    }
    let _private = crate::private_environment::Guard::install_environment(
        function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(function, this_value);
    let result = crate::vm::execute_in_environment(
        function.ops(),
        &mut registers,
        // Keep the active context so host-provided globals (console,
        // timers, capabilities) stay visible inside constructor bodies.
        &crate::vm::current_context(),
        std::rc::Rc::clone(&environment),
    )?;
    let final_this = environment.get(this_slot);
    Ok((result, final_this))
}

pub(crate) fn is_derived_constructor(function: &crate::value::FunctionValue) -> bool {
    function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0derived_constructor")
}

include!("functions_arguments_setup.rs");

pub(crate) fn execute_bound(
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let crate::value::Value::BindingCell(cell) = &bound.target {
        let mut unwrapped = bound.clone();
        unwrapped.target = cell.borrow().clone();
        return execute_bound(&unwrapped, arguments);
    }
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    match &bound.target {
        crate::value::Value::BindingCell(_) => {
            crate::functions::execute_target(&bound.target, &bound.receiver, &combined)
        }
        crate::value::Value::Builtin(builtin) if crate::conversion::is_callable(&bound.target) => {
            execute_bound_builtin(*builtin, bound, &combined)
        }
        crate::value::Value::HostCapability(capability) => {
            let capability_receiver = match &bound.receiver {
                crate::value::Value::BindingCell(cell) => cell.borrow().clone(),
                value => value.clone(),
            };
            let js_receiver = if matches!(capability.descriptor.kind, crate::ops::HostCapabilityKind::Custom(1)) {
                crate::vm::current_global_object()
            } else {
                capability_receiver.clone()
            };
            crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(&bound.target),
                Some(&js_receiver),
                &combined,
            )
        }
        crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(kind)) => {
            let capability_receiver = match &bound.receiver {
                crate::value::Value::HostCapability(_) => bound.receiver.clone(),
                crate::value::Value::BindingCell(cell) => match &*cell.borrow() {
                    crate::value::Value::HostCapability(capability) => {
                        crate::value::Value::HostCapability(capability.clone())
                    }
                    _ => crate::vm::realm_token(bound.realm)
                        .or_else(|| crate::vm::realm_token(crate::vm::current_context_or_default().realm()))
                        .ok_or(crate::execute::VmError::NotCallable)?,
                },
                _ => crate::vm::realm_token(bound.realm)
                    .or_else(|| crate::vm::realm_token(crate::vm::current_context_or_default().realm()))
                    .ok_or(crate::execute::VmError::NotCallable)?,
            };
            let js_receiver = if matches!(kind, crate::ops::HostCapabilityKind::Custom(1)) {
                crate::vm::current_global_object()
            } else {
                bound.receiver.clone()
            };
            crate::vm::execute_host_capability_with_receiver(
                *kind,
                Some(&capability_receiver),
                Some(&js_receiver),
                &combined,
            )
        }
        crate::value::Value::Function(function) => {
            execute_bound_function(function, bound, &combined)
        }
        crate::value::Value::BoundFunction(next) => execute_bound(next, &combined),
        crate::value::Value::Proxy(_) => {
            crate::proxy::proxy_apply(&bound.target, &bound.receiver, &combined)
        }
        _ => {
            crate::vm::set_not_callable_context(format!(
                "caller=functions_receiver::execute_bound bound_target={:?} bound_receiver={:?}",
                bound.target, bound.receiver
            ));
            Err(crate::execute::VmError::NotCallable)
        }
    }
}

fn execute_bound_function(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let realm = bound
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(key, value)| {
            (key == "\0realm").then(|| match value {
                crate::value::Value::HostCapability(token) => token.realm(),
                _ => crate::ops::RealmId::ROOT,
            })
        });
    let caller = wrapper_caller_realm(bound).unwrap_or(crate::ops::RealmId::ROOT);
    let call_arguments = wrap_shadow_arguments(arguments, realm, caller)?;
    let result = execute_bound_function_in_realm(function, bound, &call_arguments, realm);
    let result = match result {
        Ok(result) => result,
        Err(_) if realm.is_some() => {
            return Err(crate::reflect::shadow_wrapped_exception_error_for_realm(
                wrapper_caller_realm(bound).unwrap_or(crate::ops::RealmId::ROOT),
            ))
        }
        Err(error) => return Err(error),
    };
    finish_bound_result(result, bound, realm)
}

fn finish_bound_result(
    result: crate::value::Value,
    bound: &crate::value::BoundFunctionValue,
    realm: Option<crate::ops::RealmId>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    if realm.is_some() && crate::conversion::is_callable(&result) {
        crate::reflect::wrap_shadow_function_with_caller(
            &result,
            realm,
            wrapper_caller_realm(bound),
        )
    } else if let Some(realm) = realm.filter(|_| crate::value::is_object(&result)) {
        Err(crate::reflect::shadow_wrapped_object_error(
            wrapper_caller_realm(bound).unwrap_or(realm),
        ))
    } else {
        Ok(result)
    }
}

fn execute_bound_function_in_realm(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
    realm: Option<crate::ops::RealmId>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    match realm {
        Some(realm) if realm != crate::ops::RealmId::ROOT => {
            crate::vm::with_realm(realm, || execute(function, &bound.receiver, arguments))
                .unwrap_or_else(|| execute(function, &bound.receiver, arguments))
        }
        _ => execute(function, &bound.receiver, arguments),
    }
}

fn wrapper_caller_realm(bound: &crate::value::BoundFunctionValue) -> Option<crate::ops::RealmId> {
    bound
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(key, value)| {
            (key == "\0caller_realm").then(|| match value {
                crate::value::Value::HostCapability(token) => token.realm(),
                _ => crate::ops::RealmId::ROOT,
            })
        })
}

fn wrap_shadow_arguments(
    arguments: &[crate::value::Value],
    realm: Option<crate::ops::RealmId>,
    caller: crate::ops::RealmId,
) -> Result<Vec<crate::value::Value>, crate::execute::VmError> {
    let Some(realm) = realm else {
        return Ok(arguments.to_vec());
    };
    arguments
        .iter()
        .map(|argument| {
            if crate::conversion::is_callable(argument) {
                crate::reflect::wrap_shadow_function(argument, Some(realm))
            } else if crate::value::is_object(argument) {
                Err(crate::reflect::shadow_wrapped_argument_error_for_realm(
                    caller,
                ))
            } else {
                Ok(argument.clone())
            }
        })
        .collect()
}

fn execute_bound_builtin(
    builtin: crate::ops::Builtin,
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let realm = crate::vm::realm_id_for_intrinsic_receiver(Some(&bound.receiver));
    match realm {
        Some(realm) if realm != crate::ops::RealmId::ROOT => crate::vm::with_realm(realm, || {
            execute_builtin_target(builtin, Some(&bound.receiver), arguments)
        })
        .unwrap_or_else(|| execute_builtin_target(builtin, Some(&bound.receiver), arguments)),
        _ => execute_builtin_target(builtin, Some(&bound.receiver), arguments),
    }
}

pub(crate) fn execute_target(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    fn cap_id(kind: crate::ops::HostCapabilityKind) -> u32 {
        match kind {
            crate::ops::HostCapabilityKind::GetGlobal => 0,
            crate::ops::HostCapabilityKind::CreateRealm => 1,
            crate::ops::HostCapabilityKind::EvalScript => 2,
            crate::ops::HostCapabilityKind::DetachArrayBuffer => 3,
            crate::ops::HostCapabilityKind::IsHTMLDDA => 4,
            crate::ops::HostCapabilityKind::Custom(id) => 1000 + id as u32,
        }
    }
    fn tag(value: &crate::value::Value) -> u32 {
        match value {
            crate::value::Value::Number(_) => 0,
            crate::value::Value::Boolean(_) => 1,
            crate::value::Value::String(_) => 2,
            crate::value::Value::StringUnits(_) => 3,
            crate::value::Value::BigInt(_) => 4,
            crate::value::Value::Array(_) => 5,
            crate::value::Value::Object(_) => 6,
            crate::value::Value::ObjectAlias(_) => 7,
            crate::value::Value::BindingCell(_) => 8,
            crate::value::Value::ArrayBuffer(_) => 9,
            crate::value::Value::Float64Array(_) => 10,
            crate::value::Value::Float32Array(_) => 11,
            crate::value::Value::Int8Array(_) => 12,
            crate::value::Value::Int16Array(_) => 13,
            crate::value::Value::BigInt64Array(_) => 14,
            crate::value::Value::BigUint64Array(_) => 15,
            crate::value::Value::Uint32Array(_) => 16,
            crate::value::Value::Uint8Array(_) => 17,
            crate::value::Value::Uint8ClampedArray(_) => 18,
            crate::value::Value::Uint16Array(_) => 19,
            crate::value::Value::DataView(_) => 20,
            crate::value::Value::Builtin(_) => 21,
            crate::value::Value::Function(_) => 22,
            crate::value::Value::BoundFunction(_) => 23,
            crate::value::Value::HostCapability(_) => 24,
            _ => 25,
        }
    }
    if let crate::value::Value::BindingCell(cell) = target {
        let target = cell.borrow().clone();
        return execute_target(&target, receiver, arguments);
    }
    let mut receiver_value = receiver.clone();
    while let crate::value::Value::BindingCell(cell) = receiver_value {
        receiver_value = cell.borrow().clone();
    }
    let receiver = &receiver_value;
    if !crate::conversion::is_callable(target)
        && !matches!(target, crate::value::Value::BoundFunction(_))
    {
        let cap = match target {
            crate::value::Value::HostCapability(c) => Some(cap_id(c.descriptor.kind)),
            crate::value::Value::BoundFunction(b) => match &b.target {
                crate::value::Value::HostCapability(c) => Some(cap_id(c.descriptor.kind)),
                crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(kind)) => {
                    Some(cap_id(*kind))
                }
                _ => None,
            },
            _ => None,
        };
        return Err(crate::value::error::throw_type_error(&format!(
            "value is not callable [execute_target discriminant={} capability={cap:?}]",
            tag(target),
        )));
    }
    match target {
        crate::value::Value::BindingCell(cell) => {
            let value = cell.borrow().clone();
            execute_target(&value, receiver, arguments)
        }
        crate::value::Value::Builtin(builtin) if crate::conversion::is_callable(target) => {
            execute_builtin_target(*builtin, Some(receiver), arguments)
        }
        crate::value::Value::HostCapability(capability) => {
            crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(target),
                Some(receiver),
                arguments,
            )
        }
        crate::value::Value::Function(function) => {
            execute_in_function_realm(function, receiver, arguments)
        }
        crate::value::Value::BoundFunction(bound)
            if matches!(bound.target, crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(_))) =>
        {
            let crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(kind)) = bound.target else {
                unreachable!()
            };
            let capability_receiver = match &bound.receiver {
                crate::value::Value::HostCapability(capability) => {
                    crate::value::Value::HostCapability(capability.clone())
                }
                crate::value::Value::BindingCell(cell) => cell.borrow().clone(),
                _ => crate::vm::realm_token(bound.realm)
                    .ok_or(crate::execute::VmError::NotCallable)?,
            };
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&capability_receiver),
                Some(receiver),
                &combined,
            )
        }
        crate::value::Value::BoundFunction(bound)
            if matches!(bound.target, crate::value::Value::HostCapability(_)) =>
        {
            let crate::value::Value::HostCapability(capability) = &bound.target else {
                unreachable!()
            };
            let capability_handle = crate::value::Value::HostCapability(capability.clone());
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            if capability.descriptor.kind == crate::ops::HostCapabilityKind::Custom(1) {
                crate::vm::execute_legacy_require_direct(&capability_handle, &combined)
            } else {
                let capability_receiver = match &bound.receiver {
                    crate::value::Value::BindingCell(cell) => cell.borrow().clone(),
                    value => value.clone(),
                };
                crate::vm::execute_host_capability_with_receiver(
                    capability.descriptor.kind,
                    Some(&capability_handle),
                    Some(&capability_receiver),
                    &combined,
                )
            }
        }
        crate::value::Value::BoundFunction(bound)
            if matches!(bound.target, crate::value::Value::Builtin(_)) =>
        {
            let crate::value::Value::Builtin(builtin) = bound.target else {
                unreachable!()
            };
            // A bound intrinsic keeps its captured receiver.  The receiver
            // supplied by the caller is only relevant to an unbound method;
            // using it here made method properties (and host capabilities)
            // fail with "not callable" or an incompatible-receiver error.
            crate::vm::with_realm(bound.realm, || {
                execute_builtin_target(builtin, Some(&bound.receiver), arguments)
            })
            .unwrap_or_else(|| execute_builtin_target(builtin, Some(&bound.receiver), arguments))
        }
        crate::value::Value::BoundFunction(bound) => execute_bound(bound, arguments),
        crate::value::Value::Proxy(_) => crate::proxy::proxy_apply(target, receiver, arguments),
        _ => {
            Err(crate::execute::VmError::NotCallable)
        },
}
}


fn execute_intrinsic_bound(
    bound: &crate::value::BoundFunctionValue,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let crate::value::Value::Builtin(builtin) = bound.target else {
        unreachable!()
    };
    let cap_receiver = if matches!(builtin, crate::ops::Builtin::HostCapability(_)) {
        bound.receiver.clone()
    } else {
        receiver.clone()
    };
    crate::vm::with_realm(bound.realm, || {
        execute_builtin_target(builtin, Some(&cap_receiver), arguments)
    })
    .unwrap_or_else(|| execute_builtin_target(builtin, Some(&cap_receiver), arguments))
}

pub(crate) fn execute_in_function_realm(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let realm = crate::construct::function_realm_id(function);
    match realm {
        realm if realm != crate::ops::RealmId::ROOT => {
            crate::vm::with_realm(realm, || execute(function, receiver, arguments))
                .unwrap_or_else(|| execute(function, receiver, arguments))
        }
        _ => execute(function, receiver, arguments),
    }
}

fn execute_builtin_target(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let Some(result) = crate::regexp::execute_builtin(builtin, receiver, arguments) {
        return result;
    }
    if let crate::ops::Builtin::HostCapability(kind) = builtin {
        let cap = crate::vm::realm_token(crate::vm::current_context_or_default().realm())
            .ok_or(crate::execute::VmError::NotCallable)?;
        if matches!(kind, crate::ops::HostCapabilityKind::Custom(1))
            && matches!(receiver, Some(crate::value::Value::Undefined))
        {
            let global = crate::vm::current_global_object();
            return crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&cap),
                Some(&global),
                arguments,
            );
        }
        return crate::vm::execute_host_capability_with_receiver(kind, Some(&cap), receiver, arguments);
    }
    crate::execute::execute_builtin_with_receiver(builtin, arguments, receiver)
}

fn execute_function_call(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = receiver.ok_or(crate::execute::VmError::NotCallable)?;
    let realm = match receiver {
        crate::value::Value::Function(function) => {
            Some(crate::construct::function_realm_id(function))
        }
        crate::value::Value::BoundFunction(bound) => Some(bound.realm),
        _ => None,
    };
    if let Some(realm) = realm {
        return crate::vm::with_realm(realm, || execute_function_call_in_realm(receiver, arguments))
            .unwrap_or_else(|| execute_function_call_in_realm(receiver, arguments));
    }
    execute_function_call_in_realm(receiver, arguments)
}

fn execute_function_call_in_realm(
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let this = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    if let crate::value::Value::BoundFunction(bound) = receiver {
        if matches!(
            bound.target,
            crate::value::Value::Builtin(
                crate::ops::Builtin::ShadowRealmEvaluate
                    | crate::ops::Builtin::ShadowRealmImportValue,
            )
        ) {
            if !crate::reflect::is_shadow_realm_receiver(Some(&this)) {
                return Err(crate::reflect::shadow_type_error_for_realm(
                    Some(&bound.receiver),
                    "ShadowRealm method called on incompatible receiver",
                ));
            }
            return execute_target(&bound.target, &this, arguments.get(1..).unwrap_or_default());
        }
    }
    // Function.prototype.call invokes static Object helpers with their
    // receiver as the first argument, rather than as a `this` receiver.
    if let crate::value::Value::Builtin(
        crate::ops::Builtin::ObjectDefineProperty
        | crate::ops::Builtin::ObjectGetOwnPropertyDescriptor,
    ) = receiver
    {
        let mut call_arguments = Vec::with_capacity(arguments.len());
        call_arguments.push(this);
        call_arguments.extend_from_slice(arguments.get(1..).unwrap_or_default());
        return execute_target(receiver, &crate::value::Value::Undefined, &call_arguments);
    }
    execute_target(receiver, &this, arguments.get(1..).unwrap_or_default())
}

fn bind_function_target(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if !receiver.is_some_and(crate::conversion::is_callable) {
        return Err(crate::execute::VmError::NotCallable);
    }
    let Some(target) = receiver else {
        return Err(crate::execute::VmError::NotCallable);
    };
    // Host capabilities are already receiver-bound dispatch tokens.  A few
    // CommonJS/module bootstrap paths defensively use
    // `require.bind(undefined)` when publishing the global require; this
    // target is represented either as a host token or as the intrinsic
    // HostCapability builtin.  Preserve it as the bound receiver so the
    // subsequent call retains a valid capability instead of Global.
    let host_require_target = matches!(
        target,
        crate::value::Value::HostCapability(capability)
            if matches!(capability.descriptor.kind, crate::ops::HostCapabilityKind::Custom(1))
    ) || matches!(
        target,
        crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(
            crate::ops::HostCapabilityKind::Custom(1)
        ))
    );
    let bound_target = match arguments.first().cloned() {
        Some(crate::value::Value::Undefined) | None if host_require_target => target.clone(),
        Some(value) => value,
        None => crate::value::Value::Undefined,
    };
    let extra = arguments.get(1..).unwrap_or(&[]).to_vec();
    let name = bound_function_name(target)?;
    let length = bound_function_length(target, extra.len() as f64);
    let mut properties = Vec::new();
    insert_bound_property(
        &mut properties,
        "length",
        crate::value::Value::Number(length),
        length_descriptor(length),
    );
    insert_bound_property(
        &mut properties,
        "name",
        crate::value::Value::String(name.clone()),
        name_descriptor(&name),
    );
    Ok(crate::value::Value::BoundFunction(std::rc::Rc::new(
        crate::value::BoundFunctionValue {
            realm: crate::vm::current_context_or_default().realm(),
            target: target.clone(),
            receiver: bound_target,
            arguments: extra,
            properties: std::cell::RefCell::new(properties),
        },
    )))
}

fn insert_bound_property(
    properties: &mut Vec<(String, crate::value::Value)>,
    key: &str,
    value: crate::value::Value,
    descriptor: crate::value::Value,
) {
    properties.push((key.to_string(), value.clone()));
    properties.push((crate::builtins::descriptor_key(key), descriptor));
}

fn bound_function_name(target: &crate::value::Value) -> Result<String, crate::execute::VmError> {
    let target_name = crate::execute::get_property_result(target, "name")?;
    let value = match target_name {
        crate::value::Value::String(value) if !crate::conversion::is_symbol_string(&value) => value,
        _ => String::new(),
    };
    Ok(format!("bound {value}"))
}

fn bound_function_length(target: &crate::value::Value, bound_args: f64) -> f64 {
    let has_length = crate::builtins::object::has_own_property(
        Some(target),
        Some(&crate::value::Value::String("length".to_string())),
    );
    if !matches!(has_length, crate::value::Value::Boolean(true)) {
        return 0.0;
    }
    let length_value = match crate::execute::get_property_result(target, "length") {
        Ok(crate::value::Value::Number(length)) => length,
        _ => 0.0,
    };
    let value = to_integer_or_infinity(length_value);
    (value - bound_args).max(0.0)
}

include!("functions_arguments_execution.rs");
include!("functions_arguments_helpers.rs");
