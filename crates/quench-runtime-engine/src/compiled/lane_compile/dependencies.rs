//! What a lane body's code depends on, and its subscription to the changes that retire it.

use super::*;

/// What a lane body's code relies on beyond its guards, its direct callees' included, once
/// each. A receiver's shape is guarded at each field use (and proved again after any call,
/// `State::call_effect`, unless the VM holds it stable: `StableFacts::shapes`), so an object
/// leaving it falls out at the guard: only its slots' observations are dependencies, and a
/// compaction retires code that names shape ids (`LaneView::invalidate_shape_bindings`). A
/// holder's (prototype's) layout is read without a guard, so its departures are dependencies too.
#[derive(Default)]
pub(crate) struct LaneDependencies {
    pub(crate) slots: Vec<quench_runtime::code_dependencies::SlotDependency>,
    pub(super) departure_shapes: Vec<u32>,
    /// The direct callees' bodies, whose entries the code calls: a change of a Stable fact one of
    /// them folded retires it, and so the code that would still enter it.
    pub(super) callees: Vec<Rc<LaneMethod>>,
}

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

    /// Makes `code` retire when a fact it took changes: a slot's observation, a holder
    /// shape's departure, or one of its Stable facts or its direct callees'; `false` where a reader table is full.
    pub(crate) fn subscribe(&self, stable: &lane_region::StableFacts, code: &Rc<dyn quench_runtime::code_dependencies::DependentCode>,
        heap: &mut quench_runtime::heap::Heap, method_atoms: &mut quench_runtime::code_dependencies::CodeReaders) -> bool {
        self.slots.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
            && self.departure_shapes.iter().all(|shape| heap.subscribe_shape(*shape, code))
            && subscribe_stable(stable, code, heap, method_atoms)
            && self.subscribe_callees(code, heap, method_atoms)
    }

    /// Releases the capacity a publication's vectors hold past their length: they are built by
    /// `collect` and `dedup`, and kept for as long as the body is, its callers' too.
    pub(crate) fn trim(&mut self) {
        self.slots.shrink_to_fit();
        self.departure_shapes.shrink_to_fit();
        self.callees.shrink_to_fit();
    }

    /// The programs whose root constants the code folds, its own and its direct callees' and theirs
    /// in turn (`StableFacts::root_program`), each once: `ProgramStore::subscribe_root_constants`
    /// subscribes the code to them.
    pub(crate) fn root_programs(&self, stable: &lane_region::StableFacts) -> Vec<quench_runtime::vm::program_store::ProgramId> {
        let mut programs: Vec<quench_runtime::vm::program_store::ProgramId> = Vec::new();
        programs.extend(stable.root_program);
        for callee in &self.callees {
            for program in callee.dependencies.root_programs(&callee.stable) {
                if !programs.contains(&program) { programs.push(program); }
            }
        }
        programs
    }

    /// Subscribes `code` to the Stable facts of its direct callees' bodies, and theirs in turn.
    fn subscribe_callees(&self, code: &Rc<dyn quench_runtime::code_dependencies::DependentCode>,
        heap: &mut quench_runtime::heap::Heap, method_atoms: &mut quench_runtime::code_dependencies::CodeReaders) -> bool {
        self.callees.iter().all(|callee| subscribe_stable(&callee.stable, code, heap, method_atoms)
            && callee.dependencies.subscribe_callees(code, heap, method_atoms))
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
