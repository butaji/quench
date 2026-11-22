//! Returning instructions and call forms: a body completes the activation as the interpreter does.

use super::{assert_bodies_never_leave, run_both};

/// An instruction that carries the return flag (`return a + b`, `-x`, `o.p`, `typeof v`, `a < b`,
/// `f(x)`) is its row followed by the completion with its result: the body never leaves, and the
/// result is the interpreter's.
#[test]
fn returning_instructions_complete_the_activation_in_a_body() {
    assert_bodies_never_leave(
        "function sum(a, b) { return a + b; }
         function neg(x) { return -x; }
         function pick(o) { return o.p; }
         function kind(v) { return typeof v; }
         function less(a, b) { return a < b; }
         function twice(x) { return sum(x, x); }
         const o = { p: 7 }; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + sum(k, 1) + neg(k % 5) + pick(o) + kind(k).length + (less(k, 100) ? 1 : 0) + twice(k % 7)) | 0;
         print(t); print(sum(2, 3)); print(neg(4)); print(kind(1)); print(less(1, 2));",
    );
}

/// A returning instruction whose row misses (an overflow, a string operand, a getter) leaves at the
/// instruction with its flag, and the interpreter completes the function with the value.
#[test]
fn a_returning_instruction_that_misses_completes_in_the_interpreter() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function sum(a, b) { return a + b; }
         function pick(o) { return o.p; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + sum(k, 1) + pick({ p: k })) | 0;
         print(t); print(sum(2147483647, 1)); print(sum('a', 1)); print(sum(0.5, 0.25));
         print(pick({ get p() { return 'got'; } })); print(pick({}));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// The call forms beyond the plain call (a method call, a call through `this`, a spread, an applied
/// list, a forwarded `arguments`, a construction and a name call of a global) are the call planner's
/// chains: the body runs them in place and continues with the registers they wrote, printing what the
/// interpreter prints.
#[test]
fn every_call_form_runs_in_a_body() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function add(a, b) { return a + b; }
         const obj = { base: 2, m(x) { return x + this.base; }, n() { return this.m(1); } };
         class P { constructor(v) { this.v = v; } get() { return this.v; } }
         class Q extends P { constructor(v) { super(v + 1); this.w = 2; } }
         function forwards() { return add.apply(null, arguments); }
         function forms(i, list) {
           let s = obj.m(i) + obj.n() + add(...list) + add(i, ...list) + forwards(i, 1);
           s = s + new P(i).get() + new Q(i).get() + Math.max(i, 3);
           return s | 0;
         }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + forms(k % 7, [1, 2])) | 0;
         print(t); print(forms(0, [0, 0])); print(forms(2.5, [1, 2]));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A call form that raises, a constructor that returns an object, and a spread of a non-iterable
/// end as the interpreter's do: the same exception, caught by the same handler.
#[test]
fn a_call_form_that_raises_ends_as_the_interpreter_does() {
    let (interpreted, compiled, _) = run_both(
        "function boom(x) { if (x > 250) throw new Error('big ' + x); return x; }
         const o = { go(x) { return boom(x); } };
         function ctor(x) { return new (function () { this.v = boom(x); })().v; }
         function spread(x) { return Math.max(...x); }
         let t = 0; for (let k = 0; k < 200; k++) t = (t + o.go(k) + ctor(k) + spread([k, 1])) | 0;
         print(t);
         try { o.go(300); } catch (e) { print(e.message); }
         try { ctor(400); } catch (e) { print(e.message); }
         try { spread(5); } catch (e) { print(e.constructor.name); }",
    );
    assert_eq!(interpreted, compiled);
}

/// A returning construction and a returning method call complete the activation with their result.
#[test]
fn returning_call_forms_complete_the_activation() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "class P { constructor(v) { this.v = v; } }
         const obj = { m(x) { return x * 2; } };
         function make(v) { return new P(v); }
         function call(v) { return obj.m(v); }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + make(k).v + call(k)) | 0;
         print(t); print(make(3).v); print(call(4));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A constant that is no Number (a string, `undefined`, `null`, a boolean) is the row that writes its
/// word, an `InitializeTdz` of a frame-owned slot and a store that initializes one are the row over
/// the activation's locals with the helper step behind it as the slow arm: each has its fast row in
/// the bodies compiled, and a read of a binding in its temporal dead zone still raises.
#[test]
fn non_number_constants_and_local_initializations_are_fast_rows() {
    use quench_runtime::bytecode::Op as Opcode;
    let ops = [Opcode::LoadConst, Opcode::InitializeTdz, Opcode::StoreLocal];
    let before = ops.map(crate::compiled::base_region::fast_rows);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function tdz(n) { let s = 0; for (let i = 0; i < n; i++) { try { s = s + z; let z = 1; } catch (e) { s = s + 1; } } return s; }
         function consts(n) { let k = 'k', u = undefined, w = null, b = false, c = 0; for (let i = 0; i < n; i++) { k = k === 'k' ? 'j' : 'k'; b = !b; c = c + (u === undefined ? 1 : 0) + (w === null ? 2 : 0); } return k + b + c; }
         let t = 0; for (let m = 0; m < 300; m++) t = (t + tdz(m % 5) + consts(m % 4).length) | 0;
         print(t); print(tdz(3)); print(consts(3));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    for (op, before) in ops.into_iter().zip(before) {
        assert!(crate::compiled::base_region::fast_rows(op) > before, "{op:?} has no fast row");
    }
}

/// `typeof v === 'name'` and `switch (typeof v)` are a fast row over the tag of the operand, which leaves
/// into the interpreter's step for a heap value (a string, symbol, BigInt, object, function): the same
/// answers for every kind of value, whether the result is returned, branched on or switched on.
#[test]
fn typeof_tests_and_indexes_are_fast_rows() {
    use quench_runtime::bytecode::Op;
    let before = crate::compiled::base_region::fast_rows(Op::Unary);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function is(v) { return typeof v === 'number'; }
         function kind(v) { switch (typeof v) { case 'number': return 1; case 'string': return 2; case 'undefined': return 3; default: return 4; } }
         function mix(v) { return (typeof v !== 'boolean' ? 1 : 0) + (typeof v === 'object' ? 2 : 0) + (typeof v === 'function' ? 4 : 0); }
         function counted(n) { let c = 0; for (let i = 0; i < n; i++) { if (typeof i === 'number') c = c + 1; } return c; }
         const vs = [1, 2.5, 'a', undefined, null, true, {}, function () {}, Symbol(), 1n]; let t = 0;
         for (let k = 0; k < 300; k++) { const v = vs[k % 10]; t = (t + (is(v) ? 1 : 0) + kind(v) + mix(v) + counted(k % 4)) | 0; }
         print(t); print(is(3)); print(kind('s')); print(mix(null)); print(mix(undefined)); print(counted(5));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(crate::compiled::base_region::fast_rows(Op::Unary) > before, "no typeof fast row");
}
