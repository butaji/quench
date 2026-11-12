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
