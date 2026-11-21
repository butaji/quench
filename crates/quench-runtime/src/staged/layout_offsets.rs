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
// The re-exports are the stencils' names (`crate::layout::*`), which the runtime does not read.
#![allow(dead_code, unused_imports)]

pub(crate) use crate::staged::frame::{ACTIVATION_BYTES, ACTIVATION_KIND_OFFSET, CONTEXT_WORDS as FRAME_CONTEXT_WORDS};
use super::object_encoding::{
    DICTIONARY_STORAGE, EXTRAS_FORM, EXTRAS_TAG, EXTRAS_TAG_MASK, HEADER_CAPACITY_MASK, INLINE_FORM,
    INLINE_PROPERTY_COUNT, META_OFFSET, NARROW_FORM, NARROW_RUN_TAG, OBJECT_ARRAY, OBJECT_INLINE_STORAGE,
    OCCUPANCY_WORD_BITS, PROTO_ARRAY_RECORD_BASE, PROTO_INDEX_BITS, PROTO_OBJECT_RECORD_BASE,
    OBJECT_RECORD_CAPACITY, ARRAY_RECORD_CAPACITY, RECORDS_PER_BLOCK, RecordBlock, START_MASK, VALUE_VEC_FROZEN,
    VALUE_VEC_INTEGRITY_BITS, WASM_GC_RECORD_CAPACITY,
};
use super::object_extras::ObjectExtrasWords;

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
    record_capacity: OBJECT_RECORD_CAPACITY,
    record_size: size_of::<super::record_heads::ObjectHead>(),
    record_align: align_of::<super::record_heads::ObjectHead>(),
    layout_offset: core::mem::offset_of!(super::record_heads::ObjectHead, layout),
    meta_offset: META_OFFSET,
    auxiliary_offset: 0,
    payload_offset: core::mem::offset_of!(super::record_heads::ObjectHead, payload),
    extras_arena_start_offset: core::mem::offset_of!(ObjectExtrasWords, arena_start),
    extras_inline_values_offset: core::mem::offset_of!(ObjectExtrasWords, inline_properties),
    inline_storage_mask: OBJECT_INLINE_STORAGE,
    array_mask: OBJECT_ARRAY,
    storage_start_mask: START_MASK,
    dictionary_mask: DICTIONARY_STORAGE,
    inline_form: INLINE_FORM,
    extras_form: EXTRAS_FORM,
    narrow_form: NARROW_FORM,
    narrow_run_tag: NARROW_RUN_TAG,
    integrity_mask: VALUE_VEC_INTEGRITY_BITS,
    inline_property_count: INLINE_PROPERTY_COUNT,
    extras_tag: EXTRAS_TAG,
    extras_tag_mask: EXTRAS_TAG_MASK,
    proto_mask: PROTO_INDEX_BITS,
    proto_record_base: PROTO_OBJECT_RECORD_BASE,
    proto_record_end: PROTO_ARRAY_RECORD_BASE,
    mark_records_per_block: RECORDS_PER_BLOCK,
    mark_word_bits: OCCUPANCY_WORD_BITS,
    mark_block_size: size_of::<RecordBlock>(),
    mark_offset: core::mem::offset_of!(RecordBlock, marked),
    property_value_size: size_of::<u64>(),
    property_value_align: align_of::<u64>(),
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeFunctionLayout {
    pub(crate) record_size: usize,
    pub(crate) object_offset: usize,
}

/// The function record as the stencils see it: the runtime's `FunctionRecord` with words of the same size and
/// alignment in its fields (`Object` is an `ObjectHead`; `FunctionKind` is 12 bytes, align 4; `Value` a `u64`).
/// `base_rows` checks each against the runtime type.
pub(crate) type FunctionRecordWords = super::record_heads::FunctionRecord<super::record_heads::ObjectHead, [u32; 3], u64>;

pub(crate) const NATIVE_FUNCTION_LAYOUT: NativeFunctionLayout = NativeFunctionLayout {
    record_size: size_of::<FunctionRecordWords>(),
    object_offset: core::mem::offset_of!(FunctionRecordWords, object),
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
    record_capacity: ARRAY_RECORD_CAPACITY,
    record_size: size_of::<super::record_heads::ObjectHead>(),
    record_align: align_of::<super::record_heads::ObjectHead>(),
    elements_offset: core::mem::offset_of!(super::record_heads::ObjectHead, payload),
    extras_elements_offset: core::mem::offset_of!(ObjectExtrasWords, elements),
    elements_header_offset: 0,
    header_length_offset: core::mem::offset_of!(super::record_heads::ArrayHeaderHead, len),
    header_values_offset: size_of::<super::record_heads::ArrayHeaderHead>(),
    header_owners_offset: core::mem::offset_of!(super::record_heads::ArrayHeaderHead, owners),
    header_capacity_offset: core::mem::offset_of!(super::record_heads::ArrayHeaderHead, capacity),
    capacity_mask: HEADER_CAPACITY_MASK,
    object_offset: 0,
    frozen_mask: VALUE_VEC_FROZEN,
    integrity_mask: VALUE_VEC_INTEGRITY_BITS,
};

/// A hot record space's allocation cursor (`heap::records::RecordCursor`): two `u32` words, `next` then
/// `limit`, which the empty-allocation rows bump (`heap_rows::empty_row`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeRecordCursorLayout {
    pub(crate) next_offset: usize,
    pub(crate) limit_offset: usize,
}

pub(crate) const NATIVE_RECORD_CURSOR_LAYOUT: NativeRecordCursorLayout = NativeRecordCursorLayout {
    next_offset: 0,
    limit_offset: size_of::<u32>(),
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
    page_shift: TYPED_PAGE_SHIFT,
    page_mask: (1 << TYPED_PAGE_SHIFT) - 1,
    kind_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, kind),
    detached_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, detached),
    unshared_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, unshared),
    data_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, data),
    length_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, length),
    stamp_offset: core::mem::offset_of!(super::record_heads::NativeViewHead, stamp),
};

/// The typed-array view table's page shift: a slot's page is `slot >> TYPED_PAGE_SHIFT`, its entry the
/// slot's low bits (`heap::typed_view`).
pub(crate) const TYPED_PAGE_SHIFT: u32 = 12;

/// Where a loop entry record keeps an array's element count, after the elements' address (`LoopEntry::push_array`).
pub(crate) const LOOP_ENTRY_ARRAY_COUNT_AT: u64 = size_of::<u64>() as u64;

/// The runtime's `TypedArrayKind` (`heap/cell/typed_array_catalog.rs`, mounted once for both builds).
pub(crate) use super::typed_array_catalog::TypedArrayKind;

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
    slot_size: size_of::<u64>() as u64,
};

/// A Wasm GC object's record (`record_heads::WasmGcHead`): where its payload's address, length and type
/// index sit, so compiled code reads a field's slot inline. The record's stride is the record space's
/// element size (`WasmGcRecord`, which the head starts), not the head's, so it is not named here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeWasmGcLayout {
    pub(crate) record_capacity: usize,
    pub(crate) payload_offset: usize,
    pub(crate) length_offset: usize,
    pub(crate) type_offset: usize,
}

pub(crate) const NATIVE_WASM_GC_LAYOUT: NativeWasmGcLayout = NativeWasmGcLayout {
    record_capacity: WASM_GC_RECORD_CAPACITY,
    payload_offset: core::mem::offset_of!(super::record_heads::WasmGcHead, payload),
    length_offset: core::mem::offset_of!(super::record_heads::WasmGcHead, length),
    type_offset: core::mem::offset_of!(super::record_heads::WasmGcHead, ty),
};

#[derive(Debug)]
pub(crate) struct NativeFieldCacheLayout {
    pub(crate) entry_size: usize,
    pub(crate) receiver_offset: usize,
    pub(crate) holder_offset: usize,
    pub(crate) slot_offset: usize,
    pub(crate) holder_id_offset: usize,
    pub(crate) receiver_complement: u32,
    pub(crate) holder_complement: u32,
    pub(crate) slot_complement: u32,
    pub(crate) own_holder: u32,
}

/// The field cache entry's layout, from its `repr(C)` words and their empty values (`field_cache_layout`).
pub(crate) const NATIVE_FIELD_CACHE_LAYOUT: NativeFieldCacheLayout = NativeFieldCacheLayout {
    entry_size: size_of::<super::field_cache_layout::FieldCacheWords>(),
    receiver_offset: core::mem::offset_of!(super::field_cache_layout::FieldCacheWords, receiver),
    holder_offset: core::mem::offset_of!(super::field_cache_layout::FieldCacheWords, holder),
    slot_offset: core::mem::offset_of!(super::field_cache_layout::FieldCacheWords, slot),
    holder_id_offset: core::mem::offset_of!(super::field_cache_layout::FieldCacheWords, holder_id),
    receiver_complement: super::field_cache_layout::EMPTY.receiver,
    holder_complement: super::field_cache_layout::EMPTY.holder,
    slot_complement: super::field_cache_layout::EMPTY.slot,
    own_holder: super::field_cache_layout::NO_HOLDER,
};

pub(crate) use super::encodings::{DOUBLE_LEN, ENCODED_LEN, NARROW_LEN, OTHERS_UNKNOWN};
/// The first id past the string records' own range: a small string's id is this base plus its bits.
pub(crate) const STRING_RECORD_HOLE: u32 = 1 << 24;
/// A string id whose bits under the mask are the base is small: it names no string record.
pub(crate) const SMALL_STRING_ID_MASK: u32 = !(STRING_RECORD_HOLE - 1);
pub(crate) const SMALL_STRING_ID_BASE: u32 = STRING_RECORD_HOLE;
/// The byte offsets from a backing's first value to its `len` and `others` words: the head
/// `ArrayHeaderHead` is the backing's header, and its values follow it.
pub(crate) const LENGTH_FROM_VALUES: i32 = core::mem::offset_of!(super::record_heads::ArrayHeaderHead, len) as i32 - size_of::<super::record_heads::ArrayHeaderHead>() as i32;
pub(crate) const OTHERS_FROM_VALUES: i32 = core::mem::offset_of!(super::record_heads::ArrayHeaderHead, others) as i32 - size_of::<super::record_heads::ArrayHeaderHead>() as i32;
pub(crate) const INLINE_PROPERTIES: usize = 2;

/// The value encoding's constants under the stencil's names (`words::*`), each the runtime's own
/// `staged::value_encoding` item. `ValueLayout` and `NarrowLayout` are that file's structs.
pub(crate) use super::value_encoding::{NarrowLayout, ValueLayout};
pub(crate) mod words {
    pub(crate) use super::super::value_encoding::{
        CANONICAL_NAN, DIRECT_ARRAY_TAG, DIRECT_FUNCTION_TAG, DIRECT_ITERATOR_TAG, DIRECT_OBJECT_NARROW_CODE, DIRECT_OBJECT_TAG,
        DIRECT_OBJECT_UPPER, DIRECT_PROMISE_TAG, DIRECT_STRING_TAG, FIRST_TAG_FIELD, HEAP_FAMILY_BITS, HEAP_FAMILY_MASK,
        HEAP_FAMILY_SHIFT, LOWER_WORD, NARROW_CODE_MASK, NARROW_CODE_SHIFT, NARROW_HEAP_FAMILIES, NARROW_INDEX_BITS,
        NARROW_INDEX_SHIFT, NARROW_INTEGER_BITS, TAG_FIELD_SPAN, TAG_SHIFT, TAG_TRUE as TRUE, TAG_MASK, TAG_BOOL as FALSE,
        TAG_DELETED as DELETED, TAG_DELETED as INTERNAL_TAG, TAG_HEAP as HEAP_TAG, TAG_INT as INT_TAG, TAG_NULL as NULL,
        TAG_UNDEFINED as UNDEFINED, NARROW_INTEGER_FLAG as NARROW_INT_FLAG,
    };
}

/// Byte offsets of the fields of the compiled-code context (`CallContext`) and the dispatch
/// payload (`JsStepDispatch`, behind the context's `adapter_payload`) the rows read.
/// The call context's ABI prefix as the stencils see it: the runtime's `CallContextHead` with the environment
/// and memory values as `u64`, the compiled artifact, table words and diagnostics as bytes (only their
/// pointers are read), the program id as `u32` and the raised completion as three words (an `Option<Raised>`,
/// 24 bytes). `base_rows` checks each against the runtime type.
pub(crate) type CallContextWords = super::record_heads::CallContextHead<u64, u8, u8, u8, u32, [usize; 3]>;
pub(crate) const CALL_CONTEXT_ADAPTER_PAYLOAD: usize = core::mem::offset_of!(CallContextWords, adapter_payload);
pub(crate) const JS_STEP_LOOP_ENTRY: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, loop_entry);
pub(crate) const JS_STEP_ARRAY_RECORD_BASE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, array_record_base);
/// The storage word of the environment window the running activation holds its environment-owned locals in
/// (`JsStepDispatch::environment_storage`), which the environment rows read their slots through
/// (`staged::environment::environment_slot`).
pub(crate) const JS_STEP_ENV_STORAGE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, environment_storage);
/// The empty allocations' cursor and template words (`JsStepHead::object_cursor` and after).
pub(crate) const JS_STEP_OBJECT_CURSOR: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, object_cursor);
pub(crate) const JS_STEP_EMPTY_OBJECT_LAYOUT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, empty_object_layout);
pub(crate) const JS_STEP_EMPTY_OBJECT_PAYLOAD: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, empty_object_payload);
pub(crate) const JS_STEP_EMPTY_OBJECT_PROTO: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, empty_object_proto);
pub(crate) const JS_STEP_ARRAY_CURSOR: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, array_cursor);
pub(crate) const JS_STEP_EMPTY_ARRAY_LAYOUT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, empty_array_layout);
pub(crate) const JS_STEP_EMPTY_ARRAY_PAYLOAD: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, empty_array_payload);
pub(crate) const JS_STEP_EMPTY_ARRAY_PROTO: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, empty_array_proto);
pub(crate) const JS_STEP_ASCII_UNIT_STRINGS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, ascii_unit_strings);
pub(crate) const JS_STEP_ARMED_ARRAY_PROOFS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, armed_array_proofs);
pub(crate) const JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, array_chain_index_free_address);
/// The activation's local slots and their count (`native_locals`, `local_count`).
pub(crate) const JS_STEP_NATIVE_LOCALS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, native_locals);
pub(crate) const JS_STEP_LOCAL_COUNT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, local_count);
/// The heap bases a field read through a site's cache entry reads: the object record
/// reservation, the allocator header of the property values (whose base is at
/// `PROPERTY_BUFFER_BASE`) and the field cache entries of the running program.
pub(crate) const JS_STEP_OBJECT_RECORD_BASE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, object_record_base);
pub(crate) const JS_STEP_PROPERTY_BUFFER: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, property_buffer);
/// What a store's barrier reads (`Stage::store_gate`): the object records' mark blocks and their
/// count, and what watches a store (`store_watch`), which an unobserved store requires zero.
pub(crate) const JS_STEP_OBJECT_MARK_BLOCKS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, object_mark_blocks);
pub(crate) const JS_STEP_OBJECT_MARK_BLOCK_COUNT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, object_mark_block_count);
pub(crate) const JS_STEP_STORE_WATCH: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, store_watch);
/// The `store_watch` bit set while the property arena records its slots' value classes.
pub(crate) const STORE_WATCH_TRACKS: u64 = 1;
/// The `store_watch` bit set while old records keep reference counts (`Heap::rc_active`): every
/// store into a marked record logs then, an immediate one too (`RecordSpace::store_logs`).
pub(crate) const STORE_WATCH_COUNTS: u64 = 2;
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
/// The base pointer's offset within a property buffer: the head is the buffer's first field (`PropertyBuffer::head`).
pub(crate) const PROPERTY_BUFFER_BASE: usize = core::mem::offset_of!(super::property_buffer::PropertyBufferHead<u8>, base);
/// Bits of a body's capture word that hold the dense slots' address; the slot count sits above
/// them (`entry_cell::CAPTURE_ADDRESS_BITS`), and the pad is what a shift pair clears them with.
pub(crate) const CAPTURE_ADDRESS_PAD: u32 = u64::BITS - super::shared_words::CAPTURE_ADDRESS_BITS;
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
/// The plain-frame words a native frame's header bumps (`PlainFrames`), and the body table each depth indexes.
/// (reconciled with u-jit F5 on merge)
pub(crate) const JS_STEP_PLAIN_DEPTH: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, plain_depth);
pub(crate) const PLAIN_DEPTH_LIMIT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, plain_depth_limit);
pub(crate) const JS_STEP_BODY_TABLE: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, body_table);
pub(crate) const JS_STEP_BODY_TOP: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, body_top);
pub(crate) const JS_STEP_ACTIVATION_COUNT: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, activation_count);
/// The entry cell's run counters, and the unit a raised run adds (`entry_cell::RAISED_RUN`). The entry cell's
/// offsets come from `staged::call`, which the Wasm rows' family does not compile (`stencil_family`).
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_COUNTERS_AT: usize = crate::staged::call::ENTRY_CELL.counters_offset as usize;
pub(crate) const RAISED_RUN: u64 = 1 << u32::BITS;

/// The running activation's receiver and body-capture words live in its window's header
/// (`staged::frame::CONTEXT_THIS`, `CONTEXT_BODY`); the stencils read them at `context_word(frame, ..)`.
pub(crate) const CONTEXT_THIS: usize = crate::staged::frame::CONTEXT_THIS;
pub(crate) const CONTEXT_BODY: usize = crate::staged::frame::CONTEXT_BODY;
/// The closure environment and captures pointer a linked activation's interpreter reads (`staged::frame::CONTEXT_ENV`, `CONTEXT_CAPTURE`).
pub(crate) const CONTEXT_ENV: usize = crate::staged::frame::CONTEXT_ENV;
pub(crate) const CONTEXT_CAPTURE: usize = crate::staged::frame::CONTEXT_CAPTURE;

/// The entry cell's entry, recovery and body words (`EntryCell`), and the recovery space's
/// values and activations (`LaneRecoverySpace`).
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_ENTRY_AT: usize = crate::staged::call::ENTRY_CELL.entry_offset as usize;
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_RECOVERY_AT: usize = crate::staged::call::ENTRY_CELL.recovery_offset as usize;
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_BODY_AT: usize = crate::staged::call::ENTRY_CELL.body_offset as usize;
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_ROWS_AT: usize = crate::staged::call::ENTRY_CELL.rows_offset as usize;
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_PROVEN_AT: usize = crate::staged::call::ENTRY_CELL.proven_offset as usize;
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_NUMBERS_AT: usize = crate::staged::call::ENTRY_CELL.numbers_offset as usize;
#[cfg(not(stencil_family = "wasm"))]
pub(crate) const CELL_USES_THIS_AT: usize = crate::staged::call::ENTRY_CELL.uses_this_offset as usize;
pub(crate) const RECOVERY_VALUES_AT: usize = 0;
pub(crate) const RECOVERY_ACTIVATIONS_AT: usize = 8;

/// The flat string forms a string record is read in (`vm::wtf16::native_string`): the record's
/// size (three words), where it keeps its form's tag, each form's tag, and where a short and a
/// shared form keep their unit count. Checked against the runtime by `stencil_layout_matches`.
pub(crate) use super::string_form;
/// What an inline-cache row reads of the dispatch payload (`stencils/ic_rows.rs`): the method-cache atom bits and their
/// word count, which a store row consults, and whether stores are specialized.
pub(crate) const JS_STEP_METHOD_CACHE_ATOMS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, method_cache_atoms);
pub(crate) const JS_STEP_METHOD_CACHE_ATOM_WORDS: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, method_cache_atom_words);
pub(crate) const JS_STEP_STORES_SPECIALIZED: usize = core::mem::offset_of!(super::js_step_head::JsStepHead<u8>, stores_specialized);
/// The object layout table's fixed address (`heap::layouts::table_base`): a row reads an entry
/// with the address as its displacement. On x86-64 Linux a low address is in a sign-extended
/// 32-bit displacement's reach; Apple AArch64 reserves the low 4 GiB (the image sits at 4 GiB),
/// so the table is mapped at 8 GiB there (`MAP_FIXED` of a 32 MiB range).
#[cfg(not(all(target_arch = "aarch64", target_vendor = "apple")))]
pub(crate) const LAYOUT_TABLE_BASE: u64 = 0x7000_0000;
#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
pub(crate) const LAYOUT_TABLE_BASE: u64 = 0x2_0000_0000;
