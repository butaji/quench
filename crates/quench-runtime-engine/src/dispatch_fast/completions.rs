//! Lane activations' completions: what a return yields to the call it completes, by the
//! completion its activation was made with (`Completion`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::call_return::ChainCompletion;
use quench_runtime::staged::call_complete::{
    concrete_admits_returning_call, concrete_returning_this, concrete_returns_callee_completion,
    concrete_settles_in_lane, concrete_settles_without_promise, concrete_side_completion, concrete_write_call_result, frame_packed,
};
use quench_runtime::staged::Concrete;

/// The completion of the innermost activation with `value`, which the record at `ip`
/// returns, past the common return (`complete_plain_activation_in_lane`), by what it yields to
/// its call: a native's callback ends the run (`complete_native_callback_in_lane`), a plain
/// activation completes a run of returning calls (`return f(...)`,
/// `complete_plain_chain_in_lane`), another program's activation enters its caller's program
/// (`complete_cross_activation_in_lane`), and any other completion, or any of these past its
/// common form, completes as `complete_activation_in_lane` completes it. `None` when no
/// activation runs: the running function is a VM frame.
#[inline(always)]
pub(crate) fn complete_returning_in_lane(cx: *mut LaneContext, ip: Ip, value: Value) -> Option<Resumed> {
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
        Some(Completion::SettlesPromise) => return_from_in_lane::<false, false, false, true>(cx, value).map(|(r, next)| Resumed::At(r, next, Carried::script(cx))),
        Some(Completion::Constructs) => return_from_in_lane::<false, false, true, false>(cx, value).map(|(r, next)| Resumed::At(r, next, Carried::script(cx))),
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
fn complete_trapped_read_in_lane(cx: *mut LaneContext, ip: Ip, value: Value) -> Option<Resumed> {
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

/// The lane's primitives of a script activation's return (`call_return::ScriptReturn`), for a
/// record of leaf form `LEAF`: `window` is the running plain frame's, which a returning run moves.
pub(crate) struct LaneScriptReturn<const LEAF: u8> { pub(crate) cx: *mut LaneContext, pub(crate) window: Registers }

impl<const LEAF: u8> LaneScriptReturn<LEAF> {
    /// The return of the running activation, from the lane stack's running plain window.
    #[inline(always)]
    pub(crate) fn running(cx: *mut LaneContext) -> Self { Self { cx, window: Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get()) } }
}

impl<const LEAF: u8> quench_runtime::staged::call_return::ScriptReturn for LaneScriptReturn<LEAF> {
    type Value = Value;
    type Returned = (Registers, Ip);
    #[inline(always)]
    fn leaf(&self) -> bool { LEAF != NOT_LEAF }
    #[inline(always)]
    fn leaf_header(&mut self, value: Value) -> Option<(Registers, Ip)> { super::leaf_calls::lane_return_leaf_header(self.cx, value, LEAF) }
    #[inline(always)]
    fn complete_leaf(&mut self, value: Value) -> Option<(Registers, Ip)> { complete_leaf_in_lane(self.cx, value, LEAF) }
    #[inline(always)]
    fn plain_header(&mut self, value: Value) -> Option<(Registers, Ip)> { lane_return_header::<false>(self.cx, self.window, value) }
    #[inline(always)]
    fn returning_run(&mut self) -> Option<usize> { returning_plain_run(self.cx, self.window) }
    #[inline(always)]
    fn pop_returning(&mut self, above: usize) {
        // The run's frames were proved by `returning_plain_run`, which read their headers: the pops are one
        // walk down the caller links and one write of the plain words, not a staged pop per frame.
        if above == 0 { return; }
        let stack: &mut LaneStack = &mut unsafe { vm(self.cx) }.lane_stack;
        let (mut released, mut window, mut base) = (self.window, self.window, std::ptr::null());
        for _ in 0..above {
            released = window;
            let (_, caller_registers, caller_base) = window.plain_caller();
            (window, base) = (caller_registers, caller_base);
        }
        stack.plain.depth.set(stack.plain.depth.get() - above);
        stack.plain.registers.set(window.0);
        stack.frames_changed();
        // SAFETY: plain windows are the lane stack's own, so the outermost one's release is the top's move.
        unsafe { stack.values.set_top(released.0) };
        self.window = window;
        unsafe { ((*self.cx).registers, (*self.cx).base) = (window, base) };
    }
    #[inline(always)]
    fn side_return(&mut self, value: Value) -> Option<(Registers, Ip)> { return_from_in_lane::<false, false, false, false>(self.cx, value) }
}

/// How many plain activations, innermost first from the one on `window`, return their callee's
/// completion before one whose header returns plainly into a call that keeps it; `None` where
/// the run ends any other way. Reads headers only.
#[inline(always)]
fn returning_plain_run(cx: *mut LaneContext, mut window: Registers) -> Option<usize> {
    let depth = unsafe { vm(cx) }.lane_stack.plain.depth.get() as usize;
    let stage = &mut quench_runtime::staged::Concrete::new(std::ptr::null());
    for above in 0..depth {
        let packed = quench_runtime::staged::call_complete::frame_packed(stage, window.0.expose_provenance() as u64);
        if !quench_runtime::staged::call_complete::completes_plainly(stage, packed) {
            return None;
        }
        let (call, caller_registers, _) = window.plain_caller();
        // SAFETY: the call record belongs to the caller's live view.
        let destination = unsafe { (*call).a };
        if !quench_runtime::staged::call_complete::returns_callee_completion(stage, u64::from(destination)) {
            return (above != 0).then_some(above);
        }
        window = caller_registers;
    }
    None
}

/// The result word of the call record `call` a plain frame's header names, for the staged returns.
#[inline(always)]
fn plain_destination(_: &mut Concrete, call: u64) -> u64 {
    // SAFETY: a plain frame's call record belongs to the caller's live view (a trampoline's header
    // names the trampoline's record, which keeps its result).
    u64::from(unsafe { (*std::ptr::with_exposed_provenance::<LaneInstruction>(call as usize)).a })
}

/// The plain return of the innermost activation running on `window` (`call_complete::step_plain_return_of`):
/// a plain frame whose header completes it (no side flag, and a call that keeps its result, or any call
/// where `INTO_BODY`) pops, its caller's result register takes `value`, and its window is released.
/// `None`, which changes nothing, for any other frame: a side record's completion (`return_from_in_lane`),
/// a returning call's chain (`return_chain_in_lane`), or a record. It makes no call.
#[inline(always)]
pub(crate) fn lane_return_header<const INTO_BODY: bool>(cx: *mut LaneContext, window: Registers, value: Value) -> Option<(Registers, Ip)> {
    let stack: &mut LaneStack = &mut unsafe { vm(cx) }.lane_stack;
    debug_assert!(stack.plain.depth.get() == 0 || (stack.plain.registers.get() == window.0 && unsafe { (*cx).registers }.0 == window.0));
    let plain = (&raw const stack.plain).expose_provenance() as u64;
    let caller = quench_runtime::staged::call_complete::step_plain_return_of(&mut Concrete::new(std::ptr::null()), plain,
        window.0.expose_provenance() as u64, value.to_bits(), INTO_BODY, plain_destination, ())?;
    stack.frames_changed();
    // SAFETY: a plain window is the lane stack's own, so its release is the top's move.
    unsafe { stack.values.set_top(window.0) };
    let registers = Registers(std::ptr::with_exposed_provenance_mut(caller.registers as usize));
    unsafe { ((*cx).registers, (*cx).base) = (registers, std::ptr::with_exposed_provenance(caller.base as usize)) };
    Some((registers, following(std::ptr::with_exposed_provenance(caller.call as usize))))
}

/// `call_return::step_activation_return_of` into a running lane body's call (`body_call_helper`): a returning
/// call's register takes the result too, since the body completes its own activation with
/// it next (`LaneExit::Returned`).
#[inline(always)]
pub(crate) fn return_to_body_in_lane(cx: *mut LaneContext, value: Value) -> Option<(Registers, Ip)> {
    let window = Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get());
    lane_return_header::<true>(cx, window, value).or_else(|| return_from_in_lane::<false, true, false, false>(cx, value))
}

/// `complete_plain_activation_in_lane` of an activation of another program than its caller's
/// (`Completion::Cross`), which enters the caller's program first, for the returns past the
/// common one (`return_general_in_lane`, `return_value_general_in_lane`), so the common return
/// carries none of the program switch.
#[inline(always)]
pub(crate) fn complete_cross_activation_in_lane(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    quench_runtime::staged::call_return::step_cross_return_of(&mut LaneChain { cx }, value)
}

/// Whether a call record that returns its callee's completion runs in the lane: from an
/// activation, which completes in turn, or from the frame, which the general loop then
/// completes with the value (`frame_return`). A host permitting proper tail calls sends
/// a strict caller's terminal calls to the general dispatch path instead.
#[inline(always)]
pub(crate) fn returning_call_admitted(cx: *mut LaneContext) -> bool {
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
pub(crate) fn frame_return(vm: &mut Vm, call: Ip, value: Value) -> Ip {
    vm.lane_completion.set(LaneCompletion::Returned(value));
    exit(call)
}

/// Complete the innermost lane activation with `value` when it is the common return
/// (`call_return::step_activation_return_of`): a JavaScript activation of the running program's caller yielding
/// `value` to its call's result register. Each step loads or stores, so a handler returning
/// through it saves no registers. `None`, which changes nothing, leaves every other completion
/// to `complete_activation_in_lane`. A return record of a leaf's records (`LEAF`,
/// `LaneView::specialize_leaf_returns`) completes the running leaf call first
/// (`complete_leaf_in_lane`); no other record runs while one does.
#[inline(always)]
pub(crate) fn complete_plain_activation_in_lane<const LEAF: u8>(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    let returned = quench_runtime::staged::call_return::step_activation_return_of(&mut LaneScriptReturn::<LEAF>::running(cx), value)?;
    Some(resumed_script(cx, returned))
}

/// The completion of a `Return` record of leaf form `LEAF` past its one op (`step_return_past_op_of`),
/// from the running activation.
#[inline(always)]
pub(crate) fn complete_past_op_in_lane<const LEAF: u8>(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    let returned = quench_runtime::staged::call_return::step_return_past_op_of(&mut LaneScriptReturn::<LEAF>::running(cx), value)?;
    Some(resumed_script(cx, returned))
}

/// The completion of a `Return` record of leaf form `LEAF` in the lane: its one op from the record's
/// `window` (`step_return_one_op_of`), else past it (`complete_past_op_in_lane`).
#[inline(always)]
pub(crate) fn return_record_in_lane<const LEAF: u8>(cx: *mut LaneContext, window: Registers, value: Value) -> Option<Resumed> {
    let one_op = quench_runtime::staged::call_return::step_return_one_op_of(&mut LaneScriptReturn::<LEAF> { cx, window }, value);
    match one_op {
        Some(returned) => Some(resumed_script(cx, returned)),
        None => complete_past_op_in_lane::<LEAF>(cx, value),
    }
}

/// Where a script return continues: its caller's registers and next record, in script memory.
#[inline(always)]
pub(crate) fn resumed_script(cx: *mut LaneContext, (registers, next): (Registers, Ip)) -> Resumed {
    Resumed::At(registers, next, Carried { memory: MemoryView::script(cx), integers: quench_runtime::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) })
}

/// `complete_activation_in_lane` of a run of JavaScript activations, of one program or several,
/// each returning the completion of the call above it (`return f(...)`), down to one whose call
/// takes `value` in its result register. Only that outermost activation's caller state is
/// restored: an inner one's would be overwritten by it. `None`, which changes nothing, leaves
/// any other completion to the activation-by-activation loop.
#[inline(always)]
pub(crate) fn complete_plain_chain_in_lane(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    quench_runtime::staged::call_return::step_returning_chain_of(&mut LaneChain { cx }, value)
}

/// The lane's primitives of a returning chain and a crossing return (`call_return::ReturningChain`).
struct LaneChain { cx: *mut LaneContext }

impl quench_runtime::staged::call_return::ReturningChain for LaneChain {
    type Value = Value;
    type Resume = Resumed;
    type Frame = ActivationView;
    #[inline(always)]
    fn frames(&mut self) -> impl Iterator<Item = Option<ActivationView>> {
        // The run walked where its frames are, headers with their side records, then records: no
        // spill and no record built (one-frame-one-call.md F4).
        unsafe { vm(self.cx) }.lane_stack.frames_innermost().map(|frame| frame.completion().map(|_| frame))
    }
    #[inline(always)]
    fn completion(frame: &ActivationView) -> ChainCompletion {
        match frame.completion() {
            Some(Completion::Plain) => ChainCompletion::Plain,
            Some(Completion::Cross) => ChainCompletion::Cross,
            Some(Completion::ReturnsToBody) => ChainCompletion::ReturnsToBody,
            _ => ChainCompletion::Flagged,
        }
    }
    // SAFETY: the call record belongs to the caller's live view.
    #[inline(always)]
    fn call_result(frame: &ActivationView) -> u64 { u64::from(unsafe { (*frame.call()).a }) }
    #[inline(always)]
    fn discard(&mut self, above: usize, outermost: ActivationView) {
        // The inner ones go where they are: script activations hold no Wasm storage
        // (`LaneStack::pop_activation`), and their windows lie above the outermost's, which its
        // return releases. What they owned goes with them.
        let vm = unsafe { vm(self.cx) };
        vm.lane_stack.drop_owned_from(vm.lane_stack.activation_count() - above);
        for _ in 0..above {
            // SAFETY: an activation runs, a script one of the chain.
            unsafe { vm.lane_stack.discard_top_frame() };
        }
        if above != 0 {
            // The outermost runs now: its window is the running one its return pops.
            unsafe { (*self.cx).registers = Registers(outermost.window_ptr()) };
        }
    }
    #[inline(always)]
    fn end_body(&mut self, value: Value) -> Option<Resumed> { end_body_callee(self.cx, value) }
    #[inline(always)]
    fn reenter_caller(&mut self, outermost: ActivationView) {
        enter_program_in_lane(self.cx, outermost.caller_program());
        unsafe { (*self.cx).memory = MemoryView::script(self.cx) };
    }
    #[inline(always)]
    fn complete_plain(&mut self, value: Value) -> Option<Resumed> { complete_plain_activation_in_lane::<NOT_LEAF>(self.cx, value) }
    #[inline(always)]
    fn plain_header_crosses(&mut self) -> Option<bool> {
        let stack = &unsafe { vm(self.cx) }.lane_stack;
        (stack.plain.depth.get() != 0).then(|| {
            concrete_side_completion(frame_packed(&mut Concrete::new(std::ptr::null()), stack.plain.registers.get().expose_provenance() as u64)).crosses
        })
    }
    #[inline(always)]
    fn return_crossing(&mut self, value: Value) -> Option<Resumed> {
        let (registers, next) = return_from_in_lane::<true, false, false, false>(self.cx, value)?;
        Some(Resumed::At(registers, next, Carried::script(self.cx)))
    }
}

/// `complete_plain_chain_in_lane` out of line, for the returns past the common one
/// (`return_general_in_lane`, `return_value_general_in_lane`), whose frames stay small.
#[inline(never)]
pub(crate) fn complete_returning_chain_in_lane(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    complete_plain_chain_in_lane(cx, value)
}

/// The end of the nested run of a body's callee, the innermost activation (`Completion::ReturnsToBody`),
/// with `value`: its caller's state restored and the call's register set, as the general loop's
/// completion of it does (`body_calls::ends_run`).
#[inline(always)]
pub(crate) fn end_body_callee(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
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
pub(crate) fn complete_native_callback_in_lane(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
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
pub(crate) fn restore_caller_in_lane(cx: *mut LaneContext, frame: &ActivationView) -> (MemoryView, Value) {
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
pub(crate) fn resume_after_call_in_lane(frame: &ActivationView, memory: MemoryView, value: Value) -> Resumed {
    // SAFETY: the call record belongs to the caller's live view.
    let result = unsafe { (*frame.call()).a };
    resume_at_in_lane(frame, memory, following(frame.call()), Some((result, value)))
}

/// The caller of the returned `frame`, restored with `memory`, resumes at the record `at` of
/// its view, its register `written.0` taking `written.1` first where one is named: the call's
/// result register (`staged::call_complete::write_call_result`), or a delegate's state register.
#[inline(always)]
pub(crate) fn resume_at_in_lane(frame: &ActivationView, memory: MemoryView, at: Ip, written: Option<(quench_runtime::bytecode::Register, Value)>) -> Resumed {
    if let Some((register, value)) = written {
        concrete_write_call_result(frame.caller_registers().0.expose_provenance() as u64, register, value);
    }
    let view = Carried {
        memory,
        integers: quench_runtime::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    Resumed::At(frame.caller_registers(), at, view)
}

/// Whether the caller a returning call completes in turn with `value` returns from its frame
/// in the general loop instead: the frame itself, an activation whose promise's settlement
/// the general path makes (`settled_then_in_lane` reads the `then` it resolves with into
/// `then`), or a trap whose result the target's invariants refuse (`proxy_traps::trap_result_needs_general_path`).
#[inline(always)]
pub(crate) fn caller_returns_in_frame(vm: &mut Vm, value: Value, then: &mut Value) -> bool {
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
pub(crate) fn settled_then_in_lane(vm: &mut Vm, value: Value) -> Option<Value> {
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
pub(crate) fn settle_promise_in_lane(cx: *mut LaneContext, value: Value, then: Value, rooted: Option<quench_runtime::tier_surface::RootId>) -> Value {
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
