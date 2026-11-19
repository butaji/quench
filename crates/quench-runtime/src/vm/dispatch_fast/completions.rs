//! Lane activations' completions: what a return yields to the call it completes, by the
//! completion its activation was made with (`Completion`).
use super::*;

/// The completion of the innermost activation with `value`, which the record at `ip`
/// returns, past the common return (`complete_plain_activation_in_lane`), by what it yields to
/// its call: a native's callback ends the run (`complete_native_callback_in_lane`), a plain
/// activation completes a run of returning calls (`return f(...)`,
/// `complete_plain_chain_in_lane`), another program's activation enters its caller's program
/// (`complete_cross_activation_in_lane`), and any other completion, or any of these past its
/// common form, completes as `complete_activation_in_lane` completes it. `None` when no
/// activation runs: the running function is a VM frame.
#[inline(always)]
pub(super) fn complete_returning_in_lane<H: Host>(cx: *mut LaneContext<H>, ip: Ip, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let completion = match vm.lane_stack.activations().last().map(|activation| activation.kind()) {
        Some(ActivationKind::Script(script)) => Some(script.completion),
        _ => None,
    };
    // An array step, a return per element, is tested first.
    if completion == Some(Completion::ArrayStep) {
        return step_array_iteration_in_place_in_lane(cx, value);
    }
    let common = match completion {
        Some(Completion::ReturnsToNative) => complete_native_callback_in_lane(cx, value),
        // `complete_activation_in_lane` begins with the same chain.
        Some(Completion::Plain) => None,
        Some(Completion::Cross) => complete_cross_activation_in_lane(cx, value),
        Some(Completion::SettlesPromise) => return_from_in_lane::<H, false, false, false, true>(cx, value).map(|(r, next)| Resumed::At(r, next, Carried::script(cx))),
        Some(Completion::Constructs) => return_from_in_lane::<H, false, false, true, false>(cx, value).map(|(r, next)| Resumed::At(r, next, Carried::script(cx))),
        Some(Completion::ReadsTrapped) => complete_trapped_read_in_lane(cx, ip, value),
        _ => None,
    };
    if common.is_some() {
        return common;
    }
    complete_activation_in_lane(cx, ip, value)
}

/// `complete_returning_in_lane` of a proxy `get` trap's activation (`Completion::ReadsTrapped`)
/// whose read keeps its result: the read's register takes the trap's result, in the
/// caller's program, which a trap of another program's library leaves for. A result the
/// target's invariants may refuse returns from the trap's frame, whose pending call checks
/// it; a read that returns its value (`RETURN_REGISTER`) takes `complete_activation_in_lane`.
#[inline(always)]
fn complete_trapped_read_in_lane<H: Host>(cx: *mut LaneContext<H>, ip: Ip, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let top = vm.lane_stack.activations().last();
    if proxy_traps::trap_result_needs_general_path(vm, top, value) {
        return Some(Resumed::Exit(frame_return_in_lane(vm, ip, value)));
    }
    // SAFETY: the call record belongs to the caller's live view.
    if unsafe { (*top?.call()).a } & crate::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let activation = vm.lane_stack.pop_activation()?;
    let (caller_memory, _) = restore_caller_in_lane(cx, &activation);
    if activation.caller_program() != unsafe { (*cx).program } {
        enter_program_in_lane(cx, activation.caller_program());
    }
    Some(resume_after_call_in_lane(&activation, caller_memory, value))
}

/// `complete_activation_in_lane`'s commonest completion: a JavaScript activation of the
/// running program whose call keeps its result (`Completion::Plain`, a result register). It
/// restores what that completion restores, without moving the activation out of the stack:
/// the caller's registers, records, receiver and environments, and the call's result. `None`
/// leaves every other completion to the returns past it, which take another program's
/// activation the same way (`complete_cross_activation_in_lane`).
#[inline(always)]
pub(super) fn return_plain_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    return_from_in_lane::<H, false, false, false, false>(cx, value)
}

/// `return_plain_in_lane` into a running lane body's call (`body_call_helper`): a returning
/// call's register takes the result too, since the body completes its own activation with
/// it next (`LaneExit::Returned`).
#[inline(always)]
pub(super) fn return_to_body_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    return_from_in_lane::<H, false, true, false, false>(cx, value)
}

/// `complete_plain_activation_in_lane` of an activation of another program than its caller's
/// (`Completion::Cross`), which enters the caller's program first, for the returns past the
/// common one (`lane_return_general`, `return_value_general_in_lane`), so the common return
/// carries none of the program switch.
#[inline(always)]
pub(super) fn complete_cross_activation_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let (registers, next) = return_from_in_lane::<H, true, false, false, false>(cx, value)?;
    Some(Resumed::At(registers, next, Carried::script(cx)))
}

/// Whether a call record that returns its callee's completion runs in the lane: from an
/// activation, which completes in turn, or from the frame, which the general loop then
/// completes with the value (`frame_return_in_lane`). A host permitting proper tail calls sends
/// a strict caller's terminal calls to the general dispatch path instead.
#[inline(always)]
pub(super) fn returning_call_admitted_in_lane<H: Host>(cx: *mut LaneContext<H>) -> bool {
    let vm = unsafe { vm(cx) };
    if !vm.host.proper_tail_calls() {
        return true;
    }
    let function = running_function_in_lane(cx);
    !unsafe { &*(*cx).code }.function(function as usize).strict
}

/// The frame's own call record at `call` returns `value`, its callee's completion: the
/// general loop returns it from the frame at that instruction (`Vm::returned_in_lane`).
#[inline(always)]
pub(super) fn frame_return_in_lane<H: Host>(vm: &mut Vm<H>, call: Ip, value: Value) -> Ip {
    vm.lane_completion.set(LaneCompletion::Returned(value));
    exit(call)
}

/// Complete the innermost lane activation with `value` when it is the common return
/// (`return_plain_in_lane`): a JavaScript activation of the running program's caller yielding
/// `value` to its call's result register. Each step loads or stores, so a handler returning
/// through it saves no registers. `None`, which changes nothing, leaves every other completion
/// to `complete_activation_in_lane`. A return record of a leaf's records (`LEAF`,
/// `LaneView::specialize_leaf_returns`) completes the running leaf call first
/// (`complete_leaf_in_lane`); no other record runs while one does.
#[inline(always)]
pub(super) fn complete_plain_activation_in_lane<H: Host, const LEAF: u8>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let completed = match LEAF {
        NOT_LEAF => None,
        form => complete_leaf_in_lane::<H>(cx, value, form),
    };
    let (registers, next) = match completed {
        Some(completed) => completed,
        None => return_plain_in_lane(cx, value)?,
    };
    let view = Carried {
        memory: MemoryView::script(cx),
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    Some(Resumed::At(registers, next, view))
}

/// `complete_activation_in_lane` of a run of JavaScript activations, of one program or several,
/// each returning the completion of the call above it (`return f(...)`), down to one whose call
/// takes `value` in its result register. Only that outermost activation's caller state is
/// restored: an inner one's would be overwritten by it. `None`, which changes nothing, leaves
/// any other completion to the activation-by-activation loop.
#[inline(always)]
pub(super) fn complete_plain_chain_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let (outermost, crossed) = returning_chain_outermost(vm.lane_stack.activations())?;
    // Script activations hold no Wasm storage (`LaneStack::pop_activation`), so the inner
    // ones go without its accounting.
    debug_assert!(vm.lane_stack.activations()[outermost..].iter().all(|activation| activation.frame_storage_bytes() == 0));
    vm.lane_stack.activations_mut().truncate(outermost + 1);
    vm.lane_stack.drop_owned_from(outermost + 1);
    let top = &vm.lane_stack.activations()[outermost];
    let completion = match top.kind() {
        ActivationKind::Script(script) => script.completion,
        ActivationKind::Native | ActivationKind::Wasm { .. } | ActivationKind::WasmEntry { .. } => unreachable!("a chain holds script activations"),
    };
    if completion == Completion::ReturnsToBody {
        return end_body_callee_in_lane(cx, value);
    }
    // The outermost activation of another program than its caller's enters that program;
    // one of its own, past a crossing one above it, returns to the program it began in.
    if completion == Completion::Cross {
        return complete_cross_activation_in_lane(cx, value);
    }
    if crossed {
        enter_program_in_lane(cx, top.caller_program());
        unsafe { (*cx).memory = MemoryView::script(cx) };
    }
    complete_plain_activation_in_lane::<H, NOT_LEAF>(cx, value)
}

/// The index of the outermost activation of the run the innermost one completes, each
/// returning the completion of the call above it, and whether the run crosses programs; `None`
/// where a completion past a plain or crossing return, or the frame, ends it.
#[inline(always)]
fn returning_chain_outermost(activations: &[Activation]) -> Option<(usize, bool)> {
    let mut index = activations.len();
    let mut crossed = false;
    loop {
        index = index.checked_sub(1)?;
        let activation = &activations[index];
        let ActivationKind::Script(script) = activation.kind() else {
            return None;
        };
        // A body's callee (`Completion::ReturnsToBody`) ends the run its call started: it is
        // the outermost, whatever its call's register, and runs in its caller's program.
        if script.completion == Completion::ReturnsToBody {
            return (!crossed).then_some((index, false));
        }
        crossed |= script.completion == Completion::Cross;
        if !matches!(script.completion, Completion::Plain | Completion::Cross) {
            return None;
        }
        // SAFETY: the call record belongs to the caller's live view.
        if unsafe { (*activation.call()).a } & crate::bytecode::RETURN_REGISTER == 0 {
            return Some((index, crossed));
        }
    }
}

/// `complete_plain_chain_in_lane` out of line, for the returns past the common one
/// (`lane_return_general`, `return_value_general_in_lane`), whose frames stay small.
#[inline(never)]
pub(super) fn complete_returning_chain_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    complete_plain_chain_in_lane(cx, value)
}

/// The end of the nested run of a body's callee, the innermost activation (`Completion::ReturnsToBody`),
/// with `value`: its caller's state restored and the call's register set, as the general loop's
/// completion of it does (`body_calls::ends_run`).
#[inline(always)]
pub(super) fn end_body_callee_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let activation = vm.lane_stack.pop_activation()?;
    let (_, this) = restore_caller_in_lane(cx, &activation);
    body_calls::ends_run(cx, &activation, Completion::ReturnsToBody, this, value)
}

/// `complete_activation_in_lane` of a native's callback (`Completion::ReturnsToNative`): the
/// native's caller state restored, the run ends with `value`, which the native receives.
#[inline(always)]
pub(super) fn complete_native_callback_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.activation_count().checked_sub(1)?;
    let top: *const Activation = &vm.lane_stack.activations()[depth];
    // SAFETY: `top` is the innermost activation, which nothing else accesses.
    if !matches!(unsafe { (*top).kind() }, ActivationKind::Script(script) if script.completion == Completion::ReturnsToNative) {
        return None;
    }
    // SAFETY: a script activation holds no drop glue and no Wasm storage
    // (`Activation::frame_storage_bytes`); its slot stays as it is, unread by the stack, until
    // the next push, so it is read in place, as `return_from_in_lane` reads a plain one.
    unsafe { vm.lane_stack.activations_mut().set_len(depth) };
    vm.lane_stack.drop_owned_from(depth);
    let activation = unsafe { &*top };
    restore_caller_in_lane(cx, activation);
    vm.lane_completion.set(LaneCompletion::NativeReturned(value));
    Some(Resumed::Exit(exit(activation.call())))
}

/// Restore the lane state `activation`'s caller runs with, as its return does: the lane
/// stack below its window, the caller's registers, records and memory, and a JavaScript
/// caller's context words stay below its registers. Returns the caller's memory and the
/// returning activation's receiver, which the context held while it ran.
#[inline(always)]
pub(super) fn restore_caller_in_lane<H: Host>(cx: *mut LaneContext<H>, activation: &Activation) -> (MemoryView, Value) {
    let vm = unsafe { vm(cx) };
    // SAFETY: the values hold no drop glue.
    unsafe { vm.lane_stack.values.release_window(activation.window_word()) };
    let caller_memory = activation.caller_memory(cx);
    // The returning activation's receiver, its window's context word.
    let this = Registers(activation.window_ptr()).this();
    unsafe {
        (*cx).registers = activation.caller_registers();
        (*cx).base = activation.caller_base();
        (*cx).memory = caller_memory;
    }
    (caller_memory, this)
}

/// The caller of `activation`, restored with `memory` (`restore_caller_in_lane`), resumes
/// after its call with `value` in the call's result register.
#[inline(always)]
pub(super) fn resume_after_call_in_lane(activation: &Activation, memory: MemoryView, value: Value) -> Resumed {
    // SAFETY: the call record belongs to the caller's live view.
    activation.caller_registers().set(unsafe { (*activation.call()).a }, value);
    resume_at_in_lane(activation, memory, following(activation.call()))
}

/// The caller of `activation`, restored with `memory`, resumes at the record `at` of its view.
#[inline(always)]
pub(super) fn resume_at_in_lane(activation: &Activation, memory: MemoryView, at: Ip) -> Resumed {
    let view = Carried {
        memory,
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    Resumed::At(activation.caller_registers(), at, view)
}

/// Whether the caller a returning call completes in turn with `value` returns from its frame
/// in the general loop instead: the frame itself, an activation whose promise's settlement
/// the general path makes (`settled_then_in_lane` reads the `then` it resolves with into
/// `then`), or a trap whose result the target's invariants refuse (`proxy_traps::trap_result_needs_general_path`).
#[inline(always)]
pub(super) fn caller_returns_in_frame_in_lane<H: Host>(vm: &mut Vm<H>, value: Value, then: &mut Value) -> bool {
    match vm.lane_stack.activations().last() {
        None => true,
        Some(caller) if settling(caller) => match settled_then_in_lane(vm, value) {
            Some(read) => {
                *then = read;
                false
            }
            None => true,
        },
        caller => proxy_traps::trap_result_needs_general_path(vm, caller, value),
    }
}

/// Whether `activation` settles its call's promise (`Completion::SettlesPromise`).
#[inline(always)]
pub(super) fn settling(activation: &Activation) -> bool {
    matches!(activation.kind(), ActivationKind::Script(script) if script.completion == Completion::SettlesPromise)
}

/// The `then` a settling activation completing with `value` resolves its promise with
/// (`Vm::plain_resolution_then`), read once; `None` where it settles the promise in its frame
/// instead: its resolution would read the value's `then` impurely or run a lifecycle hook,
/// either of which may run guest code. The frame's pending call resolves the promise as
/// `Vm::promise_resolve_value` does.
#[inline(always)]
pub(super) fn settled_then_in_lane<H: Host>(vm: &mut Vm<H>, value: Value) -> Option<Value> {
    if vm.host.observes_promise_lifecycle() {
        return None;
    }
    vm.plain_resolution_then(value)
}

/// The completion of a settling activation (`Completion::SettlesPromise`) whose caller the
/// lane restored, with `value` and the `then` `settled_then_in_lane` read from it: its promise,
/// the innermost of `LaneStack::settling`, resolved with the value, which its call yields.
#[inline(never)]
pub(super) fn settle_promise_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value, then: Value) -> Value {
    let vm = unsafe { vm(cx) };
    let root = match vm.lane_stack.settling.pop().expect("each settling activation has its entry") {
        Some(root) => root,
        // Only the `await` after the call reads its promise: a primitive fulfills it, and
        // awaiting a primitive takes the one job awaiting that fulfilled promise takes.
        None if !vm.is_object_like(value) => return value,
        None => {
            let root = vm.settling_root(None);
            note_allocation(vm);
            root
        }
    };
    let promise = vm.release_settled_call_root(root);
    // SAFETY: the context's program, the caller's, outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let settled = vm.resolve_fresh_promise_plainly(code, promise, value, then);
    debug_assert!(settled, "a settling call's promise is a promise record");
    promise
}
