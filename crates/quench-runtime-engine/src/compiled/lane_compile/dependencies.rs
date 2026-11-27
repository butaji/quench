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

/// Subscribes `code` to the Stable facts it folded: what it read through prototypes
/// (`subscribe_prototype_facts`) and what it holds of the heap's objects (`subscribe_heap_facts`);
/// `false` where a reader table is full.
pub fn subscribe_stable(stable: &lane_region::StableFacts, code: &Rc<dyn quench_runtime::code_dependencies::DependentCode>,
    facts: &mut quench_runtime::vm::tier_queries::FactWatch<'_>) -> bool {
    subscribe_prototype_facts(stable, code, facts) && subscribe_heap_facts(stable, code, facts)
}

/// The prototype chains it read through, prototype values, and instanceof facts. The chains come
/// first: their walk marks a link that was no prototype yet (`Heap::chain_subjects`), a note a
/// dependency subscribed before it would take for a change.
fn subscribe_prototype_facts(stable: &lane_region::StableFacts, code: &Rc<dyn quench_runtime::code_dependencies::DependentCode>,
    facts: &mut quench_runtime::vm::tier_queries::FactWatch<'_>) -> bool {
    stable.chains.iter().chain(stable.instanceofs.iter().map(|fact| &fact.chain)).all(|chain| facts.subscribe_prototype_chain(*chain, code))
        && stable.holders.iter().all(|holder| quench_runtime::tier_surface::subscribe_prototype_values(*holder, code))
        && stable.instanceofs.iter().all(|fact| facts.subscribe_slot_observation(fact.slot_dependency(), code)
            && facts.subscribe_shape(fact.shape, code) && quench_runtime::tier_surface::subscribe_prototype_values(fact.holder, code))
}

/// Method atoms, shapes held past a call, globals, layouts, slot observations and held typed views.
fn subscribe_heap_facts(stable: &lane_region::StableFacts, code: &Rc<dyn quench_runtime::code_dependencies::DependentCode>,
    facts: &mut quench_runtime::vm::tier_queries::FactWatch<'_>) -> bool {
    stable.atoms.iter().all(|atom| facts.subscribe_atom(*atom, code))
        && stable.shapes.iter().all(|shape| facts.subscribe_shape(*shape, code))
        && stable.globals.iter().all(|fact| quench_runtime::tier_surface::subscribe_global_value(*fact, code))
        && stable.layouts.iter().all(|fact| quench_runtime::tier_surface::subscribe_global_layout(*fact, code))
        && stable.slots.iter().all(|dependency| facts.subscribe_slot_observation(*dependency, code))
        && (!stable.views || quench_runtime::tier_surface::subscribe_typed_views(code))
}
