//! Native StringDecoder byte state shared with configured Node algorithms.
use crate::modules::buffer_codec;

pub(crate) struct Layout;
impl Layout {
    pub(crate) const INCOMPLETE_START: usize = 0;
    pub(crate) const INCOMPLETE_END: usize = Self::INCOMPLETE_START + 4;
    pub(crate) const MISSING: usize = Self::INCOMPLETE_END;
    pub(crate) const BUFFERED: usize = Self::MISSING + 1;
    pub(crate) const ENCODING: usize = Self::BUFFERED + 1;
    pub(crate) const SIZE: usize = Self::ENCODING + 1;
}
// Indices are the stable native encoding ABI, also used by internal/util.
pub(crate) const ENCODINGS: &[&str] = &[
    "ascii",
    "utf8",
    "base64",
    "utf16le",
    "latin1",
    "hex",
    "buffer",
    "base64url",
];

pub(crate) const MAX_STRING_BYTES: usize = 0x1fff_ffe8;
#[derive(Debug)]
pub(crate) struct StringTooLong;
fn make_string(bytes: &[u8], encoding: &str) -> Result<Vec<u16>, StringTooLong> {
    if encoding == "utf8" && bytes.len() > MAX_STRING_BYTES {
        return Err(StringTooLong);
    }
    buffer_codec::decode_units_limited(bytes, encoding, MAX_STRING_BYTES).ok_or(StringTooLong)
}

pub(crate) struct DecoderState(pub(crate) [u8; Layout::SIZE]);
impl DecoderState {
    fn encoding(&self) -> &'static str {
        ENCODINGS[self.0[Layout::ENCODING] as usize]
    }
    fn buffered(&self) -> usize {
        self.0[Layout::BUFFERED] as usize
    }
    fn missing(&self) -> usize {
        self.0[Layout::MISSING] as usize
    }
    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let state = Self(bytes.try_into().ok()?);
        ((state.0[Layout::ENCODING] as usize) < ENCODINGS.len()
            && ENCODINGS[state.0[Layout::ENCODING] as usize] != "buffer"
            && (state.buffered() == 0) == (state.missing() == 0)
            && state.buffered() + state.missing() <= Layout::INCOMPLETE_END)
            .then_some(state)
    }
    pub(crate) fn decode(&mut self, mut input: &[u8]) -> Result<Vec<u16>, StringTooLong> {
        let encoding = self.encoding();
        if matches!(encoding, "ascii" | "latin1" | "hex") {
            return make_string(input, encoding);
        }
        let mut result = Vec::new();
        if self.missing() > 0 {
            if encoding == "utf8" {
                let unexpected = input
                    .iter()
                    .take(self.missing())
                    .position(|byte| byte & 0xc0 != 0x80);
                if let Some(count) = unexpected {
                    let buffered = self.buffered();
                    self.0[buffered..buffered + count].copy_from_slice(&input[..count]);
                    self.0[Layout::BUFFERED] += count as u8;
                    self.0[Layout::MISSING] = 0;
                    input = &input[count..];
                }
            }
            let found = input.len().min(self.missing());
            let buffered = self.buffered();
            self.0[buffered..buffered + found].copy_from_slice(&input[..found]);
            input = &input[found..];
            self.0[Layout::MISSING] -= found as u8;
            self.0[Layout::BUFFERED] += found as u8;
            if self.missing() == 0 {
                result.extend(make_string(&self.0[..self.buffered()], encoding)?);
                self.0[Layout::BUFFERED] = 0;
            }
        }
        if input.is_empty() {
            return Ok(result);
        }
        debug_assert_eq!(self.missing(), 0);
        debug_assert_eq!(self.buffered(), 0);
        let mut buffered = 0;
        let mut missing = 0;
        if encoding == "utf8" && input[input.len() - 1] & 0x80 != 0 {
            for &byte in input.iter().rev() {
                buffered += 1;
                if byte & 0xc0 == 0x80 {
                    if buffered >= Layout::INCOMPLETE_END || buffered == input.len() {
                        buffered = 0;
                        break;
                    }
                } else {
                    let total = if byte & 0xe0 == 0xc0 {
                        2
                    } else if byte & 0xf0 == 0xe0 {
                        3
                    } else if byte & 0xf8 == 0xf0 {
                        4
                    } else {
                        0
                    };
                    if buffered >= total {
                        buffered = 0;
                    }
                    missing = if buffered == 0 { 0 } else { total - buffered };
                    break;
                }
            }
        } else if encoding == "utf16le" {
            if input.len() % 2 == 1 {
                buffered = 1;
                missing = 1;
            } else if input[input.len() - 1] & 0xfc == 0xd8 {
                buffered = 2;
                missing = 2;
            }
        } else if matches!(encoding, "base64" | "base64url") {
            buffered = input.len() % 3;
            if buffered > 0 {
                missing = 3 - buffered;
            }
        }
        self.0[Layout::BUFFERED] = buffered as u8;
        self.0[Layout::MISSING] = missing as u8;
        let complete = input.len() - buffered;
        if buffered > 0 {
            self.0[..buffered].copy_from_slice(&input[complete..]);
        }
        let body = make_string(&input[..complete], encoding)?;
        if result.len() > MAX_STRING_BYTES - body.len() {
            return Err(StringTooLong);
        }
        result.extend(body);
        Ok(result)
    }
    pub(crate) fn flush(&mut self) -> Result<Vec<u16>, StringTooLong> {
        if self.encoding() == "utf16le" && self.buffered() % 2 == 1 {
            self.0[Layout::MISSING] -= 1;
            self.0[Layout::BUFFERED] -= 1;
        }
        if self.buffered() == 0 {
            return Ok(Vec::new());
        }
        let result = make_string(&self.0[..self.buffered()], self.encoding())?;
        self.0[Layout::MISSING] = 0;
        self.0[Layout::BUFFERED] = 0;
        Ok(result)
    }
}
