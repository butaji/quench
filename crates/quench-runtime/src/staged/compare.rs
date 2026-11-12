//! A conditional jump on a comparison of two operands (`Operation::CompareJump`, the fused
//! `JumpBinaryFalse`): each comparison is decided once over the stage, and its answer picks the
//! control edge through the frame's `branch`, so the interpreter, the lane and every other tier
//! jump on one definition. A pair the words do not decide is the VM's (`CallOut::Decide`).

use super::{CallOut, Domain, NumericFrame, NumericStage, Operation, Step};
use crate::bytecode::{Operand, WideInstruction};
use crate::vm::operations::{equality_binary_operator, equality_operator, NumberRelation};
use crate::Value;

/// What a `CompareJump` compares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Comparison {
    /// A Number relation of two operands the tier proved Numbers, in the `domain` it proved;
    /// the decode names the widest, `Float64`.
    Relation { relation: NumberRelation, domain: Domain },
    /// `==` or `===` (`strict`), negated for `!=` and `!==`, of operands of any type: the words
    /// decide the common pairs and the VM the rest.
    Equality { strict: bool, negated: bool },
    /// Any other operator (`instanceof`, `in`, a bitwise test): no Number relation names it, so
    /// a tier decides it by the operator, and the step is `Unresolved` for the interpreter's
    /// exact path.
    Operator(u32),
}

impl Comparison {
    /// The binary operator the VM decides this comparison by; `None` for a Number relation,
    /// which no VM step decides.
    pub(crate) fn operator(self) -> Option<u32> {
        match self {
            Self::Relation { .. } => None,
            Self::Equality { strict, negated } => Some(equality_binary_operator(strict, negated)),
            Self::Operator(operator) => Some(operator),
        }
    }

    /// Whether deciding it may run guest code (a conversion, a trap), which retires what a
    /// tier proved of objects: every comparison but a Number relation and a strict equality.
    pub(crate) const fn runs_guest_code(self) -> bool {
        !matches!(self, Self::Relation { .. } | Self::Equality { strict: true, .. })
    }
}

impl<W, P> Operation<W, P> {
    /// The `JumpBinaryFalse` row: it jumps where the comparison fails.
    pub(super) fn decode_compare(instruction: WideInstruction) -> Self {
        let operator = instruction.binary_operator_field();
        let comparison = equality_operator(operator)
            .map(|(strict, negated)| Comparison::Equality { strict, negated })
            .or_else(|| NumberRelation::from_operator(operator)
                .map(|relation| Comparison::Relation { relation, domain: Domain::Float64 }))
            .unwrap_or(Comparison::Operator(operator));
        Self::CompareJump { comparison, left: instruction.operand_b(), right: instruction.operand_c(), negate: true }
    }
}

/// Run the comparison jump `operation`: the edge its answer takes, or `Unresolved` where the
/// tier decides the comparison itself.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let Operation::CompareJump { comparison, left, right, negate } = operation else { return Step::Unresolved };
    let Some(holds) = outcome(stage, frame, comparison, [left, right]) else { return Step::Unresolved };
    let taken = if negate { stage.not(holds) } else { holds };
    Step::Control(frame.branch(stage, taken))
}

/// Whether `comparison` of `operands` holds; `None` where only the interpreter decides it.
#[inline(always)]
fn outcome<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, comparison: Comparison,
    [left, right]: [Operand; 2]) -> Option<S::Cond> {
    match comparison {
        Comparison::Relation { relation, domain } =>
            Some(super::numeric::relation_holds(stage, frame, left, right, relation, domain)),
        Comparison::Equality { strict, negated } => equality(stage, frame, [left, right], (strict, negated)),
        Comparison::Operator(_) => None,
    }
}

/// Whether the pair is equal (unequal for `negated`): the words decide it where they can
/// (`generic::decide`), the VM's `CallOut::Decide` otherwise; an operand that is no register
/// leaves a pair the words do not decide at the frame's miss edge.
#[inline(always)]
fn equality<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, [left, right]: [Operand; 2],
    (strict, negated): (bool, bool)) -> Option<S::Cond> {
    let words = [frame.operand_word(stage, left)?, frame.operand_word(stage, right)?];
    let (decided, equal) = super::generic::decide(stage, words, strict);
    let holds = if negated { stage.not(equal) } else { equal };
    let (decision, _) = frame.boolean(stage, holds);
    let value = match [left.register_index(), right.register_index()] {
        [Some(left), Some(right)] => {
            let call = CallOut::Decide { left, right, operator: equality_binary_operator(strict, negated) };
            stage.choose_leaving(decided, |_| Some(decision), |stage| frame.vm_step(stage, call, None).map(|(word, _)| word))?
        }
        _ => {
            let miss = frame.miss();
            stage.guard(decided, miss)?;
            decision
        }
    };
    Some(stage.eq_imm(value, Value::TRUE.to_bits()))
}

#[cfg(test)]
mod tests;
