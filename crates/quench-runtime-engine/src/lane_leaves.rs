//! Where a specialized lane body's leaves went: its specialization's feedback.

use quench_runtime::vm::engine_types::LANE_LEAVE_SITES;

pub(super) use quench_runtime::vm::engine_types::LaneLeaves;

#[quench_macros::ext(LaneLeavesOps1)]
impl LaneLeaves {
    /// The exits these name, listed.
    pub(super) fn listed(&self) -> Vec<crate::compiled::LaneExit> {
        self.exits().collect()
    }
}
