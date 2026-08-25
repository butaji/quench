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

fn object_prototype_property(
    receiver: &Value,
    properties: &crate::value::ObjectData,
    key: &str,
) -> Value {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))
        .map_or(Value::Undefined, |prototype| {
            get_property_with_receiver(&prototype, key, receiver).unwrap_or(Value::Undefined)
        })
}

pub(crate) fn object_property(
    properties: &Rc<crate::value::ObjectData>,
    receiver: &Value,
    key: &str,
) -> Value {
    let is_global = realm::id_for_global(properties).is_some()
        || GLOBAL_OBJECT.with(|global| {
            global
                .borrow()
                .as_ref()
                .is_some_and(|candidate| Rc::ptr_eq(candidate, properties))
        });
    if is_global {
        if let Some(value) = crate::vm::current_context_or_default().host_value(key) {
            return value;
        }
    }
    if let Some(value) = direct_object_property(properties, key) {
        return value;
    }
    let inherited = object_prototype_property(receiver, properties, key);
    if !matches!(inherited, Value::Undefined) {
        return inherited;
    }
    object_builtin_property(properties, key)
}

fn object_builtin_property(properties: &crate::value::ObjectData, key: &str) -> Value {
    crate::builtins::property(object_prototype(properties), key)
}

fn direct_object_property(properties: &Rc<crate::value::ObjectData>, key: &str) -> Option<Value> {
    let deleted_key = crate::builtins::deleted_key(key);
    let mut direct = None;
    for (name, value) in properties.iter().rev() {
        if name == &deleted_key {
            return Some(Value::Undefined);
        }
        if direct.is_none() && name == key {
            direct = Some(value);
        }
    }
    if let Some(value) = direct {
        if matches!(value, Value::Null)
            && crate::vm::global_builtin_exists(key)
            && global_object_property(properties, key).is_some()
        {
            return global_object_property(properties, key);
        }
        if key == "format"
            && matches!(
                value,
                Value::Builtin(
                    crate::ops::Builtin::IntlDateTimeFormatFormat
                        | crate::ops::Builtin::IntlNumberFormatFormat,
                )
            )
        {
            return Some(crate::vm::bind_receiver_property(
                value.clone(),
                &Value::Object(properties.clone()),
            ));
        }
        if key == "segment"
            && matches!(
                value,
                Value::Builtin(crate::ops::Builtin::IntlSegmenterSegment)
            )
        {
            return Some(crate::vm::bind_receiver_property(
                value.clone(),
                &Value::Object(properties.clone()),
            ));
        }
        return Some(property_value(&value));
    }
    if let Some(value) = global_object_property(properties, key) {
        return Some(value);
    }
    if let Some(value) = boxed_string_property(properties, key) {
        return Some(value);
    }
    null_prototype_value(properties)
}

fn null_prototype_value(properties: &crate::value::ObjectData) -> Option<Value> {
    properties
        .iter()
        .any(|(name, value)| name == "\0prototype" && matches!(value, Value::Null))
        .then_some(Value::Undefined)
}

fn global_object_property(properties: &Rc<crate::value::ObjectData>, key: &str) -> Option<Value> {
    if let Some(realm) = realm::id_for_global(properties) {
        return Some(global_property(properties, key, Some(realm)));
    }
    (GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|candidate| Rc::ptr_eq(candidate, properties))
    }) || crate::vm::is_global_object(&Value::Object(properties.clone())))
    .then(|| global_property(properties, key, None))
}

pub(crate) fn boxed_string_property(properties: &Rc<crate::value::ObjectData>, key: &str) -> Option<Value> {
    let Some((_, Value::String(value))) = properties.iter().find(|(name, _)| name == "_value")
    else {
        return None;
    };
    if crate::conversion::is_symbol_string(&value) {
        return None;
    }
    if crate::builtins::builtin_prototype_property_is_removed(
        crate::ops::Builtin::StringPrototype,
        key,
    ) {
        return None;
    }
    match get_property(&Value::String(value.clone()), key) {
        Value::Undefined => None,
        indexed => Some(indexed),
    }
}

fn object_prototype(properties: &crate::value::ObjectData) -> Builtin {
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
    if key == "constructor" {
        return realm
            .map(|realm| {
                realm::intrinsic(realm, Builtin::Object).unwrap_or(Value::Builtin(Builtin::Object))
            })
            .unwrap_or_else(|| crate::builtins::property(Builtin::ObjectPrototype, key));
    }
    if let Some(value) = crate::vm::current_context_or_default().host_value(key) {
        return value;
    }
    if let Some(binding) = crate::vm::current_context_or_default().host_binding(key) {
        let token = Value::HostCapability(Rc::new(
            crate::value::HostCapabilityValue::new(binding),
        ));
        if matches!(binding.kind, crate::ops::HostCapabilityKind::Custom(1)) {
            return Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue::new(
                binding.realm,
                Value::Builtin(Builtin::HostCapability(binding.kind)),
                token,
            )));
        }
        return token;
    }
    realm::global_builtin(key).map_or_else(
        || crate::builtins::property(Builtin::ObjectPrototype, key),
        |builtin| {
            realm.map_or_else(
                || crate::vm::realm_intrinsic_for(RealmId::ROOT, builtin),
                |realm| crate::vm::realm_intrinsic_for(realm, builtin),
            )
        },
    )
}

fn current_host_capability(kind: HostCapabilityKind) -> Value {
    let realm = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map_or(RealmId::ROOT, |rc| rc.realm())
    });
    Value::HostCapability(Rc::new(crate::value::HostCapabilityValue::new(
        HostCapabilityRef { realm, kind },
    )))
}

#[cfg(test)]
mod canonical_lookup_tests {
    use super::direct_object_property;
    use crate::value::{ObjectData, Value};
    use std::rc::Rc;

    fn object(properties: Vec<(String, Value)>) -> Rc<ObjectData> {
        Rc::new(ObjectData::new(properties))
    }

    fn named(name: &str, value: Value) -> (String, Value) {
        (name.to_string(), value)
    }

    #[test]
    fn ordinary_lookup_returns_visible_value() {
        let properties = object(vec![
            named("first", Value::Number(1.0)),
            named("second", Value::String("hit".into())),
        ]);
        assert_eq!(
            direct_object_property(&properties, "second"),
            Some(Value::String("hit".into()))
        );
    }

    #[test]
    fn lookup_preserves_descriptor_and_deleted_entries() {
        let descriptor = crate::builtins::descriptor_key("value");
        let deleted = crate::builtins::deleted_key("gone");
        let properties = object(vec![
            named("value", Value::Number(1.0)),
            (descriptor, Value::Boolean(true)),
            (deleted, Value::Undefined),
            named("gone", Value::Number(2.0)),
        ]);
        assert_eq!(
            direct_object_property(&properties, "value"),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            direct_object_property(&properties, "gone"),
            Some(Value::Undefined)
        );
    }

    #[test]
    fn lookup_preserves_last_duplicate_value() {
        let properties = object(vec![
            named("key", Value::Number(1.0)),
            named("key", Value::Number(2.0)),
        ]);
        assert_eq!(
            direct_object_property(&properties, "key"),
            Some(Value::Number(2.0))
        );
    }
    #[test]
    fn dictionary_mode_uses_canonical_vector_after_crossing_boundary() {
        let mut entries = (0..=crate::value::DICTIONARY_SLOT_THRESHOLD)
            .map(|index| named(&format!("key{index}"), Value::Number(index as f64)))
            .collect::<Vec<_>>();
        let properties = object(entries.clone());
        assert!(properties.is_dictionary());
        assert_eq!(
            direct_object_property(&properties, "key0"),
            Some(Value::Number(0.0))
        );
        assert_eq!(
            direct_object_property(&properties, "key32"),
            Some(Value::Number(32.0))
        );

        // A later write is represented in the same vector; dictionary mode
        // must not consult a stale slot/cache or lose last-write-wins order.
        entries.retain(|(name, _)| name != "key7");
        entries.push(("key7".into(), Value::String("mutated".into())));
        let mutated = object(entries);
        assert_eq!(
            direct_object_property(&mutated, "key7"),
            Some(Value::String("mutated".into()))
        );
    }

    #[test]
    fn private_entries_do_not_push_ordinary_object_into_dictionary_mode() {
        let mut entries = (0..crate::value::DICTIONARY_SLOT_THRESHOLD)
            .map(|index| named(&format!("key{index}"), Value::Undefined))
            .collect::<Vec<_>>();
        entries.push(("\0descriptor".into(), Value::Boolean(true)));
        let properties = object(entries);
        assert!(!properties.is_dictionary());
        assert_eq!(
            direct_object_property(&properties, "key31"),
            Some(Value::Undefined)
        );
    }
}
