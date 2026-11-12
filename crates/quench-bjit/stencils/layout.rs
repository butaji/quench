// The numbers the runtime's heap and value layouts fix, as the staged rows read them. A
// stencil is compiled before the runtime, so it cannot read `offset_of!` of the real records:
// these are the same facts as literals, and the runtime's `stencil_layout_matches` test fails
// when any differs (it compares the `Debug` of each struct and every constant with the
// runtime's own).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeObjectLayout {
    pub(crate) record_capacity: usize,
    pub(crate) record_size: usize,
    pub(crate) record_align: usize,
    pub(crate) meta_offset: usize,
    pub(crate) auxiliary_offset: usize,
    pub(crate) payload_offset: usize,
    pub(crate) extras_arena_start_offset: usize,
    pub(crate) extras_inline_values_offset: usize,
    pub(crate) inline_storage_mask: u32,
    pub(crate) array_mask: u32,
    pub(crate) storage_start_mask: u32,
    pub(crate) dictionary_mask: u32,
    pub(crate) inline_property_count: usize,
    pub(crate) extras_tag: u64,
    pub(crate) extras_tag_mask: u64,
    pub(crate) proto_mask: u32,
    pub(crate) proto_record_base: u32,
    pub(crate) proto_record_end: u32,
    pub(crate) mark_records_per_block: usize,
    pub(crate) mark_word_bits: usize,
    pub(crate) mark_block_size: usize,
    pub(crate) mark_offset: usize,
    pub(crate) property_value_size: usize,
    pub(crate) property_value_align: usize,
}

pub(crate) const NATIVE_OBJECT_LAYOUT: NativeObjectLayout = NativeObjectLayout {
    record_capacity: 16777216,
    record_size: 16,
    record_align: 8,
    meta_offset: 8,
    auxiliary_offset: 12,
    payload_offset: 0,
    extras_arena_start_offset: 24,
    extras_inline_values_offset: 8,
    inline_storage_mask: 536870912,
    array_mask: 268435456,
    storage_start_mask: 1073741823,
    dictionary_mask: 2147483648,
    inline_property_count: 2,
    extras_tag: 9223090561878065152,
    extras_tag_mask: 18446462598732840960,
    proto_mask: 268435455,
    proto_record_base: 134217728,
    proto_record_end: 150994944,
    mark_records_per_block: 1024,
    mark_word_bits: 64,
    mark_block_size: 512,
    mark_offset: 128,
    property_value_size: 8,
    property_value_align: 8,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeArrayLayout {
    pub(crate) record_capacity: usize,
    pub(crate) record_size: usize,
    pub(crate) record_align: usize,
    pub(crate) elements_offset: usize,
    pub(crate) elements_header_offset: usize,
    pub(crate) header_length_offset: usize,
    pub(crate) header_values_offset: usize,
    pub(crate) header_owners_offset: usize,
    pub(crate) object_offset: usize,
    pub(crate) frozen_mask: u32,
}

pub(crate) const NATIVE_ARRAY_LAYOUT: NativeArrayLayout = NativeArrayLayout {
    record_capacity: 16777216,
    record_size: 24,
    record_align: 8,
    elements_offset: 16,
    elements_header_offset: 0,
    header_length_offset: 4,
    header_values_offset: 16,
    header_owners_offset: 0,
    object_offset: 0,
    frozen_mask: 2147483648,
};

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
    pub(crate) const NARROW_INT_FLAG: u32 = 1;
    pub(crate) const NARROW_INTEGER_BITS: u32 = 30;
    pub(crate) const NARROW_CODE_SHIFT: u32 = 1;
    pub(crate) const NARROW_CODE_MASK: u32 = 0xf;
    pub(crate) const NARROW_INDEX_SHIFT: u32 = 5;
    pub(crate) const NARROW_INDEX_BITS: u32 = 25;
    pub(crate) const DIRECT_OBJECT_UPPER: u64 = 0x7ffc800000000000;
    pub(crate) const DIRECT_OBJECT_NARROW_CODE: u32 = 4;
}

/// Byte offsets of the fields of the compiled-code context (`CallContext`) and the dispatch
/// payload (`JsStepDispatch`, behind the context's `adapter_payload`) the rows read.
pub(crate) const CALL_CONTEXT_ADAPTER_PAYLOAD: usize = 248;
pub(crate) const JS_STEP_LOOP_ENTRY: usize = 0;
pub(crate) const JS_STEP_CAPTURE_VALUES: usize = 96;
pub(crate) const JS_STEP_CAPTURE_LEN: usize = 104;
pub(crate) const JS_STEP_ARRAY_RECORD_BASE: usize = 112;
