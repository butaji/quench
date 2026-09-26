use super::*;

macro_rules! numeric_integer_binary {
    ($op:expr, $a:expr, $b:expr) => {
        match $op {
            7 => Some(if $a >= $b { Value::TRUE } else { Value::FALSE }),
            8 => Some(
                $a.checked_add($b)
                    .map(Value::integer)
                    .unwrap_or_else(|| Value::number($a as f64 + $b as f64)),
            ),
            10 => Some(
                $a.checked_mul($b)
                    .map(Value::integer)
                    .unwrap_or_else(|| Value::number($a as f64 * $b as f64)),
            ),
            15 => Some(Value::integer($a >> ($b as u32 & 31))),
            19 => Some(Value::integer($a & $b)),
            _ => None,
        }
    };
}

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn numeric_binary(&self, op: u32, left: Value, right: Value) -> Option<Value> {
        let (a, b) = Value::int_pair(left, right)?;
        numeric_integer_binary!(op, a, b)
    }
}

macro_rules! numeric_integer_semantic {
    (add, $left:expr, $right:expr) => {
        $left
            .checked_add($right)
            .map(Value::integer)
            .unwrap_or_else(|| Value::number($left as f64 + $right as f64))
    };
    (multiply, $left:expr, $right:expr) => {
        $left
            .checked_mul($right)
            .map(Value::integer)
            .unwrap_or_else(|| Value::number($left as f64 * $right as f64))
    };
}

macro_rules! specialized_numeric_value {
    ($vm:expr, $program:expr, $operator:expr, $semantic:ident, $left:expr, $right:expr) => {{
        let fast = Value::int_pair($left, $right)
            .map(|(left, right)| numeric_integer_semantic!($semantic, left, right));
        #[cfg(feature = "profile-aggregate")]
        $vm.profile.numeric_binary_path(
            $operator as usize,
            fast.is_some(),
            $left.as_int().is_some() && $right.as_int().is_some(),
        );
        match fast {
            Some(value) => value,
            None => $vm.binary($program, $operator as u32, $left, $right)?,
        }
    }};
}

macro_rules! execute_specialized_numeric {
    ($vm:ident, $program:ident, $frame:ident, $ins:ident, $semantic:ident) => {{
        let operator = $ins.binary_operator();
        $vm.profile.binary(operator as usize, $ins.b(), $ins.c());
        let left = $vm.resolve_operand($program, $frame, Operand($ins.b()))?;
        let right = $vm.resolve_operand($program, $frame, Operand($ins.c()))?;
        let value = specialized_numeric_value!($vm, $program, operator, $semantic, left, right);
        if $ins.returns_from_frame() {
            return Ok(StepResult::Return(value));
        }
        if $ins.writes_numeric_local() {
            $vm.frames[$frame].locals[$ins.result_register() as usize] = value;
            $vm.profile.virtual_opcode(Op::StoreLocal as usize);
        } else {
            $vm.write($frame, $ins.result_register(), value);
        }
    }};
}

impl<H: Host> Vm<H> {
    #[inline(never)]
    pub(super) fn call_user_numeric(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: NumericArguments<'_>,
    ) -> Result<Value, JsError> {
        self.profile
            .numeric_arguments(matches!(args, NumericArguments::Registers { .. }));
        self.profile.function(id as usize);
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
            frame.locals[index] = match args {
                NumericArguments::Values(values) => {
                    values.get(index).copied().unwrap_or(Value::UNDEFINED)
                }
                NumericArguments::Registers { frame, values } => values
                    .get(index)
                    .map_or(Value::UNDEFINED, |register| self.read(frame, *register)),
            };
        }
        if function.rest {
            let elements = match args {
                NumericArguments::Values(values) => {
                    values.get(fixed..).unwrap_or_default().to_vec()
                }
                NumericArguments::Registers { frame, values } => values
                    .get(fixed..)
                    .unwrap_or_default()
                    .iter()
                    .map(|register| self.read(frame, *register))
                    .collect(),
            };
            frame.locals[fixed] = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(elements),
            });
        }
        frame.function = id;
        frame.program = self.active_program;
        frame.pc = 0;
        frame.env = parent;
        frame.this = self.call_this_value(this, function.strict)?;
        frame.captured = false;
        frame.with_base = self.with_stack.len();
        let register_count = function.registers as usize;
        if frame.registers.capacity() < register_count {
            frame
                .registers
                .reserve_exact(register_count - frame.registers.len());
        }
        // SAFETY: same compiler-issued register invariant as the general path.
        unsafe { frame.registers.set_len(register_count) };
        self.frames.push(frame);
        let result = self.run_frame_numeric(p, self.frames.len() - 1);
        let frame = self.frames.pop().unwrap();
        self.frame_pool.push(Self::recycle_frame(frame));
        result
    }

    pub(super) fn run_frame_numeric(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<Value, JsError> {
        let function = self.frames[frame].function as usize;
        let code = &p.functions[function].code;
        let mut pc = self.frames[frame].pc;
        loop {
            // SAFETY: decoded branch targets are in range and every function
            // terminates; `pc` is synchronized at every external semantic edge.
            let _instruction_pc = pc;
            let ins = unsafe { *code.get_unchecked(pc) };
            pc += 1;
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .opcode(ins.op() as usize, frame, function as u32, _instruction_pc);
            #[cfg(not(feature = "profile-aggregate"))]
            self.profile.opcode(ins.op() as usize);
            let outcome = (|| -> Result<StepResult, JsError> {
                match ins.op() {
                    Op::LoadLocal => {
                        let local = ins.local_slot();
                        let value = self.frames[frame].locals[local];
                        self.write(frame, ins.result_register(), value);
                        if let Some(target) = ins.numeric_local_store_target()
                            && let Some(integer) = value.as_int()
                        {
                            self.numeric_local_inc_store(
                                frame,
                                &mut pc,
                                integer,
                                target,
                                local as u32,
                            );
                        }
                    }
                    Op::StoreLocal => {
                        let value = self.read(frame, ins.a());
                        let local = ins.local_slot();
                        self.frames[frame].locals[local] = value;
                        self.mirror_global_lexical_binding(p, frame, local, value);
                        if let Some(register) = ins.optional_register_b() {
                            self.write(frame, register, value);
                        }
                    }
                    Op::GetIndex => {
                        #[cfg(feature = "profile-aggregate")]
                        self.profile.index_dispatch(false, true);
                        let base_operand = ins.operand_b();
                        let index_operand = ins.operand_c();
                        let base = self.numeric_index_source(frame, base_operand);
                        let index = self.numeric_index_source(frame, index_operand);
                        if base_operand.kind() == Some(crate::bytecode::OperandKind::Local) {
                            self.profile.virtual_opcode(Op::LoadLocal as usize);
                        }
                        if index_operand.kind() == Some(crate::bytecode::OperandKind::Local) {
                            self.profile.virtual_opcode(Op::LoadLocal as usize);
                        }
                        let value = self.get_index(p, base, index)?;
                        self.write(frame, ins.result_register(), value);
                    }
                    Op::SetIndex => {
                        #[cfg(feature = "profile-aggregate")]
                        self.profile.index_dispatch(true, true);
                        self.set_index_mode(
                            p,
                            self.read(frame, ins.register_b()),
                            self.read(frame, ins.register_c()),
                            self.read(frame, ins.register_a()),
                            p.functions[self.frames[frame].function as usize].strict
                                || ins.boolean_flag().expect("validated boolean immediate"),
                        )?
                    }
                    Op::DefineArrayElement => self.define_array_literal_element(
                        p,
                        self.read(frame, ins.register_b()),
                        ins.array_index() as usize,
                        self.read(frame, ins.register_a()),
                    )?,
                    Op::Binary => {
                        let operator = ins.binary_operator();
                        let left_operand = ins.operand_b();
                        let right_operand = ins.operand_c();
                        self.profile
                            .binary(operator as usize, left_operand.0, right_operand.0);
                        let left = self.resolve_operand(p, frame, left_operand)?;
                        let right = self.resolve_operand(p, frame, right_operand)?;
                        let fast = self.numeric_binary(operator, left, right);
                        #[cfg(feature = "profile-aggregate")]
                        self.profile.numeric_binary_path(
                            operator as usize,
                            fast.is_some(),
                            left.as_int().is_some() && right.as_int().is_some(),
                        );
                        let value = match fast {
                            Some(value) => value,
                            None => self.binary(p, operator, left, right)?,
                        };
                        if ins.returns_from_frame() {
                            return Ok(StepResult::Return(value));
                        }
                        if ins.writes_numeric_local() {
                            self.frames[frame].locals[ins.result_register() as usize] = value;
                            self.profile.virtual_opcode(Op::StoreLocal as usize);
                        } else {
                            self.write(frame, ins.result_register(), value);
                        }
                    }
                    Op::NumericAdd => {
                        execute_specialized_numeric!(self, p, frame, ins, add)
                    }
                    Op::NumericMultiply => {
                        execute_specialized_numeric!(self, p, frame, ins, multiply)
                    }
                    Op::IncDec => {
                        let input = self.read(frame, ins.register_b());
                        let is_decrement = ins.boolean_flag().expect("validated boolean immediate");
                        let delta = if is_decrement { -1.0 } else { 1.0 };
                        let value = if let Some(integer) = input.as_int() {
                            let next = if is_decrement {
                                integer.checked_sub(1)
                            } else {
                                integer.checked_add(1)
                            };
                            next.map(Value::integer)
                                .unwrap_or_else(|| Value::number(f64::from(integer) + delta))
                        } else {
                            Value::number(self.to_number(p, input)? + delta)
                        };
                        self.write(frame, ins.result_register(), value);
                    }
                    Op::Jump => {
                        pc = ins.jump_target() as usize;
                        self.frames[frame].pc = pc;
                        self.maybe_collect(p);
                    }
                    Op::JumpFalse => {
                        let value = self.read(frame, ins.register_a());
                        let truthy = self.truthy(value);
                        #[cfg(feature = "profile-aggregate")]
                        self.profile.branch_value(value.profile_kind(), truthy);
                        if !truthy {
                            pc = ins.jump_target() as usize;
                        }
                    }
                    Op::JumpBinaryFalse => {
                        let operator = ins.binary_operator_field();
                        let left_operand = ins.operand_b();
                        let right_operand = ins.operand_c();
                        self.profile
                            .binary(operator as usize, left_operand.0, right_operand.0);
                        let left = self.resolve_operand(p, frame, left_operand)?;
                        let right = self.resolve_operand(p, frame, right_operand)?;
                        if !self.binary_truthy(p, operator, left, right)? {
                            pc = ins.jump_target() as usize;
                        }
                    }
                    Op::Return => {
                        self.frames[frame].pc = pc;
                        return Ok(StepResult::Return(self.read(frame, ins.register_a())));
                    }
                    _ => {
                        self.frames[frame].pc = pc;
                        let result = self.numeric_step_fallback(p, frame, ins);
                        pc = self.frames[frame].pc;
                        return result;
                    }
                }
                Ok(StepResult::Continue)
            })();
            match outcome {
                Ok(StepResult::Return(value)) => return Ok(value),
                Ok(StepResult::Continue) => {}
                Ok(StepResult::TailCall) => {
                    return Err(JsError("tail call is not valid in numeric dispatch".into()));
                }
                Ok(StepResult::Await { .. }) => {
                    return Err(JsError("await is not valid in numeric dispatch".into()));
                }
                Ok(StepResult::Yield { .. }) => {
                    return Err(JsError("yield is not valid in numeric dispatch".into()));
                }
                Err(error) => {
                    let throwing_pc = pc as u32 - 1;
                    let handler = p.functions[function]
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
                        let value = error
                            .thrown_value()
                            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                        if self.frames[frame].captured {
                            let env = self.frames[frame].env;
                            let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env)
                            else {
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

    #[inline(never)]
    fn numeric_step_fallback(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        instruction: Instr,
    ) -> Result<StepResult, JsError> {
        let mut pc = self.frames[frame].pc;
        let result = self.step(p, frame, instruction.as_wide(), &mut pc);
        self.frames[frame].pc = pc;
        result
    }

    #[inline(always)]
    fn numeric_local_inc_store(
        &mut self,
        frame: usize,
        pc: &mut usize,
        integer: i32,
        target: crate::bytecode::NumericLocalStoreTarget,
        local: u32,
    ) {
        let function = self.frames[frame].function as usize;
        let delta = if target.decrement { -1 } else { 1 };
        let next = integer
            .checked_add(delta)
            .map(Value::integer)
            .unwrap_or_else(|| Value::number(f64::from(integer) + f64::from(delta)));
        self.write(frame, target.register, next);
        self.frames[frame].locals[local as usize] = next;
        self.profile_numeric_fusion(frame, function as u32, *pc, Op::IncDec);
        self.profile_numeric_fusion(frame, function as u32, *pc + 1, Op::StoreLocal);
        *pc += 2;
    }

    #[inline(always)]
    fn numeric_index_source(&self, frame: usize, operand: Operand) -> Value {
        match operand.kind() {
            Some(crate::bytecode::OperandKind::Register) => self.read(frame, operand.payload()),
            Some(crate::bytecode::OperandKind::Local) => {
                self.frames[frame].locals[operand.payload() as usize]
            }
            _ => unreachable!("numeric indexed source is a register or local"),
        }
    }

    #[inline(always)]
    fn profile_numeric_fusion(&mut self, _frame: usize, _function: u32, _pc: usize, op: Op) {
        #[cfg(feature = "profile-aggregate")]
        self.profile.opcode(op as usize, _frame, _function, _pc);
        #[cfg(not(feature = "profile-aggregate"))]
        self.profile.opcode(op as usize);
    }
}
