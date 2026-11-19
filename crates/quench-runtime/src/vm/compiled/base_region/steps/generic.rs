//! The generic semantic row: an instruction no stencil row takes, run by its opcode's semantic entry
//! (`Op::native_semantic_entry`) inside the body, so an AOT base body never leaves to an interpreter
//! or a per-PC loop. The row is the chain of `call_plan::plan_step` with the semantic call in place of
//! the helper: the live pinned registers are written back to the frame, the entry runs the record
//! (writing its next PC to a word), and the registers live at the next record are reloaded. A
//! successor that is the record's jump target branches there (`branching`); any other successor is
//! the next record. A stop (a return, a throw, a termination, a suspension) leaves with the entry's
//! (status, word), which the caller reads as it reads any semantic stop.

use super::*;
use super::branches::branching;
use quench_bjit::padded_holes;

/// The addresses a generic row calls, named by the body's holes: the record's opcode's semantic entry and
/// the operand record of its PC (`NativeJsOperands`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm::compiled::base_region) struct Semantic {
    pub(in crate::vm::compiled::base_region) entry: u64,
    pub(in crate::vm::compiled::base_region) operands: u64,
}

/// The generic row of the record at `pc`, which `decoded` holds as a `Step::Generic` (`steps::lowering`
/// under `LaneHelpers::Ahead`). Its successors are the next record and, where the record jumps, its
/// target; the registers it reads and writes are the frame's, as every semantic step's are.
pub(in crate::vm::compiled::base_region) fn generic_row(layout: &Layout, decoded: &Decoded, pc: u32, semantic: Semantic) -> Result<Planned, String> {
    let spills = layout.spills(pc);
    let mut parts = spills.iter().map(|&(place, register)| call_plan::spill_part(place, register)).collect::<Result<Vec<_>, _>>()?;
    parts.push(semantic_part(pc, semantic, decoded.target)?);
    for &(place, register) in &reloads(layout, decoded, pc) {
        parts.push(call_plan::reload_part(place, register)?);
    }
    let row = chained(parts, spills.len(), Vec::new())?;
    match decoded.target {
        Some(target) => branching(row, layout, (pc, target)),
        None => Ok(row),
    }
}

/// The target hole of a record that jumps nowhere: no next PC equals it.
const NO_TARGET: u64 = u64::MAX;

/// The semantic call of the record at `pc`: holes the pc, the entry, the operands and the jump target (none
/// as `NO_TARGET`), with the stop's exit as its only exit.
fn semantic_part(pc: u32, semantic: Semantic, target: Option<u32>) -> Result<CallPart, String> {
    let target = target.map_or(NO_TARGET, u64::from);
    let holes = padded_holes([u64::from(pc), semantic.entry, semantic.operands, target]);
    Ok(CallPart { stencil: call::stencil(CallRow::Semantic)?, holes, taken: None, exits: vec![Exit::Status], symbol: None })
}
