//! Lane metadata made only once a reader needs it: interpreted execution keeps what the
//! interpreter reads, not what a tier it never runs would.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Whether the lane completes `call`, the general path's call of the innermost `frame`,
/// itself: an async callee's promise settles on the general path
/// (`PendingGeneralCall::completion`), and a caller the general path runs without a view
/// (`ProgramStore::entry_lane_view` deferred it) resumes there, rather than deriving a view of
/// every instruction of a function that may run once (mandreel's `global_init`: 614K records,
/// 14 MB; babylonjs's module scope: 124K records, each iteration). Where a return already
/// `retired`, only a caller that loops takes its view here; any other frame returns at its
/// call record on the general path (`frame_return`).
pub(crate) fn lane_completes_return(cx: *mut LaneContext, call: &PendingGeneralCall, frame: usize, retired: bool) -> bool {
    let vm = unsafe { vm(cx) };
    call.caller + 1 == frame
        && frame + 1 == vm.frames.len()
        && vm.lane_stack.body_boundary != Some(frame)
        && !call.completion.completes_on_general_path()
        && (!caller_runs_general(cx, call) || retired && caller_loops(cx, call))
}

/// Whether `call`'s caller runs without a derived view, so the general path runs it. A caller
/// in another program (`PendingGeneralCall::caller_program`) is read in that program's table:
/// mandreel's `global_init` calls its emitters across programs, and returning into it derived a
/// view of all its 614K records for one run (peak RSS 110.9 -> 84.4 MB, no-jit).
fn caller_runs_general(cx: *mut LaneContext, call: &PendingGeneralCall) -> bool {
    let vm = unsafe { vm(cx) };
    let function = vm.frames[call.caller].function as usize;
    let views = match &call.caller_program {
        // SAFETY: the context's table holds the running program's views for the lane run.
        None => unsafe { &*(*cx).views },
        Some(saved) => match vm.programs.views_in_lane(saved.id) {
            Some(views) => views,
            None => return false,
        },
    };
    views.get(function).is_some_and(|view| quench_runtime::vm::program_store::derived_lane_view(view).is_none())
}

/// Whether `call`'s caller loops, so its view repays itself where its loop runs on.
fn caller_loops(cx: *mut LaneContext, call: &PendingGeneralCall) -> bool {
    let vm = unsafe { vm(cx) };
    let program = call.caller_program.as_ref().map_or(unsafe { (*cx).program }, |saved| saved.id);
    vm.programs.function_loops(program, vm.frames[call.caller].function)
}

