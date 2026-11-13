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
    crate::vm::compiled::tier_policy::force_admission();
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

/// The interpreter's calls into one hot callee enter its body over and over inside one lane run.
const HOT_CALLEE: &str = r#"(function () {
  function step(a, b) { let s = 0; for (let i = 0; i < 4; i++) { s = (s + a * i + b) | 0; } return s; }
  let total = 0;
  for (let round = 0; round < 20000; round++) { total = (total + step(round & 7, round & 3)) & 1048575; }
  globalThis.answer = total;
})();"#;

/// The entries of one lane run bind its words once: the run's VM, program, residual, lane
/// context, field cache entries, stack limit and payload stand (`LaneFrame::bind_run`), so an
/// entry that changes any of them is one that bound a run, and the runs are a small fraction of
/// the entries. What an entry writes itself is its window and the stacks' room.
#[test]
fn entries_of_one_lane_run_rewrite_no_word_of_the_run() {
    crate::vm::compiled::tier_policy::force_admission();
    let program = Engine::specialize(HOT_CALLEE, "hot-callee.js").unwrap();
    super::compiled::LANE_ENTRIES.with(|counts| counts.set((0, 0)));
    super::compiled::LANE_RUN_REWRITES.with(|count| count.set(0));
    let mut vm = Vm::new(TestHost::default());
    vm.programs.enable_js_lane_methods();
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let (entries, binds) = super::compiled::LANE_ENTRIES.with(std::cell::Cell::get);
    let rewrites = super::compiled::LANE_RUN_REWRITES.with(std::cell::Cell::get);
    if super::compiled::tier_policy::tier_may_run(vm.memory_policy(), crate::jit_mode::Tier::ANY) {
        assert!(entries > 1000, "the hot callee runs as a body, entered {entries} times");
        assert!(rewrites <= binds, "{rewrites} entries rewrote words of their run, {binds} bound one");
        assert!(binds * 200 < entries, "{binds} runs bound for {entries} entries");
    }
}
