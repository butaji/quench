//! Runtime glue of `compare`: the decode of a comparison from a `JumpBinaryFalse` operator
//! (`Comparison::of_operator`) and the compare-and-jump row of `Operation::decode`.
#![allow(unused_imports)]
use super::*;

/// The comparison a `JumpBinaryFalse` operator names (`Comparison::of_operator`).
pub trait ComparisonCompareExt: Sized {
    fn of_operator(operator: u32) -> Self;
}

impl ComparisonCompareExt for Comparison {
    #[cfg(not(stencil_build))]
    /// The comparison `operator` names (a `JumpBinaryFalse`'s operator field): the widest Number
    /// relation, an equality, a bitwise test, `instanceof`, else the operator itself. A tier with
    /// more facts than the operator narrows it (the observed Numbers, the proved domain).
    #[inline(always)]
    fn of_operator(operator: u32) -> Self {
        equality_operator(operator)
            .map(|(strict, negated)| Self::Equality { strict, negated })
            .or_else(|| NumberRelation::from_operator(operator)
                .map(|relation| Self::Relation { relation, domain: Domain::Float64 }))
            .or_else(|| NumberBitwise::from_operator(operator).map(Self::Bitwise))
            .or_else(|| (operator == INSTANCEOF_OPERATOR).then_some(Self::Instanceof))
            .unwrap_or(Self::Operator(operator))
    }
}

/// The compare-and-jump row of `decode` (`decode_generic` reads `JumpBinaryFalse` through it).
pub trait OperationCompareExt<W, P>: Sized {
    fn decode_compare(instruction: WideInstruction) -> Self;
}

impl<W, P> OperationCompareExt<W, P> for Operation<W, P> {
    #[cfg(not(stencil_build))]
    /// The `JumpBinaryFalse` row: it jumps where the comparison fails. Inlined into the row,
    /// which decodes its record on every run.
    #[inline(always)]
    fn decode_compare(instruction: WideInstruction) -> Self {
        let comparison = Comparison::of_operator(instruction.binary_operator_field());
        Self::CompareJump { comparison, left: instruction.operand_b(), right: instruction.operand_c(), negate: true }
    }
}

