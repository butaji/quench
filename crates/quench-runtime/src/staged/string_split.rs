//! `String.prototype.split` at a string separator: the cut offsets of a string's units, once.
//! `runs` is the one body every split derives from: the in-place split (`Vm::split_in_place`) and
//! the general one (`Vm::string_split_native`) hand their pieces out of it, and it is plain over
//! `Units` because a split's pieces are views of the receiver's units, not stage words.

use crate::vm::wtf16::Units;
use crate::vm::wtf16::scans;

/// The longest text a one-unit ASCII separator is scanned for unit by unit: one AVX2 vector, below
/// which `memchr` searches without one but still pays its setup per occurrence.
const BYTE_SCAN_MAX_BYTES: usize = 32;

/// Hands each piece of `units` cut at `separator` to `part`, at most `limit` of them, in order: the
/// last piece runs to the end of the text, and each unit alone is a piece for an empty separator.
/// The separator's occurrences are the cut offsets, found by the search that suits the widths and
/// lengths; the cut itself (`cut_pieces`) is the same for every search.
pub(crate) fn runs(units: Units<'_>, separator: Units<'_>, limit: usize, mut part: impl FnMut(Units<'_>)) {
    match (units, separator) {
        _ if separator.is_empty() => {
            (0..units.len().min(limit)).for_each(|index| part(units.slice(index..index + 1)));
        }
        (Units::Ascii(haystack), Units::Ascii(&[byte])) if haystack.len() <= BYTE_SCAN_MAX_BYTES => {
            let offsets = haystack.iter().enumerate().filter_map(|(offset, &unit)| (unit == byte).then_some(offset));
            cut_pieces(units, offsets, 1, limit, part);
        }
        (Units::Ascii(haystack), Units::Ascii(&[byte])) => {
            cut_pieces(units, memchr::memchr_iter(byte, haystack), 1, limit, part);
        }
        // A long text or needle builds one searcher for every occurrence; in short text the
        // scan of `Units::find` costs less than the searcher's construction.
        (Units::Ascii(haystack), Units::Ascii(needle)) if !scans(haystack.len(), needle.len()) => {
            cut_pieces(units, memchr::memmem::find_iter(haystack, needle), needle.len(), limit, part);
        }
        _ => {
            let next = |offset: &usize| units.find(separator, offset + separator.len());
            cut_pieces(units, std::iter::successors(units.find(separator, 0), next), separator.len(), limit, part);
        }
    }
}

/// The pieces of `units` between the separator's non-overlapping occurrences at `offsets` (in
/// order, each `separator_len` units long), at most `limit` of them: the core cut of `runs`.
fn cut_pieces(units: Units<'_>, offsets: impl Iterator<Item = usize>, separator_len: usize, limit: usize, mut part: impl FnMut(Units<'_>)) {
    let mut cursor = 0;
    for end in offsets.chain(std::iter::once(units.len())).take(limit) {
        part(units.slice(cursor..end));
        cursor = end + separator_len;
    }
}
