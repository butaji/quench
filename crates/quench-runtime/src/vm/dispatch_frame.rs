#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationOperationRecordsExt, OperationNamesExt};
use super::*;
use crate::staged::frame_arm::{FrameArm, GeneralArm};
mod loop_stack;
mod interpreter_frame;
pub(super) use interpreter_frame::{InterpreterFrame, RowRun};
mod materialized_stops;

/// Values a pooled frame's buffers keep across activations; larger buffers are shrunk.
const RETAINED_FRAME_VALUES: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cursors_leave_interpreter_decoding_cold() {
        let program = crate::Engine::specialize(
            "var n = 3; while (n > 0) n--; throw n;", "native-cursor.js",
        ).unwrap();
        let native = crate::NativeJsProgram::publish(program.clone()).unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(0.0)));
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let cursor = vm.general_cursor(ProgramId::MAIN, native.residual(), 0, true);
        assert!(cursor.decoded.is_null(), "linked code must not expand interpreter instructions");
        for mode in [ExecutionMode::NativeOnly, ExecutionMode::Compiled] {
            vm.set_execution_mode(mode);
            let cursor = vm.general_cursor(ProgramId::MAIN, native.residual(), 0, true);
            assert!(cursor.lane.is_null() && !cursor.back_edges,
                "linked code must not prepare an interpreter lane");
        }

        let mut interpreter = Vm::new(crate::test_host::TestHost::default());
        interpreter.initialize_for_snapshot(&program).unwrap();
        let cursor = interpreter.general_cursor(ProgramId::MAIN, &program, 0, false);
        assert!(!cursor.decoded.is_null(), "an interpreted function expands its dispatch cache on its first entry");
        let error = interpreter.execute(&program).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(0.0)));
    }
}

#[derive(Clone, Copy)]
pub struct GeneralCodeCursor {
    program: ProgramId,
    pub(super) function: u32,
    code: *const Instr,
    wide: *const WideInstruction,
    /// Fixed-width expansion of `code`; null while the function is cold.
    decoded: *const WideInstruction,
    /// The function's lane view, or null where the lane does not run.
    pub(super) lane: *const crate::vm::engine_types::LaneView,
    /// Whether a view-less JavaScript activation may still gain its function's view at a back edge
    /// (`note_back_edge`); false once it has.
    back_edges: bool,
}

#[cfg(not(stencil_build))] #[allow(unused_imports)] use crate::staged::glue_prelude::*;
/// A caller suspended by an in-loop frame push, resumed when its callee completes.
pub struct PendingGeneralCall {
    pub caller: usize,
    /// The caller's cursor, still valid on resume: the residual outlives the loop and a hot
    /// decoding, once built, never moves; a cold caller simply keeps reading packed code.
    /// Lane calls leave it to the general loop, which derives it only if it resumes the caller.
    pub caller_cursor: Option<GeneralCodeCursor>,
    pub call_pc: u32,
    pub destination: Register,
    /// Constructor substitution, promise settlement or generator resumption;
    /// ordinary calls yield the returned value directly.
    pub completion: CallCompletion,
    /// The call falls into a return of its result: the caller completes with the callee's
    /// value instead of storing it. Sloppy callers keep their frame (it stays visible to
    /// legacy `caller` reflection), so this chains returns rather than replacing frames.
    pub returns: bool,
    /// `None` only for a lane activation materialized past the guest-call budget, which
    /// `WASM_ACTIVATION_LIMIT` bounds instead.
    pub call_guard: Option<crate::stack::GuestCallGuard>,
    /// The caller's program when the callee belongs to another one: the return or unwind
    /// into the caller makes it executing again. `None` when both share their program.
    pub caller_program: Option<SavedProgram>,
}

/// Already materialized calls, adopted by the ordinary return/unwind loop before resuming.
pub struct RecoveredCalls {
    pub(super) calls: Vec<PendingGeneralCall>,
    pub(super) failure: Option<(u32, JsError)>,
}

/// Where a lane run that no general loop started left its frames (`Vm::native_callback_in_lane`):
/// the program the innermost executes and the error a call the lane made raised there, which
/// the general loop resumes with as it does after its own lane run.
pub struct LaneHandoff {
    pub executing: ExecutingProgram,
    pub raised: Option<JsError>,
}

/// Where a general loop resumes its entry frame other than at its PC: after a lane run
/// (`LaneHandoff`), or with the transition its linked native body already made there
/// (`Vm::complete_pushed_call`), which the loop consumes as if its own entry had made it.
pub(super) enum GeneralResume {
    Lane(LaneHandoff),
    #[cfg(feature = "native-execution")]
    Native(crate::vm::engine_types::JsNativeStep),
}

/// The general loop's executing program: its id and the residual keeping it alive (`None`
/// for the loop's entry program).
pub struct ExecutingProgram {
    pub id: ProgramId,
    pub retained: Option<Rc<ResidualProgram>>,
}

impl ExecutingProgram {
    /// The residual the executing program runs: its retained residual, else the loop's entry.
    #[inline(always)]
    fn residual<'a>(&'a self, entry_program: &'a ResidualProgram) -> &'a ResidualProgram {
        self.retained.as_deref().unwrap_or(entry_program)
    }
}

impl Frame {
    /// Sets the fields every activation places: its function, program, entry PC, environment,
    /// receiver and `with` base. The caller sets its context and `fixed_this`.
    #[inline(always)]
    fn place(&mut self, id: u32, program: ProgramId, env: Value, this: Value, with_base: usize) {
        self.function = id;
        self.program = program;
        self.pc = 0;
        self.pc_is_resume = true;
        self.binding_site_pc = None;
        self.env = env;
        self.this = this;
        self.captured = false;
        self.with_base = with_base;
    }
}

/// Whether an activation's `this` is fixed on its frame (`Frame::fixed_this`): the activation binds its
/// own receiver (`inherits_this` is false: an arrow or an eval reads a lexical `this` instead), and the
/// receiver is initialized (a derived constructor's stays deleted until `super()`). Every frame's
/// `fixed_this` is derived through this one predicate.
#[inline(always)]
pub(super) fn this_is_fixed(inherits_this: bool, this: Value) -> bool {
    !inherits_this && !this.is_deleted()
}

/// An executing program set aside while an in-loop callee of another program runs.
pub struct SavedProgram {
    pub id: ProgramId,
    /// The residual that kept it alive, `None` for the loop's entry program.
    pub retained: Option<Rc<ResidualProgram>>,
}

/// Activations the dispatch loop holds before a Wasm call exhausts the call
/// stack. A Wasm call in the loop pushes a heap frame instead of recursing
/// natively, so the native stack guard does not bound it; the guest call
/// depth does.
pub const WASM_ACTIVATION_LIMIT: usize = crate::stack::MAX_GUEST_CALL_DEPTH;

impl GeneralCodeCursor {
    #[inline(always)]
    fn instruction(self, pc: usize) -> WideInstruction {
        // SAFETY: residual validation establishes every reachable PC and wide
        // operand index, and `decoded` has one entry per code word. This cursor stays local
        // to `run_frame_general_until`: the borrowed entry residual remains alive for the
        // whole call, and a switched residual is retained by `current_program` until the
        // cursor is refreshed. Inline frame pushes are restricted to the same program. The
        // loop refreshes the cursor whenever the active frame changes.
        if !self.decoded.is_null() {
            return unsafe { *self.decoded.add(pc) };
        }
        let packed = unsafe { *self.code.add(pc) };
        if packed.is_wide() {
            // SAFETY: the same validation guarantees this packed wide index is
            // within the current function's wide-instruction table.
            unsafe { *self.wide.add(packed.wide_index()) }
        } else {
            packed.as_wide()
        }
    }
}

impl Vm {
    /// A code cursor for `function`, with its lane view where the lane runs.
    pub(super) fn general_cursor(
        &self,
        program_id: ProgramId,
        program: &ResidualProgram,
        function: u32,
        lane: bool,
    ) -> GeneralCodeCursor {
        let function_code = program.function(function as usize);
        let interprets = self.execution_mode.allows_dynamic_execution();
        let admitted = lane && self.admits_in_lane(program_id, program);
        let view = if admitted { self.entry_lane_view(program_id, function) } else { std::ptr::null() };
        // Native transitions read only boundary metadata from packed code;
        // they must not warm or allocate an interpreter dispatch expansion.
        // A Wasm function its lane runs leaves the lane only at a rare operator: the general
        // loop reads its packed code there, without a second, expanded copy.
        let lane_runs_wasm = !view.is_null() && program.kind == crate::bytecode::ProgramKind::Wasm;
        let decoded = if interprets && !lane_runs_wasm {
            function_code.decoded.on_entry(&function_code.code, &function_code.wide).as_ptr()
        } else {
            std::ptr::null()
        };
        GeneralCodeCursor {
            program: program_id,
            function,
            code: function_code.code.as_ptr(),
            wide: function_code.wide.as_ptr(),
            decoded,
            lane: view,
            // A JavaScript activation deferred its view; a back edge derives it where the function's predicted tier compiles it.
            back_edges: admitted && view.is_null() && program.kind != crate::bytecode::ProgramKind::Wasm && self.programs.compiled_tier(program_id, function),
        }
    }

    /// Takes a backward jump of `cursor`'s activation in the general path: a view-less activation
    /// derives its function's lane view now, which the lane enters at the loop's next instruction.
    #[inline(always)]
    fn note_back_edge(&self, cursor: &mut GeneralCodeCursor) {
        if std::mem::take(&mut cursor.back_edges) {
            cursor.lane = self.hot_loop_lane_view(cursor.program, cursor.function);
        }
    }

    pub fn call_this_value(&mut self, this: Value, strict: bool) -> Result<Value, JsError> {
        if strict {
            Ok(this)
        } else if this.is_null() || this.is_undefined() {
            Ok(self.realm.globals)
        } else {
            self.box_object(this)
        }
    }

    fn take_construction_target(&mut self, context: CallContext) -> Option<Value> {
        let target = self.construct_target?;
        if context.callee() != Some(target.callee) {
            return None;
        }
        self.set_construct_target(None);
        Some(target.new_target)
    }

    #[inline(always)]
    pub(super) fn initialize_activation_bindings(
        &mut self,
        frame: &mut Frame,
        arrow: bool,
        new_target: Value,
        observable: bool,
    ) {
        if !arrow && !new_target.is_undefined() {
            frame.context = frame.context.constructed();
        }
        if !observable {
            // Nothing reads the implicit bindings, and the frame's own `this` is authoritative.
            frame.fixed_this = this_is_fixed(arrow, frame.this);
            return;
        }
        if !arrow && frame.function != super::ROOT_FUNCTION_ID {
            let atom = self.runtime_atoms.new_target;
            frame.dynamic_bindings.push((atom, new_target));
        }
        let inherits_this = arrow
            || (frame.function == super::ROOT_FUNCTION_ID
                && !frame.env.is_null()
                && self
                    .programs
                    .get(frame.program)
                    .is_some_and(|program| program.kind == crate::bytecode::ProgramKind::Eval));
        if !inherits_this {
            let atom = self.runtime_atoms.lexical_this;
            frame.dynamic_bindings.push((atom, frame.this));
        }
        frame.fixed_this = this_is_fixed(inherits_this, frame.this);
    }

    #[inline(never)]
    pub(super) fn call_user(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Value, JsError> {
        self.call_user_frame(p, id, parent, this, args, context)?.into_value()
    }

    pub(super) fn call_user_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<FrameOutcome, JsError> {
        #[cfg(feature = "profile-lane-exits")]
        if let Some(caller) = self.frames.last() {
            super::lane_exits::call_target(
                self.active_program.raw(),
                caller.function,
                caller.pc as u32,
                context.callee().unwrap_or(Value::UNDEFINED),
                id,
            );
        }
        self.run_user_frame(p, id, parent, this, args, context, false)
    }

    pub(super) fn call_user_construct_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<FrameOutcome, JsError> {
        self.run_user_frame(p, id, parent, Value::UNDEFINED, args, context, true)
    }

    /// Activate a Wasm function in the running dispatch loop. Wasm bodies
    /// observe only their locals and registers: no receiver, arguments
    /// object, dynamic binding or `with` scope belongs to the activation.
    /// A known call stays within its instance, and a Wasm frame is never
    /// captured nor enters a `with` scope, so the callee's environment is
    /// the caller's: the instance environment.
    #[inline(always)]
    pub fn push_wasm_frame(
        &mut self,
        p: &ResidualProgram,
        caller: usize,
        id: u32,
        arguments: crate::bytecode::RegisterWindow,
    ) -> Result<crate::stack::GuestCallGuard, JsError> {
        let caller = &self.frames[caller];
        let arguments_base = usize::from(arguments.base);
        let arguments: *const [Value] =
            &caller.registers[arguments_base..arguments_base + usize::from(arguments.count)];
        let env = caller.env;
        // SAFETY: the caller's registers stay in place while the callee's frame is filled.
        self.push_wasm_frame_over(p, id, env, unsafe { &*arguments })
    }

    /// Activate Wasm function `id` of the active program `p` over instance environment
    /// `env`, passed the encoded `arguments`: the frame a Wasm call pushes
    /// (`push_wasm_frame`), or a JavaScript call of an exported function
    /// (`dispatch_fast::lane_wasm_export_frame`).
    #[inline(always)]
    pub fn push_wasm_frame_over(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        arguments: &[Value],
    ) -> Result<crate::stack::GuestCallGuard, JsError> {
        // An in-loop activation grows the frame stack, not the native stack, so it counts
        // against the live guest-call budget.
        let function = &p.function(id as usize);
        let frame_storage_bytes = (usize::from(function.locals)
            + usize::from(function.registers))
        .saturating_mul(std::mem::size_of::<Value>());
        let call_guard = crate::stack::GuestCallGuard::enter_with_storage(frame_storage_bytes)
            .map_err(|crate::stack::StackExhausted| JsError::wasm_trap_error(crate::WasmTrap::CallStackExhausted))?;
        let program = self.active_program;
        let with_base = self.with_stack.len();
        let mut frame = self.pooled_frame();
        // Each slot is written once: parameters land where the function
        // keeps them, every other local and register takes its default.
        let local_arguments = &arguments[..arguments
            .len()
            .min(usize::from(function.local_parameter_count()))];
        frame.locals.clear();
        if function.locals != 0 {
            frame.locals.extend_from_slice(local_arguments);
            frame
                .locals
                .resize(usize::from(function.locals), Value::UNDEFINED);
        }
        frame.prepare_wasm_registers(function, arguments);
        frame.place(id, program, env, Value::UNDEFINED, with_base);
        frame.context = CallContext::Internal;
        frame.fixed_this = false;
        self.frames.push(frame);
        Ok(call_guard)
    }

    /// Activate a Wasm function whose registers already hold `registers`:
    /// a lane activation becoming a frame. Its environment is the caller's.
    pub fn push_wasm_frame_with_registers(
        &mut self,
        p: &ResidualProgram,
        env: Value,
        id: u32,
        registers: &[Value],
    ) -> crate::stack::GuestCallGuard {
        let function = p.function(id as usize);
        debug_assert_eq!(function.locals, 0);
        let frame_storage_bytes = usize::from(function.registers)
            .saturating_mul(std::mem::size_of::<Value>());
        // Lane admission reserves these bytes before it stores the activation. Materializing
        // the lane transfers that reservation to the frame's weighted guest-call guard.
        let call_guard = crate::stack::GuestCallGuard::enter_with_storage(frame_storage_bytes)
            .expect("lane Wasm activation storage was admitted before materialization");
        let program = self.active_program;
        let with_base = self.with_stack.len();
        let mut frame = self.pooled_frame();
        frame.locals.clear();
        frame.registers.clear();
        frame.registers.extend_from_slice(registers);
        frame.initialized_registers = frame.initialized_registers.max(registers.len());
        frame.place(id, program, env, Value::UNDEFINED, with_base);
        frame.context = CallContext::Internal;
        frame.fixed_this = false;
        self.frames.push(frame);
        call_guard
    }

    pub(super) fn empty_frame(&self) -> Box<Frame> {
        Box::new(Frame::empty(self.active_program, self.with_stack.len()))
    }

    /// Starts an activation that the dispatch loop runs in place.
    ///
    /// `args` may borrow the caller's register window: setup only pushes the new activation,
    /// never touching or resizing another frame's register buffer, and frame-stack growth moves
    /// the boxed frames' pointers rather than their buffers.
    pub fn push_general_user_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<crate::stack::GuestCallGuard, JsError> {
        self.activate_user_frame(p, id, parent, this, args, context)?;
        // An in-loop activation grows the frame stack, not the native stack, so it counts
        // against the live guest-call budget rather than a native transition.
        crate::stack::GuestCallGuard::enter().map_err(|crate::stack::StackExhausted| self.unwind_exhausted_push(p))
    }

    /// Pops an in-loop activation the guest-call budget refused and yields the error.
    #[cold]
    #[inline(never)]
    fn unwind_exhausted_push(&mut self, p: &ResidualProgram) -> JsError {
        let outcome = Err(self.guest_exhaustion_error(p.kind));
        let mut frame = self.frames.pop().unwrap();
        self.deactivate_frame(&mut frame, &outcome);
        self.pool_frame(frame);
        match outcome {
            Err(error) => error,
            Ok(_) => unreachable!(),
        }
    }

    /// Runs a user activation to completion on the native stack.
    fn run_user_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
        capture_constructor_this: bool,
    ) -> Result<FrameOutcome, JsError> {
        // Some VM entry paths (notably host callbacks and nested eval) still
        // execute a user function by recursively entering `run_user_frame`
        // instead of pushing it onto the active dispatch loop. Count those
        // activations against the same guest budget so a small Node
        // `--stack_size` cannot exhaust the native worker stack first.
        let _call_guard =
            crate::stack::GuestCallGuard::enter().map_err(|crate::stack::StackExhausted| self.guest_exhaustion_error(p.kind))?;
        let frame_index = self.activate_user_frame(p, id, parent, this, args, context)?;
        let view = if capture_constructor_this { std::ptr::null() } else { self.entered_lane_view(p, id) };
        let result = if Self::runs_call_in_lane(view) {
            let cursor = self.entry_cursor(p, id, view);
            self.run_frame_from_lane(p, frame_index, &cursor, 0)
        } else {
            self.run_frame_general(p, frame_index)
        };
        self.complete_user_frame(result, capture_constructor_this)
    }

    /// The cursor of a call of function `id` of the active program `p` that enters `view`, a
    /// lane that runs it, at its first instruction.
    pub(super) fn entry_cursor(&self, p: &ResidualProgram, id: u32, view: *const crate::vm::engine_types::LaneView) -> GeneralCodeCursor {
        let body = p.function(id as usize);
        GeneralCodeCursor {
            program: self.active_program,
            function: id,
            code: body.code.as_ptr(),
            wide: body.wide.as_ptr(),
            decoded: if !view.is_null() && p.kind == crate::bytecode::ProgramKind::Wasm {
                std::ptr::null()
            } else {
                body.decoded.on_entry(&body.code, &body.wide).as_ptr()
            },
            lane: view,
            back_edges: false,
        }
    }

    /// The lane view a call of function `id` of the active program `p` enters at its first
    /// instruction from a native (a promise job, a host callback), as a lane call of it would
    /// activate it (`Vm::resolve_script_call_target`): derived once its deferred runs complete
    /// (`ProgramStore::call_may_derive_in_lane`); null where the general loop runs it.
    pub(super) fn entered_lane_view(&self, p: &ResidualProgram, id: u32) -> *const crate::vm::engine_types::LaneView {
        let program = self.active_program;
        let runs_in_lane = (Self::FRAME_ENTRY_LANE_RUNS || self.programs.ahead_images(program, p).is_some())
            && !self.tier_may_run(crate::vm::tier_policy::Tier::ANY);
        if !runs_in_lane
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || !self.admits_in_lane(program, p)
            || !self.programs.call_may_derive_in_lane(program, id)
        {
            return std::ptr::null();
        }
        self.view_any_in_lane(program, id).unwrap_or(std::ptr::null())
    }

    /// Retire the innermost frame, a user activation that ran to `result` on the native
    /// stack, and yield its outcome; a constructor's with its receiver when
    /// `capture_constructor_this`.
    #[inline(always)]
    pub fn complete_user_frame(
        &mut self,
        result: Result<FrameOutcome, JsError>,
        capture_constructor_this: bool,
    ) -> Result<FrameOutcome, JsError> {
        // A captured run took its frames into its chain (`capture_chain_parked`): none is left to retire.
        if result.as_ref().is_err_and(JsError::is_captured) {
            return result;
        }
        let mut frame = self.frames.pop().unwrap();
        self.deactivate_frame(&mut frame, &result);
        match result? {
            FrameOutcome::Complete(value) => {
                let outcome = if capture_constructor_this {
                    FrameOutcome::ConstructComplete {
                        value,
                        this: frame.this,
                    }
                } else {
                    FrameOutcome::Complete(value)
                };
                self.pool_frame(frame);
                Ok(outcome)
            }
            FrameOutcome::ConstructComplete { .. } => {
                self.pool_frame(frame);
                Err(JsError(
                    "nested constructor completion escaped its activation".into(),
                ))
            }
            FrameOutcome::Suspend(suspended @ Suspended::Await { .. }, _) => {
                Ok(FrameOutcome::Suspend(suspended, Some(FrameRecord::capture(frame))))
            }
            FrameOutcome::Suspend(Suspended::Yield { .. }, _) => {
                Err(JsError("yield requires generator continuation".into()))
            }
            FrameOutcome::ParameterInitializationComplete => Err(JsError(
                "unexpected generator parameter initialization boundary".into(),
            )),
        }
    }

    /// Binds a user function's activation and pushes it on the frame stack, returning its index.
    #[inline(always)]
    pub(super) fn activate_user_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<usize, JsError> {
        self.promote_tier0_if_due(p, id);
        let function = &p.function(id as usize);
        if id != super::ROOT_FUNCTION_ID && function.plain_activation(p) {
            self.activate_plain_frame(p, id, function, parent, this, args, context)
        } else {
            self.activate_general_frame(p, id, parent, this, args, context)
        }
    }

    /// The frame of an activation of `id` bound and left unrun, off the frame stack, as a suspension
    /// at its first instruction holds it: what an owner keeps to start in a lane run
    /// (`Vm::start_root_job`).
    pub(super) fn unstarted_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        context: CallContext,
    ) -> Result<Box<FrameRecord>, JsError> {
        self.unstarted_frame_with(p, id, parent, this, &[], context)
    }

    /// `unstarted_frame` bound with the values `args` passed.
    pub(super) fn unstarted_frame_with(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Box<FrameRecord>, JsError> {
        self.activate_user_frame(p, id, parent, this, args, context)?;
        let mut frame = self.frames.pop().expect("the activation pushed a frame");
        frame.with_objects = self.with_stack.split_off(frame.with_base);
        Ok(FrameRecord::capture(frame))
    }

    /// `activate_user_frame` for a function whose static layout describes its whole
    /// activation (`Function::plain_activation`): every local starts undefined because every
    /// parameter lives in a register, `this` is the coerced receiver or an arrow's lexical
    /// one, and no implicit binding, rest array or `arguments` object exists.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    /// The `this` a plain activation of `function` binds: an arrow's from its environment
    /// `parent` (the receiver where none is bound), uncoerced as every arrow frame's is, else
    /// the coerced receiver.
    pub(super) fn lexical_activation_this(
        &mut self,
        function: &crate::bytecode::Function,
        parent: Value,
        this: Value,
    ) -> Result<Value, JsError> {
        let captured_this = if function.is_arrow { self.captured_lexical_this(parent) } else { None };
        self.activation_receiver(function, captured_this, this)
    }

    /// The receiver an activation of `function` binds: uninitialized for a derived constructor,
    /// an arrow's captured `this` (uncoerced, the receiver where none is captured), else the
    /// receiver as a call coerces it.
    #[inline(always)]
    fn activation_receiver(
        &mut self,
        function: &crate::bytecode::Function,
        captured_this: Option<Value>,
        this: Value,
    ) -> Result<Value, JsError> {
        if function.derived_constructor {
            Ok(Value::DELETED)
        } else if function.is_arrow {
            Ok(captured_this.unwrap_or(this))
        } else {
            self.call_this_value(this, function.strict)
        }
    }

    fn activate_plain_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        function: &crate::bytecode::Function,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<usize, JsError> {
        self.profile
            .function(self.active_program.raw(), id as usize);
        let this = self.lexical_activation_this(function, parent, this)?;
        let mut frame = self.pooled_frame();
        let local_count = if function.call_plan(p).addresses_locals {
            usize::from(function.locals)
        } else {
            0
        };
        frame.reset_locals_undefined(local_count);
        self.initialize_frame_invocation(p, &mut frame, function, context, args);
        self.place_plain_frame(&mut frame, id, (parent, this));
        self.push_inherited_with_scopes(parent, function, p.kind);
        // A `new.target` set for this activation has no observable binding to land in.
        if self.take_construction_target(context).is_some() {
            frame.context = frame.context.constructed();
        }
        frame.prepare_registers_for(function);
        frame.place_plain_parameters(function, args);
        self.frames.push(frame);
        Ok(self.frames.len() - 1)
    }

    /// The frame a frameless lane activation of `id` stands for (`ScriptActivation`): the
    /// activation its call would have pushed with the same callee, receiver and `passed`
    /// arguments, holding the activation's current registers.
    #[allow(clippy::too_many_arguments)]
    pub fn push_script_frame_with_registers(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        this: Value,
        context: CallContext,
        passed: &[Value],
        registers: &[Value],
    ) {
        #[cfg(test)]
        super::activation::note_frame_made();
        // The receiver is already coerced and the body binds no `arguments` object, so
        // activation cannot fail; the registers below replace what it placed in them.
        self.activate_user_frame(p, id, env, this, passed, context)
            .expect("a frameless activation's frame activates without error");
        let frame = self.frames.last_mut().expect("the activation pushed a frame");
        frame.registers.copy_from_slice(registers);
    }

    /// The frame `push_script_frame_with_registers` would push for a plain activation of `id`
    /// (`Function::plain_activation`), made off the frame stack as `unstarted_frame_with`
    /// detaches one: bound as `activate_plain_frame` binds it, with the `with` objects it
    /// would push held by the frame, and `registers` moved into its file once rather than
    /// over the values activation filled and placed. `None` for any other function, which
    /// takes the push.
    pub fn detached_plain_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        (env, this): (Value, Value),
        context: CallContext,
        (passed, registers): (&[Value], &[Value]),
    ) -> Option<Box<Frame>> {
        let function = &p.function(id as usize);
        if id == super::ROOT_FUNCTION_ID || !function.plain_activation(p) {
            return None;
        }
        #[cfg(test)]
        super::activation::note_frame_made();
        self.promote_tier0_if_due(p, id);
        self.profile.function(self.active_program.raw(), id as usize);
        // The receiver is already coerced, so binding it cannot fail.
        let this = self.lexical_activation_this(function, env, this).expect("a frameless activation's receiver binds");
        let mut frame = self.pooled_frame();
        let local_count = if function.call_plan(p).addresses_locals { usize::from(function.locals) } else { 0 };
        frame.reset_locals_undefined(local_count);
        self.initialize_frame_invocation(p, &mut frame, function, context, passed);
        self.place_plain_frame(&mut frame, id, (env, this));
        frame.with_objects = self.captured_with_objects_for_function(env, function.inherited_with_scope, p.kind);
        // A `new.target` set for this activation has no observable binding to land in.
        if self.take_construction_target(context).is_some() {
            frame.context = frame.context.constructed();
        }
        frame.adopt_registers(function, registers);
        Some(frame)
    }

    /// Places a plain activation's frame of `id` at its function's start in the active program,
    /// with environment `env` and bound receiver `this`: the placement of `activate_plain_frame`
    /// and of a detached frame (`detached_plain_frame`).
    fn place_plain_frame(&self, frame: &mut Frame, id: u32, (env, this): (Value, Value)) {
        frame.place(id, self.active_program, env, this, self.with_stack.len());
        frame.fixed_this = this_is_fixed(false, this);
    }

    /// A frame from the pool, or a fresh one; its fields are set by activation.
    pub(super) fn pooled_frame(&mut self) -> Box<Frame> {
        self.frame_pool.pop().unwrap_or_else(|| self.empty_frame())
    }

    #[inline(never)]
    fn activate_general_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<usize, JsError> {
        self.profile
            .function(self.active_program.raw(), id as usize);
        let function = &p.function(id as usize);
        let mut frame = self.pooled_frame();
        self.bind_activation_arguments(p, &mut frame, function, context, args)?;
        let captured_this = if function.is_arrow { self.captured_lexical_this(parent) } else { None };
        let this = self.activation_receiver(function, captured_this, this)?;
        frame.place(id, self.active_program, parent, this, self.with_stack.len());
        if id == super::ROOT_FUNCTION_ID
            && self.programs.is_module(frame.program)
            && let Some(environment) = self.programs.module_environment(frame.program)
        {
            frame.env = environment;
            frame.captured = true;
        }
        self.push_inherited_with_scopes(parent, function, p.kind);
        if id == super::ROOT_FUNCTION_ID {
            self.bind_module_imports(&mut frame)?;
        }
        let observable = id == super::ROOT_FUNCTION_ID || function.activation_bindings_observable(p);
        self.bind_activation_registers(&mut frame, function, context, args, captured_this, observable);
        self.frames.push(frame);
        let frame_index = self.frames.len() - 1;
        if id == super::ROOT_FUNCTION_ID
            && self.programs.is_module(self.frames.last().unwrap().program)
        {
            let environment = self.promote_frame_environment(frame_index);
            self.programs
                .set_module_environment(self.frames[frame_index].program, environment);
        }
        Ok(frame_index)
    }

    /// Pushes the `with` objects an activation of `function` inherits from its environment `env`.
    #[inline(always)]
    fn push_inherited_with_scopes(&mut self, env: Value, function: &crate::bytecode::Function, kind: crate::bytecode::ProgramKind) {
        if self.with_scope_entered {
            self.with_stack
                .extend(self.captured_with_objects_for_function(env, function.inherited_with_scope, kind));
        }
    }

    /// Binds an activation's implicit bindings, then its registers and parameters, once its
    /// receiver and `with` scopes are placed. `observable` is whether its bindings are observed.
    fn bind_activation_registers(
        &mut self,
        frame: &mut Frame,
        function: &crate::bytecode::Function,
        context: CallContext,
        args: &[Value],
        captured_this: Option<Value>,
        observable: bool,
    ) {
        let new_target = self.take_construction_target(context).unwrap_or(Value::UNDEFINED);
        self.initialize_activation_bindings(frame, function.is_arrow, new_target, observable);
        // An arrow whose creator's `this` binding is initialized keeps that receiver: the
        // binding never changes once initialized (`Vm::lexical_this`).
        frame.fixed_this |= captured_this.is_some_and(|this| this_is_fixed(false, this));
        frame.prepare_registers_for(function);
        frame.place_parameters(function, args);
    }

    /// Replace the active frame for a terminal user call.  This is the
    /// interpreter's proper-tail-call boundary: the callee reuses the
    /// caller's frame storage, so recursive tail calls do not grow the Rust
    /// stack or the VM frame vector.
    pub(super) fn prepare_user_tail(
        &mut self,
        p: &ResidualProgram,
        frame_index: usize,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<(), JsError> {
        self.profile
            .function(self.active_program.raw(), id as usize);
        let function = &p.function(id as usize);
        let placeholder = self.pooled_frame();
        let old = std::mem::replace(&mut self.frames[frame_index], placeholder);
        self.with_stack.truncate(old.with_base);
        let mut frame = old;
        Self::recycle_frame(&mut frame);
        self.bind_activation_arguments(p, &mut frame, function, context, args)?;
        let captured_this = if function.is_arrow { self.captured_lexical_this(parent) } else { None };
        let this = self.activation_receiver(function, captured_this, this)?;
        frame.place(id, self.active_program, parent, this, self.with_stack.len());
        self.push_inherited_with_scopes(parent, function, p.kind);
        let observable = id == super::ROOT_FUNCTION_ID || function.activation_bindings_observable(p);
        self.bind_activation_registers(&mut frame, function, context, args, captured_this, observable);
        let placeholder = std::mem::replace(&mut self.frames[frame_index], frame);
        self.frame_pool.push(placeholder);
        Ok(())
    }

    /// Binds an activation's locals to its call: fixed parameters, the rest array, the
    /// function's own name, the `arguments` object and the legacy invocation record.
    #[inline(always)]
    fn bind_activation_arguments(
        &mut self,
        p: &ResidualProgram,
        frame: &mut Frame,
        function: &crate::bytecode::Function,
        context: CallContext,
        args: &[Value],
    ) -> Result<(), JsError> {
        let fixed = usize::from(function.params) - usize::from(function.rest);
        // Promoted parameters live in registers; `place_parameters` clears their
        // local slots once the frame is set up, so every fixed parameter is copied here. Every
        // other local starts undefined.
        let passed = fixed.min(args.len());
        frame.locals.clear();
        frame.locals.extend_from_slice(&args[..passed]);
        frame
            .locals
            .resize(usize::from(function.locals), Value::UNDEFINED);
        if function.rest {
            // A rest view's local stays `undefined`: its reads take the values past the fixed
            // parameters from those the frame keeps (`RestGet`), below.
            if !function.rest_view() {
                frame.locals[fixed] = self.rest_parameter_array(args.get(fixed..).unwrap_or_default());
            }
        }
        if let Some(slot) = function.self_binding_slot {
            frame.locals[usize::from(slot)] = context.callee().unwrap_or(Value::UNDEFINED);
        }
        let forwards_arguments = function.call_plan(p).forwards_arguments;
        if let Some(slot) = function.arguments_slot {
            frame.locals[usize::from(slot)] = if forwards_arguments {
                Value::UNMATERIALIZED_ARGUMENTS
            } else {
                self.activation_arguments_object(function, context, args)?
            };
        }
        self.initialize_frame_invocation(p, frame, function, context, args);
        if forwards_arguments || function.rest && function.rest_view() {
            // The frame keeps every argument for the object it may still materialize; legacy
            // reflection reads the same values, since such a frame reflects no parameter.
            debug_assert_eq!(frame.reflected_parameters, 0);
            frame.original_arguments.clear();
            frame.original_arguments.extend_from_slice(args);
        }
        Ok(())
    }

    /// The `arguments` object of activation `f`, whose binding holds
    /// `UNMATERIALIZED_ARGUMENTS`: made from the values the frame kept, as on entry, and
    /// bound so that every later read finds the same object.
    #[inline(never)]
    pub(super) fn materialize_activation_arguments(
        &mut self,
        p: &ResidualProgram,
        f: usize,
    ) -> Result<Value, JsError> {
        let frame = &self.frames[f];
        let function = &p.function(frame.function as usize);
        let slot = usize::from(
            function
                .arguments_slot
                .expect("only an `arguments` binding is unmaterialized"),
        );
        let context = frame.context;
        let args: *const [Value] = self.frames[f].original_arguments.as_slice();
        // SAFETY: the frame stays on the stack while its `arguments` object is made, nothing in that
        // path writes its kept values, and the buffer does not move when `frames` grows.
        let arguments = self.activation_arguments_object(function, context, unsafe { &*args })?;
        self.frames[f].locals[slot] = arguments;
        Ok(arguments)
    }

    /// `ArgumentsLength` (no `key`) or `ArgumentGet` of `key` on `binding`, the value of
    /// frame `f`'s `arguments` binding. An unmaterialized binding answers its length and the
    /// indices it holds from the values the frame kept, as the object made on entry would;
    /// any other key materializes it (`materialize_activation_arguments`). Any other value is
    /// read as `GetField` of `length` and `GetIndex` read it.
    pub(super) fn read_arguments_binding(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        mut binding: Value,
        key: Option<Value>,
    ) -> Result<Value, JsError> {
        if binding == Value::UNMATERIALIZED_ARGUMENTS {
            let kept = &self.frames[f].original_arguments;
            match key {
                None => return Ok(Value::number(kept.len() as f64)),
                Some(key) => {
                    if let Some(value) = self
                        .array_index_key(key)
                        .and_then(|index| kept.get(index as usize))
                    {
                        return Ok(*value);
                    }
                }
            }
            binding = self.materialize_activation_arguments(p, f)?;
        }
        match key {
            None => self.get_property(p, binding, self.length_atom),
            Some(key) => self.get_index(p, binding, key),
        }
    }

    /// Copies a program root activation's module import bindings into its locals or, for a
    /// module environment, its environment slots.
    #[inline(never)]
    fn bind_module_imports(&mut self, frame: &mut Frame) -> Result<(), JsError> {
        for &(slot, import) in self.programs.module_imports(frame.program) {
            let value = match import {
                super::program_store::ModuleImport::Value(value)
                | super::program_store::ModuleImport::Binding(_, _, value) => value,
            };
            let index = usize::from(slot);
            if frame.locals.get(index).is_none() {
                return Err(JsError("module import slot is out of bounds".into()));
            }
            if frame.captured {
                *self
                    .heap
                    .environment_slot_mut(frame.env, index)
                    .ok_or_else(|| JsError("module import slot is out of bounds".into()))? = value;
            } else {
                frame.locals[index] = value;
            }
        }
        Ok(())
    }

    /// The rest parameter's array of the arguments past the fixed parameters: a fresh array
    /// as an array literal of them makes (`literal_array`), in a young run where one is free.
    #[inline(never)]
    pub fn rest_parameter_array(&mut self, rest: &[Value]) -> Value {
        self.literal_array(rest.len(), |index| rest[index])
    }

    /// A new activation's `arguments` object, mapped onto the parameters when the function's
    /// parameter list allows it; the rest array a rest alias names (`Function::rest_alias`).
    #[inline(never)]
    pub fn activation_arguments_object(
        &mut self,
        function: &crate::bytecode::Function,
        context: CallContext,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if function.rest_alias { return Ok(self.rest_parameter_array(args)); }
        let mapped = function.arguments_are_mapped();
        let mut array = crate::heap::ArrayObject::new(
            Self::empty_object(self.object_proto),
            crate::heap::ArrayElements::from_slice(args),
        );
        array.set_arguments_object();
        if function.arguments_alias_parameters() {
            array.set_arguments_map((0..function.params.min(args.len() as u16)).collect());
        }
        // The realm's template gives the whole object at once; the first one of a realm
        // defines its properties one by one, which records the template.
        if let Some((shape, properties)) = self.arguments_template_properties(context.callee(), args, mapped)? {
            return Ok(self.heap.alloc_array_with_properties(array, shape, &properties));
        }
        let arguments = self.heap.alloc_array(array);
        self.initialize_arguments_object(arguments, context.callee(), args, mapped)?;
        Ok(arguments)
    }

    pub(super) fn initialize_frame_invocation(
        &self,
        p: &ResidualProgram,
        frame: &mut Frame,
        function: &crate::bytecode::Function,
        context: CallContext,
        args: &[Value],
    ) {
        frame.context = context;
        frame.original_arguments.clear();
        frame.reflected_parameters = 0;
        let plan = function.call_plan(p);
        if context.callable().is_none() || !plan.legacy_reflection {
            return;
        }
        let reflected = if plan.live_parameters {
            args.len().min(usize::from(function.params))
        } else {
            0
        };
        frame.reflected_parameters = reflected as u16;
        if reflected < args.len() {
            frame
                .original_arguments
                .extend_from_slice(&args[reflected..]);
        }
    }

    /// The shape and property values of an arguments object of `args` from the realm's
    /// template (`cache_arguments_object_template`): `length`, `callee` (the callable when
    /// `expose_callee`, else the template's %ThrowTypeError% accessor slot) and the iterator.
    /// `None` before the realm made one.
    fn arguments_template_properties(
        &self,
        callable: Option<Value>,
        args: &[Value],
        expose_callee: bool,
    ) -> Result<Option<(u32, [Value; ARGUMENTS_OBJECT_PROPERTY_COUNT])>, JsError> {
        let Some(template) = self
            .realm
            .intrinsics
            .arguments_objects
            .get(&(self.realm.globals, expose_callee))
            .copied()
        else {
            return Ok(None);
        };
        let anchor = self
            .heap.object(template.anchor)
            .expect("arguments template anchor is an object");
        let iterator = self
            .heap
            .property_get_ref(anchor, template.iterator_slot)
            .expect("arguments template has an iterator method");
        let mut properties = [Value::UNDEFINED; ARGUMENTS_OBJECT_PROPERTY_COUNT];
        properties[template.length_slot] = Value::number(args.len() as f64);
        properties[template.callee_slot] = if expose_callee {
            callable.ok_or_else(|| JsError::validation("arguments object requires callable identity".into()))?
        } else {
            self.heap
                .property_get_ref(anchor, template.callee_slot)
                .expect("arguments template has a callee accessor")
        };
        properties[template.iterator_slot] = iterator;
        Ok(Some((anchor.shape(), properties)))
    }

    pub(super) fn initialize_arguments_object(
        &mut self,
        arguments: Value,
        callable: Option<Value>,
        args: &[Value],
        expose_callee: bool,
    ) -> Result<(), JsError> {
        let template_key = (self.realm.globals, expose_callee);
        if let Some((shape, properties)) = self.arguments_template_properties(callable, args, expose_callee)? {
            self.heap.ensure_cell_extras(arguments);
            self.heap
                .object_mut(arguments)
                .expect("arguments object is an array")
                .set_arguments_object();
            self.heap
                .initialize_object_properties(arguments, shape, &properties);
            return Ok(());
        }
        self.heap.ensure_cell_extras(arguments);
        if let Some(mut object) = self.heap.object_mut(arguments) {
            object.set_arguments_object();
        }
        let length = self.length_atom;
        self.set_property_with_attributes(
            arguments,
            length,
            Value::number(args.len() as f64),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        let callee = self.intern_atom("callee");
        let value = if expose_callee {
            callable.ok_or_else(|| {
                JsError::validation("mapped arguments require callable identity".into())
            })?
        } else {
            Value::UNDEFINED
        };
        self.set_property(arguments, callee, value)?;
        if expose_callee {
            self.set_property_attributes(
                arguments,
                property_key::PropertyKey::string(callee),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        } else {
            let thrower = self.throw_type_error_for_current_realm();
            self.set_property_attributes(
                arguments,
                property_key::PropertyKey::string(callee),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: true,
                    getter: Some(thrower),
                    setter: Some(thrower),
                },
            );
        }
        if let Some(iterator) = self.well_known_symbols.get(WellKnownSymbol::Iterator) {
            let iterator_method = self.native_value(Native::ArrayValues);
            self.set_symbol_property(arguments, iterator, iterator_method)?;
            self.set_property_attributes(
                arguments,
                property_key::PropertyKey::symbol(iterator),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            self.cache_arguments_object_template(arguments, template_key, length, callee, iterator);
        }
        Ok(())
    }

    fn cache_arguments_object_template(
        &mut self,
        arguments: Value,
        key: (Value, bool),
        length: Atom,
        callee: Atom,
        iterator: Value,
    ) {
        let Some((shape, length_slot)) =
            self.object_property_slot(arguments, property_key::PropertyKey::string(length))
        else {
            return;
        };
        let Some((_, callee_slot)) =
            self.object_property_slot(arguments, property_key::PropertyKey::string(callee))
        else {
            return;
        };
        let Some((_, iterator_slot)) =
            self.object_property_slot(arguments, property_key::PropertyKey::symbol(iterator))
        else {
            return;
        };
        if self.shapes[shape as usize].storage_len() != ARGUMENTS_OBJECT_PROPERTY_COUNT {
            return;
        }
        let mut properties = [Value::UNDEFINED; ARGUMENTS_OBJECT_PROPERTY_COUNT];
        let Some(object) = self.heap.object(arguments) else {
            return;
        };
        let proto = object.proto();
        let Some(iterator_method) = self.heap.property_get_ref(object, iterator_slot) else {
            return;
        };
        properties[iterator_slot] = iterator_method;
        let (_, expose_callee) = key;
        if !expose_callee {
            let Some(thrower) = self.heap.property_get_ref(object, callee_slot) else {
                return;
            };
            properties[callee_slot] = thrower;
        }
        let anchor = self
            .heap
            .alloc_object_with_properties(proto, shape, &properties);
        self.realm.intrinsics.arguments_objects.insert(
            key,
            ArgumentsObjectTemplate {
                anchor,
                length_slot,
                callee_slot,
                iterator_slot,
            },
        );
    }

    pub fn promote_frame_environment(&mut self, frame: usize) -> Value {
        if self.frames[frame].captured {
            return self.frames[frame].env;
        }
        let state = &mut *self.frames[frame];
        let (mut locals, bindings) = (std::mem::take(&mut state.locals), std::mem::take(&mut state.dynamic_bindings));
        let (program, function, parent) = (state.program, state.function, state.env);
        let (env, _) = self.promoted_environment(parent, program, function, &mut locals, bindings);
        // A root's promoted environment is the one its written-once slots are read from (`root_environment`).
        if function == super::ROOT_FUNCTION_ID {
            self.programs.note_root_environment(program, env);
        }
        // The locals' buffer stays with the frame for its next activation.
        let state = &mut *self.frames[frame];
        state.locals = locals;
        state.env = env;
        state.captured = true;
        env
    }

    /// The environment under `parent` an activation of `function` of `program` makes of its
    /// `locals` when a closure first captures them, with its slot storage: it copies the slots it
    /// captures out of the locals, which read as deleted from now on, and takes over the
    /// activation's implicit `bindings`.
    pub(super) fn promoted_environment(
        &mut self,
        parent: Value,
        program: ProgramId,
        function: u32,
        locals: &mut Vec<Value>,
        bindings: Vec<(Atom, Value)>,
    ) -> (Value, *const crate::heap::EnvironmentSlots) {
        let code = self.programs.get(program);
        let selective = code
            .as_ref()
            .and_then(|code| code.function_get(function as usize))
            .and_then(|function| function.selective_capture_slots.as_deref());
        let slots = captured_slots(locals, selective);
        self.heap.alloc_environment(
            parent,
            function,
            slots,
            crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: Some(program.raw()),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: bindings.into(),
                with_objects: Box::default(),
            }),
        )
    }

    pub fn clone_frame_environment(
        &mut self,
        frame: usize,
        fresh: &[u16],
        reset: crate::bytecode::Resets<'_>,
    ) -> Result<(), JsError> {
        if !self.frames[frame].captured {
            return Ok(());
        }
        let (source, program, function) = (self.frames[frame].env, self.frames[frame].program, self.frames[frame].function);
        self.frames[frame].env = self.iteration_environment(source, program, function, fresh, reset)?;
        Ok(())
    }

    /// The copy of `source`, the captured environment of an activation of `function` of
    /// `program`, that a `CloneEnv` of a loop iteration makes: its `fresh` slots own their
    /// current values, its `reset` slots own `undefined`, and every other slot stays shared
    /// with the binding's owner, so closures of earlier iterations and the activation keep
    /// seeing one binding (`EnvironmentSlots::iteration_copy`). A frame's and an environment
    /// activation's window's clones (`clone_environment_of`) are this one copy.
    pub fn iteration_environment(
        &mut self,
        source: Value,
        program: ProgramId,
        function: u32,
        fresh: &[u16],
        reset: crate::bytecode::Resets<'_>,
    ) -> Result<Value, JsError> {
        let slots = self
            .heap
            .clone_environment_slots(source, fresh, reset)
            .ok_or_else(|| JsError("invalid environment clone slots".into()))?;
        // A copy shares the activation's dynamic bindings with it: its implicit `this` and
        // `new.target` and any a direct eval declares. A function whose names all resolve
        // statically has no direct eval, so an activation holding none now never will, and
        // its copies keep an unboxed scope of their own.
        let never_binds_dynamically = self
            .heap
            .environment_bindings(source)
            .is_some_and(Vec::is_empty)
            && self.programs.residual(program).is_some_and(|p| {
                p.function_get(function as usize).is_some_and(|function| {
                    function.names_resolve_statically(p)
                })
            });
        let owner = if never_binds_dynamically {
            None
        } else {
            Some(
                self.heap
                    .environment_binding_owner(source)
                    .ok_or_else(|| JsError("invalid environment clone owner".into()))?,
            )
        };
        let Some(crate::heap::EnvironmentRecord {
            parent,
            function,
            scope,
            ..
        }) = self.heap.environment(source)
        else {
            return Err(JsError("invalid environment clone".into()));
        };
        let (parent, function) = (*parent, *function);
        let scope_ref = crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: scope.program(),
            root_eval_scope: scope.root_eval_scope(),
            binding_site_pc: scope.binding_site_pc(),
            dynamic_bindings: owner.map_or_else(
                || crate::heap::EnvironmentBindings::Owned(Vec::new()),
                crate::heap::EnvironmentBindings::Shared,
            ),
            with_objects: scope.with_objects().into(),
        });
        Ok(self.heap.alloc_environment(parent, function, slots, scope_ref).0)
    }

    pub(super) fn activate_frame(&mut self, frame: &mut Frame) {
        frame.with_base = self.with_stack.len();
        self.with_stack.append(&mut frame.with_objects);
    }

    pub(super) fn deactivate_frame(
        &mut self,
        frame: &mut Frame,
        outcome: &Result<FrameOutcome, JsError>,
    ) {
        match outcome {
            Ok(FrameOutcome::Suspend(..)) => {
                // Without `with` scopes the activation entered, its (empty) list stays.
                if self.with_stack.len() > frame.with_base {
                    frame.with_objects = self.with_stack.split_off(frame.with_base);
                }
            }
            _ => self.finish_frame_scopes(frame),
        }
    }

    /// The retirement of a frame that returned `value` to its pending call of `completion`, which
    /// the general loop's return and the lane's return of a VM frame
    /// (`return_script_frame_in_lane`) both make: a generator's activation finishes its step,
    /// stopped as `stop` says (returned where `None`), and its result is the call's value; any
    /// other frame ends its scopes and goes back to the pool, its value unchanged. True where a
    /// generator's step finished, which may have allocated.
    pub fn retire_returned_frame(&mut self, mut completed: Box<Frame>, completion: &mut CallCompletion, value: Value,
        stop: Option<super::generator::MaterializedStop>) -> (Value, bool) {
        match completion.take_generator() {
            Some(resume) => {
                let stop = stop.unwrap_or(super::generator::MaterializedStop::Returned);
                // The frame's scopes end as its stop says: a return leaves them, a yield keeps its
                // `with` objects with the frame, which the record then holds (`FrameRecord::capture`).
                match stop {
                    super::generator::MaterializedStop::Returned => self.finish_frame_scopes(&completed),
                    _ => completed.with_objects = self.with_stack.split_off(completed.with_base),
                }
                let record = FrameRecord::capture(completed);
                (self.finish_materialized_generator_step(record, resume, value, stop), true)
            }
            None => {
                self.finish_frame_scopes(&completed);
                self.pool_frame(completed);
                (value, false)
            }
        }
    }

    /// Drops the `with` scopes a finished (not suspended) activation entered.
    #[inline(always)]
    pub fn finish_frame_scopes(&mut self, frame: &Frame) {
        self.with_stack.truncate(frame.with_base);
    }

    /// Return a Wasm frame the lane pushed to the pool.
    #[inline(always)]
    pub fn retire_wasm_frame(&mut self) {
        let frame = self.frames.pop().expect("pending frame is active");
        self.with_stack.truncate(frame.with_base);
        self.pool_frame(frame);
    }

    pub fn pool_frame(&mut self, frame: impl Into<Box<Frame>>) {
        let mut frame = frame.into();
        Self::recycle_frame(&mut frame);
        self.frame_pool.push(frame);
    }

    /// Empties a finished frame's appendable buffers and drops its callee for reuse, bounding
    /// what the buffers retain. Every activation sets legacy argument reflection and
    /// `fixed_this` itself.
    #[inline]
    pub(super) fn recycle_frame(frame: &mut Frame) {
        frame.context = CallContext::Internal;
        frame.original_arguments.clear();
        frame.with_objects.clear();
        frame.dynamic_bindings.clear();
        frame.active_iterators.clear();
        let retained = frame
            .original_arguments
            .capacity()
            .max(frame.locals.capacity())
            .max(frame.registers.capacity())
            .max(frame.dynamic_bindings.capacity());
        if retained > RETAINED_FRAME_VALUES {
            Self::shrink_frame_buffers(frame);
        }
    }

    /// Bounds what a pooled frame's buffers retain after an unusually large activation.
    #[cold]
    #[inline(never)]
    fn shrink_frame_buffers(frame: &mut Frame) {
        frame.original_arguments.shrink_to(RETAINED_FRAME_VALUES);
        if frame.locals.capacity() > RETAINED_FRAME_VALUES {
            frame.locals.clear();
            frame.locals.shrink_to(RETAINED_FRAME_VALUES);
        }
        if frame.registers.capacity() > RETAINED_FRAME_VALUES {
            frame.registers.clear();
            frame.registers.shrink_to(RETAINED_FRAME_VALUES);
        }
        frame.initialized_registers = frame.initialized_registers.min(frame.registers.len());
        frame.dynamic_bindings.shrink_to(RETAINED_FRAME_VALUES);
    }

    pub fn run_frame_general(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<FrameOutcome, JsError> {
        self.run_frame_general_until::<false>(p, frame, None, None, None)
    }

    /// Runs frame `frame` on from the transition its linked native body made at its entry.
    #[cfg(feature = "native-execution")]
    pub(super) fn run_frame_general_after(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        entered: crate::vm::engine_types::JsNativeStep,
    ) -> Result<FrameOutcome, JsError> {
        self.run_frame_general_until::<false>(p, frame, None, None, Some(GeneralResume::Native(entered)))
    }

    /// Runs the general loop. `STOPS` monomorphizes the parameter-initialization variant, the
    /// only caller with a `stop_pc`, so ordinary dispatch carries no per-instruction stop test.
    pub(super) fn run_frame_general_until<const STOPS: bool>(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        stop_pc: Option<usize>,
        initial_error: Option<JsError>,
        resume: Option<GeneralResume>,
    ) -> Result<FrameOutcome, JsError> {
        debug_assert!(STOPS || stop_pc.is_none());
        let frame_program = self.frames[frame].program;
        let allow_inline_calls = stop_pc.is_none() && initial_error.is_none();
        let previous_program = self.replace_active_program(frame_program);
        let previous_global = self.realm.globals;
        // A nested loop leaves the caller's register file active again.
        let caller_registers = self.active_registers;
        let mut pending_calls = self.pending_call_stacks.pop().unwrap_or_default();
        let outcome = if STOPS {
            self.run_general_loop::<true>(
                p, frame, stop_pc, initial_error, allow_inline_calls, &mut pending_calls, resume,
            )
        } else {
            self.run_general_loop_without_stop(
                p, frame, initial_error, allow_inline_calls, &mut pending_calls, resume,
            )
        };
        pending_calls.clear();
        self.pending_call_stacks.push(pending_calls);
        self.active_registers = caller_registers;
        self.restore_active_program(previous_program);
        self.switch_realm_global(previous_global);
        outcome
    }

    /// Finish the frames a lane run left at frame `frame`'s PC (`LaneHandoff`), the calls pending
    /// above the loop's entry frame in `pending_calls`: the general loop runs from there as it
    /// runs after its own lane run, until the entry frame returns.
    pub fn run_frames_after_lane_exit(
        &mut self,
        entry_program: &ResidualProgram,
        frame: usize,
        pending_calls: &mut Vec<PendingGeneralCall>,
        exit: LaneHandoff,
    ) -> Result<FrameOutcome, JsError> {
        let previous_global = self.realm.globals;
        let caller_registers = self.active_registers;
        let outcome = self.run_general_loop_without_stop(entry_program, frame, None, true, pending_calls, Some(GeneralResume::Lane(exit)));
        self.active_registers = caller_registers;
        self.switch_realm_global(previous_global);
        outcome
    }

    /// Whether the general loop enters `view` at `pc`: where a lane run pays, or at the first instruction of
    /// the callee a due collection's lane stop left to it (`after_lane_exit`), as a native's call would.
    fn enters_in_lane(view: *const crate::vm::engine_types::LaneView, pc: usize, after_lane_exit: bool) -> bool { Self::runs_in_lane(view, pc) || after_lane_exit && pc == 0 && Self::runs_call_in_lane(view) }

    /// Whether a frame entry may run the lane: execution-only support runs it only for a program
    /// with a loaded AOT image (`Vm::may_run_in_lane`), and instruction profiling must observe every PC.
    const FRAME_ENTRY_LANE_RUNS: bool = crate::DYNAMIC_EXECUTION_ENABLED
        && !cfg!(any(feature = "profile-aggregate", feature = "profile-memory"));

    /// Runs frame `frame` from `pc` in the lane of `cursor`'s view, which runs that
    /// instruction. The frame's own return, a generator's plain `yield` and an async
    /// function's observation-free `await` the lane stops at become the frame's outcome as
    /// the general loop's steps make them; any other stop (another instruction, a pending
    /// call, a raised error, another frame or program) resumes the general loop where the
    /// lane left (`run_frames_after_lane_exit`).
    pub(super) fn run_frame_from_lane(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        cursor: &GeneralCodeCursor,
        pc: usize,
    ) -> Result<FrameOutcome, JsError> {
        let (program, function) = (cursor.program, cursor.function);
        let previous_program = self.replace_active_program(program);
        let previous_global = self.realm.globals;
        let caller_registers = self.active_registers;
        let mut pending_calls = self.pending_call_stacks.pop().unwrap_or_default();
        let outcome = (|| {
            let _stack = self.enter_stack()?;
            // As the loop checks before each step; a termination selects no guest handler.
            self.check_execution_interrupt()?;
            self.active_registers = self.frames[frame].registers.as_mut_ptr();
            let mut executing = ExecutingProgram { id: program, retained: None };
            let mut running = frame;
            let pc = self.run_fast_lane(p, program, cursor.lane, &mut running, pc, &mut pending_calls, &mut executing, true);
            let body = p.function(function as usize);
            let stops_here = running == frame
                && pending_calls.is_empty()
                && executing.id == program
                && !self.lane_completion.is_raised()
                && pc < body.code.len();
            if stops_here
                && let Some(outcome) = self.stop_outcome_in_lane(p, frame, cursor.instruction(pc), pc, body.is_async)
            {
                return Ok(outcome);
            }
            self.frames[running].pc = pc;
            let raised = self.lane_completion.take_raised();
            self.run_frames_after_lane_exit(p, running, &mut pending_calls, LaneHandoff { executing, raised })
        })();
        pending_calls.clear();
        self.pending_call_stacks.push(pending_calls);
        self.active_registers = caller_registers;
        self.restore_active_program(previous_program);
        self.switch_realm_global(previous_global);
        outcome
    }

    /// The outcome of frame `frame`, whose lane run stopped in it at `instruction` at `pc`
    /// with no call pending, where the general loop's step there would end its run: the
    /// frame's return, or the completion a call it made returned from it (`Vm::returned_in_lane`),
    /// a generator's plain `yield`, or an async function's observation-free `await`
    /// (`stop_awaits_in_lane`). `None` leaves the instruction to the general loop.
    #[inline(always)]
    fn stop_outcome_in_lane(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        instruction: WideInstruction,
        pc: usize,
        asynchronous: bool,
    ) -> Option<FrameOutcome> {
        let returned = match self.lane_completion.take_returned() {
            Some(value) => Some(value),
            None => match crate::staged::Operation::<u64, ()>::of(instruction) {
                Some(crate::staged::Operation::Return { source }) => Some(self.read(frame, source)),
                _ => None,
            },
        };
        if let Some(value) = returned {
            self.frames[frame].pc = pc;
            self.frames[frame].pc_is_resume = true;
            return Some(FrameOutcome::Complete(value));
        }
        if !crate::staged::Operation::<u64, ()>::is_yield(instruction) {
            return asynchronous.then(|| self.stop_awaits_in_lane(p, frame, Some(instruction), pc)).flatten();
        }
        let value = self.read(frame, instruction.register_b());
        // An async generator's `yield` resolves a promise for the value, as the general step
        // does (`promise_for_value`), where that observes nothing.
        let value = if asynchronous { self.intrinsic_promise_resolve(p, value)? } else { value };
        self.frames[frame].pc = pc + 1;
        self.frames[frame].pc_is_resume = true;
        Some(FrameOutcome::Suspend(
            Suspended::Yield { value, destination: instruction.result_register(), delegated_result: None },
            None,
        ))
    }

    /// The suspension of an async frame `frame` whose lane run stopped at its `Await` record
    /// `instruction` at `pc`, made as the general loop's `Await` makes it, where the awaited
    /// value's conversion observes nothing: a non-object as it is, or an object
    /// `intrinsic_promise_resolve` converts without running guest code. `None` leaves any other
    /// stop, or a conversion that could throw into the frame's handlers, to the general loop.
    pub(super) fn stop_awaits_in_lane(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        instruction: Option<WideInstruction>,
        pc: usize,
    ) -> Option<FrameOutcome> {
        let instruction = instruction.filter(|instruction| crate::staged::Operation::<u64, ()>::is_await(*instruction))?;
        self.await_value_in_lane(p, frame, instruction.register_b(), instruction.result_register(), pc)
            .map(|value| FrameOutcome::Suspend(Suspended::Await { value, destination: instruction.result_register() }, None))
    }

    /// `stop_awaits_in_lane` of the `Await` at `pc` that reads register `awaited` and resumes into
    /// `destination`: the value the suspension awaits, with the frame set to resume after it.
    #[inline(always)]
    pub(super) fn await_value_in_lane(&mut self, p: &ResidualProgram, frame: usize, awaited: u16, destination: u16, pc: usize) -> Option<Value> {
        let _ = destination;
        let awaited = self.read(frame, awaited);
        let value = if self.is_object_like(awaited) {
            self.intrinsic_promise_resolve(p, awaited)?
        } else {
            awaited
        };
        self.frames[frame].pc = pc + 1;
        self.frames[frame].pc_is_resume = true;
        Some(value)
    }

    /// Share the ordinary loop across direct entry and lane exits. Parameter initialization
    /// retains its separately specialized stopping loop.
    #[inline(never)]
    fn run_general_loop_without_stop(
        &mut self,
        entry_program: &ResidualProgram,
        frame: usize,
        initial_error: Option<JsError>,
        allow_inline_calls: bool,
        pending_calls: &mut Vec<PendingGeneralCall>,
        resume: Option<GeneralResume>,
    ) -> Result<FrameOutcome, JsError> {
        self.run_general_loop::<false>(
            entry_program, frame, None, initial_error, allow_inline_calls, pending_calls, resume,
        )
    }

    /// The general dispatch loop of `run_frame_general_until`, whose caller restores the active
    /// program and realm around it; from a `resume`, it continues where that left.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn run_general_loop<const STOPS: bool>(
        &mut self,
        entry_program: &ResidualProgram,
        frame: usize,
        stop_pc: Option<usize>,
        initial_error: Option<JsError>,
        allow_inline_calls: bool,
        pending_calls: &mut Vec<PendingGeneralCall>,
        resume: Option<GeneralResume>,
    ) -> Result<FrameOutcome, JsError> {
        let p = entry_program;
        #[cfg(feature = "native-execution")]
        let mut entered = None;
        let (mut executing_program, mut current_program, raised) = match resume {
            Some(GeneralResume::Lane(LaneHandoff { executing, raised })) => (executing.id, executing.retained, Some(raised)),
            #[cfg(feature = "native-execution")]
            Some(GeneralResume::Native(step)) => {
                entered = Some(step);
                (self.frames[frame].program, None, None)
            }
            None => (self.frames[frame].program, None, None),
        };
        // The residual being executed: the entry program, or the one `current_program`
        // retains after a cross-program tail call. Updated only where that switch happens.
        let mut executing_residual: *const ResidualProgram = current_program.as_deref().unwrap_or(entry_program);
        let mut frame = frame;
        // The completion a call the lane made returned from the frame it left in, taken once
        // per lane exit (`LaneCompletion::Returned`) and returned at that instruction.
        let mut returned_in_lane = self.lane_completion.take_returned();
        // The initial Wasm activation is allocated before the dispatch loop and has no
        // caller guard. Account it here so root and reentrant Wasm entries obey the same
        // frame-payload limit as Wasm calls pushed by the loop.
        let _guest_frame = if p.kind == crate::bytecode::ProgramKind::Wasm {
            let function = p.function(self.frames[frame].function as usize);
            let frame_storage_bytes = (usize::from(function.locals)
                + usize::from(function.registers))
            .saturating_mul(std::mem::size_of::<Value>());
            Some(
                crate::stack::GuestCallGuard::enter_with_storage(frame_storage_bytes)
                    .map_err(|crate::stack::StackExhausted| Self::wasm_call_stack_exhausted())?,
            )
        } else {
            None
        };
        let (_stack, raised) = self.enter_general_loop_stack(p.kind, raised)?;
        // Parameter-initialization stops observe every PC, so they run the general path only. An
        // execution-only build enters the lane for a program with a loaded AOT image, whose bodies are
        // the only code it has (`Vm::may_run_in_lane`).
        let fast_lane = !STOPS && stop_pc.is_none()
            && (Self::FRAME_ENTRY_LANE_RUNS || self.programs.ahead_images(executing_program, p).is_some());
        let initial_function = self.frames[frame].function as usize;
        let mut cursor = self.general_cursor(
            executing_program,
            // SAFETY: as for `p` at the top of the loop.
            unsafe { &*executing_residual },
            initial_function as u32,
            fast_lane,
        );
        let mut pc = self.frames[frame].pc;
        // The active frame's published PC. Frames are boxed, so the slot stays put while that
        // frame is active; every frame change below re-derives it.
        let mut published_pc: *mut usize = &mut self.frames[frame].pc;
        self.active_registers = self.frames[frame].registers.as_mut_ptr();
        if let Some(error) = initial_error {
            pc = self.exception_handler_target(
                p,
                frame,
                initial_function,
                pc.saturating_sub(1) as u32,
                error,
            )?;
        }
        // An error thrown at `throwing_pc` of the running frame: the loop resumes at the
        // handler `unwind_general_loop` finds, popping in-loop activations without one.
        macro_rules! unwind_at {
            ($throwing_pc:expr, $error:expr) => {{
                let mut executing = ExecutingProgram {
                    id: executing_program,
                    retained: current_program.take(),
                };
                let unwound = self.unwind_general_loop(
                    entry_program,
                    frame,
                    pc,
                    $throwing_pc,
                    $error,
                    pending_calls,
                    &mut cursor,
                    &mut executing,
                    fast_lane,
                );
                executing_program = executing.id;
                current_program = executing.retained;
                executing_residual = current_program.as_deref().unwrap_or(entry_program);
                (frame, pc) = unwound?;
                published_pc = &mut self.frames[frame].pc;
            }};
        }
        // A lane exit resumes at its record, which the general path executes, or raises there
        // the error of the call the lane made, as after the loop's own lane run.
        let mut resumes_lane_exit = false;
        // The last step followed a lane run a due collection stopped, or a lane exit (`Vm::enters_in_lane`).
        let mut after_lane_exit = false;
        match raised {
            Some(Some(error)) => {
                let throwing_pc = pc;
                pc += 1;
                unwind_at!(throwing_pc as u32, error);
            }
            Some(None) => resumes_lane_exit = true,
            None => {}
        }
        loop {
            // SAFETY: `executing_residual` points at `entry_program`, borrowed for the whole
            // call, or at the residual `current_program` keeps alive until it is replaced,
            // and it is re-derived at that replacement.
            let p = unsafe { &*executing_residual };
            // Check the stable entry state. A native tail-call result may already
            // have replaced this frame; its old cursor still identifies the call
            // until the TailCall transition below adopts the replacement.
            debug_assert_eq!(cursor.program, self.frames[frame].program);
            debug_assert_eq!(cursor.function, self.frames[frame].function);
            if let Err(error)=self.check_execution_interrupt() {
                let mut executing = ExecutingProgram { id: executing_program, retained: current_program.take() };
                return self.unwind_general_loop(entry_program,frame,pc,pc as u32,error,pending_calls,&mut cursor,&mut executing,fast_lane)
                    .map(|_|unreachable!("host termination cannot select a guest handler"));
            }
            // A supported native function still returns through the ordinary
            // Return instruction, preserving the dispatch loop's frame and
            // constructor unwind path. Guard misses leave pc at zero.
            let mut native_step: Option<crate::vm::engine_types::JsNativeStep> = None;
            let entering_after_lane_exit = std::mem::replace(&mut after_lane_exit, resumes_lane_exit);
            let resumed = std::mem::take(&mut resumes_lane_exit);
            #[cfg(feature = "native-execution")]
            if let Some(step) = entered.take() {
                pc = step.next_pc;
                native_step = Some(step);
            }
            // An execution-only build runs a record only where the lane may run its program: a
            // linked image's body takes the record, or the record it left steps and the lane takes
            // the next one (`Vm::may_run_in_lane`). Without such an image nothing may run it.
            if native_step.is_none() && !self.may_run_in_lane(executing_program, p)
                && !(STOPS && stop_pc == Some(pc))
                && !(!resumed && Self::enters_in_lane(cursor.lane, pc, entering_after_lane_exit))
            {
                return Err(self.native_execution_rejected(p, frame, pc));
            }
            if crate::vm::tier_policy::tiers_enter(STOPS) && native_step.is_none() && !resumed && pc == 0
                && let Some(return_pc) = self.js_tier_frame_entry(p, frame)
            {
                pc = return_pc;
                continue;
            }
            if !resumed && native_step.is_none() && Self::enters_in_lane(cursor.lane, pc, entering_after_lane_exit) {
                let mut executing = ExecutingProgram { id: executing_program, retained: current_program.take() };
                pc = self.run_fast_lane(p, cursor.program, cursor.lane, &mut frame, pc, pending_calls, &mut executing, allow_inline_calls);
                returned_in_lane = self.lane_completion.take_returned();
                crate::clock::note_lane_run();
                after_lane_exit = self.heap.should_collect();
                // The lane may leave in a frame of another program, its activations' calls
                // having set the loop's executing program aside as the loop's pushes do.
                executing_program = executing.id;
                current_program = executing.retained;
                executing_residual = current_program.as_deref().unwrap_or(entry_program);
                // Lane calls and returns can leave a different activation, or a new one in the
                // same frame slot; diagnostics below must observe that activation's view.
                if self.frames[frame].function != cursor.function || executing_program != cursor.program {
                    cursor = self.general_cursor(
                        executing_program,
                        // SAFETY: as for `p` at the top of the loop.
                        unsafe { &*executing_residual },
                        self.frames[frame].function,
                        fast_lane,
                    );
                }
                // SAFETY: as for `p` at the top of the loop; the lane may have left in another program.
                #[cfg(feature = "profile-lane-exits")]
                let p = unsafe { &*executing_residual };
                #[cfg(feature = "profile-lane-exits")]
                if let Some(instruction) = p
                    .functions
                    .get(self.frames[frame].function as usize)
                    .and_then(|function| function.instructions().nth(pc))
                {
                    super::lane_exits::record(instruction.op());
                    if crate::staged::calls(instruction.op()) || crate::staged::Operation::<u64, ()>::is_construction(instruction) {
                        let callee = self.frames[frame].registers.get(usize::from(instruction.register_b())).copied();
                        let detail = match callee.and_then(|callee| self.heap.function(callee)) {
                            Some(crate::heap::FunctionRecord { kind: FunctionKind::Native(native @ (crate::heap::Native::FunctionBoundCall | crate::heap::Native::FunctionCall)), .. }) => {
                                let receiver = self.frames[frame].registers.get(usize::from(instruction.register_c())).copied().unwrap_or(Value::UNDEFINED);
                                let target = callee.and_then(|callee| self.forwarded_call(callee, receiver, &[] as &[Value])).map(|forwarded| forwarded.callee);
                                let described = match target.and_then(|target| self.heap.function(target)) {
                                    Some(crate::heap::FunctionRecord { kind: FunctionKind::Native(target), .. }) => format!("native {target:?}"),
                                    Some(crate::heap::FunctionRecord { kind: FunctionKind::User(program, id), .. }) => {
                                        let name = self.programs.residual(*program).and_then(|code| {
                                            let body = code.function_get(*id as usize)?;
                                            Some(format!("{} same_program={} registers_only={}", body.name.map(|a| code.atoms[a as usize].to_string()).unwrap_or_default(), *program == cursor.program, body.call_plan(code).registers_only))
                                        });
                                        format!("user {}", name.unwrap_or_default())
                                    }
                                    None => "unforwarded".into(),
                                };
                                format!("native {native:?} -> {described}")
                            }
                            Some(crate::heap::FunctionRecord { kind: FunctionKind::Native(native), .. }) => format!("native {native:?}"),
                            Some(crate::heap::FunctionRecord { kind: FunctionKind::User(program, id), .. }) => {
                                let plan = self.programs.residual(*program).and_then(|code| {
                                    let body = code.function_get(*id as usize)?;
                                    Some(format!("user {} same_program={} pc={pc} registers_only={} async={} gen={} class={} self={} args={:?}", body.name.map(|a| code.atoms[a as usize].to_string()).unwrap_or_default(), *program == cursor.program, body.call_plan(code).registers_only, body.is_async, body.is_generator, body.is_class_constructor, body.self_binding_slot.is_some(), body.arguments_slot.is_some()))
                                });
                                plan.unwrap_or_else(|| "user ?".into())
                            }
                            None => "not a function".into(),
                        };
                        let caller = p.function_get(self.frames[frame].function as usize).map(|f| (f.is_async, Self::runs_in_lane(cursor.lane, pc), self.frames.len() - 1 - frame, pending_calls.len()));
                        super::lane_exits::record_detail(format!("{:?} {detail} caller(async,arrow,depth,pending)={caller:?}", instruction.op()));
                    }
                    // merge: is_binary and is_private_in are not Operation constructors on the derivation side; use the Op-level predicates.
                    if matches!(crate::staged::Operation::<u64, ()>::of(instruction), Some(crate::staged::Operation::CloneEnv { .. } | crate::staged::Operation::Return { .. }
                            | crate::staged::Operation::ElementRead { .. } | crate::staged::Operation::ElementWrite { .. } | crate::staged::Operation::CompareJump { .. }))
                        || crate::staged::Operation::<u64, ()>::decode_load_name(instruction, ()).is_some() || crate::staged::Operation::<u64, ()>::is_field_read(instruction)
                        || instruction.op().traits().has(crate::bytecode::Traits::BINARY_RESULT) || crate::staged::private_in_family(instruction.op()) || crate::staged::Operation::<u64, ()>::is_field_store(instruction) {
                        let function = p.function_get(self.frames[frame].function as usize);
                        let name = function.and_then(|f| f.name).map(|a| p.atoms[a as usize].to_string()).unwrap_or_default();
                        let span = function.map(|f| format!("{:?}", f.source_span)).unwrap_or_default();
                        // A store names its cache site and field, which tell a shared overflow site
                        // (`SHARED_UNCACHED_SITE`) from a polymorphic one.
                        let store = crate::staged::Operation::<u64, ()>::is_field_store(instruction)
                            .then(|| format!(" site={} field={}", instruction.c(), &p.atoms[instruction.imm() as usize]))
                            .or_else(|| {
                                crate::staged::Operation::<u64, ()>::decode_load_name(instruction, ()).is_some()
                                    .then(|| format!(" name={}", &p.atoms[instruction.imm() as usize]))
                            })
                            .unwrap_or_default();
                        super::lane_exits::record_detail(format!("{:?} in {name} {span} pc={pc} lane={}{store}", instruction.op(), Self::runs_in_lane(cursor.lane, pc)));
                    }
                    // A `Return` exit of the loop's own entry frame: which native's call into
                    // JavaScript started this nested run.
                    if crate::staged::returns(instruction.op()) && pending_calls.is_empty() {
                        let starter = self.active_native_callable();
                        let described = match starter.and_then(|callable| self.heap.function(callable)) {
                            Some(crate::heap::FunctionRecord { kind: FunctionKind::Native(native), .. }) => {
                                let name = starter
                                    .and_then(|callable| self.own_property(callable, self.runtime_atoms.name))
                                    .and_then(|name| self.heap.string(name).map(|text| text.to_string()))
                                    .unwrap_or_default();
                                format!("{native:?} {name}")
                            }
                            Some(crate::heap::FunctionRecord { kind: FunctionKind::User(..), .. }) => "user".into(),
                            None => "none".into(),
                        };
                        super::lane_exits::record_detail(format!("Return nested run started by {described}"));
                    }
                    if crate::staged::Operation::<u64, ()>::is_field_read(instruction)
                        && let Some(lookup) = instruction.field_lookup()
                    {
                        let (base, atoms) = match lookup {
                            crate::bytecode::FieldLookup::Site(site) => {
                                let site = p.field_sites[site];
                                (site.base, [Some(site.first.0), site.second.map(|second| second.0)])
                            }
                            crate::bytecode::FieldLookup::Atom { atom, base, .. } => (base, [Some(atom), None]),
                        };
                        let names = atoms.iter().flatten().map(|atom| p.atoms[*atom as usize].to_string()).collect::<Vec<_>>().join(".");
                        let base = self.resolve_field_base(p, frame, base).ok();
                        let kind = base.and_then(|base| self.heap.get(base)).map_or("immediate", |cell| cell.profile_variant_name());
                        super::lane_exits::record_detail(format!("GetField {names} on {kind}"));
                    }
                    if matches!(crate::staged::Operation::<u64, ()>::of(instruction), Some(crate::staged::Operation::ElementRead { .. } | crate::staged::Operation::ElementWrite { .. }))
                        || matches!(crate::staged::Operation::<u64, ()>::decode_literal(instruction, ()), Some(crate::staged::Operation::DefineElement { .. })) {
                        let (base, key) = match crate::staged::Operation::<u64, ()>::of(instruction) {
                            Some(crate::staged::Operation::ElementRead { .. }) => (self.inert_operand(frame, instruction.operand_b()), self.inert_operand(frame, instruction.operand_c())),
                            Some(crate::staged::Operation::ElementWrite { .. }) => (Some(self.read(frame, instruction.register_b())), Some(self.read(frame, instruction.register_c()))),
                            _ => {
                                let registers = &self.frames[frame].registers;
                                (registers.get(usize::from(instruction.a())).copied(), registers.get(usize::from(instruction.register_b())).copied())
                            }
                        };
                        let describe = |value: Option<Value>| match value {
                            None => "?".to_string(),
                            Some(v) if v.as_int().is_some() => "int".into(),
                            Some(v) if v.as_number().is_some() => "num".into(),
                            Some(v) if self.heap.string(v).is_some() => {
                                let text = self.heap.string(v).unwrap();
                                match self.lookup_js_atom(text) {
                                    Some(atom) => format!("str(plain_key={})", self.plain_named_key(atom)),
                                    None => "str(new)".into(),
                                }
                            }
                            Some(v) if self.heap.array(v).is_some() => "array".into(),
                            Some(v) => match self.heap.get(v) {
                                _ if let Some(symbol) = self.heap.symbol_data(v) => format!("symbol {:?}", symbol.kind),
                                _ if self.heap.ordinary_view(v).is_some() => {
                                    let data = self.heap.ordinary_view(v).unwrap();
                                    format!("ordinary(ext={} side={} nullproto={} dict={})", data.is_extensible(), data.has_side_descriptors(), data.proto().is_null(), self.shape_is_dictionary(data.shape()))
                                }
                                Some(cell) => format!("cell {}", cell.profile_variant_name()),
                                None => "other".into(),
                            },
                        };
                        let dense = base.and_then(|base| self.heap.array(base)).map(|array| (array.elements().len(), array.has_indexed_descriptors(), array.is_frozen()));
                        super::lane_exits::record_detail(format!("{:?} kinds base={} key={} array={dense:?}", instruction.op(), describe(base), describe(key)));
                    }
                    if matches!(crate::staged::Operation::<u64, ()>::of(instruction), Some(crate::staged::Operation::CompareJump { .. })) || crate::staged::Operation::<u64, ()>::is_binary(instruction) {
                        let constants = self.programs.constants(cursor.program);
                        let registers = &self.frames[frame].registers;
                        let kind = |operand: crate::bytecode::Operand| {
                            let value = match operand.kind() {
                                Some(crate::bytecode::OperandKind::Register) => registers.get(usize::from(operand.payload())).copied(),
                                Some(crate::bytecode::OperandKind::Constant) => constants.get(usize::from(operand.payload())).copied(),
                                _ => None,
                            };
                            match value {
                                None => "?",
                                Some(v) if v.as_int().is_some() => "int",
                                Some(v) if v.as_number().is_some() => "num",
                                Some(v) if v.is_undefined() => "undef",
                                Some(v) if v.is_null() => "null",
                                Some(v) if v.as_bool().is_some() => "bool",
                                Some(v) => match self.heap.get(v) {
                                    _ if self.heap.is_string(v)  => "str",
                                    Some(Cell::BigInt(_)) => "bigint",
                                    _ => "obj",
                                },
                            }
                        };
                        let operator = if crate::staged::Operation::<u64, ()>::is_binary(instruction) { instruction.binary_operator() } else { u32::from(instruction.a()) };
                        super::lane_exits::record_detail(format!(
                            "{:?} op{operator} {} {}",
                            instruction.op(),
                            kind(instruction.operand_b()),
                            kind(instruction.operand_c())
                        ));
                    }
                }
                // Lane calls and returns can leave a different activation, or a new one in the
                // same frame slot; the active frame's PC slot and register file follow it.
                published_pc = &mut self.frames[frame].pc;
                self.active_registers = self.frames[frame].registers.as_mut_ptr();
                // A call the lane made in place failed: it throws at its instruction, as the
                // general path's call would have.
                if let Some(error) = self.lane_completion.take_raised() {
                    let mut executing = ExecutingProgram {
                        id: executing_program,
                        retained: current_program.take(),
                    };
                    // The loop's entry program, as at every unwind: it names the program of
                    // each frame that executes no retained one, which the executing residual
                    // `p` does not once a pending call of another program unwinds.
                    let unwound = self.unwind_general_loop(
                        entry_program,
                        frame,
                        pc + 1,
                        pc as u32,
                        error,
                        pending_calls,
                        &mut cursor,
                        &mut executing,
                        fast_lane,
                    );
                    executing_program = executing.id;
                    current_program = executing.retained;
                    executing_residual = current_program.as_deref().unwrap_or(entry_program);
                    (frame, pc) = unwound?;
                    published_pc = &mut self.frames[frame].pc;
                    continue;
                }
                // A run that stopped at the `await` of the loop's own frame, whose `Await`
                // step would end the loop with it, suspends it here where the awaited value's
                // conversion observes nothing (`stop_awaits_in_lane`); not past a native step, whose next PC may end the function.
                if pending_calls.is_empty()
                    && let Some(outcome) =
                        self.stop_awaits_in_lane(unsafe { &*executing_residual }, frame, native_step.is_none().then(|| cursor.instruction(pc)), pc)
                {
                    return Ok(outcome);
                }
            }
            // The lane may have left in another program's frame (above).
            // SAFETY: as for `p` at the top of the loop.
            let p = unsafe { &*executing_residual };
            if STOPS && stop_pc == Some(pc)
                && native_step.as_ref().is_none_or(|step| step.instruction_pc == pc)
            {
                self.frames[frame].pc = pc;
                self.frames[frame].pc_is_resume = true;
                return Ok(FrameOutcome::ParameterInitializationComplete);
            }
            let native_step = native_step.take();
            let instruction_pc = native_step.as_ref().map_or(pc, |step| step.instruction_pc);
            let ins = cursor.instruction(instruction_pc);
            let step = if let Some(step) = native_step {
                step.result
            } else if !self.may_run_in_lane(cursor.program, p) {
                // An execution-only build steps only a record a loaded AOT image's body left at
                // (`Vm::may_run_in_lane`); without such an image nothing may run the program.
                return Err(self.interpreter_unavailable());
            } else {
            #[cfg(feature = "profile-memory")]
            self.heap.set_memory_allocation_site(
                cursor.program.raw(),
                cursor.function,
                instruction_pc,
            );
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::count_general(
                || {
                    let body = p.function_get(cursor.function as usize);
                    format!(
                        "{} {} @{:?}",
                        p.source_name,
                        body.and_then(|body| body.name).map(|atom| p.atoms[atom as usize].to_string()).unwrap_or_default(),
                        body.and_then(|body| body.source_span).map(|span| span.start),
                    )
                },
                (cursor.program.raw(), cursor.function),
                ins.op(),
            );
            pc += 1;
            #[cfg(feature = "profile-aggregate")]
            self.profile.opcode(
                ins.op() as usize,
                frame,
                cursor.program.raw(),
                cursor.function,
                instruction_pc,
            );
            #[cfg(feature = "profile-aggregate")]
            self.profile.object_literal_instruction(
                cursor.program.raw(),
                cursor.function,
                instruction_pc,
                ins.op(),
            );
            #[cfg(not(feature = "profile-aggregate"))]
            self.profile.opcode(ins.op() as usize);
            // Register/local moves and field cache hits cannot allocate, collect or throw, so they
            // need neither the published root-map PC nor the general step result.
            match crate::staged::frame_arm::arm(ins.op()) {
                FrameArm::LoadLocalPlain => {
                    // SAFETY: validated bytecode bounds the slot by Function.locals.
                    let value = unsafe { self.read_validated_local(frame, ins.local_slot()) };
                    self.write(frame, ins.result_register(), value);
                    continue;
                }
                FrameArm::StoreLocalPlain => {
                    let value = self.read(frame, ins.register_a());
                    // SAFETY: validated bytecode bounds the slot by Function.locals.
                    unsafe { self.write_validated_local(frame, ins.local_slot(), value) };
                    if let Some(register) = ins.optional_register_b() {
                        self.write(frame, register, value);
                    }
                    continue;
                }
                FrameArm::Move => {
                    self.write(
                        frame,
                        ins.result_register(),
                        self.read(frame, ins.register_b()),
                    );
                    continue;
                }
                FrameArm::ConstLoad => {
                    if let Some(value) = self.active_constant(ins.constant_index()) {
                        self.write(frame, ins.result_register(), value);
                        continue;
                    }
                }
                FrameArm::LoadCapture => {
                    if let Some(value) =
                        self.cached_plain_capture(frame, ins.capture_depth(), ins.capture_slot())
                    {
                        self.write(frame, ins.result_register(), value);
                        continue;
                    }
                }
                FrameArm::JumpIf => {
                    // ToBoolean runs no guest code.
                    let value = self.read(frame, ins.register_a());
                    let truthy = self.truthy(value);
                    #[cfg(feature = "profile-aggregate")]
                    self.profile
                        .branch_value(value.profile_kind() as usize, truthy);
                    if truthy == crate::staged::frame_arm::jump_taken(ins) {
                        pc = ins.jump_target() as usize;
                        if pc <= instruction_pc {
                            self.note_back_edge(&mut cursor);
                        }
                    }
                    continue;
                }
                FrameArm::CompareJump => {
                    let operator = ins.binary_operator_field();
                    if let (Some(left), Some(right)) = (
                        self.inert_operand(frame, ins.operand_b()),
                        self.inert_operand(frame, ins.operand_c()),
                    ) && let Some(truthy) = self
                        .number_comparison_truthy(operator, left, right)
                        .or_else(|| self.identity_equality(operator, left, right))
                        .or_else(|| self.instanceof_truthy(operator, left, right))
                    {
                        self.profile.binary(
                            operator as usize,
                            ins.operand_b().0,
                            ins.operand_c().0,
                        );
                        if !truthy {
                            pc = ins.jump_target() as usize;
                            if pc <= instruction_pc {
                                self.note_back_edge(&mut cursor);
                            }
                        }
                        continue;
                    }
                }
                // A cached own-data store writes a slot in place: it neither allocates nor throws.
                // `this.x = v` is the same cached write with an authoritative `this`.
                FrameArm::FieldStore
                    if let Some(receiver) = ins.field_store_register().map(|register| self.read(frame, register))
                        .or_else(|| self.frames[frame].fixed_this.then(|| self.frames[frame].this)) => {
                    if self.cached_field_write_hit(
                        p,
                        receiver,
                        ins.atom_index(),
                        self.read(frame, ins.register_a()),
                        ins.cache_site_index(),
                    ) {
                        continue;
                    }
                }
                // Number arithmetic on register or constant operands runs no guest code,
                // allocates nothing and cannot throw.
                FrameArm::Binary | FrameArm::NumericArith
                    if !ins.returns_from_frame()
                        && let (Some(left), Some(right)) = (
                            self.inert_operand(frame, ins.operand_b()),
                            self.inert_operand(frame, ins.operand_c()),
                        )
                        && let Some(value) =
                            self.number_binary(ins.binary_operator(), left, right) =>
                {
                    self.profile.binary(
                        ins.binary_operator() as usize,
                        ins.operand_b().0,
                        ins.operand_c().0,
                    );
                    if let Some(local) = ins.numeric_local_target() {
                        self.frames[frame].locals[usize::from(local)] = value;
                        self.profile.virtual_opcode(
                            self.frames[frame].program.raw(),
                            crate::staged::STORE_LOCAL_PLAIN_ROW as usize,
                        );
                    } else {
                        self.write(frame, ins.result_register(), value);
                    }
                    continue;
                }
                // Equality decided without conversion, and instanceof reduced to a prototype
                // walk, run no guest code either.
                FrameArm::Binary
                    if !ins.returns_from_frame()
                        && ins.numeric_local_target().is_none()
                        && let (Some(left), Some(right)) = (
                            self.inert_operand(frame, ins.operand_b()),
                            self.inert_operand(frame, ins.operand_c()),
                        )
                        && let Some(equal) = self
                            .identity_equality(ins.binary_operator(), left, right)
                            .or_else(|| {
                                self.instanceof_truthy(ins.binary_operator(), left, right)
                            }) =>
                {
                    self.write(
                        frame,
                        ins.result_register(),
                        if equal { Value::TRUE } else { Value::FALSE },
                    );
                    continue;
                }
                // A field cache hit reads a data slot, and an array's length is an own data
                // property: neither allocates nor throws. Anything else takes the general path.
                // `this` is a base once the activation fixed it, as for `SetThisField`.
                FrameArm::GetField
                    if ins.result_flags() == 0
                        && let Some(crate::bytecode::FieldLookup::Atom {
                            atom,
                            base,
                            cache_site,
                        }) = ins.field_lookup()
                        && let Some(object) = match base.register_index() {
                            Some(register) => Some(self.read(frame, register)),
                            // A local base (`FieldBase::local`) is the general path's read.
                            None if base.local_index().is_some() => None,
                            None => self.frames[frame]
                                .fixed_this
                                .then(|| self.frames[frame].this),
                        } =>
                {
                    let value = match self.cached_field_hit(p, object, cache_site) {
                        super::object::FieldCacheProbe::Hit(value) => Some(value),
                        super::object::FieldCacheProbe::Ineligible => {
                            self.array_length_field(object, atom)
                        }
                        super::object::FieldCacheProbe::OrdinaryMiss => None,
                    };
                    if let Some(value) = value {
                        self.write(frame, ins.result_register(), value);
                        continue;
                    }
                }
                // A present dense array element is a data property (accessor indices are
                // stored as holes): reading it, or replacing it in an array without indexed
                // descriptors or frozen state, runs no guest code and cannot throw.
                FrameArm::GetIndex
                    if let (Some(base), Some(key)) = (
                        self.inert_operand(frame, ins.operand_b()),
                        self.inert_operand(frame, ins.operand_c()),
                    ) && let Some(value) = self.dense_array_element(base, key) =>
                {
                    self.write(frame, ins.result_register(), value);
                    continue;
                }
                FrameArm::SetIndex => {
                    let (object, key) = (
                        self.read(frame, ins.register_b()),
                        self.read(frame, ins.register_c()),
                    );
                    if self.try_replace_dense_element(
                        object,
                        key,
                        self.read(frame, ins.register_a()),
                    ) {
                        continue;
                    }
                }
                // Number increments and conversions of Numbers run no guest code, allocate
                // nothing and cannot throw.
                FrameArm::IncDec
                    if let Some(value) = super::dispatch::number_increment(
                        self.read(frame, ins.register_b()),
                        ins.boolean_flag().expect("validated boolean immediate"),
                    ) =>
                {
                    self.write(frame, ins.result_register(), value);
                    continue;
                }
                FrameArm::ToNumeric
                    if self.read(frame, ins.register_b()).as_number().is_some() => {
                    self.write(
                        frame,
                        ins.result_register(),
                        self.read(frame, ins.register_b()),
                    );
                    continue;
                }
                // A back edge collects only byte debt (`poll_byte_debt`), so it takes this path while cell debt waits.
                FrameArm::Jump if !self.heap.byte_due() => {
                    pc = ins.jump_target() as usize;
                    if pc <= instruction_pc {
                        self.note_back_edge(&mut cursor);
                    }
                    continue;
                }
                _ => {}
            }
            // GC inside a getter or native operation needs this instruction's root map.
            // SAFETY: `frame` names the active activation, which stays on the frame stack
            // for every iteration that dispatches it.
            debug_assert!(std::ptr::eq(published_pc, &self.frames[frame].pc));
            unsafe { *published_pc = instruction_pc };
            self.frames[frame].pc_is_resume = false;
            // Returns and inline calls produce their step result here rather than through the
            // general `step` dispatch.
            match crate::staged::frame_arm::general(ins.op()) {
                // A call the lane made returned its callee's completion from this frame.
                _ if let Some(value) = returned_in_lane.take() => Ok(StepResult::Return(value)),
                GeneralArm::Return => Ok(StepResult::Return(self.read(frame, ins.register_a()))),
                GeneralArm::Step => self.step(p, frame, ins, &mut pc, allow_inline_calls),
                GeneralArm::Call if allow_inline_calls => {
                    let result = match self.try_inline_reflect_apply(p, frame, ins, pc) {
                        Some(result) => result,
                        None => match self.try_inline_user_call(p, frame, ins, pc) {
                            Some(result) => result,
                            None => self.step(p, frame, ins, &mut pc, allow_inline_calls),
                        },
                    };
                    self.serviced(frame, pc, unsafe { &*executing_residual }, result)
                }
                GeneralArm::Call | GeneralArm::Allocating => {
                    let result = self.step(p, frame, ins, &mut pc, allow_inline_calls);
                    // SAFETY: as for `p` at the top of the loop.
                    self.serviced(frame, pc, unsafe { &*executing_residual }, result)
                }
            }
            };
            // The frame a generator's activation became yields (or an in-place request's awaits)
            // to its caller, as its return completes the call (`materialized_stop`).
            let (step, mut yielded) = self.materialized_stop(pending_calls.last(), frame, pc, step);
            match step {
                Ok(StepResult::Return(mut value)) => {
                    let mut settle_error;
                    loop {
                    if let Some(mut pending) = pending_calls.pop() {
                        debug_assert_eq!(frame, pending.caller + 1);
                        let (completed_frame, mut request_error) = (self.frames.pop().unwrap(), None);
                        // JavaScript in-loop pushes exclude the script root. Wasm calls
                        // share this return path and may enter defined function zero.
                        debug_assert!(
                            completed_frame.function != super::ROOT_FUNCTION_ID
                                || self
                                    .programs
                                    .wasm_function_signature(
                                        completed_frame.program,
                                        completed_frame.function,
                                    )
                                    .is_some()
                        );
                        // A derived constructor's construction yields from its `this` binding,
                        // which its frame holds as it retires (`complete_user_frame` reads the same).
                        let derived_this = matches!(pending.completion, CallCompletion::DerivedConstructor)
                            .then_some(completed_frame.this);
                        match pending.completion.take_async_request() {
                            // SAFETY: as for `p` at the top of the loop; the frame's program.
                            Some(root) => match self.settle_materialized_request(unsafe { &*executing_residual }, completed_frame, root, value, yielded.take()) {
                                Ok(promise) => value = promise,
                                Err(error) => request_error = Some((pending.call_pc, error)),
                            },
                            None => {
                                let stop = if pending.completion.is_generator() { yielded.take() } else { None };
                                value = self.retire_returned_frame(completed_frame, &mut pending.completion, value, stop).0;
                            }
                        }
                        frame = pending.caller;
                        if let Some(saved) = pending.caller_program.take() {
                            executing_program = saved.id;
                            current_program = saved.retained;
                            executing_residual = current_program.as_deref().unwrap_or(entry_program);
                            self.set_active_program(executing_program);
                        }
                        self.active_registers = self.frames[frame].registers.as_mut_ptr();
                        // SAFETY: as for `p` at the top of the loop.
                        settle_error = request_error.take().or_else(|| self.trap_result_error(unsafe { &*executing_residual }, &pending, frame, value));
                        value = self.completed_call_value(&pending.completion, frame, pending.destination, value);
                        if let Some(this) = derived_this {
                            // SAFETY: as for `p` at the top of the loop.
                            match self.derived_construct_value(unsafe { &*executing_residual }, value, this) {
                                Ok(constructed) => value = constructed,
                                Err(error) => settle_error = Some((pending.call_pc, error)),
                            }
                        }
                        if let CallCompletion::ToWasm(result) = pending.completion {
                            // SAFETY: as for `p` at the top of the loop.
                            #[cfg(feature = "wasm")]
                            match self.wasm_import_result(unsafe { &*executing_residual }, value, result) {
                                Ok(converted) => value = converted,
                                Err(error) => settle_error = Some((pending.call_pc, error)),
                            }
                            #[cfg(not(feature = "wasm"))]
                            let _ = result;
                        }
                        if let Some(root) = pending.completion.take_promise() {
                            let promise = self.release_settled_call_root(root);
                            // SAFETY: as for `p` at the top of the loop.
                            let caller_code = unsafe { &*executing_residual };
                            if let Err(error) = self.promise_resolve_value(caller_code, promise, value) {
                                settle_error = Some((pending.call_pc, error));
                            }
                            value = promise;
                        }
                        // A lane's array iteration whose callback this frame ran steps and
                        // finishes on the native's path, from the caller's frame.
                        if let Some(step) = pending.completion.take_array_step() {
                            // SAFETY: as for `p` at the top of the loop.
                            let caller_code = unsafe { &*executing_residual };
                            match self.finish_array_step(caller_code, *step, value) {
                                Ok(result) => value = result,
                                Err(error) => settle_error = Some((pending.call_pc, error)),
                            }
                            self.active_registers = self.frames[frame].registers.as_mut_ptr();
                            if self.active_program != executing_program {
                                self.set_active_program(executing_program);
                            }
                        }
                        pc = self.frames[frame].pc;
                        cursor = pending.caller_cursor.unwrap_or_else(|| {
                            self.general_cursor(
                                executing_program,
                                // SAFETY: as for `p` at the top of the loop.
                                unsafe { &*executing_residual },
                                self.frames[frame].function,
                                fast_lane,
                            )
                        });
                        published_pc = &mut self.frames[frame].pc;
                        drop(pending.call_guard);
                        if let Some((call_pc, error)) = settle_error.take() {
                            unwind_at!(call_pc, error);
                            break;
                        }
                        if pending.returns {
                            continue;
                        }
                        self.write(frame, pending.destination, value);
                        break;
                    }
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    return Ok(FrameOutcome::Complete(value));
                    }
                },
                // Cell debt was serviced in the dispatch, after an op that may allocate (`serviced`).
                Ok(StepResult::Continue) => {}
                Ok(StepResult::PushFrame { destination, call_guard, completion }) => {
                    // A callee of another program became active; the loop runs it until it returns.
                    let callee_program = self.frames[frame + 1].program;
                    let caller_program = if callee_program == executing_program {
                        None
                    } else {
                        let retained = self.programs.get(callee_program).ok_or_else(Self::missing_callee_program)?;
                        let saved = SavedProgram {
                            id: executing_program,
                            retained: current_program.replace(retained),
                        };
                        executing_program = callee_program;
                        executing_residual = current_program.as_deref().unwrap_or(entry_program);
                        Some(saved)
                    };
                    pending_calls.push(PendingGeneralCall {
                        caller: frame,
                        caller_cursor: Some(cursor),
                        call_pc: instruction_pc as u32,
                        destination,
                        completion,
                        returns: ins.returns_from_frame(),
                        call_guard: Some(call_guard),
                        caller_program,
                    });
                    frame += 1;
                    pc = self.frames[frame].pc;
                    cursor = self.general_cursor(
                        callee_program,
                        // SAFETY: as for `p` at the top of the loop.
                        unsafe { &*executing_residual },
                        self.frames[frame].function,
                        fast_lane,
                    );
                    published_pc = &mut self.frames[frame].pc;
                    self.active_registers = self.frames[frame].registers.as_mut_ptr();
                    // An entry is a poll: byte debt collects here; cell debt waits for the callee's first op boundary.
                    self.poll_byte_debt(unsafe { &*executing_residual });
                }
                Ok(StepResult::ResumeFrames(recovered)) => {
                    pending_calls.extend(recovered.calls);
                    frame = self.frames.len() - 1;
                    pc = self.frames[frame].pc;
                    // Static inline bindings admit only this program's plain functions.
                    debug_assert_eq!(self.frames[frame].program, executing_program);
                    cursor = self.general_cursor(executing_program,
                        unsafe { &*executing_residual }, self.frames[frame].function, fast_lane);
                    published_pc = &mut self.frames[frame].pc;
                    self.active_registers = self.frames[frame].registers.as_mut_ptr();
                    if let Some((call_pc, error)) = recovered.failure {
                        unwind_at!(call_pc, error);
                    }
                }
                Ok(StepResult::TailCall) => {
                    let replacement_program = self.frames[frame].program;
                    if replacement_program != executing_program {
                        self.set_active_program(replacement_program);
                        current_program =
                            Some(self.programs.get(replacement_program).ok_or_else(Self::missing_tail_call_program)?);
                    }
                    executing_program = replacement_program;
                    executing_residual = current_program.as_deref().unwrap_or(entry_program);
                    pc = self.frames[frame].pc;
                    cursor = self.general_cursor(
                        replacement_program,
                        current_program.as_deref().unwrap_or(entry_program),
                        self.frames[frame].function,
                        fast_lane,
                    );
                    published_pc = &mut self.frames[frame].pc;
                    self.active_registers = self.frames[frame].registers.as_mut_ptr();
                    // The replacement frame publishes all callee roots before this poll: byte debt only.
                    self.poll_byte_debt(current_program.as_deref().unwrap_or(entry_program));
                }
                Ok(StepResult::Suspend(Suspended::Await { value, destination })) => {
                    // An async call the loop pushed in place (`Vm::try_settling_call`) suspends
                    // here: its frame becomes the continuation its promise's reaction resumes,
                    // and its caller continues with the promise, as `call_user_maybe_async`
                    // makes both for a call it runs apart.
                    if let Some(mut pending) = pending_calls.pop_if(|pending| {
                        pending.completion.is_settling() && !pending.returns && pending.caller + 1 == frame
                    }) {
                        self.frames[frame].pc = pc;
                        self.frames[frame].pc_is_resume = true;
                        let mut suspended = self.frames.pop().expect("the awaiting frame is the innermost");
                        suspended.with_objects = self.with_stack.split_off(suspended.with_base);
                        let promise = self.release_settled_call_root(pending.completion.take_promise().expect("a settling call"));
                        // SAFETY: as for `p` at the top of the loop.
                        let callee_code = unsafe { &*executing_residual };
                        let continuation = self.suspend_activation(
                            FrameRecord::capture(suspended),
                            super::activation::Completion::Await(value),
                            Some(destination),
                            promise,
                        );
                        let enqueued = self.enqueue_async_resume(callee_code, crate::heap::AwaitKind::function(continuation), promise, value);
                        frame = pending.caller;
                        if let Some(saved) = pending.caller_program.take() {
                            executing_program = saved.id;
                            current_program = saved.retained;
                            executing_residual = current_program.as_deref().unwrap_or(entry_program);
                            self.set_active_program(executing_program);
                        }
                        self.active_registers = self.frames[frame].registers.as_mut_ptr();
                        pc = self.frames[frame].pc;
                        cursor = pending.caller_cursor.unwrap_or_else(|| {
                            self.general_cursor(
                                executing_program,
                                // SAFETY: as for `p` at the top of the loop.
                                unsafe { &*executing_residual },
                                self.frames[frame].function,
                                fast_lane,
                            )
                        });
                        published_pc = &mut self.frames[frame].pc;
                        drop(pending.call_guard);
                        match enqueued {
                            Ok(()) => self.write(frame, pending.destination, promise),
                            Err(error) => unwind_at!(pending.call_pc, error),
                        }
                        continue;
                    }
                    debug_assert!(pending_calls.is_empty());
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    return Ok(FrameOutcome::Suspend(Suspended::Await { value, destination }, None));
                }
                Ok(StepResult::Suspend(suspended @ Suspended::Yield { .. })) => {
                    debug_assert!(pending_calls.is_empty());
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    return Ok(FrameOutcome::Suspend(suspended, None));
                }
                Err(error) => unwind_at!(instruction_pc as u32, error),
            }
        }
    }

    /// The error for a Wasm call whose stack-frame storage exceeds the guest stack limit.
    #[cold]
    #[inline(never)]
    fn wasm_call_stack_exhausted() -> JsError {
        JsError::wasm_trap_error(crate::WasmTrap::CallStackExhausted)
    }

    /// Records a rejected native execution and builds the error for guest code at `pc` that native tiers may not run.
    #[cold]
    #[inline(never)]
    fn native_execution_rejected(&mut self, p: &ResidualProgram, frame: usize, pc: usize) -> JsError {
        self.record_native_execution_rejection();
        JsError::validation(format!(
            "native guest code required: {} function {} at {}",
            p.source_name, self.frames[frame].function, pc,
        ))
    }

    /// Records a rejected native execution and builds the error for a dispatch step that needs
    /// the interpreter in a build without dynamic execution.
    #[cold]
    #[inline(never)]
    fn interpreter_unavailable(&mut self) -> JsError {
        self.record_native_execution_rejection();
        JsError::validation("native guest code required: interpreter dispatch is unavailable".into())
    }

    /// The error for a callee whose residual program the VM no longer holds.
    #[cold]
    #[inline(never)]
    fn missing_callee_program() -> JsError {
        JsError::validation("missing callee program".into())
    }

    /// The error for a tail call whose residual program the VM no longer holds.
    #[cold]
    #[inline(never)]
    fn missing_tail_call_program() -> JsError {
        JsError::validation("missing tail-call program".into())
    }

    /// Finds the handler for `error` thrown at `throwing_pc`, popping in-loop activations
    /// that have none; returns the frame and PC to resume at, or the error no frame handles.
    /// Out of line and cold, so the dispatch loop keeps registers for its hot paths.
    #[cold]
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn unwind_general_loop(
        &mut self,
        entry_program: &ResidualProgram,
        mut frame: usize,
        mut pc: usize,
        mut throwing_pc: u32,
        mut error: JsError,
        pending_calls: &mut Vec<PendingGeneralCall>,
        cursor: &mut GeneralCodeCursor,
        executing: &mut ExecutingProgram,
        fast_lane: bool,
    ) -> Result<(usize, usize), JsError> {
        loop {
            // The clone keeps the residual alive for the whole iteration, past any switch of `executing`.
            let retained = executing.retained.clone();
            let p = retained.as_deref().unwrap_or(entry_program);
            let function = self.frames[frame].function as usize;
            match self.exception_handler_target(p, frame, function, throwing_pc, error) {
                Ok(target) => return Ok((frame, target)),
                Err(unhandled) => {
                    // A capture took the frames the pending calls name: the run ends with the chain.
                    if unhandled.is_captured() {
                        return Err(unhandled);
                    }
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    let Some(mut pending) = pending_calls.pop() else {
                        return Err(unhandled);
                    };
                    debug_assert_eq!(frame, pending.caller + 1);
                    let result = Err(unhandled);
                    let mut failed_frame = self.frames.pop().unwrap();
                    self.deactivate_frame(&mut failed_frame, &result);
                    self.pool_frame(failed_frame);
                    frame = pending.caller;
                    pc = self.frames[frame].pc;
                    if let Some(saved) = pending.caller_program.take() {
                        executing.id = saved.id;
                        executing.retained = saved.retained;
                        self.set_active_program(saved.id);
                    }
                    *cursor = pending.caller_cursor.unwrap_or_else(|| {
                        self.general_cursor(
                            executing.id,
                            executing.residual(entry_program),
                            self.frames[frame].function,
                            fast_lane,
                        )
                    });
                    self.active_registers = self.frames[frame].registers.as_mut_ptr();
                    throwing_pc = pending.call_pc;
                    drop(pending.call_guard);
                    error = match result {
                        Err(error) => error,
                        Ok(_) => unreachable!(),
                    };
                    // A generator whose activation became a frame finishes with its throw.
                    if let Some(resume) = pending.completion.take_generator() {
                        self.abandon_materialized_generator_step(resume);
                    }
                    if let Some(step) = pending.completion.take_array_step() {
                        self.abandon_array_step(*step);
                    }
                    // A trap leaving an exported function's frame reaches its JavaScript
                    // caller as a `RuntimeError`, as the native call's (`call_wasm_exported_function`).
                    if matches!(pending.completion, CallCompletion::WasmExport(_)) {
                        error = self.trap_raised_in_javascript(executing.residual(entry_program), error);
                    }
                    // An async callee's or in-place request's abrupt completion settles its
                    // promise, which the call yields; the caller resumes after the call.
                    error = match self.settle_abandoned_call(executing.residual(entry_program), (frame, pending.destination), &mut pending.completion, error) {
                        Ok(()) => return Ok((frame, pc)),
                        Err(next) => next,
                    };
                }
            }
        }
    }

    fn exception_handler_target(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        function: usize,
        throwing_pc: u32,
        error: JsError,
    ) -> Result<usize, JsError> {
        // A captured run's frames are gone and a termination is uncatchable: neither has a handler.
        if error.is_execution_terminated() || error.is_captured() { return Err(error); }
        let error = self.name_non_callable_callee(program, frame, function, throwing_pc, error);
        let wasm = program.kind == crate::bytecode::ProgramKind::Wasm;
        // Wasm catches Wasm exceptions and JavaScript ones (`catch_all`, or
        // `WebAssembly.JSTag`), never a trap or a link failure.
        if wasm && error.wasm_exception().is_none() && !error.is_javascript_exception() {
            return Err(error);
        }
        let handler = program.function(function).handler_at(throwing_pc);
        let Some(handler) = handler else {
            let mut error = error;
            error.capture_source_location(program, function as u32, throwing_pc);
            return Err(error);
        };
        let home = super::handler_entry::HandlerHome::Frame(frame);
        self.unwind_handler_scopes(home, handler);
        if let Some(slot) = handler.slot {
            let value = self.thrown_value_for(program, error);
            self.bind_thrown_value(program, home, slot, value)?;
        }
        Ok(handler.target as usize)
    }

    pub(super) fn captured_with_objects(&self, mut env: Value) -> Vec<Value> {
        if !self.with_scope_entered {
            return Vec::new();
        }
        let mut layers = Vec::new();
        while let Some(crate::heap::EnvironmentRecord { parent, scope, .. }) = self.heap.environment(env) {
            if !scope.with_objects().is_empty() {
                layers.push(scope.with_objects().to_vec());
            }
            env = *parent;
            if env.is_null() {
                break;
            }
        }
        layers.reverse();
        layers.into_iter().flatten().collect()
    }

    #[inline(always)]
    pub(super) fn captured_with_objects_for_function(
        &self,
        env: Value,
        inherited_with_scope: bool,
        program_kind: crate::bytecode::ProgramKind,
    ) -> Vec<Value> {
        if !self.with_scope_entered
            || !inherited_with_scope && program_kind != crate::bytecode::ProgramKind::Eval
        {
            return Vec::new();
        }
        self.captured_with_objects(env)
    }

    /// The contiguous argument registers of a call window.
    #[inline(always)]
    pub(super) fn register_window(
        &self,
        f: usize,
        window: crate::bytecode::RegisterWindow,
    ) -> &[Value] {
        debug_assert!(std::ptr::eq(
            self.active_registers,
            self.frames[f].registers.as_ptr()
        ));
        debug_assert!(
            usize::from(window.base) + usize::from(window.count) <= self.frames[f].registers.len()
        );
        // SAFETY: as for `read`: the window lies in frame `f`'s register file, which
        // `active_registers` names.
        unsafe {
            std::slice::from_raw_parts(
                self.active_registers.add(usize::from(window.base)),
                usize::from(window.count),
            )
        }
    }

    /// An operand's value when reading it can run no guest code and cannot throw: a register
    /// or a constant. Field and local operands go through `resolve_operand`.
    #[inline(always)]
    fn inert_operand(&self, frame: usize, operand: crate::bytecode::Operand) -> Option<Value> {
        match operand.kind() {
            Some(crate::bytecode::OperandKind::Register) => {
                Some(self.read(frame, operand.payload() as Register))
            }
            Some(crate::bytecode::OperandKind::Constant) => {
                self.active_constant(operand.payload() as usize)
            }
            _ => None,
        }
    }

    #[inline(always)]
    pub fn read(&self, f: usize, r: u16) -> Value {
        debug_assert!(std::ptr::eq(
            self.active_registers,
            self.frames[f].registers.as_ptr()
        ));
        debug_assert!(usize::from(r) < self.frames[f].registers.len());
        // SAFETY: register access happens only while dispatching frame `f`, whose register file
        // `active_registers` names (see `run_general_loop`); compiler-issued registers are below
        // the verified register count the file was sized to.
        unsafe { *self.active_registers.add(usize::from(r)) }
    }

    #[inline(always)]
    pub fn write(&mut self, f: usize, r: u16, v: Value) {
        debug_assert!(std::ptr::eq(
            self.active_registers,
            self.frames[f].registers.as_ptr()
        ));
        debug_assert!(usize::from(r) < self.frames[f].registers.len());
        // SAFETY: same active-frame invariant as `read`; mutation stays here.
        unsafe { *self.active_registers.add(usize::from(r)) = v };
    }

    #[inline(always)]
    /// Reads a local whose slot was checked by residual validation.
    ///
    /// # Safety
    /// `frame` must identify an active frame, and `slot` must be less than the
    /// active function's local count. Frame initialization sizes locals to that
    /// count.
    pub unsafe fn read_validated_local(&self, frame: usize, slot: usize) -> Value {
        // SAFETY: the caller establishes that the frame is active and validation
        // established `slot < Function.locals`; invocation setup sizes this frame
        // to that function's local count.
        unsafe { *self.frames.get_unchecked(frame).locals.get_unchecked(slot) }
    }

    #[inline(always)]
    /// Writes a local whose slot was checked by residual validation.
    ///
    /// # Safety
    /// As `read_validated_local`.
    pub(super) unsafe fn write_validated_local(&mut self, frame: usize, slot: usize, value: Value) {
        // SAFETY: as `read_validated_local`.
        unsafe { *self.frames.get_unchecked_mut(frame).locals.get_unchecked_mut(slot) = value; }
    }

}

/// Whether a lane-exit diagnostic reads an element of `instruction`: its decoded element read,
/// of a register base or of a frame local.
#[cfg(feature = "profile-lane-exits")]
fn is_element_read(instruction: WideInstruction) -> bool {
    matches!(
        crate::vm::compiled::decoded_completed(instruction),
        Some(crate::staged::Operation::ElementRead { .. } | crate::staged::Operation::ElementReadOperand { .. })
    )
}

/// The slots of the environment a promotion makes of `locals`: those the `selective` capture list
/// names, moved out and left deleted behind, or, with no list, all of them, which leave the locals
/// empty.
fn captured_slots(locals: &mut Vec<Value>, selective: Option<&[u16]>) -> crate::heap::EnvironmentSlots {
    let Some(captured) = selective else {
        let slots = crate::heap::EnvironmentSlots::from_values(locals.iter().copied());
        locals.clear();
        return slots;
    };
    // `captured` is sorted, so one merge pass over the locals finds its slots.
    let mut pending = captured.iter().copied().peekable();
    crate::heap::EnvironmentSlots::from_values(locals.iter_mut().enumerate().map(|(slot, value)| {
        while pending.next_if(|captured| usize::from(*captured) < slot).is_some() {}
        if pending.next_if(|captured| usize::from(*captured) == slot).is_some() {
            std::mem::replace(value, Value::DELETED)
        } else {
            Value::DELETED
        }
    }))
}
