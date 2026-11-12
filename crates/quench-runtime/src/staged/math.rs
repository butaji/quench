//! Math functions of Number arguments that a machine runs inline: their value once, for the
//! interpreter (`Concrete`, `vm::number`) and for native code (`Emit`).
use super::numeric::NumericStage;

/// A Math function of one Number that is a machine primitive or a short composition of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FloatUnary {
    Floor,
    Ceil,
    Trunc,
    Abs,
    Sqrt,
    /// The nearest binary32, as a double.
    Fround,
    /// The nearest integer, halves toward `+Infinity`, `-0` for a negative that rounds to zero.
    Round,
}

/// The value of `operation` on `value`.
#[inline(always)]
pub(crate) fn float_unary(operation: FloatUnary, value: f64) -> f64 {
    match operation {
        FloatUnary::Floor => value.floor(),
        FloatUnary::Ceil => value.ceil(),
        FloatUnary::Trunc => value.trunc(),
        FloatUnary::Abs => value.abs(),
        FloatUnary::Sqrt => value.sqrt(),
        FloatUnary::Fround => f64::from(value as f32),
        FloatUnary::Round => round_half_up(value),
    }
}

/// The fraction at which `Math.round` rounds up.
pub(crate) const ROUND_HALF: f64 = 0.5;

/// `Math.round`: the nearest integer, halves toward `+Infinity`, `-0` for a negative that rounds to zero.
#[inline(always)]
fn round_half_up(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 || value.is_infinite() {
        value
    } else if (-ROUND_HALF..0.0).contains(&value) {
        -0.0
    } else {
        let floor = value.floor();
        if value - floor < ROUND_HALF { floor } else { floor + 1.0 }
    }
}

/// `Math.max` (`maximum`) or `Math.min` of two Numbers: NaN if either is, and `-0` below `+0`
/// (the minimum of zeros is `-0` when either is, the maximum only when both are).
#[inline(always)]
pub(crate) fn float_extreme(maximum: bool, left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        return f64::NAN;
    }
    if left == 0.0 && right == 0.0 {
        let negative = if maximum {
            left.is_sign_negative() && right.is_sign_negative()
        } else {
            left.is_sign_negative() || right.is_sign_negative()
        };
        return if negative { -0.0 } else { 0.0 };
    }
    if maximum { left.max(right) } else { left.min(right) }
}

/// `operation` of the binary64 word `value`, on `stage`.
#[inline(always)]
pub(crate) fn unary<S: NumericStage>(stage: &mut S, operation: FloatUnary, value: S::Word) -> S::Word {
    stage.float_unary(operation, value)
}

/// `Math.max` (`maximum`) or `Math.min` of two binary64 words, on `stage`.
#[inline(always)]
pub(crate) fn extreme<S: NumericStage>(stage: &mut S, maximum: bool, left: S::Word, right: S::Word) -> S::Word {
    stage.float_extreme(maximum, left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extremes_order_signed_zeros_and_propagate_nan() {
        assert!(float_extreme(true, f64::NAN, 1.0).is_nan());
        assert!(float_extreme(false, 1.0, f64::NAN).is_nan());
        assert!(float_extreme(false, 0.0, -0.0).is_sign_negative());
        assert!(float_extreme(true, 0.0, -0.0).is_sign_positive());
        assert!(float_extreme(true, -0.0, -0.0).is_sign_negative());
        assert_eq!(float_extreme(true, 1.0, 2.0), 2.0);
        assert_eq!(float_extreme(false, 1.0, 2.0), 1.0);
    }

    #[test]
    fn round_takes_halves_up_and_keeps_negative_zero() {
        assert_eq!(float_unary(FloatUnary::Round, 2.5), 3.0);
        assert_eq!(float_unary(FloatUnary::Round, -2.5), -2.0);
        assert!(float_unary(FloatUnary::Round, -0.2).is_sign_negative());
        assert!(float_unary(FloatUnary::Round, f64::NAN).is_nan());
    }
}
