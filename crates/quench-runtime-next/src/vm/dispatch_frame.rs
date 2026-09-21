use super::*;

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
        self.profile.function(id as usize);
        let function = &p.functions[id as usize];
        let mut frame = self.frame_pool.pop().unwrap_or(Frame {
            function: 0,
            pc: 0,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            captured: false,
            registers: vec![],
        });
        frame
            .locals
            .resize(function.locals as usize, Value::UNDEFINED);
        frame.locals[function.params as usize..].fill(Value::UNDEFINED);
        for index in 0..function.params as usize {
            frame.locals[index] = args.get(index).copied().unwrap_or(Value::UNDEFINED);
        }
        frame.function = id;
        frame.pc = 0;
        frame.env = parent;
        frame.this = this;
        frame.captured = false;
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
        self.frame_pool.push(Self::recycle_frame(frame));
        result
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
        });
        self.frames[frame].env = env;
        self.frames[frame].captured = true;
        env
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
        frame
    }

    pub(super) fn run_frame_general(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<Value, JsError> {
        let function = self.frames[frame].function as usize;
        let code = &p.functions[function].code;
        let mut pc = self.frames[frame].pc;
        loop {
            let instruction_pc = pc;
            // SAFETY: the validated residual program has in-range branch targets
            // and a terminal Return. Effect edges publish `pc` to the frame.
            let ins = unsafe { *code.get_unchecked(pc) };
            pc += 1;
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .opcode(ins.op() as usize, frame, function as u32, instruction_pc);
            #[cfg(not(feature = "profile-aggregate"))]
            self.profile.opcode(ins.op() as usize);
            match self.step(p, frame, ins, &mut pc) {
                Ok(Some(value)) => {
                    self.frames[frame].pc = pc;
                    return Ok(value);
                }
                Ok(None) => {}
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
                        let value = self.heap.alloc(Cell::Error(error.into_message()));
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
