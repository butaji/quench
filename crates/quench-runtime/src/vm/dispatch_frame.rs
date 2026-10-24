use super::*;

/// Values a pooled frame's buffers keep across activations; larger buffers are shrunk.
const RETAINED_FRAME_VALUES: usize = 256;

#[derive(Clone, Copy)]
pub(super) struct GeneralCodeCursor {
    program: ProgramId,
    function: u32,
    code: *const Instr,
    wide: *const WideInstruction,
    /// Fixed-width expansion of `code`; null while the function is cold.
    decoded: *const WideInstruction,
    /// The function's lane view, or null where the lane does not run.
    lane: *const dispatch_fast::LaneView,
}

/// A caller suspended by an in-loop frame push, resumed when its callee completes.
pub(super) struct PendingGeneralCall {
    pub(super) caller: usize,
    /// Caller program/residual, retained across an in-loop call to another program.
    pub(super) caller_program: ProgramId,
    pub(super) caller_residual: *const ResidualProgram,
    /// The caller's cursor, still valid on resume: the residual outlives the loop and a hot
    /// decoding, once built, never moves; a cold caller simply keeps reading packed code.
    /// Lane calls leave it to the general loop, which derives it only if it resumes the caller.
    pub(super) caller_cursor: Option<GeneralCodeCursor>,
    pub(super) call_pc: u32,
    pub(super) destination: Register,
    /// Set for an in-loop `new`; see `StepResult::PushFrame`.
    pub(super) construct_this: Option<Value>,
    /// The call falls into a return of its result: the caller completes with the callee's
    /// value instead of storing it. Sloppy callers keep their frame (it stays visible to
    /// legacy `caller` reflection), so this chains returns rather than replacing frames.
    pub(super) returns: bool,
    /// `None` only for a lane activation materialized past the guest-call budget, which
    /// `WASM_ACTIVATION_LIMIT` bounds instead.
    pub(super) call_guard: Option<crate::stack::GuestCallGuard>,
}

/// Activations the dispatch loop holds before a Wasm call exhausts the call
/// stack. A Wasm call in the loop pushes a heap frame instead of recursing
/// natively, so the native stack guard does not bound it; the guest call
/// depth does.
pub(super) const WASM_ACTIVATION_LIMIT: usize = crate::stack::MAX_GUEST_CALL_DEPTH;

impl GeneralCodeCursor {
    fn new(program_id: ProgramId, program: &ResidualProgram, function: u32) -> Self {
        let function_code = &program.functions[function as usize];
        let decoded = function_code
            .decoded
            .on_entry(&function_code.code, &function_code.wide)
            .map_or(std::ptr::null(), <[WideInstruction]>::as_ptr);
        Self {
            program: program_id,
            function,
            code: function_code.code.as_ptr(),
            wide: function_code.wide.as_ptr(),
            decoded,
            lane: std::ptr::null(),
        }
    }

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

impl<H: Host> Vm<H> {
    /// A code cursor for `function`, with its lane view where the lane runs.
    pub(super) fn general_cursor(
        &self,
        program_id: ProgramId,
        program: &ResidualProgram,
        function: u32,
        lane: bool,
    ) -> GeneralCodeCursor {
        GeneralCodeCursor {
            lane: if lane && self.lane_admits(program) {
                self.lane_view(program_id, function)
            } else {
                std::ptr::null()
            },
            ..GeneralCodeCursor::new(program_id, program, function)
        }
    }

    pub(super) fn call_this_value(&mut self, this: Value, strict: bool) -> Result<Value, JsError> {
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
        self.construct_target = None;
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
            frame.fixed_this = !arrow && !frame.this.is_deleted();
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
        frame.fixed_this = !inherits_this && !frame.this.is_deleted();
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
        match self.call_user_frame(p, id, parent, this, args, context)? {
            FrameOutcome::Complete(value) | FrameOutcome::ConstructComplete { value, .. } => {
                Ok(value)
            }
            FrameOutcome::ParameterInitializationComplete => Err(JsError(
                "parameter initialization escaped a generator activation".into(),
            )),
            FrameOutcome::Await { .. } => Err(JsError("await requires async continuation".into())),
            FrameOutcome::Yield { .. } => {
                Err(JsError("yield requires generator continuation".into()))
            }
        }
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
    pub(super) fn push_wasm_frame(
        &mut self,
        p: &ResidualProgram,
        caller: usize,
        id: u32,
        arguments: crate::bytecode::RegisterWindow,
    ) -> Result<crate::stack::GuestCallGuard, JsError> {
        // An in-loop activation grows the frame stack, not the native stack, so it counts
        // against the live guest-call budget.
        let call_guard = crate::stack::GuestCallGuard::enter()
            .map_err(|()| JsError::wasm_trap_error(crate::WasmTrap::CallStackExhausted))?;
        let function = &p.functions[id as usize];
        let program = self.active_program;
        let with_base = self.with_stack.len();
        let mut frame = self
            .frame_pool
            .pop()
            .unwrap_or_else(|| Box::new(Frame::empty(program, with_base)));
        let caller = &self.frames[caller];
        let arguments_base = usize::from(arguments.base);
        let arguments =
            &caller.registers[arguments_base..arguments_base + usize::from(arguments.count)];
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
        frame.context = CallContext::Internal;
        frame.function = id;
        frame.program = program;
        frame.pc = 0;
        frame.pc_is_resume = true;
        frame.binding_site_pc = None;
        frame.env = caller.env;
        frame.this = Value::UNDEFINED;
        frame.captured = false;
        frame.with_base = with_base;
        frame.fixed_this = false;
        self.frames.push(frame);
        Ok(call_guard)
    }

    /// Activate a Wasm function whose registers already hold `registers`:
    /// a lane activation becoming a frame. Its environment is the caller's.
    pub(super) fn push_wasm_frame_with_registers(
        &mut self,
        p: &ResidualProgram,
        caller: usize,
        id: u32,
        registers: &[Value],
    ) {
        debug_assert_eq!(p.functions[id as usize].locals, 0);
        let program = self.active_program;
        let with_base = self.with_stack.len();
        let mut frame = self
            .frame_pool
            .pop()
            .unwrap_or_else(|| Box::new(Frame::empty(program, with_base)));
        frame.locals.clear();
        frame.registers.clear();
        frame.registers.extend_from_slice(registers);
        frame.initialized_registers = frame.initialized_registers.max(registers.len());
        frame.context = CallContext::Internal;
        frame.function = id;
        frame.program = program;
        frame.pc = 0;
        frame.pc_is_resume = true;
        frame.binding_site_pc = None;
        frame.env = self.frames[caller].env;
        frame.this = Value::UNDEFINED;
        frame.captured = false;
        frame.with_base = with_base;
        frame.fixed_this = false;
        self.frames.push(frame);
    }

    fn empty_frame(&self) -> Box<Frame> {
        Box::new(Frame::empty(self.active_program, self.with_stack.len()))
    }

    /// Starts an activation that the dispatch loop runs in place.
    ///
    /// `args` may borrow the caller's register window: setup only pushes the new activation,
    /// never touching or resizing another frame's register buffer, and frame-stack growth moves
    /// the boxed frames' pointers rather than their buffers.
    pub(super) fn push_general_user_frame(
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
        crate::stack::GuestCallGuard::enter().or_else(|()| Err(self.unwind_exhausted_push(p)))
    }

    /// Pops an in-loop activation the guest-call budget refused and yields the error.
    #[cold]
    #[inline(never)]
    fn unwind_exhausted_push(&mut self, p: &ResidualProgram) -> JsError {
        let outcome = Err(self.stack_exhaustion_error());
        let mut frame = self.frames.pop().unwrap();
        self.deactivate_frame(&mut frame, &outcome);
        self.persist_global_lexical_bindings(p, &frame);
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
            crate::stack::GuestCallGuard::enter().map_err(|()| self.stack_exhaustion_error())?;
        let frame_index = self.activate_user_frame(p, id, parent, this, args, context)?;
        let result = self.run_frame_general(p, frame_index);
        let mut frame = self.frames.pop().unwrap();
        self.deactivate_frame(&mut frame, &result);
        self.persist_global_lexical_bindings(p, &frame);
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
            FrameOutcome::Await {
                value, destination, ..
            } => Ok(FrameOutcome::Await {
                value,
                destination,
                frame: Some(*frame),
            }),
            FrameOutcome::Yield { .. } => {
                Err(JsError("yield requires generator continuation".into()))
            }
            FrameOutcome::ParameterInitializationComplete => Err(JsError(
                "unexpected generator parameter initialization boundary".into(),
            )),
        }
    }

    /// Binds a user function's activation and pushes it on the frame stack, returning its index.
    #[inline(always)]
    fn activate_user_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<usize, JsError> {
        let function = &p.functions[id as usize];
        if id != super::ROOT_FUNCTION_ID && function.plain_activation(&p.atoms) {
            self.activate_plain_frame(p, id, function, parent, this, args, context)
        } else {
            self.activate_general_frame(p, id, parent, this, args, context)
        }
    }

    /// `activate_user_frame` for a function whose static layout describes its whole
    /// activation (`Function::plain_activation`): every local starts undefined because every
    /// parameter lives in a register, `this` is the coerced receiver, and no implicit binding,
    /// rest array or `arguments` object exists.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
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
        let this = self.call_this_value(this, function.strict)?;
        let mut frame = self.pooled_frame();
        let local_count = if function.call_plan(&p.atoms).addresses_locals {
            usize::from(function.locals)
        } else {
            0
        };
        frame.reset_locals_undefined(local_count);
        self.initialize_frame_invocation(&p.atoms, &mut frame, function, context, args);
        frame.function = id;
        frame.program = self.active_program;
        frame.pc = 0;
        frame.pc_is_resume = true;
        frame.binding_site_pc = None;
        frame.env = parent;
        frame.this = this;
        frame.fixed_this = !this.is_deleted();
        frame.captured = false;
        frame.with_base = self.with_stack.len();
        if self.with_scope_entered {
            self.with_stack
                .extend(self.captured_with_objects_for_function(parent, function, p.kind));
        }
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
    pub(super) fn push_script_frame_with_registers(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        this: Value,
        context: CallContext,
        passed: &[Value],
        registers: &[Value],
    ) {
        // The receiver is already coerced and the body binds no `arguments` object, so
        // activation cannot fail; the registers below replace what it placed in them.
        self.activate_user_frame(p, id, env, this, passed, context)
            .expect("a frameless activation's frame activates without error");
        let frame = self
            .frames
            .last_mut()
            .expect("the activation pushed a frame");
        frame.registers.copy_from_slice(registers);
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
        if p.functions[id as usize].parameter_eval_arguments_error {
            return Err(self
                .syntax_error_result(p, "arguments binding is not allowed in function parameters")
                .expect_err("syntax_error_result must throw"));
        }
        let function = &p.functions[id as usize];
        let mut frame = self.pooled_frame();
        self.bind_activation_arguments(&p.atoms, &mut frame, function, context, args)?;
        frame.function = id;
        frame.program = self.active_program;
        frame.pc = 0;
        frame.pc_is_resume = true;
        frame.binding_site_pc = None;
        frame.env = parent;
        let arrow = function.is_arrow;
        let derived = function.derived_constructor;
        let this = if derived {
            Value::DELETED
        } else if arrow {
            self.captured_lexical_this(parent).unwrap_or(this)
        } else {
            this
        };
        frame.this = if derived || arrow {
            this
        } else {
            self.call_this_value(this, function.strict)?
        };
        frame.captured = false;
        if id == super::ROOT_FUNCTION_ID
            && self.programs.is_module(frame.program)
            && let Some(environment) = self.programs.module_environment(frame.program)
        {
            frame.env = environment;
            frame.captured = true;
        }
        frame.with_base = self.with_stack.len();
        if self.with_scope_entered {
            self.with_stack
                .extend(self.captured_with_objects_for_function(parent, function, p.kind));
        }
        if id == super::ROOT_FUNCTION_ID {
            self.bind_module_imports(&mut frame)?;
        }
        let new_target = self
            .take_construction_target(context)
            .unwrap_or(Value::UNDEFINED);
        self.initialize_activation_bindings(
            &mut frame,
            arrow,
            new_target,
            id == super::ROOT_FUNCTION_ID || function.activation_bindings_observable(&p.atoms),
        );
        frame.prepare_registers_for(function);
        frame.bind_register_parameters(function, args);
        frame.initialize_promoted_registers(function, args);
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
        if p.functions[id as usize].parameter_eval_arguments_error {
            return Err(self
                .syntax_error_result(p, "arguments binding is not allowed in function parameters")
                .expect_err("syntax_error_result must throw"));
        }
        let function = &p.functions[id as usize];
        let placeholder = self.pooled_frame();
        let old = std::mem::replace(&mut self.frames[frame_index], placeholder);
        self.with_stack.truncate(old.with_base);
        let mut frame = old;
        Self::recycle_frame(&mut frame);
        self.bind_activation_arguments(&p.atoms, &mut frame, function, context, args)?;
        frame.program = self.active_program;
        frame.function = id;
        frame.pc = 0;
        frame.pc_is_resume = true;
        frame.binding_site_pc = None;
        frame.env = parent;
        let arrow = function.is_arrow;
        let derived = function.derived_constructor;
        let this = if derived {
            Value::DELETED
        } else if arrow {
            self.captured_lexical_this(parent).unwrap_or(this)
        } else {
            this
        };
        frame.this = if derived || arrow || function.strict {
            this
        } else if this.is_null() || this.is_undefined() {
            self.realm.globals
        } else {
            self.box_object(this)?
        };
        frame.captured = false;
        frame.with_base = self.with_stack.len();
        self.with_stack
            .extend(self.captured_with_objects_for_function(parent, function, p.kind));
        let new_target = self
            .take_construction_target(context)
            .unwrap_or(Value::UNDEFINED);
        self.initialize_activation_bindings(
            &mut frame,
            arrow,
            new_target,
            id == super::ROOT_FUNCTION_ID || function.activation_bindings_observable(&p.atoms),
        );
        frame.prepare_registers_for(function);
        frame.bind_register_parameters(function, args);
        frame.initialize_promoted_registers(function, args);
        let placeholder = std::mem::replace(&mut self.frames[frame_index], frame);
        self.frame_pool.push(placeholder);
        Ok(())
    }

    /// Binds an activation's locals to its call: fixed parameters, the rest array, the
    /// function's own name, the `arguments` object and the legacy invocation record.
    #[inline(always)]
    fn bind_activation_arguments(
        &mut self,
        atoms: &crate::bytecode::AtomTable,
        frame: &mut Frame,
        function: &crate::bytecode::Function,
        context: CallContext,
        args: &[Value],
    ) -> Result<(), JsError> {
        let fixed = usize::from(function.params) - usize::from(function.rest);
        // Promoted parameters live in registers; `initialize_promoted_registers` clears their
        // local slots once the frame is set up, so every fixed parameter is copied here. Every
        // other local starts undefined.
        let passed = fixed.min(args.len());
        frame.locals.clear();
        frame.locals.extend_from_slice(&args[..passed]);
        frame
            .locals
            .resize(usize::from(function.locals), Value::UNDEFINED);
        if function.rest {
            frame.locals[fixed] = self.rest_parameter_array(args.get(fixed..).unwrap_or_default());
        }
        if let Some(slot) = function.self_binding_slot {
            frame.locals[usize::from(slot)] = context.callee().unwrap_or(Value::UNDEFINED);
        }
        let forwards_arguments = function.call_plan(atoms).forwards_arguments;
        if let Some(slot) = function.arguments_slot {
            frame.locals[usize::from(slot)] = if forwards_arguments {
                Value::UNMATERIALIZED_ARGUMENTS
            } else {
                self.activation_arguments_object(function, context, args)?
            };
        }
        self.initialize_frame_invocation(atoms, frame, function, context, args);
        if forwards_arguments {
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
        let function = &p.functions[frame.function as usize];
        let slot = usize::from(
            function
                .arguments_slot
                .expect("only an `arguments` binding is unmaterialized"),
        );
        let context = frame.context;
        let args = frame.original_arguments.clone();
        let arguments = self.activation_arguments_object(function, context, &args)?;
        self.frames[f].locals[slot] = arguments;
        Ok(arguments)
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

    /// The rest parameter's array of the arguments past the fixed parameters.
    #[inline(never)]
    fn rest_parameter_array(&mut self, rest: &[Value]) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                Rc::new(rest.to_vec()),
            ),
        })
    }

    /// A new activation's `arguments` object, mapped onto the parameters when the function's
    /// parameter list allows it.
    #[inline(never)]
    fn activation_arguments_object(
        &mut self,
        function: &crate::bytecode::Function,
        context: CallContext,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let mapped = function.arguments_are_mapped();
        let arguments = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.object_proto),
                Rc::new(args.to_vec()),
            ),
        });
        self.initialize_arguments_object(arguments, context.callee(), args, mapped)?;
        if mapped {
            let mapping = (0..function.params.min(args.len() as u16)).collect();
            if let Some(object) = self.object_data_mut(arguments) {
                object.set_arguments_map(mapping);
            }
        }
        Ok(arguments)
    }

    pub(super) fn initialize_frame_invocation(
        &self,
        atoms: &crate::bytecode::AtomTable,
        frame: &mut Frame,
        function: &crate::bytecode::Function,
        context: CallContext,
        args: &[Value],
    ) {
        frame.context = context;
        frame.original_arguments.clear();
        frame.reflected_parameters = 0;
        let plan = function.call_plan(atoms);
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

    pub(super) fn initialize_arguments_object(
        &mut self,
        arguments: Value,
        callable: Option<Value>,
        args: &[Value],
        expose_callee: bool,
    ) -> Result<(), JsError> {
        let template_key = (self.realm.globals, expose_callee);
        if let Some(template) = self
            .realm
            .intrinsics
            .arguments_objects
            .get(&template_key)
            .copied()
        {
            let anchor = self
                .object_data(template.anchor)
                .expect("arguments template anchor is an object");
            let shape = anchor.shape();
            let iterator = self
                .heap
                .property_get(anchor, template.iterator_slot)
                .expect("arguments template has an iterator method");
            let mut properties = [Value::UNDEFINED; ARGUMENTS_OBJECT_PROPERTY_COUNT];
            properties[template.length_slot] = Value::number(args.len() as f64);
            properties[template.callee_slot] = if expose_callee {
                callable.ok_or_else(|| {
                    JsError::validation("arguments object requires callable identity".into())
                })?
            } else {
                Value::UNDEFINED
            };
            properties[template.iterator_slot] = iterator;
            self.object_data_mut(arguments)
                .expect("arguments object is an array")
                .set_arguments_object();
            self.heap
                .initialize_object_properties(arguments, shape, &properties);
            return Ok(());
        }
        if let Some(object) = self.object_data_mut(arguments) {
            object.set_arguments_object();
        }
        let length = self.intern_atom("length");
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
        if let Some(iterator) = self.well_known_symbols.get("iterator").copied() {
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
        let Some(object) = self.object_data(arguments) else {
            return;
        };
        let proto = object.proto();
        let Some(iterator_method) = self.heap.property_get(object, iterator_slot) else {
            return;
        };
        properties[iterator_slot] = iterator_method;
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

    pub(super) fn promote_frame_environment(&mut self, frame: usize) -> Value {
        if self.frames[frame].captured {
            return self.frames[frame].env;
        }
        let parent = self.frames[frame].env;
        let function = self.frames[frame].function as usize;
        let selective_capture_slots =
            self.programs
                .get(self.frames[frame].program)
                .and_then(|program| {
                    program
                        .functions
                        .get(function)
                        .and_then(|function| function.selective_capture_slots.clone())
                });
        let slots = if let Some(captured) = selective_capture_slots {
            let mut slots = self.frames[frame].locals.clone();
            for slot in 0..slots.len() {
                if u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok()) {
                    self.frames[frame].locals[slot] = Value::DELETED;
                } else {
                    slots[slot] = Value::DELETED;
                }
            }
            slots
        } else {
            std::mem::take(&mut self.frames[frame].locals)
        };
        let env = self.heap.alloc(Cell::Environment {
            parent,
            function: self.frames[frame].function,
            slots: slots.into_boxed_slice().into(),
            scope: Box::new(crate::heap::EnvironmentScope {
                program: Some(self.frames[frame].program.raw()),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: std::mem::take(&mut self.frames[frame].dynamic_bindings).into(),
                with_objects: Box::default(),
            }),
        });
        self.frames[frame].env = env;
        self.frames[frame].captured = true;
        env
    }

    pub(super) fn clone_frame_environment(
        &mut self,
        frame: usize,
        fresh: &[u16],
        reset: &[u16],
    ) -> Result<(), JsError> {
        if !self.frames[frame].captured {
            return Ok(());
        }
        let source = self.frames[frame].env;
        let slots = self
            .heap
            .clone_environment_slots(source, fresh, reset)
            .ok_or_else(|| JsError("invalid environment clone slots".into()))?;
        let owner = self
            .heap
            .environment_binding_owner(source)
            .ok_or_else(|| JsError("invalid environment clone owner".into()))?;
        let Some(Cell::Environment {
            parent,
            function,
            scope,
            ..
        }) = self.heap.get(source)
        else {
            return Err(JsError("invalid environment clone".into()));
        };
        let environment = Cell::Environment {
            parent: *parent,
            function: *function,
            slots,
            scope: Box::new(crate::heap::EnvironmentScope {
                program: scope.program,
                root_eval_scope: scope.root_eval_scope,
                binding_site_pc: scope.binding_site_pc,
                dynamic_bindings: crate::heap::EnvironmentBindings::Shared(owner),
                with_objects: scope.with_objects.clone(),
            }),
        };
        let env = self.heap.alloc(environment);
        self.frames[frame].env = env;
        Ok(())
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
            Ok(FrameOutcome::Await { .. } | FrameOutcome::Yield { .. }) => {
                frame.with_objects = self.with_stack.split_off(frame.with_base);
            }
            _ => self.finish_frame_scopes(frame),
        }
    }

    /// Drops the `with` scopes a finished (not suspended) activation entered.
    #[inline(always)]
    pub(super) fn finish_frame_scopes(&mut self, frame: &Frame) {
        self.with_stack.truncate(frame.with_base);
    }

    /// Returns a finished activation's buffers to the pool without copying the frame again.
    /// Moves a detached activation back into pooled frame storage for the frame stack.
    pub(super) fn adopt_frame(&mut self, frame: Frame) -> Box<Frame> {
        match self.frame_pool.pop() {
            Some(mut slot) => {
                *slot = frame;
                slot
            }
            None => Box::new(frame),
        }
    }

    /// Return a Wasm frame the lane pushed to the pool. A Wasm activation
    /// touches only its locals and registers and runs no global lexical
    /// declarations, so only its register storage needs bounding.
    #[inline(always)]
    pub(super) fn retire_wasm_frame(&mut self) {
        let mut frame = self.frames.pop().expect("pending frame is active");
        self.with_stack.truncate(frame.with_base);
        if frame.registers.capacity() > RETAINED_FRAME_VALUES {
            frame.registers.clear();
            frame.registers.shrink_to(RETAINED_FRAME_VALUES);
            frame.initialized_registers = 0;
        }
        self.frame_pool.push(frame);
    }

    pub(super) fn pool_frame(&mut self, frame: impl Into<Box<Frame>>) {
        let mut frame = frame.into();
        Self::recycle_frame(&mut frame);
        self.frame_pool.push(frame);
    }

    /// Empties a finished frame's appendable buffers and drops its callee for reuse, bounding
    /// what the buffers retain. Every activation sets legacy argument reflection and
    /// `fixed_this` itself.
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
        for buffer in [&mut frame.locals, &mut frame.registers] {
            if buffer.capacity() > RETAINED_FRAME_VALUES {
                buffer.clear();
                buffer.shrink_to(RETAINED_FRAME_VALUES);
            }
        }
        frame.initialized_registers = frame.initialized_registers.min(frame.registers.len());
        frame.dynamic_bindings.shrink_to(RETAINED_FRAME_VALUES);
    }

    pub(super) fn run_frame_general(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<FrameOutcome, JsError> {
        self.run_frame_general_with_error(p, frame, None)
    }

    pub(super) fn run_frame_general_with_error(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        initial_error: Option<JsError>,
    ) -> Result<FrameOutcome, JsError> {
        self.run_frame_general_until::<false>(p, frame, None, initial_error)
    }

    /// Runs the general loop. `STOPS` monomorphizes the parameter-initialization variant, the
    /// only caller with a `stop_pc`, so ordinary dispatch carries no per-instruction stop test.
    pub(super) fn run_frame_general_until<const STOPS: bool>(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        stop_pc: Option<usize>,
        initial_error: Option<JsError>,
    ) -> Result<FrameOutcome, JsError> {
        debug_assert!(STOPS || stop_pc.is_none());
        let frame_program = self.frames[frame].program;
        let allow_inline_calls = stop_pc.is_none() && initial_error.is_none();
        let previous_program = self.replace_active_program(frame_program);
        let previous_global = self.realm.globals;
        // A nested loop leaves the caller's register file active again.
        let caller_registers = self.active_registers;
        let outcome =
            self.run_general_loop::<STOPS>(p, frame, stop_pc, initial_error, allow_inline_calls);
        self.active_registers = caller_registers;
        self.set_active_program(previous_program);
        self.switch_realm_global(previous_global);
        outcome
    }

    /// The general dispatch loop of `run_frame_general_until`, whose caller restores the active
    /// program and realm around it.
    #[inline(always)]
    fn run_general_loop<const STOPS: bool>(
        &mut self,
        entry_program: &ResidualProgram,
        frame: usize,
        stop_pc: Option<usize>,
        initial_error: Option<JsError>,
        allow_inline_calls: bool,
    ) -> Result<FrameOutcome, JsError> {
        let p = entry_program;
        let mut current_program: Option<Rc<ResidualProgram>> = None;
        // Cross-program in-loop frames keep these residuals alive while their raw cursors and
        // pending caller pointers are active.
        let mut retained_programs: Vec<Rc<ResidualProgram>> = Vec::new();
        // The residual being executed: the entry program, or the one `current_program`
        // retains after a cross-program tail call. Updated only where that switch happens.
        let mut executing_residual: *const ResidualProgram = entry_program;
        let mut frame = frame;
        let mut pending_calls: Vec<PendingGeneralCall> = Vec::new();
        let _stack = if p.kind == crate::bytecode::ProgramKind::Wasm {
            crate::stack::StackGuard::enter()
                .map_err(|()| JsError::wasm_trap_error(crate::WasmTrap::CallStackExhausted))?
        } else {
            self.enter_stack()?
        };
        // Parameter-initialization stops and instruction profiling observe
        // every PC, so they run the general path only.
        let fast_lane = !STOPS
            && stop_pc.is_none()
            && !cfg!(any(
                feature = "profile-aggregate",
                feature = "profile-memory"
            ));
        let initial_function = self.frames[frame].function as usize;
        let mut executing_program = self.frames[frame].program;
        let mut cursor = self.general_cursor(
            executing_program,
            entry_program,
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
        loop {
            // SAFETY: `executing_residual` points at `entry_program`, borrowed for the whole
            // call, or at the residual `current_program` keeps alive until it is replaced,
            // and it is re-derived at that replacement.
            let p = unsafe { &*executing_residual };
            if let Err(error) = self.check_execution_interrupt() {
                return self
                    .unwind_general_loop(
                        p,
                        frame,
                        pc,
                        pc as u32,
                        error,
                        &mut pending_calls,
                        &mut cursor,
                        &mut executing_program,
                        &mut executing_residual,
                        fast_lane,
                    )
                    .map(|_| unreachable!("host termination cannot select a guest handler"));
            }
            if Self::lane_runs(cursor.lane, pc) {
                pc = self.run_fast_lane(
                    p,
                    cursor.program,
                    cursor.lane,
                    &mut frame,
                    pc,
                    &mut pending_calls,
                    allow_inline_calls,
                );
                // Lane calls and returns can leave a different activation, or a new one in the
                // same frame slot; the active frame's PC slot and register file follow it.
                published_pc = &mut self.frames[frame].pc;
                self.active_registers = self.frames[frame].registers.as_mut_ptr();
                if self.frames[frame].function != cursor.function {
                    cursor = self.general_cursor(
                        cursor.program,
                        p,
                        self.frames[frame].function,
                        fast_lane,
                    );
                }
            }
            if STOPS && stop_pc == Some(pc) {
                self.frames[frame].pc = pc;
                self.frames[frame].pc_is_resume = true;
                return Ok(FrameOutcome::ParameterInitializationComplete);
            }
            debug_assert_eq!(cursor.program, self.frames[frame].program);
            debug_assert_eq!(cursor.function, self.frames[frame].function);
            let instruction_pc = pc;
            #[cfg(feature = "profile-memory")]
            self.heap.set_memory_allocation_site(
                cursor.program.raw(),
                cursor.function,
                instruction_pc,
            );
            let ins = cursor.instruction(pc);
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
            match ins.op() {
                Op::LoadLocalPlain => {
                    // SAFETY: validated bytecode bounds the slot by Function.locals.
                    let value = unsafe { self.read_validated_local(frame, ins.local_slot()) };
                    self.write(frame, ins.result_register(), value);
                    continue;
                }
                Op::StoreLocalPlain => {
                    let value = self.read(frame, ins.register_a());
                    // SAFETY: validated bytecode bounds the slot by Function.locals.
                    unsafe { self.write_validated_local(frame, ins.local_slot(), value) };
                    if let Some(register) = ins.optional_register_b() {
                        self.write(frame, register, value);
                    }
                    continue;
                }
                Op::Move => {
                    self.write(
                        frame,
                        ins.result_register(),
                        self.read(frame, ins.register_b()),
                    );
                    continue;
                }
                Op::LoadConst => {
                    if let Some(value) = self.active_constant(ins.constant_index()) {
                        self.write(frame, ins.result_register(), value);
                        continue;
                    }
                }
                Op::LoadCapture => {
                    if let Some(value) =
                        self.cached_direct_capture(frame, ins.capture_depth(), ins.capture_slot())
                    {
                        self.write(frame, ins.result_register(), value);
                        continue;
                    }
                }
                Op::JumpFalse | Op::JumpTrue => {
                    // ToBoolean runs no guest code.
                    let value = self.read(frame, ins.register_a());
                    let truthy = self.truthy(value);
                    #[cfg(feature = "profile-aggregate")]
                    self.profile
                        .branch_value(value.profile_kind() as usize, truthy);
                    if truthy == (ins.op() == Op::JumpTrue) {
                        pc = ins.jump_target() as usize;
                    }
                    continue;
                }
                Op::JumpBinaryFalse => {
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
                        }
                        continue;
                    }
                }
                // A cached own-data store writes a slot in place: it neither allocates nor throws.
                Op::SetField | Op::SetFieldStrict => {
                    if self.cached_field_write_hit(
                        p,
                        self.read(frame, ins.register_b()),
                        ins.atom_index(),
                        self.read(frame, ins.register_a()),
                        ins.cache_site_index(),
                    ) {
                        continue;
                    }
                }
                // `this.x = v` with an authoritative `this` is the same cached write.
                Op::SetThisField | Op::SetThisFieldStrict if self.frames[frame].fixed_this => {
                    if self.cached_field_write_hit(
                        p,
                        self.frames[frame].this,
                        ins.atom_index(),
                        self.read(frame, ins.register_a()),
                        ins.cache_site_index(),
                    ) {
                        continue;
                    }
                }
                // Number arithmetic on register or constant operands runs no guest code,
                // allocates nothing and cannot throw.
                Op::Binary | Op::NumericAdd | Op::NumericMultiply
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
                            Op::StoreLocalPlain as usize,
                        );
                    } else {
                        self.write(frame, ins.result_register(), value);
                    }
                    continue;
                }
                // Equality decided without conversion, and instanceof reduced to a prototype
                // walk, run no guest code either.
                Op::Binary
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
                Op::GetField
                    if ins.result_flags() == 0
                        && let Some(crate::bytecode::FieldLookup::Atom {
                            atom,
                            base,
                            cache_site,
                        }) = ins.field_lookup()
                        && let Some(object) = match base.register_index() {
                            Some(register) => Some(self.read(frame, register)),
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
                Op::GetIndex
                    if let (Some(base), Some(key)) = (
                        self.inert_operand(frame, ins.operand_b()),
                        self.inert_operand(frame, ins.operand_c()),
                    ) && let Some(value) = self.dense_array_element(base, key) =>
                {
                    self.write(frame, ins.result_register(), value);
                    continue;
                }
                Op::SetIndex => {
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
                Op::IncDec
                    if let Some(value) = super::dispatch::number_increment(
                        self.read(frame, ins.register_b()),
                        ins.boolean_flag().expect("validated boolean immediate"),
                    ) =>
                {
                    self.write(frame, ins.result_register(), value);
                    continue;
                }
                Op::ToNumeric if self.read(frame, ins.register_b()).as_number().is_some() => {
                    self.write(
                        frame,
                        ins.result_register(),
                        self.read(frame, ins.register_b()),
                    );
                    continue;
                }
                // A back edge collects only once the allocation threshold is reached.
                Op::Jump if !self.heap.should_collect() => {
                    pc = ins.jump_target() as usize;
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
            let step = match ins.op() {
                Op::Return => Ok(StepResult::Return(self.read(frame, ins.register_a()))),
                Op::Call | Op::CallArray if allow_inline_calls => {
                    match self.try_inline_reflect_apply(p, frame, ins, pc) {
                        Some(result) => result,
                        None => match self.try_inline_user_call(p, frame, ins, pc) {
                            Some(result) => result,
                            None => self.step(p, frame, ins, &mut pc, allow_inline_calls),
                        },
                    }
                }
                Op::CallMethod if allow_inline_calls => {
                    match self.try_inline_cached_method_call(p, frame, ins, pc) {
                        Some(result) => result,
                        None => self.step(p, frame, ins, &mut pc, allow_inline_calls),
                    }
                }
                _ => self.step(p, frame, ins, &mut pc, allow_inline_calls),
            };
            match step {
                Ok(StepResult::Return(mut value)) => loop {
                    if let Some(pending) = pending_calls.pop() {
                        debug_assert_eq!(frame, pending.caller + 1);
                        let completed_frame = self.frames.pop().unwrap();
                        self.finish_frame_scopes(&completed_frame);
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
                        self.pool_frame(completed_frame);
                        frame = pending.caller;
                        if pending.caller_program != executing_program {
                            self.set_active_program(pending.caller_program);
                            executing_program = pending.caller_program;
                            executing_residual = pending.caller_residual;
                        }
                        // SAFETY: the entry residual is borrowed for this loop; switched
                        // residuals are retained in `retained_programs` or `current_program`.
                        let caller_residual = unsafe { &*executing_residual };
                        self.active_registers = self.frames[frame].registers.as_mut_ptr();
                        // [[Construct]] of an ordinary constructor yields its returned object,
                        // otherwise the receiver it allocated.
                        value = match pending.construct_this {
                            Some(this) if !self.is_object_like(value) => this,
                            _ => value,
                        };
                        pc = self.frames[frame].pc;
                        cursor = pending.caller_cursor.unwrap_or_else(|| {
                            self.general_cursor(
                                executing_program,
                                caller_residual,
                                self.frames[frame].function,
                                fast_lane,
                            )
                        });
                        published_pc = &mut self.frames[frame].pc;
                        drop(pending.call_guard);
                        if pending.returns {
                            continue;
                        }
                        self.write(frame, pending.destination, value);
                        break;
                    }
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    return Ok(FrameOutcome::Complete(value));
                },
                Ok(StepResult::Continue) => {}
                Ok(StepResult::PushFrame {
                    destination,
                    call_guard,
                    construct_this,
                }) => {
                    pending_calls.push(PendingGeneralCall {
                        caller: frame,
                        caller_program: executing_program,
                        caller_residual: executing_residual,
                        caller_cursor: Some(cursor),
                        call_pc: instruction_pc as u32,
                        destination,
                        construct_this,
                        returns: ins.returns_from_frame(),
                        call_guard: Some(call_guard),
                    });
                    frame += 1;
                    pc = self.frames[frame].pc;
                    let callee_program = self.frames[frame].program;
                    if callee_program != executing_program {
                        let residual = self.programs.get(callee_program).ok_or_else(|| {
                            JsError::validation("missing in-loop call program".into())
                        })?;
                        executing_residual = Rc::as_ptr(&residual);
                        retained_programs.push(residual);
                        executing_program = callee_program;
                    }
                    // SAFETY: the residual is the borrowed entry program or is retained above.
                    let callee_residual = unsafe { &*executing_residual };
                    cursor = self.general_cursor(
                        callee_program,
                        callee_residual,
                        self.frames[frame].function,
                        fast_lane,
                    );
                    published_pc = &mut self.frames[frame].pc;
                    self.active_registers = self.frames[frame].registers.as_mut_ptr();
                }
                Ok(StepResult::TailCall) => {
                    let replacement_program = self.frames[frame].program;
                    if replacement_program != executing_program {
                        self.set_active_program(replacement_program);
                        if let Some(previous) = current_program.take() {
                            retained_programs.push(previous);
                        }
                        current_program =
                            Some(self.programs.get(replacement_program).ok_or_else(|| {
                                JsError::validation("missing tail-call program".into())
                            })?);
                    }
                    executing_program = replacement_program;
                    executing_residual = current_program.as_deref().unwrap_or(entry_program);
                    pc = self.frames[frame].pc;
                    cursor = self.general_cursor(
                        replacement_program,
                        // SAFETY: `current_program` retains the switched tail-call residual.
                        unsafe { &*executing_residual },
                        self.frames[frame].function,
                        fast_lane,
                    );
                    published_pc = &mut self.frames[frame].pc;
                    self.active_registers = self.frames[frame].registers.as_mut_ptr();
                    // The replacement frame publishes all callee roots before this back edge.
                    // SAFETY: `current_program` retains the switched tail-call residual.
                    self.maybe_collect(unsafe { &*executing_residual });
                }
                Ok(StepResult::Await { value, destination }) => {
                    debug_assert!(pending_calls.is_empty());
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    return Ok(FrameOutcome::Await {
                        value,
                        destination,
                        frame: None,
                    });
                }
                Ok(StepResult::Yield {
                    value,
                    destination,
                    delegated_result,
                }) => {
                    debug_assert!(pending_calls.is_empty());
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    return Ok(FrameOutcome::Yield {
                        value,
                        destination,
                        delegated_result,
                        frame: None,
                    });
                }
                Err(error) => {
                    (frame, pc) = self.unwind_general_loop(
                        p,
                        frame,
                        pc,
                        instruction_pc as u32,
                        error,
                        &mut pending_calls,
                        &mut cursor,
                        &mut executing_program,
                        &mut executing_residual,
                        fast_lane,
                    )?;
                    published_pc = &mut self.frames[frame].pc;
                }
            }
        }
    }

    /// Finds the handler for `error` thrown at `throwing_pc`, popping in-loop activations
    /// that have none; returns the frame and PC to resume at, or the error no frame handles.
    /// Out of line and cold, so the dispatch loop keeps registers for its hot paths.
    #[cold]
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn unwind_general_loop(
        &mut self,
        p: &ResidualProgram,
        mut frame: usize,
        mut pc: usize,
        mut throwing_pc: u32,
        mut error: JsError,
        pending_calls: &mut Vec<PendingGeneralCall>,
        cursor: &mut GeneralCodeCursor,
        executing_program: &mut ProgramId,
        executing_residual: &mut *const ResidualProgram,
        fast_lane: bool,
    ) -> Result<(usize, usize), JsError> {
        let mut active_residual = p as *const ResidualProgram;
        loop {
            let function = self.frames[frame].function as usize;
            // SAFETY: the active residual is the entry residual, a retained in-loop residual,
            // or the current tail-call residual; all remain alive for the dispatch loop.
            let active_program = unsafe { &*active_residual };
            match self.exception_handler_target(active_program, frame, function, throwing_pc, error)
            {
                Ok(target) => return Ok((frame, target)),
                Err(unhandled) => {
                    self.frames[frame].pc = pc;
                    self.frames[frame].pc_is_resume = true;
                    let Some(pending) = pending_calls.pop() else {
                        return Err(unhandled);
                    };
                    debug_assert_eq!(frame, pending.caller + 1);
                    let result = Err(unhandled);
                    let mut failed_frame = self.frames.pop().unwrap();
                    self.deactivate_frame(&mut failed_frame, &result);
                    self.persist_global_lexical_bindings(active_program, &failed_frame);
                    frame = pending.caller;
                    pc = self.frames[frame].pc;
                    *executing_program = pending.caller_program;
                    active_residual = pending.caller_residual;
                    *executing_residual = active_residual;
                    self.set_active_program(*executing_program);
                    // SAFETY: caller residuals are retained by the loop.
                    let caller_residual = unsafe { &*active_residual };
                    *cursor = pending.caller_cursor.unwrap_or_else(|| {
                        self.general_cursor(
                            *executing_program,
                            caller_residual,
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
        if error.is_execution_terminated() {
            return Err(error);
        }
        let wasm = program.kind == crate::bytecode::ProgramKind::Wasm;
        if wasm && error.wasm_exception().is_none() {
            return Err(error);
        }
        let handler = program.functions[function].handler_at(throwing_pc);
        let Some(handler) = handler else {
            let mut error = error;
            error.capture_source_location(program, function as u32, throwing_pc);
            return Err(error);
        };
        let with_depth = self.frames[frame].with_base + usize::from(handler.with_depth);
        self.with_stack.truncate(with_depth);
        if let Some(slot) = handler.slot {
            let value = self.thrown_value_for(program, error);
            if wasm {
                self.frames[frame].locals[usize::from(slot)] = value;
            } else {
                self.initialize_handler_binding(program, frame, slot, value)?;
            }
        }
        Ok(handler.target as usize)
    }

    pub(super) fn captured_with_objects(&self, mut env: Value) -> Vec<Value> {
        if !self.with_scope_entered {
            return Vec::new();
        }
        let mut layers = Vec::new();
        while let Some(Cell::Environment { parent, scope, .. }) = self.heap.get(env) {
            if !scope.with_objects.is_empty() {
                layers.push(scope.with_objects.to_vec());
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
        function: &crate::bytecode::Function,
        program_kind: crate::bytecode::ProgramKind,
    ) -> Vec<Value> {
        if !self.with_scope_entered
            || !function.inherited_with_scope && program_kind != crate::bytecode::ProgramKind::Eval
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
    pub(super) fn read(&self, f: usize, r: u16) -> Value {
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
    pub(super) fn write(&mut self, f: usize, r: u16, v: Value) {
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
    pub(super) unsafe fn read_validated_local(&self, frame: usize, slot: usize) -> Value {
        // SAFETY: the caller establishes that the frame is active and validation
        // established `slot < Function.locals`; invocation setup sizes this frame
        // to that function's local count.
        unsafe { *self.frames.get_unchecked(frame).locals.get_unchecked(slot) }
    }

    #[inline(always)]
    /// Writes a local whose slot was checked by residual validation.
    ///
    /// # Safety
    /// `frame` must identify an active frame, and `slot` must be less than the
    /// active function's local count. Frame initialization sizes locals to that
    /// count.
    pub(super) unsafe fn write_validated_local(&mut self, frame: usize, slot: usize, value: Value) {
        // SAFETY: the caller establishes that the frame is active and validation
        // established `slot < Function.locals`; invocation setup sizes this frame
        // to that function's local count.
        unsafe {
            *self
                .frames
                .get_unchecked_mut(frame)
                .locals
                .get_unchecked_mut(slot) = value;
        }
    }
}
