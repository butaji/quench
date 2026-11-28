//! Binary floating-point layouts (IEEE 754): the one authority for the field widths, masks
//! and exponent bias of each binary format the engine reads bit by bit, and for splitting a
//! finite value into an integer significand and a binary exponent.

/// A binary interchange format: a sign bit, a biased exponent field, then the fraction.
#[derive(Clone, Copy)]
pub(crate) struct FloatLayout {
    pub(crate) width: u32,
    /// The explicit fraction bits; normal values carry one more, implicit, bit above them.
    pub(crate) fraction_bits: u32,
}

/// `f64`: the Number type.
pub(crate) const BINARY64: FloatLayout = FloatLayout { width: u64::BITS, fraction_bits: f64::MANTISSA_DIGITS - 1 };
/// `Float16Array`, `DataView.prototype.getFloat16` and `Math.f16round`.
pub(crate) const BINARY16: FloatLayout = FloatLayout { width: u16::BITS, fraction_bits: 10 };

impl FloatLayout {
    pub(crate) const fn sign_shift(self) -> u32 {
        self.width - 1
    }

    pub(crate) const fn fraction_mask(self) -> u64 {
        self.implicit_bit() - 1
    }

    /// The significand bit a normal value carries above its fraction.
    pub(crate) const fn implicit_bit(self) -> u64 {
        1 << self.fraction_bits
    }

    /// The exponent field, unshifted; all ones encodes infinity and NaN.
    pub(crate) const fn exponent_mask(self) -> u64 {
        (1 << (self.sign_shift() - self.fraction_bits)) - 1
    }

    pub(crate) const fn bias(self) -> i32 {
        (self.exponent_mask() >> 1) as i32
    }

    /// The exponent of the smallest normal value, which subnormals share.
    pub(crate) const fn min_normal_exponent(self) -> i32 {
        1 - self.bias()
    }

    /// The exponent of the significand's last bit at the smallest exponent: the least
    /// subnormal is 2 to this power.
    pub(crate) const fn subnormal_exponent(self) -> i32 {
        self.min_normal_exponent() - self.fraction_bits as i32
    }

    /// Positive infinity's bits.
    pub(crate) const fn infinity(self) -> u64 {
        self.exponent_mask() << self.fraction_bits
    }

    /// A finite magnitude (sign bit clear) as `significand × 2^exponent`.
    pub(crate) const fn parts(self, bits: u64) -> (u64, i32) {
        let biased = (bits >> self.fraction_bits) & self.exponent_mask();
        let fraction = bits & self.fraction_mask();
        if biased == 0 {
            (fraction, self.subnormal_exponent())
        } else {
            (fraction | self.implicit_bit(), biased as i32 - self.bias() - self.fraction_bits as i32)
        }
    }
}

/// A finite Number's magnitude as `significand × 2^exponent`.
pub(crate) fn binary64_parts(value: f64) -> (u64, i32) {
    BINARY64.parts(value.abs().to_bits())
}

/// 2^exponent, exact down to the least subnormal, zero below it and infinity past the
/// largest exponent. (`powi` of a negative exponent takes the reciprocal of a positive power,
/// which overflows to infinity, and so to zero, below 2^-1023.)
pub(crate) fn power_of_two(exponent: i32) -> f64 {
    let layout = BINARY64;
    let bits = if exponent > layout.bias() {
        layout.infinity()
    } else if exponent >= layout.min_normal_exponent() {
        ((exponent + layout.bias()) as u64) << layout.fraction_bits
    } else if exponent >= layout.subnormal_exponent() {
        1 << (exponent - layout.subnormal_exponent())
    } else {
        0
    };
    f64::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_of_two_is_exact_across_the_whole_range() {
        for exponent in BINARY64.subnormal_exponent()..=BINARY64.bias() {
            let (significand, scale) = binary64_parts(power_of_two(exponent));
            assert_eq!((significand >> significand.trailing_zeros(), scale + significand.trailing_zeros() as i32), (1, exponent));
        }
        assert_eq!(power_of_two(BINARY64.subnormal_exponent() - 1), 0.0);
        assert_eq!(power_of_two(BINARY64.bias() + 1), f64::INFINITY);
    }

    #[test]
    fn parts_rebuild_every_class_of_finite_value() {
        for value in [f64::MIN_POSITIVE, f64::MAX, 5e-324, 1e-300, 0.1, 1.0, 3.0, 2f64.powi(63)] {
            let (significand, exponent) = binary64_parts(value);
            assert_eq!(significand as f64 * power_of_two(exponent), value);
        }
    }
}
