//! One run of a lane body: the words an entry binds in the thread's lane frame, and the call
//! through the body's entry word.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::dispatch_fast::LaneStackWords;
use super::entry_protocol::{self, EntryDescriptor, EntryKind};
use super::*;
use quench_runtime::vm::tier_events::Event;

/// The activations a run's window has no pending push for: its caller pushed the body's own.
pub(crate) const NO_PENDING_ACTIVATIONS: usize = 0;
/// A body entered through its entry cell runs before the activation of its call exists
/// (`dispatch_fast::enter_cell_body_in_lane`): one, which a leave pushes in the room reserved past
/// the activations the run's frames start from.
pub(crate) const CALLEE_ACTIVATION_PENDING: usize = 1;

/// What one run of a lane body is given.
pub(crate) struct BodyRun<'a> {
    pub(crate) method: &'a LaneMethod,
    /// Whether the body may call bodies by their compiled entries (`direct_calls_in_lane`).
    pub(crate) direct_calls: bool,
    pub(crate) program: ProgramId,
    pub(crate) residual: &'a ResidualProgram,
    /// The registers of the function the body runs.
    pub(crate) registers: *mut Value,
    pub(crate) receiver: Value,
    pub(crate) captures: *const usize,
    pub(crate) lane_context: *mut (),
    /// The activations of the running function not pushed yet (`CALLEE_ACTIVATION_PENDING`).
    pub(crate) pending: usize,
    /// Whether the frame's heap bases are those a running body of the same run reads
    /// (`Vm::run_nested_lane_method`): nothing allocated since that body checked them, so the
    /// entry does not check them again.
    pub(crate) bases_stand: bool,
}

impl LaneStackWords {
    /// Binds the stack words of a lane run's direct calls into `dispatch` (`Vm::stack_words_in_lane`).
    pub(crate) fn bind(&self, dispatch: &mut JsStepDispatch) {
        dispatch.head.lane_frames_end = self.frames_end;
        dispatch.head.lane_values_top = self.values_top;
        dispatch.head.lane_values_end = self.values_end;
        dispatch.head.lane_activations_top = self.activations_top;
        dispatch.head.plain_depth = self.plain_depth;
        dispatch.head.plain_depth_limit = self.plain_depth_limit;
        dispatch.head.activation_count = self.activation_count;
        dispatch.head.body_table = self.body_table;
        dispatch.head.body_top = self.body_top;
        dispatch.head.native_table = self.native_table;
    }
}

#[quench_macros::ext(CompiledLaneBodyRunVm)]
impl Vm {
    /// Runs `run`'s body; where it left, or that it returned.
    pub(crate) fn run_lane_body(&mut self, run: &BodyRun) -> LaneExit {
        let frame = frame_in_lane();
        self.programs.tier_events().record(Event::Entered);
        // SAFETY: the activation's capture slots outlive its body's run, which collects nothing.
        let (view, stores) = unsafe { crate::dispatch_fast::capture_view(run.captures) };
        let (capture_values, capture_len) = view.unwrap_or((std::ptr::null(), 0));
        // SAFETY: the frame lives for the thread and is never moved once made; no reference
        // into it is held across the body's run, which may enter it again
        // (`run_nested_lane_method`).
        let outer_locals = unsafe { let dispatch = &(*frame).dispatch; (dispatch.native_locals, dispatch.local_count, dispatch.environment_storage) };
        let context = unsafe { self.bind_body_entry(frame, run) };
        let captures = entry_cell::capture_word(capture_values, capture_len, stores);
        // A stencil body (`entry_body`) reads its captures word from its window's header, which whoever
        // enters it writes; a Cranelift body takes it as the entry's word. The activation's receiver is in
        // the header already (an arrow's lexical one included), which the entry's receiver word is not.
        if run.method.framed() {
            // SAFETY: a framed body runs in a lane window, whose context words precede register 0.
            unsafe { crate::dispatch_fast::set_frame_word(run.registers, quench_runtime::staged::frame::CONTEXT_BODY, captures) };
        }
        let descriptor = EntryDescriptor { kind: EntryKind::Body, receiver: run.receiver.to_bits(), captures, registers: run.registers.cast_const().cast(), entry: run.method.entry };
        // SAFETY: the body was compiled for this function, never allocates or collects, and
        // reads only the registers, constants and heap bases bound above.
        let returned = unsafe { self.call_body(run, &descriptor, context) };
        // A body re-reads its locals' address after each call-out, and a run its call-outs make
        // binds its own: the run it returns into gets back the address it entered with.
        unsafe { let dispatch = &mut (*frame).dispatch; (dispatch.native_locals, dispatch.local_count, dispatch.environment_storage) = outer_locals };
        // SAFETY: as above; a completing body wrote the record before it returned.
        let exit = entry_protocol::exit(&descriptor, returned, || unsafe { (*frame).dispatch.head.returned_pc });
        self.programs.tier_events().record(Event::ended(exit));
        if let LaneExit::Returned { .. } = exit { self.programs.tier_events().record(Event::Returned); }
        exit
    }

    /// The words an entry of `run` binds in the thread's lane frame: those of its run
    /// (`BodyRunContext::bind_run`), its registers and its stack's room, past the `pending`
    /// activations. The frame's context.
    ///
    /// # Safety
    /// `frame` is the thread's lane frame; no reference into it is live.
    unsafe fn bind_body_entry(&mut self, frame: *mut BodyRunContext, run: &BodyRun) -> *mut CallContext {
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
            // A run's field cache entries are rebound when the table moves
            // (`Vm::rebind_lane_field_entries`), not per entry: `bind_run` binds them once per
            // run, and the frame's word follows the table from then on.
            if (*frame).bind_run(bound, &self.execution_interrupt, || self.program_field_entries(run.program).unwrap_or(0)) {
                self.programs.tier_events().record(Event::RunBound);
            }
            debug_assert_eq!((*dispatch).head.field_entries, self.program_field_entries(run.program).unwrap_or(0), "the run's field cache entries stand");
            (*dispatch).native_registers = run.registers;
            let locals = crate::dispatch_fast::body_locals(run.lane_context, run.registers, crate::dispatch_fast::LocalsOf::Body);
            ((*dispatch).head.native_locals, (*dispatch).head.local_count, (*dispatch).head.environment_storage) = (locals.locals, locals.count, locals.environment);
            if !run.bases_stand { (*dispatch).refresh_object_bases_at_entry(self); }
            let vm = self as *mut Self;
            (*dispatch).bind_lane_storage(vm);
            // A run starts with no native frames; its direct calls push their callees' windows and
            // frames onto the lane stack's own tops, which the dispatch points at. A body entered
            // through its cell has an activation pending, which a leave pushes first: it makes no
            // direct call, the frames of which would sit above that activation's room.
            let words = self.stack_words_in_lane(if run.direct_calls && run.pending == NO_PENDING_ACTIVATIONS { SHADOW_FRAMES } else { 0 });
            // A b-jit body's direct call rows push whether or not the run names direct calls (`run.direct_calls` sizes
            // only the shadow frames), so in a leaf's window they refuse to the helper row.
            crate::dispatch_fast::refuse_direct_calls_in_leaf(run.lane_context, run.registers);
            words.bind(&mut *dispatch);
            &raw mut (*frame).context
        }
    }

    /// The call of `descriptor`'s entry on the context `bind_body_entry` made, through the
    /// fault-resuming trampoline where the body polls by fault.
    ///
    /// # Safety
    /// As `enter_body`, for `run`'s body and the registers `descriptor` names.
    #[inline(always)]
    unsafe fn call_body(&self, run: &BodyRun, descriptor: &EntryDescriptor, context: *mut CallContext) -> BodyReturn {
        // SAFETY: the frame's context is the one this entry owns until it returns.
        let context = unsafe { &mut *context };
        if run.method.fault_poll {
            // SAFETY: as the caller promises.
            return unsafe { self.call_polling_body(context, descriptor) };
        }
        // SAFETY: as the caller promises.
        unsafe { (descriptor.entry)(context, descriptor.receiver, descriptor.captures, descriptor.registers) }
    }

    /// `call_body` of a body that polls by fault: through the trampoline that resumes the fault,
    /// which lands in the entry that owns it. Out of line, so the other bodies' call is the
    /// entry's own.
    ///
    /// # Safety
    /// As `call_body`.
    #[inline(never)]
    unsafe fn call_polling_body(&self, context: &mut CallContext, descriptor: &EntryDescriptor) -> BodyReturn {
        // SAFETY: this entry owns the TLS binding until it returns. Reentrant entries
        // checkpoint it in `run_nested_lane_method`; the run bound the rest of it.
        context.bind_poll_page(native_poll_page(&self.execution_interrupt));
        // SAFETY: as the caller promises.
        unsafe { entry_protocol::enter(context, descriptor) }
    }
}
