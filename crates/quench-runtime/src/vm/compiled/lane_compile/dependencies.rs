//! What a lane body's code depends on, and its subscription to the changes that retire it.

use super::*;

/// What a lane body's code relies on beyond its guards, its direct callees' included, once
/// each. A receiver's shape is guarded at each field use (and proved again after any call,
/// `State::call_effect`), so an object leaving it falls out at the guard: only its slots'
/// observations are dependencies, and a compaction retires code that names shape ids
/// (`LaneView::invalidate_shape_bindings`). A holder's (prototype's) layout is read
/// without a guard, so its departures are dependencies too.
#[derive(Default)]
pub(in crate::vm) struct LaneDependencies {
    pub(in crate::vm) slots: Vec<crate::code_dependencies::SlotDependency>,
    pub(super) departure_shapes: Vec<u32>,
}

impl LaneDependencies {
    pub(in crate::vm) fn of(loads: impl Iterator<Item = NativeFieldLoad> + Clone, direct: &[DirectCallee]) -> Self {
        let mut slots: Vec<_> = loads.clone().filter_map(NativeFieldLoad::slot_dependency)
            .chain(direct.iter().filter_map(|callee| callee.body.as_ref()).flat_map(|body| body.dependencies.slots.iter().copied())).collect();
        slots.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
        slots.dedup();
        let mut departure_shapes: Vec<_> = loads.filter_map(|load| load.holder)
            .chain(direct.iter().filter_map(|callee| callee.body.as_ref()).flat_map(|body| body.dependencies.departure_shapes.iter().copied())).collect();
        departure_shapes.sort_unstable();
        departure_shapes.dedup();
        Self { slots, departure_shapes }
    }

    /// Makes `code` retire when a fact it took changes: a slot's observation, a holder
    /// shape's departure, or one of its Stable facts; `false` where a reader table is full.
    pub(in crate::vm) fn subscribe(&self, stable: &lane_region::StableFacts, code: &Rc<dyn crate::code_dependencies::DependentCode>,
        heap: &mut crate::heap::Heap, method_atoms: &mut crate::code_dependencies::CodeReaders) -> bool {
        self.slots.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
            && self.departure_shapes.iter().all(|shape| heap.subscribe_shape(*shape, code))
            && subscribe_stable(stable, code, heap, method_atoms)
    }
}

/// Subscribes `code` to the Stable facts it folded: prototype values, method atoms, globals, layouts,
/// slot observations and instanceof facts; `false` where a reader table is full.
fn subscribe_stable(stable: &lane_region::StableFacts, code: &Rc<dyn crate::code_dependencies::DependentCode>,
    heap: &mut crate::heap::Heap, method_atoms: &mut crate::code_dependencies::CodeReaders) -> bool {
    stable.holders.iter().all(|holder| crate::heap::subscribe_prototype_values(*holder, code))
        && stable.atoms.iter().all(|atom| method_atoms.subscribe(*atom, code))
        && stable.globals.iter().all(|fact| crate::heap::subscribe_global_value(*fact, code))
        && stable.layouts.iter().all(|fact| crate::heap::subscribe_global_layout(*fact, code))
        && stable.slots.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
        && stable.instanceofs.iter().all(|fact| heap.subscribe_slot_observation(fact.slot_dependency(), code)
            && heap.subscribe_shape(fact.shape, code) && crate::heap::subscribe_prototype_values(fact.holder, code))
}
