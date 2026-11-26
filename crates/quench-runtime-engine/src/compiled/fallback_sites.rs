//! The expensive fallbacks a run of compiled code takes, noted for `QUENCH_DEV=strict-fallback` and `=report`
//! (`quench_runtime::fallback`): a loop no region runs, a region or body that left its code, and a body's call of a
//! native no staged row runs. Each is cold, called only where the switch is on.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::engine_types::LaneContext;
use quench_runtime::vm::engine_types::LoopSite;
use crate::dispatch_fast::LoopContinuation;
use quench_runtime::tier_surface::Native;

/// A hot loop whose region analysis rejected an instruction, and which no body runs either, as
/// an expensive fallback (`quench_runtime::fallback`): every iteration interprets.
#[cold]
pub(crate) fn loop_rejected(cx: *mut LaneContext, site: &LoopSite) {
    let Some(failure) = site.analysis_failure else { return };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    quench_runtime::fallback::note(quench_runtime::fallback::Fallback::JitReject,
        format_args!("loop {}", crate::compiled::admission::Refusal::from(failure.reason).describe()),
        quench_runtime::fallback::Site::new(code, site.function, failure.pc));
}

/// A loop region's run that left its code for the interpreter inside the loop, as an expensive
/// fallback (`quench_runtime::fallback`).
#[cold]
pub(crate) fn loop_left(cx: *mut LaneContext, site: &LoopSite, continuation: &LoopContinuation) {
    let (pc, how) = match continuation {
        // A run that reaches a jump out of its region (its backward jump, or an enclosing loop's) ended, not fell back.
        LoopContinuation::Exit(pc) if *pc == site.backedge || jumps_at(cx, site.function, *pc) => return,
        LoopContinuation::Exit(pc) => (*pc, "loop region left"),
        LoopContinuation::Inlined { pc, .. } => (*pc, "loop region left inlined"),
        LoopContinuation::Recovered(_) => (site.header, "loop region left inlined, recovered"),
        LoopContinuation::Callee => (site.header, "loop region left into a callee"),
        _ => return,
    };
    // An inlined leave's PC is its callee's, whose function the continuation does not keep: name the loop's.
    let pc = if matches!(continuation, LoopContinuation::Exit(_)) { pc } else { site.header };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    quench_runtime::fallback::note(quench_runtime::fallback::Fallback::Deopt, format_args!("{how}"),
        quench_runtime::fallback::Site::new(code, site.function, pc));
}

/// A loop region run that entered nothing (`reason`), counted on the site; one whose entry contract refused is noted
/// as an expensive fallback (`quench_runtime::fallback`) where the switch is on: the loop interprets this trip.
#[inline(always)]
pub(crate) fn entry_refused(cx: *mut LaneContext, site: &LoopSite, reason: crate::dispatch_fast::RegionDecline) -> LoopContinuation {
    if quench_runtime::fallback::on() && reason == crate::dispatch_fast::RegionDecline::EntryContract { loop_refused(cx, site); }
    site.declined(reason)
}

#[cold]
fn loop_refused(cx: *mut LaneContext, site: &LoopSite) {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    quench_runtime::fallback::note(quench_runtime::fallback::Fallback::Deopt, format_args!("loop region refused at entry"),
        quench_runtime::fallback::Site::new(code, site.function, site.header));
}

/// A run of a body that ended at `exit`: traced (`Channel::BodyLeave`) unless it returned, and
/// noted as an expensive fallback (`body_left`) where the switch is on.
#[inline(always)]
pub(crate) fn body_ended(cx: *mut LaneContext, method: &crate::compiled::LaneMethod, exit: crate::compiled::LaneExit) {
    if !matches!(exit, crate::compiled::LaneExit::Returned { .. }) && quench_runtime::trace::Channel::BodyLeave.on() {
        eprintln!("QUENCH_BODY_LEAVE function={} exit={exit:?}", crate::dispatch_fast::running_function_in_lane(cx));
    }
    if quench_runtime::fallback::on() { body_left(cx, method, exit); }
}

/// A run of a body that left its code before completing, as an expensive fallback
/// (`quench_runtime::fallback`): at the instruction it left at, with the guard that failed there.
#[cold]
pub(crate) fn body_left(cx: *mut LaneContext, method: &crate::compiled::LaneMethod, exit: crate::compiled::LaneExit) {
    use crate::compiled::LaneExit;
    let function = crate::dispatch_fast::running_function_in_lane(cx);
    let (function, pc, how) = match exit {
        LaneExit::At(pc) => (function, pc as u32, "left"),
        LaneExit::Inlined { frame, pc } => (method.inlined.get(frame).map_or(function, |call| call.target.function), pc, "left inlined"),
        LaneExit::Entry => (function, 0, "entry proof failed"),
        LaneExit::Callee => (function, 0, "left into a callee"),
        LaneExit::Returned { .. } | LaneExit::Threw { .. } | LaneExit::Terminated | LaneExit::Stub => return,
    };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let fact = method.exits.fact_name(exit).unwrap_or("no guard");
    quench_runtime::fallback::note(quench_runtime::fallback::Fallback::Deopt, format_args!("{how}, {fact}"),
        quench_runtime::fallback::Site::new(code, function, pc));
}

/// A compiled body's call of `native`, which no staged row runs inside the body, as an expensive
/// fallback (`quench_runtime::fallback`): the body calls out to the native's Rust code.
#[cold]
pub(crate) fn opaque_native(cx: *mut LaneContext, call: &crate::dispatch_fast::body_forms::BodyCall, code: &quench_runtime::ResidualProgram, native: Native) {
    // SAFETY: the call's record lies in the running view.
    let pc = unsafe { call.ip.offset_from((*cx).base) } as u32;
    quench_runtime::fallback::note(quench_runtime::fallback::Fallback::OpaqueNative, format_args!("{native:?}"),
        quench_runtime::fallback::Site::new(code, crate::dispatch_fast::running_function_in_lane(cx), pc));
}


/// Whether the instruction at `pc` of `function` is a plain jump.
fn jumps_at(cx: *mut LaneContext, function: u32, pc: u32) -> bool {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    code.function_get(function as usize).and_then(|body| body.code.get(pc as usize).map(|packed| body.widened(*packed)))
        .is_some_and(|instruction| matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Jump)))
}
