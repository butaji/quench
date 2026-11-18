//! `parseInt` of a string's code units in radix 10, the one definition the interpreter's fast path (`vm::primitives`)
//! and the b-jit's parse row (`heap_rows`) share. It answers the int32 that `string_extra::parse_integer` answers for
//! text of ASCII whitespace, one optional sign and ASCII digits running to the text's end, when that value fits an int32
//! and is no negative zero. Any other text answers `None`, and the caller runs the full conversion.

/// The int32 `parseInt(text)` (radix 10) answers for the code units of `text`, or `None` where the text is any other form.
pub(crate) fn decimal_int32(units: impl Iterator<Item = u16>) -> Option<i32> {
    // Phases: 0 leading whitespace, 1 after a sign, 2 digits.
    let (mut phase, mut negative, mut value) = (0u8, false, 0i64);
    for unit in units {
        let digit = if (0x30..=0x39).contains(&unit) { i64::from(unit - 0x30) } else { -1 };
        phase = match (phase, unit, digit) {
            (0, 0x09..=0x0D | 0x20, _) => 0,
            (0, 0x2B | 0x2D, _) => {
                negative = unit == 0x2D;
                1
            }
            (0..=2, _, digit) if digit >= 0 => {
                value = value * 10 + digit;
                if value > 1i64 << 31 {
                    return None;
                }
                2
            }
            _ => return None,
        };
    }
    if phase != 2 || (negative && value == 0) {
        return None;
    }
    i32::try_from(if negative { -value } else { value }).ok()
}

#[cfg(test)]
mod tests {
    use super::decimal_int32;

    fn parse(text: &str) -> Option<i32> {
        decimal_int32(text.encode_utf16())
    }

    #[test]
    fn digits_with_sign_and_leading_whitespace_fit_int32() {
        assert_eq!(parse("42"), Some(42));
        assert_eq!(parse("  \t7"), Some(7));
        assert_eq!(parse("+15"), Some(15));
        assert_eq!(parse("-15"), Some(-15));
        assert_eq!(parse("+0"), Some(0));
        assert_eq!(parse("010"), Some(10));
        assert_eq!(parse("2147483647"), Some(i32::MAX));
        assert_eq!(parse("-2147483648"), Some(i32::MIN));
    }

    #[test]
    fn every_other_text_leaves() {
        for text in ["", "   ", "-", "-0", "2147483648", "-2147483649", "12px", "1 2", "0x1f", "1.5", "- 3", "\u{a0}4", "9999999999"] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }
}
