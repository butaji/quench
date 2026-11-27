//! `Number ** Number` (`Math.pow`): the constant exponents that lower to one operation on the base (`SmallPower`,
//! `power_by`, the one definition every stage row runs), and the power of any other exponent (`power`, the libm body).
use super::numeric::NumericStage;
#[cfg(not(stencil_build))]
use super::Concrete;
use crate::vm::operations::NumberArithmetic;

#[cfg(not(stencil_build))]
const EXPONENTIATION_ZERO: f64 = 0.0;
#[cfg(not(stencil_build))]
const EXPONENTIATION_ONE: f64 = 1.0;
#[cfg(not(stencil_build))]
const EXPONENTIATION_TWO: f64 = 2.0;
#[cfg(not(stencil_build))]
const ODD_INTEGER_PARITY: f64 = 1.0;

/// The constant exponents `Math.pow` (and `**`) lowers to one operation on the base: `base ** 0` is `1`, `base ** 1` is
/// `base`, `base ** 2` is `base * base` and `base ** -1` is `1 / base`. `power` itself takes these products for the
/// exponents, so every tier's power of a constant exponent and of a runtime one has the same bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmallPower {
    Zero,
    One,
    Square,
    Reciprocal,
}

impl SmallPower {
    /// The product an exponent lowers to, where it is one of the constants above (`-0` is `0`).
    pub fn of(exponent: f64) -> Option<Self> {
        if exponent == 0.0 {
            Some(Self::Zero)
        } else if exponent == 1.0 {
            Some(Self::One)
        } else if exponent == 2.0 {
            Some(Self::Square)
        } else if exponent == -1.0 {
            Some(Self::Reciprocal)
        } else {
            None
        }
    }
}

/// `base ** exponent` for the constant `small`, on `stage`: the one operation `SmallPower` names. `base` is the binary64
/// word of a Number; `base ** 0` is `1` whatever the base, NaN included.
#[inline(always)]
pub fn power_by<S: NumericStage>(stage: &mut S, small: SmallPower, base: S::Word) -> S::Word {
    match small {
        SmallPower::Zero => stage.f64_constant(1.0),
        SmallPower::One => base,
        SmallPower::Square => stage.float_arithmetic(NumberArithmetic::Multiply, base, base),
        SmallPower::Reciprocal => {
            let one = stage.f64_constant(1.0);
            stage.float_arithmetic(NumberArithmetic::Divide, one, base)
        }
    }
}

/// `Number ** Number` (`Math.pow`): NaN for a NaN exponent, the `SmallPower` products for a constant exponent, else the
/// signed infinities and zeros of the base at the edges and the libm power.
#[cfg(not(stencil_build))]
pub(crate) fn power(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if let Some(small) = SmallPower::of(exponent) {
        return f64::from_bits(power_by(&mut Concrete::pure(), small, base.to_bits()));
    }
    if base.is_nan() || base.abs() == EXPONENTIATION_ONE && exponent.is_infinite() {
        return f64::NAN;
    }
    if base.is_infinite() {
        return infinite_power(base, exponent);
    }
    if base == EXPONENTIATION_ZERO {
        return zero_power(base, exponent);
    }
    base.powf(exponent)
}

/// The power of an infinite base: infinity for a positive exponent, zero otherwise, signed by the base.
#[cfg(not(stencil_build))]
fn infinite_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        f64::INFINITY
    } else {
        EXPONENTIATION_ZERO
    };
    signed_power_magnitude(base, exponent, magnitude)
}

/// The power of a zero base: zero for a positive exponent, infinity otherwise, signed by the base.
#[cfg(not(stencil_build))]
fn zero_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        EXPONENTIATION_ZERO
    } else {
        f64::INFINITY
    };
    signed_power_magnitude(base, exponent, magnitude)
}

/// `magnitude`, negated when the base is negative and the exponent an odd integer.
#[cfg(not(stencil_build))]
fn signed_power_magnitude(base: f64, exponent: f64, magnitude: f64) -> f64 {
    if base.is_sign_negative() && is_odd_integer(exponent) {
        -magnitude
    } else {
        magnitude
    }
}

/// Whether `value` is a finite integer with an odd magnitude.
#[cfg(not(stencil_build))]
fn is_odd_integer(value: f64) -> bool {
    value.is_finite()
        && value.fract() == EXPONENTIATION_ZERO
        && value.abs() % EXPONENTIATION_TWO == ODD_INTEGER_PARITY
}
