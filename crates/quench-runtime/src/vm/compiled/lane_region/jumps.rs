//! The jumps of `Lowering::instruction`: `Jump` with its loop rotation, and the tests
//! `JumpBinaryFalse` lowers natively (a nullish comparison, a bitwise truth test, a relation).
use super::*;

impl Lowering<'_, '_> {
    /// `Jump`: a backedge to the region entry counts and polls, and a pure loop header test it
    /// reaches is rotated in; control never falls through.
    pub(super) fn jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let target = instruction.jump_target() as usize;
        if target <= pc {
            self.backedge |= target == self.coverage.entry();
            if target == self.coverage.entry() { self.count_backedge(); }
            if self.shared_backedge(state, target) { return false; }
            self.interrupt_check(state, pc as u32);
            if let Some(header) = self.unit.instructions.get(target).copied()
                && header.op().is_js_conditional_jump()
                && header.result_flags() == 0
                && header.jump_target() as usize > pc
            {
                // Rotate only the pure header test. The first visit
                // keeps the zero-trip guard; each backedge keeps its
                // poll and exact source-header recovery state.
                return self.instruction(state, target, header);
            }
        }
        self.step(state, frame::Edges::Jump, crate::staged::Operation::Jump);
        self.continue_at(state, target);
        false
    }

    /// `JumpBinaryFalse` of a nullish comparison, a bitwise truth test or a relation; `None` where
    /// the lane lowers none of them.
    pub(super) fn jump_binary_false(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let operator = instruction.binary_operator_field();
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if let Some(comparison) = nullish_comparison(&self.program.constants, instruction,
            |register| state.values[usize::from(register)].value.ty) {
            self.nullish_jump(state, pc, instruction, comparison);
            return Some(false);
        }
        if let Some(operation) = super::super::operations::NumberBitwise::from_operator(operator)
            && self.operand_supported(left) && self.operand_supported(right)
            && [left, right].iter().all(|operand| operand.kind() != Some(OperandKind::Constant)
                || super::super::facts::operand_type(&self.program.constants, *operand, |_| Type::ANY).is_subtype(Type::NUMBER))
        {
            self.bitwise_jump(state, pc, instruction, operation);
            return Some(false);
        }
        let holds = self.relation(state, pc as u32, operator, left, right)?;
        let taken = self.word_cmp_imm(IntCC::Equal, holds, 0);
        self.branch_polled(state, pc, instruction, taken);
        Some(false)
    }

    /// The branch of `instruction` on `taken`, polling for interrupts first on a backedge.
    fn branch_polled(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, taken: ir::Value) {
        let target = instruction.jump_target() as usize;
        if target <= pc { self.interrupt_check(state, pc as u32); }
        self.branch(state, pc, target, taken);
    }

    /// A bitwise truth test: signed and unsigned results are false exactly when all bits are
    /// zero, so the value operation's bits branch without boxing.
    fn bitwise_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        operation: super::super::operations::NumberBitwise) {
        let exit = self.guard_exit(state, pc as u32);
        let left = self.operand_to_int32(state, instruction.operand_b(), exit).expect("supported numeric operand");
        let right = self.operand_to_int32(state, instruction.operand_c(), exit).expect("supported numeric operand");
        let bits = emit_number_bitwise_bits(&mut self.builder, operation, left, right);
        let taken = self.word_cmp_imm(IntCC::Equal, bits, 0);
        self.branch_polled(state, pc, instruction, taken);
    }

    /// A comparison with `null` or `undefined`: decided by the operand's type where it is known,
    /// else tested on the value.
    fn nullish_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: NullishComparison) {
        let ty = state.values[usize::from(comparison.source)].value.ty;
        let target = instruction.jump_target() as usize;
        if let Some(taken) = comparison.known_outcome(ty) {
            let taken = self.int_const(types::I8, i64::from(taken));
            self.branch_polled(state, pc, instruction, taken);
        } else if target > pc {
            self.nullish_forward(state, pc, target, comparison);
        } else {
            let exit = self.guard_exit(state, pc as u32);
            let value = self.tagged(state, comparison.source);
            let taken = emit_nullish_branch(&mut self.builder, value, comparison, self.vm.html_dda, exit);
            self.branch_polled(state, pc, instruction, taken);
        }
    }

    /// Whether a register live at `unequal` holds the direct-object family value `identity`.
    fn object_family_alias(&self, state: &State, unequal: usize, identity: ir::Value) -> bool {
        self.facts.at(unequal).is_some_and(|types| types.iter().enumerate().any(|(index, ty)|
            *ty == Type::DIRECT_OBJECT_FAMILY && self.is_live(unequal, index as Register)
            && matches!(state.values[index].tagged, TaggedView::Value(alias)
                if self.builder.func.dfg.resolve_aliases(alias) == identity)))
    }

    /// A forward nullish comparison: its edges, and where the unequal arm already excludes the
    /// direct-object family a third edge that continues with the family refined.
    fn nullish_forward(&mut self, state: &mut State, pc: usize, target: usize, comparison: NullishComparison) {
        let exit = self.guard_exit(state, pc as u32);
        let value = self.tagged(state, comparison.source);
        let taken = self.builder.create_block();
        let fall = self.builder.create_block();
        let unequal = if comparison.negated { pc + 1 } else { target };
        let identity = self.builder.func.dfg.resolve_aliases(value);
        let direct = (!comparison.strict && self.object_family_alias(state, unequal, identity))
            .then(|| self.builder.create_block());
        emit_nullish_edges(&mut self.builder, value, comparison, self.vm.html_dda, exit, taken, fall, direct);
        let Some(direct) = direct else { return self.continue_branch(state, pc, target, taken, fall) };
        self.builder.switch_to_block(direct);
        let mut direct_state = state.clone();
        self.refine_object_family(&mut direct_state, value);
        self.continue_at(&mut direct_state, unequal);
        // The other unequal arm has already excluded this family.
        // Resume before the latch's moves, with the original homes.
        self.builder.switch_to_block(if comparison.negated { fall } else { taken });
        self.exit(state, unequal as u32);
        self.builder.switch_to_block(if comparison.negated { taken } else { fall });
        self.continue_at(&mut state.clone(), if comparison.negated { target } else { pc + 1 });
    }
}
