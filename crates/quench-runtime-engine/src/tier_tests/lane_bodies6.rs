//! Lane bodies against the interpreter: each program runs compiled and interpreted, and the compiled run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use super::lane_bodies5::assert_cell_crossings_match;
use super::shared_osr2::compiled_loop_diagnostics;

#[test]
#[cfg(not(quench_profiled))]
fn lane_loops_make_array_literals_of_integers_and_objects_natively() {
    // An array literal of integers, a number computed in the loop and one object twice
    // (`Allocation::ArrayOf`) is made by the loop's native code, which reads the elements back
    // as the interpreter does; the loop leaves only where a collection is due. Expected value
    // is Node's.
    let source = r#"
      function run(n) {
        var t = 0;
        for (var i = 0; i < n; i++) {
          var o = { v: i };
          var a = [i, o, i + 1, o, 7];
          t = (t + a[0] + a[1].v + a[2] + a[3].v + a[4] + a.length) | 0;
        }
        return t;
      }
      var s = 0;
      for (var k = 0; k < 200; k++) s = (s + run(300)) | 0;
      print(s);
    "#;
    for (policy, osr) in compiled_loop_diagnostics(source, "lane-array-literal-loop.js", "run", &["36660000"]) {
        assert!(osr.native_region_runs > 0 && osr.exits < osr.native_region_runs, "{policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn lane_loops_make_regexp_literals_natively_with_fresh_state() {
    // A regexp literal in a loop (`Allocation::RegExp`, `new_regexp_in_lane`) is a fresh object of
    // its site's template each iteration: matching one leaves the others' `lastIndex` alone,
    // and the loop leaves only where a collection is due. Expected value is Node's.
    let source = r#"
      function run(n) {
        var t = 0, re;
        for (var i = 0; i < n; i++) {
          re = /a(b+)c/g;
          t = (t + i) | 0;
        }
        return [re, t];
      }
      var first = run(300)[0], last, total = 0;
      for (var k = 0; k < 200; k++) { var made = run(300); last = made[0]; total = (total + made[1]) | 0; }
      first.test("xabbc");
      print([first.lastIndex, last.lastIndex, first !== last, last.source, last.flags, last.exec("xabbbc")[1], total].join(" "));
    "#;
    for (policy, osr) in compiled_loop_diagnostics(source, "lane-regexp-literal-loop.js", "run", &["5 0 true a(b+)c g bbb 8970000"]) {
        assert!(osr.native_region_runs > 0 && osr.exits < osr.native_region_runs, "{policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn compiled_loops_answer_regexp_argument_literals_as_the_interpreter_does() {
    // A literal argument of `replace` in a loop the compiled tiers run (`Allocation::RegExp`
    // with its callee, `lane_new_regexp`) answers through `Vm::regexp_literal_argument_of`:
    // shared while the callee is the realm's String method, a fresh object for any other
    // callee, which may keep and change it. Expected value is Node's.
    let source = r#"
      function run(n, s) {
        var t = 0, last;
        for (var i = 0; i < n; i++) {
          last = s.replace(/a/g, '');
          t = (t + i) | 0;
        }
        return [last, t];
      }
      var total = 0, made;
      for (var k = 0; k < 200; k++) { made = run(300, 'banana'); total = (total + made[1]) | 0; }
      var holder = { replace: function (re) { return re; } };
      var first = run(300, holder)[0], second = run(300, holder)[0];
      first.lastIndex = 3;
      print([made[0], total, first !== second, second.lastIndex, run(5, 'qaqa')[0]].join(" "));
    "#;
    // Only the answer is checked: whether the loop's call of `replace` stays native is the
    // call tiers' concern.
    let _ = compiled_loop_diagnostics(source, "lane-regexp-argument-loop.js", "run", &["bnn 8970000 true 0 qq"]);
}

/// Callees whose cell holds the stub (no body yet, a declined or a retired one) run in the
/// lane and agree with the interpreter, as do those that change from a body to the stub when
/// their shapes change under a caller that keeps calling.
#[test]
fn interpreter_calls_of_stub_callees_match_the_interpreter() {
    let source = r#"(function () {
      function cold(a) { return a + 1; }
      function hot(a) { let s = 0; for (let i = 0; i < 3; i++) { s = (s + a.x + i) | 0; } return s; }
      function thrower(a) { if (a.x === 7777) throw new Error("t"); return a.x; }
      const shapes = [{ x: 1 }, { y: 0, x: 2 }, { z: 0, y: 0, x: 3 }, { x: 4, w: 1 }];
      let total = 0, caught = 0;
      for (let round = 0; round < 6000; round++) {
        const o = shapes[(round >> 6) & 3];
        total = (total + hot(o) + cold(round & 15)) & 1048575;
        if (round % 1000 === 999) { o.x = 7777; try { thrower(o); } catch (e) { caught++; } o.x = round & 7; }
        else total = (total + thrower(o)) & 1048575;
      }
      globalThis.answer = total * 8 + caught;
    })();"#;
    assert_cell_crossings_match(source);
}

/// A body entered through its cell that leaves at its first instruction (its receiver's shape
/// or type no longer the one it proved) resumes in the activation the call would have pushed:
/// the interpreter runs the whole callee, including the throw of a `null` argument.
#[test]
fn cell_entered_bodies_leaving_at_their_first_instruction_match_the_interpreter() {
    assert_cell_crossings_match(r#"(function () {
      function first(o) { return o.x * 2 + 1; }
      const shapes = [{ x: 1 }, { y: 0, x: 2 }, { x: 3, z: 0 }, "text", { get x() { return 5; } }, null];
      let total = 0, caught = 0;
      for (let round = 0; round < 8000; round++) {
        const o = shapes[round < 6000 ? 0 : (round - 6000) % shapes.length];
        try { total = (total + first(o)) | 0; } catch (e) { caught++; }
      }
      globalThis.answer = total * 16 + caught;
    })();"#);
}

/// A body that leaves in the middle (a loop whose later element is no number) resumes at the
/// record it names, in the registers the body wrote, with the loop's partial sum.
#[test]
fn cell_entered_bodies_leaving_in_the_middle_match_the_interpreter() {
    assert_cell_crossings_match(r#"(function () {
      function sum(a) { let s = 0; for (let i = 0; i < a.length; i++) { s = (s + a[i]) | 0; } return s; }
      const ints = [1, 2, 3, 4, 5, 6, 7, 8], mixed = [1, 2, 3, "4", 5, undefined, 7, 8.5];
      let total = 0;
      for (let round = 0; round < 6000; round++) {
        total = (total + sum(round < 5000 || round % 3 ? ints : mixed)) & 1048575;
      }
      globalThis.answer = total;
    })();"#);
}

/// A body that leaves inside a call it inlined resumes in the callee's activation made on top
/// of the activation of the call the interpreter made into the body.
#[test]
fn cell_entered_bodies_leaving_inside_an_inlined_call_match_the_interpreter() {
    assert_cell_crossings_match(r#"(function () {
      function inner(o) { return o.v + 1; }
      function outer(o) { return inner(o) + inner(o) * 2; }
      const plain = { v: 3 }, other = { w: 0, v: 4 }, odd = { v: "5" };
      let total = 0;
      for (let round = 0; round < 8000; round++) {
        total = (total + outer(round < 6000 ? plain : round % 2 ? other : odd)) | 0;
      }
      globalThis.answer = total;
    })();"#);
}

/// A body whose helper call needs an activation under it (a call it does not inline) takes the
/// activation the crossing deferred first, and the callee returns into it.
#[test]
fn cell_entered_bodies_calling_through_helpers_match_the_interpreter() {
    assert_cell_crossings_match(r#"(function () {
      const callees = [(a) => a + 1, (a) => a * 2, function (a) { return this === undefined ? a : a - 1; }];
      function dispatch(k, a) { let s = a; for (let i = 0; i < 3; i++) { s = (callees[(k + i) % 3](s)) | 0; } return s; }
      let total = 0;
      for (let round = 0; round < 6000; round++) { total = (total + dispatch(round % 7, round & 15)) & 1048575; }
      globalThis.answer = total;
    })();"#);
}

/// A forwarded call (`Function.prototype.call`) of a callee with a body crosses into it too.
#[test]
fn forwarded_calls_cross_into_cell_bodies() {
    assert_cell_crossings_match(r#"(function () {
      function step(a, b) { let s = 0; for (let i = 0; i < 4; i++) { s = (s + a * i + b) | 0; } return s; }
      let total = 0;
      for (let round = 0; round < 8000; round++) { total = (total + step.call(null, round & 7, round & 3)) & 1048575; }
      globalThis.answer = total;
    })();"#);
}

pub(super) const LANE_METHOD_SITES_SOURCE: &str = r#"(function () {
  const MASK = 65535;
  function Cell(id) { this.id = id; this.sum = 0; this.peer = null; }
  Cell.prototype.add = function (a, b) { this.sum = (this.sum + a * 3 + b) & MASK; return this.sum; };
  Cell.prototype.pair = function (a, b) { return (this.add(b, a) + this.add(a, b)) & MASK; };
  Cell.prototype.far = function (p, q, r) { return (p - q + r * 2) & MASK; };
  Cell.prototype.drive = function drive(n, other) {
    let total = 0;
    for (let i = 0; i < n; i++) {
      const x = i & 255, y = (i * 7) & 127, z = (i * 3) & 63;
      total = (total + this.pair(x, y) + this.add(z, x) + this.add(y, z)) & MASK;
      total = (total + other.add(x, y) + other.far(z, y, x) + this.far(x, x, z)) & MASK;
    }
    return total;
  };
  let sum = 0;
  const cell = new Cell(1), other = new Cell(2);
  for (let round = 0; round < 400; round++) {
    sum = (sum + cell.drive(48, other) + cell.add(round, sum & 7)) & MASK;
  }
  globalThis.answer = sum + cell.sum + other.sum;
})();"#;
