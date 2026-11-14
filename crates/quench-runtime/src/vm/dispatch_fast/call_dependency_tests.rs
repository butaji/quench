//! Cached call targets rest on a fact no call tests: no `with` scope was entered. Its change
//! retires every target that relied on it.
use super::*;
use crate::test_host::TestHost;

const SOURCE: &str = r#"
      function add(x) { return x + 1; }
      function run(n) { var s = 0; for (var i = 0; i < n; i++) s += add(i); return s; }
      var before = run(100);
"#;

fn global(vm: &mut Vm<TestHost>, name: &str) -> Value {
    let atom = vm.intern_atom(name);
    vm.own_property(vm.realm.globals, atom).unwrap()
}

#[test]
fn entering_a_with_scope_retires_every_cached_target_and_withholds_new_ones() {
    let program = crate::Engine::specialize(SOURCE, "with-retire.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let callee = global(&mut vm, "add");
    assert!(vm.resolve_script_call_target(&program, callee).is_some());
    assert!(vm.script_call_entry(callee).is_some());
    vm.note_with_scope_entered();
    assert!(vm.script_call_entry(callee).is_none(), "the generation that cached it is retired");
    let retired = vm.lane_stack.generation;
    vm.note_with_scope_entered();
    assert!(vm.lane_stack.generation == retired, "a later `with` retires nothing more");
    assert!(vm.resolve_script_call_target(&program, callee).is_none(), "no target resolves under a with scope");
    vm.lane_stack.enter_generation((7, Value::UNDEFINED));
    assert!(vm.script_call_entry(callee).is_none(), "a later lane run's generation names no earlier entry");
}

const WITH_SOURCE: &str = r#"
      function add(x) { return x + 1; }
      function run(n) { var s = 0; for (var i = 0; i < n; i++) s += add(i); return s; }
      var before = run(200);
      var o = { add: function (x) { return -1; } };
      var inside; with (o) { inside = add(5); }
      var after = run(200);
      var again = run(200);
"#;

#[test]
fn calls_cached_before_a_with_scope_run_as_node_runs_them_after_it() {
    let program = crate::Engine::specialize(WITH_SOURCE, "with-later.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    for (name, expected) in [("before", 20100), ("inside", -1), ("after", 20100), ("again", 20100)] {
        assert!(global(&mut vm, name) == Value::integer(expected), "{name}");
    }
}

const REDEFINE_SOURCE: &str = r#"
      function f(x) { return x * 2; }
      function C(x) { this.x = f(x); }
      function run(n) { var s = 0; for (var i = 0; i < n; i++) s += f(i) + new C(i).x; return s; }
      var a = run(100);
      f = function (x) { return x * 3; };
      var b = run(100);
"#;

#[test]
fn a_function_redefined_after_its_site_cached_it_calls_the_new_one() {
    let program = crate::Engine::specialize(REDEFINE_SOURCE, "redefine.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    for (name, expected) in [("a", 19800), ("b", 29700)] {
        assert!(global(&mut vm, name) == Value::integer(expected), "{name}");
    }
}
