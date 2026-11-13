use crate::test_host::TestHost;

#[test]
fn numeric_pair_compositions_match_general_execution() {
    let source = include_str!("../../../tests/fixtures/constant-numeric-pairs.js");
    let run = |program| {
        let host = TestHost::default();
        super::super::Vm::new(host.clone()).execute(&program).unwrap();
        let output = host.lines();
        output.clone()
    };
    let specialized = crate::Engine::specialize(source, "constant-numeric-pairs.js").unwrap();
    let general = crate::Engine::specialize_unspecialized(source, "constant-numeric-pairs.js").unwrap();
    assert_eq!(run(specialized), run(general));
}
