//! The runtime's `crate::` paths as the shared staged rows name them, so the rows in
//! `quench-runtime/src/staged/*.rs` compile unchanged under the stencil build: the layout
//! numbers (`layout.rs`), a bare `Value`, and the stage the rows run on (`StencilStage`).

pub(crate) use crate::layout::{INLINE_PROPERTY_COUNT as _INLINE, words};

/// A tagged word. Only its constants and bit conversions exist here.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Value(u64);

impl Value {
    pub(crate) const NULL: Self = Self(words::NULL);
    pub(crate) const UNDEFINED: Self = Self(words::UNDEFINED);
    pub(crate) const DELETED: Self = Self(words::DELETED);
    pub(crate) const FALSE: Self = Self(words::FALSE);
    pub(crate) const fn to_bits(self) -> u64 {
        self.0
    }
    pub(crate) const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
}

pub(crate) mod typed_ir {
    /// The fact a stage keeps of a word: a stencil keeps none.
    #[derive(Clone, Copy)]
    pub(crate) struct Type;
    impl Type {
        pub(crate) const ANY: Self = Self;
    }
}

pub(crate) mod value {
    use crate::layout::words;
    pub(crate) struct ValueLayout;
    impl ValueLayout {
        pub(crate) const TAG_MASK: u64 = words::TAG_MASK;
        pub(crate) const INT_TAG: u64 = words::INT_TAG;
        pub(crate) const INTERNAL_TAG: u64 = words::INTERNAL_TAG;
        pub(crate) const HEAP_TAG: u64 = words::HEAP_TAG;
        pub(crate) const DIRECT_ARRAY_TAG: u64 = words::DIRECT_ARRAY_TAG;
        pub(crate) const DIRECT_OBJECT_TAG: u64 = words::DIRECT_OBJECT_TAG;
        pub(crate) const DIRECT_FUNCTION_TAG: u64 = words::DIRECT_FUNCTION_TAG;
        pub(crate) const HEAP_FAMILY_MASK: u64 = words::HEAP_FAMILY_MASK;
        pub(crate) const DIRECT_STRING_TAG: u64 = words::DIRECT_STRING_TAG;
        pub(crate) const TAG_SHIFT: u32 = words::TAG_SHIFT;
        pub(crate) const FIRST_TAG_FIELD: u64 = words::FIRST_TAG_FIELD;
        pub(crate) const TAG_FIELD_SPAN: u64 = words::TAG_FIELD_SPAN;
        pub(crate) const CANONICAL_NAN: u64 = words::CANONICAL_NAN;
    }
    pub(crate) struct NarrowLayout;
    impl NarrowLayout {
        pub(crate) const INTEGER_FLAG: u32 = words::NARROW_INTEGER_FLAG;
        pub(crate) const INTEGER_BITS: u32 = words::NARROW_INTEGER_BITS;
        pub(crate) const CODE_SHIFT: u32 = words::NARROW_CODE_SHIFT;
        pub(crate) const CODE_MASK: u32 = words::NARROW_CODE_MASK;
        pub(crate) const INDEX_SHIFT: u32 = words::NARROW_INDEX_SHIFT;
        pub(crate) const INDEX_BITS: u32 = words::NARROW_INDEX_BITS;
        pub(crate) const DIRECT_OBJECT_UPPER: u64 = words::DIRECT_OBJECT_UPPER;
        pub(crate) const DIRECT_OBJECT_CODE: u32 = words::DIRECT_OBJECT_NARROW_CODE;
        /// The process's upper-word table: a hole a row that decodes a narrow value must be
        /// given (no row does yet; an unknown hole fails `build.rs`).
        pub(crate) fn upper_words() -> *const u64 {
            unsafe extern "C" {
                static HOLE_UPPER_WORDS: u64;
            }
            &raw const HOLE_UPPER_WORDS
        }
    }
}

pub(crate) mod value_vec {
    pub(crate) use crate::layout::INLINE_PROPERTY_COUNT;
}

pub(crate) mod heap {
    pub(crate) use crate::layout::{LENGTH_FROM_VALUES, NATIVE_ARRAY_LAYOUT, NATIVE_ENVIRONMENT_LAYOUT, NATIVE_OBJECT_LAYOUT, OTHERS_FROM_VALUES, OTHERS_UNKNOWN};
    #[derive(Clone, Copy)]
    pub(crate) enum PlainPropertyStorage {
        PackedInline,
        Arena,
    }
    pub(crate) struct Object;
    pub(crate) struct ArrayObject;
    pub(crate) struct EnvironmentSlots;
    pub(crate) struct Heap;
}

pub(crate) mod vm {
    pub(crate) use crate::layout::NATIVE_FIELD_CACHE_LAYOUT;
}
