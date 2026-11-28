//! Exceptions inside compiled code: a `throw` or a callee's exception that a handler of the
//! same function covers is the catch's jump in native code, and one that none covers crosses
//! compiled frames as `LANE_BODY_THREW` (`lane_region::catches`); every result equals the
//! interpreter's.
use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use crate::vm::test_oracle::{assert_tier_agrees, Tier};
use std::{cell::RefCell, rc::Rc};

/// The compiled run's output, which the oracle holds to the interpreter's.
fn matches_interpreter(source: &str) {
    assert_tier_agrees(source, Tier::Full);
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
    matches_interpreter(&format!("(function(){{{source}}})();"));
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
    matches_interpreter(&format!("(function(){{{source}}})();"));
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
    matches_interpreter(&format!("(function(){{{source}}})();"));
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
    matches_interpreter(&format!("(function(){{{source}}})();"));
}

/// The compiled run of `source` under `tier`, which the oracle holds to the interpreter's outcome.
pub(super) fn bodies_match_interpreter(source: &str, tier: Tier) {
    let wrapped = format!("(function(){{{source}}})();");
    assert_tier_agrees(&wrapped, tier);
}

/// A compiled callee's throw reaches the compiled caller's catch through the native call:
/// no run of the caller leaves, so no frame was made for the exception.
#[test]
fn a_throw_from_a_compiled_callee_is_caught_by_a_compiled_caller_without_leaving() {
    let source = r#"
      function thrower(k) {
        let s = 0;
        for (let j = 0; j < 4; j++) s += j;
        if (k % 3 === 0) throw k + s;
        return k;
      }
      function caller(k) {
        try { return thrower(k) + 1; } catch (e) { return e * 10; }
      }
      let result = 0;
      for (let repeat = 0; repeat < 512; repeat++) {
        result = 0;
        for (let i = 0; i < 24; i++) result += caller(i);
      }
      print(String(result));
    "#;
    bodies_match_interpreter(source, Tier::Full);
}

/// An exception no compiled frame catches crosses three of them and reaches the
/// interpreter's handler with its value intact; none of them leaves.
#[test]
fn an_uncaught_throw_crosses_three_compiled_frames_to_the_interpreter() {
    let source = r#"
      function third(k) {
        let s = 0;
        for (let j = 0; j < 4; j++) s += j;
        if (k % 4 === 0) throw { k, s };
        return k;
      }
      function second(k) { let a = third(k); for (let j = 0; j < 2; j++) a += j; return a; }
      function first(k) { let b = second(k); for (let j = 0; j < 2; j++) b += j; return b; }
      let seen = 0, total = 0;
      for (let repeat = 0; repeat < 512; repeat++) {
        for (let i = 0; i < 16; i++) {
          try { total += first(i); } catch (error) { seen += error.k + error.s; }
        }
      }
      print(String(seen) + ':' + String(total));
    "#;
    bodies_match_interpreter(source, Tier::Full);
}

/// A `finally` in a compiled frame between the throw and the catch runs once per unwind, and a
/// rethrow from a catch is a new throw: the values and the order equal the interpreter's.
#[test]
fn finally_blocks_and_rethrows_across_compiled_frames_match_the_interpreter() {
    let source = r#"
      let log = 0;
      function inner(k) {
        let s = 0;
        for (let j = 0; j < 4; j++) s += j;
        if (k % 2 === 0) throw k + s;
        return k;
      }
      function guarded(k) {
        try { return inner(k) * 2; } finally { log++; }
      }
      function rethrower(k) {
        try { return guarded(k); } catch (e) { if (e % 4 === 0) throw e + 1; return -e; }
      }
      function run(n) {
        let total = 0;
        for (let i = 0; i < n; i++) {
          try { total += rethrower(i); } catch (e) { total += e * 100; }
        }
        return total * 1000 + log;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) { log = 0; result = run(24); }
      print(String(result));
    "#;
    bodies_match_interpreter(source, Tier::Full);
}

/// The source location of an uncaught exception thrown in compiled code is the throw's, as the
/// interpreter reports it.
#[test]
fn an_uncaught_exception_from_compiled_frames_reports_the_throws_location() {
    let source = r#"
      function thrower(k) {
        let s = 0;
        for (let j = 0; j < 4; j++) s += j;
        if (k === 40000) throw new Error('boom ' + s);
        return k;
      }
      function caller(k) { return thrower(k) + 1; }
      let total = 0;
      for (let i = 0; i <= 40000; i++) total += caller(i);
      print(String(total));
    "#;
    let location = |mode: crate::ExecutionMode| {
        let program = Engine::specialize(source, "catch-tests.js").unwrap();
        let mut vm = Vm::new(TestHost::writing_to(Rc::new(RefCell::new(Vec::new()))));
        vm.set_memory_policy(crate::MemoryPolicy::Throughput);
        vm.set_execution_mode(mode);
        vm.programs.enable_js_lane_methods();
        let error = vm.execute(&program).expect_err("the exception is uncaught");
        error.source_location().map(|(file, line, column, _)| (file.to_owned(), line, column))
    };
    assert_eq!(location(crate::ExecutionMode::Compiled), location(crate::ExecutionMode::Interpreted));
}

/// `source` wrapped in a function, which compiled equals the interpreter in the lines it
/// prints.
fn wrapped_matches_interpreter(source: &str) {
    matches_interpreter(&format!("(function(){{{source}}})();"))
}

/// A catch binding a closure of the catch block captures lives in the frame's environment, a
/// fresh one per entry of the handler: each closure sees its own value, whether the exception
/// was a `throw` of the loop or the exception of a call.
#[test]
fn a_catch_binding_a_closure_captures_is_fresh_per_entry() {
    let thrown = r#"
      function run(n) {
        const fns = [];
        let total = 0;
        for (let i = 0; i < n; i++) {
          try { if ((i & 1) === 0) throw i; total += 1; }
          catch (e) { fns.push(() => e + i); }
        }
        for (let k = 0; k < fns.length; k++) total += fns[k]();
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) result = run(48);
      print(String(result));
    "#;
    wrapped_matches_interpreter(thrown);
    let called = r#"
      function boom(k) { if (k % 3 === 0) throw k; return k; }
      function run(n) {
        const fns = [];
        let total = 0;
        for (let i = 0; i < n; i++) {
          try { total += boom(i); }
          catch (e) { fns.push(() => e * 2); }
        }
        for (let k = 0; k < fns.length; k++) total += fns[k]();
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) result = run(48);
      print(String(result));
    "#;
    wrapped_matches_interpreter(called);
}

/// A throw inside a `with` statement caught outside it leaves the `with` scope at the handler
/// (a name after the catch no longer reads the object), and a catch inside the statement keeps
/// it.
#[test]
fn a_catch_unwinds_the_with_scopes_it_leaves() {
    let outside = r#"
      var o = { x: 5 };
      function run(n) {
        let total = 0;
        for (let i = 0; i < n; i++) {
          try { with (o) { if ((i & 1) === 0) throw x + i; total += x; } }
          catch (e) { total += e + (typeof x === 'undefined' ? 1000 : 0); }
        }
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) result = run(48);
      print(String(result));
    "#;
    wrapped_matches_interpreter(outside);
    let inside = r#"
      var o = { x: 7 };
      function boom(k) { if (k % 3 === 0) throw k; return k; }
      function run(n) {
        let total = 0;
        for (let i = 0; i < n; i++) {
          with (o) {
            try { total += boom(i); }
            catch (e) { total += e + x; }
          }
        }
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) result = run(48);
      print(String(result));
    "#;
    wrapped_matches_interpreter(inside);
}

/// A catch inside a generator body, of a `throw` and of a call's exception, serves from the
/// generator's frame as it does in any function.
#[test]
fn a_catch_inside_a_generator_body_matches_the_interpreter() {
    let source = r#"
      function boom(k) { if (k % 4 === 0) throw k; return k; }
      function* gen(n) {
        let total = 0;
        for (let i = 0; i < n; i++) {
          try { if (i % 3 === 0) throw i; total += i; }
          catch (e) { total += e * 2; }
          try { total += boom(i); }
          catch (f) { total += f * 3; }
          yield total;
        }
        return total;
      }
      function run(n) {
        let sum = 0;
        for (const value of gen(n)) sum += value;
        return sum;
      }
      let result = 0;
      for (let repeat = 0; repeat < 256; repeat++) result = run(48);
      print(String(result));
    "#;
    wrapped_matches_interpreter(source);
}
