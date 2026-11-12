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
    pub(crate) integrity_mask: u32,
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
    integrity_mask: 3221225472,
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
    pub(crate) header_capacity_offset: usize,
    pub(crate) capacity_mask: u32,
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
    header_capacity_offset: 8,
    capacity_mask: 2147483647,
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
pub(crate) const CALL_CONTEXT_ADAPTER_PAYLOAD: usize = 248;
pub(crate) const JS_STEP_LOOP_ENTRY: usize = 0;
pub(crate) const JS_STEP_CAPTURE_VALUES: usize = 96;
pub(crate) const JS_STEP_CAPTURE_LEN: usize = 104;
pub(crate) const JS_STEP_ARRAY_RECORD_BASE: usize = 112;
/// Whether the capture slots admit every direct store, which a store row requires
/// (`environment::dense_capture_store`); the bit of a body's capture word that says so
/// (`dispatch_fast::CAPTURE_STORES`) moves here at its entry.
pub(crate) const JS_STEP_CAPTURE_STORES: usize = 360;
/// The running activation's receiver (`this`) word, `DELETED` where it has none: the entry of a
/// lane body or loop region publishes the one it is entered with, for its run.
pub(crate) const JS_STEP_RECEIVER: usize = 200;
pub(crate) const CAPTURE_STORES_BIT: u64 = 1;
/// The activation's local slots and their count (`native_locals`, `local_count`).
pub(crate) const JS_STEP_NATIVE_LOCALS: usize = 56;
pub(crate) const JS_STEP_LOCAL_COUNT: usize = 64;
/// The heap bases a field read through a site's cache entry reads: the object record
/// reservation, the allocator header of the property values (whose base is at
/// `PROPERTY_BUFFER_BASE`) and the field cache entries of the running program.
pub(crate) const JS_STEP_OBJECT_RECORD_BASE: usize = 120;
pub(crate) const JS_STEP_PROPERTY_BUFFER: usize = 152;
/// What a store's barrier reads (`Stage::store_gate`): the object records' mark blocks and their
/// count, and whether the property arena records its slots' value classes (`slots_tracked`), which
/// an unobserved store requires clear.
pub(crate) const JS_STEP_OBJECT_MARK_BLOCKS: usize = 136;
pub(crate) const JS_STEP_OBJECT_MARK_BLOCK_COUNT: usize = 144;
pub(crate) const JS_STEP_SLOTS_TRACKED: usize = 528;
/// The string record range's base, which a string `length` read finds the record from.
pub(crate) const JS_STEP_STRING_RECORD_BASE: usize = JS_STEP_SLOTS_TRACKED + core::mem::size_of::<usize>();
pub(crate) const JS_STEP_FIELD_ENTRIES: usize = 160;
pub(crate) const PROPERTY_BUFFER_BASE: usize = 0;
/// Bits of a body's capture word that hold the dense slots' address; the slot count sits above
/// them (`entry_cell::CAPTURE_ADDRESS_BITS`), and the pad is what a shift pair clears them with.
pub(crate) const CAPTURE_ADDRESS_BITS: u32 = 48;
pub(crate) const CAPTURE_ADDRESS_PAD: u32 = u64::BITS - CAPTURE_ADDRESS_BITS;
/// The record a completed body names (`LANE_BODY_RETURNED`).
pub(crate) const JS_STEP_RETURNED_PC: usize = 256;

/// More `JsStepDispatch` fields the call rows read: the lane body's VM, residual and lane
/// context (a helper call's first three arguments), the lane stack's free span and the shadow
/// frames' span. Checked against the runtime by `stencil_layout_matches`.
pub(crate) const JS_STEP_VM: usize = 208;
pub(crate) const JS_STEP_RESIDUAL: usize = 216;
pub(crate) const JS_STEP_LANE_CONTEXT: usize = 248;
pub(crate) const JS_STEP_NATIVE_TOP: usize = 264;
pub(crate) const JS_STEP_NATIVE_END: usize = 272;
pub(crate) const JS_STEP_SHADOW_TOP: usize = 280;
pub(crate) const JS_STEP_SHADOW_END: usize = 288;

/// The entry cell's entry, recovery and body words (`EntryCell`), and the recovery space's
/// values and activations (`LaneRecoverySpace`).
pub(crate) const CELL_ENTRY_AT: usize = 0;
pub(crate) const CELL_RECOVERY_AT: usize = 8;
pub(crate) const CELL_BODY_AT: usize = 16;
pub(crate) const RECOVERY_VALUES_AT: usize = 0;
pub(crate) const RECOVERY_ACTIVATIONS_AT: usize = 8;

/// The flat string forms a string record is read in (`vm::wtf16::native_string`): the record's
/// size (three words), where it keeps its form's tag, each form's tag, and where a short and a
/// shared form keep their unit count. Checked against the runtime by `stencil_layout_matches`.
pub(crate) mod string_form {
    pub(crate) const RECORD_SIZE: usize = 3 * core::mem::size_of::<usize>();
    pub(crate) const TAG_OFFSET: i32 = 0;
    pub(crate) const TAG_SHORT: u8 = 0;
    pub(crate) const TAG_ASCII: u8 = 1;
    pub(crate) const TAG_WIDE: u8 = 3;
    /// A short record's length byte follows its tag byte.
    pub(crate) const SHORT_LENGTH_OFFSET: i32 = 1;
    /// A shared record's pointer follows its tag at the next word, then its length.
    pub(crate) const SHARED_LENGTH_OFFSET: i32 = 2 * core::mem::size_of::<usize>() as i32;
}
