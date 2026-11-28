//! The Math function rows of a Number operation: `Math.max`/`min`/`pow`/`imul` and the Math functions.

use super::*;

/// The Math function `operation` of one Number `source`, as a double.
#[inline(always)]
pub(super) fn math<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand, operation: super::super::math::FloatUnary,
) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(value) = frame.number_operand(stage, source, miss).map(super::super::Float64::word) else { return Step::Unresolved };
    let result = super::super::math::unary(stage, operation, value);
    let fact = frame.number_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `Math.max` or `Math.min` of two Numbers, a double; any other operand leaves. Two int32s, which the frame proves, give
/// the int32 their order selects (`Math.max` of int32s is the greater of them, never NaN or a signed zero).
#[inline(always)]
pub(super) fn extreme<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, (left, right): (Operand, Operand), maximum: bool,
) -> Step<S::Word, F::Control> {
    if let (Some(x), Some(y)) = (proven_int_word(stage, frame, left), proven_int_word(stage, frame, right)) {
        let greater = stage.relation(NumberRelation::GreaterThan, Domain::Int32, x, y);
        let chosen = if maximum { stage.select(greater, x, y) } else { stage.select(greater, y, x) };
        let chosen = frame.int_result(stage, chosen);
        let fact = frame.int_fact();
        frame.write(stage, destination, chosen, fact);
        return Step::Value(chosen);
    }
    let Some((left, right)) = operands(stage, frame, left, right, Domain::Float64) else { return Step::Unresolved };
    let result = super::super::math::extreme(stage, maximum, left, right);
    let fact = frame.number_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `base ** exponent` of two Numbers: a program constant `power::SmallPower` names is its product on the base
/// (`power::power_by`), any other exponent the power leaf (`NumericStage::float_power`). A stencil's numeric row holds no
/// call of the leaf, so there a constant exponent that is no `SmallPower` leaves (a register exponent is the b-jit's leaf
/// call, `call_numeric_leaf`). A base or an exponent that is no Number leaves at the miss.
#[inline(always)]
pub(super) fn power<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, base: Operand, exponent: Operand) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let small = frame.constant_operand(exponent).and_then(constant_number).and_then(super::super::power::SmallPower::of);
    let Some(base) = frame.number_operand(stage, base, miss).map(super::super::Float64::word) else { return Step::Unresolved };
    let result = match small {
        Some(small) => super::super::power::power_by(stage, small, base),
        #[cfg(not(stencil_build))]
        None => {
            let Some(exponent) = frame.number_operand(stage, exponent, miss).map(super::super::Float64::word) else { return Step::Unresolved };
            stage.float_power(base, exponent)
        }
        #[cfg(stencil_build)]
        None => return Step::Unresolved,
    };
    let fact = frame.number_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `Math.imul` of two int32s: their wrapping product, `math::multiply`; any other operand leaves.
#[inline(always)]
pub(super) fn imul<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand,
) -> Step<S::Word, F::Control> {
    let Some((left, right)) = operands(stage, frame, left, right, Domain::Int32) else { return Step::Unresolved };
    let result = super::super::math::multiply(stage, left, right);
    let result = frame.int_result(stage, result);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}
