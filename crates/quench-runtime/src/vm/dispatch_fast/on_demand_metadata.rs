//! Lane metadata made only once a reader needs it: interpreted execution keeps what the
//! interpreter reads, not what a tier it never runs would.
use super::*;

impl<H: Host> Vm<H> {
    /// `LaneStack::note_site_callee` while a lane body may read the history: only the
    /// JavaScript tiers inline a site's callees (`lane_site_targets`), so interpreted
    /// execution keeps no history, as V8's jitless mode allocates no feedback for an
    /// optimizing compiler it never runs (mandreel: 12.5 MB of history at its peak).
    pub(super) fn note_site_callee(&self, link: &std::cell::Cell<u16>, target: super::super::compiled::CallTarget) {
        if self.js_tier_runs() {
            self.lane_stack.note_site_callee(link, target);
        }
    }
}

/// Whether the lane completes `call`, the general path's call of the innermost `frame`,
/// itself: an async callee's promise settles on the general path
/// (`PendingGeneralCall::completion`), and a caller the general path runs without a view
/// (`ProgramStore::entry_lane_view` deferred it) resumes there unless a return already
/// retired (`retired`), rather than deriving a view of every instruction of a function that
/// may run once (mandreel's `global_init`: 614K records, 14 MB).
pub(super) fn lane_completes_return<H: Host>(cx: *mut LaneContext<H>, call: &PendingGeneralCall, frame: usize, retired: bool) -> bool {
    let vm = unsafe { vm(cx) };
    call.caller + 1 == frame
        && frame + 1 == vm.frames.len()
        && !call.completion.completes_on_general_path()
        && (retired || !caller_runs_general(cx, call))
}

/// Whether `call`'s caller is a function of the running program whose view is not derived,
/// so the general path runs it.
fn caller_runs_general<H: Host>(cx: *mut LaneContext<H>, call: &PendingGeneralCall) -> bool {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's table holds the running program's views for the lane run.
    call.caller_program.is_none()
        && unsafe { &*(*cx).views }
            .get(vm.frames[call.caller].function as usize)
            .is_some_and(|view| super::super::program_store::derived_lane_view(view).is_none())
}
