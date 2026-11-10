//! ECMAScript integer conversions over already-converted numbers, shared by every builtin
//! and host that clamps an argument to a length.

/// ToIntegerOrInfinity of a Number: NaN and -0 become +0, an infinity stays, the rest
/// truncate toward zero.
#[inline]
pub fn to_integer_or_infinity(number: f64) -> f64 {
    if number.is_nan() {
        0.0
    } else {
        // Adding +0 turns the -0 of a truncated small negative into +0.
        number.trunc() + 0.0
    }
}

/// A relative index (`slice`, `at`, `copyWithin` and friends) resolved against `len`: a
/// negative count is from the end, and the result is clamped to `0..=len`.
#[inline]
pub fn relative_index(number: f64, len: usize) -> usize {
    let integer = to_integer_or_infinity(number);
    if integer < 0.0 {
        len.saturating_sub((-integer) as usize)
    } else {
        (integer as usize).min(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_index_clamps_like_the_spec() {
        let cases = [
            (f64::NAN, 5, 0),
            (-0.0, 5, 0),
            (f64::NEG_INFINITY, 5, 0),
            (f64::INFINITY, 5, 5),
            (-1.9, 5, 4),
            (-9.0, 5, 0),
            (2.9, 5, 2),
            (9.0, 5, 5),
            (-0.5, 5, 0),
        ];
        for (number, len, expected) in cases {
            assert_eq!(relative_index(number, len), expected, "{number}");
        }
        assert!(to_integer_or_infinity(-0.5).is_sign_positive());
    }
}
