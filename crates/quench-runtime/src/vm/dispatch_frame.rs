use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn call_this_value(&mut self, this: Value, strict: bool) -> Result<Value, JsError> {
        if strict {
            Ok(this)
        } else if this.is_null() || this.is_undefined() {
            Ok(self.realm.globals)
        } else {
            self.box_object(this)
        }
    }

    pub(super) fn initialize_activation_bindings(
        &mut self,
        frame: &mut Frame,
        arrow: bool,
        new_target: Value,
    ) {
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
        self.call_user_frame_mode(p, id, parent, this, args, context, false)
    }

    pub(super) fn call_user_construct_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<FrameOutcome, JsError> {
        self.call_user_frame_mode(p, id, parent, Value::UNDEFINED, args, context, true)
    }

    fn call_user_frame_mode(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
        capture_constructor_this: bool,
    ) -> Result<FrameOutcome, JsError> {
        self.profile.function(id as usize);
        if p.functions[id as usize].parameter_eval_arguments_error {
            return Err(self
                .syntax_error_result(p, "arguments binding is not allowed in function parameters")
                .expect_err("syntax_error_result must throw"));
        }
        let function = &p.functions[id as usize];
        let mut frame = self.frame_pool.pop().unwrap_or(Frame {
            context: CallContext::Internal,
            original_arguments: vec![],
            program: self.active_program,
            function: 0,
            pc: 0,
            binding_site_pc: None,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            dynamic_bindings: vec![],
            captured: false,
            registers: vec![],
            active_iterators: vec![],
            with_objects: Vec::new(),
            with_base: self.with_stack.len(),
        });
        frame
            .locals
            .resize(function.locals as usize, Value::UNDEFINED);
        frame.locals[function.params as usize..].fill(Value::UNDEFINED);
        let fixed = usize::from(function.params) - usize::from(function.rest);
        for index in 0..fixed {
            frame.locals[index] = args.get(index).copied().unwrap_or(Value::UNDEFINED);
        }
        if function.rest {
            let elements = args.get(fixed..).unwrap_or_default().to_vec();
            frame.locals[fixed] = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                elements: Rc::new(elements),
            });
        }
        if let Some(slot) = function.self_binding_slot {
            frame.locals[usize::from(slot)] = context.callee().unwrap_or(Value::UNDEFINED);
        }
        if let Some(slot) = function.arguments_slot {
            let mapped = function.arguments_are_mapped();
            let arguments = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.object_proto),
                elements: Rc::new(args.to_vec()),
            });
            frame.locals[usize::from(slot)] = arguments;
            self.initialize_arguments_object(arguments, context.callee(), args, mapped)?;
            if mapped {
                let mapping = (0..function.params.min(args.len() as u16)).collect();
                if let Some(object) = self.object_data_mut(arguments) {
                    object.set_arguments_map(mapping);
                }
            }
        }
        self.initialize_frame_invocation(&mut frame, context, args);
        frame.function = id;
        frame.program = self.active_program;
        frame.pc = 0;
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
        self.with_stack
            .extend(self.captured_with_objects_for_function(parent, function, p.kind));
        if id == super::ROOT_FUNCTION_ID {
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
                        .ok_or_else(|| JsError("module import slot is out of bounds".into()))? =
                        value;
                } else {
                    frame.locals[index] = value;
                }
            }
        }
        let new_target = self.construct_target.take().unwrap_or(Value::UNDEFINED);
        self.initialize_activation_bindings(&mut frame, arrow, new_target);
        let register_count = function.registers as usize;
        let run_numeric = numeric_frame_is_safe(function, capture_constructor_this);
        frame.prepare_registers(register_count);
        self.frames.push(frame);
        let frame_index = self.frames.len() - 1;
        if id == super::ROOT_FUNCTION_ID
            && self.programs.is_module(self.frames.last().unwrap().program)
        {
            let environment = self.promote_frame_environment(frame_index);
            self.programs
                .set_module_environment(self.frames[frame_index].program, environment);
        }
        // Numeric dispatch changes the body loop only. Frame activation and cleanup
        // remain shared; unsupported tail-call and suspension completions use general.
        let result = if run_numeric {
            let active_program =
                std::mem::replace(&mut self.active_program, self.frames[frame_index].program);
            let result = self
                .run_frame_numeric(p, frame_index)
                .map(FrameOutcome::Complete);
            self.active_program = active_program;
            result
        } else {
            self.run_frame_general(p, frame_index)
        };
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
                self.frame_pool.push(Self::recycle_frame(frame));
                Ok(outcome)
            }
            FrameOutcome::ConstructComplete { .. } => {
                self.frame_pool.push(Self::recycle_frame(frame));
                Err(JsError(
                    "nested constructor completion escaped its activation".into(),
                ))
            }
            FrameOutcome::Await {
                value, destination, ..
            } => Ok(FrameOutcome::Await {
                value,
                destination,
                frame: Some(frame),
            }),
            FrameOutcome::Yield { .. } => {
                Err(JsError("yield requires generator continuation".into()))
            }
            FrameOutcome::ParameterInitializationComplete => Err(JsError(
                "unexpected generator parameter initialization boundary".into(),
            )),
        }
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
        self.profile.function(id as usize);
        if p.functions[id as usize].parameter_eval_arguments_error {
            return Err(self
                .syntax_error_result(p, "arguments binding is not allowed in function parameters")
                .expect_err("syntax_error_result must throw"));
        }
        let function = &p.functions[id as usize];
        let old = std::mem::replace(
            &mut self.frames[frame_index],
            Frame {
                context: CallContext::Internal,
                original_arguments: vec![],
                program: self.active_program,
                function: 0,
                pc: 0,
                binding_site_pc: None,
                env: Value::NULL,
                this: Value::UNDEFINED,
                locals: vec![],
                dynamic_bindings: vec![],
                captured: false,
                registers: vec![],
                active_iterators: vec![],
                with_objects: Vec::new(),
                with_base: self.with_stack.len(),
            },
        );
        self.with_stack.truncate(old.with_base);
        let mut frame = Self::recycle_frame(old);
        frame
            .locals
            .resize(function.locals as usize, Value::UNDEFINED);
        frame.locals[function.params as usize..].fill(Value::UNDEFINED);
        let fixed = usize::from(function.params) - usize::from(function.rest);
        for index in 0..fixed {
            frame.locals[index] = args.get(index).copied().unwrap_or(Value::UNDEFINED);
        }
        if function.rest {
            let elements = args.get(fixed..).unwrap_or_default().to_vec();
            frame.locals[fixed] = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                elements: Rc::new(elements),
            });
        }
        if let Some(slot) = function.self_binding_slot {
            frame.locals[usize::from(slot)] = context.callee().unwrap_or(Value::UNDEFINED);
        }
        if let Some(slot) = function.arguments_slot {
            let mapped = function.arguments_are_mapped();
            let arguments = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.object_proto),
                elements: Rc::new(args.to_vec()),
            });
            frame.locals[usize::from(slot)] = arguments;
            self.initialize_arguments_object(arguments, context.callee(), args, mapped)?;
            if mapped {
                let mapping = (0..function.params.min(args.len() as u16)).collect();
                if let Some(object) = self.object_data_mut(arguments) {
                    object.set_arguments_map(mapping);
                }
            }
        }
        self.initialize_frame_invocation(&mut frame, context, args);
        frame.program = self.active_program;
        frame.function = id;
        frame.pc = 0;
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
        let new_target = self.construct_target.take().unwrap_or(Value::UNDEFINED);
        self.initialize_activation_bindings(&mut frame, arrow, new_target);
        let register_count = function.registers as usize;
        frame.prepare_registers(register_count);
        self.frames[frame_index] = frame;
        Ok(())
    }

    pub(super) fn initialize_frame_invocation(
        &self,
        frame: &mut Frame,
        context: CallContext,
        args: &[Value],
    ) {
        frame.context = context;
        frame.original_arguments.clear();
        if context
            .callable()
            .is_some_and(|function| !self.function_caller_is_restricted(function))
        {
            frame.original_arguments.extend_from_slice(args);
        }
    }

    pub(super) fn initialize_arguments_object(
        &mut self,
        arguments: Value,
        callable: Option<Value>,
        args: &[Value],
        expose_callee: bool,
    ) -> Result<(), JsError> {
        if let Some(object) = self.object_data_mut(arguments) {
            object.set_arguments_object();
        }
        let length = self.intern_atom("length");
        self.set_property(arguments, length, Value::number(args.len() as f64))?;
        self.set_property_attributes(
            arguments,
            property_key::PropertyKey::string(length),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
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
            self.set_symbol_property(arguments, iterator, self.native_value(Native::ArrayValues))?;
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
        }
        Ok(())
    }

    pub(super) fn promote_frame_environment(&mut self, frame: usize) -> Value {
        if self.frames[frame].captured {
            return self.frames[frame].env;
        }
        let parent = self.frames[frame].env;
        let function = self.frames[frame].function as usize;
        let selective_capture_slots = self
            .programs
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
                if u16::try_from(slot)
                    .is_ok_and(|slot| captured.binary_search(&slot).is_ok())
                {
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
            program: Some(self.frames[frame].program.raw()),
            root_eval_scope: false,
            binding_site_pc: None,
            function: self.frames[frame].function,
            slots: slots.into_boxed_slice().into(),
            dynamic_bindings: std::mem::take(&mut self.frames[frame].dynamic_bindings).into(),
            with_objects: Vec::new(),
        });
        self.frames[frame].env = env;
        self.frames[frame].captured = true;
        env
    }

    pub(super) fn clone_frame_environment(
        &mut self,
        frame: usize,
        fresh: &[u16],
    ) -> Result<(), JsError> {
        if !self.frames[frame].captured {
            return Ok(());
        }
        let source = self.frames[frame].env;
        let slots = self
            .heap
            .clone_environment_slots(source, fresh)
            .ok_or_else(|| JsError("invalid environment clone slots".into()))?;
        let owner = self
            .heap
            .environment_binding_owner(source)
            .ok_or_else(|| JsError("invalid environment clone owner".into()))?;
        let Some(Cell::Environment {
            parent,
            program,
            root_eval_scope,
            binding_site_pc,
            function,
            with_objects,
            ..
        }) = self.heap.get(source)
        else {
            return Err(JsError("invalid environment clone".into()));
        };
        let environment = Cell::Environment {
            parent: *parent,
            program: *program,
            root_eval_scope: *root_eval_scope,
            binding_site_pc: *binding_site_pc,
            function: *function,
            slots,
            dynamic_bindings: crate::heap::EnvironmentBindings::Shared(owner),
            with_objects: with_objects.clone(),
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
            _ => self.with_stack.truncate(frame.with_base),
        }
    }

    pub(super) fn recycle_frame(mut frame: Frame) -> Frame {
        frame.context = CallContext::Internal;
        frame.original_arguments.clear();
        frame.with_objects = Vec::new();
        const RETAINED_VALUES: usize = 256;
        if frame.original_arguments.capacity() > RETAINED_VALUES {
            frame.original_arguments.shrink_to(RETAINED_VALUES);
        }
        if frame.locals.capacity() > RETAINED_VALUES {
            frame.locals.clear();
            frame.locals.shrink_to(RETAINED_VALUES);
        }
        if frame.registers.capacity() > RETAINED_VALUES {
            frame.registers.clear();
            frame.registers.shrink_to(RETAINED_VALUES);
        }
        if frame.dynamic_bindings.capacity() > RETAINED_VALUES {
            frame.dynamic_bindings.clear();
            frame.dynamic_bindings.shrink_to(RETAINED_VALUES);
        } else {
            frame.dynamic_bindings.clear();
        }
        frame.active_iterators.clear();
        frame
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
        self.run_frame_general_until(p, frame, None, initial_error)
    }

    pub(super) fn run_frame_general_until(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        stop_pc: Option<usize>,
        initial_error: Option<JsError>,
    ) -> Result<FrameOutcome, JsError> {
        let entry_program = p;
        let frame_program = self.frames[frame].program;
        let previous_program = std::mem::replace(&mut self.active_program, frame_program);
        let previous_global = self.realm.globals;
        let outcome = (|| {
            let mut current_program: Option<Rc<ResidualProgram>> = None;
            let _stack = if p.kind == crate::bytecode::ProgramKind::Wasm {
                crate::stack::StackGuard::enter()
                    .map_err(|()| JsError::wasm_trap_error(crate::WasmTrap::CallStackExhausted))?
            } else {
                self.enter_stack()?
            };
            let initial_function = self.frames[frame].function as usize;
            let mut pc = self.frames[frame].pc;
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
                let p = current_program.as_deref().unwrap_or(p);
                if stop_pc == Some(pc) {
                    self.frames[frame].pc = pc;
                    return Ok(FrameOutcome::ParameterInitializationComplete);
                }
                let executing_program = self.frames[frame].program;
                let function = self.frames[frame].function as usize;
                let code = &p.functions[function].code;
                let instruction_pc = pc;
                // GC inside a getter or native operation needs this instruction's root map.
                self.frames[frame].pc = instruction_pc;
                // SAFETY: the validated residual program has in-range branch targets
                // and a terminal Return. The frame publishes the active instruction.
                let packed = unsafe { *code.get_unchecked(pc) };
                let ins = if packed.is_wide() {
                    p.functions[function].wide[packed.wide_index()]
                } else {
                    packed.as_wide()
                };
                pc += 1;
                #[cfg(feature = "profile-aggregate")]
                self.profile
                    .opcode(ins.op() as usize, frame, function as u32, instruction_pc);
                #[cfg(not(feature = "profile-aggregate"))]
                self.profile.opcode(ins.op() as usize);
                match self.step(p, frame, ins, &mut pc) {
                    Ok(StepResult::Return(value)) => {
                        self.frames[frame].pc = pc;
                        return Ok(FrameOutcome::Complete(value));
                    }
                    Ok(StepResult::Continue) => {}
                    Ok(StepResult::TailCall) => {
                        if self.frames[frame].program != executing_program {
                            self.active_program = self.frames[frame].program;
                            current_program =
                                Some(self.programs.get(self.frames[frame].program).ok_or_else(
                                    || JsError::validation("missing tail-call program".into()),
                                )?);
                        }
                        pc = self.frames[frame].pc;
                        // The replacement frame publishes all callee roots before this back edge.
                        self.maybe_collect(current_program.as_deref().unwrap_or(entry_program));
                    }
                    Ok(StepResult::Await { value, destination }) => {
                        self.frames[frame].pc = pc;
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
                        self.frames[frame].pc = pc;
                        return Ok(FrameOutcome::Yield {
                            value,
                            destination,
                            delegated_result,
                            frame: None,
                        });
                    }
                    Err(error) => {
                        pc = match self.exception_handler_target(
                            p,
                            frame,
                            function,
                            instruction_pc as u32,
                            error,
                        ) {
                            Ok(target) => target,
                            Err(error) => {
                                self.frames[frame].pc = pc;
                                return Err(error);
                            }
                        };
                    }
                }
            }
        })();
        self.active_program = previous_program;
        self.realm.globals = previous_global;
        outcome
    }

    fn exception_handler_target(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        function: usize,
        throwing_pc: u32,
        error: JsError,
    ) -> Result<usize, JsError> {
        let wasm = program.kind == crate::bytecode::ProgramKind::Wasm;
        if wasm && error.wasm_exception().is_none() {
            return Err(error);
        }
        let handler = program.functions[function]
            .handlers
            .iter()
            .find(|handler| throwing_pc >= handler.start && throwing_pc < handler.end)
            .copied();
        let Some(handler) = handler else {
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
        let mut layers = Vec::new();
        while let Some(Cell::Environment {
            parent,
            with_objects,
            ..
        }) = self.heap.get(env)
        {
            if !with_objects.is_empty() {
                layers.push(with_objects.clone());
            }
            env = *parent;
            if env.is_null() {
                break;
            }
        }
        layers.reverse();
        layers.into_iter().flatten().collect()
    }

    pub(super) fn captured_with_objects_for_function(
        &self,
        env: Value,
        function: &crate::bytecode::Function,
        program_kind: crate::bytecode::ProgramKind,
    ) -> Vec<Value> {
        if !function.inherited_with_scope && program_kind != crate::bytecode::ProgramKind::Eval {
            return Vec::new();
        }
        self.captured_with_objects(env)
    }

    #[inline(always)]
    pub(super) fn read(&self, f: usize, r: u16) -> Value {
        // SAFETY: compiler-issued registers are defined before use; each frame
        // is sized to the verified maximum register before interpretation.
        unsafe {
            *self
                .frames
                .get_unchecked(f)
                .registers
                .get_unchecked(r as usize)
        }
    }

    #[inline(always)]
    pub(super) fn write(&mut self, f: usize, r: u16, v: Value) {
        // SAFETY: same compiler/frame invariant as `read`; mutation stays here.
        unsafe {
            *self
                .frames
                .get_unchecked_mut(f)
                .registers
                .get_unchecked_mut(r as usize) = v;
        }
    }
}

fn numeric_frame_is_safe(
    function: &crate::bytecode::Function,
    capture_constructor_this: bool,
) -> bool {
    if function.dispatch != crate::bytecode::DispatchClass::Numeric
        || capture_constructor_this
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || function.class_field_initializer
    {
        return false;
    }
    !function.strict
        || !function.code.iter().enumerate().any(|(pc, instruction)| {
            instruction.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Call
                && (instruction.returns_from_frame()
                    || function
                        .code
                        .get(pc + 1)
                        .is_some_and(|next| next.op() == Op::Return))
        })
}
