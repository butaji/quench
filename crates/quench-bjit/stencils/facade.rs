//! The runtime's `crate::` paths as the shared staged rows name them, so the rows in
//! `quench-runtime/src/staged/*.rs` compile unchanged under the stencil build: the layout
//! numbers (`layout.rs`), a bare `Value`, and the stage the rows run on (`StencilStage`).

pub(crate) use crate::layout::{INLINE_PROPERTIES as _INLINE, words};

/// A tagged word. Only its constants and bit conversions exist here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// The runtime's `ValueType` (`api_native.rs`): the same variants in the same order, so each
/// discriminant the lattice's bit rows shift by is the runtime's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueType {
    Undefined,
    Null,
    Boolean,
    Number,
    String,
    Symbol,
    BigInt,
    Object,
    Function,
}

// The runtime's type lattice (`quench-runtime/src/typed_ir`), included unchanged: `lattice.rs`
// holds `Type`, `int_range.rs` `IntRange`, `int_type.rs` the integer-range operations, and
// `representation.rs` the slot encodings. Their `crate::` paths resolve to the items here.
#[path = "../../quench-runtime/src/typed_ir/lattice.rs"]
mod lattice;
#[path = "../../quench-runtime/src/typed_ir/int_range.rs"]
mod int_range;
#[path = "../../quench-runtime/src/typed_ir/int_type.rs"]
mod int_type;
#[path = "../../quench-runtime/src/typed_ir/representation.rs"]
mod representation;

/// The stencil's view of the runtime's `typed_ir`: the same `Type`, `TypeKind`, `IntRange` and
/// `Representation` the interpreter and the u-jit use, so a row folds the same facts.
pub(crate) mod typed_ir {
    pub(crate) use super::int_range::IntRange;
    pub(crate) use super::lattice::{Type, TypeKind};
    pub(crate) use super::representation::Representation;
}

pub(crate) mod value {
    /// 2^63, the first double past `i64` (`number_conversion::EXACT_I64_TRUNCATION_LIMIT`).
    pub(crate) const EXACT_I64_TRUNCATION_LIMIT: f64 = 9_223_372_036_854_775_808.0;
    use crate::layout::words;
    pub(crate) use super::operations::WORD32_SHIFT_MASK;
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
        pub(crate) const DIRECT_PROMISE_TAG: u64 = words::DIRECT_PROMISE_TAG;
        pub(crate) const DIRECT_ITERATOR_TAG: u64 = words::DIRECT_ITERATOR_TAG;
        pub(crate) const DIRECT_COLLECTION_TAG: u64 = words::DIRECT_COLLECTION_TAG;
        pub(crate) const DIRECT_FUNCTION_TAG: u64 = words::DIRECT_FUNCTION_TAG;
        pub(crate) const DIRECT_PROMISE_TAG: u64 = words::DIRECT_PROMISE_TAG;
        pub(crate) const DIRECT_ITERATOR_TAG: u64 = words::DIRECT_ITERATOR_TAG;
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
    /// The object layout table (`heap::layouts`) at its fixed address (`LAYOUT_TABLE_BASE`).
    pub(crate) mod layouts {
        /// Distinct headers the table holds (`heap::layouts::LAYOUT_CAPACITY`).
        const LAYOUT_CAPACITY: usize = 1 << 22;
        /// `heap::layouts::word`: the header word of the interned layout `id`.
        #[inline(always)]
        pub(crate) fn word(id: u32) -> u64 {
            // SAFETY: an interned id's entry, written before the id existed.
            unsafe { *(crate::layout::LAYOUT_TABLE_BASE as *const u64).add(id as usize) }
        }
        /// `heap::layouts::auxiliary`: the low half of layout `id`'s header word, 0 past the table.
        #[inline(always)]
        pub(crate) fn auxiliary(id: u32) -> u32 {
            if id as usize >= LAYOUT_CAPACITY {
                return 0;
            }
            // SAFETY: the runtime maps the table before any stencil runs; an entry below the
            // capacity is readable (zero until interned).
            unsafe { *(crate::layout::LAYOUT_TABLE_BASE as *const u64).add(id as usize) as u32 }
        }
    }
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
/// The `typeof` names and their indices, the runtime's own file.
#[path = "../../quench-runtime/src/bytecode/typeof_names.rs"]
#[allow(dead_code)]
mod typeof_names;

pub(crate) mod bytecode {
    pub(crate) type Register = u16;

    pub(crate) use super::typeof_names::typeof_name_index;

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
