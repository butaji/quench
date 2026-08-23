#[test]
fn machine_resolves_frame_ranges_from_its_function_store() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    assert_eq!(
        machine.store.as_ref().and_then(|store| store.get(function.range)),
        function.ops()
    );
}

#[test]
fn code_arena_can_import_existing_function_ranges() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let mut arena = super::CodeArena::new();
    let range = arena.append_function(&function).expect("function range");
    assert_eq!(arena.freeze().get(range).map(<[_]>::len), Some(1));
}

#[test]
fn linked_nested_bodies_share_one_immutable_code_store() {
    let child = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let root = super::FunctionCode::from_ops(vec![super::Op::IteratorBinding { iterator: 0, body: child, close_normal: false }]);
    let Some([super::Op::IteratorBinding { body, .. }]) = root.ops() else { panic!("iterator binding"); };
    assert!(std::rc::Rc::ptr_eq(&root.store, &body.store));
}

#[test]
fn register_window_is_pre_sized_from_code_metadata() {
    let window = RegisterWindow::with_count(3);
    assert_eq!(window.values, vec![Value::Undefined; 3]);
}
