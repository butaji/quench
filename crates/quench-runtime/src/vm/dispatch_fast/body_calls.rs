//! A lane body's call of a callee that does not complete in a body of its own: the callee's
//! activation runs in the lane, nested under the body, and its return comes back to the
//! body's call, so the caller's specialized code keeps running (V8's and JSC's baseline
//! callers return into compiled code whatever tier the callee ran in).
use super::*;
use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT};
use super::lane_body_tier::{audit_nanos, Timed};

/// The end of a nested run at the return with `value` of `activation`, popped with its
/// caller's state restored: a native's callback's (`Completion::ReturnsToNative`), whose
/// native takes the value, or a body's callee's, whose call's register takes it, a
/// construction's receiver (`this`) for a non-object. `None` for any other completion.
#[inline(always)]
pub(super) fn ends_run<H: Host>(cx: *mut LaneContext<H>, activation: &Activation, completion: Completion, this: Value, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    match completion {
        Completion::ReturnsToNative => {}
        Completion::ReturnsToBody | Completion::ConstructsToBody => {
            let value = setters::completed_value(vm, activation, completion, this, value);
            // SAFETY: the call record belongs to the caller's live view, whose registers the context names.
            let destination = unsafe { (*activation.call).a } & crate::bytecode::REGISTER_MASK;
            unsafe { (*cx).registers }.set(destination, value);
        }
        _ => return None,
    }
    vm.lane_native_returned = Some(value);
    Some(Resumed::Exit(exit(activation.call)))
}

/// Whether `activation`, which becomes the frame at index `frame`, is the entry frame of
/// the general loop that finishes it, as a native's callback is. A body's callee is not:
/// its frame's return leaves the lane instead (`LaneStack::body_boundary`), which the
/// innermost such frame decides.
pub(super) fn enters_as_frame(stack: &mut LaneStack, activation: &Activation, frame: usize) -> bool {
    match activation.script().map(|script| script.completion) {
        Some(Completion::ReturnsToNative) => true,
        Some(Completion::ReturnsToBody | Completion::ConstructsToBody) => {
            stack.body_boundary = Some(frame);
            false
        }
        _ => false,
    }
}

/// Whether `resume`, the record a body's callee left at, is where the lane left for the
/// general path (`finish`), not a record the lane continues at.
#[inline(always)]
pub(super) fn left_lane(resume: Ip) -> bool {
    resume.addr() & EXIT_TAG != 0
}

/// Changes the completion of the lane's `depth`th activation `from` one `to` another;
/// false, changing nothing, where it holds another or is no script activation.
fn recomplete<H: Host>(cx: *mut LaneContext<H>, depth: usize, from: Completion, to: Completion) -> bool {
    match unsafe { vm(cx) }.lane_stack.activations.get_mut(depth).map(|activation| &mut activation.kind) {
        Some(ActivationKind::Script(script)) if script.completion == from => {
            script.completion = to;
            true
        }
        _ => false,
    }
}

/// Leaves the body for the lane at `resume`, the callee's activation's record
/// (`LaneExit::Callee`).
fn leave<H: Host>(cx: *mut LaneContext<H>, resume: Ip, acc: Value) -> u64 {
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = acc; }
    LANE_CALL_LEFT
}

/// The rest of the activation a body's call made, the lane's `depth`th, from its record
/// `resume` with accumulator `acc`: run in the lane to its return, which completes the call
/// (`LANE_CALL_COMPLETED`). A run that leaves the lane for the general path leaves the body
/// too, at the record the lane left at (`left_lane`), as does a call no nested run takes:
/// one below such a leave, or without native stack.
pub(super) fn finish<H: Host>(cx: *mut LaneContext<H>, depth: usize, resume: Ip, acc: Value, constructs: bool) -> u64 {
    let (plain, nested) = if constructs { (Completion::Constructs, Completion::ConstructsToBody) } else { (Completion::Plain, Completion::ReturnsToBody) };
    let Ok(_stack) = crate::stack::StackGuard::enter() else { return leave(cx, resume, acc) };
    if left_lane(resume) || !recomplete(cx, depth, plain, nested) {
        return leave(cx, resume, acc);
    }
    let left = run_nested(cx, resume, acc);
    recomplete(cx, depth, nested, plain);
    ended(cx, left)
}

/// The status of a body's call whose nested run ended as `run_nested` says.
fn ended<H: Host>(cx: *mut LaneContext<H>, left: Option<Ip>) -> u64 {
    left.map_or(LANE_CALL_COMPLETED, |left| leave(cx, exit(left), Value::UNDEFINED))
}

/// The lane run from `resume` with accumulator `acc`, nested under the running body: `None`
/// where it ended with a value for the body's call (`ends_run`), else the record the lane
/// left at.
fn run_nested<H: Host>(cx: *mut LaneContext<H>, resume: Ip, acc: Value) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    unsafe {
        (*cx).resumed_acc = Slot::of(acc);
        (*cx).memory = MemoryView::script(cx);
    }
    let outer = vm.lane_stack.body_boundary.take();
    let left = lane_dispatch(cx, resume);
    vm.lane_stack.body_boundary = outer;
    vm.lane_native_returned.take().is_none().then_some(left)
}

/// A body's call of an array callback method whose first callback's activation the body made
/// (`array_steps::lane_body_array_iteration`), its record `resume`: run in the lane until the
/// iteration completes the call (`LANE_CALL_COMPLETED`); a run that leaves the lane for the
/// general path leaves the body at the record it left at.
pub(super) fn finish_array_iteration<H: Host>(cx: *mut LaneContext<H>, resume: Ip) -> u64 {
    ended(cx, run_nested(cx, resume, Value::UNDEFINED))
}

/// An entry of the function `own` views that `timed` (`LaneView::audit_entry`) put on the
/// interpreter's side: `Err` with the record the lane continues at after the entry ran
/// interpreted and timed (the activation, run from its first record `start` nested to its
/// return, completed, so its caller continues after the call, or left the lane, the exit
/// record); `Ok` for an entry that runs its body, timed or not. An interpreted side runs only
/// for the innermost activation of a plain call or construction of this function whose call
/// keeps its result; any other entry runs its body untimed.
pub(super) fn audited<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, function: u32, start: Ip, timed: Option<Timed>) -> Result<(), Ip> {
    if timed != Some(Timed::Interpreter) {
        return Ok(());
    }
    let vm = unsafe { vm(cx) };
    let Some(depth) = vm.lane_stack.activations.len().checked_sub(1) else { return Ok(()) };
    let top = &vm.lane_stack.activations[depth];
    let constructs = match top.script().map(|script| script.completion) {
        Some(Completion::Plain) => false,
        Some(Completion::Constructs) => true,
        _ => return Ok(()),
    };
    let call = top.call;
    // SAFETY: the call record belongs to the caller's live view.
    if top.function != function || unsafe { (*call).a } & crate::bytecode::RETURN_REGISTER != 0 {
        return Ok(());
    }
    let started = std::time::Instant::now();
    if finish(cx, depth, start, Value::UNDEFINED, constructs) == LANE_CALL_COMPLETED {
        own.audit_record(Timed::Interpreter, audit_nanos(started));
        return Err(following(call));
    }
    // SAFETY: `finish` set it before it left.
    let resume = unsafe { (*cx).callee_resume };
    if left_lane(resume) { Err(resume) } else { Ok(()) }
}
