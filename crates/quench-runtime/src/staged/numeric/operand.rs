//! Reads of a Number operand as an int32 or binary64 word, proved by the frame.

use super::*;

/// The int32 word of an operand the frame proves an int32 (a register its type fixes, or an int32 constant), with no
/// test; `None` for any other operand, which the caller reads as a double.
#[inline(always)]
pub(super) fn proven_int_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operand: Operand) -> Option<S::Word> {
    match frame.constant_operand(operand) {
        Some(value) => Some(super::super::Int32::constant(stage, value.as_int()?).word()),
        None => frame.held_int(stage, operand).map(super::super::Int32::word),
    }
}

/// Both operands in `domain`'s representation, each proved by the frame; `None` where either
/// is not.
#[inline(always)]
pub(super) fn operands<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, domain: Domain) -> Option<(S::Word, S::Word)> {
    let miss = frame.miss();
    Some(match domain {
        Domain::Int32 => (int32_word(stage, frame, left, miss)?.word(), int32_word(stage, frame, right, miss)?.word()),
        Domain::Float64 => (float_word(stage, frame, left, miss)?.word(), float_word(stage, frame, right, miss)?.word()),
    })
}

/// `operand`'s int32 word: a constant int32 is the stage's own constant (`i32_constant`), so a constant
/// left operand folds in every tier; any other operand is `int_operand`'s.
#[inline(always)]
pub(super) fn int32_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operand: Operand, miss: S::Miss) -> Option<super::super::IntOf<S>> {
    match frame.constant_operand(operand) {
        Some(value) => Some(super::super::Int32::constant(stage, value.as_int()?)),
        None => frame.int_operand(stage, operand, miss),
    }
}

/// `operand`'s binary64 word: a constant Number is the stage's own constant (`f64_constant`), else
/// `number_operand`'s. Subsumes the constant-left Number handlers (`lane_operator_constant_left`, one per
/// operator): the operator is the row's, and the constant folds in every tier.
#[inline(always)]
pub(super) fn float_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operand: Operand, miss: S::Miss) -> Option<super::super::NumberOf<S>> {
    match frame.constant_operand(operand) {
        Some(value) => Some(super::super::Float64::constant(stage, value.as_number()?)),
        None => frame.number_operand(stage, operand, miss),
    }
}
