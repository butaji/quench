//! Searching text for a run of units.
use super::Units;

/// The longest haystack, and needle, searched by scanning for the needle's first byte and
/// comparing the rest at each hit: shorter than this, building `memchr::memmem`'s searcher
/// costs more than the scan it saves, and the scan's worst case stays their product.
const SCANNED_HAYSTACK_MAX_BYTES: usize = 1 << 10;
const SCANNED_NEEDLE_MAX_BYTES: usize = 32;

impl<'a> Units<'a> {
    /// The first index at or after `start` where `search` occurs.
    pub(crate) fn find(self, search: Units<'_>, start: usize) -> Option<usize> {
        if search.is_empty() {
            return Some(start.min(self.len()));
        }
        if search.len() > self.len() {
            return None;
        }
        let last = self.len() - search.len();
        if let (Self::Ascii(haystack), Units::Ascii(needle)) = (self, search) {
            if start > last {
                return None;
            }
            if let [unit] = needle {
                return memchr::memchr(*unit, &haystack[start..]).map(|offset| start + offset);
            }
            return find_ascii(&haystack[start..], needle).map(|offset| start + offset);
        }
        let first = search.at(0);
        (start..=last).find(|index| {
            self.at(*index) == first && self.slice(*index..*index + search.len()) == search
        })
    }
}

/// The first offset of `needle`, which is not empty, in `haystack`.
fn find_ascii(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if haystack.len() > SCANNED_HAYSTACK_MAX_BYTES || needle.len() > SCANNED_NEEDLE_MAX_BYTES {
        return memchr::memmem::find(haystack, needle);
    }
    let (first, rest) = needle.split_first()?;
    let starts = haystack.len().checked_sub(needle.len())? + 1;
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
