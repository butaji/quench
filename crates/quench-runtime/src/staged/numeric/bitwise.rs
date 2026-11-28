//! Bitwise rows: `~`, the int32 bitwise operators and their word readers.

use super::*;

/// `~source` of an int32 `source`: `-1 - source`, which never overflows.
#[inline(always)]
pub(super) fn bitwise_not<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(value) = frame.int_operand(stage, source, miss).map(super::super::Int32::word) else {
        return general_unary(stage, frame, crate::vm::operations::unary_operator::BitwiseNot, (destination, source));
    };
    let all_ones = stage.i32_constant(-1);
    int_arithmetic_result(stage, frame, destination, all_ones, value,
        NumberArithmetic::Subtract, LEAVES, miss)
}

/// `operand`'s ToInt32 word for a bitwise operator: a constant's ToInt32 is the stage's own constant
/// (`i32_constant`), else `int32_operand`'s.
#[inline(always)]
pub fn bitwise_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operand: Operand, miss: S::Miss) -> Option<super::super::IntOf<S>> {
    match frame.constant_operand(operand) {
        Some(value) => {
            let int = match value.as_int() {
                Some(int) => int,
                None => crate::value::number_to_u32(value.as_number()?) as i32,
            };
            Some(super::super::Int32::constant(stage, int))
        }
        None => frame.int32_operand(stage, operand, miss),
    }
}

/// The bitwise `operation` of two operands' ToInt32, defining its int32 in `destination`. A
/// zero-fill shift leaves where its count is zero and its dividend negative (the Number is above
/// `i32::MAX`, which no int32 holds).
#[inline(always)]
pub(super) fn bitwise_value<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand, operation: NumberBitwise) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(left) = bitwise_word(stage, frame, left, miss).map(super::super::Int32::word) else { return Step::Unresolved };
    let Some(right) = bitwise_word(stage, frame, right, miss).map(super::super::Int32::word) else { return Step::Unresolved };
    if operation == NumberBitwise::ShiftRightUnsigned {
        let count = stage.and_imm(right, 31u64);
        let zero_count = stage.eq_imm(count, 0u64);
        let negative = stage.i32_is_negative(left);
        let widens = stage.both(zero_count, negative);
        // A frame of tagged words writes a result past int32 (`x >>> 0` of a negative `x`) as its boxed
        // binary64 word, under the one tagged fact; any other frame leaves there.
        if let Some(fact) = frame.tagged_number_fact() {
            let bits = stage.int_bitwise(operation, left, right);
            let int = frame.int_result(stage, bits);
            let result = stage.choose(widens, #[inline(always)] |stage| {
                let number = stage.u32_to_f64(left);
                stage.box_number(super::super::Float64::of_row(number))
            }, #[inline(always)] |_| int);
            frame.write(stage, destination, result, fact);
            return Step::Value(result);
        }
        if stage.guard_false(widens, miss).is_none() {
            return Step::Unresolved;
        }
    }
    let bits = stage.int_bitwise(operation, left, right);
    let result = frame.int_result(stage, bits);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}
