//! Loop steps the registers' int32 kinds prove (`int_kinds`): an `i++` back edge over a register-register test takes
//! its `PROVEN_STEP` form, at load (`prove_loop_steps`) or when the loop site holding it retires (`retired_step`);
//! and what native code hands back that retires speculative kinds (`native_handed_doubles`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{checked_step, int_kinds, proven_step_forms, Handler, Ip, LaneContext, LaneInstruction, LoopSite, Operand};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::meaning::{means, Meaning};
use quench_runtime::vm::engine_types::LaneView;

/// The counter of `step` where it is a flagless `i++` in place, and its kind where that is an int32 one.
fn int_counter(step: WideInstruction, kinds: &int_kinds::IntKinds) -> Option<(u16, bool)> {
    let in_place = means(step, Meaning::IncDec) && step.numeric_local_target().is_none() && step.boolean_flag() == Some(false)
        && step.result_flags() == 0 && step.result_register() == step.register_b();
    let kind = kinds.of(step.register_b());
    (in_place && kind != int_kinds::Kind::Tagged).then(|| (step.register_b(), kind == int_kinds::Kind::Speculative))
}

/// Gives each `(IncDec, Jump)` back edge over an int32 counter that its target's test compares its `PROVEN_STEP` form; a
/// speculative one joins `speculating` with its checked form. Returns the counters before a backward jump, which a loop
/// site holds and which fuse only once it retires (`retired_step`).
pub(crate) fn prove_loop_steps(decoded: &[WideInstruction], records: &mut [LaneInstruction], kinds: &int_kinds::IntKinds,
    speculating: &mut Vec<(u32, *const (), *const ())>) -> Vec<(u32, u16, bool)> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    let mut held = Vec::new();
    for head in 0..decoded.len().saturating_sub(1) {
        let Some((counter, speculative)) = int_counter(decoded[head], kinds) else { continue };
        if quench_runtime::staged::jumps(decoded[head + 1].op()) && decoded[head + 1].jump_target() <= head as u32 + 1 {
            if speculative { speculating.extend(proven_step_forms().into_iter().map(|(checked, proven)| (head as u32, proven as *const (), checked as *const ()))); }
            held.push((head as u32, counter, speculative));
        }
        let Some((checked, proven)) = checked_step(records[head].handler) else { continue };
        let Ok(target) = usize::try_from((head + 1) as isize + records[head + 1].jump() / record) else { continue };
        if !decoded.get(target).is_some_and(|&test| means(test, Meaning::CompareJump)) || Operand(records[target].b).register_index() != Some(counter) { continue }
        if speculative { speculating.push((head as u32, proven as *const (), checked as *const ())); }
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
/// (`prove_loop_steps`, a speculative one while speculation stands) and the loop's `test` compares it.
pub(crate) fn retired_step(cx: *mut LaneContext, site: &LoopSite, update: Ip, test: &LaneInstruction, fused: Handler) -> Handler {
    let Some(view) = site_view(cx, site) else { return fused };
    let Some((_, proven)) = checked_step(fused as *const ()) else { return fused };
    // SAFETY: the update is a record of the view.
    let Ok(pc) = u32::try_from(unsafe { crate::dispatch_fast::pc_in_view(view.records(), update) }) else { return fused };
    let proved = view.proven_counters.iter().any(|&(at, counter, speculative)| at == pc
        && (!speculative || int_kinds::speculation_allowed()) && Operand(test.b).register_index() == Some(counter));
    if proved { proven } else { fused }
}

/// Whether `site`'s native code, ending at `exit`, handed back doubles (`admission::retire_after_native`): native frames it
/// left, or, short of a return, a register its view reads as `Speculative` that no longer holds an int32.
pub(crate) fn native_handed_doubles(cx: *mut LaneContext, site: &LoopSite, exit: crate::compiled::LaneExit, holds_int: impl Fn(u16) -> bool) -> bool {
    let stands = || site_view(cx, site).is_some_and(|view| view.speculative_registers.iter().all(|register| holds_int(*register)));
    (!matches!(exit, crate::compiled::LaneExit::Returned { .. }) && !stands()) || super::native_frames::left_native_frames(cx)
}
