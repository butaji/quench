//! The back-edge poll every specializing tier shares: one load of the interrupt flag and one
//! guard. The interpreter has no poll of its own (its loop checks run in the host's dispatch),
//! so only a stage that emits code calls this row.

use super::flow::FlowStage;
use super::region::Region;

/// Polls the interrupt flag that `attention` addresses (the call context's pointer to the flag
/// byte): one byte load, zero-extended, and one guard that continues where the flag is clear and
/// leaves at `miss` where it is set. Any work after an interrupted edge belongs on the `miss` path.
#[inline(always)]
pub fn back_edge_poll<S: FlowStage>(stage: &mut S, attention: S::Word, miss: S::Miss) -> Option<()> {
    let requested = stage.load_u8(Region::Dispatch.mutable(), attention, 0);
    let clear = stage.eq_imm(requested, 0u64);
    stage.guard(clear, miss)
}
