//! What a lane body's code depends on, and its subscription to the changes that retire it.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::LaneDependencies;

#[quench_macros::ext(LaneDependenciesOps1)]
impl LaneDependencies {
    pub(crate) fn of(loads: impl Iterator<Item = NativeFieldLoad> + Clone, direct: &[DirectCallee]) -> Self {
        let mut slots: Vec<_> = loads.clone().filter_map(NativeFieldLoad::slot_dependency)
            .chain(direct.iter().filter_map(|callee| callee.body.as_ref()).flat_map(|body| body.dependencies.slots.iter().copied())).collect();
        slots.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
        slots.dedup();
        let mut departure_shapes: Vec<_> = loads.filter_map(|load| load.holder)
            .chain(direct.iter().filter_map(|callee| callee.body.as_ref()).flat_map(|body| body.dependencies.departure_shapes.iter().copied())).collect();
        departure_shapes.sort_unstable();
        departure_shapes.dedup();
        let callees = direct.iter().filter_map(|callee| callee.body.clone()).collect();
        Self { slots, departure_shapes, callees }
    }
}

/// Subscribes `code` to the Stable facts it folded: the prototype chains it read through, prototype
/// values, method atoms, shapes held past a call, globals, layouts, slot observations and instanceof
/// facts; `false` where a reader table is full. The chains come first: their walk marks a link that was
/// no prototype yet (`Heap::chain_subjects`), a note a dependency subscribed before it would take for a
/// change.
fn subscribe_stable(stable: &lane_region::StableFacts, code: &Rc<dyn quench_runtime::code_dependencies::DependentCode>,
    heap: &mut quench_runtime::heap::Heap, method_atoms: &mut quench_runtime::code_dependencies::CodeReaders) -> bool {
    stable.chains.iter().chain(stable.instanceofs.iter().map(|fact| &fact.chain)).all(|chain| heap.subscribe_prototype_chain(*chain, code))
        && stable.holders.iter().all(|holder| quench_runtime::heap::subscribe_prototype_values(*holder, code))
        && stable.atoms.iter().all(|atom| method_atoms.subscribe(*atom, code))
        && stable.shapes.iter().all(|shape| heap.subscribe_shape(*shape, code))
        && stable.globals.iter().all(|fact| quench_runtime::heap::subscribe_global_value(*fact, code))
        && stable.layouts.iter().all(|fact| quench_runtime::heap::subscribe_global_layout(*fact, code))
        && stable.slots.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
        && stable.instanceofs.iter().all(|fact| heap.subscribe_slot_observation(fact.slot_dependency(), code)
            && heap.subscribe_shape(fact.shape, code) && quench_runtime::heap::subscribe_prototype_values(fact.holder, code))
}
