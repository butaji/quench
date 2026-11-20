//! Lane activations' completions: what a return yields to the call it completes, by the
//! completion its activation was made with (`Completion`).
use super::*;
use crate::staged::call_complete::{
    concrete_admits_returning_call, concrete_plain_return_admits, concrete_returning_this, concrete_returns_callee_completion,
    concrete_settles_in_lane, concrete_settles_without_promise, concrete_side_completion, concrete_write_call_result, frame_packed,
};
use crate::staged::Concrete;

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
    // The innermost's completion read where it is: a header's side record names it, or the
    // header alone (`Plain`, or a trampoline's), or a record (no spill and no record built,
    // one-frame-one-call.md F4).
    let completion = vm.lane_stack.top_frame().and_then(|frame| frame.completion());
    // An array step, a return per element, is tested first.
    if completion == Some(Completion::ArrayStep) {
        return step_array_iteration_in_place_in_lane(cx, value);
    }
    let common = match completion {
        // A callback's record: one still a plain header returned through its trampoline.
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
    // The trap's frame read where it is (no spill, one-frame-one-call.md F4).
    let top = vm.lane_stack.top_frame();
    if proxy_traps::trap_result_needs_general_path(vm, top.map_or(SidePayload::None, |frame| frame.payload()), value) {
        return Some(Resumed::Exit(frame_return(vm, ip, value)));
    }
    // SAFETY: the call record belongs to the caller's live view.
    if concrete_returns_callee_completion(unsafe { (*top?.call()).a }) {
        return None;
    }
    let frame = vm.lane_stack.pop_activation()?;
    let (caller_memory, _) = restore_caller_in_lane(cx, &frame);
    if frame.caller_program() != unsafe { (*cx).program } {
        enter_program_in_lane(cx, frame.caller_program());
    }
    Some(resume_after_call_in_lane(&frame, caller_memory, value))
}

/// `complete_activation_in_lane`'s commonest completion: a JavaScript activation of the
/// running program whose call keeps its result (`Completion::Plain`, a result register). It
/// restores what that completion restores, without moving the activation out of the stack:
/// the caller's registers, records, receiver and environments, and the call's result. `None`
/// leaves every other completion to the returns past it, which take another program's
/// activation the same way (`complete_cross_activation_in_lane`).
#[inline(always)]
pub(super) fn return_plain_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    // The header's plain return first (`lane_return_header`), then the side record's.
    let window = Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get());
    lane_return_header::<H, false>(cx, window, value).or_else(|| return_from_in_lane::<H, false, false, false, false>(cx, value))
}

/// The plain return of the innermost activation where it is a plain one running on `window`
/// whose header completes it (`staged::call_complete::plain_return_admits`: no side flag, and
/// a call that keeps its result, or any call where `INTO_BODY`): the header's pop row
/// (`staged::protocol::pop_plain`, `LaneStack::pop_plain`) names the caller, whose result
/// register takes `value` (`staged::call_complete::write_call_result`). `None`, which changes
/// nothing, for any other frame: a side record's completion (`return_from_in_lane`), a returning
/// call's chain (`return_chain_in_lane`), or a record. It reads no record and makes no call.
#[inline(always)]
pub(super) fn lane_return_header<H: Host, const INTO_BODY: bool>(cx: *mut LaneContext<H>, window: Registers, value: Value) -> Option<(Registers, Ip)> {
    let stack: &mut LaneStack = &mut unsafe { vm(cx) }.lane_stack;
    if stack.plain.depth.get() == 0 {
        return None;
    }
    debug_assert!(stack.plain.registers.get() == window.0 && unsafe { (*cx).registers }.0 == window.0, "the innermost plain activation runs");
    // SAFETY: the call record belongs to the caller's live view (a trampoline's header names
    // the trampoline's record, which keeps its result).
    let destination = unsafe { (*window.plain_caller().0).a };
    if !concrete_plain_return_admits(window.0.expose_provenance() as u64, destination, INTO_BODY) {
        return None;
    }
    // SAFETY: a plain activation runs on `window`, checked above.
    let (call, caller_registers, caller_base) = unsafe { stack.pop_plain(window.0) };
    unsafe { (*cx).registers = caller_registers; (*cx).base = caller_base; }
    concrete_write_call_result(caller_registers.0.expose_provenance() as u64, destination & crate::bytecode::REGISTER_MASK, value);
    Some((caller_registers, following(call)))
}

/// `return_plain_in_lane` into a running lane body's call (`body_call_helper`): a returning
/// call's register takes the result too, since the body completes its own activation with
/// it next (`LaneExit::Returned`).
#[inline(always)]
pub(super) fn return_to_body_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    let window = Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get());
    lane_return_header::<H, true>(cx, window, value).or_else(|| return_from_in_lane::<H, false, true, false, false>(cx, value))
}

/// `complete_plain_activation_in_lane` of an activation of another program than its caller's
/// (`Completion::Cross`), which enters the caller's program first, for the returns past the
/// common one (`return_general_in_lane`, `return_value_general_in_lane`), so the common return
/// carries none of the program switch.
#[inline(always)]
pub(super) fn complete_cross_activation_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    // A plain frame crosses by its header's flag (`staged::call_complete::side_completion`):
    // one without it declines before its side record is read.
    let stack = &unsafe { vm(cx) }.lane_stack;
    if stack.plain.depth.get() != 0 {
        let packed = frame_packed(&mut Concrete::new(std::ptr::null()), stack.plain.registers.get().expose_provenance() as u64);
        if !concrete_side_completion(packed).crosses {
            return None;
        }
    }
    let (registers, next) = return_from_in_lane::<H, true, false, false, false>(cx, value)?;
    Some(Resumed::At(registers, next, Carried::script(cx)))
}

/// Whether a call record that returns its callee's completion runs in the lane: from an
/// activation, which completes in turn, or from the frame, which the general loop then
/// completes with the value (`frame_return`). A host permitting proper tail calls sends
/// a strict caller's terminal calls to the general dispatch path instead.
#[inline(always)]
pub(super) fn returning_call_admitted<H: Host>(cx: *mut LaneContext<H>) -> bool {
    let vm = unsafe { vm(cx) };
    let proper = vm.host.proper_tail_calls();
    // The running function's strictness is read only where proper tail calls are on.
    let strict = proper && {
        let function = running_function_in_lane(cx);
        unsafe { &*(*cx).code }.function(function as usize).strict
    };
    concrete_admits_returning_call(proper, strict)
}

/// The frame's own call record at `call` returns `value`, its callee's completion: the
/// general loop returns it from the frame at that instruction (`Vm::returned_in_lane`).
#[inline(always)]
pub(super) fn frame_return<H: Host>(vm: &mut Vm<H>, call: Ip, value: Value) -> Ip {
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
    // The run walked where its frames are, headers with their side records, then records: no
    // spill and no record built (one-frame-one-call.md F4).
    let (above, top, crossed) = returning_chain_outermost(vm.lane_stack.frames_innermost())?;
    // The inner ones go where they are: script activations hold no Wasm storage
    // (`LaneStack::pop_activation`), and their windows lie above the outermost's, which its
    // return releases. What they owned goes with them.
    let outermost = vm.lane_stack.activation_count() - above;
    vm.lane_stack.drop_owned_from(outermost);
    for _ in 0..above {
        // SAFETY: an activation runs, a script one of the chain.
        unsafe { vm.lane_stack.discard_top_frame() };
    }
    if above != 0 {
        // The outermost runs now: its window is the running one its return pops.
        unsafe { (*cx).registers = Registers(top.window_ptr()) };
    }
    let completion = top.completion().expect("a chain holds script activations");
    if completion == Completion::ReturnsToBody {
        return end_body_callee(cx, value);
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

/// The outermost activation of the run the innermost one completes, each returning the
/// completion of the call above it, from `frames`, innermost first: how many lie above it, its
/// return words, and whether the run crosses programs; `None` where a completion past a plain
/// or crossing return, or the frame, ends it. Each test reads the frame's completion and its
/// call record's return bit (`staged::call_complete::returns_callee_completion`).
#[inline(always)]
fn returning_chain_outermost(mut frames: impl Iterator<Item = ActivationView>) -> Option<(usize, ActivationView, bool)> {
    let mut above = 0;
    let mut crossed = false;
    loop {
        let frame = frames.next()?;
        let completion = frame.completion()?;
        // A body's callee (`Completion::ReturnsToBody`) ends the run its call started: it is
        // the outermost, whatever its call's register, and runs in its caller's program.
        if completion == Completion::ReturnsToBody {
            return (!crossed).then_some((above, frame, false));
        }
        crossed |= completion == Completion::Cross;
        if completion.needs_completion_flag() {
            return None;
        }
        // SAFETY: the call record belongs to the caller's live view.
        if !concrete_returns_callee_completion(unsafe { (*frame.call()).a }) {
            return Some((above, frame, crossed));
        }
        above += 1;
    }
}

/// `complete_plain_chain_in_lane` out of line, for the returns past the common one
/// (`return_general_in_lane`, `return_value_general_in_lane`), whose frames stay small.
#[inline(never)]
pub(super) fn complete_returning_chain_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    complete_plain_chain_in_lane(cx, value)
}

/// The end of the nested run of a body's callee, the innermost activation (`Completion::ReturnsToBody`),
/// with `value`: its caller's state restored and the call's register set, as the general loop's
/// completion of it does (`body_calls::ends_run`).
#[inline(always)]
pub(super) fn end_body_callee<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let frame = vm.lane_stack.pop_activation()?;
    let (_, this) = restore_caller_in_lane(cx, &frame);
    body_calls::ends_run(cx, &frame, Completion::ReturnsToBody, this, value)
}

/// `complete_activation_in_lane` of a native's callback (`Completion::ReturnsToNative`): the
/// native's caller state restored, the run ends with `value`, which the native receives. Only a
/// callback's record reaches it: one that keeps its values, a gathering one, or a plain header a
/// spill made a record (`LaneStack::spill_plain`). A callback still a plain header returns
/// through its trampoline instead (`callback_trampoline_returned`), which ends the run the same way.
#[inline(always)]
pub(super) fn complete_native_callback_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // The callback read where it is, then popped where it is (`LaneStack::pop_activation`): no
    // spill, and a header's pop builds no record.
    if vm.lane_stack.top_frame()?.completion() != Some(Completion::ReturnsToNative) {
        return None;
    }
    let frame = vm.lane_stack.pop_activation()?;
    restore_caller_in_lane(cx, &frame);
    vm.lane_completion.set(LaneCompletion::NativeReturned(value));
    Some(Resumed::Exit(exit(frame.call())))
}

/// Restore the lane state the caller of the returning frame (`frame`, its return words) runs
/// with, as its return does: the lane stack below its window, the caller's registers, records
/// and memory, and a JavaScript caller's context words stay below its registers. Returns the
/// caller's memory and the returning frame's receiver (`staged::call_complete::returning_this`),
/// which the context held while it ran.
#[inline(always)]
pub(super) fn restore_caller_in_lane<H: Host>(cx: *mut LaneContext<H>, frame: &ActivationView) -> (MemoryView, Value) {
    let vm = unsafe { vm(cx) };
    // SAFETY: the values hold no drop glue.
    unsafe { vm.lane_stack.values.release_window(frame.window_word()) };
    let caller_memory = frame.caller_memory(cx);
    // The returning frame's receiver, its window's context word.
    let this = concrete_returning_this(frame.window_ptr().expose_provenance() as u64);
    unsafe {
        (*cx).registers = frame.caller_registers();
        (*cx).base = frame.caller_base();
        (*cx).memory = caller_memory;
    }
    (caller_memory, this)
}

/// The caller of the returned `frame`, restored with `memory` (`restore_caller_in_lane`), resumes
/// after its call with `value` in the call's result register.
#[inline(always)]
pub(super) fn resume_after_call_in_lane(frame: &ActivationView, memory: MemoryView, value: Value) -> Resumed {
    // SAFETY: the call record belongs to the caller's live view.
    let result = unsafe { (*frame.call()).a };
    resume_at_in_lane(frame, memory, following(frame.call()), Some((result, value)))
}

/// The caller of the returned `frame`, restored with `memory`, resumes at the record `at` of
/// its view, its register `written.0` taking `written.1` first where one is named: the call's
/// result register (`staged::call_complete::write_call_result`), or a delegate's state register.
#[inline(always)]
pub(super) fn resume_at_in_lane(frame: &ActivationView, memory: MemoryView, at: Ip, written: Option<(crate::bytecode::Register, Value)>) -> Resumed {
    if let Some((register, value)) = written {
        concrete_write_call_result(frame.caller_registers().0.expose_provenance() as u64, register, value);
    }
    let view = Carried {
        memory,
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    Resumed::At(frame.caller_registers(), at, view)
}

/// Whether the caller a returning call completes in turn with `value` returns from its frame
/// in the general loop instead: the frame itself, an activation whose promise's settlement
/// the general path makes (`settled_then_in_lane` reads the `then` it resolves with into
/// `then`), or a trap whose result the target's invariants refuse (`proxy_traps::trap_result_needs_general_path`).
#[inline(always)]
pub(super) fn caller_returns_in_frame<H: Host>(vm: &mut Vm<H>, value: Value, then: &mut Value) -> bool {
    // The caller read where it is (no spill, one-frame-one-call.md F4).
    match vm.lane_stack.top_frame() {
        None => true,
        Some(caller) if caller.completion() == Some(Completion::SettlesPromise) => match settled_then_in_lane(vm, value) {
            Some(read) => {
                *then = read;
                false
            }
            None => true,
        },
        Some(caller) => proxy_traps::trap_result_needs_general_path(vm, caller.payload(), value),
    }
}

/// The `then` a settling activation completing with `value` resolves its promise with
/// (`Vm::plain_resolution_then`), read once; `None` where it settles the promise in its frame
/// instead: its resolution would read the value's `then` impurely or run a lifecycle hook,
/// either of which may run guest code. The frame's pending call resolves the promise as
/// `Vm::promise_resolve_value` does.
#[inline(always)]
pub(super) fn settled_then_in_lane<H: Host>(vm: &mut Vm<H>, value: Value) -> Option<Value> {
    if !concrete_settles_in_lane(vm.host.observes_promise_lifecycle()) {
        return None;
    }
    vm.plain_resolution_then(value)
}

/// The completion of a settling activation (`Completion::SettlesPromise`) whose caller the
/// lane restored, with `value` and the `then` `settled_then_in_lane` read from it: its promise,
/// `rooted`, which its side record carried (`SidePayload::Settles`), resolved with the value,
/// which its call yields.
#[inline(never)]
pub(super) fn settle_promise_in_lane<H: Host>(cx: *mut LaneContext<H>, value: Value, then: Value, rooted: Option<crate::heap::RootId>) -> Value {
    let vm = unsafe { vm(cx) };
    // Only the `await` after the call reads an unrooted promise: a primitive fulfills it, and
    // awaiting a primitive takes the one job awaiting that fulfilled promise takes.
    if concrete_settles_without_promise(rooted.is_some(), vm.is_object_like(value)) {
        return value;
    }
    if rooted.is_none() {
        note_allocation(vm);
    }
    // An unrooted promise is made now (`Vm::settling_promise`); a rooted one is released.
    let promise = vm.settling_promise(rooted);
    // SAFETY: the context's program, the caller's, outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let settled = vm.resolve_promise_with_then(code, promise, value, then).is_ok();
    debug_assert!(settled, "a settling call's promise is a promise record");
    promise
}
