use super::*;
use quench_runtime::ops::RealmId;
use rqj::{Engine, Runtime};

#[test]
fn canonical_process_root_survives_global_replacement_and_refreshes_with_the_vm() {
    let host = NodeHost::new(RealmId::ROOT, vec!["quench-node".into()]);
    let state = host.state();
    let mut runtime = Runtime::new(host);
    let program = Engine::specialize(
        "var original = process; globalThis.process = { replacement: true };",
        "process-root-lifecycle.js",
    )
    .unwrap();
    let mut previous = None;
    for _ in 0..2 {
        runtime.execute(&program).unwrap();
        if let Some(previous) = previous {
            assert!(!runtime.root_is_live(previous));
        }
        let cached = match state.borrow().process_module.as_ref().unwrap() {
            ProcessModule::Shared(root) => *root,
            ProcessModule::Legacy(_) => panic!("shared initialization retained a legacy value"),
        };
        runtime.collect(&program).unwrap();
        assert!(runtime.root_is_live(cached));
        let global = runtime.global_root().unwrap();
        let original_key = runtime.string_rooted("original");
        let original = runtime.get_property_rooted(global, original_key).unwrap();
        let process_key = runtime.string_rooted("process");
        let replacement = runtime.get_property_rooted(global, process_key).unwrap();
        assert_eq!(runtime.rooted_value(cached), runtime.rooted_value(original));
        assert_ne!(
            runtime.rooted_value(cached),
            runtime.rooted_value(replacement)
        );
        for root in [global, original_key, original, process_key, replacement] {
            assert!(runtime.release_root(root));
        }
        previous = Some(cached);
    }
}
