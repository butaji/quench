//! The room bodies called from bodies found short, which the next body entry grows the stacks
//! to (`LaneStack::recovery_demand`): a call cannot grow the stacks its caller's registers live in.

use super::{LaneRecoverySpace, LaneView, Vm};

#[quench_macros::ext(LaneRecoverySpaceOps2)]
impl LaneRecoverySpace {
    fn max(self, other: Self) -> Self {
        Self { values: self.values.max(other.values), activations: self.activations.max(other.activations) }
    }
}

/// Records that a body call into `own` found the stacks short of its body's room.
pub(crate) fn note_short(vm: &Vm, own: &LaneView) {
    let Some(method) = own.body_view_in_lane() else { return };
    super::body_refused_in_lane(vm, own);
    vm.lane_stack.recovery_demand.set(vm.lane_stack.recovery_demand.get().max(method.recovery_space()));
}

/// `room` and the demand: what an entry's stacks must hold.
pub(crate) fn with_demand(vm: &Vm, room: LaneRecoverySpace) -> LaneRecoverySpace {
    room.max(vm.lane_stack.recovery_demand.get())
}

/// Whether the stacks hold the demand.
pub(crate) fn demand_met(vm: &Vm) -> bool {
    let demand = vm.lane_stack.recovery_demand.get();
    vm.lane_stack.frame_capacity() - vm.lane_stack.activation_count() >= demand.activations
        && vm.lane_stack.values.room() >= demand.values
}
