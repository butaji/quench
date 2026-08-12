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

fn empty_function_code() -> crate::machine::FunctionCode {
    let mut arena = crate::machine::CodeArena::new();
    let range = arena.append_slice(&[]);
    crate::machine::FunctionCode::new(arena.freeze(), range)
}
