//! Inline ASCII text: read as a `str`, and joined without leaving its inline form.
use super::{ShortAscii, SHORT_ASCII_CAPACITY};

impl ShortAscii {
    pub(crate) fn as_str(&self) -> &str {
        // SAFETY: only `FlatUnits::ascii` fills the bytes, from ASCII text.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..usize::from(self.length)]) }
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
