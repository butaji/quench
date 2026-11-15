use crate::float_layout::{BINARY16, BINARY64, power_of_two};

pub(super) fn f16_round(value: f64) -> f64 {
    half_to_f64(f64_to_half(value))
}

pub(super) fn f64_to_half(value: f64) -> u16 {
    let (wide, half) = (BINARY64, BINARY16);
    let narrowing = wide.fraction_bits - half.fraction_bits;
    let bits = value.to_bits();
    let sign = ((bits >> wide.sign_shift()) as u16) << half.sign_shift();
    let exponent = (bits >> wide.fraction_bits) & wide.exponent_mask();
    let fraction = bits & wide.fraction_mask();
    let infinity = half.infinity() as u16;
    if exponent == wide.exponent_mask() {
        // A NaN keeps its top payload bits, and at least one so it stays a NaN.
        return sign
            | if fraction == 0 { infinity } else { infinity | ((fraction >> narrowing) as u16).max(1) };
    }
    let absolute = value.abs();
    if absolute < power_of_two(half.min_normal_exponent()) {
        // Rounding up past the subnormals gives the smallest normal.
        let rounded = round_half(absolute * power_of_two(-half.subnormal_exponent()));
        return sign | rounded.min(half.implicit_bit() as u16);
    }
    let unbiased = exponent as i32 - wide.bias();
    if unbiased > half.bias() {
        return sign | infinity;
    }
    let mut significand = (fraction >> narrowing) as u16;
    let remainder = fraction & ((1 << narrowing) - 1);
    let halfway = 1 << (narrowing - 1);
    if remainder > halfway || (remainder == halfway && significand & 1 != 0) {
        significand += 1;
    }
    // A significand rounded up to the implicit bit carries into the exponent, and from the
    // largest exponent into infinity.
    let encoded = (((unbiased + half.bias()) as u16) << half.fraction_bits) + significand;
    sign | encoded.min(infinity)
}

/// The fraction of a value halfway between two integers, where rounding takes the even one.
const HALF: f64 = 0.5;

fn round_half(value: f64) -> u16 {
    let lower = value.floor() as u64;
    let fraction = value - lower as f64;
    (lower + u64::from(fraction > HALF || (fraction == HALF && lower & 1 != 0))) as u16
}

pub(super) fn half_to_f64(bits: u16) -> f64 {
    let (wide, half) = (BINARY64, BINARY16);
    let bits = u64::from(bits);
    let sign = (bits >> half.sign_shift()) << wide.sign_shift();
    let magnitude = bits & !(1 << half.sign_shift());
    let magnitude = if magnitude >= half.infinity() {
        // Infinity and NaN keep their payload in the top fraction bits.
        let payload = (magnitude & half.fraction_mask()) << (wide.fraction_bits - half.fraction_bits);
        f64::from_bits(wide.infinity() | payload)
    } else {
        let (significand, exponent) = half.parts(magnitude);
        significand as f64 * power_of_two(exponent)
    };
    f64::from_bits(magnitude.to_bits() | sign)
}
