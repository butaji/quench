//! `parseInt` of a string's code units in radix 10, a staged row: the one definition the interpreter's fast path (`vm::primitives`)
//! and the b-jit's parse row (`heap_rows`) share. It answers the int32 that `string_extra::parse_integer` answers for
//! text of ASCII whitespace, one optional sign and ASCII digits running to the text's end, when that value fits an int32
//! and is no negative zero. Any other text answers `None`, and the caller runs the full conversion.

use super::Stage;
use super::code_units::code_unit;

/// The int32 `parseInt(text)` (radix 10) answers for the `length` code units at `units` (two
/// bytes a unit where `wide`), as a word holding the int32, and whether the text has that form:
/// ASCII whitespace, one optional sign and ASCII digits running to the end, a value that fits an
/// int32 and is no negative zero. Three scans in turn, each of only its own units: the leading
/// whitespace, the sign, the digits; a text that starts with a digit skips the first two. The one body of the interpreter's fast path and the b-jit's
/// parse row.
#[inline(always)]
pub fn decimal_int32_row<S: Stage>(stage: &mut S, wide: bool, units: S::Word, length: S::Word) -> (S::Cond, S::Word) {
    // Text that starts with a digit (the common case) has no whitespace or sign to scan.
    let some = stage.ne_imm(length, 0u64);
    let zero = stage.constant(0);
    let first = stage.choose(some, #[inline(always)] |stage| code_unit(stage, wide, units, zero), #[inline(always)] |stage| stage.constant(0));
    let first_digit = stage.add_imm(first, 0x30u64.wrapping_neg());
    let ten = stage.constant(10);
    let leads = stage.ult(first_digit, ten);
    // Else the first unit past the leading whitespace (or the length), then one optional sign;
    // the sign is the start's top bit until it is split off.
    let marked = stage.choose(leads, #[inline(always)] |stage| stage.constant(0), #[inline(always)] |stage| {
        let index = stage.choose(some, #[inline(always)] |stage| {
            let zero = stage.constant(0);
            let [index] = stage.loop_words([zero], #[inline(always)] |stage, [index]| {
                let unit = code_unit(stage, wide, units, index);
                let control = stage.add_imm(unit, 0x09u64.wrapping_neg());
                let five = stage.constant(5);
                let is_control = stage.ult(control, five);
                let is_space = stage.eq_imm(unit, 0x20u64);
                let blank = stage.any(is_control, is_space);
                let next = stage.add_imm(index, 1u64);
                let more = stage.ult(next, length);
                (stage.all(blank, more), [stage.select(blank, next, index)])
            });
            index
        }, #[inline(always)] |stage| stage.constant(0));
        let inside = stage.ult(index, length);
        let unit = stage.choose(inside, #[inline(always)] |stage| code_unit(stage, wide, units, index), #[inline(always)] |stage| stage.constant(0));
        let minus = stage.eq_imm(unit, 0x2Du64);
        let plus = stage.eq_imm(unit, 0x2Bu64);
        let signed = stage.any(minus, plus);
        let after = stage.add_imm(index, 1u64);
        let start = stage.select(signed, after, index);
        let marked = stage.or_imm(start, 1u64 << 63);
        stage.select(minus, marked, start)
    });
    let sign = stage.shr_imm(marked, 63u64);
    let negative = stage.ne_imm(sign, 0u64);
    let start = stage.and_imm(marked, u64::MAX >> 1);
    // The digits to the end: their value, or all ones where a unit is no digit or the value
    // passes 2^31.
    let digits = stage.ult(start, length);
    let value = stage.choose(digits, #[inline(always)] |stage| {
        let zero = stage.constant(0);
        let [_, value] = stage.loop_words([start, zero], #[inline(always)] |stage, [index, value]| {
            let unit = code_unit(stage, wide, units, index);
            let digit = stage.add_imm(unit, 0x30u64.wrapping_neg());
            let ten = stage.constant(10);
            let is_digit = stage.ult(digit, ten);
            let tens = stage.mul_imm(value, 10u64);
            let grown = stage.add(tens, digit);
            let over = stage.ugt_imm(grown, 1u64 << 31);
            let within = stage.not(over);
            let good = stage.all(is_digit, within);
            let next = stage.add_imm(index, 1u64);
            let more = stage.ult(next, length);
            let failed = stage.constant(u64::MAX);
            (stage.all(good, more), [next, stage.select(good, grown, failed)])
        });
        value
    }, #[inline(always)] |stage| stage.constant(u64::MAX));
    // A positive value fits below 2^31; a negative one at 2^31 too, but it is never zero.
    let limit = stage.constant(1u64 << 31);
    let below = stage.ult(value, limit);
    let positive = stage.not(negative);
    let positive_ok = stage.all(positive, below);
    let at_most = stage.ugt_imm(value, 1u64 << 31);
    let negative_fits = stage.not(at_most);
    let nonzero = stage.ne_imm(value, 0u64);
    let negative_nonzero = stage.all(negative, nonzero);
    let negative_ok = stage.all(negative_nonzero, negative_fits);
    let ok = stage.any(positive_ok, negative_ok);
    let zero = stage.constant(0);
    let negated = stage.sub(zero, value);
    (ok, stage.select(negative, negated, value))
}

/// `decimal_int32_row` on `Concrete` over the units at `units`, `length` of them, as an int32.
#[cfg(not(stencil_build))]
pub fn decimal_int32_at(wide: bool, units: u64, length: u64) -> Option<i32> {
    let mut stage = super::Concrete::pure();
    let (ok, value) = decimal_int32_row(&mut stage, wide, units, length);
    ok.then_some(value as i32)
}

/// The int32 `parseInt(text)` (radix 10) answers for the code units of `text`: the row on
/// `Concrete` over a copy of them, for text not already flat.
#[cfg(not(stencil_build))]
pub fn decimal_int32(units: impl Iterator<Item = u16>) -> Option<i32> {
    let units: Vec<u16> = units.collect();
    decimal_int32_at(true, units.as_ptr() as u64, units.len() as u64)
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
