//! Whether a function loops, kept beside its view-less runs in its `lane_deferred` cell: a lane
//! return into a caller without a view derives one only for a caller that loops
//! (`lane_completes_return`).
use super::{ProgramId, ProgramStore};

/// The bits of a function's `lane_deferred` cell that count its view-less runs; the others
/// keep whether it loops (`ProgramStore::function_loops`).
pub(super) const DEFERRED_RUNS: u8 = 0x3f;
/// Set once whether the function loops is known, and with it `LOOPS` where it does.
const LOOP_KNOWN: u8 = 0x80;
const LOOPS: u8 = 0x40;

impl ProgramStore {
    /// Whether `function` of program `id` loops, scanned once and kept beside its deferred runs.
    pub fn function_loops(&self, id: ProgramId, function: u32) -> bool {
        let Some(entry) = self.entry_in_lane(id) else { return false };
        let Some(cell) = entry.lane_deferred.get(function as usize) else { return false };
        let held = cell.get();
        if held & LOOP_KNOWN != 0 {
            return held & LOOPS != 0;
        }
        let loops = entry.residual.function_get(function as usize).is_some_and(crate::bytecode::Function::loops);
        cell.set(held | LOOP_KNOWN | if loops { LOOPS } else { 0 });
        loops
    }

}
