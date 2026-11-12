//! Exceptions inside compiled code: a `throw` a handler of the same function covers is the
//! catch's jump in native code (`lane_region::catches`), and every result equals the
//! interpreter's.
use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// The printed lines of `source` run in `mode`, and the native region runs of its function
/// `name` (`None` in the interpreter).
fn run_in(mode: crate::ExecutionMode, source: &str, name: &str) -> (Vec<String>, Option<u64>) {
    let program = Engine::specialize(source, "catch-tests.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name))
        .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(mode);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.execute(&program).unwrap();
    let runs = (mode == crate::ExecutionMode::Compiled).then(|| {
        let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
        let view = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("lane view");
        u64::from(view.osr_diagnostics().native_region_runs)
    });
    let lines = output.borrow().clone();
    (lines, runs)
}

/// The compiled run's output, which must equal the interpreter's, and its native runs.
fn matches_interpreter(source: &str, name: &str) -> u64 {
    let (expected, _) = run_in(crate::ExecutionMode::Interpreted, source, name);
    let (actual, runs) = run_in(crate::ExecutionMode::Compiled, source, name);
    assert_eq!(actual, expected);
    runs.unwrap()
}

/// A throw and its catch in a hot loop run in the loop's native region: the thrown value
/// reaches the catch's binding without the interpreter re-executing the `throw`.
#[test]
fn a_throw_caught_in_a_hot_loop_runs_natively() {
    let source = r#"
      function run(n) {
        let total = 0;
        for (let i = 0; i < n; i++) {
          try { if ((i & 3) === 0) throw i; total = (total + 1) | 0; }
          catch (e) { total = (total + e * 2) | 0; }
        }
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 512; repeat++) result = run(64);
      print(String(result));
    "#;
    let runs = matches_interpreter(&format!("(function(){{{source}}})();"), "run");
    assert!(runs > 0, "the loop with a caught throw runs in a native region");
}

/// A catch that binds nothing, and a thrown non-number, take the same jump.
#[test]
fn a_binding_less_catch_and_object_throws_match_the_interpreter() {
    let source = r#"
      function run(n) {
        let hits = 0, sum = 0;
        for (let i = 0; i < n; i++) {
          try { if (i % 5 === 0) throw { v: i }; hits++; }
          catch (e) { sum += e.v; }
          try { if (i % 7 === 0) throw 'x'; }
          catch { hits += 2; }
        }
        return hits * 1000 + sum;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) result = run(70);
      print(String(result));
    "#;
    matches_interpreter(&format!("(function(){{{source}}})();"), "run");
}

/// A callee's throw reaches the caller's catch, and a `finally` runs on both paths, whatever
/// tier each runs in.
#[test]
fn nested_throws_and_finally_blocks_match_the_interpreter() {
    let source = r#"
      function inner(k) { if (k % 3 === 0) throw k; return k; }
      function middle(k) { return inner(k) + 1; }
      function run(n) {
        let total = 0, cleaned = 0;
        for (let i = 0; i < n; i++) {
          try {
            try { total += middle(i); }
            finally { cleaned++; }
          } catch (e) { total += e * 10; }
        }
        return total * 100 + cleaned;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) result = run(48);
      print(String(result));
    "#;
    matches_interpreter(&format!("(function(){{{source}}})();"), "run");
}

/// An uncaught throw still reaches the caller's handler exactly, with the value intact.
#[test]
fn an_uncaught_throw_in_a_hot_loop_reaches_the_outer_handler() {
    let source = r#"
      function run(n) {
        let total = 0;
        for (let i = 0; i < n; i++) {
          try { if (i === n - 1) throw 'last'; total += i; }
          catch (e) { if (e === 'last') throw { total }; }
        }
        return total;
      }
      let seen = 0;
      for (let repeat = 0; repeat < 256; repeat++) {
        try { run(32); } catch (error) { seen += error.total; }
      }
      print(String(seen));
    "#;
    matches_interpreter(&format!("(function(){{{source}}})();"), "run");
}
