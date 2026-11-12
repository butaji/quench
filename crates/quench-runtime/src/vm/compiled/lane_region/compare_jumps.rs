//! The conditional jump on two values (`Operation::CompareJump`, the `JumpBinaryFalse` row),
//! lowered by its comparison and by what the facts prove of its operands: a test against
//! `null` or `undefined` by the value's tag, a bitwise test by its bits, an ordering or an
//! equality by the staged step (whose answer picks the edge through `Frame::branch`), and any
//! other by the lane's own relation.
use super::*;
use crate::staged::{Comparison, Domain, Operation};
use crate::vm::operations::NumberRelation;

impl Lowering<'_, '_> {
    /// The comparison-jump row of `instruction`, where it decodes to one.
    fn compare_row(&self, instruction: WideInstruction) -> Option<Operation<ir::Value, EmitProof>> {
        let row = Operation::decode(instruction, |_| None)?;
        matches!(row, Operation::CompareJump { .. }).then_some(row)
    }

    /// Whether `instruction` is a comparison jump.
    pub(super) fn is_compare_jump(&self, instruction: WideInstruction) -> bool {
        self.compare_row(instruction).is_some()
    }

    /// The comparison jump at `pc`; whether it lowered, which ends the block (the jump's edges
    /// continue the lowering themselves).
    pub(super) fn compare_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(Operation::CompareJump { comparison, .. }) = self.compare_row(instruction) else {
            unreachable!("a comparison jump decodes")
        };
        let register_type = |register: Register| state.values[usize::from(register)].value.ty;
        if let Some(nullish) = nullish_comparison(&self.program.constants, instruction, register_type) {
            self.nullish_jump(state, pc, instruction, nullish);
            return true;
        }
        self.bitwise_jump(state, pc, instruction) || self.relation_jump(state, pc, instruction, comparison)
    }

    /// `taken` jumps to `target` or falls through; a backward jump polls for an interrupt first.
    fn polled_branch(&mut self, state: &mut State, pc: usize, target: usize, taken: ir::Value) {
        if target <= pc { self.interrupt_check(state, pc as u32); }
        self.branch(state, pc, target, taken);
    }

    /// A test against `null` or `undefined`: the outcome where the value's type decides it, else
    /// the tag's, with a forward test's unequal edge refining a direct object.
    fn nullish_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: NullishComparison) {
        let target = instruction.jump_target() as usize;
        let ty = state.values[usize::from(comparison.source)].value.ty;
        if let Some(taken) = comparison.known_outcome(ty) {
            let taken = self.ins().iconst(types::I8, i64::from(taken));
            self.polled_branch(state, pc, target, taken);
        } else if target > pc {
            self.nullish_forward(state, (pc, target), comparison);
        } else {
            let exit = self.guard_exit(state, pc as u32);
            let value = self.tagged(state, comparison.source);
            let taken = emit_nullish_branch(&mut self.builder, value, comparison, self.vm.html_dda, exit);
            self.polled_branch(state, pc, target, taken);
        }
    }

    /// A forward nullish test the value's type leaves open: its two edges, and where the unequal
    /// edge's successor wants a direct object, a third that refines the value.
    fn nullish_forward(&mut self, state: &mut State, (pc, target): (usize, usize), comparison: NullishComparison) {
        let exit = self.guard_exit(state, pc as u32);
        let value = self.tagged(state, comparison.source);
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        let unequal = if comparison.negated { pc + 1 } else { target };
        let direct = self.direct_object_edge(state, value, comparison, unequal);
        emit_nullish_edges(&mut self.builder, value, comparison, self.vm.html_dda, exit, taken, fall, direct);
        match direct {
            Some(direct) => self.nullish_direct(state, (pc, target, unequal), value, comparison, [direct, taken, fall]),
            None => self.continue_branch(state, pc, target, taken, fall),
        }
    }

    /// The block of a loose nullish test's unequal edge where a register of its successor holds
    /// `value` as a direct object family.
    fn direct_object_edge(&mut self, state: &State, value: ir::Value, comparison: NullishComparison,
        unequal: usize) -> Option<ir::Block> {
        let identity = self.builder.func.dfg.resolve_aliases(value);
        let direct = !comparison.strict && self.facts.at(unequal)
            .is_some_and(|types| types.iter().enumerate().any(|(index, ty)|
                *ty == Type::DIRECT_OBJECT_FAMILY && self.is_live(unequal, index as Register)
                && matches!(state.values[index].tagged, TaggedView::Value(alias)
                    if self.builder.func.dfg.resolve_aliases(alias) == identity)));
        direct.then(|| self.builder.create_block())
    }

    /// The third edge of a nullish test: the successor of the unequal edge runs on `direct`
    /// with the value refined. The other unequal arm has already excluded this family, so it
    /// resumes before the latch's moves, with the original homes.
    fn nullish_direct(&mut self, state: &mut State, (pc, target, unequal): (usize, usize, usize), value: ir::Value,
        comparison: NullishComparison, [direct, taken, fall]: [ir::Block; 3]) {
        self.builder.switch_to_block(direct);
        let mut direct_state = state.clone();
        self.refine_object_family(&mut direct_state, value);
        self.continue_at(&mut direct_state, unequal);
        let (excluded, kept, kept_pc) = if comparison.negated { (fall, taken, target) } else { (taken, fall, pc + 1) };
        self.builder.switch_to_block(excluded);
        self.exit(state, unequal as u32);
        self.builder.switch_to_block(kept);
        self.continue_at(&mut state.clone(), kept_pc);
    }

    /// A test of a bitwise result of Numbers (`if (flags & MASK)`): the operator's bits are zero
    /// exactly when the test is false, so the Boolean is never made. Whether it lowered.
    fn bitwise_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        let Some(operation) = super::super::operations::NumberBitwise::from_operator(instruction.binary_operator_field())
            else { return false };
        if !(self.numbers_supported(left, right) && self.operand_supported(left) && self.operand_supported(right)) {
            return false;
        }
        let exit = self.guard_exit(state, pc as u32);
        let left = self.operand_to_int32(state, left, exit).expect("supported numeric operand");
        let right = self.operand_to_int32(state, right, exit).expect("supported numeric operand");
        let bits = emit_number_bitwise_bits(&mut self.builder, operation, left, right);
        let taken = self.ins().icmp_imm_u(IntCC::Equal, bits, 0);
        self.polled_branch(state, pc, instruction.jump_target() as usize, taken);
        true
    }

    /// Whether neither constant operand is anything but a Number: a register's type is for the
    /// facts to prove, a constant's is known.
    pub(super) fn numbers_supported(&self, left: Operand, right: Operand) -> bool {
        [left, right].iter().all(|operand| operand.kind() != Some(OperandKind::Constant)
            || super::super::facts::operand_type(&self.program.constants, *operand, |_| Type::ANY).is_subtype(Type::NUMBER))
    }

    /// The Number domain the facts prove of two operands: int32 where both are, binary64 where a
    /// register holds an unboxed one; `None` where only their tags type them.
    pub(super) fn number_domain(&self, state: &State, left: Operand, right: Operand) -> Option<Domain> {
        let integer = [left, right].iter().all(|operand| super::super::facts::operand_type(
            &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
            .is_subtype(Type::INT32));
        let tagged = |operand: &Operand| operand.register_index()
            .is_none_or(|register| state.values[usize::from(register)].value.representation == Representation::Tagged);
        if integer { return Some(Domain::Int32); }
        (!(tagged(&left) && tagged(&right))).then_some(Domain::Float64)
    }

    /// The comparison the facts and the operands settle on for the staged step: an ordering of
    /// operands proved Numbers in the domain they prove, an equality of operands both typed Numbers
    /// as that relation, any other equality as it is. `None` where the lane's own relation
    /// lowers it (operands only their tags type, any other operator).
    fn specialized(&self, state: &State, comparison: Comparison, [left, right]: [Operand; 2]) -> Option<Comparison> {
        if !(self.operand_supported(left) && self.operand_supported(right)) { return None; }
        let domain = self.number_domain(state, left, right);
        match comparison {
            Comparison::Relation { relation, .. } if self.numbers_supported(left, right) =>
                domain.map(|domain| Comparison::Relation { relation, domain }),
            Comparison::Equality { negated, .. } if self.numbers_typed(state, left, right) => domain.map(|domain|
                Comparison::Relation { relation: if negated { NumberRelation::NotEqual } else { NumberRelation::Equal }, domain }),
            Comparison::Equality { .. } => Some(comparison),
            _ => None,
        }
    }

    /// Whether both operands are typed Numbers: a register by its facts, a constant by its value.
    fn numbers_typed(&self, state: &State, left: Operand, right: Operand) -> bool {
        [left, right].iter().all(|operand| super::super::facts::operand_type(&self.program.constants, *operand,
            |register| state.values[usize::from(register)].value.ty).is_subtype(Type::NUMBER))
    }

    /// A relation jump: the staged step of the comparison its operands settle on, else the lane's
    /// own relation (`Lowering::relation`). Whether it lowered.
    fn relation_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: Comparison) -> bool {
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if let Some(comparison) = self.specialized(state, comparison, [left, right]) {
            self.staged_jump(state, pc, instruction, comparison);
            return true;
        }
        let operator = instruction.binary_operator_field();
        let Some(holds) = self.relation(state, pc as u32, operator, left, right) else { return false };
        let taken = self.ins().icmp_imm_u(IntCC::Equal, holds, 0);
        self.polled_branch(state, pc, instruction.jump_target() as usize, taken);
        true
    }

    /// The staged jump on `comparison` of the instruction's operands: it jumps where the
    /// comparison fails, and its edges continue the lowering. Where the VM decides a pair the words
    /// leave, the guest code it may run retires what was proved of objects.
    fn staged_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: Comparison) {
        let (target, pc32) = (instruction.jump_target() as usize, pc as u32);
        if target <= pc { self.interrupt_check(state, pc32); }
        let exit = self.guard_exit(state, pc32);
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        let row = Operation::CompareJump { comparison, left: instruction.operand_b(), right: instruction.operand_c(), negate: true };
        let site = self.decide_site(pc, &row);
        let sites = frame::Sites { call_out: site.as_ref(), ..frame::Sites::default() };
        self.step_with(state, frame::Edges::Branch { target: taken, following: fall, miss: exit }, row, None, sites);
        if comparison.runs_guest_code() { state.call_effect(); }
        self.continue_branch(state, pc, target, taken, fall);
    }
}
