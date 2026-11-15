use crate::float_layout::{BINARY64, power_of_two};
use num_bigint::{BigInt, Sign};

#[derive(Default)]
pub(super) struct PreciseSumState {
    infinity: Option<bool>,
    invalid: bool,
    positive_zero: bool,
    nonzero: bool,
}

impl PreciseSumState {
    pub(super) fn add(&mut self, value: f64, sum: &mut BigInt) {
        if value.is_nan() {
            self.invalid = true;
        } else if value.is_infinite() {
            self.invalid |= self
                .infinity
                .is_some_and(|sign| sign != value.is_sign_positive());
            self.infinity.get_or_insert_with(|| value.is_sign_positive());
        } else if value == 0.0 {
            self.positive_zero |= value.is_sign_positive();
        } else {
            self.nonzero = true;
            let bits = value.to_bits();
            let fraction = bits & BINARY64.fraction_mask();
            let exponent = ((bits >> BINARY64.fraction_bits) & BINARY64.exponent_mask()) as usize;
            let significand = if exponent == 0 {
                fraction
            } else {
                fraction | BINARY64.implicit_bit()
            };
            let units = BigInt::from(significand) << exponent.saturating_sub(1);
            *sum += if value.is_sign_negative() {
                -units
            } else {
                units
            };
        }
    }

    pub(super) fn finish(self, sum: BigInt) -> f64 {
        if self.invalid {
            f64::NAN
        } else if let Some(positive) = self.infinity {
            if positive {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            }
        } else if sum == BigInt::from(0_u8) {
            if !self.nonzero && !self.positive_zero {
                -0.0
            } else {
                0.0
            }
        } else {
            scaled_binary_sum(sum)
        }
    }
}

fn scaled_binary_sum(sum: BigInt) -> f64 {
    let negative = sum.sign() == Sign::Minus;
    let magnitude = if negative { -sum } else { sum };
    let bits = magnitude.magnitude().bits() as usize;
    let least = BINARY64.subnormal_exponent();
    let rounded = if bits < f64::MANTISSA_DIGITS as usize {
        magnitude.to_string().parse::<f64>().unwrap_or(f64::NAN) * power_of_two(least)
    } else {
        let shift = bits - f64::MANTISSA_DIGITS as usize;
        let mut significand = &magnitude >> shift;
        let remainder = &magnitude - (&significand << shift);
        let halfway = BigInt::from(1_u8) << (shift - 1);
        if remainder > halfway
            || remainder == halfway && (&significand & BigInt::from(1_u8)) != BigInt::from(0_u8)
        {
            significand += 1_u8;
        }
        significand.to_string().parse::<f64>().unwrap_or(f64::NAN) * power_of_two(shift as i32 + least)
    };
    if negative { -rounded } else { rounded }
}
