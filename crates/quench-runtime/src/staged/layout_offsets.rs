// The stencil layout numbers: the runtime's heap, value, frame and dispatch offsets as the staged
// rows read them. ONE source. The stencil crate `#[path]`-includes this file (`stencils/layout.rs`
// re-exports it), and the runtime includes it as `crate::staged::layout_offsets`. The `JS_STEP_*`
// offsets are `offset_of!` of the shared `#[repr(C)]` head `staged/js_step_head.rs`, which the
// runtime embeds as the first field of `JsStepDispatch`. The native records' offsets are `offset_of!`
// of the heads in `staged/record_heads.rs` (`ObjectHead`, `ArrayHeaderHead`, `NativeViewHead`), which
// the runtime embeds as the first field of `Object`, `Header` and `NativeView`; the shared length and
// mark encodings are `staged/encodings.rs`. Values not yet derived are literals, because a stencil is
// compiled before the runtime; `vm/compiled/base_rows.rs` holds `const` checks that each equals the
// runtime's own `offset_of!` / `size_of` / const, so a drift is a build error there.
//
// A stencil names nothing in the runtime except the frame words of `staged/frame.rs` and the entry
// cell of `staged/call.rs`, which both builds compile.
#![allow(dead_code)]

pub(crate) use crate::staged::frame::{ACTIVATION_BYTES, ACTIVATION_KIND_OFFSET, CONTEXT_WORDS as FRAME_CONTEXT_WORDS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeObjectLayout {
    pub(crate) record_capacity: usize,
    pub(crate) record_size: usize,
    pub(crate) record_align: usize,
    pub(crate) layout_offset: usize,
    pub(crate) meta_offset: usize,
    pub(crate) auxiliary_offset: usize,
    pub(crate) payload_offset: usize,
    pub(crate) extras_arena_start_offset: usize,
    pub(crate) extras_inline_values_offset: usize,
    pub(crate) inline_storage_mask: u32,
    pub(crate) array_mask: u32,
    pub(crate) storage_start_mask: u32,
    pub(crate) dictionary_mask: u32,
    pub(crate) inline_form: u32,
    pub(crate) extras_form: u32,
    pub(crate) narrow_form: u32,
    pub(crate) narrow_run_tag: u64,
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
    record_size: size_of::<super::record_heads::ObjectHead>(),
    record_align: align_of::<super::record_heads::ObjectHead>(),
    layout_offset: core::mem::offset_of!(super::record_heads::ObjectHead, layout),
    meta_offset: 4,
    auxiliary_offset: 0,
    payload_offset: core::mem::offset_of!(super::record_heads::ObjectHead, payload),
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

pub(crate) const NATIVE_ARRAY_LAYOUT: NativeArrayLayout = NativeArrayLayout {
    record_capacity: 16777216,
    record_size: size_of::<super::record_heads::ObjectHead>(),
    record_align: align_of::<super::record_heads::ObjectHead>(),
    elements_offset: core::mem::offset_of!(super::record_heads::ObjectHead, payload),
    extras_elements_offset: 28,
    elements_header_offset: 0,
    header_length_offset: core::mem::offset_of!(super::record_heads::ArrayHeaderHead, len),
    header_values_offset: size_of::<super::record_heads::ArrayHeaderHead>(),
    header_owners_offset: core::mem::offset_of!(super::record_heads::ArrayHeaderHead, owners),
    header_capacity_offset: core::mem::offset_of!(super::record_heads::ArrayHeaderHead, capacity),
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
    entry_size: size_of::<usize>(),
    page_shift: 12,
    page_mask: 4095,
    kind_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, kind),
    detached_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, detached),
    unshared_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, unshared),
    data_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, data),
    length_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, length),
    stamp_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, stamp),
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
    sparse_mark: super::encodings::MARK_BIT as u64,
    slots_offset: size_of::<usize>() as u64,
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

pub(crate) use super::encodings::{DOUBLE_LEN, ENCODED_LEN, NARROW_LEN, OTHERS_UNKNOWN};
/// A string id whose bits under the mask are the base is small: it names no string record.
pub(crate) const SMALL_STRING_ID_MASK: u32 = 4278190080;
pub(crate) const SMALL_STRING_ID_BASE: u32 = 16777216;
/// The byte offsets from a backing's first value to its `len` and `others` words: the head
/// `ArrayHeaderHead` is the backing's header, and its values follow it.
pub(crate) const LENGTH_FROM_VALUES: i32 = core::mem::offset_of!(super::record_heads::ArrayHeaderHead, len) as i32 - size_of::<super::record_heads::ArrayHeaderHead>() as i32;
pub(crate) const OTHERS_FROM_VALUES: i32 = core::mem::offset_of!(super::record_heads::ArrayHeaderHead, others) as i32 - size_of::<super::record_heads::ArrayHeaderHead>() as i32;
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
    pub(crate) const DIRECT_PROMISE_TAG: u64 = 0x7ffc200000000000;
    pub(crate) const DIRECT_ITERATOR_TAG: u64 = 0x7ffca00000000000;
    pub(crate) const DIRECT_COLLECTION_TAG: u64 = 0x7ffcb00000000000;
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
pub(crate) const JS_STEP_LOOP_ENTRY: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, loop_entry);
pub(crate) const JS_STEP_ARRAY_RECORD_BASE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, array_record_base);
/// The storage word of the environment window the running activation holds its environment-owned locals in
/// (`JsStepDispatch::environment_storage`), which the environment rows read their slots through
/// (`staged::environment::environment_slot`).
pub(crate) const JS_STEP_ENV_STORAGE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, environment_storage);
/// The activation's local slots and their count (`native_locals`, `local_count`).
pub(crate) const JS_STEP_NATIVE_LOCALS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, native_locals);
pub(crate) const JS_STEP_LOCAL_COUNT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, local_count);
/// The heap bases a field read through a site's cache entry reads: the object record
/// reservation, the allocator header of the property values (whose base is at
/// `PROPERTY_BUFFER_BASE`) and the field cache entries of the running program.
pub(crate) const JS_STEP_OBJECT_RECORD_BASE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, object_record_base);
pub(crate) const JS_STEP_PROPERTY_BUFFER: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, property_buffer);
/// What a store's barrier reads (`Stage::store_gate`): the object records' mark blocks and their
/// count, and whether the property arena records its slots' value classes (`slots_tracked`), which
/// an unobserved store requires clear.
pub(crate) const JS_STEP_OBJECT_MARK_BLOCKS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, object_mark_blocks);
pub(crate) const JS_STEP_OBJECT_MARK_BLOCK_COUNT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, object_mark_block_count);
pub(crate) const JS_STEP_SLOTS_TRACKED: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, slots_tracked);
/// The heap's collection epoch as of the last refresh (`JsStepDispatch::epoch`), which a for-in step
/// compares to the epoch its state's snapshot was listed in (`staged::iteration::for_in_next`).
pub(crate) const JS_STEP_EPOCH: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, epoch);
/// The string record range's base, which a string `length` read finds the record from.
pub(crate) const JS_STEP_STRING_RECORD_BASE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, string_record_base);
/// What a store into a function's own properties reads (`StagedRecordSpace::Function`): the function
/// records' mark blocks and their count, as the object records' (`JS_STEP_OBJECT_MARK_BLOCKS`); and the
/// empty string's word, which a function's inferred name replaces (`staged::naming`).
pub(crate) const JS_STEP_FUNCTION_MARK_BLOCKS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, function_mark_blocks);
pub(crate) const JS_STEP_FUNCTION_MARK_BLOCK_COUNT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, function_mark_block_count);
pub(crate) const JS_STEP_EMPTY_STRING: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, empty_string);
/// The function record reservation's base (`JsStepDispatch::function_record_base`).
pub(crate) const JS_STEP_FUNCTION_RECORD_BASE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, function_record_base);
pub(crate) const JS_STEP_FIELD_ENTRIES: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, field_entries);
pub(crate) const PROPERTY_BUFFER_BASE: usize = 0;
/// Bits of a body's capture word that hold the dense slots' address; the slot count sits above
/// them (`entry_cell::CAPTURE_ADDRESS_BITS`), and the pad is what a shift pair clears them with.
pub(crate) const CAPTURE_ADDRESS_PAD: u32 = 16;
/// The record a completed body names (`LANE_BODY_RETURNED`).
pub(crate) const JS_STEP_RETURNED_PC: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, returned_pc);
/// More `JsStepDispatch` fields the call rows read: the lane body's VM, residual and lane
/// context (a helper call's first three arguments), the lane stack's free span and the shadow
/// frames' span. Checked against the runtime by `stencil_layout_matches`.
pub(crate) const JS_STEP_VM: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, vm);
pub(crate) const JS_STEP_RESIDUAL: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, residual);
pub(crate) const JS_STEP_LANE_CONTEXT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, lane_context);
/// The lane stack's words (`Vm::lane_stack_words`): the addresses of the values' and the activations' tops,
/// the end of the values' room and the end of the frames' room.
pub(crate) const JS_STEP_LANE_VALUES_TOP: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, lane_values_top);
pub(crate) const JS_STEP_LANE_VALUES_END: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, lane_values_end);
pub(crate) const JS_STEP_LANE_ACTIVATIONS_TOP: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, lane_activations_top);
pub(crate) const JS_STEP_LANE_FRAMES_END: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, lane_frames_end);
/// The entry cell's run counters, and the unit a raised run adds (`entry_cell::RAISED_RUN`).
pub(crate) const CELL_COUNTERS_AT: usize = crate::staged::call::ENTRY_CELL.counters_offset as usize;
pub(crate) const RAISED_RUN: u64 = 1 << 32;

/// The running activation's receiver and body-capture words live in its window's header
/// (`staged::frame::CONTEXT_THIS`, `CONTEXT_BODY`); the stencils read them at `context_word(frame, ..)`.
pub(crate) const CONTEXT_THIS: usize = crate::staged::frame::CONTEXT_THIS;
pub(crate) const CONTEXT_BODY: usize = crate::staged::frame::CONTEXT_BODY;

/// The entry cell's entry, recovery and body words (`EntryCell`), and the recovery space's
/// values and activations (`LaneRecoverySpace`).
pub(crate) const CELL_ENTRY_AT: usize = crate::staged::call::ENTRY_CELL.entry_offset as usize;
pub(crate) const CELL_RECOVERY_AT: usize = crate::staged::call::ENTRY_CELL.recovery_offset as usize;
pub(crate) const CELL_BODY_AT: usize = crate::staged::call::ENTRY_CELL.body_offset as usize;
pub(crate) const CELL_ROWS_AT: usize = crate::staged::call::ENTRY_CELL.rows_offset as usize;
pub(crate) const RECOVERY_VALUES_AT: usize = 0;
pub(crate) const RECOVERY_ACTIVATIONS_AT: usize = 8;

/// The flat string forms a string record is read in (`vm::wtf16::native_string`): the record's
/// size (three words), where it keeps its form's tag, each form's tag, and where a short and a
/// shared form keep their unit count. Checked against the runtime by `stencil_layout_matches`.
pub(crate) mod string_form {
    pub(crate) const RECORD_SIZE: usize = 3 * core::mem::size_of::<usize>();
    /// A short record's length byte follows its tag byte.
    pub(crate) const SHORT_LENGTH_OFFSET: i32 = 1;
    /// A shared record's pointer follows its tag at the next word, then its length.
    pub(crate) const SHARED_LENGTH_OFFSET: i32 = 2 * core::mem::size_of::<usize>() as i32;
    /// The units of a short record follow its length byte.
    pub(crate) const SHORT_UNITS_OFFSET: u64 = 2;
    /// A shared record's pointer follows its tag at the next word.
    pub(crate) const SHARED_POINTER_OFFSET: i32 = core::mem::size_of::<usize>() as i32;
    /// The reference counts (strong and weak) an `Rc` allocation holds before its value.
    const RC_COUNTS: u64 = 2;
    /// How far into the shared allocation the units start: past the reference counts of an `Rc`.
    pub(crate) const fn shared_units_offset() -> u64 {
        RC_COUNTS * core::mem::size_of::<usize>() as u64
    }
}
/// What an inline-cache row reads of the dispatch payload (`stencils/ic_rows.rs`): the method-cache atom bits and their
/// word count, which a store row consults, and whether stores are specialized.
pub(crate) const JS_STEP_METHOD_CACHE_ATOMS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, method_cache_atoms);
pub(crate) const JS_STEP_METHOD_CACHE_ATOM_WORDS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, method_cache_atom_words);
pub(crate) const JS_STEP_STORES_SPECIALIZED: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, stores_specialized);
/// The object layout table's fixed address (`heap::layouts::table_base`) on x86-64 Linux, the
/// stencils' target: a row reads an entry with the address as its displacement.
pub(crate) const LAYOUT_TABLE_BASE: u64 = 0x7000_0000;

/// The value layout's constants, as the rows name them (`ValueLayout`, `NarrowLayout`).
pub(crate) struct ValueLayout;
impl ValueLayout {
    pub(crate) const TAG_MASK: u64 = words::TAG_MASK;
    pub(crate) const INT_TAG: u64 = words::INT_TAG;
    pub(crate) const INTERNAL_TAG: u64 = words::INTERNAL_TAG;
    pub(crate) const HEAP_TAG: u64 = words::HEAP_TAG;
    pub(crate) const DIRECT_ARRAY_TAG: u64 = words::DIRECT_ARRAY_TAG;
    pub(crate) const DIRECT_OBJECT_TAG: u64 = words::DIRECT_OBJECT_TAG;
    pub(crate) const DIRECT_FUNCTION_TAG: u64 = words::DIRECT_FUNCTION_TAG;
    pub(crate) const DIRECT_PROMISE_TAG: u64 = words::DIRECT_PROMISE_TAG;
    pub(crate) const DIRECT_ITERATOR_TAG: u64 = words::DIRECT_ITERATOR_TAG;
    pub(crate) const DIRECT_COLLECTION_TAG: u64 = words::DIRECT_COLLECTION_TAG;
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
}
