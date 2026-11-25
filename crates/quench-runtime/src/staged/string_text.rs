//! ASCII text builtins (trim, case, pad, repeat): one body per builtin, over flat unit slices.
//! The general natives (`vm::primitives`, `vm::wtf16::conversions::case`) and the in-place ASCII
//! paths (`vm::inplace::ascii_text`) both call these, so each rule is written once. Trim is the
//! staged row (`strings::trim_bounds`) run on `Concrete`; case, pad and repeat are still plain
//! loops over units. The caller makes the one allocation of the result.

use std::ops::Range;

/// The filler of `padStart` and `padEnd` when none is given.
pub(crate) const PAD_FILL: &[u8] = b" ";

/// The ECMAScript WhiteSpace and LineTerminator code units that `trim` strips: the staged test
/// (`strings::is_trim_space`) on `Concrete`.
pub(crate) fn is_ecma_whitespace(unit: u16) -> bool {
    let mut stage = super::Concrete::new(std::ptr::null());
    super::strings::is_trim_space(&mut stage, super::builtins::UnitWidth::Wide, u64::from(unit))
}

/// The units that `trim`, `trimStart` or `trimEnd` keeps of `units` (bytes of an ASCII or Latin-1
/// string, or wide units): the staged row (`strings::trim_bounds`) on `Concrete`, over the units in place.
pub(crate) fn trim_range<T: Copy + Into<u16>>(units: &[T], from_start: bool, from_end: bool) -> Range<usize> {
    let width = if size_of::<T>() == 2 { super::builtins::UnitWidth::Wide } else { super::builtins::UnitWidth::Narrow };
    let mut stage = super::Concrete::new(std::ptr::null());
    let (start, end) = super::strings::trim_bounds(&mut stage, width, units.as_ptr() as u64, units.len() as u64, (from_start, from_end));
    start as usize..end as usize
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
