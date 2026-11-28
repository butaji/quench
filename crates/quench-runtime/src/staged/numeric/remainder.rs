//! The `%` operator over int32 and binary64 operands.

use super::*;

/// `left % right` in `domain`. An int32 remainder is an int32 except at a zero divisor (NaN)
/// and at a zero with a negative dividend (`-0`, `i32::MIN % -1` included): those leave. A
/// Float64 remainder takes `remainder_number`'s int32 fast path where both operands are int32.
#[inline(always)]
pub(super) fn remainder<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand, domain: Domain,
    (zero, divisor_nonzero): (NegativeZero, bool)) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some((left, right)) = operands(stage, frame, left, right, domain) else { return Step::Unresolved };
    let (result, fact) = match domain {
        Domain::Float64 => {
            // A pair the integral remainder does not answer (a fraction, a zero divisor, `-0`) takes
            // the binary64 remainder, which is `%` for every pair of Numbers.
            let (result, inexact) = stage.integral_remainder(left, right);
            let result = stage.choose(inexact, #[inline(always)] |stage| stage.float_remainder(left, right), #[inline(always)] |_| result);
            (result, frame.number_fact())
        }
        Domain::Int32 => {
            if !divisor_nonzero {
                let zero_divisor = stage.i32_is_zero(right);
                if stage.guard_false(zero_divisor, miss).is_none() {
                    return Step::Unresolved;
                }
            }
            let result = stage.int_remainder(left, right);
            if zero == NegativeZero::Exit {
                let is_zero = stage.i32_is_zero(result);
                let negative = stage.i32_is_negative(left);
                let negative_zero = stage.both(is_zero, negative);
                if stage.guard_false(negative_zero, miss).is_none() {
                    return Step::Unresolved;
                }
            }
            (frame.int_result(stage, result), frame.int_fact())
        }
    };
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `left % right` of two Number operands, as a binary64 word. Where both are int32 at run time,
/// the divisor is nonzero and the dividend is non-negative, the remainder is one `idiv` and a
/// conversion of its int32 result: a non-negative dividend's remainder is never `-0` and needs
/// no fmod. Every other pair takes the binary64 remainder. `None` where an operand is no Number,
/// which leaves the row.
#[inline(always)]
fn remainder_number<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand) -> Option<S::Word> {
    // A pair match rather than `Option::zip`, whose instance stays out of line across crates.
    let (Some((left_int, left_test)), Some((right_int, right_test))) =
        (fast_int(stage, frame, left), fast_int(stage, frame, right))
    else {
        return remainder_float(stage, frame, left, right);
    };
    let divisor_nonzero = stage.i32_is_zero(right_int);
    let divisor_nonzero = stage.not(divisor_nonzero);
    let dividend_negative = stage.i32_is_negative(left_int);
    let mut fast = stage.not(dividend_negative);
    fast = stage.both(fast, divisor_nonzero);
    for test in [left_test, right_test].into_iter().flatten() {
        fast = stage.both(fast, test);
    }
    stage.choose_leaving(fast, #[inline(always)] |stage| {
        let remainder = stage.int_remainder(left_int, right_int);
        Some(stage.i32_to_f64(remainder))
    }, #[inline(always)] |stage| remainder_float(stage, frame, left, right))
}

/// `left % right` of two binary64 operands (`float_word`), the general remainder.
#[inline(always)]
fn remainder_float<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand) -> Option<S::Word> {
    let (left, right) = operands(stage, frame, left, right, Domain::Float64)?;
    Some(stage.float_remainder(left, right))
}
