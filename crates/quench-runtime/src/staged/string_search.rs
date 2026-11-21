//! String search builtins (`indexOf`, `lastIndexOf`, `includes`, `startsWith`, `endsWith`) and cuts
//! (`slice`, `substring`, `substr`) over a string's units. One body each: the in-place calls
//! (`vm::primitives::in_place`) and the general calls (`Vm::string_search_general`, the
//! `primitives` substring arms) all derive from `search` and `cut`. The units are a plain view,
//! not a copy, so these are plain functions over `Units` rather than `Stage` rows.

use std::ops::Range;

use crate::Value;
use crate::heap::Native;
use crate::vm::wtf16::Units;

/// The end argument `substr` takes when it is left out: every unit from the start. `i64::MAX`
/// reaches the end from any start, so `cut` needs no length for it.
pub(crate) const SUBSTR_REST: i64 = i64::MAX;

/// The position a search method takes from a left-out or undefined argument, as a number: the
/// end of the text for `endsWith` and `lastIndexOf`, the start for the others.
pub(crate) fn search_default(native: Native, length: usize) -> f64 {
    if matches!(native, Native::StringEndsWith | Native::StringLastIndexOf) {
        length as f64
    } else {
        0.0
    }
}

/// The position a search method searches from, given its position argument as a number and the
/// text's `length`: ToIntegerOrInfinity clamped to `0..=length`. NaN is the start, except for
/// `lastIndexOf`, where it is the end, as +Infinity is.
pub(crate) fn clamped_position(native: Native, number: f64, length: usize) -> usize {
    if number.is_nan() {
        return if native == Native::StringLastIndexOf { length } else { 0 };
    }
    number.trunc().max(0.0).min(length as f64) as usize
}

/// The result of a string search method at `position` (already clamped by `clamped_position`)
/// of `needle` in `text`: the index it finds, or the boolean it answers. `endsWith` compares the
/// affix that ends at `position`.
pub(crate) fn search(native: Native, text: Units<'_>, needle: Units<'_>, position: usize) -> Value {
    match native {
        Native::StringIndexOf => index_value(text.find(needle, position)),
        Native::StringLastIndexOf => index_value(last_index(text, needle, position)),
        Native::StringIncludes => Value::boolean(text.find(needle, position).is_some()),
        Native::StringStartsWith => Value::boolean(text.slice(position..text.len()).starts_with(needle)),
        _ => Value::boolean(
            position
                .checked_sub(needle.len())
                .is_some_and(|start| text.slice(start..position) == needle),
        ),
    }
}

/// The last offset at or before `position` where `needle` occurs in `text`. ASCII text takes the
/// byte search (`ascii_rfind`); other units take `Units::rfind`, which answers the same.
fn last_index(text: Units<'_>, needle: Units<'_>, position: usize) -> Option<usize> {
    match (text, needle) {
        (Units::Ascii(text), Units::Ascii(needle)) => ascii_rfind(text, needle, position),
        _ => text.rfind(needle, position),
    }
}

/// The last offset at or before `position` where the ASCII `needle` occurs in `text`, as
/// `lastIndexOf` searches: `position` is first clamped to the last offset a match can start at.
/// A one-byte needle is found by `memrchr`, a longer one by `memmem`'s reverse search.
fn ascii_rfind(text: &[u8], needle: &[u8], position: usize) -> Option<usize> {
    let last = text.len().checked_sub(needle.len())?;
    let from = position.min(last);
    match needle {
        [] => Some(from),
        [byte] => memchr::memrchr(*byte, &text[..=from]),
        _ => memchr::memmem::rfind(&text[..from + needle.len()], needle),
    }
}

/// A search index as a number value: the index found, or -1 for none.
fn index_value(found: Option<usize>) -> Value {
    Value::number(found.map_or(-1.0, |index| index as f64))
}

/// The units range `slice`, `substring` or `substr` cuts from a string of `length` units, given
/// its integer arguments. `slice` takes `start` and `end` (negative bounds count from the end);
/// `substring` takes them in either order, clamped; `substr` takes `start` (negative counts from
/// the end) and, in the `end` argument, a count (`SUBSTR_REST` for every unit from the start).
pub(crate) fn cut(native: Native, length: i64, start: i64, end: i64) -> Range<usize> {
    match native {
        Native::StringSubstr => {
            let start = if start < 0 { (length + start).max(0) } else { start.min(length) };
            let end = start.saturating_add(end.max(0)).min(length);
            start as usize..end as usize
        }
        _ => {
            let (start, end) = if native == Native::StringSlice {
                let from_end = |bound: i64| if bound < 0 { (length + bound).max(0) } else { bound.min(length) };
                (from_end(start), from_end(end))
            } else {
                let (start, end) = (start.clamp(0, length), end.clamp(0, length));
                (start.min(end), start.max(end))
            };
            start as usize..end.max(start) as usize
        }
    }
}
