//! A retired shape or slot binding unlinks the native code that relied on it, and the code stays
//! alive while a caller or recovery still holds it.
use super::*;

#[cfg(test)]
#[test]
fn osr_shape_retirement_unlinks_actual_code_and_keeps_recovery_alive() {
    let program = quench_runtime::Engine::specialize(
        "var owner={value:1}; var list=[owner]; function run(list,n){var s=0;for(var i=0;i<n;i++){var o=list[i&0];s=(s+o.value)|0;}return s;} for(var j=0;j<50;j++)run(list,1000);",
        "osr-shape-retirement.js").unwrap();
    let mut vm = Vm::new(quench_runtime::test_host::TestHost::default());
    vm.set_memory_policy(quench_runtime::MemoryPolicy::Throughput);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let atom = vm.intern_atom("owner");
    let owner = vm.own_property(vm.realm.globals, atom).unwrap();
    let shape = vm.heap.object(owner).unwrap().shape();
    let mut publications = Vec::new();
    for view in vm.programs.views_in_lane(super::program_store::ProgramId::MAIN).unwrap()
        .iter().filter_map(super::program_store::derived_lane_view) {
        for site in &view.loops {
            let tier = site.tier.borrow();
            if let Some(publication) = &tier.code
                && publication.lease().is_some_and(|code| code.facts.shape_dependencies.contains(&shape)) {
                publications.push(publication.clone());
            }
        }
    }
    assert!(!publications.is_empty(), "must exercise actual OSR field code");
    let recovery: Vec<_> = publications.iter().map(|code| code.lease().unwrap()).collect();
    let weak: Vec<_> = recovery.iter().map(Rc::downgrade).collect();
    // A receiver departure falls out at its use guard; only compaction renumbers embedded IDs.
    let added = vm.intern_atom("added");
    let next = vm.transition_shape(shape, added);
    vm.heap.property_add(owner, Value::integer(2), next);
    assert!(publications.iter().all(|code| code.is_published() && code.lease().is_some()),
        "a receiver shape departure must fall out at its guard, not retire the code");
    vm.programs.invalidate_shape_bindings();
    assert!(publications.iter().all(|code| !code.is_published() && code.lease().is_none()));
    assert!(weak.iter().all(|code| code.upgrade().is_some()));
    drop(recovery);
    assert!(weak.iter().all(|code| code.upgrade().is_none()));
}

/// A VM that ran a program whose `root` calls its `leaf`, which reads `owner.edgeValue`, with the call
/// targets of both resolved: the program, the leaf's callee value and the two targets.
#[cfg(test)]
fn direct_slot_fixture() -> (Vm, quench_runtime::ResidualProgram, Value, ScriptCallTarget, ScriptCallTarget) {
    let program = quench_runtime::Engine::specialize(
        "var owner={edgeValue:1}; function leaf(){var n=0;for(var i=0;i<5;i++)n=(n+owner.edgeValue)|0;return n;} function root(){return leaf();} for(var warm=0;warm<40;warm++)root();",
        "direct-slot-retirement.js").unwrap();
    program.compile_released();
    let mut vm = Vm::new(quench_runtime::test_host::TestHost::default());
    vm.execute(&program).unwrap();
    vm.programs.enable_js_lane_methods();
    vm.keep_slot_representations();
    let leaf_atom = vm.intern_atom("leaf");
    let root_atom = vm.intern_atom("root");
    let leaf = vm.own_property(vm.realm.globals, leaf_atom).unwrap();
    let root = vm.own_property(vm.realm.globals, root_atom).unwrap();
    let target = vm.resolve_script_call_target(&program, leaf).unwrap();
    let root_target = vm.resolve_script_call_target(&program, root).unwrap();
    assert!(target.entry == CallEntry::Activates && !target.view.is_null());
    assert!(!root_target.view.is_null());
    (vm, program, leaf, target, root_target)
}

/// Stores `value` to `owner.edgeValue` of the fixture's program, retiring what read the slot.
#[cfg(test)]
fn store_edge_value(vm: &mut Vm, value: f64) {
    let owner_atom = vm.intern_atom("owner");
    let owner = vm.own_property(vm.realm.globals, owner_atom).unwrap();
    let atom = vm.intern_atom("edgeValue");
    let slot = vm.shape_slot(vm.heap.object(owner).unwrap().shape(), atom).unwrap();
    vm.heap.property_set(owner, slot, Value::number(value));
}

#[cfg(test)]
#[test]
fn direct_callers_subscribe_to_callee_slot_dependencies() {
    use std::rc::Rc;
    let (mut vm, program, leaf, target, root_target) = direct_slot_fixture();
    let leaf_method = vm.compile_lane_method(&program, target.function,
        &|_, _| Vec::new(), &|_| (0, 0), Vec::new(), &[]).unwrap();
    assert!(!leaf_method.dependencies.slots.is_empty());
    let leaf_publication = vm.programs.publish_lane_method(leaf_method, &mut vm.heap, vm.method_cache_atoms.readers()).unwrap();
    let leaf_body = leaf_publication.lease().unwrap();
    assert!(leaf_body.calls_only_natively());
    let code = program.function(root_target.function as usize);
    let pc = code.instructions().position(|instruction| instruction.op().is_call()).unwrap();
    let mut direct = vec![super::compiled::DirectCandidates::new(); code.code.len()];
    // Resolve the capture view exactly as production direct-call publication does.
    let (capture_values, capture_len) = unsafe { quench_runtime::heap::EnvironmentSlots::native_storage_view(capture_reads(target.capture)) }
        .unwrap_or((std::ptr::null(), 0));
    direct[pc].push(super::compiled::DirectCallee {
        target, method: Some(leaf_publication.clone()), body: Some(leaf_body.clone()),
        call: unsafe { &*root_target.view }.records().wrapping_add(pc).addr(), caller_base: unsafe { &*root_target.view }.records().addr(),
        cell: unsafe { &*target.view }.entry_cell(),
        callee: leaf, registers: target.registers,
        sloppy: (target.this_binding == ThisBinding::Sloppy).then_some(vm.realm.globals), lexical: false,
        capture_values, capture_len, capture_stores: capture_stores(target.capture).is_some(), forwarder: None,
    });
    let root_method = vm.compile_lane_method(&program, root_target.function,
        &|_, _| Vec::new(), &|_| (0, 0), direct, &[]).unwrap();
    assert_eq!(root_method.direct.len(), 1);
    assert_eq!(root_method.dependencies.slots, leaf_body.dependencies.slots);
    let root_publication = vm.programs.publish_lane_method(root_method, &mut vm.heap, vm.method_cache_atoms.readers()).unwrap();
    let active_root = root_publication.lease().unwrap();
    let leaf_code = Rc::downgrade(&leaf_body);
    drop(leaf_body);
    store_edge_value(&mut vm, 0.5);
    assert!(leaf_publication.lease().is_none());
    assert!(root_publication.lease().is_none(), "retire embedded direct entry too");
    assert!(leaf_code.upgrade().is_some(), "active caller pins its retired callee");
    drop(active_root);
    assert!(leaf_code.upgrade().is_none());
}
