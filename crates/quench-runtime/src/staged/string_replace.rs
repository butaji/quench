//! `String.prototype.replace` and `replaceAll` at a string pattern, and every replace's walk of its
//! matches. `match_starts` is the one search of a string pattern's matches. `walk_matches` is the one
//! gap-then-match step: each unmatched run of the input is handed to the caller, then each match in
//! turn, so the replacement texts are written in input order by every caller (`Vm::replace_units`,
//! `Vm::replace_with_callback`, the RegExp fast paths). `ascii_plan` is the flat ASCII case of the
//! same matches: a template with no `$` is written in one fill (`replaced_ascii`).

use std::convert::Infallible;
use std::mem::MaybeUninit;
use std::ops::Range;

use crate::vm::wtf16::{JsString, MAX_STRING_UNITS, Units};

/// The inline capacity of a replace's match offsets: a typical search's matches stay off the heap.
const REPLACE_INLINE_MATCHES: usize = 8;
/// The start offsets of the matches of a replace, in order.
type MatchOffsets = smallvec::SmallVec<[usize; REPLACE_INLINE_MATCHES]>;
/// The longest piece `put` writes byte by byte: two machine words, as a copy call costs more than
/// a few bytes.
const INLINE_PIECE_BYTES: usize = 2 * size_of::<usize>();

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

/// One step of a replace's walk of its input: an unmatched run of input units, or a match.
pub(crate) enum Piece<T> {
    /// The input units between the previous match and the next (possibly empty), or the tail
    /// after the last match.
    Gap(Range<usize>),
    /// A match, whose replacement the caller writes in its place.
    Match(T),
}

/// Walks an input of `input_len` units with its `matches` in order, handing `emit` the unmatched
/// gap before each match, then the match, then the gap after the last one. Each match's range
/// begins at or past the previous match's end. This is the one gap-then-match order of every
/// replace; the callers differ only in the match scan they feed it and in what a match writes.
pub(crate) fn walk_matches<T, E>(
    input_len: usize,
    matches: impl IntoIterator<Item = T>,
    range: impl Fn(&T) -> Range<usize>,
    mut emit: impl FnMut(Piece<T>) -> Result<(), E>,
) -> Result<(), E> {
    let mut next = 0;
    for found in matches {
        let span = range(&found);
        emit(Piece::Gap(next..span.start))?;
        next = span.end;
        emit(Piece::Match(found))?;
    }
    emit(Piece::Gap(next..input_len))
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
    let ranges = offsets.iter().map(|&index| index..index + needle.len());
    Some(ReplacePlan::Derived(replaced_ascii(text, with, ranges, length)))
}

/// The length of a replace's result: `text_len` less `count` matches of `needle_len` bytes, plus
/// `count` replacements of `with_len`; `None` past the longest string.
fn replaced_len(text_len: usize, needle_len: usize, with_len: usize, count: usize) -> Option<usize> {
    let kept = text_len.checked_sub(count.checked_mul(needle_len)?)?;
    let length = kept.checked_add(count.checked_mul(with_len)?)?;
    (length <= MAX_STRING_UNITS).then_some(length)
}

/// `text` with each of `ranges` (in order, as `walk_matches` takes them) replaced by `with`,
/// written once into storage of `length` bytes. `length` is the total the caller counted for the
/// replace, and the write checks that every byte of the uninitialized storage is filled.
pub(crate) fn replaced_ascii(
    text: &[u8],
    with: &[u8],
    ranges: impl IntoIterator<Item = Range<usize>>,
    length: usize,
) -> JsString {
    JsString::from_ascii_filled(length, |out| {
        let mut at = 0;
        let written = walk_matches(text.len(), ranges, Range::clone, |piece| -> Result<(), Infallible> {
            at = match piece {
                Piece::Gap(gap) => put(out, at, &text[gap]),
                Piece::Match(_) => put(out, at, with),
            };
            Ok(())
        });
        written.unwrap_or_else(|never| match never {});
        assert_eq!(at, out.len(), "the matches fill the length the replace counted");
    })
}

/// Copies `bytes` into `out` at `at`, returning the offset past them; a short piece is written
/// byte by byte, as a call to copy a few bytes costs more than the bytes.
#[inline(always)]
fn put(out: &mut [MaybeUninit<u8>], at: usize, bytes: &[u8]) -> usize {
    let target = &mut out[at..at + bytes.len()];
    if bytes.len() <= INLINE_PIECE_BYTES {
        for (unit, byte) in target.iter_mut().zip(bytes) {
            unit.write(*byte);
        }
    } else {
        target.write_copy_of_slice(bytes);
    }
    at + bytes.len()
}
