//! Searching text for a run of units.
use super::Units;

/// The longest haystack, and needle, searched by scanning for the needle's first byte and
/// comparing the rest at each hit: shorter than this, building `memchr::memmem`'s searcher
/// costs more than the scan it saves, and the scan's worst case stays their product.
const SCANNED_HAYSTACK_MAX_BYTES: usize = 1 << 10;
const SCANNED_NEEDLE_MAX_BYTES: usize = 32;
/// The longest needle `find_filtered` searches for: its block filter compares one first and one
/// last unit per start offset.
const FILTER_NEEDLE_MAX_BYTES: usize = 16;
/// Start offsets the block filter tests at once: one SSE2 register of bytes.
const FILTER_LANES: usize = 16;

/// Whether a search of a haystack of `haystack` bytes for a needle of `needle` bytes scans for
/// the needle's first byte rather than building a `memchr::memmem` searcher.
pub(crate) fn scans(haystack: usize, needle: usize) -> bool {
    haystack <= SCANNED_HAYSTACK_MAX_BYTES && needle <= SCANNED_NEEDLE_MAX_BYTES
}

impl<'a> Units<'a> {
    /// The first index at or after `start` where `search` occurs.
    pub(crate) fn find(self, search: Units<'_>, start: usize) -> Option<usize> {
        if search.is_empty() {
            return Some(start.min(self.len()));
        }
        let last = self.len().checked_sub(search.len())?;
        if start > last || self.ascii_cannot_contain(search) {
            return None;
        }
        let found = match (self, search) {
            (Self::Ascii(haystack), Units::Ascii(needle)) => find_ascii(&haystack[start..], needle),
            (Self::Ascii(haystack), Units::Wide(needle)) | (Self::Latin1(haystack), Units::Wide(needle)) => {
                find_units(&haystack[start..], needle)
            }
            (Self::Ascii(haystack), Units::Latin1(needle)) | (Self::Latin1(haystack), Units::Ascii(needle) | Units::Latin1(needle)) => {
                find_units(&haystack[start..], needle)
            }
            (Self::Wide(haystack), Units::Ascii(needle) | Units::Latin1(needle)) => find_units(&haystack[start..], needle),
            (Self::Wide(haystack), Units::Wide(needle)) => find_units(&haystack[start..], needle),
        };
        found.map(|offset| start + offset)
    }

    /// The last index at or before `from`, clamped to the length of the text, where `search`
    /// occurs: the search `lastIndexOf` makes.
    pub(crate) fn rfind(self, search: Units<'_>, from: usize) -> Option<usize> {
        let last = self.len().checked_sub(search.len())?;
        let from = from.min(last);
        if search.is_empty() {
            return Some(from);
        }
        if self.ascii_cannot_contain(search) {
            return None;
        }
        // ASCII in ASCII: a one-byte needle by `memrchr`, a longer one by `memmem`'s reverse search.
        if let (Self::Ascii(text), Units::Ascii(needle)) = (self, search) {
            return match needle {
                [byte] => memchr::memrchr(*byte, &text[..=from]),
                _ => memchr::memmem::rfind(&text[..from + needle.len()], needle),
            };
        }
        let first = search.at(0);
        (0..=from).rev().find(|&at| self.at(at) == first && self.slice(at..at + search.len()) == search)
    }

    /// Whether ASCII text cannot hold `search`, because it has a unit that ASCII text never has.
    fn ascii_cannot_contain(self, search: Units<'_>) -> bool {
        matches!((self, search), (Self::Ascii(_), Units::Wide(units)) if units.iter().any(|unit| !super::is_ascii_unit(*unit)))
    }
}

/// The first offset of `needle`, which is not empty, in `haystack`. A needle of one byte is
/// found by `memchr`, a short one by the block filter, a longer one by a scan for its first
/// byte, or `memmem` past the scan's bounds.
pub(crate) fn find_ascii(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    match needle {
        [] => Some(0),
        [byte] => memchr::memchr(*byte, haystack),
        _ if needle.len() > haystack.len() => None,
        _ if needle.len() <= FILTER_NEEDLE_MAX_BYTES => find_filtered(haystack, needle),
        _ if scans(haystack.len(), needle.len()) => find_scanned(haystack, needle),
        _ => memchr::memmem::find(haystack, needle),
    }
}

/// The first offset of `needle`, which is not empty, in `haystack`, comparing units of either
/// width as `u16`s: each unit is widened as it is read, so neither side is converted or copied.
fn find_units<H: Copy + Into<u16>, N: Copy + Into<u16>>(haystack: &[H], needle: &[N]) -> Option<usize> {
    let (first, rest) = needle.split_first()?;
    let starts = haystack.len().checked_sub(needle.len())? + 1;
    (0..starts).find(|&at| {
        same_unit(haystack[at], *first)
            && haystack[at + 1..at + needle.len()].iter().zip(rest).all(|(unit, expected)| same_unit(*unit, *expected))
    })
}

/// Whether two units, of either width, are the same code unit.
fn same_unit<H: Into<u16>, N: Into<u16>>(unit: H, expected: N) -> bool {
    let (unit, expected): (u16, u16) = (unit.into(), expected.into());
    unit == expected
}

/// `find_ascii` of a needle longer than one byte and at most `SCANNED_NEEDLE_MAX_BYTES`: the
/// needle's first byte is scanned for, and the rest compared at each hit.
fn find_scanned(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let (first, rest) = needle.split_first()?;
    let starts = haystack.len() - needle.len() + 1;
    let mut at = 0;
    while let Some(offset) = memchr::memchr(*first, &haystack[at..starts]) {
        let candidate = at + offset;
        if haystack[candidate + 1..candidate + needle.len()] == *rest {
            return Some(candidate);
        }
        at = candidate + 1;
    }
    None
}

/// `find_ascii` of a needle of two to `FILTER_NEEDLE_MAX_BYTES` bytes: the start offsets are
/// taken `FILTER_LANES` at a time, and the offsets in a block whose first and last units both
/// match are verified against the whole needle, in order. The rest of the offsets, fewer than
/// a block, are compared one by one.
fn find_filtered(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let starts = haystack.len() - needle.len() + 1;
    let mut start = 0;
    while start + FILTER_LANES <= starts {
        let mut hits = filter_block(haystack, needle, start);
        while hits != 0 {
            let at = start + hits.trailing_zeros() as usize;
            if haystack[at..at + needle.len()] == *needle {
                return Some(at);
            }
            hits &= hits - 1;
        }
        start += FILTER_LANES;
    }
    (start..starts).find(|&at| haystack[at..at + needle.len()] == *needle)
}

/// The lanes, one bit each, of the `FILTER_LANES` start offsets from `start` whose unit is
/// `needle`'s first and whose unit `needle`'s length less one past it is its last. Only called
/// while `start + FILTER_LANES <= haystack.len() - needle.len() + 1`.
#[cfg(target_arch = "x86_64")]
fn filter_block(haystack: &[u8], needle: &[u8], start: usize) -> u32 {
    use std::arch::x86_64::{
        __m128i, _mm_and_si128, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8,
    };
    let tail = start + needle.len() - 1;
    debug_assert!(tail + FILTER_LANES <= haystack.len());
    // SAFETY: the caller's bound gives `start + FILTER_LANES <= haystack.len() - needle.len() + 1`,
    // so the two 16-byte unaligned loads, at `start` and at `tail`, end by `tail + FILTER_LANES`,
    // which is at most `haystack.len()`: both lie inside the slice.
    unsafe {
        let heads = _mm_loadu_si128(haystack.as_ptr().add(start).cast::<__m128i>());
        let tails = _mm_loadu_si128(haystack.as_ptr().add(tail).cast::<__m128i>());
        let first = _mm_cmpeq_epi8(heads, _mm_set1_epi8(needle[0] as i8));
        let last = _mm_cmpeq_epi8(tails, _mm_set1_epi8(needle[needle.len() - 1] as i8));
        _mm_movemask_epi8(_mm_and_si128(first, last)) as u32
    }
}

/// `filter_block` without SSE2: the same lanes, tested one at a time.
#[cfg(not(target_arch = "x86_64"))]
fn filter_block(haystack: &[u8], needle: &[u8], start: usize) -> u32 {
    let tail = start + needle.len() - 1;
    (0..FILTER_LANES)
        .filter(|&lane| haystack[start + lane] == needle[0] && haystack[tail + lane] == needle[needle.len() - 1])
        .fold(0u32, |bits, lane| bits | (1 << lane))
}
