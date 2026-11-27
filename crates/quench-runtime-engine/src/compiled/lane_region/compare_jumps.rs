//! The conditional jump on two values (`Operation::CompareJump`, the `JumpBinaryFalse` row),
//! lowered by its comparison and by what the facts prove of its operands: a test against `null`
//! or `undefined`, a bitwise test, an ordering, an equality, `instanceof` or `in`, all by the
//! staged step (whose answer picks the edge through `Frame::branch`). An operator the step does
//! not name leaves the jump to the exact path.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::{nullish_of, Comparison, Domain, Operation};
use quench_runtime::vm::operations::NumberRelation;

impl Lowering<'_, '_> {
    /// The comparison-jump row of `instruction`, where it decodes to one.
    fn compare_row(&self, instruction: WideInstruction) -> Option<Operation<ir::Value, EmitProof>> {
        let row = Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY)?;
        matches!(row, Operation::CompareJump { .. }).then_some(row)
    }

    /// Whether `instruction` is a comparison jump.
    pub(crate) fn is_compare_jump(&self, instruction: WideInstruction) -> bool {
        self.compare_row(instruction).is_some()
    }

    /// The comparison jump at `pc`; whether it lowered, which ends the block (the jump's edges
    /// continue the lowering themselves).
    pub(crate) fn compare_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(Operation::CompareJump { comparison, .. }) = self.compare_row(instruction) else {
            unreachable!("a comparison jump decodes")
        };
        if let Some(pending) = self.pending_unit.take().filter(|pending| pending.pc == pc) {
            self.unit_compare_jump(state, pc, instruction, pending);
            return true;
        }
        if let Some((comparison, operands)) = self.nullish_test(state, instruction) {
            self.staged_jump(state, pc, instruction, comparison, operands);
            return true;
        }
        if self.strict_equality_jump(state, pc, instruction, comparison) {
            return true;
        }
        self.relation_jump(state, pc, instruction, comparison)
    }

    /// The jump of a strict comparison of a string element's unit with a one-unit constant
    /// (`Lowering::unit_compare`): where the comparison fails the jump is taken, as `staged_jump` takes
    /// it. The unit is the one-character string's, so `===` holds exactly where the unit is the constant's.
    fn unit_compare_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, pending: PendingUnit) {
        let target = instruction.jump_target() as usize;
        if target <= pc { self.back_edge_check(state, pc, target); }
        let equal = self.raw_compare_imm(Relation::Equal, pending.unit, i64::from(pending.constant));
        let holds = if pending.negated { self.raw_binary_imm(Binary::Xor, equal, 1) } else { equal };
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        self.branch_if(holds, (fall, &[]), (taken, &[]));
        self.continue_branch(state, pc, target, taken, fall);
    }

    /// A strict (in)equality jump of two operands not both typed Numbers, where the lane has its
    /// helpers: the value lowering's `equality` decides the pair (identical words, the inline string
    /// records, the cells helper for other string pairs), where the staged step would make the
    /// `Decide` call-out for every pair its words leave. Whether it lowered, which ends the block.
    fn strict_equality_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: Comparison) -> bool {
        let Comparison::Equality { strict: true, negated } = comparison else { return false };
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if self.hoisted.contains_key(&pc) || self.helpers.is_numeric() || self.numbers_typed(state, left, right)
            || !(self.operand_supported(left) && self.operand_supported(right)) {
            return false;
        }
        let Some(equal) = self.equality(state, pc as u32, left, right, true) else { return false };
        // The jump is taken where the comparison fails, as `staged_jump` takes it.
        let holds = if negated { self.raw_binary_imm(Binary::Xor, equal, 1) } else { equal };
        let target = instruction.jump_target() as usize;
        if target <= pc { self.back_edge_check(state, pc, target); }
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        self.branch_if(holds, (fall, &[]), (taken, &[]));
        self.continue_branch(state, pc, target, taken, fall);
        true
    }

    /// The nullish test the facts settle on: an equality of a register with an operand typed exactly
    /// `null` or `undefined`, the tested register first.
    fn nullish_test(&self, state: &State, instruction: WideInstruction) -> Option<(Comparison, [Operand; 2])> {
        let register_type = |register: Register| state.values[usize::from(register)].value.ty;
        let NullishComparison { source, expected, strict, negated } =
            nullish_comparison(&self.program.constants, instruction, register_type)?;
        let (b, c) = (instruction.operand_b(), instruction.operand_c());
        let other = if c.register_index() == Some(source) { b } else { c };
        Some((Comparison::Nullish { expected: nullish_of(expected)?, strict, negated }, [Operand::register(source), other]))
    }

    /// Whether neither constant operand is anything but a Number: a register's type is for the
    /// facts to prove, a constant's is known.
    pub(crate) fn numbers_supported(&self, left: Operand, right: Operand) -> bool {
        [left, right].iter().all(|operand| operand.kind() != Some(OperandKind::Constant)
            || super::super::facts::operand_type(&self.program.constants, *operand, |_| Type::ANY).is_subtype(Type::NUMBER))
    }

    /// The Number domain the facts prove of two operands: int32 where both are, binary64 where a
    /// register holds an unboxed one; `None` where only their tags type them.
    pub(crate) fn number_domain(&self, state: &State, left: Operand, right: Operand) -> Option<Domain> {
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
    fn specialized(&self, state: &State, (comparison, operator): (Comparison, u32), operands: [Operand; 2]) -> Option<Comparison> {
        let [left, right] = operands;
        if !(self.operand_supported(left) && self.operand_supported(right)) { return None; }
        let domain = self.number_domain(state, left, right).unwrap_or(Domain::Float64);
        let helpers = !self.helpers.is_numeric();
        let numberless = [left, right].iter().filter_map(|operand| operand.register_index())
            .any(|register| super::super::facts::numberless(state.values[usize::from(register)].value.ty));
        match comparison.by_vm() {
            // A relation of a register the facts prove is no Number: the VM's decision, whose
            // operands are registers (a constant operand leaves the jump to the exact path).
            Comparison::Relation { .. } if numberless && helpers
                && [left, right].iter().all(|operand| operand.register_index().is_some()) =>
                Some(Comparison::Decided(operator)),
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
        if let Some(read) = self.hoisted.get(&pc).copied() {
            self.decided_jump(state, pc, instruction, read.value);
            return true;
        }
        let operands = [instruction.operand_b(), instruction.operand_c()];
        let Some(comparison) = self.specialized(state, (comparison, instruction.binary_operator_field()), operands) else { return false };
        self.staged_jump(state, pc, instruction, comparison, operands);
        true
    }

    /// The jump on a decision its loop's entry held (`Lowering::hoist_decisions`): taken where the
    /// held Boolean is false.
    fn decided_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, held: Variable) {
        let target = instruction.jump_target() as usize;
        if target <= pc { self.back_edge_check(state, pc, target); }
        let word = self.builder.use_var(held);
        let holds = quench_runtime::staged::value::boolean_truthy(&mut emit(&mut self.builder), word);
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        self.branch_if(holds, (fall, &[]), (taken, &[]));
        self.continue_branch(state, pc, target, taken, fall);
    }

    /// The staged jump on `comparison` of `operands`: it jumps where the
    /// comparison fails, and its edges continue the lowering. Where the VM decides a pair the words
    /// leave, the guest code it may run retires what was proved of objects.
    fn staged_jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, comparison: Comparison,
        [left, right]: [Operand; 2]) {
        let (target, pc32) = (instruction.jump_target() as usize, pc as u32);
        if target <= pc { self.back_edge_check(state, pc, target); }
        let exit = self.guard_exit_for(state, pc32, ExitFact::NumberKind);
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        let row = Operation::CompareJump { comparison, left, right, negate: true };
        // A relation the VM decides by its operator alone (`Decided`) has no record step: where the
        // `Decide` helper cannot answer, the jump leaves to the exact path.
        let site = self.decide_record(pc, [left, right]).filter(|_| comparison.operator().is_some() && !matches!(comparison, Comparison::Decided(_)))
            .map(|record| self.call_out_site(pc, record, None));
        let prototype = (comparison == Comparison::Instanceof).then(|| self.hold_instanceof(state, exit, right)).flatten();
        let sites = frame::Sites { call_out: site.as_ref(), prototype, ..frame::Sites::default() };
        self.step_with(state, frame::Edges::Branch { target: taken, following: fall, miss: exit }, row, StepWith { sites, ..StepWith::default() });
        if comparison.runs_guest_code() { state.call_effect(); }
        self.continue_branch(state, pc, target, taken, fall);
    }
}
