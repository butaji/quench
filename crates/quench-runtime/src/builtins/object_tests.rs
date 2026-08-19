#[test]
fn has_own_observes_function_own_properties() {
    let function = Value::Function(Rc::new(FunctionValue {
        code: empty_function_code(),
        params: 2,
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::ops::FunctionStrictness::Sloppy,
        is_async: false,
        mapped_arguments: true,
        captures: crate::environment::Environment::new(),
        with_captures: Vec::new(),
        properties: Rc::new(RefCell::new(vec![
            ("length".to_string(), Value::Number(2.0)),
            ("custom".to_string(), Value::Boolean(true)),
        ])),
        private_slots: Rc::new(RefCell::new(Vec::new())),
        private_environment: crate::private_environment::PrivateEnvironment::default(),
        instance_fields: Rc::new(RefCell::new(Vec::new())),
    }));
    assert_eq!(
        execute_special(
            Builtin::ObjectHasOwnProperty,
            None,
            &[function.clone(), Value::String("length".to_string())],
        )
        .unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        execute_special(
            Builtin::ObjectHasOwnProperty,
            None,
            &[function, Value::String("custom".to_string())],
        )
        .unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_accessor_define_keeps_creation_order() {
    let arr = Value::array(Vec::new());
    let getter = Value::Function(Rc::new(FunctionValue {
        code: empty_function_code(),
        params: 0,
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::ops::FunctionStrictness::Sloppy,
        is_async: false,
        mapped_arguments: false,
        captures: crate::environment::Environment::new(),
        with_captures: Vec::new(),
        properties: Rc::new(RefCell::new(Vec::new())),
        private_slots: Rc::new(RefCell::new(Vec::new())),
        private_environment: crate::private_environment::PrivateEnvironment::default(),
        instance_fields: Rc::new(RefCell::new(Vec::new())),
    }));
    let arr = crate::builtins::define_own_property(
        &arr,
        "a",
        &[
            ("get".to_string(), getter.clone()),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(true)),
        ],
    )
    .unwrap();
    let arr = crate::builtins::set_property(arr, "b", Value::Number(2.0));
    let arr =
        crate::builtins::define_own_property(&arr, "a", &[("get".to_string(), getter)]).unwrap();
    assert_eq!(
        crate::own_keys::enumerate_object_properties(&arr),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn define_own_property_keeps_creation_order() {
    let obj = Value::Object(Rc::new(ObjectData::new(Vec::new())));
    assert!(crate::properties::set_with_receiver(&obj, "a", &Value::Number(1.0), &obj,).unwrap());
    let obj = crate::locals::resolved_replacement(obj);
    assert!(crate::properties::set_with_receiver(&obj, "b", &Value::Number(2.0), &obj,).unwrap());
    let obj = crate::locals::resolved_replacement(obj);
    let obj = crate::builtins::define_own_property(
        &obj,
        "a",
        &[("value".to_string(), Value::Number(11.0))],
    )
    .unwrap();
    assert_eq!(
        crate::own_keys::enumerate_object_properties(&obj),
        vec!["a".to_string(), "b".to_string()]
    );
}

fn empty_function_code() -> crate::machine::FunctionCode {
    let mut arena = crate::machine::CodeArena::new();
    let range = arena.append_slice(&[]);
    crate::machine::FunctionCode::new(arena.freeze(), range)
}

#[test]
fn array_property_descriptor_exposes_assigned_value() {
    let array =
        crate::builtins::set_property(Value::array(Vec::new()), "custom", Value::Boolean(true));
    let descriptor =
        descriptor(Some(&array), Some(&Value::String("custom".into()))).expect("array descriptor");
    assert_eq!(
        crate::execute::get_property(&descriptor, "value"),
        Value::Boolean(true)
    );
    assert_eq!(
        crate::execute::get_property(&descriptor, "enumerable"),
        Value::Boolean(true)
    );
}

#[test]
fn get_own_property_descriptor_throws_on_nullish_target() {
    let error = execute_builtin_with_receiver(
        Builtin::ObjectGetOwnPropertyDescriptor,
        &[Value::Null, Value::String("x".to_string())],
        None,
    )
    .unwrap_err();
    assert!(matches!(error, VmError::Thrown(_)));
}
