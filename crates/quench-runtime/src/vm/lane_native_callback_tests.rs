//! A compiled lane body's calls of natives that call back into JavaScript
//! (`dispatch_fast::native_calls::lane_body_native_out`) run without the body leaving.
use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// A function calling `Array.prototype.map`, `Map.prototype.forEach` and `Array.prototype.sort`
/// with callbacks, run until it compiles.
const CALLBACK_SOURCE: &str = r#"
    function run(array, map, seed) {
      let total = 0;
      for (const value of array.map((item) => item + seed)) total += value;
      map.forEach((value, key) => { total += value - key; });
      const sorted = array.slice().sort((left, right) => right - left);
      return total + sorted[0] * 3;
    }
    const array = [5, 3, 9, 1, 7];
    const map = new Map([[1, 10], [2, 20]]);
    let result = 0;
    for (let repeat = 0; repeat < 600; repeat++) result += run(array, map, repeat);
    print(result);
"#;

fn run_in(mode: crate::ExecutionMode) -> (Vec<String>, Vm<TestHost>) {
    let program = Engine::specialize(&format!("(function(){{{CALLBACK_SOURCE}}})();"), "lane-native-callbacks.js").unwrap();
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
    let (interpreted, _) = run_in(crate::ExecutionMode::Interpreted);
    let (compiled, vm) = run_in(crate::ExecutionMode::Compiled);
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
