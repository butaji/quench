//! An exception no handler of a windowed activation covers ends the activation in its owner
//! instead of making the activation a frame for the general loop to unwind
//! (`settle_raised_window`): a generator completes (its record is done) and its caller, whose
//! step raised, continues unwinding; an async function or async generator's job ends with the
//! error (`JobEnd::Threw`), which its promise or request settles as a rejection.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::lane_roots::LaneRoot;
use super::*;

/// Ends the innermost activation, a window whose function has no handler for the exception in
/// `Vm::lane_raised` at `record`, and goes on from where it settled: the record its caller
/// unwinds from, or the exit of a job's run. `None`, before any effect, where the activation is
/// no window, owns open loops (whose iterators a frame's unwinding closes) or runs beside a
/// native's deferred call.
pub(crate) fn settle_raised_window(cx: *mut LaneContext, record: Ip) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    if vm.lane_deferred.is_some() || vm.lane_stack.native_boundary.is_some() {
        return None;
    }
    let depth = vm.lane_stack.activation_count().checked_sub(1)?;
    let activation = vm.lane_stack.top_frame()?;
    // An in-place request's throw fails it where its frame unwinds (`fail_materialized_request`).
    let script = activation.script().filter(|script| script.completion.is_windowed() && script.completion != Completion::AsyncRequest)?;
    if vm.lane_stack.iterators.last().is_some_and(|(owner, _)| *owner == depth) {
        return None;
    }
    // SAFETY: the context's program outlives the lane run, and `record` is one of the running
    // function's records.
    let (code, call_pc) = unsafe { (&*(*cx).code, record.offset_from((*cx).base) as u32) };
    if code.function(activation.function() as usize).handler_at(call_pc).is_some() {
        return None;
    }
    let owner = unsafe { (*cx).root };
    let popped = vm.lane_stack.pop_activation()?;
    restore_caller_in_lane(cx, &popped);
    Some(match (owner, script.completion) {
        (LaneRoot::Job(job), Completion::ResumesJob) => {
            vm.finish_job_window(job);
            exit(record)
        }
        _ => {
            vm.finish_generator_window(script.callee);
            raised_exit_in_lane(cx, activation.call())
        }
    })
}
