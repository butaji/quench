use super::*;
use crate::bytecode::MAPPED_ARGUMENTS_BIT;

impl<H: Host> Vm<H> {
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
            FrameOutcome::Complete(value) => Ok(value),
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
        self.profile.function(id as usize);
        if p.functions[id as usize].parameter_eval_arguments_error {
            return Err(self
                .syntax_error_result(p, "arguments binding is not allowed in function parameters")
                .expect_err("syntax_error_result must throw"));
        }
        let function = &p.functions[id as usize];
        let mut frame = self.frame_pool.pop().unwrap_or(Frame {
            function: 0,
            pc: 0,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            dynamic_bindings: vec![],
            captured: false,
            registers: vec![],
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
        frame.this = if function.strict {
            this
        } else if this.is_null() || this.is_undefined() {
            self.realm.globals
        } else {
            self.box_object(this)?
        };
        frame.captured = false;
        frame.with_base = self.with_stack.len();
        let register_count = function.registers as usize;
        if frame.registers.capacity() < register_count {
            frame
                .registers
                .reserve_exact(register_count - frame.registers.len());
        }
        // SAFETY: compiler-issued registers are defined before use; Value has no drop glue.
        unsafe { frame.registers.set_len(register_count) };
        self.frames.push(frame);
        let result = self.run_frame_general(p, self.frames.len() - 1);
        let frame = self.frames.pop().unwrap();
        match result? {
            FrameOutcome::Complete(value) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                Ok(FrameOutcome::Complete(value))
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
        }
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
            if let Some(function) = self.function_values.get(id as usize).and_then(|entries| {
                entries
                    .iter()
                    .find(|(environment, _)| *environment == parent)
                    .map(|(_, function)| *function)
            }) {
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
            self.descriptors.insert(
                (arguments, property_key::PropertyKey::symbol(iterator)),
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
            slots: slots.into_boxed_slice(),
            dynamic_bindings: self.frames[frame].dynamic_bindings.clone(),
        });
        self.frames[frame].env = env;
        self.frames[frame].captured = true;
        env
    }

    pub(super) fn clone_frame_environment(&mut self, frame: usize) {
        let source = self.promote_frame_environment(frame);
        let Some(Cell::Environment { parent, slots, dynamic_bindings }) = self.heap.get(source).cloned() else {
            return;
        };
        let env = self.heap.alloc(Cell::Environment { parent, slots, dynamic_bindings });
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
        let function = self.frames[frame].function as usize;
        let code = &p.functions[function].code;
        let mut pc = self.frames[frame].pc;
        if let Some(error) = initial_error {
            let throwing_pc = pc.saturating_sub(1) as u32;
            let handler = p.functions[function]
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
                Ok(StepResult::Await { value, destination }) => {
                    self.frames[frame].pc = pc;
                    return Ok(FrameOutcome::Await {
                        value,
                        destination,
                        frame: None,
                    });
                }
                Ok(StepResult::Yield { value, destination }) => {
                    self.frames[frame].pc = pc;
                    return Ok(FrameOutcome::Yield {
                        value,
                        destination,
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
