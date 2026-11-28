//! The forms a whole-function body makes in place: each has its row (or its helper step), and a body
//! that holds it runs as the interpreter does, with the interpreter's step behind a fast arm.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::tests::{run_after, run_both, run_in};
use quench_runtime::bytecode::Op as Opcode;
use crate::compiled::base_region::fast_rows;
use crate::test_oracle::{assert_tier_agrees, Expect, Tier};

/// A read of a binding in its temporal dead zone, and an assignment that is no initialization, leave a fast row
/// into the interpreter's step, which raises; an initialized binding is read and written by the row.
#[test]
fn local_reads_and_assignments_check_their_temporal_dead_zone_in_a_row() {
    let ops = [Opcode::LoadLocal, Opcode::StoreLocal];
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
    let before = fast_rows(Opcode::Unary);
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
    assert!(fast_rows(Opcode::Unary) > before, "no typeof string fast row");
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

/// `v instanceof K` takes the general path (the helper's decision over the value and the constructor), and a write of the
/// constructor's `prototype` makes the next answer the interpreter's.
#[test]
fn instanceof_takes_the_general_path_and_sees_a_prototype_change() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "class A {} class B extends A {} class C {} function F() {}
         function is(v, K) { return v instanceof K; }
         function isA(v) { return v instanceof A; }
         function isF(v) { return v instanceof F; }
         const f = new F();
         const vs = [new A(), new B(), new C(), {}, [], 1, 'a', null, undefined, A, f];
         let t = 0;
         for (let k = 0; k < 600; k++) { const v = vs[k % 11]; t = (t + (is(v, A) ? 1 : 0) + (isA(v) ? 2 : 0) + (is(v, B) ? 4 : 0) + (isF(v) ? 8 : 0)) | 0; }
         print(t); print(isF(f)); print(is(f, F));
         F.prototype = {};
         print(isF(f)); print(isF(new F())); print(is(f, F));
         for (let k = 0; k < 600; k++) t = (t + (isF(vs[k % 11]) ? 1 : 0)) | 0;
         print(t);",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// `'k' in o` of a constant key that one receiver layout answered is a row over the object's exact shape and prototype
/// link (the boolean that layout answered: an own key, an inherited one or an absent one), with the interpreter's
/// answer for any other layout; a prototype that gains or loses the key retires the body that held the answer.
#[test]
fn in_of_a_constant_key_is_a_row_and_a_prototype_layout_change_retires_it() {
    let before = fast_rows(Opcode::Binary);
    let (interpreted, compiled, [runs, _]) = run_both(
        "class P { m() { return 1; } } class Q extends P {}
         function own(o) { return 'a' in o; }
         function inherited(o) { return 'm' in o; }
         function absent(o) { return 'z' in o; }
         const q = new Q(); q.a = 1; const plain = { a: 1 }; const bare = Object.create(null);
         let t = 0;
         for (let k = 0; k < 600; k++) t = (t + (own(q) ? 1 : 0) + (inherited(q) ? 2 : 0) + (absent(q) ? 4 : 0) + (own(q) ? 8 : 0)) | 0;
         print(t); print(own(bare)); print(inherited(bare)); print(absent(plain)); print(own([])); print(inherited(P.prototype));
         P.prototype.z = 1;
         print(absent(q)); print(absent(plain));
         delete P.prototype.m;
         print(inherited(q)); print(inherited(new Q()));
         for (let k = 0; k < 600; k++) t = (t + (absent(q) ? 1 : 0) + (inherited(q) ? 2 : 0)) | 0;
         print(t);",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(fast_rows(Opcode::Binary) > before, "no `in` row");
}

/// A function whose closures capture some of its locals runs as the lane activation of an environment window, and its body
/// reads and assigns those locals through the window's storage, in place before a closure makes the environment and in it
/// after (`ENV_ROW`, `ENV_SET_ROW`), and the others among the window's locals; a binding in its dead zone is the
/// interpreter's.
#[test]
fn an_environment_activation_runs_as_a_body_over_its_window() {
    let ops = [Opcode::LoadEnvLocal, Opcode::StoreEnvLocal];
    let before = ops.map(fast_rows);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function tally(n) {
           let c = 0, d = 0; c = c + 1; d = d + c;
           const bump = () => { c = c + 1; };
           for (let i = 0; i < n; i++) { bump(); d = d + c; c = c + 2; }
           return d + c;
         }
         function dead(n) { let s = 0; const f = () => z; for (let i = 0; i < n; i++) { try { s = s + z; } catch (e) { s = s + 1; } } let z = 1; return s + f(); }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + tally(k % 5) + dead(k % 3)) | 0;
         print(t); print(tally(4)); print(dead(2));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    for (op, before) in ops.into_iter().zip(before) {
        assert!(fast_rows(op) > before, "{op:?} has no fast row");
    }
}

/// A closure-heavy function whose let/const bindings closures capture runs its hot loop as one body in every tier: the
/// dead zone opens and each declaration defines its binding in the window's storage (`ENV_DEFINE_ROW`, or the u-jit's
/// staged rows), the reads and assignments go through it, and no run of the body leaves before it completes.
#[test]
fn environment_declarations_and_reads_run_in_a_body_without_leaving() {
    let source = "function tally(n) {
           let c = 0; const step = () => { c = c + 1; return c; };
           let d = 0;
           for (let i = 0; i < n; i++) { let e = i + 1; const f = e * 2; d = d + f; c = c + e; }
           return d + c + step();
         }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + tally(8 + k % 5)) | 0;
         print(t); print(tally(3));";
    assert_tier_agrees(source, Tier::Full, Expect { completed: 101, leaves: 0 });
}

/// A read of an environment binding in its dead zone raises the interpreter's `ReferenceError`, in every tier.
#[test]
fn an_environment_binding_in_its_dead_zone_raises_as_the_interpreter_does() {
    let source = "function dead(n) {
           const g = () => z; let s = 0;
           for (let i = 0; i < n; i++) { try { s = s + z; } catch (e) { s = s + (e instanceof ReferenceError ? 1 : 100); } }
           let z = 1; return s + g();
         }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + dead(k % 4)) | 0;
         print(t); print(dead(3));";
    let (interpreted, compiled, [runs, _]) = run_both(source);
    assert_eq!(interpreted, compiled);
    assert!(runs > 0, "no body ran");
}

/// `new` of a constructor whose locals closures capture activates it as an environment activation and runs its body.
#[test]
fn new_of_a_constructor_that_captures_variables_runs_as_a_body() {
    let source = "function Counter(n) { let c = n; this.bump = () => { c = c + 1; return c; }; for (let i = 0; i < n; i++) c = c + i; this.start = c; }
         class Box { constructor(n) { let v = n; this.get = () => v; for (let i = 0; i < n; i++) v = v + 1; this.v = v; } }
         let t = 0; for (let k = 0; k < 300; k++) { const a = new Counter(k % 5); const b = new Box(k % 4); t = (t + a.bump() + a.start + b.get() + b.v) | 0; }
         print(t); print(new Counter(3).bump()); print(new Box(2).get());";
    let (interpreted, compiled, [runs, _]) = run_both(source);
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// `'k' in o` as a branch is the comparison's row writing its boolean to a scratch register and the jump on it, as the
/// value forms are rows: the same answers the VM remembered, which leave at the jump for any other value and layout, where the
/// interpreter decides. `v instanceof A` as a branch takes the general path.
#[test]
fn in_jumps_are_rows_followed_by_the_branch() {
    let before = fast_rows(Opcode::JumpBinaryFalse);
    let (interpreted, compiled, [runs, _]) = run_both(
        "class A {} class B extends A {} function F() {}
         function pick(v) { if (v instanceof A) return 1; return 2; }
         function has(o) { if ('a' in o) return 4; return 8; }
         function both(v, o) { let s = 0; for (let i = 0; i < 3; i++) { if (v instanceof A) s = s + 1; if ('a' in o) s = s + 2; } return s; }
         const q = new B(); const p = { a: 1 };
         let t = 0;
         for (let k = 0; k < 600; k++) t = (t + pick(q) + has(p) + both(q, p)) | 0;
         print(t); print(pick(new F())); print(has({})); print(both({}, { a: 2 })); print(pick(q)); print(has(p));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(fast_rows(Opcode::JumpBinaryFalse) > before, "no `in` jump row");
}

/// `o.m(x)` and `this.m(x)` are a load of the method (`GetField`) followed by a plain call of the register that holds it, which is a direct
/// call through the callee's entry cell once the site recorded its callee: for one receiver layout, for two, and with the prototype's
/// method replaced after the body was built.
#[test]
fn a_method_call_of_a_cached_callee_is_a_direct_call_after_its_load() {
    let before = fast_rows(Opcode::Call);
    let (interpreted, compiled, [runs, returns]) = run_both(
        "class A { constructor(n) { this.n = n; } add(x) { return this.n + x; } twice(x) { return this.add(x) + this.add(1); } }
         function drive(o, n) { let s = 0; for (let i = 0; i < n; i++) s = (s + o.add(i) + o.twice(i)) | 0; return s; }
         const a = new A(3); let t = 0;
         for (let k = 0; k < 300; k++) t = (t + drive(a, 5)) | 0;
         print(t); print(drive(a, 7)); print(drive(new A(9), 4));
         print(drive({ n: 1, add(x) { return x; }, twice(x) { return x * 2; } }, 3));
         for (let k = 0; k < 300; k++) t = (t + drive(k % 2 ? a : { n: k, add(x) { return x + 2; }, twice(x) { return x * 3; } }, 4)) | 0;
         print(t);
         A.prototype.add = function (x) { return x - 1; };
         print(drive(a, 6));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100 && returns > 100, "the bodies ran {runs} times and completed {returns}");
    assert!(fast_rows(Opcode::Call) > before, "no direct call row");
}

/// A global lexical binding the declarative record retains (a finished script's top-level `let`) is read and written by a row over its word at
/// an address of the record, under the global layout: a new declaration, which moves the record, retires the body, and a name that is
/// no binding, or a binding in its dead zone, is the interpreter's.
#[test]
fn global_lexical_bindings_are_read_and_written_live_by_rows() {
    let ops = [Opcode::LoadName, Opcode::StoreName];
    let before = ops.map(fast_rows);
    let (interpreted, compiled, [runs, returns]) = run_after(
        &[("let counter = 0; let limit = 5;", "declarations.js")],
        "function bump(i) { counter = (counter + i) | 0; return counter + limit; }
         function absent() { return missing; }
         let t = 0;
         for (let k = 0; k < 600; k++) t = (t + bump(k % 7)) | 0;
         print(t); print(counter); print(limit);
         try { absent(); } catch (e) { print(e instanceof ReferenceError); }
         for (let k = 0; k < 600; k++) t = (t + bump(k % 5)) | 0;
         print(t); print(counter);",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100 && returns > 100, "the bodies ran {runs} times and completed {returns}");
    for (op, before) in ops.into_iter().zip(before) {
        assert!(fast_rows(op) > before, "{op:?} has no lexical binding row");
    }
}
