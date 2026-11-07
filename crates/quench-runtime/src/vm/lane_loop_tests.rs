use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// A call-free loop the loop tier rejects (every iteration allocates) runs in its function's
/// lane body, which makes each array literal with its elements in one allocation as the
/// interpreter does, under collections the allocations bring.
#[test]
fn lane_body_runs_a_rejected_allocating_loop_with_fused_array_literals() {
    let source = r#"
      function run(seed) {
        let total = 0;
        for (let i = 0; i < 4000; i++) { const a = [i, seed, i]; total += a[0] + a[1] + a[2] + a.length; }
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 32; repeat++) result = run(repeat);
      print(result);
    "#;
    let program = Engine::specialize(source, "lane-loop-literals.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    let expected: u64 = (0..4000u64).map(|i| 2 * i + 31 + 3).sum();
    assert_eq!(output.borrow().as_slice(), [expected.to_string()]);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let view = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("loop lane view");
    let report = view.body_stats.report(0, view.lane_body_state(), &|_, _| None).expect("the loop runs in the lane body");
    assert!(report.contains("\"state\":\"specialized\"") && !report.contains("\"completed\":0,"), "the body runs the loop: {report}");
}

/// Runs `source` compiled and returns its output and `run`'s lane body report.
fn lane_body_run(source: &str) -> (Vec<String>, String) {
    let program = Engine::specialize(source, "lane-loop.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let view = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("loop lane view");
    let report = view.body_stats.report(0, view.lane_body_state(), &|_, _| None).unwrap_or_default();
    let lines = output.borrow().clone();
    (lines, report)
}

/// A string or BigInt operand takes the interpreter's own operator in the lane body; an
/// object operand, whose coercion runs guest code, leaves to the interpreter first.
#[test]
fn lane_body_runs_string_and_bigint_operators_by_the_interpreters_definition() {
    let (output, report) = lane_body_run(r#"
      let coerced = 0;
      const odd = { valueOf() { coerced++; return "!"; } };
      function run(seed, odd) {
        let text = "", big = BigInt(seed);
        for (let i = 0; i < 3000; i++) { text += "ab"; if (i === 1500) text += odd; big = (big * 3n + 1n) & 0xffffn; }
        return text.length + ":" + big;
      }
      let result;
      for (let repeat = 0; repeat < 32; repeat++) result = run(repeat, odd);
      print(result + " " + coerced);
    "#);
    let mut big: u64 = 31;
    for _ in 0..3000 { big = (big * 3 + 1) & 0xffff; }
    assert_eq!(output, [format!("6001:{big} 32")]);
    // The body iterates natively: it leaves at the object's `+` (pc 9) once a run, never at
    // the loop's header (pc 6).
    assert!(report.contains("\"state\":\"specialized\"") && report.contains("@9 ") && !report.contains("@6 "), "the body runs the loop: {report}");
}
