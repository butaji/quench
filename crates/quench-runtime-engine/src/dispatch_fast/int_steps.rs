//! Loop steps the registers' int32 kinds prove (`int_kinds`): the `(IncDec, Jump)` back edge over a fixed
//! register-register comparison takes its `PROVEN_STEP` form (`inc_dec_test_row`), at load (`prove_loop_steps`) or when a
//! loop site that held the back edge retires (`retired_step`); and what native code hands back that retires the
//! speculative kinds (`native_handed_doubles`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{checked_step, int_kinds, proven_step_forms, Handler, Ip, LaneContext, LaneInstruction, LoopSite, Operand};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::meaning::{means, Meaning};
use quench_runtime::vm::engine_types::LaneView;

/// The counter of `step` where it is `i++` in place, with no numeric local target and no flags.
fn in_place_counter(step: WideInstruction) -> Option<u16> {
    let in_place = means(step, Meaning::IncDec) && step.numeric_local_target().is_none() && step.boolean_flag() == Some(false)
        && step.result_flags() == 0 && step.result_register() == step.register_b();
    in_place.then(|| step.register_b())
}

/// The `(IncDec, Jump)` back edges whose step and test the int32 kinds prove: `i++` in place of a register of an int32
/// kind, then a fixed register-register comparison whose left operand is that register. Each takes its `PROVEN_STEP`
/// form, which reads the counter with no tag test and gives the test its new value as held; one over a speculative
/// counter is listed in `speculating` with its checked form (`LaneView::retire_int_speculation`). Returns the steps a
/// loop site holds the back edge after (`held_counters`), which fuse only when the site retires.
pub(crate) fn prove_loop_steps(decoded: &[WideInstruction], records: &mut [LaneInstruction], kinds: &int_kinds::IntKinds,
    speculating: &mut Vec<(u32, *const (), *const ())>) -> Vec<(u32, u16, bool)> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    for head in 0..records.len().saturating_sub(1) {
        let Some((checked, proven)) = checked_step(records[head].handler) else { continue };
        let Some(counter) = in_place_counter(decoded[head]) else { continue };
        let kind = kinds.of(counter);
        let Ok(target) = usize::try_from((head + 1) as isize + records[head + 1].jump() / record) else { continue };
        let tests = decoded.get(target).is_some_and(|&test| means(test, Meaning::CompareJump)) && Operand(records[target].b).register_index() == Some(counter);
        if kind == int_kinds::Kind::Tagged || !tests {
            continue;
        }
        if kind == int_kinds::Kind::Speculative {
            speculating.push((head as u32, proven as *const (), checked as *const ()));
        }
        records[head].handler = proven as *const ();
    }
    held_counters(decoded, kinds, speculating)
}

/// The steps before a backward jump, which `prove_loop_steps` cannot prove where a loop site holds that jump: `i++` in place of a
/// register of an int32 kind, then a backward jump. Each is listed with its counter and whether its kind is speculative;
/// a speculative one's proven forms join `speculating`, which restores a form only where it stands.
fn held_counters(decoded: &[WideInstruction], kinds: &int_kinds::IntKinds,
    speculating: &mut Vec<(u32, *const (), *const ())>) -> Vec<(u32, u16, bool)> {
    let held = |head: usize| quench_runtime::staged::jumps(decoded[head + 1].op()) && decoded[head + 1].jump_target() <= head as u32 + 1;
    let mut counters = Vec::new();
    for head in (0..decoded.len().saturating_sub(1)).filter(|&head| held(head)) {
        let Some(counter) = in_place_counter(decoded[head]) else { continue };
        let kind = kinds.of(counter);
        if kind == int_kinds::Kind::Tagged {
            continue;
        }
        let speculative = kind == int_kinds::Kind::Speculative;
        if speculative {
            speculating.extend(proven_step_forms().into_iter().map(|(checked, proven)| (head as u32, proven as *const (), checked as *const ())));
        }
        counters.push((head as u32, counter, speculative));
    }
    counters
}

/// The view of `site`'s function, where it has one.
fn site_view(cx: *mut LaneContext, site: &LoopSite) -> Option<&'static LaneView> {
    // SAFETY: the program's views outlive the lane run.
    unsafe { &*(*cx).views }.get(site.function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)
}

/// `fused`, the back edge retiring `site` gives its `IncDec` record `update` (`update_fusion`), in its proven form where
/// the view's kinds proved that record's counter (`held_counters`, a speculative one while speculation stands) and the
/// loop's test `test` compares it: the step the loader's `prove_loop_steps` gives.
pub(crate) fn retired_step(cx: *mut LaneContext, site: &LoopSite, update: Ip, test: &LaneInstruction, fused: Handler) -> Handler {
    let Some(view) = site_view(cx, site) else { return fused };
    let Some((_, proven)) = checked_step(fused as *const ()) else { return fused };
    // SAFETY: the update is a record of the view.
    let Ok(pc) = u32::try_from(unsafe { update.offset_from(view.records()) }) else { return fused };
    let proved = view.proven_counters.iter().any(|&(at, counter, speculative)| at == pc
        && (!speculative || int_kinds::speculation_allowed()) && Operand(test.b).register_index() == Some(counter));
    if proved { proven } else { fused }
}

/// Whether the native code of `site`'s region, which ended at `exit` with its activation's registers `registers`, handed
/// the lane what it may have stepped as doubles (`admission::retire_after_native`): native frames it left, or, where it
/// hands back its own activation (any exit but a return), a register its view reads as a `Speculative` int32 that no
/// longer holds one.
pub(crate) fn native_handed_doubles(cx: *mut LaneContext, site: &LoopSite, exit: crate::compiled::LaneExit, holds_int: impl Fn(u16) -> bool) -> bool {
    let stands = || site_view(cx, site).is_some_and(|view| view.speculative_registers.iter().all(|register| holds_int(*register)));
    (!matches!(exit, crate::compiled::LaneExit::Returned { .. }) && !stands()) || super::native_frames::left_native_frames(cx)
}
