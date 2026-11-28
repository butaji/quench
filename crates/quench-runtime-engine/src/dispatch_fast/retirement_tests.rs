//! A retired shape or slot binding unlinks the native code that relied on it, and the code stays
//! alive while a caller or recovery still holds it.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

#[cfg(test)]
#[test]
fn osr_shape_retirement_unlinks_actual_code_and_keeps_recovery_alive() {
    let program = quench_runtime::Engine::specialize(
        "var owner={value:1}; var list=[owner]; function run(list,n){var s=0;for(var i=0;i<n;i++){var o=list[i&0];s=(s+o.value)|0;}return s;} for(var j=0;j<50;j++)run(list,1000);",
        "osr-shape-retirement.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    vm.set_memory_policy(quench_runtime::MemoryPolicy::Throughput);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    let atom = vm.intern_atom("owner");
    let owner = vm.own_property(vm.realm.globals, atom).unwrap();
    let shape = vm.heap_reads().object(owner).unwrap().shape();
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
    vm.add_property(owner, Value::integer(2), next);
    assert!(publications.iter().all(|code| code.is_published() && code.lease().is_some()),
        "a receiver shape departure must fall out at its guard, not retire the code");
    vm.programs.invalidate_shape_bindings();
    assert!(publications.iter().all(|code| !code.is_published() && code.lease().is_none()));
    assert!(weak.iter().all(|code| code.upgrade().is_some()));
    drop(recovery);
    assert!(weak.iter().all(|code| code.upgrade().is_none()));
}
