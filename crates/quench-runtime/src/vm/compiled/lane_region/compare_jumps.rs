//! The conditional jump on two values (`Operation::CompareJump`, the `JumpBinaryFalse` row),
//! lowered by its comparison and by what the facts prove of its operands: a test against `null`
//! or `undefined`, a bitwise test, an ordering, an equality, `instanceof` or `in`, all by the
//! staged step (whose answer picks the edge through `Frame::branch`). An operator the step does
//! not name leaves the jump to the exact path.
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
    /// operands proved Numbers in the domain they prove, binary64 where only their tags type them
    /// (the step tests the tags once), an equality of operands both typed Numbers as that relation,
    /// a bitwise test of Numbers, and an equality, `instanceof` or `in` of registers in a body with
    /// the VM's helpers, whose decision runs on the operands the call-out carries. `None` where no
    /// step names it (a region with no VM helper, any other operator).
    fn specialized(&self, state: &State, comparison: Comparison, operands: [Operand; 2]) -> Option<Comparison> {
        let [left, right] = operands;
        if !(self.operand_supported(left) && self.operand_supported(right)) { return None; }
        let domain = self.number_domain(state, left, right).unwrap_or(Domain::Float64);
        let helpers = !self.helpers.is_numeric();
        match comparison.by_vm() {
            Comparison::Relation { relation, .. } if self.numbers_supported(left, right) =>
                Some(Comparison::Relation { relation, domain }),
            Comparison::Equality { negated, .. } if self.numbers_typed(state, left, right) => Some(Comparison::Relation {
                relation: if negated { NumberRelation::NotEqual } else { NumberRelation::Equal }, domain }),
            decided @ (Comparison::Equality { .. } | Comparison::Instanceof | Comparison::Membership) if helpers => Some(decided),
            Comparison::Bitwise(_) if self.numbers_supported(left, right) => Some(comparison),
            _ => None,
        }
    }

    /// Whether both operands are typed Numbers: a register by its facts, a constant by its value.
    fn numbers_typed(&self, state: &State, left: Operand, right: Operand) -> bool {
        [left, right].iter().all(|operand| super::super::facts::operand_type(&self.program.constants, *operand,
            |register| state.values[usize::from(register)].value.ty).is_subtype(Type::NUMBER))
    }

    /// A relation jump: the staged step of the comparison its operands settle on. Whether it
    /// lowered.
    fn relation_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: Comparison) -> bool {
        let operands = [instruction.operand_b(), instruction.operand_c()];
        let Some(comparison) = self.specialized(state, comparison, operands) else { return false };
        self.staged_jump(state, pc, instruction, comparison, operands);
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
        let prototype = (comparison == Comparison::Instanceof).then(|| self.hold_instanceof(state, exit, right)).flatten();
        let sites = frame::Sites { call_out: site.as_ref(), prototype, ..frame::Sites::default() };
        self.step_with(state, frame::Edges::Branch { target: taken, following: fall, miss: exit }, row, None, sites);
        if comparison.runs_guest_code() { state.call_effect(); }
        self.continue_branch(state, pc, target, taken, fall);
    }
}
