use super::*;
use crate::bytecode::MAPPED_ARGUMENTS_BIT;

pub(super) fn is_derived_constructor(function: &crate::bytecode::Function) -> bool {
    function.derived_constructor
        || function
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::SuperCallCheck)
        || function
            .wide
            .iter()
            .any(|instruction| instruction.op() == Op::SuperCallCheck)
}

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

    #[inline(never)]
    pub(super) fn call_user(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match self.call_user_frame(p, id, parent, this, args)? {
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
    ) -> Result<FrameOutcome, JsError> {
        self.call_user_frame_mode(p, id, parent, this, args, false)
    }

    pub(super) fn call_user_construct_frame(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        args: &[Value],
    ) -> Result<FrameOutcome, JsError> {
        self.call_user_frame_mode(p, id, parent, Value::UNDEFINED, args, true)
    }

    fn call_user_frame_mode(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
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
            program: self.active_program,
            function: 0,
            pc: 0,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            dynamic_bindings: vec![],
            captured: false,
            registers: vec![],
            active_iterators: vec![],
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
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(elements),
            });
        }
        if let Some(slot) = function
            .local_atoms
            .iter()
            .position(|atom| self.atom_name(*atom).contains("\0rqj:self-binding:"))
        {
            frame.locals[slot] = self.function_values[&(self.active_program, id)]
                .iter()
                .rev()
                .find_map(|(closure_env, value)| (*closure_env == parent).then_some(*value))
                .unwrap_or(Value::UNDEFINED);
        }
        if let Some(encoded_slot) = function.arguments_slot {
            let mapped = encoded_slot & MAPPED_ARGUMENTS_BIT != 0;
            let slot = encoded_slot & !MAPPED_ARGUMENTS_BIT;
            let arguments = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.object_proto),
                elements: Rc::new(args.to_vec()),
            });
            frame.locals[usize::from(slot)] = arguments;
            self.initialize_arguments_object(p, arguments, id, parent, args, mapped)?;
            if mapped {
                let mapping = (0..function.params.min(args.len() as u16)).collect();
                if let Some(object) = self.object_data_mut(arguments) {
                    object.arguments_map = Some(mapping);
                }
            }
        }
        frame.function = id;
        frame.program = self.active_program;
        frame.pc = 0;
        frame.env = parent;
        let arrow = function
            .name
            .is_some_and(|atom| self.atom_name(atom) == "\0rqj:arrow");
        let derived = is_derived_constructor(function);
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
        frame.with_base = self.with_stack.len();
        self.with_stack.extend(self.captured_with_objects(parent));
        if id == super::ROOT_FUNCTION_ID {
            for &(slot, import) in self.programs.module_imports(frame.program) {
                let value = match import {
                    super::program_store::ModuleImport::Value(value)
                    | super::program_store::ModuleImport::Binding(_, _, value) => value,
                };
                let Some(local) = frame.locals.get_mut(usize::from(slot)) else {
                    return Err(JsError("module import slot is out of bounds".into()));
                };
                *local = value;
            }
        }
        let new_target_atom = self.intern_atom("\0rqj:new-target");
        if !arrow {
            frame.dynamic_bindings.push((
                new_target_atom,
                self.construct_target.unwrap_or(Value::UNDEFINED),
            ));
        }
        // Arrows resolve `new.target` through their captured environment;
        // ordinary calls own a fresh binding, and constructor calls own the
        // active target. Derived `super()` establishes its own constructor call.
        self.construct_target = None;
        let lexical_this_atom = self.intern_atom("\0rqj:lexical-this");
        frame.dynamic_bindings.push((lexical_this_atom, frame.this));
        if !arrow {
            let super_called_atom = self.intern_atom("\0rqj:super-called");
            frame
                .dynamic_bindings
                .push((super_called_atom, Value::FALSE));
        }
        let register_count = function.registers as usize;
        if frame.registers.capacity() < register_count {
            frame
                .registers
                .reserve_exact(register_count - frame.registers.len());
        }
        // SAFETY: compiler-issued registers are defined before use; Value has no drop glue.
        unsafe { frame.registers.set_len(register_count) };
        self.frames.push(frame);
        if id == super::ROOT_FUNCTION_ID
            && self.programs.is_module(self.frames.last().unwrap().program)
        {
            let frame_index = self.frames.len() - 1;
            let environment = self.promote_frame_environment(frame_index);
            self.programs
                .set_module_environment(self.frames[frame_index].program, environment);
        }
        let result = self.run_frame_general(p, self.frames.len() - 1);
        let frame = self.frames.pop().unwrap();
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
                program: self.active_program,
                function: 0,
                pc: 0,
                env: Value::NULL,
                this: Value::UNDEFINED,
                locals: vec![],
                dynamic_bindings: vec![],
                captured: false,
                registers: vec![],
                active_iterators: vec![],
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
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(elements),
            });
        }
        if let Some(slot) = function
            .local_atoms
            .iter()
            .position(|atom| self.atom_name(*atom).contains("\0rqj:self-binding:"))
        {
            frame.locals[slot] = self.function_values[&(self.active_program, id)]
                .iter()
                .rev()
                .find_map(|(closure_env, value)| (*closure_env == parent).then_some(*value))
                .unwrap_or(Value::UNDEFINED);
        }
        if let Some(encoded_slot) = function.arguments_slot {
            let mapped = encoded_slot & MAPPED_ARGUMENTS_BIT != 0;
            let slot = encoded_slot & !MAPPED_ARGUMENTS_BIT;
            let arguments = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.object_proto),
                elements: Rc::new(args.to_vec()),
            });
            frame.locals[usize::from(slot)] = arguments;
            self.initialize_arguments_object(p, arguments, id, parent, args, mapped)?;
            if mapped {
                let mapping = (0..function.params.min(args.len() as u16)).collect();
                if let Some(object) = self.object_data_mut(arguments) {
                    object.arguments_map = Some(mapping);
                }
            }
        }
        frame.function = id;
        frame.pc = 0;
        frame.env = parent;
        let arrow = function
            .name
            .is_some_and(|atom| self.atom_name(atom) == "\0rqj:arrow");
        let derived = is_derived_constructor(function);
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
        self.with_stack.extend(self.captured_with_objects(parent));
        let new_target_atom = self.intern_atom("\0rqj:new-target");
        if !arrow {
            frame.dynamic_bindings.push((
                new_target_atom,
                self.construct_target.unwrap_or(Value::UNDEFINED),
            ));
        }
        self.construct_target = None;
        let lexical_this_atom = self.intern_atom("\0rqj:lexical-this");
        frame.dynamic_bindings.push((lexical_this_atom, frame.this));
        if !arrow {
            let super_called_atom = self.intern_atom("\0rqj:super-called");
            frame
                .dynamic_bindings
                .push((super_called_atom, Value::FALSE));
        }
        let register_count = function.registers as usize;
        if frame.registers.capacity() < register_count {
            frame
                .registers
                .reserve_exact(register_count - frame.registers.len());
        }
        // SAFETY: compiler-issued registers are defined before use; Value has no drop glue.
        unsafe { frame.registers.set_len(register_count) };
        self.frames[frame_index] = frame;
        Ok(())
    }

    pub(super) fn initialize_arguments_object(
        &mut self,
        p: &ResidualProgram,
        arguments: Value,
        id: u32,
        parent: Value,
        args: &[Value],
        mapped: bool,
    ) -> Result<(), JsError> {
        if let Some(object) = self.object_data_mut(arguments) {
            object.arguments_object = true;
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
        let value = if mapped {
            if let Some(function) = self
                .function_values
                .get(&(self.active_program, id))
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|(environment, _)| *environment == parent)
                        .map(|(_, function)| *function)
                })
            {
                function
            } else {
                self.closure(p, id, parent)?
            }
        } else {
            Value::UNDEFINED
        };
        self.set_property(arguments, callee, value)?;
        if mapped {
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
            let thrower = self.native_value(Native::ThrowTypeError);
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
        let slots = std::mem::take(&mut self.frames[frame].locals);
        let env = self.heap.alloc(Cell::Environment {
            parent,
            program: Some(self.frames[frame].program.raw()),
            root_eval_scope: false,
            function: self.frames[frame].function,
            slots: slots.into_boxed_slice(),
            dynamic_bindings: self.frames[frame].dynamic_bindings.clone(),
            with_objects: Vec::new(),
        });
        self.frames[frame].env = env;
        self.frames[frame].captured = true;
        env
    }

    pub(super) fn clone_frame_environment(&mut self, frame: usize) {
        let source = self.promote_frame_environment(frame);
        let Some(Cell::Environment {
            parent,
            program,
            root_eval_scope,
            function,
            slots,
            dynamic_bindings,
            with_objects,
        }) = self.heap.get(source).cloned()
        else {
            return;
        };
        let env = self.heap.alloc(Cell::Environment {
            parent,
            program,
            root_eval_scope,
            function,
            slots,
            dynamic_bindings,
            with_objects,
        });
        self.frames[frame].env = env;
    }

    pub(super) fn recycle_frame(mut frame: Frame) -> Frame {
        const RETAINED_VALUES: usize = 256;
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
        let initial_function = self.frames[frame].function as usize;
        let mut pc = self.frames[frame].pc;
        if let Some(error) = initial_error {
            let throwing_pc = pc.saturating_sub(1) as u32;
            let handler = p.functions[initial_function]
                .handlers
                .iter()
                .find(|handler| throwing_pc >= handler.start && throwing_pc < handler.end)
                .copied();
            let Some(handler) = handler else {
                return Err(error);
            };
            if let Some(slot) = handler.slot {
                let value = self.thrown_value_for(p, error);
                if self.frames[frame].captured {
                    let env = self.frames[frame].env;
                    let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) else {
                        return Err(JsError("invalid catch environment".into()));
                    };
                    slots[slot as usize] = value;
                } else {
                    self.frames[frame].locals[slot as usize] = value;
                }
            }
            pc = handler.target as usize;
        }
        loop {
            if stop_pc == Some(pc) {
                self.frames[frame].pc = pc;
                return Ok(FrameOutcome::ParameterInitializationComplete);
            }
            let function = self.frames[frame].function as usize;
            let code = &p.functions[function].code;
            let instruction_pc = pc;
            // SAFETY: the validated residual program has in-range branch targets
            // and a terminal Return. Effect edges publish `pc` to the frame.
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
                    pc = self.frames[frame].pc;
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
                    let throwing_pc = instruction_pc as u32;
                    let handler = p.functions[function]
                        .handlers
                        .iter()
                        .find(|handler| throwing_pc >= handler.start && throwing_pc < handler.end)
                        .copied();
                    let Some(handler) = handler else {
                        self.frames[frame].pc = pc;
                        return Err(error);
                    };
                    if let Some(slot) = handler.slot {
                        let value = self.thrown_value_for(p, error);
                        if self.frames[frame].captured {
                            let env = self.frames[frame].env;
                            let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env)
                            else {
                                self.frames[frame].pc = pc;
                                return Err(JsError("invalid catch environment".into()));
                            };
                            slots[slot as usize] = value;
                        } else {
                            self.frames[frame].locals[slot as usize] = value;
                        }
                    }
                    pc = handler.target as usize;
                }
            }
        }
    }

    fn captured_with_objects(&self, mut env: Value) -> Vec<Value> {
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
