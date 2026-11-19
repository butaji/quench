//! Lane activations' completions: what a return yields to the call it completes, by the
//! completion its activation was made with (`Completion`).
use super::*;
use crate::staged::call_complete::{
    concrete_admits_returning_call, concrete_plain_return_admits, concrete_returning_this, concrete_returns_callee_completion,
    concrete_settles_in_lane, concrete_settles_without_promise, concrete_side_completion, concrete_write_call_result, frame_packed,
};
use crate::staged::Concrete;

/// The completion of the innermost activation with `value`, which the record at `ip`
/// returns, past the common return (`lane_complete_plain_activation`), by what it yields to
/// its call: a native's callback ends the run (`lane_complete_native_callback`), a plain
/// activation completes a run of returning calls (`return f(...)`,
/// `lane_complete_plain_chain`), another program's activation enters its caller's program
/// (`lane_complete_cross_activation`), and any other completion, or any of these past its
/// common form, completes as `lane_complete_activation` completes it. `None` when no
/// activation runs: the running function is a VM frame.
#[inline(always)]
pub(super) fn lane_complete_returning<H: Host>(cx: *mut LaneContext<H>, ip: Ip, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // The innermost's completion read where it is: a header's side record names it, or the
    // header alone (`Plain`, or a trampoline's), or a record (no spill and no record built,
    // one-frame-one-call.md F4).
    let completion = vm.lane_stack.top_return().and_then(|frame| frame.completion);
    // An array step, a return per element, is tested first.
    if completion == Some(Completion::ArrayStep) {
        return lane_step_array_iteration_in_place(cx, value);
    }
    let common = match completion {
        // A callback's record: one still a plain header returned through its trampoline.
        Some(Completion::ReturnsToNative) => lane_complete_native_callback(cx, value),
        // `lane_complete_activation` begins with the same chain.
        Some(Completion::Plain) => None,
        Some(Completion::Cross) => lane_complete_cross_activation(cx, value),
        Some(Completion::SettlesPromise) => lane_return_from::<H, false, false, false, true>(cx, value).map(|(r, next)| Resumed::At(r, next, Carried::script(cx))),
        Some(Completion::Constructs) => lane_return_from::<H, false, false, true, false>(cx, value).map(|(r, next)| Resumed::At(r, next, Carried::script(cx))),
        Some(Completion::ReadsTrapped) => lane_complete_trapped_read(cx, ip, value),
        _ => None,
    };
    if common.is_some() {
        return common;
    }
    lane_complete_activation(cx, ip, value)
}

/// `lane_complete_returning` of a proxy `get` trap's activation (`Completion::ReadsTrapped`)
/// whose read keeps its result: the read's register takes the trap's result, in the
/// caller's program, which a trap of another program's library leaves for. A result the
/// target's invariants may refuse returns from the trap's frame, whose pending call checks
/// it; a read that returns its value (`RETURN_REGISTER`) takes `lane_complete_activation`.
#[inline(always)]
fn lane_complete_trapped_read<H: Host>(cx: *mut LaneContext<H>, ip: Ip, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // The trap's frame read where it is (no spill, one-frame-one-call.md F4).
    let top = vm.lane_stack.top_return();
    if proxy_traps::trap_result_needs_general_path(vm, top.and_then(|frame| frame.completion), value) {
        return Some(Resumed::Exit(lane_frame_return(vm, ip, value)));
    }
    // SAFETY: the call record belongs to the caller's live view.
    if concrete_returns_callee_completion(unsafe { (*top?.call).a }) {
        return None;
    }
    let frame = vm.lane_stack.pop_activation()?;
    let (caller_memory, _) = lane_restore_caller(cx, &frame);
    if frame.caller_program != unsafe { (*cx).program } {
        lane_enter_program(cx, frame.caller_program);
    }
    Some(lane_resume_after_call(&frame, caller_memory, value))
}

/// `lane_complete_activation`'s commonest completion: a JavaScript activation of the
/// running program whose call keeps its result (`Completion::Plain`, a result register). It
/// restores what that completion restores, without moving the activation out of the stack:
/// the caller's registers, records, receiver and environments, and the call's result. `None`
/// leaves every other completion to the returns past it, which take another program's
/// activation the same way (`lane_complete_cross_activation`).
#[inline(always)]
pub(super) fn lane_return_plain<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    // The header's plain return first (`lane_return_header`), then the side record's.
    let window = Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get());
    lane_return_header::<H, false>(cx, window, value).or_else(|| lane_return_from::<H, false, false, false, false>(cx, value))
}

/// The plain return of the innermost activation where it is a plain one running on `window`
/// whose header completes it (`staged::call_complete::plain_return_admits`: no side flag, and
/// a call that keeps its result, or any call where `INTO_BODY`): the header's pop row
/// (`staged::protocol::pop_plain`, `LaneStack::pop_plain`) names the caller, whose result
/// register takes `value` (`staged::call_complete::write_call_result`). `None`, which changes
/// nothing, for any other frame: a side record's completion (`lane_return_from`), a returning
/// call's chain (`lane_return_chain`), or a record. It reads no record and makes no call.
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

/// `lane_return_plain` into a running lane body's call (`lane_body_call`): a returning
/// call's register takes the result too, since the body completes its own activation with
/// it next (`LaneExit::Returned`).
#[inline(always)]
pub(super) fn lane_return_to_body<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    let window = Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get());
    lane_return_header::<H, true>(cx, window, value).or_else(|| lane_return_from::<H, false, true, false, false>(cx, value))
}

/// `lane_complete_plain_activation` of an activation of another program than its caller's
/// (`Completion::Cross`), which enters the caller's program first, for the returns past the
/// common one (`lane_return_general`, `lane_return_value_general`), so the common return
/// carries none of the program switch.
#[inline(always)]
pub(super) fn lane_complete_cross_activation<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    // A plain frame crosses by its header's flag (`staged::call_complete::side_completion`):
    // one without it declines before its side record is read.
    let stack = &unsafe { vm(cx) }.lane_stack;
    if stack.plain.depth.get() != 0 {
        let packed = frame_packed(&mut Concrete::new(std::ptr::null()), stack.plain.registers.get().expose_provenance() as u64);
        if !concrete_side_completion(packed).crosses {
            return None;
        }
    }
    let (registers, next) = lane_return_from::<H, true, false, false, false>(cx, value)?;
    Some(Resumed::At(registers, next, Carried::script(cx)))
}

/// Whether a call record that returns its callee's completion runs in the lane: from an
/// activation, which completes in turn, or from the frame, which the general loop then
/// completes with the value (`lane_frame_return`). A host permitting proper tail calls sends
/// a strict caller's terminal calls to the general dispatch path instead.
#[inline(always)]
pub(super) fn lane_returning_call_admitted<H: Host>(cx: *mut LaneContext<H>) -> bool {
    let vm = unsafe { vm(cx) };
    let proper = vm.host.proper_tail_calls();
    // The running function's strictness is read only where proper tail calls are on.
    let strict = proper && {
        let function = lane_running_function(cx);
        unsafe { &*(*cx).code }.function(function as usize).strict
    };
    concrete_admits_returning_call(proper, strict)
}

/// The frame's own call record at `call` returns `value`, its callee's completion: the
/// general loop returns it from the frame at that instruction (`Vm::lane_returned`).
#[inline(always)]
pub(super) fn lane_frame_return<H: Host>(vm: &mut Vm<H>, call: Ip, value: Value) -> Ip {
    vm.lane_completion.set(LaneCompletion::Returned(value));
    exit(call)
}

/// Complete the innermost lane activation with `value` when it is the common return
/// (`lane_return_plain`): a JavaScript activation of the running program's caller yielding
/// `value` to its call's result register. Each step loads or stores, so a handler returning
/// through it saves no registers. `None`, which changes nothing, leaves every other completion
/// to `lane_complete_activation`. A return record of a leaf's records (`LEAF`,
/// `LaneView::specialize_leaf_returns`) completes the running leaf call first
/// (`lane_complete_leaf`); no other record runs while one does.
#[inline(always)]
pub(super) fn lane_complete_plain_activation<H: Host, const LEAF: u8>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let completed = match LEAF {
        NOT_LEAF => None,
        form => lane_complete_leaf::<H>(cx, value, form),
    };
    let (registers, next) = match completed {
        Some(completed) => completed,
        None => lane_return_plain(cx, value)?,
    };
    let view = Carried {
        memory: MemoryView::script(cx),
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    Some(Resumed::At(registers, next, view))
}

/// `lane_complete_activation` of a run of JavaScript activations, of one program or several,
/// each returning the completion of the call above it (`return f(...)`), down to one whose call
/// takes `value` in its result register. Only that outermost activation's caller state is
/// restored: an inner one's would be overwritten by it. `None`, which changes nothing, leaves
/// any other completion to the activation-by-activation loop.
#[inline(always)]
pub(super) fn lane_complete_plain_chain<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // The run walked where its frames are, headers with their side records, then records: no
    // spill and no record built (one-frame-one-call.md F4).
    let (above, top, crossed) = returning_chain_outermost(vm.lane_stack.frame_returns_innermost())?;
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
    let completion = top.completion.expect("a chain holds script activations");
    if completion == Completion::ReturnsToBody {
        return lane_end_body_callee(cx, value);
    }
    // The outermost activation of another program than its caller's enters that program;
    // one of its own, past a crossing one above it, returns to the program it began in.
    if completion == Completion::Cross {
        return lane_complete_cross_activation(cx, value);
    }
    if crossed {
        lane_enter_program(cx, top.caller_program);
        unsafe { (*cx).memory = MemoryView::script(cx) };
    }
    lane_complete_plain_activation::<H, NOT_LEAF>(cx, value)
}

/// The outermost activation of the run the innermost one completes, each returning the
/// completion of the call above it, from `frames`, innermost first: how many lie above it, its
/// return words, and whether the run crosses programs; `None` where a completion past a plain
/// or crossing return, or the frame, ends it. Each test reads the frame's completion and its
/// call record's return bit (`staged::call_complete::returns_callee_completion`).
#[inline(always)]
fn returning_chain_outermost(mut frames: impl Iterator<Item = FrameReturn>) -> Option<(usize, FrameReturn, bool)> {
    let mut above = 0;
    let mut crossed = false;
    loop {
        let frame = frames.next()?;
        let completion = frame.completion?;
        // A body's callee (`Completion::ReturnsToBody`) ends the run its call started: it is
        // the outermost, whatever its call's register, and runs in its caller's program.
        if completion == Completion::ReturnsToBody {
            return (!crossed).then_some((above, frame, false));
        }
        crossed |= completion == Completion::Cross;
        if !matches!(completion, Completion::Plain | Completion::Cross) {
            return None;
        }
        // SAFETY: the call record belongs to the caller's live view.
        if !concrete_returns_callee_completion(unsafe { (*frame.call).a }) {
            return Some((above, frame, crossed));
        }
        above += 1;
    }
}

/// `lane_complete_plain_chain` out of line, for the returns past the common one
/// (`lane_return_general`, `lane_return_value_general`), whose frames stay small.
#[inline(never)]
pub(super) fn lane_complete_returning_chain<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    lane_complete_plain_chain(cx, value)
}

/// The end of the nested run of a body's callee, the innermost activation (`Completion::ReturnsToBody`),
/// with `value`: its caller's state restored and the call's register set, as the general loop's
/// completion of it does (`body_calls::ends_run`).
#[inline(always)]
pub(super) fn lane_end_body_callee<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let frame = vm.lane_stack.pop_activation()?;
    let (_, this) = lane_restore_caller(cx, &frame);
    body_calls::ends_run(cx, &frame, Completion::ReturnsToBody, this, value)
}

/// `lane_complete_activation` of a native's callback (`Completion::ReturnsToNative`): the
/// native's caller state restored, the run ends with `value`, which the native receives. Only a
/// callback's record reaches it: one that keeps its values, a gathering one, or a plain header a
/// spill made a record (`LaneStack::spill_plain`). A callback still a plain header returns
/// through its trampoline instead (`callback_trampoline_returned`), which ends the run the same way.
#[inline(always)]
pub(super) fn lane_complete_native_callback<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // The callback read where it is, then popped where it is (`LaneStack::pop_activation`): no
    // spill, and a header's pop builds no record.
    if vm.lane_stack.top_return()?.completion != Some(Completion::ReturnsToNative) {
        return None;
    }
    let frame = vm.lane_stack.pop_activation()?;
    lane_restore_caller(cx, &frame);
    vm.lane_completion.set(LaneCompletion::NativeReturned(value));
    Some(Resumed::Exit(exit(frame.call)))
}

/// Restore the lane state the caller of the returning frame (`frame`, its return words) runs
/// with, as its return does: the lane stack below its window, the caller's registers, records
/// and memory, and a JavaScript caller's context words stay below its registers. Returns the
/// caller's memory and the returning frame's receiver (`staged::call_complete::returning_this`),
/// which the context held while it ran.
#[inline(always)]
pub(super) fn lane_restore_caller<H: Host>(cx: *mut LaneContext<H>, frame: &FrameReturn) -> (MemoryView, Value) {
    let vm = unsafe { vm(cx) };
    // SAFETY: the values hold no drop glue.
    unsafe { vm.lane_stack.values.release_window(frame.window) };
    let caller_memory = frame.caller_memory(cx);
    // The returning frame's receiver, its window's context word.
    let this = concrete_returning_this(frame.window_ptr().expose_provenance() as u64);
    unsafe {
        (*cx).registers = frame.caller_registers;
        (*cx).base = frame.caller_base;
        (*cx).memory = caller_memory;
    }
    (caller_memory, this)
}

/// The caller of the returned `frame`, restored with `memory` (`lane_restore_caller`), resumes
/// after its call with `value` in the call's result register.
#[inline(always)]
pub(super) fn lane_resume_after_call(frame: &FrameReturn, memory: MemoryView, value: Value) -> Resumed {
    // SAFETY: the call record belongs to the caller's live view.
    let result = unsafe { (*frame.call).a };
    lane_resume_at(frame, memory, following(frame.call), Some((result, value)))
}

/// The caller of the returned `frame`, restored with `memory`, resumes at the record `at` of
/// its view, its register `written.0` taking `written.1` first where one is named: the call's
/// result register (`staged::call_complete::write_call_result`), or a delegate's state register.
#[inline(always)]
pub(super) fn lane_resume_at(frame: &FrameReturn, memory: MemoryView, at: Ip, written: Option<(crate::bytecode::Register, Value)>) -> Resumed {
    if let Some((register, value)) = written {
        concrete_write_call_result(frame.caller_registers.0.expose_provenance() as u64, register, value);
    }
    let view = Carried {
        memory,
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    Resumed::At(frame.caller_registers, at, view)
}

/// Whether the caller a returning call completes in turn with `value` returns from its frame
/// in the general loop instead: the frame itself, an activation whose promise's settlement
/// the general path makes (`lane_settled_then` reads the `then` it resolves with into
/// `then`), or a trap whose result the target's invariants refuse (`proxy_traps::trap_result_needs_general_path`).
#[inline(always)]
pub(super) fn lane_caller_returns_in_frame<H: Host>(vm: &mut Vm<H>, value: Value, then: &mut Value) -> bool {
    // The caller read where it is (no spill, one-frame-one-call.md F4).
    match vm.lane_stack.top_return().map(|caller| caller.completion) {
        None => true,
        Some(Some(Completion::SettlesPromise)) => match lane_settled_then(vm, value) {
            Some(read) => {
                *then = read;
                false
            }
            None => true,
        },
        Some(completion) => proxy_traps::trap_result_needs_general_path(vm, completion, value),
    }
}

/// The `then` a settling activation completing with `value` resolves its promise with
/// (`Vm::plain_resolution_then`), read once; `None` where it settles the promise in its frame
/// instead: its resolution would read the value's `then` impurely or run a lifecycle hook,
/// either of which may run guest code. The frame's pending call resolves the promise as
/// `Vm::promise_resolve_value` does.
#[inline(always)]
pub(super) fn lane_settled_then<H: Host>(vm: &mut Vm<H>, value: Value) -> Option<Value> {
    if !concrete_settles_in_lane(vm.host.observes_promise_lifecycle()) {
        return None;
    }
    vm.plain_resolution_then(value)
}

/// The completion of a settling activation (`Completion::SettlesPromise`) whose caller the
/// lane restored, with `value` and the `then` `lane_settled_then` read from it: its promise,
/// the innermost of `LaneStack::settling`, resolved with the value, which its call yields.
#[inline(never)]
pub(super) fn lane_settle_promise<H: Host>(cx: *mut LaneContext<H>, value: Value, then: Value) -> Value {
    let vm = unsafe { vm(cx) };
    let rooted = vm.lane_stack.settling.pop().expect("each settling activation has its entry");
    // Only the `await` after the call reads an unrooted promise: a primitive fulfills it, and
    // awaiting a primitive takes the one job awaiting that fulfilled promise takes.
    if concrete_settles_without_promise(rooted.is_some(), vm.is_object_like(value)) {
        return value;
    }
    let root = rooted.unwrap_or_else(|| {
        let root = vm.settling_root(None);
        note_allocation(vm);
        root
    });
    let promise = vm.release_settled_call_root(root);
    // SAFETY: the context's program, the caller's, outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let settled = vm.resolve_fresh_promise_plainly(code, promise, value, then);
    debug_assert!(settled, "a settling call's promise is a promise record");
    promise
}
