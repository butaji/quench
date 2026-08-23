//! Isolated one-word NaN-boxed value prototype.
//!
//! Values whose exponent is not all ones are IEEE-754 numbers.  Tagged values
//! use the quiet-NaN prefix `0x7ff8` and a 48-bit payload.  The payload's top
//! four bits are a tag; the remaining 44 bits carry the value.

use core::fmt;

const TAG_PREFIX: u64 = 0x7ff8_0000_0000_0000;
const TAG_SHIFT: u32 = 44;
const TAG_MASK: u64 = 0x0000_f000_0000_0000;
const PAYLOAD_BITS: u32 = 44;
const PAYLOAD_MASK: u64 = (1u64 << PAYLOAD_BITS) - 1;
const I31_MIN: i32 = -(1 << 30);
const I31_MAX: i32 = (1 << 30) - 1;
const HEAP_INDEX_BITS: u32 = 24;
const HEAP_INDEX_MASK: u64 = (1u64 << HEAP_INDEX_BITS) - 1;
const HEAP_GENERATION_BITS: u32 = PAYLOAD_BITS - HEAP_INDEX_BITS;
const HEAP_GENERATION_MASK: u64 = (1u64 << HEAP_GENERATION_BITS) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapRef {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecodedValue {
    Number(f64),
    I31(i32),
    Bool(bool),
    Null,
    Undefined,
    Builtin(u64),
    HeapRef(HeapRef),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TaggedValue(u64);

impl TaggedValue {
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
    pub const fn bits(self) -> u64 {
        self.0
    }

    #[inline]
    pub fn number(value: f64) -> Self {
        if value.is_nan() {
            Self(TAG_PREFIX)
        } else {
            Self(value.to_bits())
        }
    }
    #[inline]
    pub fn i31(value: i32) -> Option<Self> {
        (I31_MIN..=I31_MAX)
            .contains(&value)
            .then(|| Self::tag(1, (value as i64 as u64) & ((1u64 << 31) - 1)))
    }
    #[inline]
    pub fn bool(value: bool) -> Self {
        Self::tag(2, value as u64)
    }
    #[inline]
    pub const fn null() -> Self {
        Self::tag(3, 0)
    }
    #[inline]
    pub const fn undefined() -> Self {
        Self::tag(4, 0)
    }
    pub fn builtin(payload: u64) -> Option<Self> {
        (payload <= PAYLOAD_MASK).then(|| Self::tag(5, payload))
    }
    pub fn heap_ref(reference: HeapRef) -> Option<Self> {
        (reference.index as u64 <= HEAP_INDEX_MASK
            && reference.generation as u64 <= HEAP_GENERATION_MASK)
            .then(|| {
                Self::tag(
                    6,
                    (reference.generation as u64) << HEAP_INDEX_BITS | reference.index as u64,
                )
            })
    }
    #[inline]
    pub fn decode(self) -> DecodedValue {
        let tag = ((self.0 & TAG_MASK) >> TAG_SHIFT) as u8;
        if self.0 & TAG_PREFIX != TAG_PREFIX || tag == 0 {
            return DecodedValue::Number(f64::from_bits(self.0));
        }
        let payload = self.0 & PAYLOAD_MASK;
        match tag {
            1 => DecodedValue::I31(((payload as u32) << 1) as i32 >> 1),
            // Constructors only emit the canonical one-bit boolean payload.
            // Treat arbitrary bits supplied through `from_bits` as malformed
            // rather than silently changing their meaning.
            2 if payload <= 1 => DecodedValue::Bool(payload != 0),
            // Null and undefined are singleton values and therefore have no
            // payload.  Reject forged encodings instead of accepting them.
            3 if payload == 0 => DecodedValue::Null,
            4 if payload == 0 => DecodedValue::Undefined,
            5 => DecodedValue::Builtin(payload),
            6 => DecodedValue::HeapRef(HeapRef {
                index: (payload & HEAP_INDEX_MASK) as u32,
                generation: (payload >> HEAP_INDEX_BITS) as u32,
            }),
            _ => DecodedValue::Number(f64::NAN),
        }
    }
    /// Return the IEEE-754 number represented by this word, if it is a number.
    ///
    /// Tagged scalar values are deliberately not coerced here: callers must
    /// handle I31 and the other primitive tags according to JavaScript rules.
    #[inline]
    pub fn number_value(self) -> Option<f64> {
        match self.decode() {
            DecodedValue::Number(value) => Some(value),
            _ => None,
        }
    }

    const fn tag(tag: u8, payload: u64) -> Self {
        Self(TAG_PREFIX | (tag as u64) << 44 | payload)
    }
}

impl fmt::Debug for TaggedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TaggedValue")
            .field(&format_args!("0x{:016x}", self.0))
            .finish()
    }
}

#[cfg(test)]
#[path = "tagged_value_tests.rs"]
mod tests;
