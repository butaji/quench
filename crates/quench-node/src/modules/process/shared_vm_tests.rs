use super::*;
use quench_runtime_next::{Engine, Runtime};

#[test]
fn shared_module_cache_roots_survive_collection_and_expire_on_fresh_execution() {
    let directory =
        std::env::temp_dir().join(format!("quench-module-root-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let filename = directory.join("main.cjs");
    std::fs::write(&filename, "module.exports = { answer: 42 };").unwrap();
    let host =
        NodeHost::new(vec!["quench-node".into()]).with_commonjs_entry(filename);
    let state = host.state();
    let mut runtime = Runtime::new(host);
    let program = Engine::specialize("", "module-root-lifecycle.js").unwrap();
    let mut previous = None;
    for _ in 0..2 {
        runtime.execute(&program).unwrap();
        if let Some(previous) = previous {
            assert!(!runtime.root_is_live(previous));
        }
        let cached = match &state.borrow().module_cache {
            crate::host::ModuleCache::Shared(cache) => {
                assert_eq!(cache.len(), 1);
                *cache.values().next().unwrap()
            }
            crate::host::ModuleCache::Legacy(_) => {
                panic!("shared execution retained a legacy cache")
            }
        };
        runtime.collect(&program).unwrap();
        let key = runtime.string_rooted("exports");
        let exports = runtime.get_property_rooted(cached, key).unwrap();
        let key = runtime.string_rooted("answer");
        let answer = runtime.get_property_rooted(exports, key).unwrap();
        assert_eq!(
            runtime.rooted_value(answer).unwrap().as_number(),
            Some(42.0)
        );
        assert!(runtime.release_root(exports));
        assert!(runtime.release_root(answer));
        previous = Some(cached);
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn canonical_process_root_survives_global_replacement_and_refreshes_with_the_vm() {
    let host = NodeHost::new(vec!["quench-node".into()]);
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
