//! ASCII text builtins (trim, case, pad, repeat): one body per builtin, over flat unit slices.
//! The general natives (`vm::primitives`, `vm::wtf16::conversions::case`) and the in-place ASCII
//! paths (`vm::inplace::ascii_text`) both call these, so each rule is written once. Each is a
//! staged row (`strings::trim_bounds`, `pad_into_row`, `repeat_into_row`) or a span primitive
//! (`Stage::case_span`) run on `Concrete`. The caller makes the one allocation of the result.

use std::ops::Range;

/// The filler of `padStart` and `padEnd` when none is given.
pub(crate) const PAD_FILL: &[u8] = b" ";

/// The ECMAScript WhiteSpace and LineTerminator code points (StrWhiteSpaceChar) that `trim`,
/// number parsing and the host's parsers strip: one definition for every crate.
pub fn is_ecma_whitespace_char(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_ecma_whitespace)
}

/// The ECMAScript WhiteSpace and LineTerminator code units that `trim` strips: the staged test
/// (`strings::is_trim_space`) on `Concrete`.
pub(crate) fn is_ecma_whitespace(unit: u16) -> bool {
    let mut stage = super::Concrete::pure();
    super::strings::is_trim_space(&mut stage, super::builtins::UnitWidth::Wide, u64::from(unit))
}

/// The units that `trim`, `trimStart` or `trimEnd` keeps of `units` (bytes of an ASCII or Latin-1
/// string, or wide units): the staged row (`strings::trim_bounds`) on `Concrete`, over the units in place.
pub(crate) fn trim_range<T: Copy + Into<u16>>(units: &[T], from_start: bool, from_end: bool) -> Range<usize> {
    let width = if size_of::<T>() == 2 { super::builtins::UnitWidth::Wide } else { super::builtins::UnitWidth::Narrow };
    let mut stage = super::Concrete::pure();
    let (start, end) = super::strings::trim_bounds(&mut stage, width, units.as_ptr() as u64, units.len() as u64, (from_start, from_end));
    start as usize..end as usize
}

/// The ASCII case mapping of `byte`, upper or lower; any other byte maps to itself.
pub(crate) fn ascii_case_byte(byte: u8, upper: bool) -> u8 {
    if upper { byte.to_ascii_uppercase() } else { byte.to_ascii_lowercase() }
}

/// Whether the ASCII case mapping changes any of `bytes`.
pub(crate) fn ascii_case_changes(bytes: &[u8], upper: bool) -> bool {
    use super::Stage;
    let mut stage = super::Concrete::pure();
    stage.case_changes_span(bytes.as_ptr() as u64, bytes.len() as u64, upper)
}

/// The ASCII case mapping of `bytes` written into `out`, which is as long as `bytes`: the span
/// primitive (`Stage::case_span`) on `Concrete`.
pub(crate) fn ascii_case(bytes: &[u8], upper: bool, out: &mut [u8]) {
    assert_eq!(out.len(), bytes.len());
    ascii_case_raw(bytes, upper, out.as_mut_ptr());
}

/// `ascii_case` into the `bytes.len()` bytes at `out`, which may be uninitialized; the caller
/// proves they are writable and apart from `bytes`.
pub(crate) fn ascii_case_raw(bytes: &[u8], upper: bool, out: *mut u8) {
    use super::Stage;
    let mut stage = super::Concrete::pure();
    stage.case_span(out as u64, bytes.as_ptr() as u64, bytes.len() as u64, upper);
}

/// The length of `padStart` or `padEnd` of a `len`-unit receiver to `target` units, or `None`
/// where the receiver is already that long and is its own result.
pub(crate) fn pad_length(len: usize, target: usize) -> Option<usize> {
    (target > len).then_some(target)
}

/// `text` written into `out`, the rest of `out` filled with repeats of `fill` (non-empty): at its
/// end when `at_end` (`padEnd`), else at its start (`padStart`).
pub(crate) fn pad_into<T: Copy>(out: &mut [T], text: &[T], fill: &[T], at_end: bool) {
    assert!(out.len() > text.len() && !fill.is_empty() && size_of::<T>() <= 2);
    let mut stage = super::Concrete::pure();
    super::strings::pad_into_row(&mut stage, size_of::<T>() == 2, out.as_mut_ptr() as u64, out.len() as u64,
        text.as_ptr() as u64, text.len() as u64, fill.as_ptr() as u64, fill.len() as u64, at_end);
}

/// `unit` repeated through `out`, whose length is a multiple of `unit.len()` (non-empty): the one
/// byte writer of a `repeat` over flat ASCII units.
pub(crate) fn repeat_into(out: &mut [u8], unit: &[u8]) {
    assert!(!unit.is_empty() && out.len() % unit.len() == 0);
    let mut stage = super::Concrete::pure();
    super::strings::repeat_into_row(&mut stage, out.as_mut_ptr() as u64, out.len() as u64, unit.as_ptr() as u64, unit.len() as u64);
}
