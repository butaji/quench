//! A function's lane body compiled ahead, its leaves bound to its base body: where the lane
//! lowering leaves at record `pc` (`LaneExit::At`), the emitted exit writes the lane's live
//! values to the Frame in the layout the base body reads and jumps to `Symbol::BasePc`, so the
//! base body continues the same activation. Inlined-callee leaves materialize the callee's
//! activation first, as the JIT's `body_inlined_activation_in_lane` does, and enter the
//! callee's base body. The entry of the function is the lane body where there is one, the
//! base body otherwise; `ahead_entry`'s per-PC loop is gone.
//!
//! Owner: lane/base join (and the deletion of the per-PC fallback).

use super::{ImageTarget, LaneImage, base::BaseImage, symbols::SymbolTable};
use crate::bytecode::ResidualProgram;
use crate::compile::aot_facts::AotFacts;

/// The lane body of function `id` with its leaves entering `base`, or `None` where the lane
/// lowering gives no native record (the base body is then the entry).
pub(crate) fn lower_lane(
    program: &ResidualProgram,
    facts: &AotFacts,
    target: ImageTarget,
    id: u32,
    base: &BaseImage,
    symbols: &mut SymbolTable,
) -> Result<Option<LaneImage>, String> {
    // TODO: lower for `target.isa`; the host's ISA until `IsaBaseline` selects the compile target.
    let _ = target;
    Ok(super::super::compiled::closed_lane_image(program, id, facts, base, symbols))
}
