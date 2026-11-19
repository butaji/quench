//! The liveness analyses the lowering passes run over `CompileScratch`'s buffers. The transfer
//! table and the successor table are filled in place, so an analysis allocates only the live
//! sets it returns.

use super::{MethodArguments, RegisterSet, ROOT_MAP_REGISTERS, analyze_wide_with, budget, transfer};
use crate::bytecode::{FieldSite, Function, Superinstruction};
use crate::compile::CompileScratch;

/// Fills `out` with each instruction's (uses, definitions) masks, as `liveness::transfers`
/// derives them, when the function's registers fit the masks and the budget admits it; whether
/// it did. `out` is left empty otherwise.
pub(crate) fn transfers_into(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
    out: &mut Vec<(RegisterSet, RegisterSet)>,
) -> bool {
    let admitted = function.registers <= ROOT_MAP_REGISTERS && budget::admits(function);
    out.clear();
    if admitted {
        out.extend(function.instructions().map(|instruction| transfer(instruction, methods, fields, superinstructions)));
    }
    admitted
}

/// `liveness::analyze`, with its transfer table and successor table in `scratch`'s buffers.
pub(crate) fn analyze_with(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
    scratch: &mut CompileScratch,
) -> Option<Vec<RegisterSet>> {
    let mut transfers = std::mem::take(&mut scratch.transfers);
    let live = if transfers_into(function, methods, fields, superinstructions, &mut transfers) {
        let mut successors = std::mem::take(&mut scratch.successors);
        successors.refill(function);
        let live = analyze_wide_with(function, &transfers, &successors, scratch);
        scratch.recycle_successors(successors);
        Some(live)
    } else {
        None
    };
    scratch.recycle_transfers(transfers);
    live
}
