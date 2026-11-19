//! `String.prototype.replace` and `replaceAll` at a string pattern: the match offsets and the replaced
//! string, once. `match_starts` is the one search of matches, which `Vm::replace_units` walks to
//! append each replacement (its GetSubstitution where the template holds a `$`). `ascii_plan` is the
//! flat ASCII case of the same matches: a template with no `$` is written in one fill (`write_replaced`).

use std::mem::MaybeUninit;

use crate::vm::wtf16::{JsString, MAX_STRING_UNITS, Units};

/// The inline capacity of a replace's match offsets: a typical search's matches stay off the heap.
const REPLACE_INLINE_MATCHES: usize = 8;
/// The start offsets of the matches of a replace, in order.
type MatchOffsets = smallvec::SmallVec<[usize; REPLACE_INLINE_MATCHES]>;

/// The start offsets of the matches of `search` in `input`, in order: only the first unless
/// `replace_all`. Matches of a non-empty search do not overlap, so each search starts past the
/// previous match; an empty search matches at each unit boundary, so a replace-all search resumes
/// one unit on.
pub(crate) fn match_starts<'a>(input: Units<'a>, search: Units<'a>, replace_all: bool) -> impl Iterator<Item = usize> + 'a {
    let advance = search.len().max(1);
    std::iter::successors(input.find(search, 0), move |&index| {
        let next = index + advance;
        if replace_all && next <= input.len() { input.find(search, next) } else { None }
    })
}

/// What a flat ASCII replace comes to: the receiver as it is, or the string it makes.
pub(crate) enum ReplacePlan {
    /// No match: the receiver itself is the result.
    Unchanged,
    /// The replaced string, written in full once.
    Derived(JsString),
}

/// The plan of a replace over the flat ASCII `text`, `needle` and `with`, all read in place. `None`
/// when the replace needs the general path: a `$` in the replacement (its GetSubstitution), or a
/// length past the longest string.
pub(crate) fn ascii_plan(text: &[u8], needle: &[u8], with: &[u8], replace_all: bool) -> Option<ReplacePlan> {
    if with.contains(&b'$') {
        return None;
    }
    let offsets: MatchOffsets = match_starts(Units::Ascii(text), Units::Ascii(needle), replace_all).collect();
    if offsets.is_empty() {
        return Some(ReplacePlan::Unchanged);
    }
    let length = replaced_len(text.len(), needle.len(), with.len(), offsets.len())?;
    let string = JsString::from_ascii_filled(length, |out| write_replaced(out, text, needle.len(), with, &offsets));
    Some(ReplacePlan::Derived(string))
}

/// The length of a replace's result: `text_len` less `count` matches of `needle_len` bytes, plus
/// `count` replacements of `with_len`; `None` past the longest string.
fn replaced_len(text_len: usize, needle_len: usize, with_len: usize, count: usize) -> Option<usize> {
    let kept = text_len.checked_sub(count.checked_mul(needle_len)?)?;
    let length = kept.checked_add(count.checked_mul(with_len)?)?;
    (length <= MAX_STRING_UNITS).then_some(length)
}

/// Writes `text` into `out` with each match at `offsets` (of `needle_len` bytes) replaced by
/// `with`. `out` is exactly the length `replaced_len` gives, and every byte of it is written
/// (checked below), so no byte of the uninitialized storage is left unset.
fn write_replaced(out: &mut [MaybeUninit<u8>], text: &[u8], needle_len: usize, with: &[u8], offsets: &[usize]) {
    let mut at = 0;
    let mut cursor = 0;
    for &index in offsets {
        at = put(out, at, &text[cursor..index]);
        at = put(out, at, with);
        cursor = index + needle_len;
    }
    at = put(out, at, &text[cursor..]);
    assert_eq!(at, out.len(), "the matches fill the length `replaced_len` counted");
}

/// Copies `bytes` into `out` at `at`, returning the offset past them; a single byte is one store.
#[inline(always)]
fn put(out: &mut [MaybeUninit<u8>], at: usize, bytes: &[u8]) -> usize {
    if let [byte] = bytes {
        out[at].write(*byte);
        return at + 1;
    }
    let target = &mut out[at..at + bytes.len()];
    // SAFETY: `target` has exactly `bytes.len()` slots, which `bytes` cannot overlap (it is a
    // borrow of the heap or of the pattern text, never of `out`); `u8` and `MaybeUninit<u8>`
    // share a layout, so the copy initializes the slots.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.as_mut_ptr().cast::<u8>(), bytes.len()) };
    at + bytes.len()
}
