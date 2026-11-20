//! The generic semantic row: an instruction no stencil row takes, run by its opcode's semantic entry
//! (`Op::native_semantic_entry`) inside the body, so an AOT base body never leaves to an interpreter
//! or a per-PC loop. The row is the chain of `call_plan::plan_step` with the semantic call in place of
//! the helper: the live pinned registers are written back to the frame, the entry runs the record
//! (writing its next PC to a word), and the registers live at the next record are reloaded. A
//! successor that is the record's jump target branches there (`branching`); any other successor is
//! the next record. A stop leaves with the entry's (status, word), which is already the body's own
//! protocol: the entry settles a return as `LANE_BODY_RETURNED`, an exception as `LANE_BODY_THREW`
//! and a termination as `TERMINATED_STATUS` (`semantic_stop::settle`).
//!
//! The entry and the operand record are symbols the image's loader resolves: `Symbol::SemanticEntry`
//! of the record's opcode and `Symbol::Operands` of its (function, PC), so the row names no address
//! of the compiling process.

use super::*;
use super::branches::branching;
use crate::vm::aot_image::Symbol;
use quench_bjit::padded_holes;

/// The semantic row's holes that name symbols: its entry and its operand record (`call_semantic`'s V1, V2).
const ENTRY_HOLE: u8 = 1;
const OPERANDS_HOLE: u8 = 2;

/// The target hole of a record that jumps nowhere: no next PC equals it.
const NO_TARGET: u64 = u64::MAX;

/// What a generic row calls, as the symbols it names: the record's opcode's semantic entry and the
/// operand record of its (function, PC) (`NativeJsOperands`), with the caller PC the record runs at
/// and the one it jumps to, where it jumps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm::compiled::base_region) struct Semantic {
    pub(in crate::vm::compiled::base_region) entry: Symbol,
    pub(in crate::vm::compiled::base_region) operands: Symbol,
    record: u32,
    jump: Option<u32>,
}

impl Semantic {
    /// The semantic call of the caller record that row `pc` of `layout` runs; none for a row of an
    /// inlined callee, whose record is no instruction of the activation the entry runs on.
    pub(in crate::vm::compiled::base_region) fn of(layout: &Layout, pc: u32) -> Result<Self, String> {
        if layout.inlined(pc) {
            return Err("generic row: an inlined callee's record".into());
        }
        let record = layout.record(pc);
        let instruction = layout.span.instruction(record).ok_or("generic row: no instruction")?;
        Ok(Self {
            entry: Symbol::SemanticEntry(instruction.op() as u16),
            operands: Symbol::Operands { function: layout.id, pc: record },
            record,
            jump: instruction.op().control_flow_layout().jumps().then(|| instruction.jump_target()),
        })
    }

    /// The row the record's jump lands on: the stitch's own target where the row carries one, else the
    /// first row of the jump's caller PC (`Layout::entries`).
    fn target(self, layout: &Layout, decoded: &Decoded) -> Result<Option<u32>, String> {
        let Some(jump) = self.jump else { return Ok(None) };
        if decoded.target.is_some() {
            return Ok(decoded.target);
        }
        let entry = layout.entries.binary_search_by_key(&jump, |&(pc, _)| pc).map_err(|_| "generic row: no row at the jump's target")?;
        Ok(Some(layout.entries[entry].1))
    }
}

/// The slow arm of a guard (`Step::Guard`) under `LaneHelpers::Ahead`: the record's generic row, as an
/// image body never leaves; none elsewhere, where the guard's miss leaves for the interpreter.
pub(in crate::vm::compiled::base_region) fn ahead_miss(decoded: &Decoded, layout: &Layout, pc: u32, helpers: LaneHelpers) -> Result<Option<Planned>, String> {
    if !helpers.generic_rows() {
        return Ok(None);
    }
    generic_row(layout, decoded, pc, Semantic::of(layout, pc)?).map(Some)
}

/// The generic row of the record at `pc`, which `decoded` holds as a `Step::Generic` (`steps::lowering`
/// under `LaneHelpers::Ahead`). Its successors are the next record and, where the record jumps, its
/// target; the registers it reads and writes are the frame's, as every semantic step's are.
pub(in crate::vm::compiled::base_region) fn generic_row(layout: &Layout, decoded: &Decoded, pc: u32, semantic: Semantic) -> Result<Planned, String> {
    let spills = layout.spills(pc);
    let mut parts = spills.iter().map(|&(place, register)| call_plan::spill_part(place, register)).collect::<Result<Vec<_>, _>>()?;
    parts.push(semantic_part(semantic)?);
    for &(place, register) in &reloads(layout, decoded, pc) {
        parts.push(call_plan::reload_part(place, register)?);
    }
    let row = chained(parts, spills.len(), Vec::new())?;
    match semantic.target(layout, decoded)? {
        Some(target) => branching(row, layout, (pc, target)),
        None => Ok(row),
    }
}

/// The semantic call of a record: holes its caller PC, the entry, the operands and the caller PC it
/// jumps to (none as `NO_TARGET`), the entry and operands as symbols, with the stop's exit as its
/// only exit.
fn semantic_part(semantic: Semantic) -> Result<CallPart, String> {
    let holes = padded_holes([u64::from(semantic.record), 0, 0, semantic.jump.map_or(NO_TARGET, u64::from)]);
    let symbols = [Some((ENTRY_HOLE, semantic.entry)), Some((OPERANDS_HOLE, semantic.operands))];
    Ok(CallPart { stencil: call::stencil(CallRow::Semantic)?, holes, taken: None, exits: vec![Exit::Status], symbols })
}
