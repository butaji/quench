use super::*;

impl Lowering<'_, '_> {
    /// A bitwise operand's ToInt32 (`frame::operand_to_int32`).
    pub(super) fn operand_to_int32(&mut self, state: &mut State, operand: Operand, exit: ir::Block) -> Option<ir::Value> {
        frame::operand_to_int32(&mut self.builder, &mut self.registers, state, &self.program.constants, operand, exit).map(quench_runtime::staged::Int32::word)
    }

    /// Whether only the result's ToInt32 is read of `register`, written at `pc`: the next
    /// instruction is its one reader, a bitwise operator, which applies ToInt32 to its operands.
    /// A sum's or difference's wrapping int32 result then serves for int32 operands, and
    /// ToInt32 of its exact double for any others. Not through a further sum: ToInt32 does not
    /// commute with adding a non-integer (trunc(-3 + 0.5) differs from trunc(-3 + 2^32 + 0.5)
    /// mod 2^32), so V8 truncates chains only of int32 inputs.
    pub(super) fn truncated(&self, pc: usize, register: Register) -> bool {
        let (at, register) = self.reader(pc + 1, register);
        let Some(next) = self.unit.instructions.get(at).copied() else { return false };
        if !quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary) || next.returns_from_frame() || next.writes_current_this() { return false; }
        let reads = [next.operand_b(), next.operand_c()].iter().filter(|operand| operand.register_index() == Some(register)).count();
        if reads == 0 || !(next.result_register() == register || self.dead_after(at, register)) { return false; }
        let operator = next.binary_operator();
        if crate::compiled::number_bitwise(next, operator)
            .is_some_and(|bitwise| bitwise != super::operations::NumberBitwise::ShiftRightUnsigned) {
            return true;
        }
        false
    }

    /// Whether the int32 remainder written to `register` at `pc` is read only by a sum or difference
    /// (`Add` or `Subtract`) whose own result only its ToInt32 is read of (`truncated`), and `register`
    /// is dead past that sum unless the sum writes it. ToInt32 reads a `-0` and a `0` alike, and a sum
    /// with either differs from the other in the sign of a zero only, so the remainder's sign is unread.
    pub(crate) fn truncated_sum_operand(&self, pc: usize, register: Register) -> bool {
        let (at, reader) = self.reader(pc + 1, register);
        let Some(next) = self.unit.instructions.get(at).copied() else { return false };
        if !quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary) || next.returns_from_frame()
            || next.writes_current_this() || next.result_flags() != 0 { return false; }
        if !matches!(crate::compiled::number_arithmetic(next, next.binary_operator()),
            Some(quench_runtime::vm::operations::NumberArithmetic::Add | quench_runtime::vm::operations::NumberArithmetic::Subtract)) { return false; }
        let reads = [next.operand_b(), next.operand_c()].iter().filter(|operand| operand.register_index() == Some(reader)).count();
        reads > 0 && (next.result_register() == reader || self.dead_after(at, reader)) && self.truncated(at, next.result_register())
    }

    /// Whether only the ToInt32 of the int32 result written to `register` at `pc` is read: through
    /// at most `TRUNCATION_CHAIN` sums or differences with an int32 constant, into a bitwise
    /// operator (`truncated`). Each sum adds less than 2^31 to an exact integer, so a chain from a
    /// value under 2^52 keeps every double exact, and ToInt32 of an exact integer is its low 32
    /// bits: the wrapping int32 steps compute it. Only int32 constants join the chain: a
    /// non-integer would not commute with ToInt32 (`truncated`).
    pub(crate) fn truncated_through_sums(&self, pc: usize, register: Register) -> bool {
        let (mut pc, mut register) = (pc, register);
        for _ in 0..=TRUNCATION_CHAIN {
            if self.truncated(pc, register) { return true; }
            let Some(next) = self.int32_constant_sum(pc, register) else { return false };
            (pc, register) = next;
        }
        false
    }

    /// Whether the int32 product written to `register` at `pc` is read only through its ToInt32, by
    /// a sum or difference as its next instruction whose other operand is another int32 register
    /// (`state`, the facts before `pc`, prove it int32 and the product does not write it), and then
    /// through `truncated_through_sums`. Each added int32 keeps the chain's sums exact in binary64
    /// as an int32 constant does, so the wrapping product and sum give the low 32 bits of the
    /// exact sum: its ToInt32, as `(a * 3 + i) | 0` (`int_mul`).
    pub(crate) fn truncated_through_int32_sum(&self, state: &State, pc: usize, register: Register) -> bool {
        let (at, reader) = self.reader(pc + 1, register);
        if at != pc + 1 || reader != register { return false; }
        let Some(next) = self.unit.instructions.get(at).copied() else { return false };
        if !quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary) || next.returns_from_frame()
            || next.writes_current_this() || next.result_flags() != 0 { return false; }
        let Some(operation) = crate::compiled::number_arithmetic(next, next.binary_operator()) else { return false };
        if !operation.int32_projection_is_wrapping() { return false; }
        let int32_register = |operand: Operand| operand.register_index().filter(|other| *other != register)
            .is_some_and(|other| state.values[usize::from(other)].value.ty.is_subtype(Type::INT32));
        let [first, second] = [next.operand_b(), next.operand_c()];
        let paired = (first.register_index() == Some(register) && int32_register(second))
            || (second.register_index() == Some(register) && int32_register(first));
        if !paired || !(next.result_register() == register || self.dead_after(at, register)) { return false; }
        self.truncated_through_sums(at, next.result_register())
    }

    /// The sum or difference that next reads `register`, written at `pc`, with an int32 constant
    /// as its other operand and `register` dead past it: its PC and result register.
    fn int32_constant_sum(&self, pc: usize, register: Register) -> Option<(usize, Register)> {
        let (at, register) = self.reader(pc + 1, register);
        let next = self.unit.instructions.get(at).copied()?;
        use quench_runtime::bytecode::ImmediateRole as Role;
        let operator = matches!(next.op().immediate_role(), Role::BinaryOperator | Role::AdditionOperator | Role::MultiplicationOperator);
        if !operator || next.returns_from_frame() || next.writes_current_this() || next.result_flags() != 0 { return None; }
        let operation = crate::compiled::number_arithmetic(next, next.binary_operator())?;
        if !operation.int32_projection_is_wrapping() { return None; }
        let operands = [next.operand_b(), next.operand_c()];
        let int32_constant = |operand: Operand| operand.kind() == Some(OperandKind::Constant)
            && self.program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate).is_some_and(super::facts::is_int32);
        let [first, second] = operands;
        let paired = (first.register_index() == Some(register) && int32_constant(second))
            || (second.register_index() == Some(register) && int32_constant(first));
        (paired && (next.result_register() == register || self.dead_after(at, register))).then(|| (at, next.result_register()))
    }

    /// Whether `register` is dead past the instruction at `at`. An inlined callee's returning
    /// instruction is followed by its completion (`lane_unit`): moves and a jump out of the
    /// callee that share its origin, whose liveness row is the returning instruction's own. Past
    /// that completion the callee's frame is gone, so its registers are dead unless a completion
    /// move reads them.
    fn dead_after(&self, at: usize, register: Register) -> bool {
        use super::lane_unit::Origin;
        let origin = self.unit.origins.get(at).copied();
        if let Some(Origin::Inlined { frame, .. }) = origin
            && self.unit.origins.get(at + 1).copied() == origin {
            let inlined = self.unit.frames[frame];
            let own = (inlined.base..inlined.base + inlined.registers).contains(&register);
            let mut pc = at + 1;
            while own && self.unit.origins.get(pc).copied() == origin {
                let Some(instruction) = self.unit.instructions.get(pc).copied() else { return false };
                if instruction.op().copies_register() {
                    if instruction.register_b() == register { return false; }
                } else if instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Jump {
                    let target = instruction.jump_target() as usize;
                    let leaves = !matches!(self.unit.origins.get(target).copied(), Some(Origin::Inlined { frame: to, .. }) if to == frame);
                    return leaves;
                } else {
                    return false;
                }
                pc += 1;
            }
        }
        !self.is_live(at + 1, register)
    }

    /// Where `register` at `at` is next read, past an inlined completion's move and jump (`lane_unit`).
    fn reader(&self, at: usize, register: Register) -> (usize, Register) {
        let instruction = |at: usize| self.unit.instructions.get(at).copied();
        let (at, register) = match instruction(at) {
            Some(moved) if moved.op().copies_register() && moved.register_b() == register && !self.is_live(at + 1, register) => (at + 1, moved.result_register()),
            _ => (at, register),
        };
        match instruction(at) { Some(jump) if jump.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Jump => (jump.jump_target() as usize, register), _ => (at, register) }
    }
}
