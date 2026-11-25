//! The back-edge poll every specializing tier shares: one load of the interrupt flag and one
//! guard. The interpreter has no poll of its own (its loop checks run in the host's dispatch),
//! so only a specializing stage calls these rows.

use super::Stage;
use super::region::Region;

/// Polls the interrupt flag that `attention` addresses (the call context's pointer to the flag
/// byte): one byte load, zero-extended, and one guard that continues where the flag is clear and
/// leaves at `miss` where it is set. Any work after an interrupted edge belongs on the `miss` path.
#[inline(always)]
pub fn back_edge_poll<S: Stage>(stage: &mut S, attention: S::Word, miss: S::Miss) -> Option<()> {
    let requested = stage.load_u8(Region::Dispatch.mutable(), attention, 0);
    let clear = stage.eq_imm(requested, 0u64);
    stage.guard(clear, miss)
}

/// The poll gated by the process-wide attention byte at `process`: that byte is set whenever any
/// run's flag is, so where it reads clear no run is interrupted and the row continues. Where it is
/// set, the run's own flag (the pointer at `flag_slot`, the call context's word) decides through
/// `back_edge_poll`. The gate is a branch, so the common clear path makes one load.
#[inline(always)]
pub fn gated_poll<S: Stage>(stage: &mut S, process: S::Word, flag_slot: S::Word, miss: S::Miss) -> Option<()> {
    let any = stage.load_u8(Region::Dispatch.mutable(), process, 0);
    let quiet = stage.eq_imm(any, 0u64);
    let zero = stage.constant(0);
    stage.choose_leaving(quiet, |_| Some(zero), |stage| {
        let flag = stage.load_u64(Region::Dispatch.mutable(), flag_slot, 0);
        back_edge_poll(stage, flag, miss).map(|()| zero)
    }).map(drop)
}
