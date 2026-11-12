//! The room bodies called from bodies found short, which the next body entry grows the stacks
//! to (`LaneStack::recovery_demand`): a call cannot grow the stacks its caller's registers live in.

use super::{Host, LaneRecoverySpace, LaneView, Vm};

impl LaneRecoverySpace {
    fn max(self, other: Self) -> Self {
        Self { values: self.values.max(other.values), activations: self.activations.max(other.activations) }
    }
}

/// Records that a body call into `own` found the stacks short of its body's room.
pub(super) fn note_short<H: Host>(vm: &Vm<H>, own: &LaneView) {
    let Some(method) = own.lane_body() else { return };
    if vm.programs.js_jit_diagnostics_enabled() { own.body_stats.refused.set(own.body_stats.refused.get() + 1); }
    vm.lane_stack.recovery_demand.set(vm.lane_stack.recovery_demand.get().max(method.recovery_space()));
}

/// `room` and the demand: what an entry's stacks must hold.
pub(super) fn with_demand<H: Host>(vm: &Vm<H>, room: LaneRecoverySpace) -> LaneRecoverySpace {
    room.max(vm.lane_stack.recovery_demand.get())
}

/// Whether the stacks hold the demand.
pub(super) fn demand_met<H: Host>(vm: &Vm<H>) -> bool {
    let demand = vm.lane_stack.recovery_demand.get();
    vm.lane_stack.activations.capacity() - vm.lane_stack.activations.len() >= demand.activations
        && vm.lane_stack.values.capacity() - vm.lane_stack.values.len() >= demand.values
}
