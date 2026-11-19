//! The payload layout of a Wasm GC object, derived once per declared type (`WasmTypes::layout`).
//! layout: a payload is bytes. A struct's fields sit in declaration order, each at the natural
//! alignment of its storage (i8 one byte, i16 two, i32 and f32 four, the rest a word), and the
//! payload rounds up to a word; a descriptor, when the type has one, is the reference word after
//! those. An array's payload starts with its length, a word, then its elements at their storage
//! size, packed. A reference, or a vector's box, is a `Value` word, which the collector follows;
//! no other word is traced.

use crate::WasmTypes;
use std::ops::Range;
use wasmparser::{CompositeInnerType, StorageType, ValType};

/// A word: a `Value`'s bits, an array's length, a descriptor.
const WORD: usize = 8;

/// Where an object's words sit in its payload, from the shape of its declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ObjectLayout {
    /// A struct: each field's offset in declaration order, the offsets of its reference words (the
    /// reference fields, then the descriptor), and the payload's size.
    Struct { offsets: Box<[usize]>, references: Box<[usize]>, bytes: usize },
    /// An array: the storage size of each element, and whether an element is a reference word.
    Array { element: usize, reference: bool },
}

impl ObjectLayout {
    /// The layout of a struct of `fields` in declaration order, with a descriptor word last when
    /// `descriptor`.
    pub(crate) fn structure(fields: impl IntoIterator<Item = StorageType>, descriptor: bool) -> Self {
        let mut offsets = Vec::new();
        let mut references = Vec::new();
        let mut end: usize = 0;
        for storage in fields {
            let width = storage_bytes(storage);
            let offset = end.next_multiple_of(width);
            offsets.push(offset);
            if holds_reference(storage) {
                references.push(offset);
            }
            end = offset + width;
        }
        let mut bytes = end.next_multiple_of(WORD);
        if descriptor {
            references.push(bytes);
            bytes += WORD;
        }
        Self::Struct { offsets: offsets.into(), references: references.into(), bytes }
    }

    /// The layout of an array of `element` storage.
    pub(crate) fn array(element: StorageType) -> Self {
        Self::Array { element: storage_bytes(element), reference: holds_reference(element) }
    }

    /// The layout of each declaration of `types`, by its index; `None` for one that is no struct
    /// or array.
    pub(crate) fn of_module(types: &WasmTypes) -> Box<[Option<Self>]> {
        let declarations: usize = types.groups().iter().map(Vec::len).sum();
        (0..declarations).map(|index| Self::of_declaration(types, index)).collect()
    }

    fn of_declaration(types: &WasmTypes, index: usize) -> Option<Self> {
        let composite = &types.get(index)?.composite_type;
        if composite.shared {
            return None;
        }
        match &composite.inner {
            CompositeInnerType::Struct(structure) => {
                let descriptor = types.descriptor_type(index as u32).is_some();
                Some(Self::structure(structure.fields.iter().map(|field| field.element_type), descriptor))
            }
            CompositeInnerType::Array(array) => Some(Self::array(array.0.element_type)),
            CompositeInnerType::Func(_) | CompositeInnerType::Cont(_) => None,
        }
    }

    /// The payload's size: a struct's, or an array of `length` elements; `None` on overflow.
    pub(crate) fn bytes(&self, length: usize) -> Option<usize> {
        match self {
            Self::Struct { bytes, .. } => Some(*bytes),
            Self::Array { element, .. } => length.checked_mul(*element)?.checked_add(WORD),
        }
    }

    /// Where field `index` of a struct, or element `index` of an array, starts; `None` for a struct
    /// field it lacks. An array index is bounded by the array's length, which the caller checks.
    pub(crate) fn offset(&self, index: usize) -> Option<usize> {
        match self {
            Self::Struct { offsets, .. } => offsets.get(index).copied(),
            Self::Array { element, .. } => index.checked_mul(*element)?.checked_add(WORD),
        }
    }

    /// The payload bytes of `extent` elements of an array from element `index`; `None` for a struct.
    /// Copies between arrays use the destination's layout: admitted storages have one size.
    pub(crate) fn element_range(&self, index: usize, extent: usize) -> Option<Range<usize>> {
        let Self::Array { element, .. } = self else {
            return None;
        };
        let start = self.offset(index)?;
        Some(start..start.checked_add(extent.checked_mul(*element)?)?)
    }
}

/// The bytes a storage takes in a payload: a packed field its width, a 32-bit number four bytes,
/// anything else a word (a 64-bit number, a reference, a vector's box).
fn storage_bytes(storage: StorageType) -> usize {
    match storage {
        StorageType::I8 => 1,
        StorageType::I16 => 2,
        StorageType::Val(ValType::I32 | ValType::F32) => 4,
        StorageType::Val(_) => WORD,
    }
}

/// Whether a storage's word is a `Value` the collector follows: a reference, or a vector's box.
fn holds_reference(storage: StorageType) -> bool {
    matches!(storage, StorageType::Val(ValType::Ref(_) | ValType::V128))
}
