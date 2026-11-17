//! Inline ASCII text: read as a `str`, and joined without leaving its inline form.
use super::{ShortAscii, SHORT_ASCII_CAPACITY};

/// The bytes of one word of an inline-text comparison, and the words that cover the text.
const COMPARED_WORD: usize = size_of::<u64>();
const COMPARED_WORDS: usize = SHORT_ASCII_CAPACITY.div_ceil(COMPARED_WORD);

impl ShortAscii {
    pub(crate) fn as_str(&self) -> &str {
        // SAFETY: only `FlatUnits::ascii` fills the bytes, from ASCII text.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..usize::from(self.length)]) }
    }

    /// The code-unit order of two inline texts, as big-endian word compares: both pad
    /// their unused bytes with zero, so a padded tie is a prefix, which the lengths order.
    #[inline(always)]
    pub(super) fn order(&self, other: &Self) -> std::cmp::Ordering {
        let words = |text: &Self| {
            let mut padded = [0; COMPARED_WORDS * COMPARED_WORD];
            padded[..SHORT_ASCII_CAPACITY].copy_from_slice(&text.bytes);
            let mut words = [0u64; COMPARED_WORDS];
            for (word, chunk) in words.iter_mut().zip(padded.chunks_exact(COMPARED_WORD)) {
                *word = u64::from_be_bytes(chunk.try_into().unwrap_or_default());
            }
            words
        };
        words(self).cmp(&words(other)).then(self.length.cmp(&other.length))
    }

    /// This text then `tail`, when both fit inline: whole fixed-size copies, as both pad their
    /// unused bytes with zero.
    #[inline(always)]
    pub(super) fn joined(&self, tail: &Self) -> Option<Self> {
        let (head_length, length) = (usize::from(self.length), usize::from(self.length) + usize::from(tail.length));
        if length > SHORT_ASCII_CAPACITY {
            return None;
        }
        let mut bytes = [0; SHORT_ASCII_CAPACITY + SHORT_ASCII_CAPACITY];
        bytes[..SHORT_ASCII_CAPACITY].copy_from_slice(&self.bytes);
        bytes[head_length..head_length + SHORT_ASCII_CAPACITY].copy_from_slice(&tail.bytes);
        Some(Self { length: length as u8, bytes: bytes[..SHORT_ASCII_CAPACITY].try_into().ok()? })
    }
}
