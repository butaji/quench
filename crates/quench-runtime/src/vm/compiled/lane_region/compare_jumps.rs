//! The conditional jump on two values (`Operation::CompareJump`, the `JumpBinaryFalse` row),
//! lowered by its comparison and by what the facts prove of its operands: a test against `null`
//! or `undefined`, a bitwise test, an ordering, an equality or `instanceof` by the staged step
//! (whose answer picks the edge through `Frame::branch`), and any other by the lane's own relation.
use super::*;
use crate::staged::{Comparison, Domain, Nullish, Operation};
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
        if let Some((comparison, operands)) = self.nullish_test(state, instruction) {
            self.staged_jump(state, pc, instruction, comparison, operands);
            return true;
        }
        self.relation_jump(state, pc, instruction, comparison)
    }

    /// The nullish test the facts settle on: an equality of a register with an operand typed exactly
    /// `null` or `undefined`, the tested register first.
    fn nullish_test(&self, state: &State, instruction: WideInstruction) -> Option<(Comparison, [Operand; 2])> {
        let register_type = |register: Register| state.values[usize::from(register)].value.ty;
        let NullishComparison { source, expected, strict, negated } =
            nullish_comparison(&self.program.constants, instruction, register_type)?;
        let (b, c) = (instruction.operand_b(), instruction.operand_c());
        let other = if c.register_index() == Some(source) { b } else { c };
        Some((Comparison::Nullish { expected: Nullish::of(expected)?, strict, negated }, [Operand::register(source), other]))
    }

    /// `taken` jumps to `target` or falls through; a backward jump polls for an interrupt first.
    fn polled_branch(&mut self, state: &mut State, pc: usize, target: usize, taken: ir::Value) {
        if target <= pc { self.interrupt_check(state, pc as u32); }
        self.branch(state, pc, target, taken);
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
    /// as that relation, a bitwise test of Numbers, and an equality or `instanceof` of registers
    /// where the VM can decide a pair the words leave. `None` where the lane's own relation lowers
    /// it (operands only their tags type, a region with no VM helper, any other operator).
    fn specialized(&self, state: &State, pc: usize, comparison: Comparison, operands: [Operand; 2]) -> Option<Comparison> {
        let [left, right] = operands;
        if !(self.operand_supported(left) && self.operand_supported(right)) { return None; }
        let domain = self.number_domain(state, left, right);
        let by_vm = self.decide_record(pc, operands).is_some();
        match comparison {
            Comparison::Relation { relation, .. } if self.numbers_supported(left, right) =>
                domain.map(|domain| Comparison::Relation { relation, domain }),
            Comparison::Equality { negated, .. } if self.numbers_typed(state, left, right) => domain.map(|domain|
                Comparison::Relation { relation: if negated { NumberRelation::NotEqual } else { NumberRelation::Equal }, domain }),
            Comparison::Equality { .. } | Comparison::Instanceof if by_vm => Some(comparison),
            Comparison::Bitwise(_) if self.numbers_supported(left, right) => Some(comparison),
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
        if let Some(comparison) = self.specialized(state, pc, comparison, [left, right]) {
            self.staged_jump(state, pc, instruction, comparison, [left, right]);
            return true;
        }
        let operator = instruction.binary_operator_field();
        let Some(holds) = self.relation(state, pc as u32, operator, left, right) else { return false };
        let taken = self.ins().icmp_imm_u(IntCC::Equal, holds, 0);
        self.polled_branch(state, pc, instruction.jump_target() as usize, taken);
        true
    }

    /// The staged jump on `comparison` of `operands`: it jumps where the
    /// comparison fails, and its edges continue the lowering. Where the VM decides a pair the words
    /// leave, the guest code it may run retires what was proved of objects.
    fn staged_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: Comparison,
        [left, right]: [Operand; 2]) {
        let (target, pc32) = (instruction.jump_target() as usize, pc as u32);
        if target <= pc { self.interrupt_check(state, pc32); }
        let exit = self.guard_exit(state, pc32);
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        let row = Operation::CompareJump { comparison, left, right, negate: true };
        let site = self.decide_record(pc, [left, right]).filter(|_| comparison.operator().is_some())
            .map(|record| self.call_out_site(pc, record));
        let sites = frame::Sites { call_out: site.as_ref(), ..frame::Sites::default() };
        self.step_with(state, frame::Edges::Branch { target: taken, following: fall, miss: exit }, row, None, sites);
        if comparison.runs_guest_code() { state.call_effect(); }
        self.continue_branch(state, pc, target, taken, fall);
    }
}
