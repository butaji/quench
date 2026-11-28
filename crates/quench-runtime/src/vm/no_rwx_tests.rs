//! W^X: after compiled code has run, no mapping of the process is writable and executable at
//! once (Linux; Apple's MAP_JIT views are the one platform exception).

use crate::Engine;
use crate::test_host::TestHost;
use super::Vm;
use std::cell::RefCell;
use std::rc::Rc;

/// The `/proc/self/maps` lines whose permissions include both `w` and `x`.
fn writable_executable_mappings() -> Vec<String> {
    std::fs::read_to_string("/proc/self/maps").expect("maps readable").lines()
        .filter(|line| line.split_whitespace().nth(1).is_some_and(|perms| perms.starts_with("rw") && perms.as_bytes().get(2) == Some(&b'x')))
        .map(str::to_owned).collect()
}

#[cfg(target_os = "linux")]
#[test]
fn compiled_code_never_maps_a_page_writable_and_executable() {
    let source = r#"function step(state) { state.count = (state.count + 1) | 0; return state.count; }
        function run(state, n) { let total = 0; for (let i = 0; i < n; i++) total = (total + step(state)) | 0; return total; }
        const state = {count: 0};
        let sum = 0;
        for (let round = 0; round < 40; round++) sum = (sum + run(state, 50000)) | 0;
        print(String(state.count));"#;
    let program = Engine::specialize(source, "no-rwx.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["2000000"]);
    // The control: compiled code ran, so a read-execute view of the code heap is mapped.
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    assert!(maps.lines().any(|line| line.split_whitespace().nth(1) == Some("r-xs")), "no compiled code was published");
    let bad = writable_executable_mappings();
    assert!(bad.is_empty(), "writable and executable mappings: {bad:?}");
}
