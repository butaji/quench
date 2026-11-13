//! A compiled lane body's calls of natives that call back into JavaScript
//! (`dispatch_fast::native_calls::lane_body_native_out`) run without the body leaving.
use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// A function calling `Array.prototype.map`, `Map.prototype.forEach` and `Array.prototype.sort`
/// with callbacks, run until it compiles. The callbacks are made once outside it and keep their
/// state in `state`: a body that makes closures or opens a dead zone is not lowered, which is
/// no part of what these tests are about.
const CALLBACK_SOURCE: &str = r#"
    var state = { total: 0, seed: 0 };
    function addSeed(item) { return item + state.seed; }
    function accumulate(value, key) { state.total += value - key; }
    function descending(left, right) { return right - left; }
    function run(array, map, seed) {
      state.seed = seed;
      state.total = 0;
      var mapped = array.map(addSeed);
      for (var at = 0; at < mapped.length; at++) state.total += mapped[at];
      map.forEach(accumulate);
      var sorted = array.slice().sort(descending);
      return state.total + sorted[0] * 3;
    }
    var array = [5, 3, 9, 1, 7];
    var map = new Map([[1, 10], [2, 20]]);
    var result = 0;
    for (var repeat = 0; repeat < 600; repeat++) result += run(array, map, repeat);
    print(result);
"#;

/// A function calling each array callback method and the collections' `forEach` with
/// callbacks, run until it compiles.
const ITERATION_SOURCE: &str = r#"
    var state = { total: 0, seed: 0 };
    function weigh(item, index) { state.total += item * index; }
    function shift(item) { return item + state.seed; }
    function sum(accumulated, item) { return accumulated + item; }
    function odd(item) { return item % 2; }
    function fold(accumulated, item) { return accumulated * 2 + item; }
    function pair(item) { return [item, state.seed]; }
    function big(item) { return item > 8; }
    function positive(item) { return item > 0; }
    function above(item) { return item > 4; }
    function add(value) { state.total += value; }
    function spread(value, key) { state.total += value - key; }
    function ascending(left, right) { return left - right; }
    function run(array, set, map, seed) {
      state.seed = seed;
      state.total = 0;
      array.forEach(weigh);
      state.total += array.map(shift).reduce(sum, 0);
      state.total += array.filter(odd).reduceRight(fold);
      state.total += array.flatMap(pair).length;
      if (array.some(big) && array.every(positive)) state.total += 1;
      state.total += array.find(above) + array.findIndex(above);
      set.forEach(add);
      map.forEach(spread);
      return state.total + array.slice().sort(ascending)[1];
    }
    var array = [5, 3, 9, 1, 7];
    var set = new Set([4, 6]);
    var map = new Map([[1, 10], [2, 20]]);
    var result = 0;
    for (var repeat = 0; repeat < 600; repeat++) result += run(array, set, map, repeat);
    print(result);
"#;

fn run_source(mode: crate::ExecutionMode, source: &str) -> (Vec<String>, Vm<TestHost>) {
    let program = Engine::specialize(&format!("(function(){{{source}}})();"), "lane-native-callbacks.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(mode);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.execute(&program).unwrap();
    let printed = output.borrow().clone();
    (printed, vm)
}

#[test]
fn a_compiled_body_calls_natives_that_call_back_without_leaving() {
    crate::vm::compiled::tier_policy::force_admission();
    assert_body_stays(CALLBACK_SOURCE);
}

/// Array callback methods called from a compiled body run their callbacks as lane
/// activations (`array_steps::lane_body_array_iteration`) and the body continues after the
/// iteration: the output equals the interpreter's and no call leaves the body.
#[test]
fn a_compiled_body_iterates_arrays_and_collections_without_leaving() {
    crate::vm::compiled::tier_policy::force_admission();
    assert_body_stays(ITERATION_SOURCE);
}

fn assert_body_stays(source: &str) {
    let (interpreted, _) = run_source(crate::ExecutionMode::Interpreted, source);
    let (compiled, vm) = run_source(crate::ExecutionMode::Compiled, source);
    assert_eq!(interpreted, compiled);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let reports: Vec<String> = views.iter()
        .filter_map(super::program_store::derived_lane_view)
        .filter_map(|view| view.body_stats.report(0, view.lane_body_state(), &|_, _| None))
        .collect();
    let run = reports.iter().find(|report| report.contains("\"completed\":") && !report.contains("\"completed\":0,"));
    let run = run.unwrap_or_else(|| panic!("a body completes: {reports:?}"));
    assert!(run.contains("\"left_at\":0,"), "no native call leaves the body: {run}");
}
