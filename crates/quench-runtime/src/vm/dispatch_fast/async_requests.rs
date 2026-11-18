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
pub(super) fn running_request<H: Host>(vm: &Vm<H>) -> bool {
    vm.lane_stack.activations().last().and_then(Activation::script).is_some_and(|script| script.completion == Completion::AsyncRequest)
}

/// `next` of an async generator, called at `ip` with registers `r`: the generator resumes in
/// place with the value the call passes (`lane_resume_request`). `None`, before any effect,
/// leaves the call to the native.
#[inline(never)]
pub(super) fn lane_async_generator_next<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let generator = r.get(i.c);
    if i.a & crate::bytecode::RETURN_REGISTER != 0 || !vm.requests_in_place(generator) {
        return None;
    }
    // SAFETY: the call's window is registers of the running function.
    let sent = unsafe { lane_call_window(r, i) }.first().copied().unwrap_or(Value::UNDEFINED);
    let (registers, start) = lane_resume_request(cx, r, ip, i, generator, sent)?;
    Some(Resumed::At(registers, start, Carried::script(cx)))
}

/// `throw` of an async generator suspended at a `yield` of its own (`Vm::throws_in_place`),
/// called at `ip` with registers `r`: the generator resumes in place and the error the call
/// passes raises at its `yield`, where a handler of its function catches it or its frame fails
/// the request (`settle_abandoned_call`). `None`, before any effect, leaves the call to the
/// native.
#[inline(never)]
pub(super) fn lane_async_generator_throw<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let generator = r.get(i.c);
    if i.a & crate::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let (function, pc) = vm.throws_in_place(generator)?;
    // SAFETY: the call's window is registers of the running function.
    let thrown = unsafe { lane_call_window(r, i) }.first().copied().unwrap_or(Value::UNDEFINED);
    lane_resume_request(cx, r, ip, i, generator, Value::UNDEFINED)?;
    // The error raises at the `yield` the frame suspended at, the record before its resume point.
    let suspended = pc.saturating_sub(1);
    vm.lane_completion.raise(JsError::thrown(thrown, "async generator throw".into()));
    vm.lane_raise_site = super::super::compiled::raise_site(function, suspended as u32);
    // SAFETY: the resumed function's view is the running one, whose records start at `base`.
    Some(Resumed::Exit(lane_raised_exit(cx, unsafe { (*cx).base.add(suspended) })))
}

/// The request of `generator`, whose `next` or `throw` the call at `ip` with registers `r`
/// makes: its promise made and rooted in `LaneStack::settling`, the generator resumes as an
/// activation receiving `sent` (`resume_generator_in_lane`). The registers and record the lane
/// continues at, or `None`, before any effect but a promise left to the collector.
fn lane_resume_request<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction, generator: Value, sent: Value) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let promise = vm.promise_object().ok()?;
    note_allocation(vm);
    let root = vm.heap.root(promise);
    vm.lane_stack.settling.push(Some(root));
    let resumed = generator_windows::resume_generator_in_lane(cx, r, ip, generator, r.get(i.b), sent, Completion::AsyncRequest);
    if resumed.is_none() {
        vm.lane_stack.settling.pop();
        vm.heap.release_root(root);
    }
    resumed
}

/// The promise of the innermost in-place request, unrooted as its activation stops.
fn take_request_promise<H: Host>(vm: &mut Vm<H>) -> Value {
    let root = vm.lane_stack.settling.pop().expect("each request activation has its entry");
    let root = vm.settling_root(root);
    vm.release_settled_call_root(root)
}

/// `await` (or, where `yielded`, `yield`) at `ip` of the in-place request's activation, on
/// registers `r`: the awaited value, converted as an `await`'s source is (`await_source_in_lane`);
/// the frame suspends back into the generator's record, resuming after this record, the
/// activation pops, and the caller goes on with the request's promise. A conversion that could
/// run guest code takes the general path.
#[inline(never)]
pub(super) fn lane_suspend_request<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction, yielded: bool) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let Some(value) = vm.await_source_in_lane(code, r.get(i.b)) else {
        return Resumed::Exit(exit(ip));
    };
    note_allocation(vm);
    let depth = vm.lane_stack.activation_count() - 1;
    let activation = *vm.lane_stack.activations().last().expect("a request's activation runs");
    let ActivationKind::Script(script) = activation.kind else { unreachable!("a request runs as a script activation") };
    // SAFETY: the record lies in the running view.
    let pc = unsafe { ip.offset_from((*cx).base) } as usize;
    vm.suspend_request_window(script.callee, (pc + 1, i.a & crate::bytecode::REGISTER_MASK, depth), yielded, value);
    vm.lane_stack.pop_activation();
    let (memory, _) = lane_restore_caller(cx, &activation);
    let promise = take_request_promise(vm);
    let settled = vm.await_request(code, script.callee, promise, yielded, value);
    request_settled(vm, &activation, memory, promise, settled)
}

/// The return of the in-place request's `activation`, popped with its caller restored
/// (`memory`), with `value`: the generator completes and the caller goes on with the promise.
pub(super) fn lane_request_returned<H: Host>(cx: *mut LaneContext<H>, activation: &Activation, memory: MemoryView, value: Value) -> Resumed {
    let vm = unsafe { vm(cx) };
    let ActivationKind::Script(script) = activation.kind else { unreachable!("a request runs as a script activation") };
    vm.finish_request_window(script.callee);
    let promise = take_request_promise(vm);
    // SAFETY: the context's program outlives the lane run.
    let settled = vm.complete_request(unsafe { &*(*cx).code }, script.callee, promise, value);
    request_settled(vm, activation, memory, promise, settled)
}

/// The caller of the request `activation` goes on after its call with `promise`, or raises at
/// the call the error its settlement threw.
fn request_settled<H: Host>(vm: &mut Vm<H>, activation: &Activation, memory: MemoryView, promise: Value, settled: Result<(), JsError>) -> Resumed {
    match settled {
        Ok(()) => lane_resume_after_call(activation, memory, promise),
        Err(error) => {
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(activation.call))
        }
    }
}
