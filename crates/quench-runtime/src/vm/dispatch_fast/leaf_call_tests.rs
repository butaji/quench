use super::*;
use crate::test_host::TestHost;

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
      function loops(x) { var s = 0; for (var i = 0; i < x; i++) s += i; return s; }
      function args(x) { return arguments.length; }
      class P { #v = 1; read() { return this.#v; } }
      var p = new P(), readP = P.prototype.read;
      for (var i = 0; i < 2; i++) { plain(i); branch(i); bump(); method.call({ n: 1 }, i); setter.call({ n: 1 }, i); readsGlobal(i); picks({ a: 1, b: 2 }, i); calls(i); loops(i); args(i); p.read(); }
"#;

#[test]
fn leaf_bodies_take_the_leaf_entry_and_others_activate() {
    // A body of register, capture, `this`, field read and store, global name and returning
    // read records with forward jumps is a leaf (`leaf_body`), whose records read the
    // context or not; a call, a loop, `arguments` or a private read keeps the activation.
    let program = crate::Engine::specialize(LEAF_ENTRY_SOURCE, "leaf-entries.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let leaf = |entry: CallEntry| entry.leaf_form() != NOT_LEAF;
    for (name, expected) in [
        ("plain", Some(CallEntry::Leaf)),
        ("branch", Some(CallEntry::Leaf)),
        ("bump", Some(CallEntry::CaptureLeaf)),
        ("method", Some(CallEntry::ContextLeaf)),
        ("setter", Some(CallEntry::ContextLeaf)),
        ("readsGlobal", Some(CallEntry::Leaf)),
        ("picks", Some(CallEntry::Leaf)),
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
