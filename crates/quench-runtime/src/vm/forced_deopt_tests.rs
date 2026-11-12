//! Forced deopt (R26): compile a program's functions with one guard forced to leave, for every
//! guard in turn, and the run must end exactly as the interpreter's does. A guard that leaves
//! wrongly shows as a different answer, so any mismatch is a bug in the projection that emitted
//! that guard.

use crate::test_host::TestHost;
use crate::{Engine, Value};
use super::super::{Vm, compiled::forced_miss};

/// The `answer` global after one run in `mode`.
fn answer(source: &str, name: &str, mode: crate::ExecutionMode) -> Option<f64> {
    let program = Engine::specialize(source, name).unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.programs.enable_js_lane_methods();
    vm.set_execution_mode(mode);
    vm.execute(&program).unwrap();
    let global = vm.realm.globals;
    let atom = vm.intern_atom("answer");
    vm.own_property(global, atom).and_then(Value::as_number)
}

/// Runs `source` with each of its first `limit` guards forced; returns (guards, mismatches).
pub(super) fn forced_everywhere(source: &str, name: &str, limit: usize) -> (usize, Vec<usize>) {
    let expected = answer(source, name, crate::ExecutionMode::Interpreted);
    forced_miss::arm(None);
    assert_eq!(answer(source, name, crate::ExecutionMode::Compiled), expected, "unforced compile");
    let guards = forced_miss::emitted();
    let mut mismatches = Vec::new();
    for forced in 0..guards.min(limit) {
        forced_miss::arm(Some(forced));
        if answer(source, name, crate::ExecutionMode::Compiled) != expected {
            mismatches.push(forced);
        }
    }
    forced_miss::arm(None);
    (guards, mismatches)
}

#[test]
fn every_forced_guard_leaves_exactly_as_the_interpreter_runs() {
    let source = r#"
     (function () {
      function Strength(v) { this.v = v; }
      Strength.stronger = function (a, b) { return a.v < b.v; };
      Strength.weakestOf = function (a, b) { return this.stronger(a, b) ? b : a; };
      Strength.LIMIT = 3;
      function pick(a, b) { return Strength.weakestOf(a, b).v + Strength.LIMIT; }
      var a = new Strength(1), b = new Strength(5), o = { p: 2, q: 3 }, arr = [1, 2, 3];
      let t = 0;
      for (let i = 0; i < 20000; i++) {
        t += pick(a, b) + o.p * o.q + arr[i & 1] + arr.length;
        if (i % 7 === 0) o.p = i & 3;
      }
      globalThis.answer = t;
     })();
    "#;
    let (guards, mismatches) = forced_everywhere(source, "forced-deopt-basic.js", 150);
    eprintln!("forced deopt: {guards} guards, {} forced, mismatches {mismatches:?}", guards.min(150));
    assert!(guards > 0, "the program compiled no guard");
    assert!(mismatches.is_empty(), "forced guards {mismatches:?} changed the answer");
}

/// Programs by the projections they stress: construction and prototype reads, captured
/// variables and closures, array literals with string keys, polymorphic calls.
const PROGRAMS: &[(&str, &str)] = &[
    ("forced-deopt-construct.js", r#"
     (function () {
      function Vec(x, y) { this.x = x; this.y = y; this.len = 0; }
      Vec.prototype.add = function (o) { return new Vec(this.x + o.x, this.y + o.y); };
      Vec.prototype.dot = function (o) { return this.x * o.x + this.y * o.y; };
      class P extends Vec { constructor(x, y) { super(x, y); this.z = x - y; } }
      let t = 0, a = new Vec(1, 2), b = new P(3, 4);
      for (let i = 0; i < 20000; i++) {
        const c = (i & 1 ? a : b).add(a);
        t += c.dot(b) + (c.z || 0) + (i % 5 === 0 ? new P(i, 1).z : 0);
      }
      globalThis.answer = t;
     })();
    "#),
    ("forced-deopt-closures.js", r#"
     (function () {
      function counter(step) { let n = 0; return { inc() { n += step; return n; }, get() { return n; } }; }
      const cs = [counter(1), counter(2), counter(3)];
      let t = 0, shared = 5;
      const bump = () => { shared += 1; return shared; };
      for (let i = 0; i < 20000; i++) {
        const c = cs[i % 3];
        t += c.inc() % 11 + bump() % 7 + (i < 15000 ? i : 'k'.length);
      }
      globalThis.answer = t + cs[0].get();
     })();
    "#),
    ("forced-deopt-polymorphic.js", r#"
     (function () {
      const shapes = [{ k: 1, v: 2 }, { v: 3, k: 4 }, { k: 5, w: 1, v: 6 }, [7, 8], 'str', 9];
      function read(o) { return (o.k || 0) + (o.v || 0) + (o.length || 0); }
      let t = 0;
      for (let i = 0; i < 20000; i++) {
        t += read(shapes[i % (i < 10000 ? 3 : 6)]);
        if (i % 1000 === 0) shapes[0].k = i;
      }
      globalThis.answer = t;
     })();
    "#),
];

#[test]
fn forced_guards_hold_across_the_corpus() {
    let mut total = (0, 0);
    for (name, source) in PROGRAMS {
        let (guards, mismatches) = forced_everywhere(source, name, 250);
        eprintln!("forced deopt {name}: {guards} guards, {} forced, mismatches {mismatches:?}", guards.min(250));
        total = (total.0 + guards, total.1 + mismatches.len());
        assert!(mismatches.is_empty(), "{name}: forced guards {mismatches:?} changed the answer");
    }
    eprintln!("forced deopt corpus: {} guards, {} mismatches", total.0, total.1);
}

/// Generated programs (`research/tools/gen-tiers.py`, thread 16): mixed argument types and a
/// shape change midway, each setting `answer`.
const GENERATED: &[(&str, &str)] = &[
    ("g7_0000.js", include_str!("forced_deopt_programs/g7_0000.js")),
    ("g7_0001.js", include_str!("forced_deopt_programs/g7_0001.js")),
    ("g7_0002.js", include_str!("forced_deopt_programs/g7_0002.js")),
    ("g7_0003.js", include_str!("forced_deopt_programs/g7_0003.js")),
    ("g7_0004.js", include_str!("forced_deopt_programs/g7_0004.js")),
    ("g7_0005.js", include_str!("forced_deopt_programs/g7_0005.js")),
    ("g7_0006.js", include_str!("forced_deopt_programs/g7_0006.js")),
    ("g7_0007.js", include_str!("forced_deopt_programs/g7_0007.js")),
    ("g7_0008.js", include_str!("forced_deopt_programs/g7_0008.js")),
    ("g7_0009.js", include_str!("forced_deopt_programs/g7_0009.js")),
    ("g7_0010.js", include_str!("forced_deopt_programs/g7_0010.js")),
    ("g7_0011.js", include_str!("forced_deopt_programs/g7_0011.js")),
];

/// 68 s: run with `--ignored`.
#[test]
#[ignore]
fn forced_guards_hold_across_generated_programs() {
    let mut total = (0, 0, 0);
    for (name, source) in GENERATED {
        let (guards, mismatches) = forced_everywhere(source, name, 250);
        eprintln!("forced deopt {name}: {guards} guards, {} forced, mismatches {mismatches:?}", guards.min(250));
        total = (total.0 + guards, total.1 + guards.min(250), total.2 + mismatches.len());
        if !mismatches.is_empty() { eprintln!("MISMATCH {name}: guards {mismatches:?}"); }
    }
    eprintln!("forced deopt generated: {} guards, {} forced, {} mismatches", total.0, total.1, total.2);
    assert_eq!(total.2, 0);
}
