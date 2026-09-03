fn emit_function_expression(
    ops: &mut Vec<Op>,
    next: &mut u16,
    body: Vec<Op>,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
    declared_name: Option<&str>,
    source: Option<String>,
) -> u16 {
    let function = emit_function_op(ops, next, body, params, captures, metadata, source);
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
    crate::register_file::RegisterFile,
    std::rc::Rc<crate::environment::Environment>,
) {
    let mut parameters = crate::register_file::RegisterFile::new();
    parameters.reserve(usize::from(function.params).saturating_add(4));
    for index in 0..usize::from(function.params) {
        parameters.push(
            arguments
                .get(index)
                .cloned()
                .unwrap_or(crate::value::Value::Undefined),
        );
    }
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
    let environment =
        crate::environment::Environment::child_registers(&function.captures, parameters);
    let arguments_slot = function.captures.len() as u16 + function.params;
    if function.code.uses_slot(arguments_slot) {
        let arguments = arguments_object(function, arguments.to_vec(), &environment);
        environment.set(arguments_slot, arguments);
        if !matches!(function.kind, FunctionKind::Arrow) {
            mark_arguments_immutable(function, &environment, arguments_slot);
        }
    }
    let register_count = function.code.len().max(32);
    (
        crate::register_file::RegisterFile::with_undefined(register_count),
        environment,
    )
}

pub(crate) fn scope_captures(function: &crate::value::FunctionValue) -> Vec<crate::value::Value> {
    if function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0dynamic_function")
    {
        vec![function.captures.get(0)]
    } else {
        function.with_captures.clone()
    }
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
    if let Some(result) = execute_forwarding_constructor(function, this_value, arguments) {
        return result;
    }
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
    let scope_captures = crate::functions::scope_captures(function);
    let _with_scope = crate::with_scope::FunctionGuard::install(&scope_captures);
    let result = crate::vm::execute_code_in_environment(
        function
            .code
            .code()
            .ok_or(crate::execute::VmError::MissingReturn)?,
        &mut registers,
        // Keep the active context so host-provided globals (console,
        // timers, capabilities) stay visible inside constructor bodies.
        crate::vm::current_context().as_ref(),
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
include!("functions_forwarding_constructor.rs");

pub(crate) fn execute_bound(
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    match &bound.target {
        crate::value::Value::BindingCell(_) => {
            crate::functions::execute_target(&bound.target, &bound.receiver, &combined)
        }
        crate::value::Value::Builtin(builtin) if crate::conversion::is_callable(&bound.target) => {
            execute_bound_builtin(*builtin, bound, &combined)
        }
        crate::value::Value::Function(function) => {
            execute_bound_function(function, bound, &combined)
        }
        crate::value::Value::BoundFunction(next) => execute_bound(next, &combined),
        crate::value::Value::Proxy(_) => {
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
            if let Some(realm) = realm {
                let caller = wrapper_caller_realm(bound).unwrap_or(crate::ops::RealmId::ROOT);
                let call_arguments = wrap_shadow_arguments(&combined, Some(realm), caller)?;
                let result =
                    crate::proxy::proxy_apply(&bound.target, &bound.receiver, &call_arguments)
                        .map_err(|_| {
                            crate::reflect::shadow_wrapped_exception_error_for_realm(caller)
                        })?;
                return finish_bound_result(result, bound, Some(realm));
            }
            crate::proxy::proxy_apply(&bound.target, &bound.receiver, &combined)
        }
        _ => Err(crate::execute::VmError::NotCallable),
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
        Err(_error) if realm.is_some() && !bound_target_is_class(bound) => {
            return Err(crate::reflect::shadow_wrapped_exception_error_for_realm(
                wrapper_caller_realm(bound).unwrap_or(crate::ops::RealmId::ROOT),
            ))
        }
        Err(error) => return Err(error),
    };
    finish_bound_result(result, bound, realm)
}

fn bound_target_is_class(bound: &crate::value::BoundFunctionValue) -> bool {
    matches!(&bound.target, crate::value::Value::Function(function) if crate::functions::is_class_constructor(function))
}

fn finish_bound_result(
    result: crate::value::Value,
    bound: &crate::value::BoundFunctionValue,
    realm: Option<crate::ops::RealmId>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    if realm.is_some() && crate::conversion::is_callable(&result) {
        crate::reflect::wrap_shadow_function_with_caller_mode(
            &result,
            realm,
            wrapper_caller_realm(bound),
            wrapper_caller_realm_explicit(bound),
        )
    } else if let Some(realm) = realm.filter(|_| crate::value::is_object(&result)) {
        let caller = wrapper_caller_realm(bound).unwrap_or(realm);
        if wrapper_caller_realm_explicit(bound) {
            Err(crate::reflect::shadow_wrapped_object_error_for_realm(
                caller,
            ))
        } else {
            Err(crate::reflect::shadow_wrapped_object_error(caller))
        }
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

fn wrapper_caller_realm_explicit(bound: &crate::value::BoundFunctionValue) -> bool {
    bound.properties.borrow().iter().rev().any(|(key, value)| {
        key == "\0caller_realm_explicit" && *value == crate::value::Value::Boolean(true)
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
    // Proxy.revocable's revoke function is a bound intrinsic whose receiver
    // is the proxy itself. Realm probing the receiver would re-enter an
    // active get trap when revoke is called from that trap.
    if builtin == crate::ops::Builtin::ProxyRevoke {
        return crate::proxy::revoke(Some(&bound.receiver));
    }
    let realm = crate::vm::realm_id_for_intrinsic_receiver(Some(&bound.receiver));
    match realm {
        Some(realm) if realm != crate::ops::RealmId::ROOT => crate::vm::with_realm(realm, || {
            execute_builtin_target(builtin, Some(&bound.receiver), arguments)
        })
        .unwrap_or_else(|| execute_builtin_target(builtin, Some(&bound.receiver), arguments)),
        _ => execute_builtin_target(builtin, Some(&bound.receiver), arguments),
    }
}

#[inline(never)]
pub(crate) fn execute_target(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match target {
        crate::value::Value::BindingCell(cell) => {
            let value = cell.load();
            execute_target(&value, receiver, arguments)
        }
        crate::value::Value::Builtin(builtin) if crate::conversion::is_callable(target) => {
            let receiver = if matches!(receiver, crate::value::Value::Undefined) {
                crate::super_scope::current_receiver().unwrap_or_else(|| receiver.clone())
            } else {
                receiver.clone()
            };
            execute_builtin_target(*builtin, Some(&receiver), arguments)
        }
        crate::value::Value::Function(function) => {
            execute_in_function_realm(function, receiver, arguments)
        }
        crate::value::Value::BoundFunction(bound)
            if matches!(
                bound.target,
                crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(_))
            ) =>
        {
            let crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(kind)) =
                bound.target
            else {
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
        crate::value::Value::BoundFunction(bound)
            if matches!(bound.target, crate::value::Value::Builtin(_))
                && !crate::conversion::is_callable(&bound.target) =>
        {
            Err(crate::execute::VmError::NotCallable)
        }
        crate::value::Value::BoundFunction(bound)
            if (crate::vm::is_intrinsic_bound(bound)
                || bound.realm != crate::ops::RealmId::ROOT)
                && matches!(bound.target, crate::value::Value::Builtin(_)) =>
        {
            let crate::value::Value::Builtin(builtin) = bound.target else {
                unreachable!()
            };
            // A bound intrinsic keeps its captured receiver.  The receiver
            // supplied by the caller is only relevant to an unbound method;
            // using it here made method properties (and host capabilities)
            // fail with "not callable" or an incompatible-receiver error.
            let receiver = if crate::builtins::builtin_name(builtin).starts_with("get ")
                || matches!(
                    builtin,
                    crate::ops::Builtin::StringToString | crate::ops::Builtin::StringValueOf
                ) {
                receiver
            } else {
                &bound.receiver
            };
            crate::vm::with_realm(bound.realm, || {
                execute_builtin_target(builtin, Some(receiver), arguments)
            })
            .unwrap_or_else(|| execute_builtin_target(builtin, Some(&bound.receiver), arguments))
        }
        crate::value::Value::BoundFunction(bound) => execute_bound(bound, arguments),
        crate::value::Value::Proxy(_) => crate::proxy::proxy_apply(target, receiver, arguments),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn execute_in_function_realm(
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
    if crate::builtins::object::is_intrinsic_prototype(builtin) {
        return Err(crate::execute::VmError::NotCallable);
    }
    if let crate::ops::Builtin::HostCapability(kind) = builtin {
        return crate::vm::execute_host_capability(kind, receiver, arguments);
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
            let realm = crate::construct::function_realm_id(function);
            Some(realm)
        }
        crate::value::Value::BoundFunction(bound) => Some(bound.realm),
        _ => None,
    };
    if let Some(realm) = realm {
        return crate::vm::with_realm(realm, || {
            execute_function_call_in_realm(receiver, arguments)
        })
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
                crate::ops::Builtin::ErrorPrototypeNameGetter
                    | crate::ops::Builtin::ErrorPrototypeMessageGetter
                    | crate::ops::Builtin::ErrorPrototypeCauseGetter
                    | crate::ops::Builtin::ErrorPrototypeStackGetter
                    | crate::ops::Builtin::ErrorPrototypeStackSetter
            )
        ) {
            return execute_target(&bound.target, &this, arguments.get(1..).unwrap_or_default());
        }
        let receiver_bound = bound.properties.borrow().iter().any(|(key, value)| {
            key == "\0receiver_bound_method" && *value == crate::value::Value::Boolean(true)
        });
        if receiver_bound {
            return execute_target(&bound.target, &this, arguments.get(1..).unwrap_or_default());
        }
    }
    if let crate::value::Value::BoundFunction(bound) = receiver {
        if let crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(kind)) =
            bound.target
        {
            let capability = match &bound.receiver {
                crate::value::Value::HostCapability(capability) => {
                    crate::value::Value::HostCapability(capability.clone())
                }
                _ => crate::vm::realm_token(bound.realm)
                    .ok_or(crate::execute::VmError::NotCallable)?,
            };
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments.get(1..).unwrap_or_default());
            return crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&capability),
                Some(&this),
                &combined,
            );
        }
    }
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
    let requested_receiver = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    let mut extra = arguments.get(1..).unwrap_or(&[]).to_vec();
    let (target, bound_target) = match target {
        crate::value::Value::BoundFunction(bound)
            if matches!(
                bound.target,
                crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(_))
            ) =>
        {
            let call_target = bound.target.clone();
            let bound_receiver = bound.receiver.clone();
            extra.splice(0..0, bound.arguments.clone());
            (call_target, bound_receiver)
        }
        target => (target.clone(), requested_receiver),
    };
    let name = bound_function_name(&target)?;
    let length = bound_function_length(&target, extra.len() as f64);
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
            target,
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
