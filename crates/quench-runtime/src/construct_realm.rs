fn realm_default_prototype(target: &Value, new_target: &Value) -> Option<Value> {
    if let Value::Function(function) = new_target {
        let global = function.captures.get(0);
        if let Some(prototype) = default_prototype_from_constructor(target, &global) {
            return Some(prototype);
        }
        if !matches!(target, Value::Builtin(_)) {
            return object_default_prototype(&global);
        }
    }
    if let Value::BoundFunction(bound) = new_target {
        if let Value::HostCapability(capability) = &bound.receiver {
            if let Some(Some(value)) = crate::vm::with_realm(capability.realm(), || {
                let global = crate::vm::current_global_object();
                default_prototype_from_constructor(target, &global)
            }) {
                return Some(value);
            }
            if matches!(target, Value::Builtin(crate::ops::Builtin::AsyncFunction)) {
                return crate::vm::realm::intrinsic(
                    capability.realm(),
                    crate::ops::Builtin::AsyncFunctionPrototype,
                );
            }
        }
        if let Value::Function(function) = &bound.target {
            let global = function.captures.get(0);
            if let Some(prototype) = default_prototype_from_constructor(target, &global) {
                return Some(prototype);
            }
            if !matches!(target, Value::Builtin(_)) {
                return object_default_prototype(&global);
            }
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
