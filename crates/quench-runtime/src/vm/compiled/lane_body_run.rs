//! One run of a lane body: the words an entry binds in the thread's lane frame, and the call
//! through the body's entry word.

use super::*;

/// The activations a run's window has no pending push for: its caller pushed the body's own.
pub(in crate::vm) const NO_PENDING_ACTIVATIONS: usize = 0;
/// A body entered through its entry cell runs before the activation of its call exists
/// (`dispatch_fast::lane_enter_cell_body`): one, which a leave pushes in the room reserved past
/// the activations the run's frames start from.
pub(in crate::vm) const CALLEE_ACTIVATION_PENDING: usize = 1;

/// What one run of a lane body is given.
pub(in crate::vm) struct BodyRun<'a> {
    pub(in crate::vm) method: &'a LaneMethod,
    /// Whether the body may call bodies by their compiled entries (`lane_direct_calls`).
    pub(in crate::vm) direct_calls: bool,
    pub(in crate::vm) program: ProgramId,
    pub(in crate::vm) residual: &'a ResidualProgram,
    /// The registers of the function the body runs.
    pub(in crate::vm) registers: *mut Value,
    pub(in crate::vm) receiver: Value,
    pub(in crate::vm) captures: *const usize,
    pub(in crate::vm) lane_context: *mut (),
    /// The activations of the running function not pushed yet (`CALLEE_ACTIVATION_PENDING`).
    pub(in crate::vm) pending: usize,
}

impl<H: Host> Vm<H> {
    /// Runs `run`'s body; where it left, or that it returned.
    pub(in crate::vm) fn run_lane_body(&mut self, run: &BodyRun) -> LaneExit {
        let frame = lane_frame();
        #[cfg(test)]
        LANE_ENTRIES.with(|counts| counts.set((counts.get().0 + 1, counts.get().1)));
        // SAFETY: the activation's capture slots outlive its body's run, which collects nothing.
        let (capture_values, capture_len) = unsafe { crate::heap::EnvironmentSlots::native_storage_view(run.captures) }
            .unwrap_or((std::ptr::null(), 0));
        // SAFETY: the frame lives for the thread and is never moved once made; no reference
        // into it is held across the body's run, which may enter it again
        // (`run_nested_lane_method`).
        #[cfg(test)]
        let bound = unsafe { (*frame).run_words() };
        let context = unsafe { self.bind_body_entry(frame, run) };
        #[cfg(test)]
        LANE_RUN_REWRITES.with(|count| count.set(count.get() + usize::from(bound != unsafe { (*frame).run_words() })));
        let captures = entry_cell::capture_word(capture_values, capture_len);
        // SAFETY: the body was compiled for this function, never allocates or collects, and
        // reads only the registers, constants and heap bases bound above.
        let BodyReturn { status, value: word } = unsafe { self.call_body(run, context, captures) };
        if status == LANE_BODY_RETURNED {
            // SAFETY: as above; the body wrote the record before it returned.
            let pc = unsafe { (*frame).dispatch.returned_pc };
            return LaneExit::Returned { pc, value: Value::from_bits(word) };
        }
        Self::lane_method_exit(status, word)
    }

    /// The words an entry of `run` binds in the thread's lane frame: those of its run
    /// (`LaneFrame::bind_run`), its registers and its stack's room, past the `pending`
    /// activations. The frame's context.
    ///
    /// # Safety
    /// `frame` is the thread's lane frame; no reference into it is live.
    unsafe fn bind_body_entry(&mut self, frame: *mut LaneFrame, run: &BodyRun) -> *mut CallContext {
        // SAFETY: as the caller promises.
        unsafe {
            debug_assert!((*frame).scratch.len() >= run.method.scratch_width, "a body's publication sized the scratch window");
            let dispatch = &raw mut (*frame).dispatch;
            let bound = LaneRun {
                vm: std::ptr::from_ref(self).addr(),
                program_id: run.program,
                lane_context: run.lane_context.addr(),
                residual: std::ptr::from_ref(run.residual).addr(),
                stores_specialized: usize::from(self.specialized && run.residual.specialized),
            };
            (*frame).bind_run(bound, &self.execution_interrupt, || self.program_field_entries(run.program).unwrap_or(0));
            debug_assert_eq!((*dispatch).field_entries, self.program_field_entries(run.program).unwrap_or(0), "the run's field cache entries stand");
            (*dispatch).native_registers = run.registers;
            (*dispatch).refresh_object_bases_at_entry(self);
            let vm = self as *mut Self;
            (*dispatch).bind_lane_storage(vm);
            // A run starts with no native frames; its direct calls place their callees past
            // the stack's values.
            let (top, end, activations, shadow) = self.lane_stack_spare(run.pending);
            ((*dispatch).native_top, (*dispatch).native_end) = (top.addr(), end.addr());
            // A run whose lane may not make the frames' activations makes no direct call.
            let frames = if run.direct_calls { SHADOW_FRAMES.min(activations) } else { 0 };
            (*dispatch).shadow_base = shadow.addr();
            ((*dispatch).shadow_top, (*dispatch).shadow_end) = (shadow.addr(), shadow.add(frames).addr());
            &raw mut (*frame).context
        }
    }

    /// The call of `run`'s entry on the context `bind_body_entry` made, through the
    /// fault-resuming trampoline where the body polls by fault.
    ///
    /// # Safety
    /// As `enter_body`, for `run`'s body and the registers it names.
    #[inline(always)]
    unsafe fn call_body(&self, run: &BodyRun, context: *mut CallContext, captures: u64) -> BodyReturn {
        let (receiver, registers) = (run.receiver.to_bits(), run.registers.cast_const().cast());
        // SAFETY: the frame's context is the one this entry owns until it returns.
        let context = unsafe { &mut *context };
        if run.method.fault_poll {
            // SAFETY: as the caller promises.
            return unsafe { self.call_polling_body(run, context, receiver, captures, registers) };
        }
        // SAFETY: as the caller promises.
        unsafe { (run.method.entry)(context, receiver, captures, registers) }
    }

    /// `call_body` of a body that polls by fault: through the trampoline that resumes the fault,
    /// which lands in the entry that owns it. Out of line, so the other bodies' call is the
    /// entry's own.
    ///
    /// # Safety
    /// As `call_body`.
    #[inline(never)]
    unsafe fn call_polling_body(&self, run: &BodyRun, context: &mut CallContext, receiver: u64, captures: u64, registers: *const u64) -> BodyReturn {
        // SAFETY: this entry owns the TLS binding until it returns. Reentrant entries
        // checkpoint it in `run_nested_lane_method`; the run bound the rest of it.
        context.bind_poll_page(native_poll_page(&self.execution_interrupt));
        // SAFETY: as the caller promises.
        unsafe { enter_body(context, receiver, captures, registers, run.method.entry) }
    }
}
