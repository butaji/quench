//! Loop steps the solve's int32 cells prove (`int32_cell`): an `i++` back edge over a register-register test takes
//! its `PROVEN_STEP` form, at load (`prove_loop_steps`) or when the loop site holding it retires (`retired_step`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{checked_step, int32_cell, operand_type, Handler, Ip, LaneContext, LaneInstruction, LoopSite, Operand};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::compile::aot_facts::AotFacts;
use quench_runtime::staged::meaning::{means, Meaning};
use quench_runtime::vm::engine_types::LaneView;

/// The counter of `step` where it is a flagless `i++` in place, when the solve's cell proves it int32 before the step.
fn int_counter(step: WideInstruction, pc: u32, facts: Option<&AotFacts>, function: u32) -> Option<u16> {
    let in_place = means(step, Meaning::IncDec) && step.numeric_local_target().is_none() && step.boolean_flag() == Some(false)
        && step.result_flags() == 0 && step.result_register() == step.register_b();
    (in_place && int32_cell(operand_type(facts, function, pc, step.register_b()))).then(|| step.register_b())
}

/// Gives each `(IncDec, Jump)` back edge over an int32 counter that its target's test compares its `PROVEN_STEP` form.
/// Returns the counters before a backward jump, which a loop site holds and which fuse only once it retires (`retired_step`).
pub(crate) fn prove_loop_steps(decoded: &[WideInstruction], records: &mut [LaneInstruction], facts: Option<&AotFacts>, function: u32) -> Vec<(u32, u16)> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    let mut held = Vec::new();
    for head in 0..decoded.len().saturating_sub(1) {
        let Some(counter) = int_counter(decoded[head], head as u32, facts, function) else { continue };
        if quench_runtime::staged::jumps(decoded[head + 1].op()) && decoded[head + 1].jump_target() <= head as u32 + 1 {
            held.push((head as u32, counter));
        }
        let Some((_, proven)) = checked_step(records[head].handler) else { continue };
        let Ok(target) = usize::try_from((head + 1) as isize + records[head + 1].jump() / record) else { continue };
        if !decoded.get(target).is_some_and(|&test| means(test, Meaning::CompareJump)) || Operand(records[target].b).register_index() != Some(counter) { continue }
        records[head].handler = proven as *const ();
    }
    held
}

/// The view of `site`'s function, where it has one.
fn site_view(cx: *mut LaneContext, site: &LoopSite) -> Option<&'static LaneView> {
    // SAFETY: the program's views outlive the lane run.
    unsafe { &*(*cx).views }.get(site.function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)
}

/// `fused`, the step retiring `site` gives its `update` record, in its proven form where the loader held that counter
/// (`prove_loop_steps`) and the loop's `test` compares it.
pub(crate) fn retired_step(cx: *mut LaneContext, site: &LoopSite, update: Ip, test: &LaneInstruction, fused: Handler) -> Handler {
    let Some(view) = site_view(cx, site) else { return fused };
    let Some((_, proven)) = checked_step(fused as *const ()) else { return fused };
    // SAFETY: the update is a record of the view.
    let Ok(pc) = u32::try_from(unsafe { crate::dispatch_fast::pc_in_view(view.records(), update) }) else { return fused };
    let proved = view.proven_counters.iter().any(|&(at, counter)| at == pc && Operand(test.b).register_index() == Some(counter));
    if proved { proven } else { fused }
}
