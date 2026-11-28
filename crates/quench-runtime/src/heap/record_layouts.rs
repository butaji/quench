//! The native readers' views of the fixed record layouts: where an array, function, object or environment
//! record keeps the words generated code reads, each derived from the storage types themselves.
use super::{array_elements, cell, records, thin_slice};
use crate::value::Value;
use crate::value_vec::INLINE_PROPERTY_COUNT;

/// The native reader's view of an ordinary array's fixed record and dense backing.
///
/// Every field is derived from the storage types below; this is a projection of the heap's
/// representation, not a second copy of its length or element state. A consumer resolves the
/// current live array record from the current rooted `Value`, then loads its current
/// `ArrayElements` header and backing on every access. It must not retain a record or backing
/// pointer across a VM safepoint: record addresses are stable for the heap lifetime, but a
/// copy-on-write array mutation can replace and free its backing. The read itself must not
/// call into the VM or permit collection while the borrowed raw pointers are in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeArrayLayout {
    pub record_capacity: usize,
    pub record_size: usize,
    pub record_align: usize,
    /// Where an array record keeps its backing's header pointer without extras: its
    /// object's payload word. With extras, that word points at them (`NATIVE_OBJECT_LAYOUT`'s
    /// extras tag), and they keep the pointer at `extras_elements_offset`.
    pub elements_offset: usize,
    pub extras_elements_offset: usize,
    pub elements_header_offset: usize,
    pub header_length_offset: usize,
    pub header_values_offset: usize,
    /// The backing's owner count, one where no other array shares it.
    pub header_owners_offset: usize,
    /// The backing's capacity word: the values it has room for, under `capacity_mask`.
    pub header_capacity_offset: usize,
    pub capacity_mask: u32,
    /// The record's object part, whose meta and payload words `NATIVE_OBJECT_LAYOUT` names.
    pub object_offset: usize,
    /// The meta bit of a frozen array.
    pub frozen_mask: u32,
    /// The meta bits of an array whose elements may not all be added or removed: frozen, or
    /// not extensible.
    pub integrity_mask: u32,
}

/// The native reader's view of a function record: its own properties are the `Object` at
/// `object_offset`, whose words `NATIVE_OBJECT_LAYOUT` names. The record is `repr(C)`, so the layout
/// is the declaration's. Generated code reads and, through the function space's barrier
/// (`StagedRecordSpace::Function`, `emit_unlogged_property_store_guard`), stores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeFunctionLayout {
    pub record_size: usize,
    pub object_offset: usize,
}

pub const NATIVE_FUNCTION_LAYOUT: NativeFunctionLayout = NativeFunctionLayout {
    record_size: cell::NATIVE_FUNCTION_RECORD_SIZE,
    object_offset: cell::NATIVE_FUNCTION_OBJECT_OFFSET,
};

/// A Map or Set record (`CollectionRecord`): its ordinary object part at `object_offset`, whose
/// shape and prototype link a method read by name of it tests (`staged::elements::plain_collection_link`).
pub const NATIVE_COLLECTION_LAYOUT: crate::staged::elements::CollectionLayout = crate::staged::elements::CollectionLayout {
    record_size: cell::NATIVE_COLLECTION_RECORD_SIZE,
    object_offset: cell::NATIVE_COLLECTION_OBJECT_OFFSET,
};

/// Layout facts for code generators which emit read-only ordinary-array accesses.
pub const NATIVE_ARRAY_LAYOUT: NativeArrayLayout = NativeArrayLayout {
    record_capacity: records::ARRAY_RECORD_CAPACITY,
    record_size: cell::NATIVE_ARRAY_RECORD_SIZE,
    record_align: cell::NATIVE_ARRAY_RECORD_ALIGN,
    elements_offset: cell::NATIVE_ARRAY_ELEMENTS_OFFSET,
    extras_elements_offset: cell::NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET,
    elements_header_offset: array_elements::NATIVE_HEADER_POINTER_OFFSET,
    header_length_offset: array_elements::NATIVE_HEADER_LENGTH_OFFSET,
    header_values_offset: array_elements::NATIVE_HEADER_VALUES_OFFSET,
    header_owners_offset: array_elements::NATIVE_HEADER_OWNERS_OFFSET,
    header_capacity_offset: array_elements::NATIVE_HEADER_CAPACITY_OFFSET,
    capacity_mask: array_elements::NATIVE_HEADER_CAPACITY_MASK,
    object_offset: cell::NATIVE_ARRAY_OBJECT_OFFSET,
    frozen_mask: crate::value_vec::VALUE_VEC_FROZEN,
    integrity_mask: crate::value_vec::VALUE_VEC_INTEGRITY_BITS,
};

/// The native view of a hot record space's allocation cursor (`records::RecordCursor`): the run
/// `[next, limit)` of fresh ids whose occupancy is already set. An inline allocation loads
/// `next` and `limit`, and when `next < limit` stores `next + 1`, takes the old `next` as the
/// record id, and writes the record at the space's base plus `id * record_size`. Otherwise it
/// calls out to `Heap::alloc_*`, which refills the run (`RecordSpace::cursor_refill`), then
/// `retire_cursor` at each collection frees the unallocated rest. Its words are `u32`s.
/// Generated code reads only the two offsets; the cursor's size is pinned below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeRecordCursorLayout {
    pub next_offset: usize,
    pub limit_offset: usize,
}

pub const NATIVE_RECORD_CURSOR_LAYOUT: NativeRecordCursorLayout = NativeRecordCursorLayout {
    next_offset: std::mem::offset_of!(records::RecordCursor, next),
    limit_offset: std::mem::offset_of!(records::RecordCursor, limit),
};

const _: () = assert!(
    std::mem::size_of::<records::RecordCursor>() == 8 && std::mem::align_of::<records::RecordCursor>() == 4,
    "generated code reads a record cursor as two u32 words"
);

/// Which invariants an existing-slot store must preserve. Native views cannot
/// replace property storage; typed native regions also leave on content widening.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PropertyStoreLayout {
    Preserve,
    /// Native code leaves before a store that retires its content observations.
    PreserveNative,
    MayWiden,
}

/// The native reader's view of an ordinary object record and its out-of-line property words.
///
/// Generated code may resolve only a current direct-object Value to a live record, must
/// compare its exact auxiliary shape (which also excludes dictionary storage), and must
/// finish before any safepoint or property-arena mutation. `properties` pointers are scoped
/// to one synchronous no-safepoint execution because growth and compaction can move the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeObjectLayout {
    pub record_capacity: usize,
    pub record_size: usize,
    pub record_align: usize,
    /// Where a record keeps its layout id (`heap::layouts`).
    pub layout_offset: usize,
    /// Where a layout's table entry keeps the `meta` and auxiliary words.
    pub meta_offset: usize,
    pub auxiliary_offset: usize,
    pub payload_offset: usize,
    pub extras_arena_start_offset: usize,
    pub extras_inline_values_offset: usize,
    pub inline_storage_mask: u32,
    pub array_mask: u32,
    pub storage_start_mask: u32,
    pub dictionary_mask: u32,
    /// The auxiliary word's storage-form bits (`value_vec::INLINE_FORM`, `EXTRAS_FORM`,
    /// `NARROW_FORM`), which a recorded shape leaves out: a guard of a recorded shape masks
    /// them off the word.
    pub inline_form: u32,
    pub extras_form: u32,
    pub narrow_form: u32,
    pub narrow_run_tag: u64,
    /// The meta bits of an object that is not extensible or is frozen (`VALUE_VEC_INTEGRITY_BITS`).
    pub integrity_mask: u32,
    pub inline_property_count: usize,
    pub extras_tag: u64,
    pub extras_tag_mask: u64,
    pub proto_mask: u32,
    pub proto_record_base: u32,
    pub proto_record_end: u32,
    pub mark_records_per_block: usize,
    pub mark_word_bits: usize,
    pub mark_block_size: usize,
    pub mark_offset: usize,
    pub property_value_size: usize,
    pub property_value_align: usize,
}

/// Layout facts for code generators which emit read-only ordinary-object field accesses.
pub const NATIVE_OBJECT_LAYOUT: NativeObjectLayout = NativeObjectLayout {
    record_capacity: records::OBJECT_RECORD_CAPACITY,
    record_size: cell::NATIVE_OBJECT_RECORD_SIZE,
    record_align: cell::NATIVE_OBJECT_RECORD_ALIGN,
    layout_offset: cell::NATIVE_OBJECT_LAYOUT_OFFSET,
    meta_offset: cell::NATIVE_OBJECT_META_OFFSET,
    auxiliary_offset: cell::NATIVE_OBJECT_AUXILIARY_OFFSET,
    payload_offset: cell::NATIVE_OBJECT_PAYLOAD_OFFSET,
    extras_arena_start_offset: cell::NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET,
    extras_inline_values_offset: cell::NATIVE_OBJECT_EXTRAS_INLINE_VALUES_OFFSET,
    inline_storage_mask: cell::NATIVE_OBJECT_INLINE_STORAGE_MASK,
    array_mask: cell::NATIVE_OBJECT_ARRAY_MASK,
    storage_start_mask: crate::value_vec::START_MASK,
    dictionary_mask: crate::value_vec::DICTIONARY_STORAGE,
    inline_form: cell::NATIVE_OBJECT_INLINE_FORM,
    extras_form: cell::NATIVE_OBJECT_EXTRAS_FORM,
    narrow_form: crate::value_vec::NARROW_FORM,
    narrow_run_tag: cell::NATIVE_OBJECT_NARROW_RUN_TAG,
    integrity_mask: crate::value_vec::VALUE_VEC_INTEGRITY_BITS,
    inline_property_count: INLINE_PROPERTY_COUNT,
    extras_tag: cell::NATIVE_OBJECT_EXTRAS_TAG,
    extras_tag_mask: cell::NATIVE_OBJECT_EXTRAS_TAG_MASK,
    proto_mask: cell::NATIVE_OBJECT_PROTO_MASK,
    proto_record_base: cell::NATIVE_OBJECT_PROTO_RECORD_BASE,
    proto_record_end: cell::NATIVE_OBJECT_PROTO_RECORD_END,
    mark_records_per_block: records::NATIVE_RECORDS_PER_BLOCK,
    mark_word_bits: records::NATIVE_RECORD_MARK_WORD_BITS,
    mark_block_size: records::NATIVE_RECORD_BLOCK_SIZE,
    mark_offset: records::NATIVE_RECORD_MARK_OFFSET,
    property_value_size: std::mem::size_of::<Value>(),
    property_value_align: std::mem::align_of::<Value>(),
};

/// Layout facts for code generators reading a dense environment-slot storage
/// (`EnvironmentSlots::dense`, `dense_plain`): the storage word holds a pointer to the slice
/// header; the header's top bit marks a sparse storage and the rest is the length; the
/// slots follow the header, one `Value` word each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeEnvironmentLayout {
    /// Offset in the storage word of the header pointer.
    pub header_pointer_offset: i32,
    /// Header bit set on a sparse storage; the length is the other bits.
    pub sparse_mark: u64,
    /// Offset from the header to the first slot.
    pub slots_offset: u64,
    pub slot_size: u64,
}

pub const NATIVE_ENVIRONMENT_LAYOUT: NativeEnvironmentLayout = NativeEnvironmentLayout {
    header_pointer_offset: 0,
    sparse_mark: thin_slice::NATIVE_MARK_BIT,
    slots_offset: thin_slice::NATIVE_WORD_ELEMENTS_OFFSET as u64,
    slot_size: std::mem::size_of::<Value>() as u64,
};

/// The header of storage that holds no slot: marked sparse, so a dense read of it misses.
static ABSENT_ENVIRONMENT_HEADER: u64 = thin_slice::NATIVE_MARK_BIT;
/// The storage word that names that header, as an `EnvironmentSlots` names its own.
static ABSENT_ENVIRONMENT_STORAGE: &u64 = &ABSENT_ENVIRONMENT_HEADER;

/// The address of storage every native environment read misses at (`staged::environment::environment_slot`): the word
/// a payload holds where its running body has no environment window.
pub fn absent_environment_storage() -> u64 {
    std::ptr::from_ref(&ABSENT_ENVIRONMENT_STORAGE).addr() as u64
}

const _: () =
    assert!(NATIVE_OBJECT_LAYOUT.record_size.is_multiple_of(NATIVE_OBJECT_LAYOUT.record_align));
const _: () =
    assert!(NATIVE_OBJECT_LAYOUT.property_value_size == std::mem::size_of::<Value>());
const _: () =
    assert!(NATIVE_OBJECT_LAYOUT.property_value_align == std::mem::align_of::<Value>());
const _: () = assert!(NATIVE_OBJECT_LAYOUT.inline_property_count > 0);
const _: () =
    assert!(NATIVE_OBJECT_LAYOUT.storage_start_mask & NATIVE_OBJECT_LAYOUT.dictionary_mask == 0);

const _: () = assert!(
    NATIVE_ARRAY_LAYOUT
        .record_size
        .is_multiple_of(NATIVE_ARRAY_LAYOUT.record_align)
);
