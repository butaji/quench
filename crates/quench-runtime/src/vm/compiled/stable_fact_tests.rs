//! Stable facts: what a lowering folds under code dependencies, and that changing one
//! retires the code that folded it.

use super::*;

/// The lane-body compilation of the named function of `source`, after it ran.
fn compiled_lane_body(source: &str, name: &str) -> (Vm<crate::test_host::TestHost>, ResidualProgram, LaneMethod) {
    let program = crate::Engine::specialize(source, "stable-facts.js").unwrap();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    vm.execute(&program).unwrap();
    vm.programs.enable_js_lane_methods();
    let function = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    let method = vm.compile_lane_method(&program, function as u32,
        &|_, _| Vec::new(), &|_| (0, 0), Vec::new(), None, &[]).unwrap();
    (vm, program, method)
}

/// Installs a method cache entry for `owner.atom`, as a call site's first call does.
fn name_in_a_method_cache(vm: &mut Vm<crate::test_host::TestHost>, owner: Value, atom: Atom) {
    let callee = vm.own_property(owner, atom).unwrap();
    let shape = vm.heap.object_record(owner).unwrap().shape();
    let target = vm.call_target(callee).unwrap();
    let slot = vm.shape_slot(shape, atom).unwrap() as u16;
    let site = vm.method_caches.len();
    vm.method_caches.push([super::super::EMPTY_METHOD_CACHE; 2]);
    vm.record_method_cache(site, super::super::MethodCache { shape, atom, proto: owner, callee,
        guard: super::super::MethodGuard { owner, owner_shape: shape, slot, depth: 0 }, target: Some(target) });
}

#[test]
fn a_direct_store_holds_unnamed_method_atoms_until_a_cache_names_one() {
    let (mut vm, _program, method) = compiled_lane_body(
        "var o={a:1,b:2}; var owner={a(){return 1},m(){return 2}}; \
         function write(x){ for (var i=0;i<3;i++) { o.a = x; } } write(2); write(3);", "write");
    let (a, m) = (vm.intern_atom("a"), vm.intern_atom("m"));
    assert_eq!(method.stable.atoms, vec![a], "the store folds that no method cache names `a`");
    let invalidation = method.invalidation.clone();
    let published = vm.programs.publish_lane_method(method, &mut vm.heap, vm.method_cache_atoms.readers()).unwrap();
    let active = published.lease().unwrap();
    let owner_atom = vm.intern_atom("owner");
    let owner = vm.own_property(vm.realm.globals, owner_atom).unwrap();
    name_in_a_method_cache(&mut vm, owner, m);
    assert!(published.lease().is_some() && !invalidation.is_set(), "another name changes nothing");
    name_in_a_method_cache(&mut vm, owner, a);
    assert!(published.lease().is_none(), "a cache naming `a` retires the code that stored it directly");
    assert!(invalidation.is_set(), "a run inside it leaves at its next invalidation point");
    drop(active);
}

#[test]
fn a_folded_prototype_value_retires_its_readers_when_it_changes() {
    let (mut vm, _program, method) = compiled_lane_body(
        "var C = function C(){ this.x = 1; }; C.prototype.k = 5; var c = new C(); \
         function read(){ var n = 0; for (var i = 0; i < 5; i++) { n = (n + c.k) | 0; } return n; } \
         for (var j = 0; j < 4; j++) read();", "read");
    assert!(!method.stable.holders.is_empty(), "the inherited read takes the prototype's value as a constant");
    let invalidation = method.invalidation.clone();
    let published = vm.programs.publish_lane_method(method, &mut vm.heap, vm.method_cache_atoms.readers()).unwrap();
    let (instance_atom, k) = (vm.intern_atom("c"), vm.intern_atom("k"));
    let instance = vm.own_property(vm.realm.globals, instance_atom).unwrap();
    let prototype = vm.heap.object(instance).unwrap().proto();
    let shape = vm.heap.object(prototype).unwrap().shape();
    let slot = vm.shape_slot(shape, k).unwrap();
    vm.heap.property_set(prototype, slot, Value::integer(6));
    assert!(published.lease().is_none(), "the prototype's write retires the code that folded its value");
    assert!(invalidation.is_set());
}

/// Stable facts change while compiled bodies hold them: a prototype's value, written by a
/// callee in the middle of a hot loop that read it as a constant, and a method name a cache
/// first names after a loop stored that name directly. Each mode computes the same values.
#[test]
fn stable_facts_changed_mid_run_leave_compiled_bodies_with_exact_results() {
    let source = r#"
        var C = function () {}; C.prototype.k = 1; var c = new C();
        function bump() { C.prototype.k = C.prototype.k + 1; }
        function read(n) { var s = 0; for (var i = 0; i < n; i++) { s = (s + c.k) | 0; if (i === 50) bump(); } return s; }
        var reads = 0; for (var j = 0; j < 400; j++) reads = (reads + read(100)) | 0;
        var o = { a: 0, b: 0 };
        function store(x) { for (var i = 0; i < 20; i++) { o.a = x + i; } return o.a; }
        var stores = 0; for (var j = 0; j < 400; j++) stores = (stores + store(j)) | 0;
        var named = { a() { return 7; } }; var calls = 0;
        for (var j = 0; j < 50; j++) calls += named.a();
        for (var j = 0; j < 400; j++) stores = (stores + store(j)) | 0;
        globalThis.answer = [reads, stores, calls, C.prototype.k, o.a].join(",");
    "#;
    let program = crate::Engine::specialize(source, "stable-facts-mid-run.js").unwrap();
    let mut answers = Vec::new();
    for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(mode);
        vm.programs.enable_js_lane_methods();
        vm.execute(&program).unwrap();
        let answer = vm.intern_atom("answer");
        let value = vm.own_property(vm.realm.globals, answer).unwrap();
        answers.push(vm.heap.string(value).unwrap().to_string());
    }
    assert_eq!(answers[0], answers[1], "interpreted and compiled runs agree");
}
