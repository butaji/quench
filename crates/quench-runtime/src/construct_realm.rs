/// Peel `BindingCell` so construct and Get see the same identity.
pub(crate) fn peel_construct_value(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => peel_construct_value(&cell.borrow()),
        value => value.clone(),
    }
}

/// BoundFunction [[Construct]]: if newTarget is the bound function, use [[BoundTargetFunction]].
pub(crate) fn bound_construct_new_target(this: &Value, new_target: &Value) -> Value {
    let this = peel_construct_value(this);
    let new_target = peel_construct_value(new_target);
    let Value::BoundFunction(bound) = &this else {
        return new_target;
    };
    if crate::builtins::same_value(Some(&this), Some(&new_target)) {
        peel_construct_value(&bound.target)
    } else {
        new_target
    }
}

/// GetPrototypeFromConstructor: Get(newTarget, "prototype"), else realm default.
pub(crate) fn get_prototype_from_constructor(
    constructor: &Value,
    default: impl FnOnce(crate::ops::RealmId) -> Value,
) -> Value {
    let constructor = peel_construct_value(constructor);
    let proto = crate::execute::get_property_result(&constructor, "prototype")
        .unwrap_or(Value::Undefined);
    if crate::value::is_object(&proto) {
        return proto;
    }
    default(constructor_realm(&constructor))
}

fn constructor_realm(constructor: &Value) -> crate::ops::RealmId {
    fn value_realm(value: &Value) -> Option<crate::ops::RealmId> {
        match value {
            Value::Function(function) => Some(function_realm_id(function)),
            Value::Proxy(proxy) => value_realm(&proxy.target),
            Value::BoundFunction(bound) => bound
                .properties
                .borrow()
                .iter()
                .rev()
                .find_map(|(key, value)| {
                    (key == "\0realm").then(|| match value {
                        Value::HostCapability(token) => Some(token.realm()),
                        Value::Number(number) => Some(crate::ops::RealmId::new(*number as u64)),
                        _ => None,
                    })?
                })
                .or_else(|| match &bound.receiver {
                    Value::HostCapability(capability) => Some(capability.realm()),
                    receiver => value_realm(receiver),
                })
                .or_else(|| value_realm(&bound.target)),
            _ => None,
        }
    }
    value_realm(constructor).unwrap_or(crate::ops::RealmId::ROOT)
}

pub(crate) fn function_realm_id(
    function: &crate::value::FunctionValue,
) -> crate::ops::RealmId {
    fn global_realm(value: &Value) -> Option<crate::ops::RealmId> {
        match value {
            Value::BindingCell(cell) => global_realm(&cell.borrow()),
            Value::ObjectAlias(alias) => alias
                .0
                .borrow()
                .upgrade()
                .and_then(|object| crate::vm::realm_id_for_global_value(&Value::Object(object))),
            value => crate::vm::realm_id_for_global_value(value),
        }
    }
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(key, value)| {
            (key == "\0realm").then(|| match value {
                Value::HostCapability(token) => Some(token.realm()),
                Value::Number(number) => Some(crate::ops::RealmId::new(*number as u64)),
                _ => None,
            })?
        })
        .or_else(|| global_realm(&function.captures.get(0)))
        .unwrap_or(crate::ops::RealmId::ROOT)
}

pub(crate) fn generator_kind_prototype(
    function: &crate::value::FunctionValue,
    realm: crate::ops::RealmId,
) -> Value {
    if function.is_async {
        crate::builtins::async_generator_prototype_in(realm)
    } else {
        crate::builtins::generator_prototype_in(realm)
    }
}

fn realm_default_prototype(target: &Value, new_target: &Value) -> Option<Value> {
    if let Value::Function(function) = new_target {
        if let Value::Builtin(builtin) = target {
            if let Some(prototype) = crate::builtin_meta::instance_prototype(*builtin) {
                return Some(crate::vm::realm_intrinsic_for(
                    function_realm_id(function),
                    prototype,
                ));
            }
        }
        let global = function.captures.get(0);
        if let Some(prototype) = default_prototype_from_constructor(target, &global) {
            return Some(prototype);
        }
        if !matches!(target, Value::Builtin(_)) {
            return object_default_prototype(&global);
        }
    }
    if let Value::BoundFunction(bound) = new_target {
        if let Value::Function(function) = &bound.target {
            if let Value::Builtin(builtin) = target {
                if let Some(prototype) = crate::builtin_meta::instance_prototype(*builtin) {
                    return Some(crate::vm::realm_intrinsic_for(
                        function_realm_id(function),
                        prototype,
                    ));
                }
            }
            let global = function.captures.get(0);
            if let Some(prototype) = default_prototype_from_constructor(target, &global) {
                return Some(prototype);
            }
            if !matches!(target, Value::Builtin(_)) {
                return object_default_prototype(&global);
            }
        }
        if let Value::HostCapability(capability) = &bound.receiver {
            if let Value::Builtin(builtin) = target {
                if let Some(prototype) = crate::builtin_meta::instance_prototype(*builtin) {
                    return Some(crate::vm::realm_intrinsic_for(capability.realm(), prototype));
                }
            }
        }
    }
    if let Value::Builtin(builtin) = target {
        if let Some(prototype) = crate::builtin_meta::instance_prototype(*builtin) {
            return Some(crate::vm::realm_intrinsic_for(
                constructor_realm(new_target),
                prototype,
            ));
        }
    }
    builtin_default_prototype(target)
}

fn object_default_prototype(global: &Value) -> Option<Value> {
    let object_constructor = crate::execute::get_property(global, "Object");
    let object_prototype = crate::execute::get_property(&object_constructor, "prototype");
    if crate::value::is_object(&object_prototype) {
        return Some(object_prototype);
    }
    None
}

fn default_prototype_from_constructor(target: &Value, global: &Value) -> Option<Value> {
    let Value::Builtin(builtin) = target else {
        return None;
    };
    if let Some(realm) = crate::vm::realm_id_for_global_value(global) {
        if let Some(prototype) = crate::builtin_meta::instance_prototype(*builtin) {
            return Some(crate::vm::realm_intrinsic_for(realm, prototype));
        }
    }
    let constructor = crate::execute::get_property(global, crate::builtins::builtin_name(*builtin));
    let prototype = crate::execute::get_property(&constructor, "prototype");
    if crate::value::is_object(&prototype) {
        return Some(prototype);
    }
    None
}

fn construct_bound_in_realm(
    bound: &crate::value::BoundFunctionValue,
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if let Value::HostCapability(capability) = &bound.receiver {
        return crate::vm::with_realm(capability.realm(), || construct_builtin(builtin, arguments))
            .unwrap_or_else(|| Err(crate::vm::not_callable()));
    }
    construct_builtin(builtin, arguments)
}
