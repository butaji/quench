//! The native frames of a lane body's direct calls, which live in the lane stack's own
//! activations. Emitted code pushes one by writing its prefix and `ActivationKind::Native`
//! (`compiled::DirectCallee`), through the stack's top word; the lane links it, once, when a
//! helper first observes it: a link writes the activation its call would have made from the
//! frames' descriptors alone, and never moves the context, so the body keeps running natively.
//! A leave inside the frames then continues the lane in the innermost (`advance_into_frames`).
use super::*;
use super::super::compiled::{ShadowFrame, LANE_CALL_LEFT, LANE_CALL_TERMINATED};
use crate::staged::frame::{CONTEXT_CALL, CONTEXT_CALLER_BASE, CONTEXT_PACKED};

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
    fn of_context<H: Host>(cx: *mut LaneContext<H>) -> Self {
        // SAFETY: the running lane's context, which nothing else accesses.
        unsafe { Self { registers: (*cx).registers, base: (*cx).base, program: (*cx).program } }
    }

    /// What the body `frame` runs with, which a lane call into its callee would have made the
    /// context's (`lane_enter_window`). A direct callee runs in its caller's program.
    fn of_frame(frame: &ShadowFrame) -> Self {
        // SAFETY: a frame names the caller-owned callee, whose view outlives the run.
        let callee = unsafe { &*frame.callee };
        Self {
            registers: Registers(frame.window as *mut Value),
            base: unsafe { (*callee.target.view).records() },
            program: callee.target.program(),
        }
    }

    /// Makes this the context's state; the program is the run's, which no direct call changes.
    #[inline(always)]
    fn install<H: Host>(self, cx: *mut LaneContext<H>) {
        // SAFETY: as `of_context`.
        unsafe { (*cx).registers = self.registers; (*cx).base = self.base; }
    }
}

impl<H: Host> Vm<H> {
    /// Links the native frames on top of the activations, if any: the one test a helper
    /// that observes the lane stack makes before it does.
    #[inline(always)]
    pub(super) fn link_natives(&mut self, cx: *mut LaneContext<H>) {
        if self.lane_stack.top_is_native() { link_unlinked(cx); }
    }
}

#[cold]
#[inline(never)]
fn link_unlinked<H: Host>(cx: *mut LaneContext<H>) {
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

impl LaneStack {
    /// The native headers not yet linked, the top run of the plain headers, outermost first, each with
    /// its table index: the frames below the plain headers plus the header's place in the plain run.
    pub(super) fn unlinked_native_headers(&self) -> Vec<(Registers, usize)> {
        let (depth, base) = (self.plain.depth.get(), self.frames_below_plain());
        let mut window = Registers(self.plain.registers.get());
        let mut found = Vec::new();
        for above in 0..depth {
            if header_packed(window) & crate::staged::frame::NATIVE_FLAG == 0 { break; }
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
            let native = if header_packed(window) & crate::staged::frame::NATIVE_FLAG != 0 {
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
/// at its push. The window takes the context words its call would have written (`lane_enter_window`).
fn link_header<H: Host>(cx: *mut LaneContext<H>, window: Registers, index: usize) {
    let vm = unsafe { vm(cx) };
    let stack = &mut vm.lane_stack;
    // SAFETY: the table entry was written by the push that made the header; its descriptor outlives the run.
    let callee = unsafe { &*(stack.native_table[index] as *const super::super::compiled::DirectCallee) };
    let argc = ImmediateLayout::argument_count(unsafe { (*(callee.call as Ip)).imm() });
    let completion = if callee.forwarder.is_some() { Completion::Forwarder } else { Completion::Plain };
    let program = unsafe { (*cx).program };
    let packed = header_packed(window);
    // SAFETY: the header's words lie below its window.
    unsafe {
        let words = window.0;
        *words.sub(CONTEXT_CALL).cast::<u64>() = callee.call as u64;
        *words.sub(CONTEXT_CALLER_BASE).cast::<u64>() = callee.caller_base as u64;
        let flags = (packed & !crate::staged::frame::NATIVE_FLAG) | crate::staged::frame::COMPLETION_FLAG;
        *words.sub(CONTEXT_PACKED).cast::<u64>() = flags | (u64::from(argc) << crate::staged::frame::ARGC_SHIFT);
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
pub(super) fn link_on_observe<H: Host>(cx: *mut LaneContext<H>, depth: usize) {
    let vm = unsafe { vm(cx) };
    // SAFETY: each slot below the top holds at least its prefix.
    let below = depth.checked_sub(1).map(|below| unsafe { (*vm.lane_stack.records_mut().slot(below)).native }).filter(|frame| !frame.callee.is_null());
    let caller = below.map_or_else(|| CallerState::of_context(cx), |frame| CallerState::of_frame(&frame));
    let slot = vm.lane_stack.records_mut().slot(depth);
    // SAFETY: the frame names the caller-owned callee, whose call record lies in its caller's view.
    let (frame, callee) = unsafe { let frame = (*slot).native; (frame, &*frame.callee) };
    let argc = ImmediateLayout::argument_count(unsafe { (*(callee.call as Ip)).imm() });
    let completion = if callee.forwarder.is_some() { Completion::Forwarder } else { Completion::Plain };
    // SAFETY: the slot lies within the activations' capacity, below the top.
    unsafe {
        slot.write(Activation::of_parts(
            frame,
            callee.target.function,
            callee.call as Ip,
            caller.base,
            caller.registers,
            caller.program,
            ActivationKind::Script(ScriptActivation {
                callee: callee.target.callee,
                argc,
                completion,
                forwards: false,
            kept: 0,
                environment: false,
                record: std::ptr::null_mut(),
            }),
        ));
    }
    // The frame's window takes the context words its call would have written (`lane_enter_window`).
    let window = Registers(frame.window as *mut Value);
    window.set_this(Value::from_bits(frame.this));
    window.set_env(callee.target.env);
    window.set_capture(callee.target.capture);
    vm.programs.record_js_jit_lane_entry(callee.target.program(), callee.target.function);
}

/// Runs `helper`, which pushes activations or runs guest code, with the context of the
/// innermost native frame's body, as the lane's own calls run: they take their caller's state
/// from the context. The frames are linked first, then a body entered through its cell, which has
/// no activation yet, becomes one (`lane_materialize_running_leaf`): the record the helper runs
/// is that activation's, not the frame below it. The owner's state returns unless the helper left
/// the lane in an activation of its own or ended the run.
#[inline(always)]
pub(super) fn in_innermost_frame<H: Host>(cx: *mut LaneContext<H>, helper: impl FnOnce() -> u64) -> u64 {
    let vm = unsafe { vm(cx) };
    vm.link_natives(cx);
    lane_materialize_running_leaf(cx);
    let Some(frame) = vm.lane_stack.top_native() else { return helper() };
    let owner = CallerState::of_context(cx);
    // Native frames are records on top of the records: a record index.
    let depth = vm.lane_stack.record_count();
    CallerState::of_frame(&frame).install(cx);
    let status = helper();
    if status == LANE_CALL_LEFT {
        // The lane continues above the frames, which return into the interpreter.
        vm.lane_stack.records_mut().retire_bodies(depth);
    } else if status != LANE_CALL_TERMINATED {
        owner.install(cx);
    }
    status
}

/// The environment a running body's captures resolve in when it runs as a native callee: its
/// closure's, which no lane activation holds; `None` in the activation's own.
pub(super) fn innermost_callee_env<H: Host>(cx: *mut LaneContext<H>) -> Option<Value> {
    // SAFETY: a frame names the caller-owned callee, whose descriptor outlives the run.
    unsafe { vm(cx) }.lane_stack.top_native().map(|frame| unsafe { (*frame.callee).target.env })
}

/// Continues the lane in the innermost native frame a run left inside: the frames become
/// the lane's own activations (their prefix no longer names a callee, and a skipped
/// forwarding constructor rides in its side record, `SidePayload::Forwarder`), and the context becomes the
/// innermost's. Its view and the body it ran, where the run left inside frames.
#[inline(always)]
pub(super) fn advance_into_frames<H: Host>(cx: *mut LaneContext<H>) -> Option<(&'static LaneView, &'static super::super::compiled::LaneMethod)> {
    // A run that left no native frame, the common completion, has nothing to link or advance.
    if !left_native_frames(cx) { return None; }
    advance_into_native_frames(cx)
}

/// Whether the run that just ended left native frames on the lane stack, linked or not.
#[inline(always)]
pub(super) fn left_native_frames<H: Host>(cx: *mut LaneContext<H>) -> bool {
    let stack = &unsafe { vm(cx) }.lane_stack;
    stack.top_is_native() || stack.native_depth() != 0
}

/// `advance_into_frames` past its test: the native frames to link and advance into.
#[inline(never)]
fn advance_into_native_frames<H: Host>(cx: *mut LaneContext<H>) -> Option<(&'static LaneView, &'static super::super::compiled::LaneMethod)> {
    let vm = unsafe { vm(cx) };
    vm.link_natives(cx);
    let (count, len) = (vm.lane_stack.native_depth(), vm.lane_stack.record_count());
    let mut innermost = None;
    for depth in len - count..len {
        let slot = vm.lane_stack.records_mut().slot(depth);
        // SAFETY: a linked frame holds its prefix; its callee outlives the run.
        let frame = unsafe { (*slot).native };
        // The leave inside the callee's body counts on its cell's word, which no handler entry notes.
        unsafe { &*(*frame.callee).target.view }.note_native_leave();
        unsafe { (*slot).native.callee = std::ptr::null() };
        // Its skipped constructor, its descriptor's until now, rides in its side record from
        // here on (`LaneStack::record_sided`); the frames are the innermost, outermost first.
        if let Some(skipped) = unsafe { &*frame.callee }.forwarder {
            vm.lane_stack.push_record_side(depth, SidePayload::Forwarder(skipped));
        }
        innermost = Some(frame);
    }
    let frame = innermost?;
    CallerState::of_frame(&frame).install(cx);
    // The body the call ran is the cell's at that moment, not the descriptor's; the caller's
    // code lease pins a named callee body through recovery, and the view pins every body it published.
    let callee = unsafe { &*frame.callee };
    Some(unsafe { (&*callee.target.view, &*(frame.body as *const super::super::compiled::LaneMethod)) })
}

/// Drops the native frames a terminated run left, with their windows.
pub(super) fn discard_frames<H: Host>(cx: *mut LaneContext<H>) {
    let vm = unsafe { vm(cx) };
    let (count, len) = (vm.lane_stack.native_depth(), vm.lane_stack.record_count());
    if count == 0 { return; }
    // SAFETY: the values hold no drop glue; native frames own no side record.
    unsafe {
        let window = (*vm.lane_stack.records_mut().slot(len - count)).window_word();
        vm.lane_stack.records_mut().set_len(len - count);
        vm.lane_stack.values.release_window(window);
    }
}
