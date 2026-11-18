//! The native frames of a lane body's direct calls, which live in the lane stack's own
//! activations. Emitted code pushes one by writing its prefix and `ActivationKind::Native`
//! (`compiled::DirectCallee`), through the stack's top word; the lane links it, once, when a
//! helper first observes it: a link writes the activation its call would have made from the
//! frames' descriptors alone, and never moves the context, so the body keeps running natively.
//! A leave inside the frames then continues the lane in the innermost (`advance_into_frames`).
use super::*;
use super::super::compiled::{ShadowFrame, LANE_CALL_LEFT, LANE_CALL_TERMINATED};

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
        if self.lane_stack.activations.top_is_native() { link_unlinked(cx); }
    }
}

#[cold]
#[inline(never)]
fn link_unlinked<H: Host>(cx: *mut LaneContext<H>) {
    let activations = &unsafe { vm(cx) }.lane_stack.activations;
    let (unlinked, len) = (activations.unlinked(), activations.len());
    for depth in len - unlinked..len { link_on_observe(cx, depth); }
}

/// Links the native frame at `depth`: the `Script` activation its call would have made. Its
/// caller is the frame below it where that one has a descriptor, else the running context,
/// which is the state of the activation the body runs in. A link never advances the context.
pub(super) fn link_on_observe<H: Host>(cx: *mut LaneContext<H>, depth: usize) {
    let vm = unsafe { vm(cx) };
    // SAFETY: each slot below the top holds at least its prefix.
    let below = depth.checked_sub(1).map(|below| unsafe { (*vm.lane_stack.activations.slot(below)).native }).filter(|frame| !frame.callee.is_null());
    let caller = below.map_or_else(|| CallerState::of_context(cx), |frame| CallerState::of_frame(&frame));
    let slot = vm.lane_stack.activations.slot(depth);
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
    let Some(frame) = vm.lane_stack.activations.top_native() else { return helper() };
    let owner = CallerState::of_context(cx);
    let depth = vm.lane_stack.activations.len();
    CallerState::of_frame(&frame).install(cx);
    let status = helper();
    if status == LANE_CALL_LEFT {
        // The lane continues above the frames, which return into the interpreter.
        vm.lane_stack.activations.retire_bodies(depth);
    } else if status != LANE_CALL_TERMINATED {
        owner.install(cx);
    }
    status
}

/// The environment a running body's captures resolve in when it runs as a native callee: its
/// closure's, which no lane activation holds; `None` in the activation's own.
pub(super) fn innermost_callee_env<H: Host>(cx: *mut LaneContext<H>) -> Option<Value> {
    // SAFETY: a frame names the caller-owned callee, whose descriptor outlives the run.
    unsafe { vm(cx) }.lane_stack.activations.top_native().map(|frame| unsafe { (*frame.callee).target.env })
}

/// Continues the lane in the innermost native frame a run left inside: the frames become
/// the lane's own activations (their prefix no longer names a callee, and a skipped
/// forwarding constructor joins `LaneStack::forwarders`), and the context becomes the
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
    let activations = &unsafe { vm(cx) }.lane_stack.activations;
    activations.top_is_native() || activations.native_depth() != 0
}

/// `advance_into_frames` past its test: the native frames to link and advance into.
#[inline(never)]
fn advance_into_native_frames<H: Host>(cx: *mut LaneContext<H>) -> Option<(&'static LaneView, &'static super::super::compiled::LaneMethod)> {
    let vm = unsafe { vm(cx) };
    vm.link_natives(cx);
    let (count, len) = (vm.lane_stack.activations.native_depth(), vm.lane_stack.activations.len());
    let mut innermost = None;
    for depth in len - count..len {
        let slot = vm.lane_stack.activations.slot(depth);
        // SAFETY: a linked frame holds its prefix; its callee outlives the run.
        let frame = unsafe { (*slot).native };
        if let Some(skipped) = unsafe { &*frame.callee }.forwarder { vm.lane_stack.forwarders.push(skipped); }
        // The leave inside the callee's body counts on its cell's word, which no handler entry notes.
        unsafe { &*(*frame.callee).target.view }.note_native_leave();
        unsafe { (*slot).native.callee = std::ptr::null() };
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
    let (count, len) = (vm.lane_stack.activations.native_depth(), vm.lane_stack.activations.len());
    if count == 0 { return; }
    // SAFETY: the values hold no drop glue.
    unsafe {
        let window = (*vm.lane_stack.activations.slot(len - count)).window_word();
        vm.lane_stack.activations.set_len(len - count);
        vm.lane_stack.values.release_window(window);
    }
}
