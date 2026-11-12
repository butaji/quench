//! The forms a whole-function body makes in place: each has its row (or its helper step), and a body
//! that holds it runs as the interpreter does, with the interpreter's step behind a fast arm.
use super::tests::run_both;
use crate::bytecode::Op;
use crate::vm::compiled::base_region::fast_rows;

/// A read of a binding in its temporal dead zone, and an assignment that is no initialization, leave a fast row
/// into the interpreter's step, which raises; an initialized binding is read and written by the row.
#[test]
fn local_reads_and_assignments_check_their_temporal_dead_zone_in_a_row() {
    let ops = [Op::LoadLocal, Op::StoreLocal];
    let before = ops.map(fast_rows);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function reads(n) { let s = 0; for (let i = 0; i < n; i++) { let a = i; s = s + a; { let b = s; b = b + 1; s = b; } } return s; }
         function dead(n) { let s = 0; for (let i = 0; i < n; i++) { try { s = s + z; } catch (e) { s = s + 1; } try { z = 2; } catch (e) { s = s + 2; } let z = 1; } return s; }
         let t = 0; for (let m = 0; m < 300; m++) t = (t + reads(m % 6) + dead(m % 4)) | 0;
         print(t); print(reads(4)); print(dead(3));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    for (op, before) in ops.into_iter().zip(before) {
        assert!(fast_rows(op) > before, "{op:?} has no fast row");
    }
}

/// `typeof v` as a string is the entry of the VM's table of result strings its tag selects, so the strings are
/// the ones every `typeof` and every string constant spelling one yield (identity under `===`), for every kind of
/// value; a heap value leaves the row for the interpreter's step.
#[test]
fn typeof_strings_are_the_interned_table_entries() {
    let before = fast_rows(Op::Unary);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function name(v) { return typeof v; }
         function same(v) { const s = typeof v; return s === 'number' || s === 'undefined' || s === 'object'; }
         function ident(v) { return typeof v === typeof name; }
         function counted(n) { let c = ''; for (let i = 0; i < n; i++) c = typeof i; return c; }
         const vs = [1, 2.5, 'a', undefined, null, true, {}, function () {}, Symbol(), 1n]; let t = 0, s = '';
         for (let k = 0; k < 300; k++) { const v = vs[k % 10]; s = name(v); t = (t + s.length + (same(v) ? 1 : 0) + (ident(v) ? 2 : 0) + counted(k % 3).length) | 0; }
         print(t); print(s); print(name(3)); print(name(null)); print(name(() => 1)); print(counted(2));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(fast_rows(Op::Unary) > before, "no typeof string fast row");
}

/// `super()` binds the derived constructor's `this` and the rows after it read the receiver it bound: the helper
/// step that binds the frame's `this` publishes the same receiver to the payload the rows read.
#[test]
fn initialize_this_publishes_the_receiver_to_the_rows_after_it() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "class A { constructor(n) { this.a = n; } }
         class B extends A { constructor(n) { let s = 0; for (let i = 0; i < n; i++) s = s + i; super(s); this.b = this.a + 1; this.c = this.b * 2; } }
         let t = 0; for (let k = 0; k < 300; k++) { const o = new B(k % 5); t = (t + o.a + o.b + o.c) | 0; }
         print(t); const q = new B(4); print(q.a); print(q.b); print(q.c);",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 0, "no body ran");
}

/// A ten-deep chain of compiled calls whose inner callees make steps the helper runs in place (`instanceof`,
/// `in`, a `typeof` string, a binary operator of a constant and a register): each runs as a native callee, where
/// the step helper links the frames and makes the step from the activation's registers, so no callee's body is
/// left at a step: every run of a body completes its activation (a leave, a helper's miss in a callee included, ends a
/// run without it), so the leaves the b-jit counted are zero.
#[test]
fn a_ten_deep_call_chain_makes_its_steps_in_the_callees() {
    let links: String = (1..=10).map(|depth| {
        let inner = if depth == 10 { "0".to_string() } else { format!("link{}(o, n)", depth + 1) };
        format!("function link{depth}(o, n) {{ const k = typeof o; return {inner} + (o instanceof Object ? 1 : 0) + ('p' in o ? 2 : 0) + k.length + (n % {depth}) + n * 3; }}\n")
    }).collect();
    let (interpreted, compiled, [runs, returns]) = run_both(&format!(
        "{links}let t = 0; const o = {{ p: 1 }};
         for (let k = 0; k < 400; k++) t = (t + link1(o, k)) | 0;
         print(t); print(link1(o, 7)); print(link1({{}}, 3));"
    ));
    assert_eq!(interpreted, compiled);
    assert!(runs > 1000, "the bodies ran {runs} times");
    assert_eq!(runs - returns, 0, "{} of {runs} body runs left for the interpreter", runs - returns);
}

/// A call that binds `this` (`super(...)` with an argument that is a call) and a direct `eval` are the helper's own
/// step made in place, not leaves: a body holding either runs as the interpreter does, the receiver `super()` bound
/// read by the rows after it and the eval's effect on the function's scope seen by the names after it.
#[test]
fn calls_that_bind_this_or_run_eval_are_helper_calls() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function twice(x) { return x * 2; }
         class A { constructor(n) { this.a = n; } }
         class B extends A { constructor(n) { super(twice(n) + 1); this.b = this.a + n; } }
         function ev(n) { let s = 0; for (let i = 0; i < n; i++) { s = s + eval('i + 1'); } return s; }
         let t = 0; for (let k = 0; k < 300; k++) { const o = new B(k % 4); t = (t + o.a + o.b + ev(k % 3)) | 0; }
         print(t); const q = new B(5); print(q.a); print(q.b); print(ev(4));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 0, "no body ran");
}

/// A binding the function's own environment holds (one a closure captures) is read by the row over the frame's
/// environment view, which the body's helpers reload; a dead zone or a slot outside the view leaves into the
/// helper step, which raises or answers.
#[test]
fn environment_local_reads_are_rows_over_the_environment_view() {
    let before = fast_rows(Op::LoadEnvLocal);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function counter(n) { let c = 0; let d = 5; const inc = () => { c = c + 1; return c; }; for (let i = 0; i < n; i++) { inc(); d = d + c; } return c + d; }
         function dead(n) { let s = 0; const f = () => z; for (let i = 0; i < n; i++) { try { s = s + z; } catch (e) { s = s + 1; } } let z = 2; return s + f(); }
         let t = 0; for (let m = 0; m < 300; m++) t = (t + counter(m % 6) + dead(m % 3)) | 0;
         print(t); print(counter(4)); print(dead(2));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(fast_rows(Op::LoadEnvLocal) > before, "LoadEnvLocal has no fast row");
}

/// An assignment to a binding the function's own environment holds (one a closure captures) is written by the row over
/// the environment view where the view admits every direct store, taken when the body is entered; a dead zone or a slot
/// outside the view leaves into the helper step, which raises or answers.
#[test]
fn environment_local_assignments_are_rows_over_the_environment_view() {
    let before = fast_rows(Op::StoreEnvLocal);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function counter(n) { let c = 0; let d = 5; const inc = () => { c = c + 1; return c; }; for (let i = 0; i < n; i++) { c = c + 2; d = d + c; inc(); } return c + d; }
         function dead(n) { let s = 0; const f = () => z; for (let i = 0; i < n; i++) { try { z = 1; } catch (e) { s = s + 1; } } let z = 2; return s + f(); }
         let t = 0; for (let m = 0; m < 300; m++) t = (t + counter(m % 6) + dead(m % 3)) | 0;
         print(t); print(counter(4)); print(dead(2));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(fast_rows(Op::StoreEnvLocal) > before, "StoreEnvLocal has no fast row");
}

/// A numeric operator whose result is a local slot (a function that keeps its locals in slots, here because it
/// evals) is the helper step's, which writes the frame's local the rows read, not a leave: every run of the body completes.
#[test]
fn a_numeric_operator_with_a_local_target_is_a_helper_step() {
    let (interpreted, compiled, [runs, returns]) = run_both(
        "function sum(n) { let s = 0; for (let i = 0; i < n; i++) { s = s + i * 2; } eval('0'); return s; }
         let t = 0; for (let m = 0; m < 300; m++) t = (t + sum(m % 7)) | 0;
         print(t); print(sum(5)); print(sum(0));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert_eq!(runs, returns, "{} of {runs} body runs left for the interpreter", runs - returns);
}
