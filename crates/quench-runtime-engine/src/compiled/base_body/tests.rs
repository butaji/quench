#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::Engine;
use quench_runtime::test_host::TestHost;
use crate::test_oracle::{assert_tier_agrees_after, Expect, Tier};
use quench_runtime::vm::tier_events::Event;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

#[path = "tests/inlined.rs"]
pub(crate) mod inlined;
#[path = "tests/returns.rs"]
pub(crate) mod returns;

/// What a program prints, run in the interpreter and compiled, and the runs of lane bodies
/// the second made (all, and those that completed their activation).
pub(crate) fn run_both(source: &str) -> (Vec<String>, Vec<String>, [usize; 2]) {
    run_after(&[], source)
}

/// `run_both` of `source` after the global scripts `scripts` (source and name) ran in the same realm: what they declare
/// is retained in the realm's records when `source` runs. The oracle has the tiers agree and engage; the printed lines
/// come twice, as the two runs share them.
pub(crate) fn run_after(scripts: &[(&str, &str)], source: &str) -> (Vec<String>, Vec<String>, [usize; 2]) {
    let run = assert_tier_agrees_after(scripts, source, Tier::Full, Expect::ENGAGED);
    let runs = [Event::Entered, Event::Returned].map(|event| run.events.count(event) as usize);
    (run.outcome.printed.clone(), run.outcome.printed, runs)
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
         function drive() { const f = make(3); let t = 0;
         for (let i = 0; i < 300; i++) t = (t + f(i)) | 0;
         return t; }
         print(drive());",
    );
}

/// The same loop as `capture_reads_run_in_a_body` in a Script's root runs whole in the root's
/// loop region: the root's block bindings are registers, its global declarations environment
/// slots, and nothing leaves.
#[test]
fn a_script_root_loop_over_a_closure_runs_whole_in_a_region() {
    let run = assert_tier_agrees_after(
        &[],
        "function make(k) { return function (x) { return x * k + 1; }; }
         const f = make(3); let t = 0;
         for (let i = 0; i < 300; i++) t = (t + f(i)) | 0;
         print(t);",
        Tier::Full,
        Expect::ENGAGED,
    );
    assert_eq!(run.events.count(Event::OsrLeft), 0);
    assert_eq!(run.events.count(Event::LeftAt), 0);
    assert!(run.events.count(Event::OsrEntered) >= 1);
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
         function drive(next) { let t = 0;
         for (let k = 0; k < 300; k++) t = (t + next()) | 0;
         return t; }
         const next = counter(); print(drive(next)); print(next());",
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

/// A loop region stores an own field through a row and reads the field it just wrote from the
/// site's entry, not from the entry record's snapshot, which the store would stale; a capture store
/// in a closure's loop writes the activation's own slots. The output is the interpreter's.
#[test]
fn loop_regions_store_fields_and_captures_without_a_stale_snapshot() {
    let (interpreted, compiled, _) = run_both(
        "function fill(o, n) { for (let k = 0; k < n; k++) { o.count = o.count + 1; o.last = o.count + o.step; } return o.last; }
         const o = { count: 0, last: 0, step: 5 };
         print(fill(o, 600)); print(o.count); print(fill(o, 600)); print(o.last);
         function make() { let acc = 0; return function (n) { for (let k = 0; k < n; k++) { acc = (acc + k) | 0; } return acc; }; }
         const f = make(); print(f(500)); print(f(500));",
    );
    assert_eq!(interpreted, compiled);
}

/// A field the receiver's prototype holds folds to the value the holder holds when the body is
/// compiled, under a dependency on the holder's values: a change a call inside the body makes is
/// seen by the read after the call (the body reads its invalidation word there), and a change
/// between runs retires the body.
#[test]
fn folded_prototype_reads_see_changes_a_call_in_the_body_makes() {
    let (interpreted, compiled, _) = run_both(
        "const proto = { scale: 3 }; const o = Object.create(proto); o.x = 2;
         function bump() { proto.scale = proto.scale + 1; }
         function area(p) { const a = p.x * p.scale; bump(); return a + p.x * p.scale; }
         let t = 0; for (let k = 0; k < 300; k++) { proto.scale = 3; t = (t + area(o)) | 0; }
         print(t); print(area(o)); proto.scale = 10; print(area(o));",
    );
    assert_eq!(interpreted, compiled);
}

/// A `throw` a handler of the same function covers is the catch's jump in the body: the thrown
/// value goes to the handler's register (or nowhere, for a catch with no binding) and control to
/// its block, so every activation completes in the body, with the output the interpreter's.
#[test]
fn a_throw_a_handler_covers_is_a_jump_in_the_body() {
    assert_bodies_never_leave(
        "function f(n) { let s = 0; for (let i = 0; i < n; i++) { try { if (i % 3 === 0) throw i; s = s + 1; } catch (e) { s = s + e; } } return s; }
         function g(n) { let s = 0; for (let i = 0; i < n; i++) { try { throw 1; } catch { s = s + 2; } } return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k % 9) + g(3)) | 0;
         print(t); print(f(20)); print(g(5));",
    );
}

/// A `throw` of a non-Error value, nested handlers and a rethrow from a handler the same function
/// covers all end in the handler the interpreter finds.
#[test]
fn nested_handlers_and_rethrows_print_what_the_interpreter_prints() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "function f(n, v) { let s = 0; for (let i = 0; i < n; i++) { try { try { throw v; } catch (e) { s = s + 1; throw i; } } catch (g) { s = s + g; } } return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k % 7, k % 2 ? 'a' : null)) | 0;
         print(t); print(f(4, undefined)); print(f(3, { x: 1 }));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A `throw` no handler of its function covers ends the body at the `throw`, the exception stashed
/// for the lane to raise there: the caller's handler gets the same value and message.
#[test]
fn a_throw_no_handler_covers_ends_the_body_and_reaches_the_caller() {
    let (interpreted, compiled, _) = run_both(
        "function pick(x) { if (x > 5) throw new RangeError('big ' + x); if (x > 3) throw x; return x; }
         function f(n) { let s = 0; for (let i = 0; i < n; i++) { try { s = s + pick(i); } catch (e) { s = s + (typeof e === 'number' ? e : e.message.length); } } return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k % 9)) | 0;
         print(t);
         try { pick(9); } catch (e) { print(e.name); print(e.message); }
         try { pick(4); } catch (e) { print(e); }",
    );
    assert_eq!(interpreted, compiled);
}

/// `typeof`, `instanceof` and `in` are the interpreter's operations made in place by the step
/// helper, for every kind of operand: the body completes and prints what the interpreter prints.
#[test]
fn typeof_instanceof_and_in_run_in_a_body() {
    assert_bodies_never_leave(
        "function kind(v) { return (typeof v === 'number' ? 1 : 0) + (typeof v === 'string' ? 2 : 0) + (typeof v === 'function' ? 4 : 0) + (typeof v === 'undefined' ? 8 : 0); }
         function has(o, k) { return k in o ? 1 : 0; }
         function inst(v, c) { return v instanceof c ? 1 : 0; }
         const values = [1, 'a', function () {}, undefined, null, {}, [], 2.5]; let t = 0;
         for (let k = 0; k < 300; k++) t = (t + kind(values[k % 8]) + has({ p: 1 }, k % 2 ? 'p' : 'q') + inst([], k % 2 ? Array : Object)) | 0;
         print(t); print(kind(Symbol())); print(has([1, 2], 1)); print(has([1, 2], 'length')); print(inst(function () {}, Function));",
    );
    let (interpreted, compiled, _) = run_both(
        "function has(o, k) { return k in o; }
         function inst(v, c) { return v instanceof c; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + (has({ p: 1 }, 'p') ? 1 : 0) + (inst([], Array) ? 1 : 0)) | 0;
         print(t);
         try { has(5, 'p'); } catch (e) { print(e.constructor.name); }
         try { inst({}, 5); } catch (e) { print(e.constructor.name); }
         try { inst({}, { [Symbol.hasInstance]() { throw new Error('h'); } }); } catch (e) { print(e.message); }",
    );
    assert_eq!(interpreted, compiled);
}

/// A block scope per iteration (`CloneEnv`) with closures that capture its bindings, and a
/// binding in its temporal dead zone, run in the body: each closure sees its own iteration's copy.
#[test]
fn block_scopes_with_closures_run_in_a_body() {
    assert_bodies_never_leave(
        "function scopes(n) { let first, last; for (let j = 0; j < n; j++) { let c = j * 2; const g = function () { return c + j; }; if (j === 0) first = g; else last = g; } return (first ? first() : 0) + (last ? last() : 0); }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + scopes(k % 6)) | 0;
         print(t); print(scopes(4));",
    );
    let (interpreted, compiled, _) = run_both(
        "function tdz(n) { let s = 0; for (let i = 0; i < n; i++) { try { s = s + z; let z = 1; } catch (e) { s = s + 1; } } return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + tdz(k % 5)) | 0;
         print(t); print(tdz(3));",
    );
    assert_eq!(interpreted, compiled);
}

/// A class's construction (the constructor's own body, `super`, heritage validation and a method
/// that reads `super`) prints what the interpreter prints; the constructions leave their callers'
/// bodies (`Construct` is the interpreter's).
#[test]
fn class_construction_and_super_print_what_the_interpreter_prints() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "class A { constructor(v) { this.v = v; } get() { return this.v; } }
         class B extends A { constructor(v) { super(v + 1); this.w = 2; } get() { return super.get() * this.w; } }
         function make(v) { return new B(v).get(); }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + make(k % 5)) | 0;
         print(t);
         try { class Bad extends 5 {} } catch (e) { print(e.constructor.name); }
         try { new (class C extends A { constructor() { this.x = 1; } })(); } catch (e) { print(e.constructor.name); }",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
}

/// A callee of the realm active when a body is compiled may be inlined; once another realm is active
/// the same function belongs to a foreign one, and the fold is refused (its rows would read the
/// active realm's globals, not its own).
#[test]
fn a_callee_of_another_realm_is_not_inlinable() {
    let program = Engine::specialize("function g(x) { return x + 1; }", "realm.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    let atom = vm.lookup_atom("g").unwrap();
    let callee = vm.own_property(vm.realm.globals, atom).unwrap();
    let Some(quench_runtime::tier_surface::FunctionRecord { kind: quench_runtime::tier_surface::FunctionKind::User(_, function), .. }) = vm.function_record(callee) else { panic!("a script function") };
    let function = *function;
    let residual = vm.programs.get(vm.active_program).unwrap();
    assert!(vm.same_realm_user_target(callee).is_some(), "the function belongs to the active realm");
    let foreign = vm.create_realm_global(&program).unwrap();
    vm.switch_realm_global(foreign);
    assert!(vm.inlinable_callee(&residual, callee, function).is_none(), "a callee of another realm is not folded");
}

/// A global counter read and written by name, and the `typeof` of a global, run as rows over the
/// global object's slot (held under its layout), so a body makes none of them leave.
#[test]
fn global_counter_loops_run_in_a_body_without_leaving() {
    assert_bodies_never_leave(
        "var counter = 0;
         function bump(n) { for (let i = 0; i < n; i++) { counter = (counter + i) | 0; } return counter; }
         function kind() { return typeof counter; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + bump(9)) | 0;
         print(t); print(counter); print(kind());",
    );
}

/// A global redefined as an accessor retires the code that held its slot: the writes and reads that
/// follow are the accessor's, as in the interpreter, and so is a store of another type.
#[test]
fn a_global_redefined_as_an_accessor_retires_the_bodies_that_held_it() {
    let (interpreted, compiled, [runs, _]) = run_both(
        "globalThis.total = 0; let reads = 0;
         function bump(n) { for (let i = 0; i < n; i++) { total = (total + i) | 0; } return total; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + bump(5)) | 0;
         print(t);
         Object.defineProperty(globalThis, 'total', { get() { reads++; return 3; }, set(v) { reads += 10; }, configurable: true });
         for (let k = 0; k < 20; k++) t = (t + bump(5)) | 0;
         print(t); print(reads); print(typeof total);
         total = 'text'; print(typeof total); print(bump(2));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the body ran {runs} times");
}

