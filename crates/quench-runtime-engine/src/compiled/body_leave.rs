//! The trace of a lane body's run that ends anywhere but its completion (`QUENCH_TRACE=body-leave`).

use quench_runtime::vm::engine_types::LaneContext;

/// A run of a body that ended at `exit`: traced (`Channel::BodyLeave`) unless it returned.
#[inline(always)]
pub(crate) fn body_ended(cx: *mut LaneContext, exit: crate::compiled::LaneExit) {
    if !matches!(exit, crate::compiled::LaneExit::Returned { .. }) && quench_runtime::trace::Channel::BodyLeave.on() {
        eprintln!("QUENCH_BODY_LEAVE function={} exit={exit:?}", crate::dispatch_fast::running_function_in_lane(cx));
    }
}
