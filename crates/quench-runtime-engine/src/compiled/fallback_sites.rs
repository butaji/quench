//! The expensive fallbacks a run of compiled code takes, noted for `QUENCH_DEV=strict-fallback` and `=report`
//! (`quench_runtime::fallback`): a loop no region runs, a region or body that left its code, and a body's call of a
//! native no staged row runs. Each is cold, called only where the switch is on.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::engine_types::LaneContext;
use quench_runtime::tier_surface::Native;

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


