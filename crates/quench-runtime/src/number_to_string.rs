//! Number::toString(10) (ECMA-262 6.1.6.1.20): the shortest round-trip decimal digits of a
//! double, laid out by the magnitude of their decimal point.

/// Number::toString switches to exponential form for a decimal point past this position:
/// the 21-digit integer boundary of step 7.
const MAX_FIXED_POINT_POSITION: i32 = 21;
/// ... and for one at or before this position, below 10^-6 (step 8's `-6 < n`).
const MIN_FIXED_POINT_POSITION: i32 = -6;
/// Below 2^53 every integer is a double, so its shortest round-trip digits are exactly its
/// decimal digits, which an integer formatter writes directly.
const EXACT_INTEGER_MAGNITUDE: f64 = (1u64 << f64::MANTISSA_DIGITS) as f64;

/// `value` as Number::toString(10) writes it.
pub(crate) fn format(value: f64) -> String {
    let mut text = Vec::new();
    write(value, &mut text);
    // SAFETY: `write` appends ASCII only.
    unsafe { String::from_utf8_unchecked(text) }
}

/// Appends Number::toString(10) of `value` to `out`.
pub(crate) fn write(value: f64, out: &mut Vec<u8>) {
    if value.is_nan() {
        out.extend_from_slice(b"NaN");
        return;
    }
    if value == 0.0 {
        out.push(b'0');
        return;
    }
    if value.is_sign_negative() {
        out.push(b'-');
    }
    if value.is_infinite() {
        out.extend_from_slice(b"Infinity");
        return;
    }
    let magnitude = value.abs();
    if magnitude < EXACT_INTEGER_MAGNITUDE && magnitude.fract() == 0.0 {
        out.extend_from_slice(itoa::Buffer::new().format(magnitude as u64).as_bytes());
        return;
    }
    let mut shortest = ryu::Buffer::new();
    let text = shortest.format_finite(magnitude);
    // Ryu writes positional notation for a decimal point position in (-5, 16], within
    // Number::toString's (-6, 21], and then the same text, but for the `.0` it gives an
    // integer.
    if !text.contains('e') {
        out.extend_from_slice(text.strip_suffix(".0").unwrap_or(text).as_bytes());
        return;
    }
    let (digits, point) = decimal_digits(text);
    lay_out(&digits, point, out);
}

/// The significant digits of Ryu's shortest round-trip text for a positive double, without
/// leading or trailing zeros, and the position of the decimal point relative to the first
/// of them: the `k` digits `s` and the `n` of Number::toString, value = 0.s × 10^n.
fn decimal_digits(text: &str) -> (smallvec::SmallVec<[u8; 24]>, i32) {
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().expect("Ryu exponent")),
        None => (text, 0),
    };
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut point = integer.len() as i32 + exponent;
    let mut digits: smallvec::SmallVec<[u8; 24]> =
        integer.bytes().chain(fraction.bytes()).collect();
    let leading = digits.iter().take_while(|&&digit| digit == b'0').count();
    digits.drain(..leading);
    point -= leading as i32;
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    (digits, point)
}

/// Steps 6-10 of Number::toString for digits `s` (k of them) and point position `n`.
fn lay_out(digits: &[u8], point: i32, out: &mut Vec<u8>) {
    let count = digits.len() as i32;
    if count <= point && point <= MAX_FIXED_POINT_POSITION {
        out.extend_from_slice(digits);
        out.resize(out.len() + (point - count) as usize, b'0');
    } else if 0 < point && point <= MAX_FIXED_POINT_POSITION {
        let (integer, fraction) = digits.split_at(point as usize);
        out.extend_from_slice(integer);
        out.push(b'.');
        out.extend_from_slice(fraction);
    } else if MIN_FIXED_POINT_POSITION < point && point <= 0 {
        out.extend_from_slice(b"0.");
        out.resize(out.len() + (-point) as usize, b'0');
        out.extend_from_slice(digits);
    } else {
        let (first, rest) = digits.split_first().expect("a nonzero value has digits");
        out.push(*first);
        if !rest.is_empty() {
            out.push(b'.');
            out.extend_from_slice(rest);
        }
        let exponent = point - 1;
        out.push(b'e');
        out.push(if exponent < 0 { b'-' } else { b'+' });
        out.extend_from_slice(itoa::Buffer::new().format(exponent.unsigned_abs()).as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::format;

    /// The layout core's float formatting gives, which this module replaced: its digits are
    /// also the shortest round-trip digits.
    fn reference(value: f64) -> String {
        const EXACT_INTEGER_MAGNITUDE: f64 = (1u64 << f64::MANTISSA_DIGITS) as f64;
        if value.is_nan() {
            return "NaN".into();
        }
        if value.is_infinite() {
            return if value < 0.0 { "-Infinity" } else { "Infinity" }.into();
        }
        if value == 0.0 {
            return "0".into();
        }
        let magnitude = value.abs();
        if magnitude < EXACT_INTEGER_MAGNITUDE && value.fract() == 0.0 {
            return (value as i64).to_string();
        }
        if !(1e-6..1e21).contains(&magnitude) {
            let scientific = format!("{value:e}");
            let (mantissa, exponent) = scientific.split_once('e').unwrap();
            let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
            let exponent = exponent.parse::<i32>().unwrap();
            return format!("{mantissa}e{exponent:+}");
        }
        value.to_string()
    }

    #[test]
    fn layout_matches_number_to_string() {
        for (value, text) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (-2.5e30, "-2.5e+30"),
            (f64::MAX, "1.7976931348623157e+308"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
            (9007199254740993.0, "9007199254740992"),
            (0.000123, "0.000123"),
        ] {
            assert_eq!(format(value), text, "{value:e}");
        }
    }

    /// The significant digits of a Number::toString text, without sign, point or exponent.
    fn significant_digits(text: &str) -> usize {
        let mantissa = text.split('e').next().unwrap();
        mantissa.bytes().filter(u8::is_ascii_digit).skip_while(|&digit| digit == b'0').count()
            - mantissa.bytes().rev().take_while(|&byte| byte == b'0').count()
    }

    #[test]
    fn digits_round_trip_as_shortly_as_the_previous_formatter() {
        // A fixed linear congruential walk over the bit space, plus each power of ten. Core's
        // formatter rounds a tie between two shortest candidates away from even (0x4302f5a940e27caa
        // is 667082108456853.25: it wrote ...853.3, Number::toString and V8 write ...853.2), so
        // the texts are compared by round trip and digit count rather than byte for byte.
        let mut bits: u64 = 0x9E37_79B9_7F4A_7C15;
        let powers = (-330..=310).map(|exponent: i32| format!("1e{exponent}").parse::<f64>().unwrap());
        let walk = std::iter::repeat_with(|| {
            bits = bits.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            f64::from_bits(bits)
        });
        for value in walk.take(200_000).chain(powers) {
            let (text, previous) = (format(value), reference(value));
            if value.is_finite() {
                assert_eq!(text.parse::<f64>().unwrap(), value, "{value:e}: {text}");
            }
            assert!(
                text == previous || significant_digits(&text) == significant_digits(&previous),
                "{value:e}: {text} vs {previous}"
            );
        }
        assert_eq!(format(f64::from_bits(0x4302_f5a9_40e2_7caa)), "667082108456853.2");
    }
}
