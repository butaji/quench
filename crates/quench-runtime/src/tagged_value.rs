//! Canonical one-word value layout for the execute path.
//!
//! Values whose exponent is not all ones are IEEE-754 numbers.  Tagged values
//! use the quiet-NaN prefix `0x7ff8` and a 48-bit payload.  The payload's top
//! three bits are a tag; the remaining 45 bits carry the value.

use core::fmt;

const TAG_PREFIX: u64 = 0x7ff8_0000_0000_0000;
const TAG_SHIFT: u32 = 45;
const TAG_MASK: u64 = 0x0000_e000_0000_0000;
const PAYLOAD_BITS: u32 = 45;
const PAYLOAD_MASK: u64 = (1u64 << PAYLOAD_BITS) - 1;
const I31_MIN: i32 = -(1 << 30);
const I31_MAX: i32 = (1 << 30) - 1;
const HEAP_INDEX_BITS: u32 = 24;
const HEAP_INDEX_MASK: u64 = (1u64 << HEAP_INDEX_BITS) - 1;
const HEAP_GENERATION_BITS: u32 = PAYLOAD_BITS - HEAP_INDEX_BITS;
const HEAP_GENERATION_MASK: u64 = (1u64 << HEAP_GENERATION_BITS) - 1;
const HEAP_POINTER_SHIFT: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapHandle {
    pub reference: crate::identity::HeapRef,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecodedValue {
    Number(f64),
    I31(i32),
    Bool(bool),
    Null,
    Undefined,
    ObjectPtr(usize),
    ArrayPtr(usize),
    FunctionPtr(usize),
    HeapRef(HeapHandle),
    HeapPtr(usize),
}

macro_rules! value_tag_facts {
    ($( $name:ident = $tag:literal, rc = $owns_rc:literal => $accessor:ident($decoded:pat); )+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        enum ValueTag { $( $name = $tag, )+ }

        impl ValueTag {
            const fn decode(value: u8) -> Option<Self> {
                match value { $( $tag => Some(Self::$name), )+ _ => None }
            }

            const fn owns_rc(self) -> bool {
                match self { $( Self::$name => $owns_rc, )+ }
            }
        }

        const TAG_COUNT: usize = [$( ValueTag::$name as u8, )+].len();

        impl TaggedValue {
            $(
                #[inline]
                pub fn $accessor(self) -> bool {
                    matches!(self.decode(), $decoded)
                }
            )+

            #[inline(always)]
            pub(crate) fn owns_rc(self) -> bool {
                if self.0 & TAG_PREFIX != TAG_PREFIX {
                    return false;
                }
                let tag = ((self.0 & TAG_MASK) >> TAG_SHIFT) as u8;
                ValueTag::decode(tag).is_some_and(ValueTag::owns_rc)
            }
        }
    };
}

value_tag_facts! {
    I31 = 1, rc = false => is_i31(DecodedValue::I31(_));
    Primitive = 2, rc = false => is_primitive(DecodedValue::Bool(_) | DecodedValue::Null | DecodedValue::Undefined);
    ObjectPtr = 3, rc = true => is_object_ptr(DecodedValue::ObjectPtr(_));
    ArrayPtr = 4, rc = true => is_array_ptr(DecodedValue::ArrayPtr(_));
    FunctionPtr = 5, rc = true => is_function_ptr(DecodedValue::FunctionPtr(_));
    HeapPtr = 6, rc = true => is_heap_ptr(DecodedValue::HeapPtr(_));
    HeapRef = 7, rc = false => is_heap_ref(DecodedValue::HeapRef(_));
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
            .then(|| Self::tag(ValueTag::I31, (value as i64 as u64) & ((1u64 << 31) - 1)))
    }
    #[inline]
    pub fn bool(value: bool) -> Self {
        Self::tag(ValueTag::Primitive, value as u64)
    }
    #[inline]
    pub const fn null() -> Self {
        Self::tag(ValueTag::Primitive, 2)
    }
    #[inline]
    pub const fn undefined() -> Self {
        Self::tag(ValueTag::Primitive, 3)
    }
    pub fn heap_ref(handle: HeapHandle) -> Option<Self> {
        (u64::from(handle.reference.0) <= HEAP_INDEX_MASK
            && u64::from(handle.generation) <= HEAP_GENERATION_MASK)
            .then(|| {
                Self::tag(
                    ValueTag::HeapRef,
                    u64::from(handle.generation) << HEAP_INDEX_BITS | u64::from(handle.reference.0),
                )
            })
    }
    pub fn heap_ptr(pointer: usize) -> Option<Self> {
        Self::pointer(ValueTag::HeapPtr, pointer)
    }
    pub fn object_ptr(pointer: usize) -> Option<Self> {
        Self::pointer(ValueTag::ObjectPtr, pointer)
    }
    pub fn array_ptr(pointer: usize) -> Option<Self> {
        Self::pointer(ValueTag::ArrayPtr, pointer)
    }
    pub fn function_ptr(pointer: usize) -> Option<Self> {
        Self::pointer(ValueTag::FunctionPtr, pointer)
    }
    fn pointer(tag: ValueTag, pointer: usize) -> Option<Self> {
        let pointer = pointer as u64;
        (pointer & ((1 << HEAP_POINTER_SHIFT) - 1) == 0
            && pointer >> HEAP_POINTER_SHIFT <= PAYLOAD_MASK)
            .then(|| Self::tag(tag, pointer >> HEAP_POINTER_SHIFT))
    }
    #[inline]
    pub fn decode(self) -> DecodedValue {
        let tag = ((self.0 & TAG_MASK) >> TAG_SHIFT) as u8;
        if self.0 & TAG_PREFIX != TAG_PREFIX || tag == 0 {
            return DecodedValue::Number(f64::from_bits(self.0));
        }
        let payload = self.0 & PAYLOAD_MASK;
        match ValueTag::decode(tag) {
            Some(ValueTag::I31) => DecodedValue::I31(((payload as u32) << 1) as i32 >> 1),
            // Constructors only emit the canonical one-bit boolean payload.
            // Treat arbitrary bits supplied through `from_bits` as malformed
            // rather than silently changing their meaning.
            Some(ValueTag::Primitive) if payload <= 1 => DecodedValue::Bool(payload != 0),
            Some(ValueTag::Primitive) if payload == 2 => DecodedValue::Null,
            Some(ValueTag::Primitive) if payload == 3 => DecodedValue::Undefined,
            Some(ValueTag::ObjectPtr) => {
                DecodedValue::ObjectPtr((payload << HEAP_POINTER_SHIFT) as usize)
            }
            Some(ValueTag::ArrayPtr) => {
                DecodedValue::ArrayPtr((payload << HEAP_POINTER_SHIFT) as usize)
            }
            Some(ValueTag::FunctionPtr) => {
                DecodedValue::FunctionPtr((payload << HEAP_POINTER_SHIFT) as usize)
            }
            Some(ValueTag::HeapRef) => DecodedValue::HeapRef(HeapHandle {
                reference: crate::identity::HeapRef((payload & HEAP_INDEX_MASK) as u32),
                generation: (payload >> HEAP_INDEX_BITS) as u32,
            }),
            Some(ValueTag::HeapPtr) => {
                DecodedValue::HeapPtr((payload << HEAP_POINTER_SHIFT) as usize)
            }
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

    const fn tag(tag: ValueTag, payload: u64) -> Self {
        Self(TAG_PREFIX | (tag as u64) << TAG_SHIFT | payload)
    }
}

const _: () = assert!(core::mem::size_of::<TaggedValue>() == 8);
const _: () = assert!(core::mem::align_of::<TaggedValue>() == 8);
const _: () = assert!(TAG_COUNT == 7);

impl fmt::Debug for TaggedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TaggedValue")
            .field(&format_args!("0x{:016x}", self.0))
            .finish()
    }
}

impl crate::value::Value {
    #[inline]
    pub fn to_tagged(&self) -> Option<TaggedValue> {
        match self {
            Self::Number(value) => Some(TaggedValue::number(*value)),
            Self::Boolean(value) => Some(TaggedValue::bool(*value)),
            Self::Null => Some(TaggedValue::null()),
            Self::Undefined => Some(TaggedValue::undefined()),
            _ => None,
        }
    }

    #[inline]
    pub fn from_tagged(value: TaggedValue) -> Option<Self> {
        match value.decode() {
            DecodedValue::Number(value) => Some(Self::Number(value)),
            DecodedValue::I31(value) => Some(Self::Number(f64::from(value))),
            DecodedValue::Bool(value) => Some(Self::Boolean(value)),
            DecodedValue::Null => Some(Self::Null),
            DecodedValue::Undefined => Some(Self::Undefined),
            DecodedValue::ObjectPtr(_)
            | DecodedValue::ArrayPtr(_)
            | DecodedValue::FunctionPtr(_)
            | DecodedValue::HeapRef(_)
            | DecodedValue::HeapPtr(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "tagged_value_tests.rs"]
mod tests;
