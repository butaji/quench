fn accessor_descriptor(getter: Builtin) -> Value {
    accessor_descriptor_with_setter(getter, None)
}

fn accessor_descriptor_with_setter(getter: Builtin, setter: Option<Builtin>) -> Value {
    let set = setter.map_or(Value::Undefined, Value::Builtin);
    Value::Object(Rc::new(ObjectData::new(vec![
        ("get".to_string(), Value::Builtin(getter)),
        ("set".to_string(), set),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}

fn realm_accessor_descriptor(
    realm: crate::ops::RealmId,
    getter: Builtin,
    setter: Option<Builtin>,
) -> Value {
    let getter = crate::vm::realm_intrinsic_for(realm, getter);
    let setter = setter.map_or(Value::Undefined, |builtin| {
        crate::vm::realm_intrinsic_for(realm, builtin)
    });
    accessor_descriptor_values(getter, setter)
}

fn accessor_descriptor_values(getter: Value, setter: Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("get".to_string(), getter),
        ("set".to_string(), setter),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}
