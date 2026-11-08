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
