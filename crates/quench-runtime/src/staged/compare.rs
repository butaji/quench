//! A conditional jump on a comparison of two operands (`Operation::CompareJump`, the fused
//! `JumpBinaryFalse`): the comparison is the staged Number relation, and its answer picks the
//! control edge through the frame's `branch`, so the interpreter, the lane and every other tier
//! jump on one definition.

use super::{Domain, NumericFrame, NumericStage, Operation, Step};
use crate::bytecode::WideInstruction;
use crate::vm::operations::NumberRelation;

/// What a `CompareJump` compares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Comparison {
    /// A Number relation of two operands the tier proved Numbers, in the `domain` it proved;
    /// the decode names the widest, `Float64`.
    Relation { relation: NumberRelation, domain: Domain },
    /// Any other operator (`instanceof`, `in`, a bitwise test): no Number relation names it, so
    /// a tier decides it by the operator, and the step is `Unresolved` for the interpreter's
    /// exact path.
    Operator(u32),
}

impl<W, P> Operation<W, P> {
    /// The `JumpBinaryFalse` row: it jumps where the comparison fails.
    pub(super) fn decode_compare(instruction: WideInstruction) -> Self {
        let operator = instruction.binary_operator_field();
        let comparison = NumberRelation::from_operator(operator).map_or(Comparison::Operator(operator),
            |relation| Comparison::Relation { relation, domain: Domain::Float64 });
        Self::CompareJump { comparison, left: instruction.operand_b(), right: instruction.operand_c(), negate: true }
    }
}

/// Run the comparison jump `operation`: the edge its answer takes, or `Unresolved` where the
/// tier decides the comparison itself.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let Operation::CompareJump { comparison: Comparison::Relation { relation, domain }, left, right, negate } = operation
        else { return Step::Unresolved };
    let holds = super::numeric::relation_holds(stage, frame, left, right, relation, domain);
    let taken = if negate { stage.not(holds) } else { holds };
    Step::Control(frame.branch(stage, taken))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Op, Operand};
    use crate::staged::tests::ConcreteFrame;
    use crate::staged::{Concrete, Edge};
    use crate::Value;
    use oxc_ast::ast::BinaryOperator;

    /// The edge a comparison jump of registers 0 and 1 takes on the interpreter's stage.
    fn edge(comparison: Comparison, [left, right]: [Value; 2], negate: bool) -> Edge {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = ConcreteFrame { registers: vec![left.to_bits(), right.to_bits()], heap: None };
        let operation = Operation::CompareJump { comparison, left: Operand::register(0), right: Operand::register(1), negate };
        match super::super::step(&mut stage, &mut frame, operation) {
            Step::Control(edge) => edge,
            _ => panic!("a relation of Numbers jumps one way or the other"),
        }
    }

    /// The relation by the language's own operators over binary64, independent of the staged one.
    fn probe(relation: NumberRelation, left: f64, right: f64) -> bool {
        match relation {
            NumberRelation::Equal => left == right,
            NumberRelation::NotEqual => left != right,
            NumberRelation::LessThan => left < right,
            NumberRelation::LessEqual => left <= right,
            NumberRelation::GreaterThan => left > right,
            NumberRelation::GreaterEqual => left >= right,
        }
    }

    const RELATIONS: [NumberRelation; 6] = [NumberRelation::Equal, NumberRelation::NotEqual, NumberRelation::LessThan,
        NumberRelation::LessEqual, NumberRelation::GreaterThan, NumberRelation::GreaterEqual];

    #[test]
    fn a_relation_jump_takes_the_edge_the_language_decides_in_both_domains() {
        let numbers = [0.0, -0.0, 1.0, -3.0, 2.5, f64::NAN, f64::INFINITY];
        for relation in RELATIONS {
            for (left, right) in numbers.iter().flat_map(|left| numbers.iter().map(move |right| (*left, *right))) {
                let holds = probe(relation, left, right);
                let values = [Value::number(left), Value::number(right)];
                let float = Comparison::Relation { relation, domain: Domain::Float64 };
                assert_eq!(edge(float, values, false), if holds { Edge::Target } else { Edge::Following }, "{relation:?} {left} {right}");
                assert_eq!(edge(float, values, true), if holds { Edge::Following } else { Edge::Target }, "negated {relation:?} {left} {right}");
            }
        }
    }

    #[test]
    fn an_int32_relation_jump_compares_the_integers() {
        let integers = [-7, 0, 3, i32::MAX, i32::MIN];
        for relation in RELATIONS {
            for (left, right) in integers.iter().flat_map(|left| integers.iter().map(move |right| (*left, *right))) {
                let holds = probe(relation, f64::from(left), f64::from(right));
                let values = [Value::integer(left), Value::integer(right)];
                let int = Comparison::Relation { relation, domain: Domain::Int32 };
                assert_eq!(edge(int, values, true), if holds { Edge::Following } else { Edge::Target }, "{relation:?} {left} {right}");
            }
        }
    }

    /// `JumpBinaryFalse` decodes to a jump on the failing comparison: a relational operator to
    /// its relation, any other operator to itself.
    #[test]
    fn jump_binary_false_decodes_to_a_jump_where_the_comparison_fails() {
        let decode = |operator: BinaryOperator| {
            let (left, right) = (Operand::register(1).0, Operand::register(2).0);
            let instruction = WideInstruction::new(Op::JumpBinaryFalse, operator as u16, left, right, 9);
            Operation::<u64, ()>::decode(instruction, |_| None)
        };
        let Some(Operation::CompareJump { comparison, left, right, negate }) = decode(BinaryOperator::LessThan)
            else { panic!("a comparison jump decodes") };
        assert_eq!(comparison, Comparison::Relation { relation: NumberRelation::LessThan, domain: Domain::Float64 });
        assert_eq!((left.register_index(), right.register_index(), negate), (Some(1), Some(2), true));
        let Some(Operation::CompareJump { comparison, .. }) = decode(BinaryOperator::Instanceof)
            else { panic!("a comparison jump decodes") };
        assert_eq!(comparison, Comparison::Operator(BinaryOperator::Instanceof as u32));
    }

    #[test]
    fn an_operator_no_relation_names_is_left_to_the_tier() {
        let operation = Operation::CompareJump { comparison: Comparison::Operator(BinaryOperator::In as u32),
            left: Operand::register(0), right: Operand::register(1), negate: true };
        let mut frame = ConcreteFrame { registers: vec![0, 0], heap: None };
        let mut stage = Concrete::new(std::ptr::null());
        assert!(matches!(super::super::step(&mut stage, &mut frame, operation), Step::Unresolved));
    }
}
