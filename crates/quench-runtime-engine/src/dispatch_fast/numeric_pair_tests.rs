use quench_runtime::test_host::TestHost;

#[test]
fn numeric_pair_compositions_match_general_execution() {
    let source = include_str!("../../../tests/fixtures/constant-numeric-pairs.js");
    let run = |program| {
        let host = TestHost::default();
        quench_runtime::vm::Vm::new(host.clone()).execute(&program).unwrap();
        let output = host.lines();
        output.clone()
    };
    let specialized = quench_runtime::Engine::specialize(source, "constant-numeric-pairs.js").unwrap();
    let general = quench_runtime::Engine::specialize_unspecialized(source, "constant-numeric-pairs.js").unwrap();
    assert_eq!(run(specialized), run(general));
}
