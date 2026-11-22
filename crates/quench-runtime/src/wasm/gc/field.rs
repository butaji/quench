//! A GC field access's operand, resolved by lowering.
use crate::Value;
use wasmparser::{StorageType, ValType};

/// A field operand: the struct field index (0 for an array element), the field's storage and its
/// value class, all resolved by lowering, so an access neither looks up its declaration nor
/// re-checks what validation proved. Layout, low bits first: the storage, the value class, the
/// index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcField(u32);

/// The Wasm kind of a field's word, which a tier gives its register: a 32-bit word (`i32`, `f32`
/// and every packed storage), a 64-bit number (`i64`, `f64`), a reference, or a vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcFieldValue {
    Narrow,
    Wide,
    Reference,
    Vector,
}

/// The packed storages and the bits each keeps, in tag order after a value field's tag 0.
const PACKED: [(StorageType, i32); 2] = [(StorageType::I8, u8::MAX as i32), (StorageType::I16, u16::MAX as i32)];
const STORAGE_BITS: u32 = PACKED.len().ilog2() + 1;
const STORAGE_MASK: u32 = (1 << STORAGE_BITS) - 1;
/// The value classes in tag order, the tag's bits above the storage.
const CLASSES: [GcFieldValue; 4] = [GcFieldValue::Narrow, GcFieldValue::Wide, GcFieldValue::Reference, GcFieldValue::Vector];
const CLASS_SHIFT: u32 = STORAGE_BITS;
const CLASS_MASK: u32 = (1 << CLASS_BITS) - 1;
const CLASS_BITS: u32 = CLASSES.len().ilog2();
const INDEX_SHIFT: u32 = CLASS_SHIFT + CLASS_BITS;

/// The value class of a storage: a packed field is narrow (its integer is 32 bits).
const fn class_of(storage: StorageType) -> GcFieldValue {
    match storage {
        StorageType::I8 | StorageType::I16 | StorageType::Val(ValType::I32 | ValType::F32) => GcFieldValue::Narrow,
        StorageType::Val(ValType::I64 | ValType::F64) => GcFieldValue::Wide,
        StorageType::Val(ValType::Ref(_)) => GcFieldValue::Reference,
        StorageType::Val(_) => GcFieldValue::Vector,
    }
}

impl GcField {
    #[inline(always)]
    pub(crate) fn new(index: u32, storage: StorageType) -> Option<Self> {
        let class = CLASSES.iter().position(|class| *class == class_of(storage)).expect("every class is listed") as u32;
        let packed = PACKED.iter().position(|(packed, _)| *packed == storage);
        let storage = packed.map_or(0, |at| at + 1) as u32;
        (index <= u32::MAX >> INDEX_SHIFT).then_some(Self(index << INDEX_SHIFT | class << CLASS_SHIFT | storage))
    }

    #[inline(always)]
    pub(crate) fn from_tag(tag: u32) -> Option<Self> {
        ((tag & STORAGE_MASK) as usize <= PACKED.len()).then_some(Self(tag))
    }

    #[inline(always)]
    pub(crate) fn tag(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub(crate) fn index(self) -> u32 {
        self.0 >> INDEX_SHIFT
    }

    /// The Wasm kind of the field's word; a packed field is `Narrow`, its integer.
    #[inline(always)]
    pub(crate) fn value(self) -> GcFieldValue {
        CLASSES[((self.0 >> CLASS_SHIFT) & CLASS_MASK) as usize]
    }

    /// The bits a packed field keeps; `None` for a value field.
    #[inline(always)]
    fn mask(self) -> Option<i32> {
        (self.0 & STORAGE_MASK).checked_sub(1).map(|at| PACKED[at as usize].1)
    }

    /// The number of bits a packed field keeps; `None` for a value field.
    #[inline(always)]
    pub(crate) fn packed_bits(self) -> Option<u32> {
        self.mask().map(i32::count_ones)
    }

    /// The bytes the field's storage takes in a payload (`ObjectLayout`'s rule): a packed field its
    /// width, a 32-bit word four, anything else a word.
    #[inline(always)]
    pub(crate) fn size(self) -> usize {
        match self.packed_bits() {
            Some(bits) => bits as usize / 8,
            None if self.value() == GcFieldValue::Narrow => 4,
            None => 8,
        }
    }

    /// The bits `bytes` hold for the field, zero-extended; `bytes` is the field's `size()` long.
    #[inline(always)]
    pub(crate) fn load(self, bytes: &[u8]) -> u64 {
        let mut word = [0u8; 8];
        word[..bytes.len()].copy_from_slice(bytes);
        u64::from_le_bytes(word)
    }

    /// Stores the low bits of `bits` in `bytes`, the field's storage: a packed field keeps its
    /// low bits, so an unsigned read returns what is stored.
    #[inline(always)]
    pub(crate) fn store(self, bytes: &mut [u8], bits: u64) {
        let length = bytes.len();
        bytes.copy_from_slice(&bits.to_le_bytes()[..length]);
    }

    /// The bits a value stores in a field of this storage: an integer's bits for a narrow field,
    /// the value's own bits otherwise. A 64-bit number is not a `GcField` concern (its bits are the
    /// cell's), so the caller resolves it first.
    #[inline(always)]
    pub(crate) fn raw_of(self, value: Value) -> u64 {
        match self.value() {
            GcFieldValue::Narrow => u64::from(value.as_int().expect("validated Wasm integer field") as u32),
            _ => value.to_bits(),
        }
    }
}

/// The `c` operand of a struct read when the lowering cannot carry the field's payload offset in it
/// (the offset does not fit): the read finds the field by its index instead.
pub const NO_STATIC_OFFSET: u16 = u16::MAX;

/// The static payload offset a struct read's `c` operand carries (`FieldLayout::StaticOffset`), or
/// `None` for `NO_STATIC_OFFSET`.
#[inline(always)]
pub fn static_offset(operand: u16) -> Option<u32> {
    (operand != NO_STATIC_OFFSET).then_some(u32::from(operand))
}
