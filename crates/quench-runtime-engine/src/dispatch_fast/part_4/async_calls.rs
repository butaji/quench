//! Async calls in a lane: starting one, resuming its caller, and parking the caller.
use super::*;

/// `frame_call_in_lane` of an async function the general loop would push as a settling call
/// (`Vm::in_loop_settling_target`): the callee starts as a job (`Vm::start_async_call`) beside
/// the caller's activations, as a native that runs beside them does (`call_native_out_in_lane`),
/// so its first `await` suspends its window in place and the caller continues in the lane with
/// the promise. `None` leaves the call to the general path: a call that completes its caller,
/// or a callee of another program or not yet derived.
#[inline(never)]
pub(in super::super) fn start_async_call_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction, callee: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if !unsafe { (*cx).script_calls } || i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let (program, function, env) = vm.in_loop_settling_target(code, callee)?;
    if program != unsafe { (*cx).program } || !vm.call_may_derive_in_lane(program, function) {
        return None;
    }
    let window = usize::from(ImmediateLayout::call_window_base(i.imm()));
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    // SAFETY: the call's window is registers of the running function; the values are copied
    // before any activation becomes a frame.
    let arguments = quench_runtime::vm::call_arguments::CallArguments::from_slice(unsafe { std::slice::from_raw_parts(r.0.add(window), argc) });
    let this = r.get(i.c);
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { record_pc(cx, ip) } as usize;
    let beside = park_async_caller_in_lane(vm, cx, call_pc);
    let context = CallContext::user_function(function, callee);
    let result = vm.start_async_call(code, function, env, this, arguments.as_slice(), context);
    note_allocation(vm);
    vm.lane_stack.refresh_activation_limit();
    Some(async_call_resumed_in_lane(vm, cx, ip, i, call_pc, beside, result))
}

/// Where the lane goes once `start_async_call_in_lane`'s job has run its first step: on with the
/// promise in the call's register, or out with the error, materializing the deferred caller
/// the job left unobserved so the error has a frame to unwind.
pub(in super::super) fn async_call_resumed_in_lane(vm: &mut Vm, cx: *mut LaneContext, ip: Ip, i: &LaneInstruction, call_pc: usize, beside: bool, result: Result<Value, JsError>) -> Resumed {
    let unobserved = beside && vm.lane_deferred.take().is_some();
    match result {
        Ok(promise) => {
            // The activations may have become frames, whose registers the context now names.
            let r = unsafe { (*cx).registers };
            r.set(i.a, promise);
            Resumed::At(r, following(ip), Carried::script(cx))
        }
        Err(error) => {
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
                vm.materialize_deferred_lane();
            }
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(ip))
        }
    }
}

/// Where `start_async_call_in_lane`'s caller resumes once the callee's job settles its first
/// step: beside other activations, a deferred lane call the job materializes only if it
/// observes the caller (true); otherwise the caller's own frame, resuming past the call.
pub(in super::super) fn park_async_caller_in_lane(vm: &mut Vm, cx: *mut LaneContext, call_pc: usize) -> bool {
    let beside = vm.lane_stack.has_frames();
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
    } else {
        // SAFETY: the context outlives the lane run.
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].pc = call_pc + 1;
        vm.frames[frame].pc_is_resume = true;
    }
    beside
}
