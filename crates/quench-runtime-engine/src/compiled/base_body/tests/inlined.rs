//! Calls a body inlines: a callee's rows in the caller's body, its guards and its resumes.

use super::{assert_bodies_never_leave, run_both};

/// A call of a small callee runs as the callee's rows in the caller's body. A guard that fails
/// inside it (an operand that is no Number) leaves the body, so the output is the interpreter's
/// for every kind of operand.
#[test]
fn a_guard_failing_inside_an_inlined_callee_prints_what_the_interpreter_prints() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function add(a, b) { return a + b; }
         function f(n, x) { let s = 0; for (let i = 0; i < n; i++) s = s + add(i, x); return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k % 9, 2)) | 0;
         print(t); print(f(5, 1.5)); print(f(4, '1')); print(f(3, null)); print(f(2, { valueOf() { return 7; } }));
         print(f(3, 2147483647)); print(f(3, -0));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// The callee an inlined site was stitched for is proved at the call: a redefinition, and a
/// different function arriving through the same register, run as themselves.
#[test]
fn a_redefined_callee_is_not_run_as_the_inlined_one() {
    let (interpreted, compiled, _) = run_both(
        "var g = function (a) { return a + 1; };
         function f(n) { let s = 0; for (let i = 0; i < n; i++) s = s + g(i); return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k % 7)) | 0;
         print(t);
         g = function (a) { return a * 100; };
         print(f(5));
         g = (a) => -a;
         print(f(6));",
    );
    assert_eq!(interpreted, compiled);
}

/// A callee with window registers, a loop and two returns runs as rows of the caller's body: its
/// jumps are the site's own, each return moves to the call's result and jumps past the site, and
/// no call row is planned (the site is counted). The output is the interpreter's for every kind
/// of argument.
#[test]
fn a_callee_with_a_loop_and_two_returns_runs_inlined() {
    let before = crate::compiled::base_region::inlined_sites();
    let (interpreted, compiled, [runs, _]) = run_both(
        "function tri(n) { var s = 0, i = 0; while (i < n) { s = s + i; i = i + 1; } if (s > 1000) return 1000; return s; }
         function f(k) { var t = 0; for (var j = 0; j < 4; j++) t = t + tri(k + j); return t; }
         var t = 0; for (var k = 0; k < 300; k++) t = (t + f(k % 9)) | 0;
         print(t); print(f(70)); print(f(2.5)); print(f('3')); print(f(-4));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(crate::compiled::base_region::inlined_sites() > before, "the callee's site was inlined");
}

/// A guard that fails in the middle of an inlined callee (a double or a string where the loop
/// added ints, an int32 overflow after some iterations) leaves inside the callee: the interpreter
/// resumes there with the callee's registers as the body held them, and the parameter the callee
/// never writes (`x`) read from the caller's argument register, and finishes the loop. The output
/// is the interpreter's.
#[test]
fn a_guard_failing_mid_callee_resumes_in_the_callee() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function walk(n, x) { var s = 0, i = 0; while (i < n) { s = s + x; i = i + 1; } return s; }
         function f(n, x) { var t = 0; for (var j = 0; j < 3; j++) t = t + walk(n, x); return t; }
         var t = 0; for (var k = 0; k < 300; k++) t = (t + f(k % 7, 3)) | 0;
         print(t); print(f(9, 0.5)); print(f(40, 100000000)); print(f(5, '2')); print(f(4, null)); print(f(6, -0));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A callee a global holds is inlined under the global's Stable fact: no row proves it at the call,
/// and the bodies still complete every activation they run.
#[test]
fn a_global_callee_is_inlined_without_a_guard_and_completes() {
    let before = crate::compiled::base_region::inlined_sites();
    assert_bodies_never_leave(
        "function sq(x) { return x * x; }
         function f(n) { var s = 0; for (var i = 0; i < n; i++) s = s + sq(i); return s; }
         var t = 0; for (var k = 0; k < 300; k++) t = (t + f(k % 9)) | 0;
         print(t);",
    );
    assert!(crate::compiled::base_region::inlined_sites() > before, "the callee's site was inlined");
}

/// Redefining the global that held the inlined callee retires the body that folded it: the next
/// run calls the new function, and a call in the middle of a body that redefines it is called as
/// the interpreter calls it.
#[test]
fn redefining_a_global_callee_retires_the_body_that_inlined_it() {
    let (interpreted, compiled, _) = run_both(
        "function bump(a) { return a + 1; }
         function f(n) { var s = 0; for (var i = 0; i < n; i++) s = s + bump(i); return s; }
         function swap() { bump = function (a) { return a - 1; }; return 0; }
         function g(n) { var s = 0; for (var i = 0; i < n; i++) { s = s + bump(i); if (i == 2) swap(); } return s; }
         var t = 0; for (var k = 0; k < 300; k++) t = (t + f(k % 7)) | 0;
         print(t); print(f(6));
         for (var k = 0; k < 300; k++) t = (t + g(1)) | 0;
         print(g(6));
         bump = function (a) { return a * 100; };
         print(f(5));",
    );
    assert_eq!(interpreted, compiled);
}

/// A callee a closure holds is no global's: its site keeps the row that tests the callee register,
/// and another closure of the same function that holds another callee runs as itself.
#[test]
fn a_closure_held_callee_keeps_its_guard() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function inc(a) { return a + 1; }
         function dbl(a) { return a * 2; }
         function make(h) { return function (n) { var t = 0; for (var i = 0; i < n; i++) t = t + h(i); return t; }; }
         const f = make(inc), g = make(dbl);
         var t = 0; for (var k = 0; k < 300; k++) t = (t + f(k % 8)) | 0;
         print(t); print(g(7)); print(f(5)); print(g(4)); print(f(0.5));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// `return f(x)` is the call and the completion with its result: a chain of returning calls, the
/// innermost callee inlined, runs in bodies that never leave.
#[test]
fn returning_calls_run_in_a_body_and_chain() {
    assert_bodies_never_leave(
        "function leaf(x) { return x * 3 + 1; }
         function mid(x) { return leaf(x + 1); }
         function top(x) { return mid(x); }
         var t = 0; for (var k = 0; k < 300; k++) t = (t + top(k % 11)) | 0;
         print(t);",
    );
}

/// A returning call whose callee is not the one recorded, or whose argument is no int32, leaves at
/// the call and the interpreter makes it, so the output is the interpreter's.
#[test]
fn a_returning_call_that_misses_prints_what_the_interpreter_prints() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function twice(x) { return x + x; }
         function f(x) { return twice(x); }
         var t = 0; for (var k = 0; k < 300; k++) t = (t + f(k % 9)) | 0;
         print(t); print(f(0.25)); print(f('4')); print(f(2147483647));
         twice = function (x) { return -x; };
         print(f(5));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A returning call (`return g(a, b)`) whose callee inlines completes the caller with the result its
/// site moved to the call's result register: where that register is pinned (an int the callee's
/// constants make), the completion spills it to its frame slot first, as every completion does. The
/// comparator shape of test262 `TypedArray/sort_modifications.js`, whose sort read `undefined` back.
#[test]
fn a_returning_call_completes_with_the_inlined_callees_pinned_result() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function lt(a, b) { return a < b ? -1 : a > b ? 1 : 0; }
         function lt1(a, b) { let x; if (a < b) x = 1; else x = 0; return x; }
         var f = (function (g) { return function (a, b) { return g(a, b); }; })(lt);
         var h = (function (g) { return function (a, b) { return g(a, b); }; })(lt1);
         var t = 0; for (var i = 0; i < 400; i++) t = (t * 3 + f((i * 7) % 13, (i * 5) % 11) + h(i % 5, i % 3)) | 0;
         print(t); print(f(1, 2)); print(f(2, 1)); print(f(2, 2)); print(h(0, 1)); print(h(1, 0));
         var ta = new Int8Array(64); for (var k = 0; k < 64; k++) ta[k] = 64 - k;
         ta.sort(f); print(ta.join(','));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A call of `Math.sqrt` runs as its row: the callee's identity guarded (a replaced `Math.sqrt` runs as
/// itself), then the staged Math operation of a Number argument (any other argument leaves for the
/// interpreter's call). `-0`, negatives, infinities and coercing arguments print what the interpreter prints.
#[test]
fn a_math_sqrt_call_runs_as_its_row_under_an_identity_guard() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function f(xs) { let s = 0; for (let i = 0; i < xs.length; i++) s += Math.sqrt(xs[i]); return s; }
         function g(x) { return Math.sqrt(x); }
         let t = 0; for (let k = 0; k < 300; k++) t += f([k, k + 1, 4]) + g(k);
         print(t); print(f([4, 9, -1, 0, -0, 2.25, Infinity, '16', null, undefined, { valueOf() { return 25; } }]));
         print(Object.is(g(-0), -0)); print(g('x')); print(g(2));
         const old = Math.sqrt; Math.sqrt = (x) => x * 10; print(f([1, 2, 3])); print(g(5));
         Math.sqrt = old; print(f([1, 4, 9])); print(g(16));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}
