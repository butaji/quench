//! An async generator's `next` the lane runs in place (`Completion::AsyncRequest`): the request's
//! promise is made, the generator resumes as an activation over its record's register file, as a
//! generator's step does (`generator_windows`), and the call yields the promise once the
//! activation stops: its `await` or `yield` suspends it back into its record, which awaits as the
//! request's job would (`Vm::await_request`), and its return completes the generator
//! (`Vm::complete_request`). A throw no handler of its function covers, or anything else the lane
//! leaves, makes it a frame whose pending call settles the request
//! (`Vm::settle_materialized_request`). Where the generator is running, done, has requests
//! queued or does not resume in this realm and program, the native runs the call, as the native
//! queues or resumes it in a run of its own (`Vm::resume_request`).
use super::*;

/// Whether the innermost activation is an async generator's in-place request.
pub(super) fn running_request(vm: &Vm) -> bool {
    vm.lane_stack.top_frame().and_then(|frame| frame.script()).is_some_and(|script| script.completion == Completion::AsyncRequest)
}

/// `next` of an async generator, called at `ip` with registers `r`: the generator resumes in
/// place with the value the call passes (`resume_request_in_lane`). `None`, before any effect,
/// leaves the call to the native.
#[inline(never)]
pub(super) fn async_generator_next_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let generator = r.get(i.c);
    if i.a & crate::bytecode::RETURN_REGISTER != 0 || !vm.requests_in_place(generator) {
        return None;
    }
    // SAFETY: the call's window is registers of the running function.
    let sent = unsafe { call_window_in_lane(r, i) }.first().copied().unwrap_or(Value::UNDEFINED);
    let (registers, start) = resume_request_in_lane(cx, r, ip, i, generator, sent)?;
    Some(Resumed::At(registers, start, Carried::script(cx)))
}

/// `throw` of an async generator suspended at a `yield` of its own (`Vm::throws_in_place`),
/// called at `ip` with registers `r`: the generator resumes in place and the error the call
/// passes raises at its `yield`, where a handler of its function catches it or its frame fails
/// the request (`settle_abandoned_call`). `None`, before any effect, leaves the call to the
/// native.
#[inline(never)]
pub(super) fn async_generator_throw_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let generator = r.get(i.c);
    if i.a & crate::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let (function, pc) = vm.throws_in_place(generator)?;
    // SAFETY: the call's window is registers of the running function.
    let thrown = unsafe { call_window_in_lane(r, i) }.first().copied().unwrap_or(Value::UNDEFINED);
    resume_request_in_lane(cx, r, ip, i, generator, Value::UNDEFINED)?;
    // The error raises at the `yield` the frame suspended at, the record before its resume point.
    let suspended = pc.saturating_sub(1);
    vm.lane_completion.raise(JsError::thrown(thrown, "async generator throw".into()));
    vm.lane_raise_site = super::super::compiled::raise_site(function, suspended as u32);
    // SAFETY: the resumed function's view is the running one, whose records start at `base`.
    Some(Resumed::Exit(raised_exit_in_lane(cx, unsafe { (*cx).base.add(suspended) })))
}

/// The request of `generator`, whose `next` or `throw` the call at `ip` with registers `r`
/// makes: its promise made and rooted in the activation's side record (`SidePayload::Settles`),
/// the generator resumes as an
/// activation receiving `sent` (`resume_generator_in_lane`). The registers and record the lane
/// continues at, or `None`, before any effect but a promise left to the collector.
fn resume_request_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction, generator: Value, sent: Value) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let promise = vm.promise_object().ok()?;
    note_allocation(vm);
    let root = vm.heap.root(promise);
    // The promise rides in the request activation's side record (one-frame-one-call.md F4 (3)),
    // attached as the activation is pushed.
    let resumed = generator_windows::resume_generator_in_lane(cx, r, ip, generator, r.get(i.b), sent, (Completion::AsyncRequest, SidePayload::Settles(Some(root))));
    if resumed.is_none() {
        vm.heap.release_root(root);
    }
    resumed
}

/// The promise of the in-place request `frame`, unrooted as its activation stops.
fn take_request_promise(vm: &mut Vm, frame: &ActivationView) -> Value {
    vm.settling_promise(frame.payload().settles())
}

/// `await` (or, where `yielded`, `yield`) of the in-place request's activation on `value`, the
/// converted value its row suspends on (`lane_frame::suspension_row_in_lane`): the frame suspends back
/// into the generator's record, resuming at `pc` with the value it is resumed with in
/// `destination`, the activation pops, and the caller goes on with the request's promise.
#[inline(never)]
pub(super) fn suspend_request_in_lane(cx: *mut LaneContext, (pc, destination): (usize, crate::bytecode::Register),
    yielded: bool, value: Value) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The converted value may be a fresh promise, which the safepoints after the run collect.
    note_allocation(vm);
    let depth = vm.lane_stack.activation_count() - 1;
    let callee = vm.lane_stack.top_frame().expect("a request's activation runs").callee();
    vm.suspend_request_window(callee, (pc, destination, depth), yielded, value);
    let activation = vm.lane_stack.pop_activation().expect("a request's activation runs");
    let (memory, _) = restore_caller_in_lane(cx, &activation);
    let promise = take_request_promise(vm, &activation);
    let script = activation.script().expect("a request runs as a script activation");
    let settled = vm.await_request(code, script.callee, promise, yielded, value);
    request_settled(vm, &activation, memory, promise, settled)
}

/// The return of the in-place request's `activation`, popped with its caller restored
/// (`memory`), with `value`: the generator completes and the caller goes on with the promise.
pub(super) fn request_returned_in_lane(cx: *mut LaneContext, activation: &ActivationView, memory: MemoryView, value: Value) -> Resumed {
    let vm = unsafe { vm(cx) };
    let script = activation.script().expect("a request runs as a script activation");
    vm.finish_request_window(script.callee);
    let promise = take_request_promise(vm, activation);
    // SAFETY: the context's program outlives the lane run.
    let settled = vm.complete_request(unsafe { &*(*cx).code }, script.callee, promise, value);
    request_settled(vm, activation, memory, promise, settled)
}

/// The caller of the request `activation` goes on after its call with `promise`, or raises at
/// the call the error its settlement threw.
fn request_settled(vm: &mut Vm, activation: &ActivationView, memory: MemoryView, promise: Value, settled: Result<(), JsError>) -> Resumed {
    match settled {
        Ok(()) => resume_after_call_in_lane(activation, memory, promise),
        Err(error) => {
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(activation.call()))
        }
    }
}
