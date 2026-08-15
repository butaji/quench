use crate::execute::VmError;
use crate::value::Value;

pub(crate) fn builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    if builtin == crate::ops::Builtin::ShadowRealmImportValue {
        return import_value(arguments, receiver);
    }
    if builtin == crate::ops::Builtin::ShadowRealmEvaluate && !is_shadow_realm_receiver(receiver) {
        return Err(shadow_type_error_for_realm(
            receiver,
            "ShadowRealm.prototype.evaluate called on incompatible receiver",
        ));
    }
    if builtin == crate::ops::Builtin::ShadowRealmEvaluate && !matches!(value, Value::String(_)) {
        return Err(shadow_type_error_for_realm(
            receiver,
            "ShadowRealm.prototype.evaluate requires a string",
        ));
    }
    let realm = crate::vm::realm_id_for_intrinsic_receiver(receiver);
    let direct_syntax_error = matches!(
        value,
        Value::String(source)
            if crate::reduce::reduce_eval_source(source, false, true, false, &[], &[]).is_err()
    );
    let error_realm = shadow_creation_realm(receiver);
    let result = evaluate(value, false, realm, error_realm).map_err(|error| {
        if builtin == crate::ops::Builtin::ShadowRealmEvaluate
            && !direct_syntax_error
            && matches!(error, VmError::Thrown(_))
        {
            shadow_type_error_for_realm(receiver, "ShadowRealm evaluation threw a value")
        } else {
            error
        }
    })?;
    if builtin == crate::ops::Builtin::ShadowRealmEvaluate
        && crate::conversion::is_callable(&result)
    {
        return wrap_shadow_function_with_caller(&result, realm, error_realm);
    }
    if builtin == crate::ops::Builtin::ShadowRealmEvaluate && crate::value::is_object(&result) {
        return Err(shadow_type_error_for_realm(
            receiver,
            "ShadowRealm evaluation must return a primitive",
        ));
    }
    Ok(result)
}

fn import_value(arguments: &[Value], receiver: Option<&Value>) -> Result<Value, VmError> {
    if !is_shadow_realm_receiver(receiver) {
        return Err(shadow_type_error_for_realm(
            receiver,
            "ShadowRealm.prototype.importValue called on incompatible receiver",
        ));
    }
    let specifier = arguments.first().ok_or_else(|| {
        crate::value::error::throw_type_error(
            "ShadowRealm.prototype.importValue requires a specifier",
        )
    })?;
    crate::conversion::to_string(specifier)?;
    if let Some(export_name) = arguments.get(1) {
        if !matches!(export_name, Value::String(_)) {
            return Err(shadow_type_error_for_realm(
                receiver,
                "ShadowRealm.prototype.importValue export name must be a string",
            ));
        }
    }
    Ok(crate::promise::promise_reject(&[Value::Builtin(
        crate::ops::Builtin::TypeError,
    )]))
}

pub(crate) fn wrap_shadow_function(
    target: &Value,
    realm: Option<crate::ops::RealmId>,
) -> Result<Value, VmError> {
    wrap_shadow_function_with_caller(target, realm, None)
}

pub(crate) fn wrap_shadow_function_with_caller(
    target: &Value,
    realm: Option<crate::ops::RealmId>,
    caller: Option<crate::ops::RealmId>,
) -> Result<Value, VmError> {
    let name = match shadow_property(target, "name", realm)? {
        Value::String(value) if !crate::conversion::is_symbol_string(&value) => value,
        _ => String::new(),
    };
    let length = match shadow_property(target, "length", realm)? {
        Value::Number(value) if value.is_finite() => value.max(0.0).trunc(),
        Value::Number(value) if value.is_infinite() && value.is_sign_positive() => value,
        _ => 0.0,
    };
    let mut properties = vec![
        ("name".to_string(), Value::String(name.clone())),
        (
            crate::builtins::descriptor_key("name"),
            name_descriptor(&name),
        ),
        ("length".to_string(), Value::Number(length)),
        (
            crate::builtins::descriptor_key("length"),
            length_descriptor(length),
        ),
    ];
    if let Some(realm) = realm.and_then(crate::vm::realm_token) {
        properties.push(("\0realm".to_string(), realm));
    }
    if let Some(caller) = caller.and_then(crate::vm::realm_token) {
        properties.push(("\0caller_realm".to_string(), caller));
    }
    Ok(Value::BoundFunction(std::rc::Rc::new(
        crate::value::BoundFunctionValue {
            target: target.clone(),
            receiver: Value::Undefined,
            arguments: Vec::new(),
            properties: std::cell::RefCell::new(properties),
        },
    )))
}

fn shadow_property(
    target: &Value,
    key: &str,
    realm: Option<crate::ops::RealmId>,
) -> Result<Value, VmError> {
    let read = || {
        if matches!(target, Value::Proxy(_)) {
            crate::proxy::proxy_get_own_property_descriptor(target, key)
                .and_then(|_| crate::execute::get_property_result(target, key))
        } else {
            crate::execute::get_property_result(target, key)
        }
    };
    let result = realm
        .filter(|realm| *realm != crate::ops::RealmId::ROOT)
        .and_then(|realm| crate::vm::with_realm(realm, read))
        .unwrap_or_else(read);
    result.map_err(|_| shadow_type_error("ShadowRealm wrapped function metadata failed"))
}

fn shadow_type_error(message: &str) -> VmError {
    shadow_type_error_with_constructor(message, Value::Builtin(crate::ops::Builtin::TypeError))
}

pub(crate) fn shadow_type_error_for_realm(receiver: Option<&Value>, message: &str) -> VmError {
    let realm = shadow_creation_realm(receiver)
        .or_else(|| crate::vm::realm_id_for_intrinsic_receiver(receiver));
    let constructor = realm
        .and_then(|realm| {
            crate::vm::with_realm(realm, || {
                crate::vm::realm_intrinsic(crate::ops::Builtin::TypeError)
            })
        })
        .unwrap_or(Value::Builtin(crate::ops::Builtin::TypeError));
    shadow_type_error_with_constructor(message, constructor)
}

fn shadow_creation_realm(receiver: Option<&Value>) -> Option<crate::ops::RealmId> {
    let Some(Value::Object(properties)) = receiver else {
        return None;
    };
    properties.iter().find_map(|(key, value)| {
        (key == "\0creation_realm").then(|| match value {
            Value::HostCapability(token) => crate::vm::realm_id_for_intrinsic_receiver(Some(
                &Value::HostCapability(token.clone()),
            )),
            _ => Some(crate::ops::RealmId::ROOT),
        })?
    })
}

fn shadow_type_error_with_constructor(message: &str, constructor: Value) -> VmError {
    let error = crate::builtins::error(
        crate::ops::Builtin::TypeError,
        &[Value::String(message.to_string())],
    );
    VmError::Thrown(crate::builtins::set_property(
        error,
        "constructor",
        constructor,
    ))
}

pub(crate) fn shadow_wrapped_object_error(realm: crate::ops::RealmId) -> VmError {
    let constructor = crate::vm::with_realm(realm, || {
        crate::vm::realm_intrinsic(crate::ops::Builtin::TypeError)
    })
    .unwrap_or(Value::Builtin(crate::ops::Builtin::TypeError));
    shadow_type_error_with_constructor(
        "ShadowRealm wrapped function must return a primitive",
        constructor,
    )
}

pub(crate) fn shadow_wrapped_argument_error_for_realm(
    realm: crate::ops::RealmId,
) -> VmError {
    let constructor = crate::vm::with_realm(realm, || {
        crate::vm::realm_intrinsic(crate::ops::Builtin::TypeError)
    })
    .unwrap_or(Value::Builtin(crate::ops::Builtin::TypeError));
    shadow_type_error_with_constructor(
        "ShadowRealm wrapped function argument must be primitive or callable",
        constructor,
    )
}

pub(crate) fn shadow_wrapped_exception_error_for_realm(realm: crate::ops::RealmId) -> VmError {
    let constructor = crate::vm::with_realm(realm, || {
        crate::vm::realm_intrinsic(crate::ops::Builtin::TypeError)
    })
    .unwrap_or(Value::Builtin(crate::ops::Builtin::TypeError));
    shadow_type_error_with_constructor(
        "ShadowRealm wrapped function threw an exception",
        constructor,
    )
}

pub(crate) fn is_shadow_realm_receiver(receiver: Option<&Value>) -> bool {
    matches!(
        receiver,
        Some(Value::Object(properties))
            if properties.iter().any(|(key, value)| {
                key == "\0prototype"
                    && *value == Value::Builtin(crate::ops::Builtin::ShadowRealmPrototype)
            })
    )
}

fn name_descriptor(value: &str) -> Value {
    descriptor(Value::String(value.to_string()))
}

fn length_descriptor(value: f64) -> Value {
    descriptor(Value::Number(value))
}

fn descriptor(value: Value) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}

pub(crate) fn execute_eval(
    registers: &mut Vec<Value>,
    input: EvalExecution<'_>,
) -> Result<(), VmError> {
    let source = crate::execute::read_register(registers, input.source)?;
    let value = if input.direct {
        evaluate_direct(
            &source,
            input.strict,
            input.global,
            input.bindings,
            input.reusable_var_names,
            input.forbidden_var_names,
        )?
    } else {
        evaluate(&source, input.strict, None, None)?
    };
    crate::execute::write_value(registers, input.dst, value);
    Ok(())
}

pub(crate) struct EvalExecution<'a> {
    pub(crate) dst: u16,
    pub(crate) source: u16,
    pub(crate) strict: bool,
    pub(crate) global: bool,
    pub(crate) direct: bool,
    pub(crate) bindings: &'a [(String, u16)],
    pub(crate) reusable_var_names: &'a [String],
    pub(crate) forbidden_var_names: &'a [String],
}

fn evaluate(
    value: &Value,
    strict: bool,
    realm: Option<crate::ops::RealmId>,
    error_realm: Option<crate::ops::RealmId>,
) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let bindings = vec![
        ("globalThis".to_string(), 0),
        ("\0script_this".to_string(), 0),
    ];
    let program = crate::reduce::reduce_eval_source(source, strict, true, false, &bindings, &[])
        .map_err(|errors| syntax_error(errors, error_realm))?;
    match realm {
        Some(realm) => crate::vm::execute_indirect_eval_in_realm(realm, program.ops()),
        None => crate::vm::execute_indirect_eval(program.ops()),
    }
}

fn evaluate_direct(
    value: &Value,
    strict: bool,
    global: bool,
    bindings: &[(String, u16)],
    reusable_var_names: &[String],
    forbidden_var_names: &[String],
) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let grammar = crate::semantic::EvalGrammarContext {
        new_target: bindings.iter().any(|(name, _)| name == "\0new_target"),
        super_property: crate::super_scope::is_active(),
    };
    let program = crate::reduce::reduce_statements::reduce_eval_source_in_context(
        source,
        strict,
        global,
        bindings,
        reusable_var_names,
        forbidden_var_names,
        grammar,
    )
    .map_err(|errors| syntax_error(errors, None))?;
    execute_direct_eval(program.ops(), program.facts.strict)
}

fn execute_direct_eval(ops: &[crate::ops::Op], strict: bool) -> Result<Value, VmError> {
    let environment = crate::environment::Environment::child(&crate::locals::current(), Vec::new());
    let _guard = crate::locals::EnvironmentGuard::install(environment);
    let _strict_eval = crate::locals::StrictEvalGuard::install(strict);
    crate::execute::execute_in_place(ops, &mut Vec::new())
}

fn syntax_error(errors: Vec<String>, realm: Option<crate::ops::RealmId>) -> VmError {
    let error = crate::builtins::error(
        crate::ops::Builtin::SyntaxError,
        &[Value::String(errors.join("; "))],
    );
    let constructor = realm
        .and_then(|realm| {
            crate::vm::with_realm(realm, || {
                crate::vm::realm_intrinsic(crate::ops::Builtin::SyntaxError)
            })
        })
        .unwrap_or(Value::Builtin(crate::ops::Builtin::SyntaxError));
    VmError::Thrown(crate::builtins::set_property(
        error,
        "constructor",
        constructor,
    ))
}
