use super::*;
use crate::Engine;
use crate::test_host::TestHost;

/// Cache sites of a program wide enough to outgrow the field cache table's first mapping.
const WIDE_PROGRAM_SITES: usize = 20_000;

/// Runs `source` in `vm`; the number it leaves in `globalThis.answer`.
fn answer(vm: &mut Vm<TestHost>, source: &str, name: &str) -> Option<f64> {
    let program = Engine::specialize(source, name).unwrap();
    vm.execute(&program).unwrap();
    let atom = vm.intern_atom("answer");
    vm.own_property(vm.realm.globals, atom).and_then(Value::as_number)
}

/// A program install grows the field cache table, which moves its mapping; the install itself
/// re-points the lane frame's field cache entries (`Vm::field_caches_moved`), so the entries of
/// the hot body that run after it read the new mapping (an entry asserts it in debug builds)
/// and agree with the run before.
#[test]
fn a_program_install_that_moves_the_field_cache_table_leaves_lane_entries_reading_it() {
    let hot = "globalThis.hot = function (o) { let s = 0; for (let i = 0; i < 4; i++) { s = (s + o.x + o.y) | 0; } return s; };";
    let drive = "globalThis.o = { x: 1, y: 2 }; let t = 0; for (let r = 0; r < 4000; r++) { t = (t + hot(o)) | 0; } globalThis.answer = t;";
    let wide: String = (0..WIDE_PROGRAM_SITES).map(|site| format!("w.f{site} = {site};")).collect();
    let mut vm = Vm::new(TestHost::default());
    vm.programs.enable_js_lane_methods();
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    answer(&mut vm, hot, "hot.js");
    let before = answer(&mut vm, drive, "drive.js");
    let epoch = vm.field_entries_epoch;
    answer(&mut vm, &format!("var w = {{}}; {wide}"), "wide.js");
    assert!(vm.field_entries_epoch > epoch, "installing the wide program grew the table");
    assert!(before.is_some(), "the drive leaves its answer");
    assert_eq!(answer(&mut vm, drive, "drive-again.js"), before);
}
