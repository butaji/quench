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

/// An instruction with no row in the middle of the function leaves the body at its PC, and the
/// interpreter resumes there with the registers the rows computed.
#[test]
fn an_uncovered_instruction_in_the_middle_leaves_and_resumes() {
    let (interpreted, compiled, [runs, returns]) = run_both(
        "function g(a, b) { let s = 0; for (let i = 0; i < b; i++) { s = s + a * i; } const m = Math.floor(s / 3); let r = m; for (let j = 0; j < 4; j++) r = r + j * a; return r + a; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + g(k, 9)) | 0;
         print(t); print(g(7, 11)); print(g(-5, 4)); print(g(1.5, 6));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100 && returns < runs, "the body left {} of {runs} runs", runs - returns);
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

/// An array's `length` is read from its record in the body, at each test of the loop; the
/// interpreter's answer for an array with extras (an arguments object) or a non-array base is
/// the instruction's own, which a row leaves for.
#[test]
fn array_length_runs_in_a_body() {
    assert_bodies_never_leave(
        "function sum(a) { let s = 0; for (let i = 0; i < a.length; i++) s = s + a[i]; return s; }
         const xs = [1, 2, 3, 4, 5, 6, 7, 8]; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + sum(xs)) | 0;
         print(t); print(sum([])); print(sum([5]));",
    );
    let (interpreted, compiled, _) = run_both(
        "function n(a) { return a.length; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + n([k, 2])) | 0;
         print(t); print(n('hello')); print(n({ length: 4 })); print(n(function (a, b) {}));
         print((function () { return n(arguments); })(1, 2, 3));",
    );
    assert_eq!(interpreted, compiled);
}

/// A depth-0 capture's store runs in the body, into the slots the activation's capture word says
/// admit every direct store (the closure's environment is already in the remembered set); a read
/// after it sees the write, and the interpreter's counter agrees.
#[test]
fn capture_writes_run_in_a_body() {
    assert_bodies_never_leave(
        "function counter() { let n = 0; return function () { n = n + 1; return n; }; }
         const next = counter(); let t = 0;
         for (let k = 0; k < 300; k++) t = (t + next()) | 0;
         print(t); print(next());",
    );
}

/// A field the receiver's prototype holds is read through the site's cache entry, which names the
/// holder: the receiver's shape and prototype link and the holder's shape are the entry's, and the
/// value is the holder's, read at each use, so a prototype whose field changes between runs reads
/// the new value in the body as in the interpreter.
#[test]
fn prototype_held_fields_run_in_a_body() {
    assert_bodies_never_leave(
        "const proto = { scale: 3 }; const o = Object.create(proto); o.x = 2;
         function area(p) { return p.x * p.scale; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + area(o)) | 0;
         print(t); proto.scale = 5; print(area(o));",
    );
}

/// A nested lookup (`a.b.c`, which the compiler fuses into one instruction over two cache sites)
/// is one row that reads the second entry's field of the object the first entry reads; a
/// third level is another row over the first's result.
#[test]
fn nested_field_chains_run_in_a_body() {
    assert_bodies_never_leave(
        "function deep(a) { return a.inner.x + a.inner.y + a.inner.core.z; }
         const o = { inner: { x: 3, y: 4, core: { z: 5 } } }; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + deep(o)) | 0;
         print(t);",
    );
}

/// An own data field's store runs through the site's cache entry (packed int32 slots here), under
/// the store gate and with no method cache naming the field; and the same stores on flat storage,
/// of doubles, print what the interpreter prints.
#[test]
fn property_writes_run_in_a_body() {
    assert_bodies_never_leave(
        "function bump(p, d) { p.x = p.x + d; p.y = p.y - d; return p.x + p.y; }
         const pt = { x: 1, y: 2 }; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + bump(pt, 1)) | 0;
         print(t); print(pt.x); print(pt.y);",
    );
    let (interpreted, compiled, _) = run_both(
        "function scale(o, f) { o.a = o.a * f; o.b = o.b + 1; o.c = o.c + 2; o.d = o.d * f; return o.b + o.c; }
         const q = { a: 1, b: 3, c: 4, d: 2 }; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + scale(q, 1.5)) | 0;
         print(t); print(q.a); print(q.d); print(scale({ a: 'x', b: 1, c: 2, d: 3 }, 2));",
    );
    assert_eq!(interpreted, compiled);
}

/// `this.x` reads and stores run in a method's body through the site entries, the receiver the
/// body's entry publishes to the payload; a method of a class reads its instance the same way.
#[test]
fn this_fields_run_in_a_method_body() {
    assert_bodies_never_leave(
        "const counter = { n: 0, step: 2, bump() { this.n = this.n + this.step; return this.n; } };
         class Acc { constructor(s) { this.total = 0; this.s = s; } add(x) { this.total = this.total + x * this.s; return this.total; } }
         const acc = new Acc(3); let t = 0;
         for (let k = 0; k < 300; k++) t = (t + counter.bump() + acc.add(k)) | 0;
         print(t); print(counter.n); print(acc.total);",
    );
}
