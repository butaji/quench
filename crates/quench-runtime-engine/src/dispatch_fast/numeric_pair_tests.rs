#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;

#[test]
fn numeric_pair_compositions_match_general_execution() {
    let source = include_str!("../../../quench-runtime/tests/fixtures/constant-numeric-pairs.js");
    let run = |program| {
        let host = TestHost::default();
        let mut vm = quench_runtime::test_support::vm(host.clone());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let output = host.lines();
        output.clone()
    };
    let specialized = quench_runtime::Engine::specialize(source, "constant-numeric-pairs.js").unwrap();
    let general = quench_runtime::Engine::specialize_unspecialized(source, "constant-numeric-pairs.js").unwrap();
    assert_eq!(run(specialized), run(general));
}
