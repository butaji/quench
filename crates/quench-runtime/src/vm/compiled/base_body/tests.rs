use crate::{Engine, ExecutionMode, JitMode};
use crate::test_host::TestHost;

/// What a program prints, run in the interpreter and under `--b-jit`, and the runs of lane bodies
/// the second made (all, and those that completed their activation).
fn run_both(source: &str) -> (Vec<String>, Vec<String>, [usize; 2]) {
    let program = Engine::specialize(source, "base-body.js").unwrap();
    let printed = |mode: ExecutionMode| {
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = crate::vm::Vm::new(TestHost::writing_to(output.clone()));
        vm.set_execution_mode(mode);
        vm.programs.enable_js_lane_methods();
        vm.execute(&program).unwrap();
        output.take()
    };
    let interpreted = printed(ExecutionMode::Interpreted);
    super::super::LANE_BODY_RUNS.with(|runs| runs.set([0; 2]));
    let compiled = crate::jit_mode::with_mode(JitMode::BaseOnly, || printed(ExecutionMode::Compiled));
    (interpreted, compiled, super::super::LANE_BODY_RUNS.with(std::cell::Cell::get))
}

/// A loop function entered many times runs as a whole-function body, which completes its
/// activations, and prints what the interpreter does.
#[test]
fn a_loop_function_runs_as_a_body() {
    let (interpreted, compiled, [runs, returns]) = run_both(
        "function f(a, b) { let s = 0; for (let i = 0; i < b; i++) s = s + a * i; return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k, 20)) | 0;
         print(t); print(f(3, 5)); print(f(0.5, 8)); print(f(2147483647, 3)); print(f('3', 4));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100 && returns > 100, "the body ran {runs} times and completed {returns}");
}

/// An instruction the interpreter runs in the middle of the function (a native call through the
/// helper row) continues the body, whose rows go on with the registers the call left.
#[test]
fn a_native_call_in_the_middle_continues_the_body() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function g(a, b) { let s = 0; for (let i = 0; i < b; i++) { s = s + a * i; } const m = Math.floor(s / 3); let r = m; for (let j = 0; j < 4; j++) r = r + j * a; return r + a; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + g(k, 9)) | 0;
         print(t); print(g(7, 11)); print(g(-5, 4)); print(g(1.5, 6));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the body ran {runs} times");
}

/// A call of a script function, direct through its entry cell where its body exists and by the
/// helper row before, returns its result into the caller's register, which the caller's rows read.
#[test]
fn a_script_call_runs_in_a_body() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function sq(x) { return x * x; }
         function f(n) { let s = 0; for (let i = 0; i < n; i++) s = s + sq(i); return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k % 9)) | 0;
         print(t); print(f(12)); print(f(2.5));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A callee that raises, or one that leaves inside, ends the caller's body as the interpreter's
/// run does: the same output and the same exception.
#[test]
fn a_leave_inside_a_callee_ends_the_callers_body() {
    let (interpreted, compiled, _) = run_both(
        "function pick(x) { if (x > 250) throw new Error('big ' + x); return x + 1; }
         function f(n) { let s = 0; for (let i = 0; i < n; i++) s = s + pick(i); return s; }
         let t = 0; for (let k = 0; k < 200; k++) t = (t + f(k % 9)) | 0;
         print(t);
         try { f(300); } catch (e) { print(e.message); }",
    );
    assert_eq!(interpreted, compiled);
}

/// Recursion: each activation is its own body run or interpreter run, and the results agree.
#[test]
fn recursion_prints_what_the_interpreter_prints() {
    let (interpreted, compiled, _) = run_both(
        "function fib(n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); }
         print(fib(22)); print(fib(5)); print(fib(1.5));",
    );
    assert_eq!(interpreted, compiled);
}

/// Runs `source` and checks that every activation a body ran completed in it: the interpreter's
/// output, no leave, and enough runs to show bodies ran at all.
fn assert_bodies_never_leave(source: &str) {
    let (interpreted, compiled, [runs, returns]) = run_both(source);
    assert_eq!(interpreted, compiled);
    assert!(runs > 100 && returns == runs, "the body ran {runs} times and completed {returns}");
}

/// A field read through the site's cache entry runs in the body.
#[test]
fn property_reads_run_in_a_body() {
    assert_bodies_never_leave(
        "function norm(p) { return p.x * p.x + p.y * p.y; }
         const pt = { x: 3, y: 4 }; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + norm(pt)) | 0;
         print(t);",
    );
}

/// An element read, and one replaced in place, run in the body; the array is a parameter, so its
/// register is a tagged frame slot the rows read.
#[test]
fn element_reads_and_stores_run_in_a_body() {
    assert_bodies_never_leave(
        "function fill(a, n) { let s = 0; for (let i = 0; i < n; i++) { a[i] = i * 2; s = s + a[i]; } return s; }
         const xs = [0, 0, 0, 0, 0, 0, 0, 0]; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + fill(xs, 8)) | 0;
         print(t); print(xs[7]);",
    );
}

/// A depth-0 capture read runs in the body, the captures word the entry gets unpacked into the
/// payload for the run.
#[test]
fn capture_reads_run_in_a_body() {
    assert_bodies_never_leave(
        "function make(k) { return function (x) { return x * k + 1; }; }
         const f = make(3); let t = 0;
         for (let i = 0; i < 300; i++) t = (t + f(i)) | 0;
         print(t);",
    );
}

/// A `while` loop whose test and back edge are rows runs in the body, as does the `ToNumeric` of
/// a postfix update's old value; one that is no Number leaves at it, and the interpreter converts.
#[test]
fn loops_and_to_numeric_run_in_a_body() {
    assert_bodies_never_leave(
        "function down(n) { let c = 0; while (n > 0) { c = c + n--; } return c; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + down(k % 7)) | 0;
         print(t);",
    );
    let (interpreted, compiled, _) = run_both(
        "function down(n) { let c = 0; while (n > 0) { c = c + n--; } return c; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + down(k % 7)) | 0;
         print(t); print(down('5')); print(down(null)); print(down(2.5));",
    );
    assert_eq!(interpreted, compiled);
}

/// Branches back that are conditional (`continue`, `do`-`while`) poll the interrupt flag and
/// print what the interpreter prints.
#[test]
fn conditional_branches_back_print_what_the_interpreter_prints() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function odds(n) { let i = 0, c = 0; do { i = i + 1; if (i < 3) continue; c = c + i; } while (i < n); return c; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + odds(k % 11 + 1)) | 0;
         print(t); print(odds(40));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the body ran {runs} times");
}

/// A parameter dead at entry gets no entry guard: it may hold anything, and the body still runs.
#[test]
fn a_dead_parameter_is_not_guarded_at_entry() {
    assert_bodies_never_leave(
        "function f(a, junk) { junk = 1; return a + junk; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k, k % 2 ? 'x' : { y: 1 })) | 0;
         print(t);",
    );
}

/// `this`, the truthiness of a value (a branch and a negation) and a number's negation run as
/// rows in the body, and print what the interpreter prints for every kind of value.
#[test]
fn this_truthiness_and_negation_print_what_the_interpreter_prints() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function f(a, b) { let c = this ? 1 : 0; if (!a) c = c + 10; if (b) c = c + 100; return c + -a + (!b ? 1000 : 0); }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k % 5, k % 3)) | 0;
         print(t); print(f(0, 0)); print(f(2.5, '')); print(f(-0, 'x')); print(f(NaN, null)); print(f('7', 1)); print(f.call(null, 1, 2));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the body ran {runs} times");
}

/// A call of a small callee runs as the callee's one row in the caller's body. A guard that fails
/// inside it (an operand that is no Number) leaves at the call, where the interpreter makes the
/// whole call, so the output is the interpreter's for every kind of operand.
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
