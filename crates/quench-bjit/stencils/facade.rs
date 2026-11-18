//! The runtime's `crate::` paths as the shared staged rows name them, so the rows in
//! `quench-runtime/src/staged/*.rs` compile unchanged under the stencil build: the layout
//! numbers (`layout.rs`), a bare `Value`, and the stage the rows run on (`StencilStage`).

pub(crate) use crate::layout::{INLINE_PROPERTIES as _INLINE, words};

/// A tagged word. Only its constants and bit conversions exist here.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Value(u64);

impl Value {
    pub(crate) const NULL: Self = Self(words::NULL);
    pub(crate) const UNDEFINED: Self = Self(words::UNDEFINED);
    pub(crate) const DELETED: Self = Self(words::DELETED);
    pub(crate) const FALSE: Self = Self(words::FALSE);
    pub(crate) const TRUE: Self = Self(words::TRUE);
    pub(crate) const fn to_bits(self) -> u64 {
        self.0
    }
    pub(crate) const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
    /// What the shared rows' interpreter-side readers ask of a word; no stencil row calls them.
    pub(crate) const fn is_deleted(self) -> bool {
        self.0 == words::DELETED
    }
    pub(crate) const fn heap_index(self) -> Option<u32> {
        if self.0 & words::TAG_MASK == words::HEAP_TAG { Some(self.0 as u32) } else { None }
    }
}

pub(crate) mod typed_ir {
    /// An element kind's integer range, which only the runtime's seeds read.
    #[derive(Clone, Copy)]
    pub(crate) struct IntRange {
        pub(crate) min: i64,
        pub(crate) max: i64,
    }
    /// The fact a stage keeps of a word: a stencil keeps only the one a row states, that its word
    /// is a Number (`ic_rows::write`), which is immediate.
    #[derive(Clone, Copy)]
    pub(crate) struct Type(bool);
    impl Type {
        pub(crate) const ANY: Self = Self(false);
        pub(crate) const IMMEDIATE: Self = Self(false);
        pub(crate) const NUMBER: Self = Self(true);
        pub(crate) const fn int(_range: IntRange) -> Self { Self(false) }
        pub(crate) const fn is_int32(self) -> bool { false }
        /// A stencil proves no subtype but a stated Number's of Number and immediate: every other
        /// gate is the stage's.
        pub(crate) const fn is_subtype(self, _of: Self) -> bool { self.0 }
    }
}

pub(crate) mod value {
    use crate::layout::words;
    pub(crate) const LOWER_WORD: u64 = words::LOWER_WORD;
    pub(crate) const HEAP_FAMILY_BITS: u64 = words::HEAP_FAMILY_BITS;
    pub(crate) const HEAP_FAMILY_SHIFT: u32 = words::HEAP_FAMILY_SHIFT;
    pub(crate) const NARROW_HEAP_FAMILIES: usize = words::NARROW_HEAP_FAMILIES;
    pub(crate) const TAG_UNDEFINED: u64 = words::UNDEFINED;
    pub(crate) const TAG_NULL: u64 = words::NULL;
    pub(crate) const TAG_BOOL: u64 = words::FALSE;
    pub(crate) const TAG_DELETED: u64 = words::DELETED;
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
        pub(crate) const INTEGER_FLAG: u32 = words::NARROW_INT_FLAG;
        pub(crate) const INTEGER_BITS: u32 = words::NARROW_INTEGER_BITS;
        pub(crate) const CODE_SHIFT: u32 = words::NARROW_CODE_SHIFT;
        pub(crate) const CODE_MASK: u32 = words::NARROW_CODE_MASK;
        pub(crate) const INDEX_SHIFT: u32 = words::NARROW_INDEX_SHIFT;
        pub(crate) const INDEX_BITS: u32 = words::NARROW_INDEX_BITS;
        pub(crate) const DIRECT_OBJECT_UPPER: u64 = words::DIRECT_OBJECT_UPPER;
        pub(crate) const DIRECT_OBJECT_CODE: u32 = words::DIRECT_OBJECT_NARROW_CODE;
        /// The process's upper-word table, which the one row that decodes a narrow value (the
        /// site field read, `heap_rows.rs`) is given as value hole 1.
        #[inline(always)]
        pub(crate) fn upper_words() -> *const u64 {
            crate::words::hole64::<{ crate::words::HOLE_A }>() as *const u64
        }
    }
}

/// ToInt32 and the clamp the staged rows convert by (`units.rs`).
#[path = "units.rs"]
pub(crate) mod units;

pub(crate) mod value_vec {
    pub(crate) use crate::layout::INLINE_PROPERTIES as INLINE_PROPERTY_COUNT;
    /// What the shared rows' interpreter-side readers ask of an arena word; no stencil row calls it.
    pub(crate) const fn decode_staged_word(word: u64) -> super::Value {
        super::Value::from_bits(word)
    }
}

pub(crate) mod heap {
    pub(crate) use crate::layout::{DOUBLE_LEN, ENCODED_LEN, LENGTH_FROM_VALUES, NARROW_LEN, NATIVE_ARRAY_LAYOUT, NATIVE_ENVIRONMENT_LAYOUT, NATIVE_FUNCTION_LAYOUT, NATIVE_OBJECT_LAYOUT, OTHERS_FROM_VALUES, OTHERS_UNKNOWN, SMALL_STRING_ID_BASE, SMALL_STRING_ID_MASK, TypedArrayKind};
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

/// The Number operation rows, the runtime's own file: `crate::vm::operations` to the shared
/// rows, which name nothing else of the runtime.
#[path = "../../quench-runtime/src/vm/operations/number_rows.rs"]
pub(crate) mod operations;

pub(crate) mod vm {
    pub(crate) use super::operations;
    pub(crate) use crate::layout::NATIVE_FIELD_CACHE_LAYOUT;
    pub(crate) mod wtf16 {
        pub(crate) struct JsString;
        pub(crate) mod native_string {
            pub(crate) use crate::layout::shared_words::string_tag::*;
            pub(crate) use crate::layout::string_form::*;
        }
    }
}

/// The register and operand names the staged numeric rows take. Only a register operand exists
/// here: a row names its places by register.
pub(crate) mod bytecode {
    pub(crate) type Register = u16;

    #[derive(Clone, Copy)]
    pub(crate) struct Operand(Register);

    impl Operand {
        pub(crate) fn register(value: Register) -> Self {
            Self(value)
        }
        pub(crate) fn register_index(self) -> Option<Register> {
            Some(self.0)
        }
    }
}
