use super::operations::{numeric_integer_add_result, numeric_integer_multiply_result};
use super::*;

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn numeric_binary(&self, op: u32, left: Value, right: Value) -> Option<Value> {
        self.numeric_integer_binary(op, left, right)
    }
}

macro_rules! specialized_numeric_value {
    ($vm:expr, $program:expr, $operator:path, $integer_handler:ident, $left:expr, $right:expr) => {{
        let fast =
            Value::int_pair($left, $right).map(|(left, right)| $integer_handler(left, right));
        if fast.is_some() {
            $vm.record_binary_value_path(
                $operator as u32,
                $left,
                $right,
                crate::profile::BinaryValuePath::IntegerFastPath,
            );
        }
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
    ($vm:ident, $program:ident, $frame:ident, $ins:ident, $operator:path, $integer_handler:ident) => {{
        let left_operand = $ins.operand_b();
        let right_operand = $ins.operand_c();
        $vm.profile
            .binary($operator as usize, left_operand.0, right_operand.0);
        let left = $vm.resolve_operand($program, $frame, left_operand)?;
        let right = $vm.resolve_operand($program, $frame, right_operand)?;
        let value =
            specialized_numeric_value!($vm, $program, $operator, $integer_handler, left, right);
        if $ins.returns_from_frame() {
            return Ok(StepResult::Return(value));
        }
        if let Some(local) = $ins.numeric_local_target() {
            $vm.frames[$frame].locals[local as usize] = value;
            $vm.profile.virtual_opcode(Op::StoreLocal as usize);
        } else {
            $vm.write($frame, $ins.result_register(), value);
        }
    }};
}

impl<H: Host> Vm<H> {
    pub(super) fn run_frame_numeric(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let function = self.frames[frame].function as usize;
        let code = &p.functions[function].code;
        let mut pc = self.frames[frame].pc;
        loop {
            // SAFETY: decoded branch targets are in range and every function
            // terminates; the frame publishes the active instruction for GC.
            let _instruction_pc = pc;
            self.frames[frame].pc = _instruction_pc;
            let ins = unsafe { *code.get_unchecked(pc) };
            pc += 1;
            #[cfg(feature = "profile-aggregate")]
            self.profile
                .opcode(ins.op() as usize, frame, function as u32, _instruction_pc);
            #[cfg(feature = "profile-aggregate")]
            self.profile.object_literal_instruction(
                self.frames[frame].program.raw(),
                function as u32,
                _instruction_pc,
                ins.op(),
            );
            #[cfg(not(feature = "profile-aggregate"))]
            self.profile.opcode(ins.op() as usize);
            let outcome = (|| -> Result<StepResult, JsError> {
                match ins.op() {
                    Op::LoadLocalPlain => {
                        let local = ins.local_slot();
                        // SAFETY: validated bytecode bounds the slot by Function.locals,
                        // and frame setup sizes locals to that count.
                        let value = unsafe { self.read_validated_local(frame, local) };
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
                    Op::LoadLocal => {
                        let local = ins.local_slot();
                        let value = self.load_local_binding(p, frame, local, None)?;
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
                        let value = self.read(frame, ins.register_a());
                        let local = ins.local_slot();
                        self.check_local_assignment_initialized(
                            p,
                            frame,
                            local,
                            ins.boolean_field(crate::bytecode::InstructionField::C)
                                .unwrap_or(false),
                        )?;
                        self.frames[frame].locals[local] = value;
                        self.mirror_global_lexical_binding(p, frame, local, value);
                        if let Some(register) = ins.optional_register_b() {
                            self.write(frame, register, value);
                        }
                    }
                    Op::StoreLocalPlain => {
                        let value = self.read(frame, ins.register_a());
                        // SAFETY: validated bytecode bounds the slot by Function.locals,
                        // and frame setup sizes locals to that count.
                        unsafe { self.write_validated_local(frame, ins.local_slot(), value) };
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
                        if fast.is_some() {
                            self.record_binary_value_path(
                                operator,
                                left,
                                right,
                                crate::profile::BinaryValuePath::IntegerFastPath,
                            );
                        }
                        let value = match fast {
                            Some(value) => value,
                            None => self.binary(p, operator, left, right)?,
                        };
                        if ins.returns_from_frame() {
                            return Ok(StepResult::Return(value));
                        }
                        if let Some(local) = ins.numeric_local_target() {
                            self.frames[frame].locals[local as usize] = value;
                            self.profile.virtual_opcode(Op::StoreLocal as usize);
                        } else {
                            self.write(frame, ins.result_register(), value);
                        }
                    }
                    Op::NumericAdd => {
                        execute_specialized_numeric!(
                            self,
                            p,
                            frame,
                            ins,
                            oxc_ast::ast::BinaryOperator::Addition,
                            numeric_integer_add_result
                        )
                    }
                    Op::NumericMultiply => {
                        execute_specialized_numeric!(
                            self,
                            p,
                            frame,
                            ins,
                            oxc_ast::ast::BinaryOperator::Multiplication,
                            numeric_integer_multiply_result
                        )
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
                        self.profile
                            .branch_value(value.profile_kind() as usize, truthy);
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
                    Op::JumpUnaryFalse => {
                        let operator = ins.unary_operator_field();
                        let input = self.resolve_operand(p, frame, ins.operand_b())?;
                        let value = self.unary(p, operator, input)?;
                        if !self.truthy(value) {
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
                Ok(StepResult::PushFrame { .. }) => {
                    unreachable!("numeric dispatch cannot push general frames")
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
                        self.initialize_handler_binding(p, frame, slot, value)?;
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
        self.frames[frame].pc = pc - 1;
        let result = self.step(p, frame, instruction.as_wide(), &mut pc, false);
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
        self.profile
            .fused_opcode(op as usize, _frame, _function, _pc);
        #[cfg(not(feature = "profile-aggregate"))]
        self.profile.fused_opcode(op as usize);
    }
}
