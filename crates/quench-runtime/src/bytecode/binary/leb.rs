//! The LEB128 varints of packed records: seven payload bits a byte, the high bit set on every
//! byte but the last.

use super::{BinaryReader, BinaryWriter};

/// The bit a varint byte sets when another byte follows it.
const CONTINUATION: u8 = 0x80;
/// The payload bits of a varint byte.
const PAYLOAD_MASK: u8 = !CONTINUATION;
/// Payload bits a varint byte carries.
const PAYLOAD_BITS: u32 = 7;
/// Bytes a `u32` varint takes at most: five of seven bits cover 32.
const MAX_BYTES: u32 = 5;

impl BinaryWriter {
    /// Appends `value` as a varint.
    pub(super) fn leb(&mut self, mut value: u32) {
        while value >= u32::from(CONTINUATION) {
            self.bytes.push(value as u8 | CONTINUATION);
            value >>= PAYLOAD_BITS;
        }
        self.bytes.push(value as u8);
    }
}

impl BinaryReader<'_> {
    /// Reads a varint: a one-byte value without a call, longer ones in `leb_long`.
    pub(super) fn leb(&mut self) -> Result<u32, String> {
        if let Some(&byte) = self.bytes.get(self.cursor)
            && byte < CONTINUATION
        {
            self.cursor += 1;
            return Ok(u32::from(byte));
        }
        self.leb_long()
    }

    /// Reads a varint of up to `MAX_BYTES` bytes.
    fn leb_long(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for shift in (0..MAX_BYTES * PAYLOAD_BITS).step_by(PAYLOAD_BITS as usize) {
            let byte = self.u8()?;
            value |= u32::from(byte & PAYLOAD_MASK) << shift;
            if byte & CONTINUATION == 0 {
                return Ok(value);
            }
        }
        Err("invalid packed integer".into())
    }
}
