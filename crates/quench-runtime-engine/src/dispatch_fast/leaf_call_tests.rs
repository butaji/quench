use super::*;
use quench_runtime::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

const LEAF_ENTRY_SOURCE: &str = r#"
      function plain(x) { return (x + 1) | 0; }
      function branch(x) { return x < 3 ? x : -x; }
      var bump = (function () { var c = 0; return function () { c = (c + 1) | 0; return c; }; })();
      function method(x) { return (x + this.n) | 0; }
      function setter(x) { this.n = x; }
      globalThis.G0 = 1;
      function readsGlobal(x) { return x + G0; }
      function picks(o, f) { return f ? o.a : o.b; }
      function calls(x) { return plain(x); }
      function Pt(x) { this.x = x; }
      function make(C, x) { return new C(x); }
      function loops(x) { var s = 0; for (var i = 0; i < x; i++) s += i; return s; }
      function args(x) { return arguments.length; }
      class P { #v = 1; read() { return this.#v; } }
      var p = new P(), readP = P.prototype.read;
      for (var i = 0; i < 2; i++) { plain(i); branch(i); bump(); method.call({ n: 1 }, i); setter.call({ n: 1 }, i); readsGlobal(i); picks({ a: 1, b: 2 }, i); calls(i); loops(i); args(i); p.read(); }
"#;

#[test]
fn leaf_bodies_take_the_leaf_entry_and_others_activate() {
    // A body of register, capture, `this`, field read and store, global name, construction and
    // returning read records with forward jumps is a leaf (`leaf_body`), whose records read the
    // context or not; a call, a loop, `arguments` or a private read keeps the activation.
    let program = quench_runtime::Engine::specialize(LEAF_ENTRY_SOURCE, "leaf-entries.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let leaf = |entry: CallEntry| entry.leaf_form() != NOT_LEAF;
    for (name, expected) in [
        ("plain", Some(CallEntry::Leaf)),
        ("branch", Some(CallEntry::Leaf)),
        ("bump", Some(CallEntry::CaptureLeaf)),
        ("method", Some(CallEntry::ThisLeaf)),
        ("setter", Some(CallEntry::ThisLeaf)),
        ("readsGlobal", Some(CallEntry::Leaf)),
        ("picks", Some(CallEntry::Leaf)),
        ("make", Some(CallEntry::Leaf)),
        ("calls", None),
        ("loops", None),
        ("args", None),
        ("readP", None),
    ] {
        let atom = vm.intern_atom(name);
        let callee = vm.own_property(vm.realm.globals, atom).unwrap();
        let entry = vm.resolve_script_call_target(&program, callee).map(|target| target.entry);
        match expected {
            Some(expected) => assert!(entry == Some(expected), "{name} is a leaf"),
            None => assert!(!entry.is_some_and(leaf), "{name} is no leaf"),
        }
    }
}

#[test]
fn a_leaf_returning_construction_completes() {
    // `return new V(...)` in a function the lane runs as a leaf, warm, through a forwarding
    // constructor: the construction completes the leaf's activation, as a frame would.
    let source = r#"
      var Class = { create: function () { return function () { this.initialize.apply(this, arguments); }; } };
      var V = Class.create();
      V.prototype = { initialize: function (x, y) { this.x = x; this.y = y; } };
      function make(i) { return new V(i, 2); }
      function run(n) { var s = 0; for (var i = 0; i < n; i++) { var v = make(i); s += v.x + v.y; } return s; }
      function warm() { for (var k = 0; k < 40; k++) { run(300); } }
      warm();
      print(run(10));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = quench_runtime::Engine::specialize(source, "leaf-returning-construct.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["65"]);
}
