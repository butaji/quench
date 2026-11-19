// The head of an object's extras record: the words generated code reads of an object whose values
// or elements are displaced there. `heap::cell::ObjectExtras` embeds it first (`Deref` forwards its
// fields), so its offsets are the head's. Generic over the runtime's inline-value and element-backing
// types; the stencil instantiates it with words of the same size and alignment (`ObjectExtrasWords`),
// and `base_rows` checks the runtime's offsets against it.
#![allow(dead_code)]

use super::object_encoding::INLINE_PROPERTY_COUNT;

/// The fields native code reads of an object's extras, in declaration order (`#[repr(C)]`).
#[repr(C)]
#[derive(Clone, Debug, Default)]
pub(crate) struct ObjectExtrasHead<Inline, Elements> {
    /// The object's inline property values, displaced from its payload by the extras pointer.
    pub(crate) inline_properties: Inline,
    /// An array's storage position, displaced from its first payload word.
    pub(crate) arena_start: u32,
    /// An array's element backing, displaced from its payload word (`ArrayObject`); empty for any
    /// other object.
    pub(crate) elements: Elements,
}

/// The head with the stencil's words: the inline values as words, the element backing as two words.
pub(crate) type ObjectExtrasWords = ObjectExtrasHead<[u64; INLINE_PROPERTY_COUNT], [u32; 2]>;
