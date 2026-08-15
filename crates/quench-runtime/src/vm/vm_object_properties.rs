fn object_alias_property(alias: &crate::value::ObjectAliasValue, key: &str) -> Value {
    alias
        .0
        .borrow()
        .upgrade()
        .map_or(Value::Undefined, |object| {
            let object = crate::locals::resolved_replacement(Value::Object(object));
            get_property(&object, key)
        })
}

pub(crate) fn has_restricted_function_property(value: &Value, key: &str) -> bool {
    if !matches!(key, "caller" | "arguments") {
        return false;
    }
    let is_restricted = match value {
        Value::Function(function) => {
            matches!(
                function.kind,
                crate::ops::FunctionKind::Arrow | crate::ops::FunctionKind::Generator
            ) || function.strictness == crate::ops::FunctionStrictness::Strict
        }
        Value::BoundFunction(_) => true,
        _ => return false,
    };
    let properties = match value {
        Value::Function(function) => &function.properties.borrow()[..],
        Value::BoundFunction(bound) => &bound.properties.borrow()[..],
        _ => &[],
    };
    is_restricted && !properties.iter().any(|(name, _)| name == key)
}

fn object_prototype_property(receiver: &Value, properties: &[(String, Value)], key: &str) -> Value {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))
        .map_or(Value::Undefined, |prototype| {
            get_property_with_receiver(prototype, key, receiver).unwrap_or(Value::Undefined)
        })
}

pub(crate) fn object_property(
    properties: &Rc<crate::value::ObjectData>,
    receiver: &Value,
    key: &str,
) -> Value {
    if properties
        .iter()
        .any(|(name, _)| name == &crate::builtins::deleted_key(key))
    {
        return Value::Undefined;
    }
    if let Some((_, value)) = properties.iter().rev().find(|(name, _)| name == key) {
        return property_value(value);
    }
    if let Some(realm) = realm::id_for_global(properties) {
        return global_property(properties, key, Some(realm));
    }
    if GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|candidate| Rc::ptr_eq(candidate, properties))
    }) {
        return global_property(properties, key, None);
    }
    if let Some(value) = boxed_string_property(properties, key) {
        return value;
    }
    if properties
        .iter()
        .any(|(name, value)| name == "\0prototype" && matches!(value, Value::Null))
    {
        return Value::Undefined;
    }
    let inherited = object_prototype_property(receiver, properties, key);
    if !matches!(inherited, Value::Undefined) {
        return inherited;
    }
    let prototype = object_prototype(properties);
    if key == "constructor" {
        return crate::builtins::property(prototype, key);
    }
    crate::builtins::property(prototype, key)
}

fn boxed_string_property(properties: &Rc<crate::value::ObjectData>, key: &str) -> Option<Value> {
    let Some((_, Value::String(value))) = properties.iter().find(|(name, _)| name == "_value")
    else {
        return None;
    };
    if crate::conversion::is_symbol_string(value) {
        return None;
    }
    match get_property(&Value::String(value.clone()), key) {
        Value::Undefined => None,
        indexed => Some(indexed),
    }
}

fn object_prototype(properties: &[(String, Value)]) -> Builtin {
    if let Some((_, value)) = properties.iter().find(|(name, _)| name == "_value") {
        return match value {
            Value::String(value) if value.contains('\0') => Builtin::SymbolPrototype,
            Value::String(_) => Builtin::StringPrototype,
            Value::Number(_) => Builtin::NumberPrototype,
            Value::Boolean(_) => Builtin::BooleanPrototype,
            Value::BigInt(_) => Builtin::BigIntPrototype,
            _ => Builtin::ObjectPrototype,
        };
    }
    if properties.iter().any(|(name, _)| name == "timeValue") {
        Builtin::DatePrototype
    } else if properties.iter().any(|(name, _)| name == "source")
        && properties.iter().any(|(name, _)| name == "flags")
    {
        Builtin::RegExpPrototype
    } else {
        Builtin::ObjectPrototype
    }
}

fn global_property(
    properties: &Rc<crate::value::ObjectData>,
    key: &str,
    realm: Option<RealmId>,
) -> Value {
    if key == "globalThis" {
        return Value::Object(properties.clone());
    }
    if realm.is_none() {
        if let Some(binding) = crate::vm::current_context_or_default().host_binding(key) {
            return Value::HostCapability(Rc::new(crate::value::HostCapabilityValue::new(binding)));
        }
    }
    realm::global_builtin(key).map_or_else(
        || crate::builtins::property(Builtin::ObjectPrototype, key),
        |builtin| {
            realm.map_or_else(
                || Value::Builtin(builtin),
                |realm| realm::intrinsic(realm, builtin).unwrap_or(Value::Undefined),
            )
        },
    )
}

fn current_host_capability(kind: HostCapabilityKind) -> Value {
    let realm = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map_or(RealmId::ROOT, VmContext::realm)
    });
    Value::HostCapability(Rc::new(crate::value::HostCapabilityValue::new(
        HostCapabilityRef { realm, kind },
    )))
}
