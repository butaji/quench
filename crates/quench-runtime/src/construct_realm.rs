fn realm_default_prototype(target: &Value, new_target: &Value) -> Option<Value> {
    if let Value::BoundFunction(bound) = new_target {
        if let Value::HostCapability(capability) = &bound.receiver {
            let builtin = builtin_default_name(target)?;
            return crate::vm::intrinsic_for_realm(capability.realm(), builtin);
        }
    }
    builtin_default_prototype(target)
}

fn builtin_default_name(target: &Value) -> Option<crate::ops::Builtin> {
    match target {
        Value::Builtin(crate::ops::Builtin::Boolean) => Some(crate::ops::Builtin::BooleanPrototype),
        Value::Builtin(crate::ops::Builtin::Number) => Some(crate::ops::Builtin::NumberPrototype),
        Value::Builtin(crate::ops::Builtin::String) => Some(crate::ops::Builtin::StringPrototype),
        Value::Builtin(crate::ops::Builtin::Object) => Some(crate::ops::Builtin::ObjectPrototype),
        _ => None,
    }
}
