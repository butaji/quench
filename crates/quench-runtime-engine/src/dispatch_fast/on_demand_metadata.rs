//! Lane metadata made only once a reader needs it: interpreted execution keeps what the
//! interpreter reads, not what a tier it never runs would.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

#[quench_macros::ext(DispatchFastOnDemandMetadataVm)]
impl Vm {
    /// `LaneStack::note_site_callee` while a lane body may read the history: only the
    /// JavaScript tiers inline a site's callees (`site_targets_in_lane`), so interpreted
    /// execution keeps no history, as V8's jitless mode allocates no feedback for an
    /// optimizing compiler it never runs (mandreel: 12.5 MB of history at its peak).
    pub(crate) fn note_site_callee(&self, link: &std::cell::Cell<u16>, target: crate::compiled::CallTarget) {
        if self.tier_may_run(crate::compiled::admission::Tier::ANY) {
            self.lane_stack.note_site_callee(link, target);
        }
    }
}

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

/// A helper call to a callee the caller's code could call by its entry, which it compiled before: past
/// `late_call_calls` the body specializes again and a calling loop region's code goes (`LoopTier::late_callee`),
/// answered true so the call leaves the region. A settled site (`LaneContext::late_settled`) counts no more.
pub(crate) fn record_late_call(cx: *mut LaneContext, ip: Ip, target: &ScriptCallTarget, argc: u16) -> bool {
    // A loop region's direct call of another closure of the function it plans rebinds to it (`DirectCallee::bound`): the
    // site's plan stands, so the call counts as no late callee.
    if crate::compiled::direct_callee::rebind_closure(cx, ip, target, argc) { return false; }
    if unsafe { (*cx).late_settled } == ip { return false; }
    let vm = unsafe { vm(cx) };
    // The innermost frame's function: a native callee's frame is linked, so it is the activation's.
    let function = match vm.lane_stack.top_frame() {
        Some(activation) => activation.function(),
        None => { let Some(frame) = vm.frames.get(unsafe { (*cx).frame() }) else { return false }; frame.function }
    };
    let Some(caller) = unsafe { &*(*cx).views }.get(function as usize).and_then(super::program_store::derived_lane_view) else { return false };
    if !caller.owns_record(ip) { return false; }
    if caller.note_late_call() {
        // A callee still warming toward its body is called by its entry once it has one: count again until then.
        if unsafe { &*(*cx).views }.get(target.function as usize).and_then(super::program_store::derived_lane_view).is_some_and(|own| own.warming()) { caller.recount_late_calls(); return false; }
        if direct_callee_in_lane(vm, caller, function, 0, argc, *target).is_none() { return false; }
        caller.respecialize();
        let (pc, region) = (unsafe { ip.offset_from(caller.records()) } as u32, unsafe { (*cx).region_payload } != 0);
        return caller.loops.iter().filter(|site| region && (site.header..=site.backedge).contains(&pc))
            .fold(false, |left, site| site.tier.borrow_mut().late_callee(site.tiers) | left);
    } else if caller.late_calls_settled() { unsafe { (*cx).late_settled = ip } }
    false
}
