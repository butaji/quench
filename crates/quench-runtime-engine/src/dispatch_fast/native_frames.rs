//! The native frames of a lane body's direct calls, which live in the lane stack's own
//! activations. Emitted code pushes one by writing its prefix and `ActivationKind::Native`
//! (`compiled::DirectCallee`), through the stack's top word; the lane links it, once, when a
//! helper first observes it: a link writes the activation its call would have made from the
//! frames' descriptors alone, and never moves the context, so the body keeps running natively.
//! A leave inside the frames then continues the lane in the innermost (`advance_into_frames`).
use super::*;
use crate::compiled::{DirectCallee, LANE_CALL_LEFT, LANE_CALL_TERMINATED};
use quench_runtime::staged::frame::{CONTEXT_CALL, CONTEXT_CALLER_BASE, CONTEXT_PACKED};

/// What a call's caller runs with, which its activation saves for the return: the registers
/// and records of its function, and its program. Its receiver, environment and captures are
/// its window's context words (`Registers::this`, `env`, `capture`), which stay where they are.
#[derive(Clone, Copy)]
struct CallerState {
    registers: Registers,
    base: Ip,
    program: ProgramId,
}

impl CallerState {
    #[inline(always)]
    fn of_context(cx: *mut LaneContext) -> Self {
        // SAFETY: the running lane's context, which nothing else accesses.
        unsafe { Self { registers: (*cx).registers, base: (*cx).base, program: (*cx).program } }
    }

    /// What the body of a native frame (its `callee` descriptor, its `window`) runs with, which a
    /// lane call into its callee would have made the context's (`enter_window_in_lane`). A direct
    /// callee runs in its caller's program.
    fn of_frame(callee: *const DirectCallee, window: usize) -> Self {
        // SAFETY: a frame names the caller-owned callee, whose view outlives the run.
        let callee = unsafe { &*callee };
        Self {
            registers: Registers(window as *mut Value),
            base: unsafe { (*callee.target.view).records() },
            program: callee.target.program(),
        }
    }

    /// Makes this the context's state; the program is the run's, which no direct call changes.
    #[inline(always)]
    fn install(self, cx: *mut LaneContext) {
        // SAFETY: as `of_context`.
        unsafe { (*cx).registers = self.registers; (*cx).base = self.base; }
    }
}

#[quench_macros::ext(DispatchFastNativeFramesVm)]
impl Vm {
    /// Links the native frames on top of the activations, if any: the one test a helper
    /// that observes the lane stack makes before it does.
    #[inline(always)]
    pub(super) fn link_natives(&mut self, cx: *mut LaneContext) {
        if self.lane_stack.top_is_native() { link_unlinked(cx); }
    }
}

#[cold]
#[inline(never)]
fn link_unlinked(cx: *mut LaneContext) {
    // SAFETY: the reference ends here; native frames are records on top of the records.
    let activations = unsafe { vm(cx).lane_stack.records_unspilled() };
    let (unlinked, len) = (activations.unlinked(), activations.len());
    for depth in len - unlinked..len { link_on_observe(cx, depth); }
    // The plain headers above the records, outermost first (one-frame-one-call.md F5).
    for (window, index) in unsafe { vm(cx) }.lane_stack.unlinked_native_headers() {
        link_header(cx, window, index);
    }
}

/// The NATIVE_FLAG bit of a plain header's packed word, read through `window`.
#[inline(always)]
pub(super) fn header_packed(window: Registers) -> u64 {
    // SAFETY: a plain header's words lie below its window, which the lane stack holds.
    unsafe { *window.0.sub(CONTEXT_PACKED).cast::<u64>() }
}

#[quench_macros::ext(LaneStackOps6)]
impl LaneStack {
    /// The native headers not yet linked, the top run of the plain headers, outermost first, each with
    /// its table index: the frames below the plain headers plus the header's place in the plain run.
    pub(super) fn unlinked_native_headers(&self) -> Vec<(Registers, usize)> {
        let (depth, base) = (self.plain.depth.get(), self.frames_below_plain());
        let mut window = Registers(self.plain.registers.get());
        let mut found = Vec::new();
        for above in 0..depth {
            if header_packed(window) & quench_runtime::staged::frame::NATIVE_FLAG == 0 { break; }
            found.push((window, base + depth - 1 - above));
            // SAFETY: the header's caller registers word, written at its push.
            window = window.plain_caller().1;
        }
        found.reverse();
        found
    }

    /// How many native headers run on top, linked or not (`native_depth` above a plain header): a
    /// header is a native frame while its NATIVE_FLAG is set, or its side record names a descriptor.
    pub(super) fn header_native_depth(&self) -> usize {
        // SAFETY: the sides are read, not written, here.
        let sides = unsafe { &*self.sides.get() };
        let mut window = Registers(self.plain.registers.get());
        let mut side_at = sides.len();
        let mut count = 0;
        for _ in 0..self.plain.depth.get() {
            let native = if header_packed(window) & quench_runtime::staged::frame::NATIVE_FLAG != 0 {
                true
            } else if super::lane_frames::header_sided(window) {
                side_at -= 1;
                sides[side_at].native != super::lane_frames::NO_NATIVE
            } else {
                false
            };
            if !native { break; }
            count += 1;
            // SAFETY: as in `unlinked_native_headers`.
            window = window.plain_caller().1;
        }
        count
    }
}

/// Links the native header at `window`, whose table index is `index` (one-frame-one-call.md F5): its call
/// record and caller base come from its descriptor, its argc from its call record, and it gets a side record
/// naming the descriptor, pushed on top of the sides. Its caller registers are the header's own word, written
/// at its push. The window takes the context words its call would have written (`enter_window_in_lane`).
fn link_header(cx: *mut LaneContext, window: Registers, index: usize) {
    let vm = unsafe { vm(cx) };
    let stack = &mut vm.lane_stack;
    // SAFETY: the table entry was written by the push that made the header; its descriptor outlives the run.
    let callee = unsafe { &*(stack.native_table[index] as *const crate::compiled::DirectCallee) };
    let argc = ImmediateLayout::argument_count(unsafe { (*(callee.call as Ip)).imm() });
    let completion = if callee.forwarder.is_some() { Completion::Forwarder } else { Completion::Plain };
    let program = unsafe { (*cx).program };
    let packed = header_packed(window);
    // SAFETY: the header's words lie below its window.
    unsafe {
        let words = window.0;
        *words.sub(CONTEXT_CALL).cast::<u64>() = callee.call as u64;
        *words.sub(CONTEXT_CALLER_BASE).cast::<u64>() = callee.caller_base as u64;
        let flags = (packed & !quench_runtime::staged::frame::NATIVE_FLAG) | quench_runtime::staged::frame::COMPLETION_FLAG;
        *words.sub(CONTEXT_PACKED).cast::<u64>() = flags | (u64::from(argc) << quench_runtime::staged::frame::ARGC_SHIFT);
    }
    window.set_env(callee.target.env);
    window.set_capture(callee.target.capture);
    let mut side = super::lane_frames::Side::plain(completion, program);
    side.native = index;
    if let Some(skipped) = callee.forwarder { side.payload = super::lane_frames::SidePayload::Forwarder(skipped); }
    // SAFETY: the sides are written through `&mut self`'s stack, and no walk runs during a link.
    unsafe { (*stack.sides.get()).push(side) };
    vm.programs.record_js_jit_lane_entry(callee.target.program(), callee.target.function);
}

/// Links the native frame at `depth`: the `Script` activation its call would have made. Its
/// caller is the frame below it where that one has a descriptor, else the running context,
/// which is the state of the activation the body runs in. A link never advances the context.
pub(super) fn link_on_observe(cx: *mut LaneContext, depth: usize) {
    let vm = unsafe { vm(cx) };
    // SAFETY: each slot below the top holds at least its prefix.
    let below = depth.checked_sub(1).map(|below| unsafe { let record = &*vm.lane_stack.records_mut().slot(below); (record.callee, record.window) }).filter(|&(callee, _)| !callee.is_null());
    let caller = below.map_or_else(|| CallerState::of_context(cx), |(callee, window)| CallerState::of_frame(callee, window));
    let slot = vm.lane_stack.records_mut().slot(depth);
    // SAFETY: the frame names the caller-owned callee, whose call record lies in its caller's view.
    let (callee_at, this, body, window_at) = unsafe { let prefix = &*slot; (prefix.callee, prefix.this, prefix.body, prefix.window) };
    let callee = unsafe { &*callee_at };
    let argc = ImmediateLayout::argument_count(unsafe { (*(callee.call as Ip)).imm() });
    let completion = if callee.forwarder.is_some() { Completion::Forwarder } else { Completion::Plain };
    // SAFETY: the slot lies within the activations' capacity, below the top.
    unsafe {
        slot.write(Activation {
            callee: callee_at, this, body, window: window_at,
            function: callee.target.function,
            call: callee.call as Ip,
            caller_base: caller.base,
            caller_registers: caller.registers,
            caller_program: caller.program,
            kind: ActivationKind::Script(ScriptActivation {
                callee: callee.target.callee,
                argc,
                completion,
                forwards: false,
                kept: 0,
                environment: false,
                record: std::ptr::null_mut(),
            }),
        });
    }
    // The frame's window takes the context words its call would have written (`enter_window_in_lane`).
    let window = Registers(window_at as *mut Value);
    window.set_this(Value::from_bits(this));
    window.set_env(callee.target.env);
    window.set_capture(callee.target.capture);
    vm.programs.record_js_jit_lane_entry(callee.target.program(), callee.target.function);
}

/// Runs `helper`, which pushes activations or runs guest code, with the context of the
/// innermost native frame's body, as the lane's own calls run: they take their caller's state
/// from the context. The frames are linked first, then a body entered through its cell, which has
/// no activation yet, becomes one (`materialize_running_leaf_in_lane`): the record the helper runs
/// is that activation's, not the frame below it. The owner's state returns unless the helper left
/// the lane in an activation of its own or ended the run.
#[inline(always)]
pub(super) fn in_innermost_frame(cx: *mut LaneContext, helper: impl FnOnce() -> u64) -> u64 {
    in_innermost_frame_of(cx, helper)
}

#[inline(always)]
fn in_innermost_frame_of(cx: *mut LaneContext, helper: impl FnOnce() -> u64) -> u64 {
    let vm = unsafe { vm(cx) };
    vm.link_natives(cx);
    materialize_running_leaf_in_lane(cx);
    let Some(frame) = vm.lane_stack.top_native() else { return helper() };
    let owner = CallerState::of_context(cx);
    // Native frames are records on top of the records: a record index.
    let depth = vm.lane_stack.record_count();
    CallerState::of_frame(frame.callee, frame.window).install(cx);
    let status = helper();
    if status == LANE_CALL_LEFT {
        // The lane continues above the frames, which return into the interpreter.
        retire_header_bodies(cx);
        vm.lane_stack.records_mut().retire_bodies(depth);
    } else if status != LANE_CALL_TERMINATED {
        owner.install(cx);
    }
    status
}

/// The environment a running body's captures resolve in when it runs as a native callee: its
/// closure's, which no lane activation holds; `None` in the activation's own.
pub(super) fn innermost_callee_env(cx: *mut LaneContext) -> Option<Value> {
    // SAFETY: a frame names the caller-owned callee, whose descriptor outlives the run.
    unsafe { vm(cx) }.lane_stack.top_native().map(|frame| unsafe { (*frame.callee).target.env })
}

/// Continues the lane in the innermost native frame a run left inside: the frames become
/// the lane's own activations (their prefix no longer names a callee, and a skipped
/// forwarding constructor rides in its side record, `SidePayload::Forwarder`), and the context becomes the
/// innermost's. Its view and the body it ran, where the run left inside frames.
#[inline(always)]
pub(super) fn advance_into_frames(cx: *mut LaneContext) -> Option<(&'static LaneView, &'static crate::compiled::LaneMethod)> {
    // A run that left no native frame, the common completion, has nothing to link or advance.
    if !left_native_frames(cx) { return None; }
    advance_into_native_frames(cx)
}

/// Whether the run that just ended left native frames on the lane stack, linked or not.
#[inline(always)]
pub(super) fn left_native_frames(cx: *mut LaneContext) -> bool {
    let stack = &unsafe { vm(cx) }.lane_stack;
    stack.top_is_native() || stack.native_depth() != 0
}

/// `advance_into_frames` past its test: the native frames to link and advance into.
#[inline(never)]
fn advance_into_native_frames(cx: *mut LaneContext) -> Option<(&'static LaneView, &'static crate::compiled::LaneMethod)> {
    let vm = unsafe { vm(cx) };
    vm.link_natives(cx);
    let headers = vm.lane_stack.header_native_depth();
    let (count, len) = (vm.lane_stack.record_native_depth_in_run(headers), vm.lane_stack.record_count());
    // The innermost frame's callee, body and window, as the frames leave it.
    let mut innermost: Option<(*const DirectCallee, usize, usize)> = None;
    for depth in len - count..len {
        let slot = vm.lane_stack.records_mut().slot(depth);
        // SAFETY: a linked frame holds its prefix; its callee outlives the run.
        let (callee, body, window) = unsafe { let prefix = &*slot; (prefix.callee, prefix.body, prefix.window) };
        // The leave inside the callee's body counts on its cell's word, which no handler entry notes.
        unsafe { &*(*callee).target.view }.note_native_leave();
        unsafe { (*slot).callee = std::ptr::null() };
        // Its skipped constructor, its descriptor's until now, rides in its side record from
        // here on (`LaneStack::record_sided`); the frames are the innermost, outermost first.
        if let Some(skipped) = unsafe { &*callee }.forwarder {
            vm.lane_stack.push_record_side(depth, SidePayload::Forwarder(skipped));
        }
        innermost = Some((callee, body, window));
    }
    // The plain headers that are native, innermost first: each is advanced into and its side names no
    // callee after (`Side::native`); the innermost's prefix is the one the lane continues in.
    if headers != 0 {
        let stack = &mut vm.lane_stack;
        let depth = stack.plain.depth.get();
        let base = stack.frames_below_plain();
        let mut window = Registers(stack.plain.registers.get());
        for above in 0..headers {
            let index = base + depth - 1 - above;
            let side_at = unsafe { (*stack.sides.get()).len() } - 1 - above;
            let side = unsafe { &mut (&mut *stack.sides.get())[side_at] };
            let desc = unsafe { &*(stack.native_table[index] as *const crate::compiled::DirectCallee) };
            unsafe { &*desc.target.view }.note_native_leave();
            let frame = (desc as *const DirectCallee, stack.body_table[index], window.0.addr());
            if let Some(skipped) = desc.forwarder { side.payload = super::lane_frames::SidePayload::Forwarder(skipped); }
            side.native = super::lane_frames::NO_NATIVE;
            if above == 0 { innermost = Some(frame); }
            window = window.plain_caller().1;
        }
    }
    let (callee_at, body, window) = innermost?;
    CallerState::of_frame(callee_at, window).install(cx);
    // The body the call ran is the cell's at that moment, not the descriptor's; the caller's
    // code lease pins a named callee body through recovery, and the view pins every body it published.
    let callee = unsafe { &*callee_at };
    Some(unsafe { (&*callee.target.view, &*(body as *const crate::compiled::LaneMethod)) })
}

/// Drops the native frames a terminated run left, with their windows.
pub(super) fn discard_frames(cx: *mut LaneContext) {
    let vm = unsafe { vm(cx) };
    let headers = vm.lane_stack.header_native_depth();
    let (count, len) = (vm.lane_stack.record_native_depth_in_run(headers), vm.lane_stack.record_count());
    if headers == 0 && count == 0 { return; }
    // The headers first, innermost out: the last one popped is the outermost, whose window the run releases.
    let mut outer = None;
    for _ in 0..headers { outer = vm.lane_stack.pop_activation().map(|frame| frame.window_word()); }
    if count != 0 {
        // SAFETY: the values hold no drop glue; native frames own no side record.
        unsafe {
            outer = Some((*vm.lane_stack.records_mut().slot(len - count)).window_word());
            vm.lane_stack.records_mut().set_len(len - count);
        }
    }
    // SAFETY: the window belonged to the outermost discarded native record, now gone.
    if let Some(window) = outer { unsafe { vm.lane_stack.values.release_window(window) }; }
}

/// Retires the bodies of the native headers the lane continues in, innermost out, as `retire_bodies` does for
/// records: their table entries name no body after this (one-frame-one-call.md F5).
pub(super) fn retire_header_bodies(cx: *mut LaneContext) {
    let stack = &mut unsafe { vm(cx) }.lane_stack;
    let headers = stack.header_native_depth();
    let (depth, base) = (stack.plain.depth.get(), stack.frames_below_plain());
    for above in 0..headers {
        let index = base + depth - 1 - above;
        if stack.body_table[index] == 0 { break; }
        stack.body_table[index] = 0;
    }
}
