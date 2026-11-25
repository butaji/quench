//! ASCII text builtins (trim, case, pad, repeat): one body per builtin, over flat unit slices.
//! The general natives (`vm::primitives`, `vm::wtf16::conversions::case`) and the in-place ASCII
//! paths (`vm::inplace::ascii_text`) both call these, so each rule is written once. They are plain
//! functions rather than `Stage`-generic: each is a loop over text units that the `Stage` trait
//! does not express yet, and the caller makes the one allocation of the result.

use std::ops::Range;

/// The filler of `padStart` and `padEnd` when none is given.
pub(crate) const PAD_FILL: &[u8] = b" ";

/// The ECMAScript WhiteSpace and LineTerminator code points (StrWhiteSpaceChar) that `trim`,
/// number parsing and the host's parsers strip: one definition for every crate.
pub fn is_ecma_whitespace_char(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_ecma_whitespace)
}

/// The ECMAScript WhiteSpace and LineTerminator code units that `trim` strips.
pub(crate) fn is_ecma_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000D
            | 0x0020
            | 0x00A0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028..=0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

/// The units that `trim`, `trimStart` or `trimEnd` keeps of `units`: the range left once the
/// whitespace at the chosen ends (`from_start`, `from_end`) is stripped. Derives from the
/// general `trim` of `primitives::call_primitive_native`; a Latin-1 or ASCII unit widens to its code.
pub(crate) fn trim_range<T: Copy + Into<u16>>(units: &[T], from_start: bool, from_end: bool) -> Range<usize> {
    let is_space = |unit: &T| is_ecma_whitespace((*unit).into());
    let start = if from_start {
        units.iter().position(|unit| !is_space(unit)).unwrap_or(units.len())
    } else {
        0
    };
    let end = if from_end {
        units.iter().rposition(|unit| !is_space(unit)).map_or(start, |last| last + 1)
    } else {
        units.len()
    };
    start..end
}

/// The ASCII case mapping of `byte`, upper or lower; any other byte maps to itself.
pub(crate) fn ascii_case_byte(byte: u8, upper: bool) -> u8 {
    if upper { byte.to_ascii_uppercase() } else { byte.to_ascii_lowercase() }
}

/// Whether the ASCII case mapping changes any of `bytes`.
pub(crate) fn ascii_case_changes(bytes: &[u8], upper: bool) -> bool {
    bytes.iter().any(|byte| ascii_case_byte(*byte, upper) != *byte)
}

/// The ASCII case mapping of `bytes` written into `out`, which is as long as `bytes`.
pub(crate) fn ascii_case(bytes: &[u8], upper: bool, out: &mut [u8]) {
    for (slot, byte) in out.iter_mut().zip(bytes) {
        *slot = ascii_case_byte(*byte, upper);
    }
}

/// The length of `padStart` or `padEnd` of a `len`-unit receiver to `target` units, or `None`
/// where the receiver is already that long and is its own result.
pub(crate) fn pad_length(len: usize, target: usize) -> Option<usize> {
    (target > len).then_some(target)
}

/// `text` written into `out`, the rest of `out` filled with repeats of `fill` (non-empty): at its
/// end when `at_end` (`padEnd`), else at its start (`padStart`).
pub(crate) fn pad_into<T: Copy>(out: &mut [T], text: &[T], fill: &[T], at_end: bool) {
    let pad_len = out.len() - text.len();
    let (text_out, pad_out) = if at_end {
        out.split_at_mut(text.len())
    } else {
        let (pad_out, text_out) = out.split_at_mut(pad_len);
        (text_out, pad_out)
    };
    text_out.copy_from_slice(text);
    for (slot, unit) in pad_out.iter_mut().zip(fill.iter().copied().cycle()) {
        *slot = unit;
    }
}

/// `unit` repeated through `out`, whose length is a multiple of `unit.len()` (non-empty): the one
/// byte writer of a `repeat` over flat ASCII units.
pub(crate) fn repeat_into(out: &mut [u8], unit: &[u8]) {
    for chunk in out.chunks_exact_mut(unit.len()) {
        chunk.copy_from_slice(unit);
    }
}
