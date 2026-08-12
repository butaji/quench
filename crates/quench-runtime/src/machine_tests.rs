#[test]
fn machine_resolves_frame_ranges_from_its_function_store() {
    let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
    let machine = Machine::with_function(&function, EnvironmentRef(0), 1);
    assert_eq!(
        machine.store.as_ref().and_then(|store| store.get(function.range)),
        function.ops()
    );
}
