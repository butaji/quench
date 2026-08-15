fn array_descriptor(values: &crate::value::ArrayData, key: &str) -> Option<Value> {
    if values.is_strict_arguments() && key == "callee" {
        return Some(strict_callee_descriptor(values));
    }
    if let Some(descriptor) = values.descriptor(key) {
        return Some(refresh_array_descriptor(values, key, descriptor));
    }
    if values.is_arguments() && matches!(key, "length" | "callee") {
        return values
            .property(key)
            .map(|value| descriptor_object_with_flags(value, true, false, true));
    }
    if values.is_arguments() && key == "Symbol.iterator" {
        return values
            .property(key)
            .map(|value| descriptor_object_with_flags(value, true, false, true));
    }
    if key == "length" {
        return Some(descriptor_object_with_flags(
            Value::Number(values.logical_len() as f64),
            true,
            false,
            false,
        ));
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get_index(index))
        .map(|value| descriptor_object(&value))
        .or_else(|| {
            values
                .property(key)
                .map(|value| descriptor_object_with_flags(value, true, true, true))
        })
}
fn refresh_array_descriptor(
    values: &crate::value::ArrayData,
    key: &str,
    mut descriptor: Value,
) -> Value {
    let (Ok(index), Value::Object(properties)) = (key.parse::<usize>(), &mut descriptor) else {
        return descriptor;
    };
    let Some(value) = values.get_index(index) else {
        return descriptor;
    };
    if let Some((_, current)) = Rc::make_mut(properties)
        .iter_mut()
        .find(|(name, _)| name == "value")
    {
        *current = value;
    }
    descriptor
}
fn strict_callee_descriptor(values: &crate::value::ArrayData) -> Value {
    let thrower = strict_arguments_thrower(values);
    Value::Object(Rc::new(ObjectData::new(vec![
        ("get".to_string(), thrower.clone()),
        ("set".to_string(), thrower),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(false)),
    ])))
}

fn strict_arguments_thrower(values: &crate::value::ArrayData) -> Value {
    let realm = values.property("\0realm").and_then(|value| match value {
        Value::HostCapability(token) => Some(token.realm()),
        _ => None,
    });
    realm
        .and_then(|realm| {
            crate::vm::with_realm(realm, || crate::vm::realm_intrinsic(Builtin::ThrowTypeError))
        })
        .unwrap_or_else(|| crate::vm::realm_intrinsic(Builtin::ThrowTypeError))
}
