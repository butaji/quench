//! `String.prototype.split`'s cuts of a string's units.
use super::*;

impl JsString {
    /// The runs `String.prototype.split` cuts this string into at `separator`, at most
    /// `limit` of them, handed to `part` in order: each unit alone for an empty separator.
    pub(crate) fn split_runs(&self, separator: Units<'_>, limit: usize, mut part: impl FnMut(Units<'_>)) {
        let units = self.units();
        match (units, separator) {
            _ if separator.is_empty() => {
                (0..units.len().min(limit)).for_each(|index| part(units.slice(index..index + 1)));
            }
            // One searcher serves every occurrence; a one-unit separator is a byte search, a
            // plain scan in text too short for `memchr`'s vectors to pay for its setup.
            (Units::Ascii(haystack), Units::Ascii(&[byte])) if haystack.len() <= BYTE_SCAN_MAX_BYTES => {
                let offsets = haystack.iter().enumerate().filter_map(|(offset, &unit)| (unit == byte).then_some(offset));
                cut_runs(units, offsets, 1, limit, part);
            }
            (Units::Ascii(haystack), Units::Ascii(&[byte])) => {
                cut_runs(units, memchr::memchr_iter(byte, haystack), 1, limit, part);
            }
            (Units::Ascii(haystack), Units::Ascii(needle)) => {
                cut_runs(units, memchr::memmem::find_iter(haystack, needle), needle.len(), limit, part);
            }
            _ => {
                let next = |offset: &usize| units.find(separator, offset + separator.len());
                cut_runs(units, std::iter::successors(units.find(separator, 0), next), separator.len(), limit, part);
            }
        }
    }
}

/// The longest text `JsString::split_runs` scans for a byte unit by unit: one AVX2 vector,
/// below which `memchr` searches without one but still pays its setup per occurrence.
const BYTE_SCAN_MAX_BYTES: usize = 32;

/// `JsString::split_runs` at `offsets`, the separator's non-overlapping occurrences in
/// order, each `separator_len` units long.
fn cut_runs(units: Units<'_>, offsets: impl Iterator<Item = usize>, separator_len: usize, limit: usize, mut part: impl FnMut(Units<'_>)) {
    let mut cursor = 0;
    for end in offsets.chain(std::iter::once(units.len())).take(limit) {
        part(units.slice(cursor..end));
        cursor = end + separator_len;
    }
}
