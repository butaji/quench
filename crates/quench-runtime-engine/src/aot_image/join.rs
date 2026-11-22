//! A function's lane body compiled ahead, its leaves bound to its base body: where the lane
//! lowering leaves at record `pc` (`LaneExit::At`), the emitted exit writes the lane's live
//! values to the Frame in the layout the base body reads and jumps to `Symbol::BasePc`, so the
//! base body continues the same activation. Inlined-callee leaves materialize the callee's
//! activation first, as the JIT's `body_inlined_activation_in_lane` does, and enter the
//! callee's base body. The entry of the function is the lane body where there is one, the
//! base body otherwise; `ahead_entry`'s per-PC loop is gone.
//!
//! Owner: lane/base join (and the deletion of the per-PC fallback).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{CallSite, ImageTarget, LaneImage, base::BaseImage, symbols::SymbolTable};
use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::compile::aot_facts::AotFacts;
use crate::compiled::ClosedFrozen;

/// The lane body of function `id` with its leaves entering `base`, its proven calls reading the
/// records of `calls` (the base body's), or `None` where the lane lowering gives no native record
/// (the base body is then the entry).
pub(crate) fn lower_lane(
    program: &ResidualProgram,
    (facts, frozen): (&AotFacts, &ClosedFrozen),
    target: &ImageTarget,
    id: u32,
    (base, calls): (&BaseImage, &[CallSite]),
    symbols: &mut SymbolTable,
) -> Result<Option<LaneImage>, String> {
    // TODO: lower for `target.isa`; the host's ISA until `IsaBaseline` selects the compile target.
    let _ = target;
    let lane = crate::compiled::closed_lane_image(program, id, (facts, frozen), (base, calls), symbols);
    if quench_runtime::trace::Channel::AotLanes.on() && let Err(reason) = lane {
        eprintln!("aot-lanes: function {id} ({} instructions): base body, {reason}", program.function(id as usize).instructions().count());
    }
    Ok(lane.ok())
}
