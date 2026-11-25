//! String search builtins (`indexOf`, `lastIndexOf`, `includes`, `startsWith`, `endsWith`) and cuts
//! (`slice`, `substring`, `substr`) over a string's units. One body each: the in-place calls
//! (`vm::primitives::in_place`) and the general calls (`Vm::string_search_general`, the
//! `primitives` substring arms) all derive from `search` and `cut`. The searches are one staged row
//! (`search_row`) over the span primitives (`staged::spans`), which the interpreter runs on
//! `Concrete` with the tuned searches; `cut` and the position conversions are plain arithmetic.

use std::ops::Range;

use crate::Value;
use crate::heap::Native;
use crate::vm::wtf16::Units;
use super::spans::{concrete_span, UnitSpan};

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

/// A string search method's answer, as its row computes it over the span primitives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SearchMethod {
    IndexOf,
    LastIndexOf,
    Includes,
    StartsWith,
    EndsWith,
}

/// The index a search method finds (`IndexOf`, `LastIndexOf`: none is `u64::MAX`) or the boolean it
/// answers (as 0 or 1), for `needle` in `text` at `position`, which is at most the text's length
/// (`clamped_position`). The row of the five search methods: `indexOf` and `includes` find forward,
/// `lastIndexOf` backward from `position`, `startsWith` compares the affix at `position` and
/// `endsWith` the one ending there, each where the needle fits.
#[inline(always)]
pub fn search_row<S: super::Stage>(stage: &mut S, method: SearchMethod, text: UnitSpan<S::Word>, needle: UnitSpan<S::Word>,
    position: S::Word) -> S::Word {
    let (one, none) = (stage.constant(1), stage.constant(0));
    match method {
        SearchMethod::IndexOf => stage.find_span(text, needle, position).1,
        SearchMethod::LastIndexOf => stage.rfind_span(text, needle, position).1,
        SearchMethod::Includes => {
            let found = stage.find_span(text, needle, position).0;
            stage.select(found, one, none)
        }
        SearchMethod::StartsWith => {
            let left = stage.sub(text.length, position);
            let short = stage.ult(left, needle.length);
            let word = stage.choose(short, #[inline(always)] |stage| stage.constant(0), #[inline(always)] |stage| {
                let same = stage.span_equal_at(text, needle, position);
                let (one, none) = (stage.constant(1), stage.constant(0));
                stage.select(same, one, none)
            });
            word
        }
        SearchMethod::EndsWith => {
            let short = stage.ult(position, needle.length);
            stage.choose(short, #[inline(always)] |stage| stage.constant(0), #[inline(always)] |stage| {
                let start = stage.sub(position, needle.length);
                let same = stage.span_equal_at(text, needle, start);
                let (one, none) = (stage.constant(1), stage.constant(0));
                stage.select(same, one, none)
            })
        }
    }
}

/// The result of a string search method at `position` (already clamped by `clamped_position`)
/// of `needle` in `text`: the index it finds, or the boolean it answers. The row (`search_row`) on
/// `Concrete`, whose span primitives are the tuned searches.
pub(crate) fn search(native: Native, text: Units<'_>, needle: Units<'_>, position: usize) -> Value {
    let method = match native {
        Native::StringIndexOf => SearchMethod::IndexOf,
        Native::StringLastIndexOf => SearchMethod::LastIndexOf,
        Native::StringIncludes => SearchMethod::Includes,
        Native::StringStartsWith => SearchMethod::StartsWith,
        _ => SearchMethod::EndsWith,
    };
    let mut stage = super::Concrete::new(std::ptr::null());
    let word = search_row(&mut stage, method, concrete_span(text), concrete_span(needle), position as u64);
    match method {
        SearchMethod::IndexOf | SearchMethod::LastIndexOf => index_value((word != u64::MAX).then_some(word as usize)),
        _ => Value::boolean(word != 0),
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
