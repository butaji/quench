//! The one hash of a string's text: every table keyed by string contents (atoms, Map and
//! Set keys, interned strings) hashes with it, and a heap string record computes it once
//! (`Heap::string_hash`). It reads the UTF-16 units, so every spelling of the same units
//! (narrow or wide, a slice or a copy) hashes alike.

use std::hash::Hasher;

use super::{is_ascii_unit, FlatUnits, JsString, Repr, ShortAscii, Units, SHORT_ASCII_CAPACITY};

/// A text hash: 32 bits, never zero, so a record's unset cache entry needs no extra bit.
pub(crate) type TextHash = u32;

/// Bytes of ASCII text the hasher takes as one word.
const ASCII_BYTES_PER_WORD: usize = size_of::<u64>();
/// Wide units the hasher takes as one word, one per 16-bit lane.
const WIDE_UNITS_PER_WORD: usize = size_of::<u64>() / size_of::<u16>();

impl Units<'_> {
    pub(crate) fn text_hash(self) -> TextHash {
        match self {
            Units::Ascii(bytes) => ascii_hash(bytes),
            Units::Wide(units) if units.iter().all(|unit| is_ascii_unit(*unit)) => {
                fold(hash_ascii_words(units.len(), units.chunks(ASCII_BYTES_PER_WORD).map(|chunk| {
                    chunk.iter().rev().fold(0, |word, unit| (word << u8::BITS) | u64::from(*unit))
                })))
            }
            Units::Wide(units) => fold(hash_wide(units)),
        }
    }
}

impl JsString {
    /// The text hash of inline text, which costs less to make than to look up once made.
    #[inline]
    pub(crate) fn inline_text_hash(&self) -> Option<TextHash> {
        match &self.repr {
            Repr::Flat(FlatUnits::Short(text)) => Some(short_hash(text)),
            _ => None,
        }
    }

    /// Whether this string's hash is made on each use (`inline_text_hash`), never kept.
    pub(crate) fn hashes_inline(&self) -> bool {
        matches!(&self.repr, Repr::Flat(FlatUnits::Short(_)))
    }
}

/// `Units::text_hash` of the UTF-16 form of `text`.
pub(crate) fn str_text_hash(text: &str) -> TextHash {
    if text.is_ascii() {
        return ascii_hash(text.as_bytes());
    }
    // Non-ASCII text has a non-ASCII unit, so it takes the wide hash.
    fold(hash_wide(&text.encode_utf16().collect::<Vec<_>>()))
}

/// The 64-bit table hash of a text hash: hashbrown picks buckets from the low bits and tags
/// from the high bits, so the 32 bits fill both halves.
pub(crate) fn table_hash(hash: TextHash) -> u64 {
    u64::from(hash) | (u64::from(hash) << u32::BITS)
}

/// 2^64 over the golden ratio: Fibonacci hashing's multiplier (Knuth, TAOCP 6.4).
const FIBONACCI_MULTIPLIER: u64 = 0x9e37_79b9_7f4a_7c15;

/// Folds a hasher's word to a nonzero `TextHash`. The hasher's multiplies carry a word's
/// bits only upward, so its high half is folded down and the product's high half kept,
/// which every input bit reaches; tables pick buckets and tags from all 32 bits alike.
fn fold(hash: u64) -> TextHash {
    let mixed = (hash ^ (hash >> u32::BITS)).wrapping_mul(FIBONACCI_MULTIPLIER);
    ((mixed >> u32::BITS) as u32).max(1)
}

/// The hash of ASCII text: its length, then each `ASCII_BYTES_PER_WORD` bytes as one
/// little-endian word, the last one zero-padded. Wide units that are all ASCII hash as
/// their bytes would, so equal texts agree whatever their storage.
fn ascii_hash(bytes: &[u8]) -> TextHash {
    let (words, rest) = bytes.as_chunks::<ASCII_BYTES_PER_WORD>();
    let last = (!rest.is_empty()).then(|| rest.iter().rev().fold(0, |word, byte| (word << u8::BITS) | u64::from(*byte)));
    fold(hash_ascii_words(bytes.len(), words.iter().map(|word| u64::from_le_bytes(*word)).chain(last)))
}

/// `ascii_hash` of inline text, its words read in place: copying them out first would
/// make each word's load wait on the copy's narrower stores.
fn short_hash(text: &ShortAscii) -> TextHash {
    let length = usize::from(text.length);
    let words = (0..length.div_ceil(ASCII_BYTES_PER_WORD)).map(|index| short_word(&text.bytes, index));
    fold(hash_ascii_words(length, words))
}

/// The `index`th hash word of inline text: unused bytes are zero, as the last word's padding
/// is, and a word reaching past the bytes is read ending at their end and shifted down.
fn short_word(bytes: &[u8; SHORT_ASCII_CAPACITY], index: usize) -> u64 {
    let wanted = index * ASCII_BYTES_PER_WORD;
    let start = wanted.min(SHORT_ASCII_CAPACITY - ASCII_BYTES_PER_WORD);
    let word = bytes[start..].first_chunk::<ASCII_BYTES_PER_WORD>().expect("a whole word fits");
    u64::from_le_bytes(*word) >> ((wanted - start) as u32 * u8::BITS)
}

fn hash_ascii_words(len: usize, words: impl Iterator<Item = u64>) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_usize(len);
    words.for_each(|word| hasher.write_u64(word));
    hasher.finish()
}

/// The hash of UTF-16 units with at least one non-ASCII: their count, then each whole group
/// of `WIDE_UNITS_PER_WORD` units as one word (the first unit in the lowest lane), then each
/// remaining unit. Such text never equals ASCII text, so it hashes apart.
fn hash_wide(units: &[u16]) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_usize(units.len());
    let (words, rest) = units.as_chunks::<WIDE_UNITS_PER_WORD>();
    for word in words {
        hasher.write_u64(word.iter().rev().fold(0, |word, unit| (word << u16::BITS) | u64::from(*unit)));
    }
    rest.iter().for_each(|unit| hasher.write_u16(*unit));
    hasher.finish()
}

#[cfg(test)]
mod tests {
    /// ASCII text hashes as its UTF-16 units and its inline form do, at every inline length.
    #[test]
    fn ascii_and_wide_hashes_agree() {
        let text = "abcdefghijklmnopqrstuv";
        for len in 0..=text.len() {
            let ascii = &text.as_bytes()[..len];
            let wide: Vec<u16> = ascii.iter().map(|&byte| u16::from(byte)).collect();
            assert_eq!(super::Units::Ascii(ascii).text_hash(), super::Units::Wide(&wide).text_hash());
            assert_eq!(super::str_text_hash(&text[..len]), super::Units::Ascii(ascii).text_hash());
            let inline = super::JsString::from_str(&text[..len]);
            assert_eq!(inline.inline_text_hash(), Some(super::Units::Ascii(ascii).text_hash()));
        }
        let wide = "ключ-значение";
        let units: Vec<u16> = wide.encode_utf16().collect();
        assert_eq!(super::str_text_hash(wide), super::Units::Wide(&units).text_hash());
    }
}
