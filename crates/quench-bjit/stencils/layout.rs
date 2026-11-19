//! The stencils' layout numbers. The values live in `quench-runtime/src/staged/layout_offsets.rs`,
//! which both builds include (`#[path]`), so a stencil and the runtime read one set of literals; the
//! runtime checks them against its own `offset_of!` values in `vm/compiled/base_rows.rs`.
//! The two words shared with the host (`call_record`, `shared_words`) are mounted here.

#[path = "../../quench-runtime/src/staged/layout_offsets.rs"]
mod offsets;
pub(crate) use offsets::*;

pub(crate) const NATIVE_OBJECT_LAYOUT: NativeObjectLayout = NativeObjectLayout {
    record_capacity: 1073741824,
    record_size: 12,
    record_align: 4,
    layout_offset: 8,
    meta_offset: 4,
    auxiliary_offset: 0,
    payload_offset: 0,
    extras_arena_start_offset: 24,
    extras_inline_values_offset: 8,
    inline_storage_mask: 536870912,
    array_mask: 268435456,
    storage_start_mask: 1073741823,
    dictionary_mask: 2147483648,
    inline_form: 536870912,
    extras_form: 268435456,
    narrow_form: 134217728,
    narrow_run_tag: 4294967296,
    integrity_mask: 3221225472,
    inline_property_count: 2,
    extras_tag: 9223090561878065152,
    extras_tag_mask: 18446462598732840960,
    proto_mask: 268435455,
    proto_record_base: 67108864,
    proto_record_end: 134217728,
    mark_records_per_block: 1024,
    mark_word_bits: 64,
    mark_block_size: 512,
    mark_offset: 128,
    property_value_size: 8,
    property_value_align: 8,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeFunctionLayout {
    pub(crate) record_size: usize,
    pub(crate) object_offset: usize,
}

pub(crate) const NATIVE_FUNCTION_LAYOUT: NativeFunctionLayout = NativeFunctionLayout {
    record_size: 40,
    object_offset: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeArrayLayout {
    pub(crate) record_capacity: usize,
    pub(crate) record_size: usize,
    pub(crate) record_align: usize,
    pub(crate) elements_offset: usize,
    pub(crate) extras_elements_offset: usize,
    pub(crate) elements_header_offset: usize,
    pub(crate) header_length_offset: usize,
    pub(crate) header_values_offset: usize,
    pub(crate) header_owners_offset: usize,
    pub(crate) header_capacity_offset: usize,
    pub(crate) capacity_mask: u32,
    pub(crate) object_offset: usize,
    pub(crate) frozen_mask: u32,
    pub(crate) integrity_mask: u32,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeRecordCursorLayout {
    pub(crate) size: usize,
    pub(crate) align: usize,
    pub(crate) next_offset: usize,
    pub(crate) limit_offset: usize,
    pub(crate) records_per_block: usize,
}

/// The runtime's `records::RecordCursor`: two `u32` words, `next` then `limit`. Read by the
/// runtime's `stencil_layout_matches` test; no stencil allocates inline yet.
#[allow(dead_code)]
pub(crate) const NATIVE_RECORD_CURSOR_LAYOUT: NativeRecordCursorLayout = NativeRecordCursorLayout {
    size: 8,
    align: 4,
    next_offset: 0,
    limit_offset: 4,
    records_per_block: 1024,
};

pub(crate) const NATIVE_ARRAY_LAYOUT: NativeArrayLayout = NativeArrayLayout {
    record_capacity: 1073741824,
    record_size: 12,
    record_align: 4,
    elements_offset: 0,
    extras_elements_offset: 28,
    elements_header_offset: 0,
    header_length_offset: 4,
    header_values_offset: 16,
    header_owners_offset: 0,
    header_capacity_offset: 8,
    capacity_mask: 1073741823,
    object_offset: 0,
    frozen_mask: 2147483648,
    integrity_mask: 3221225472,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeTypedArrayLayout {
    pub(crate) entry_size: usize,
    pub(crate) page_shift: u32,
    pub(crate) page_mask: usize,
    pub(crate) kind_offset: usize,
    pub(crate) detached_offset: usize,
    pub(crate) unshared_offset: usize,
    pub(crate) data_offset: usize,
    pub(crate) length_offset: usize,
    pub(crate) stamp_offset: usize,
}

pub(crate) const NATIVE_TYPED_ARRAY_LAYOUT: NativeTypedArrayLayout = NativeTypedArrayLayout {
    entry_size: 8,
    page_shift: 12,
    page_mask: 4095,
    kind_offset: 20,
    detached_offset: 21,
    unshared_offset: 22,
    data_offset: 0,
    length_offset: 16,
    stamp_offset: 8,
};

/// Where a loop entry record keeps an array's element count, after the elements' address (`LoopEntry::push_array`).
pub(crate) const LOOP_ENTRY_ARRAY_COUNT_AT: u64 = size_of::<u64>() as u64;

/// The runtime's `TypedArrayKind`, in its order (`stencil_layout_matches` checks both).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedArrayKind {
    Uint8,
    Uint8Clamped,
    Uint16,
    Uint32,
    Int8,
    Int16,
    Int32,
    BigInt64,
    BigUint64,
    Float16,
    Float32,
    Float64,
}
impl TypedArrayKind {
    /// Every kind, in discriminant order.
    pub(crate) const ALL: [Self; 12] = [Self::Uint8, Self::Uint8Clamped, Self::Uint16, Self::Uint32, Self::Int8, Self::Int16, Self::Int32, Self::BigInt64, Self::BigUint64, Self::Float16, Self::Float32, Self::Float64];
    /// The element width in bytes.
    pub(crate) const fn width(self) -> usize {
        match self {
            Self::Uint8 | Self::Uint8Clamped | Self::Int8 => size_of::<u8>(),
            Self::Uint16 | Self::Int16 | Self::Float16 => size_of::<u16>(),
            Self::Uint32 | Self::Int32 | Self::Float32 => size_of::<u32>(),
            Self::BigInt64 | Self::BigUint64 | Self::Float64 => size_of::<u64>(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeEnvironmentLayout {
    pub(crate) header_pointer_offset: i32,
    pub(crate) sparse_mark: u64,
    pub(crate) slots_offset: u64,
    pub(crate) slot_size: u64,
}

pub(crate) const NATIVE_ENVIRONMENT_LAYOUT: NativeEnvironmentLayout = NativeEnvironmentLayout {
    header_pointer_offset: 0,
    sparse_mark: 9223372036854775808,
    slots_offset: 8,
    slot_size: 8,
};

#[derive(Debug)]
pub(crate) struct NativeFieldCacheLayout {
    pub(crate) entry_size: usize,
    pub(crate) receiver_offset: usize,
    pub(crate) holder_offset: usize,
    pub(crate) slot_offset: usize,
    pub(crate) receiver_complement: u32,
    pub(crate) holder_complement: u32,
    pub(crate) slot_complement: u32,
    pub(crate) own_holder: u32,
}

pub(crate) const NATIVE_FIELD_CACHE_LAYOUT: NativeFieldCacheLayout = NativeFieldCacheLayout {
    entry_size: 12,
    receiver_offset: 0,
    holder_offset: 4,
    slot_offset: 8,
    receiver_complement: 4294967295,
    holder_complement: 4294967295,
    slot_complement: 0,
    own_holder: 4294967295,
};

pub(crate) const OTHERS_UNKNOWN: u32 = 4294967295;
/// A string id whose bits under the mask are the base is small: it names no string record.
pub(crate) const SMALL_STRING_ID_MASK: u32 = 4278190080;
pub(crate) const SMALL_STRING_ID_BASE: u32 = 16777216;
pub(crate) const NARROW_LEN: u32 = 2147483648;
pub(crate) const DOUBLE_LEN: u32 = 1073741824;
pub(crate) const ENCODED_LEN: u32 = 3221225472;
pub(crate) const LENGTH_FROM_VALUES: i32 = -12;
pub(crate) const OTHERS_FROM_VALUES: i32 = -4;
pub(crate) const INLINE_PROPERTIES: usize = 2;

/// `ValueLayout`, `Value` and `NarrowLayout` constants.
pub(crate) mod words {
    pub(crate) const TAG_MASK: u64 = 0xffff000000000000;
    pub(crate) const INT_TAG: u64 = 0x7ffd000000000000;
    pub(crate) const INTERNAL_TAG: u64 = 0x7ffe000000000000;
    pub(crate) const HEAP_TAG: u64 = 0x7ffc000000000000;
    pub(crate) const DIRECT_ARRAY_TAG: u64 = 0x7ffc400000000000;
    pub(crate) const DIRECT_OBJECT_TAG: u64 = 0x7ffc800000000000;
    pub(crate) const DIRECT_FUNCTION_TAG: u64 = 0x7ffcc00000000000;
    pub(crate) const HEAP_FAMILY_MASK: u64 = 0xfffff00000000000;
    pub(crate) const DIRECT_STRING_TAG: u64 = 0x7ffce00000000000;
    pub(crate) const TAG_SHIFT: u32 = 48;
    pub(crate) const FIRST_TAG_FIELD: u64 = 0x7ff9;
    pub(crate) const TAG_FIELD_SPAN: u64 = 0x5;
    pub(crate) const CANONICAL_NAN: u64 = 0x7ff8000000000000;
    pub(crate) const NULL: u64 = 0x7ffa000000000000;
    pub(crate) const UNDEFINED: u64 = 0x7ff9000000000000;
    pub(crate) const DELETED: u64 = 0x7ffe000000000000;
    pub(crate) const FALSE: u64 = 0x7ffb000000000000;
    pub(crate) const TRUE: u64 = FALSE | 1;
    pub(crate) const NARROW_INT_FLAG: u32 = 1;
    pub(crate) const NARROW_INTEGER_BITS: u32 = 30;
    pub(crate) const NARROW_CODE_SHIFT: u32 = 1;
    pub(crate) const NARROW_CODE_MASK: u32 = 0xf;
    pub(crate) const NARROW_INDEX_SHIFT: u32 = 5;
    pub(crate) const NARROW_INDEX_BITS: u32 = 25;
    pub(crate) const DIRECT_OBJECT_UPPER: u64 = 0x7ffc800000000000;
    pub(crate) const DIRECT_OBJECT_NARROW_CODE: u32 = 4;
    pub(crate) const LOWER_WORD: u64 = 0xffff_ffff;
    pub(crate) const HEAP_FAMILY_BITS: u64 = 0x0000_e000_0000_0000;
    pub(crate) const HEAP_FAMILY_SHIFT: u32 = 45;
    pub(crate) const NARROW_HEAP_FAMILIES: usize = 8;
}

/// Byte offsets of the fields of the compiled-code context (`CallContext`) and the dispatch
/// payload (`JsStepDispatch`, behind the context's `adapter_payload`) the rows read.
pub(crate) const CALL_CONTEXT_ADAPTER_PAYLOAD: usize = 240;
/// The lane-call protocol, shared with the host (`quench_bjit::call_record`).
#[path = "../src/call_record.rs"]
pub(crate) mod call_record;
/// The words shared with the host (`quench_bjit::shared_words`).
#[path = "../src/shared_words.rs"]
pub(crate) mod shared_words;
