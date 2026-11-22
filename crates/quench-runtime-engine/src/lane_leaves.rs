//! Where a specialized lane body's leaves went: its specialization's feedback.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::engine_types::LANE_LEAVE_SITES;

pub(crate) use quench_runtime::vm::engine_types::LaneLeaves;

#[quench_macros::ext(LaneLeavesOps1)]
impl LaneLeaves {
    /// The exits these name, listed.
    pub(crate) fn listed(&self) -> Vec<crate::compiled::LaneExit> {
        self.exits().collect()
    }
}
