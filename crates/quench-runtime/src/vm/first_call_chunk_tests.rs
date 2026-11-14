use super::Vm;
use crate::Engine;
use crate::test_host::TestHost;

/// `B` compiles as a nested first-call stub in the indirect eval's code; the first `super()`
/// constructs it with `D` as `new.target`, so that instance has `D`'s methods too.
#[test]
fn stub_constructed_by_super_keeps_the_derived_new_target() {
    let source = "(0, eval)('(function () { function B() { this.b = 1; } \
                  class D extends B { constructor() { super(); } m() { return 2; } } \
                  globalThis.result = [new D(), new D()].map(d => typeof d.m + d.b).join(); })()');";
    let program = Engine::specialize(source, "first-call-super.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("result");
    let value = vm.own_property(vm.realm.globals, name).unwrap();
    assert_eq!(vm.to_string(&program, value).unwrap(), "function1,function1");
}

/// Runs `source` and returns the string its `globalThis.result` holds.
fn result_of(source: &str) -> String {
    let program = Engine::specialize(source, "first-call-direct.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("result");
    let value = vm.own_property(vm.realm.globals, name).unwrap();
    vm.to_string(&program, value).unwrap()
}

/// A direct eval's nested functions compile as stubs and read the caller's bindings by name
/// when first called, after the caller has returned.
#[test]
fn direct_eval_stubs_read_the_callers_bindings() {
    let source = "function t(p) { let a = 10; var c = 30; \
                  eval('function inner(x) { return x + a + c + p; } function other() { return inner(1) * 2; } var made = other;'); \
                  return [inner(5), other(), made()].join(); } \
                  function late() { let k = 5; return eval('(function () { k++; return k; })'); } \
                  const f = late(); globalThis.result = t(1) + ';' + [f(), f()].join();";
    assert_eq!(result_of(source), "46,84,84;6,7");
}

/// Each iteration's `let` binding and a catch parameter reach a stub as the eager compile does.
#[test]
fn direct_eval_stubs_keep_per_iteration_and_catch_bindings() {
    let source = "function t() { const fns = []; for (let i = 0; i < 3; i++) { eval('fns.push(function () { return i; })'); } \
                  try { throw new Error('boom'); } catch (err) { fns.push(eval('(function () { return err.message; })')); } \
                  return fns.map(f => f()).join(); } globalThis.result = t();";
    assert_eq!(result_of(source), "0,1,2,boom");
}

/// A stub of a function a direct eval declares sees `with` scopes and hoists like eager code.
#[test]
fn direct_eval_stubs_see_with_scopes_and_hoist() {
    let source = "function w() { with ({ v: 3 }) { return eval('(function () { return v; })')(); } } \
                  function h() { eval('var hoisted = 5; function hf() { return hoisted; }'); return [hoisted, hf(), typeof hf].join(); } \
                  globalThis.result = w() + ';' + h();";
    assert_eq!(result_of(source), "3;5,5,function");
}

/// Nested functions of a called stub are stubs in turn, at every level.
#[test]
fn stubs_nest_at_every_level_of_a_wrapper() {
    let source = "function run() { let outer = 1; return eval('(function () { var m = 2; \
                  function l1(a) { var n = 3; function l2(b) { function l3(c) { return [a, b, c, m, n, outer].join(); } return l3(b + 1); } return l2(a + 1); } \
                  return { l1: l1, unused: function () { return nope; } }; })()'); } \
                  const r = run(); globalThis.result = r.l1(1) + ';' + r.l1(5);";
    assert_eq!(result_of(source), "1,2,3,2,3,1;5,6,7,2,3,1");
}

/// The same call site evaluating the same text twice shares one program whose stubs close
/// over each evaluation's own scope.
#[test]
fn direct_eval_cache_keeps_stubs_per_evaluation() {
    let source = "function t(x) { return eval('(function () { return x; })'); } \
                  const a = t(1), b = t(2); globalThis.result = [a(), b(), a === b].join();";
    assert_eq!(result_of(source), "1,2,false");
}

/// An early error in a function no one calls is reported when the eval runs.
#[test]
fn direct_eval_reports_early_errors_of_uncalled_functions() {
    let source = "function t() { try { eval('(function () { function never() { let a; let a; } })'); return 'no error'; } \
                  catch (e) { return e.constructor.name; } } globalThis.result = t();";
    assert_eq!(result_of(source), "SyntaxError");
}

/// Runs `source` and returns its `globalThis.result` with the number of programs the VM
/// stores: a chunk that appended to its stub's program adds none.
fn result_and_programs(source: &str) -> (String, usize) {
    let program = Engine::specialize(source, "first-call-append.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("result");
    let value = vm.own_property(vm.realm.globals, name).unwrap();
    (vm.to_string(&program, value).unwrap(), vm.programs.len())
}

/// Stubs of an indirect eval, called one from another, compile into the eval's own program.
#[test]
fn chunks_append_to_their_stubs_program() {
    let source = "(0, eval)('function a(x) { return b(x) + 1; } function b(y) { return y * 2 + k; } var k = 4; globalThis.result = a(3) + \",\" + a(5);')";
    assert_eq!(result_and_programs(source), ("11,15".to_string(), 2));
}

/// A chunk's closures read the bindings of the code that made its stub, and its own nested
/// functions and constants, shared with its program's, resolve as they do in eager code.
#[test]
fn appended_chunks_close_over_their_parents() {
    let source = "(0, eval)('(function () { var m = 2; function l1(a) { var n = 3; return function (b) { return [a, b, m, n, \"x\", 2].join(); }; } \
                  globalThis.f = l1(1); globalThis.g = l1(7); })(); globalThis.result = f(2) + \";\" + g(8);')";
    assert_eq!(result_and_programs(source), ("1,2,2,3,x,2;7,8,2,3,x,2".to_string(), 2));
}

/// A chunk bigger than the room its program reserved compiles as a program of its own.
#[test]
fn a_chunk_without_room_compiles_as_its_own_program() {
    let table: Vec<String> = (0..400).map(|index| format!("{index}.5")).collect();
    let source = format!("(0, eval)('function big() {{ return [{}].length; }} globalThis.result = big();')", table.join(","));
    assert_eq!(result_and_programs(&source), ("400".to_string(), 3));
}
