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
    /// The canonical NaN, as the runtime's `Value::NAN`.
    pub(crate) const NAN: Self = Self(words::CANONICAL_NAN);
    /// The runtime's `Value::boolean`.
    pub(crate) const fn boolean(value: bool) -> Self {
        if value { Self::TRUE } else { Self::FALSE }
    }
    /// The runtime's `Value::integer`: the int32 tag and the payload's low 32 bits.
    pub(crate) const fn integer(value: i32) -> Self {
        Self(words::INT_TAG | value as u32 as u64)
    }
    /// The runtime's `Value::as_int`.
    pub(crate) const fn as_int(self) -> Option<i32> {
        if self.0 & words::TAG_MASK == words::INT_TAG { Some(self.0 as u32 as i32) } else { None }
    }
    /// Whether the word is a tag, not a double: its tag field lies in the tag range, as the
    /// runtime's `Value::is_tagged`.
    const fn is_tagged(self) -> bool {
        ((self.0 >> words::TAG_SHIFT) as u32).wrapping_sub(words::FIRST_TAG_FIELD as u32) <= words::TAG_FIELD_SPAN as u32
    }
    /// The runtime's `Value::as_number`: a double's bits, or an int32 payload.
    pub(crate) fn as_number(self) -> Option<f64> {
        if !self.is_tagged() {
            Some(f64::from_bits(self.0))
        } else if self.0 & words::TAG_MASK == words::INT_TAG {
            Some(self.0 as u32 as i32 as f64)
        } else {
            None
        }
    }
    /// The runtime's `Value::number`: an integral value within int32 (but not -0) as an
    /// int32, NaN as the canonical NaN, any other double as its bits. The one copy is
    /// `words::canonical`, which the stencil rows also call.
    #[inline(always)]
    pub(crate) fn number(value: f64) -> Self {
        Self(crate::words::canonical(value))
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
#[path = "../../quench-runtime/src/typed_ir/binding.rs"]
mod binding;
// The binary64 fields the runtime's Number conversions read (`float_layout.rs`), included unchanged.
#[path = "../../quench-runtime/src/float_layout.rs"]
pub(crate) mod float_layout;

/// The stencil's view of the runtime's `typed_ir`: the same `Type`, `TypeKind`, `IntRange` and
/// `Representation` the interpreter and the u-jit use, so a row folds the same facts.
pub(crate) mod typed_ir {
    pub(crate) use super::int_range::IntRange;
    pub(crate) use super::lattice::{Type, TypeKind, language_bit};
    pub(crate) use super::representation::{Representation, repr};
    pub(crate) use super::binding::{BindingTime, Division, Proof};
}

/// The runtime's Number conversions (ToInt32 and its kin), which a row folds into a constant operand.
#[path = "../../quench-runtime/src/value/number_conversion.rs"]
#[allow(dead_code)] // the rows take ToInt32 alone
mod number_conversion;

pub(crate) mod value {
    pub(crate) use super::operations::WORD32_SHIFT_MASK;
    pub(crate) use super::number_conversion::{exact_uint32, number_to_u32, EXACT_I64_TRUNCATION_LIMIT};
    pub(crate) use crate::layout::value_encoding::{
        HEAP_FAMILY_BITS, HEAP_FAMILY_SHIFT, LOWER_WORD, NARROW_HEAP_FAMILIES, NarrowLayout, TAG_BOOL, TAG_DELETED, TAG_NULL,
        TAG_UNDEFINED, ValueLayout,
    };
    impl NarrowLayout {
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
    pub(crate) use crate::layout::{DOUBLE_LEN, ENCODED_LEN, LENGTH_FROM_VALUES, NARROW_LEN, NATIVE_ARRAY_LAYOUT, NATIVE_ENVIRONMENT_LAYOUT, NATIVE_FUNCTION_LAYOUT, NATIVE_OBJECT_LAYOUT, NATIVE_TYPED_ARRAY_LAYOUT, OTHERS_FROM_VALUES, OTHERS_UNKNOWN, SMALL_STRING_ID_BASE, SMALL_STRING_ID_MASK, TypedArrayKind};
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
        pub(crate) const LAYOUT_CAPACITY: usize = 1 << 22;
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
        /// A flat string's units, as the runtime's `Units`: only the concrete callers of the
        /// shared string rows build one; no stencil does.
        #[derive(Clone, Copy)]
        #[allow(dead_code)]
        pub(crate) enum Units<'a> {
            Ascii(&'a [u8]),
            Latin1(&'a [u8]),
            Wide(&'a [u16]),
        }
        pub(crate) mod native_string {
            pub(crate) use crate::layout::shared_words::string_tag::*;
            pub(crate) use crate::layout::string_form::*;
        }
    }
}

/// The runtime's own operand encodings (`bytecode/operands.rs`), so a row decodes the same words the
/// runtime writes; `bytecode` names them as the runtime's `crate::bytecode` does.
#[path = "../../quench-runtime/src/bytecode/operands.rs"]
#[allow(dead_code)] // the rows read part of the encodings
mod operands;

/// A literal's element registers (`bytecode/register_list.rs`), which an allocation row carries.
#[path = "../../quench-runtime/src/bytecode/register_list.rs"]
#[allow(dead_code)]
mod register_list;

/// The register, atom and operand names the staged rows take.
pub(crate) mod bytecode {
    pub(crate) type Register = u16;
    pub(crate) type Atom = u32;
    pub(crate) use super::operands::{ConstructArguments, FieldBase, FieldLookup, Operand, OperandKind, RegisterWindow, REGISTER_MASK, SET_THIS_REGISTER};
    pub(crate) use super::register_list::{ARRAY_LITERAL_WINDOW, RegisterList};
}
