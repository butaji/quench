// The object, array and record words generated code reads: the storage forms and integrity bits of an
// object's auxiliary word, the extras and narrow-run tags of its payload word, the prototype index ranges,
// the array header's capacity bits and the record block's mark words. Bit encodings and sizes only, so the
// file compiles with `core` alone. ONE source: `value_vec`, `heap::cell`, `heap::records` and
// `heap::array_elements` re-export these, and `staged/layout_offsets.rs` derives the stencil's layouts from them.
#![allow(dead_code)]

/// The auxiliary word's storage position (`value_vec::START_MASK`): the bits below the flags.
pub const START_MASK: u32 = 0x3fff_ffff;
/// Meta bit of an object that is not extensible (`value_vec::NON_EXTENSIBLE`).
pub const NON_EXTENSIBLE: u32 = 1 << 30;
/// Meta bit of a frozen object (`value_vec::FROZEN`).
pub const FROZEN: u32 = 1 << 31;
/// The auxiliary bit of dictionary storage (`value_vec::DICTIONARY_STORAGE`).
pub const DICTIONARY_STORAGE: u32 = 1 << 31;
/// An object whose property access is not the ordinary one its shape describes: set once, never cleared
/// (`value_vec::EXOTIC_ACCESS`).
pub const EXOTIC_ACCESS: u32 = 1 << 30;
/// An object record's storage form, kept in its auxiliary word beside the shape: its values are inline
/// (`value_vec::INLINE_FORM`), behind extras (`EXTRAS_FORM`), or in a narrow run (`NARROW_FORM`).
pub const INLINE_FORM: u32 = 1 << 29;
pub const EXTRAS_FORM: u32 = 1 << 28;
pub const NARROW_FORM: u32 = 1 << 27;
/// The auxiliary word's meta bits of an object that is not extensible or is frozen.
pub const VALUE_VEC_INTEGRITY_BITS: u32 = NON_EXTENSIBLE | FROZEN;
/// The meta bit of a frozen array's elements (`value_vec::VALUE_VEC_FROZEN`).
pub const VALUE_VEC_FROZEN: u32 = FROZEN;
/// Properties an object keeps in its record (`value_vec::INLINE_PROPERTY_COUNT`).
pub const INLINE_PROPERTY_COUNT: usize = 2;

/// The meta bit of an array record (`heap::cell::OBJECT_ARRAY`).
pub const OBJECT_ARRAY: u32 = 1 << 28;
/// The meta bit of an object whose values are inline (`heap::cell::OBJECT_INLINE_STORAGE`).
pub const OBJECT_INLINE_STORAGE: u32 = 1 << 29;
/// The payload word's tag of an object with extras (`heap::cell::EXTRAS_TAG`), and its mask.
pub const EXTRAS_TAG: u64 = 0x7fff_0000_0000_0000;
pub const EXTRAS_TAG_MASK: u64 = 0xffff_0000_0000_0000;
/// The payload word's bit of a narrow run (`heap::cell::NARROW_RUN_TAG`): one unit past the start.
pub const NARROW_RUN_TAG: u64 = 1 << u32::BITS;

/// The prototype index bits of an object's meta word (`heap::cell::PROTO_INDEX_BITS`).
pub const PROTO_INDEX_BITS: u32 = (1 << 28) - 1;
/// The object records a heap holds, by legacy slot (`heap::cell::MAX_HEAP_CELLS`).
pub const MAX_HEAP_CELLS: usize = 1 << 27;
/// The prototype links one record family's window holds (`heap::cell::*_PROTO_WINDOW`). A record
/// space grows past its window (the capacities below); a record past it that becomes a prototype
/// takes an overflow link (`heap::prototypes`).
pub const PROTO_RECORD_WINDOW: usize = 1 << 24;
/// The prototype index of the first object record, and of each family's window after it.
pub const PROTO_OBJECT_RECORD_BASE: u32 = MAX_HEAP_CELLS as u32;
pub const PROTO_ARRAY_RECORD_BASE: u32 = PROTO_OBJECT_RECORD_BASE + PROTO_RECORD_WINDOW as u32;
/// The records each record space holds (`heap::records`), the one source the heap's spaces and the
/// layouts generated code reads (`layout_offsets`) take. The spaces reserve address space only
/// (`MAP_NORESERVE`), so a large capacity costs no memory until records are used. Every capacity is
/// a power of two and a multiple of `RECORDS_PER_BLOCK`.
pub const OBJECT_RECORD_CAPACITY: usize = 1 << 30;
pub const ARRAY_RECORD_CAPACITY: usize = 1 << 30;
pub const FUNCTION_RECORD_CAPACITY: usize = 1 << 30;
pub const PROMISE_RECORD_CAPACITY: usize = 1 << 28;
pub const ITERATOR_RECORD_CAPACITY: usize = 1 << 28;
pub const COLLECTION_RECORD_CAPACITY: usize = 1 << 28;
/// Await states and Wasm vectors are never prototypes.
pub const AWAIT_RECORD_CAPACITY: usize = 1 << 28;
pub const V128_RECORD_CAPACITY: usize = 1 << 28;
/// Wasm GC objects: records whose head holds the payload's address and length (`record_heads::WasmGcHead`).
pub const WASM_GC_RECORD_CAPACITY: usize = 1 << 28;

/// Where an object record keeps its `meta` word: the layout's entry holds `auxiliary` low and `meta` high.
pub const META_OFFSET: usize = size_of::<u32>();

/// The dense backing's capacity word: its `BIASED` and `KEEP_WIDE` allocator flags; the rest counts values
/// (`heap::array_elements`).
pub const BIASED: u32 = 1 << 31;
pub const KEEP_WIDE: u32 = 1 << 30;
pub const HEADER_CAPACITY_MASK: u32 = !(BIASED | KEEP_WIDE);

/// The records of one block (`heap::records::RECORDS_PER_BLOCK`), and the bits of one occupancy word.
pub const RECORDS_PER_BLOCK: usize = 1_024;
pub const OCCUPANCY_WORD_BITS: usize = u64::BITS as usize;
/// The words of each of a block's bit sets.
pub const BLOCK_WORDS: usize = RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS;

/// A record block's four bit sets, in the order native code reads them (`marked` at `NATIVE_RECORD_MARK_OFFSET`).
#[repr(C)]
pub struct RecordBlock {
    pub occupied: [u64; BLOCK_WORDS],
    pub marked: [u64; BLOCK_WORDS],
    /// Young records that survived one minor collection (`PromotionAge::Second`): unmarked,
    /// swept by the next minor collection, which frees or promotes them.
    pub aged: [u64; BLOCK_WORDS],
    /// Freed ids no allocation takes until `RecordSpace::release_withheld`
    /// (`RecordSpace::withhold`).
    pub withheld: [u64; BLOCK_WORDS],
}

/// A class cell (`heap::class_space`) is a run of granules, one `CLASS_HEADER_BYTES` each. Its
/// first granule is the header: the class id in the low half (`heap::class_space` writes it there),
/// and the cell's extras id in the high half. Staged code reads the class as the low u32 only.
pub const CLASS_HEADER_BYTES: usize = size_of::<u64>();
/// The class-id word that no class names: what a word that is not a live cell reads as in a class-keyed
/// row, so every class arm and guard misses. It lies past the class table's capacity, so no class id is it.
pub const NO_CLASS: u32 = u32::MAX;

/// The class space's regions: region `k` starts at granule `k << REGION_SHIFT` and holds cells of
/// `2^k` granules, so a granule's local cell index is `(g & REGION_MASK) >> k`.
pub const REGION_SHIFT: u32 = 29;
pub const REGION_MASK: u32 = (1 << REGION_SHIFT) - 1;
/// The regions, one per capacity class `0..CAPACITY_CLASSES`.
pub const CAPACITY_CLASSES: usize = 6;

/// The capacity-region bits of a class word (`heap::layouts::intern_class`): bits 22 to 24 of its auxiliary word,
/// above the shape field (`value_vec::SHAPE_MASK`). A record's auxiliary word has none.
pub const CLASS_REGION_SHIFT: u32 = 22;
pub const CLASS_REGION_MASK: u32 = 0b111 << CLASS_REGION_SHIFT;

/// The fields a cell of region `region` holds: its `2^region` granules less the header. A cell's capacity class is
/// its region, so no word records it.
pub const fn region_field_capacity(region: u32) -> usize {
    (1 << region) - 1
}