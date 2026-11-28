use crate::engine_prelude::*;
use quench_runtime::Engine;
use quench_runtime::test_host::TestHost;
use crate::test_oracle::{assert_tier_agrees, Tier};

/// Cache sites of a program wide enough to outgrow the field cache table's first mapping.
const WIDE_PROGRAM_SITES: usize = 20_000;

/// Runs `source` in `vm`; the number it leaves in `globalThis.answer`.
fn answer(vm: &mut Vm, source: &str, name: &str) -> Option<f64> {
    let program = Engine::specialize(source, name).unwrap();
    quench_runtime::test_support::execute(vm, &program).unwrap();
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
    // Each execution is a fresh realm, so a drive defines its own `hot`.
    let drive = format!("{hot} globalThis.o = {{ x: 1, y: 2 }}; let t = 0; for (let r = 0; r < 4000; r++) {{ t = (t + hot(o)) | 0; }} globalThis.answer = t;");
    let drive = drive.as_str();
    let wide: String = (0..WIDE_PROGRAM_SITES).map(|site| format!("w.f{site} = {site};")).collect();
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    vm.programs.enable_js_lane_methods();
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let before = answer(&mut vm, drive, "drive.js");
    let epoch = vm.cache_epochs.field_entries;
    answer(&mut vm, &format!("var w = {{}}; {wide}"), "wide.js");
    assert!(vm.cache_epochs.field_entries > epoch, "installing the wide program grew the table");
    assert!(before.is_some(), "the drive leaves its answer");
    assert_eq!(answer(&mut vm, drive, "drive-again.js"), before);
}

/// A recursive numeric function calls itself through its scalar entry (`lane_region::scalar`):
/// its results, an overflow that restarts the call generically, and a redefinition of its binding,
/// which retires the entry, agree with the interpreter.
#[test]
fn a_recursive_numeric_function_agrees_with_the_interpreter_through_its_scalar_entry() {
    let fib = "function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }";
    let wide = "function wide(n) { return n < 2 ? 1073741824 : wide(n - 1) + wide(n - 2); }";
    let redefined = "var before = 0; for (var r = 0; r < 30; r++) before += fib(12); fib = function (n) { return 7; }; globalThis.answer = before + fib(20);";
    for source in [
        format!("{fib} globalThis.answer = fib(25);"),
        format!("{wide} globalThis.answer = wide(6) + wide(12);"),
        format!("{fib} {redefined}"),
    ] {
        assert_tier_agrees(&source, Tier::Full);
    }
}
