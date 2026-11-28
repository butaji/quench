use crate::Value;
use crate::heap::growth::{GrowthPolicy, grown_capacity};
use crate::typed_ir::SlotType;
use rustc_hash::FxHashMap;

mod buffer; mod plan_values;
mod subscribe;
mod runs; mod arena; mod snapshot; mod shapes; mod access; mod dictionary; mod young; mod inspect; mod release; mod allocate;
use buffer::PropertyBuffer;
use runs::{FreeRuns, Nursery, Reservation};
pub(crate) use runs::YoungRuns;
#[allow(unused_imports)]
pub use buffer::NATIVE_PROPERTY_BUFFER_LAYOUT;

/// The word a Number-typed slot stores for `value` (`Value::from_f64_raw` semantics): an int32 word
/// becomes its double's bits; any other word, a double included, is itself. Stores never canonicalize
/// a Number slot (decision 4.1), so readers of a Number slot need no tag test.
#[inline(always)]
pub(crate) fn raw_number_word(value: Value) -> Value {
    value.as_number().map_or(value, Value::from_f64_raw)
}

/// The `Value` of a word a staged read took from an arena run. Every arena word is the one `Value`
/// its slot holds: a Number is its `Value::number` encoding (a double's raw bits under invariant I,
/// with NaN canonical), so the word is the value.
#[inline(always)]
pub fn decode_staged_word(word: u64) -> Value {
    Value::from_bits(word)
}

// The storage-form and integrity bits are the shared object encoding (`staged::object_encoding`).
pub use crate::staged::object_encoding::{CLASS_REGION_SHIFT, DICTIONARY_STORAGE, EXOTIC_ACCESS, EXTRAS_FORM, FROZEN, INLINE_FORM, NARROW_FORM, NON_EXTENSIBLE, RESERVED_AUX_BIT, START_MASK};
pub const STORAGE_FORM: u32 = INLINE_FORM | EXTRAS_FORM | NARROW_FORM;
/// The auxiliary bits a shape occupies: below the class region bits (`CLASS_REGION_SHIFT`), which a class word uses
/// for its cell's capacity region and no shape or record word sets. This narrows the shape field from 25 bits to 22,
/// so `SHAPE_LIMIT` is 4 M shapes less one (the last stays unused, see `set_auxiliary`).
pub(crate) const SHAPE_MASK: u32 = (1 << CLASS_REGION_SHIFT) - 1;
/// Shapes are numbered below `SHAPE_LIMIT`, which the all-ones sentinel bounds.
pub(crate) const SHAPE_LIMIT: u32 = SHAPE_MASK;
const _: () = assert!(RESERVED_AUX_BIT & (STORAGE_FORM | DICTIONARY_STORAGE | EXOTIC_ACCESS) == 0 && SHAPE_MASK == SHAPE_LIMIT);
const _: () = assert!(SHAPE_MASK & (RESERVED_AUX_BIT | STORAGE_FORM | DICTIONARY_STORAGE | EXOTIC_ACCESS) == 0);
const _: () = assert!(STORAGE_FORM & (DICTIONARY_STORAGE | EXOTIC_ACCESS) == 0 && INLINE_FORM & EXTRAS_FORM == 0);
/// The words a narrow run of `len` values fills.
#[inline(always)] const fn narrow_words(len: usize) -> usize {
    len.div_ceil(2)
}
/// The narrow value at `slot` of the narrow run whose first word is `words`.
///
/// # Safety
/// `words` addresses a narrow run holding `slot`.
#[inline(always)] unsafe fn narrow_slot(words: *const Value, slot: usize) -> u32 {
    let half = if cfg!(target_endian = "little") { slot % 2 } else { 1 - slot % 2 };
    // SAFETY: the caller's run holds the word; a `Value` is a `u64`, two `u32` halves.
    unsafe { words.add(slot / 2).cast::<u32>().add(half).read() }
}
const EMPTY_START: u32 = START_MASK;
const INLINE_PROPERTY_START: u32 = EMPTY_START - 1;
const MAX_ARENA_START: usize = INLINE_PROPERTY_START as usize;
/// Properties an object keeps in its record (`staged::object_encoding`). The Score lane runs three for
/// RayTrace's vectors at the cost of max RSS (Splay 162 -> 204 MB); the integration branch keeps
/// memory-first defaults (the owner's direction, 2026-10-17 and 2026-10-18).
pub use crate::staged::object_encoding::INLINE_PROPERTY_COUNT;
/// The smallest arena range. An object reaches the arena by outgrowing its inline slots, or
/// by storing a value they cannot hold before that; a range for its first values would regrow
/// as it grows past them.
const MIN_CAPACITY: usize = (INLINE_PROPERTY_COUNT + 1).next_power_of_two();
/// Range capacities are the size classes `capacity_class` derives, one free list each.
const BUCKETS: usize = 128;
/// The most an add that moves an object's run reserves for it, as a multiple of the length
/// the run grows to: the size the shape tree predicts (`Vm::predicted_run_capacity`), clamped
/// to this. A prediction that comes true saves the moves that growing by doubling would take;
/// one that does not wastes at most what doubling would have wasted, since the reserve never
/// exceeds twice the length.
pub(crate) const PREDICTED_RUN_CAP_FACTOR: usize = 2;

/// The capacity of a range holding `len` values: `MIN_CAPACITY`, the step of 6, then from 8
/// the quarter steps of each doubling (8, 10, 12, 14, 16, 20, 24, 28, 32, ...), the smallest
/// that holds them. Quarter steps leave a range at most a fifth unused where three-quarter
/// steps left a third (ai-astar: 250K nodes of 9 values held 12 each, 6 MB idle), and every
/// class is a multiple of `CAPACITY_CLASS_STEP`, which `young_run_padded` copies by.
#[inline] pub(crate) const fn capacity_class(len: usize) -> usize {
    if len <= MIN_CAPACITY {
        return MIN_CAPACITY;
    }
    if len <= 2 * MIN_CAPACITY {
        return if len <= MIN_CAPACITY + MIN_CAPACITY / 2 { MIN_CAPACITY + MIN_CAPACITY / 2 } else { 2 * MIN_CAPACITY };
    }
    let quarter = len.next_power_of_two() / 8;
    len.div_ceil(quarter) * quarter
}
/// Whether the arena range of `len` values has room for one more without moving: its capacity is a
/// class of its length alone (`ordered_capacity`), so for a range that exists (`len` above zero)
/// the room is a fact of the shape's length, which a staged add reads as a layout fact.
pub(crate) const fn range_has_room(len: usize) -> bool {
    len > 0 && len < capacity_class(len)
}
/// Released values on the free lists, as a share (`1 / divisor`) of the arena's length, that
/// make a full collection compact the arena. Freed ranges are reused within their size class,
/// so compaction recovers only what other classes leave idle; rewriting every live object's
/// storage on each full collection cost source-map-wtb 3 s of 27 (TOD-137).
/// The values each size class adds to the one before it up to `2 * MIN_CAPACITY`, the
/// unit `young_run_padded` copies past `MIN_CAPACITY`.
const CAPACITY_CLASS_STEP: usize = capacity_class(MIN_CAPACITY + 1) - MIN_CAPACITY;
const COMPACTION_RELEASED_SHARE_DIVISOR: usize = 4;
/// The arena capacity a compaction keeps, as a multiple of the values it kept: the room one
/// doubling of the compacted arena would give. Before, a load burst's growth stayed resident
/// for good: a fastify server under 500 connections grew the arena to 654K values (5.2 MB)
/// while full collections kept 48-250K, and returning the rest lowered the board's final
/// peak 40.7 -> 37.8 MB (median of 8 rounds, bombardier, board phases, plain x86_64 build,
/// 2026-10-26); V8-v7 RSS and Score and the npm-perf fastify workload were unchanged.
const COMPACTED_CAPACITY_FACTOR: usize = 2;
/// The arena size from which a full collection that leaves over half its values released gives
/// the pages of those released runs back (`ValueArena::return_free_pages`). Below it the
/// resident waste is small and the next compaction recovers it.
const FREE_PAGES_MIN_BYTES: usize = 16 << 20;
/// The values the nursery (`Nursery`) adds to its range at a time once its runs reach the end
/// of it, while it ends the arena: one resize and one fill of `undefined` per that many values
/// rather than per run. A range keeps its size across collections, so it grows to the largest
/// cycle of new runs. Large (1 MB of values) because allocations of ordinary runs end the arena and strand a small range: raytrace then took the slow path on 150k of 164k constructs (0 now).
const NURSERY_GROWTH: usize = 1 << 17;
/// The longest array whose elements a young run holds (`ValueArena::young_elements`): a
/// young array's elements are copied once more when it survives or is first written, so the
/// bound keeps that copy as short as the inline element initialization V8 unrolls for
/// literals (`kElementLoopUnrollLimit`, js-create-lowering.cc).
const YOUNG_ELEMENTS_MAX: usize = 16;

/// Entries in the direct-mapped `ValueArena::reservations` table; a power of two.
const RESERVATIONS: usize = 16;

#[quench_macros::test_visible]
/// Compact per-object shape, integrity, and property-storage metadata.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueVec {
    start: u32,
    auxiliary: u32,
}

/// The table of a dictionary: each slot index to its stored word (a deleted slot holds `Value::DELETED`).
pub(crate) type DictionaryTable = FxHashMap<u32, Value>;

/// `ValueVec::start` bits that hold the storage position: an arena offset, a dictionary id,
/// or one of the empty and inline markers.
pub(crate) const VALUE_VEC_START_BITS: u32 = START_MASK;
/// `ValueVec::start` bits that hold the object's integrity level, and the integrity bit of a frozen
/// object, whose elements no store replaces (`staged::object_encoding`).
pub(crate) use crate::staged::object_encoding::{VALUE_VEC_FROZEN, VALUE_VEC_INTEGRITY_BITS};

impl ValueVec {
    /// The two words, for an owner that stores them apart.
    #[inline] pub(crate) const fn into_parts(self) -> (u32, u32) {
        (self.start, self.auxiliary)
    }

    #[inline(always)] pub(crate) const fn from_parts(start: u32, auxiliary: u32) -> Self {
        Self { start, auxiliary }
    }

    #[inline] pub(crate) const fn new() -> Self {
        Self {
            start: EMPTY_START,
            auxiliary: 0,
        }
    }

    #[inline] pub(crate) const fn inline_property_storage(shape: u32) -> Self {
        Self {
            start: INLINE_PROPERTY_START,
            auxiliary: shape,
        }
    }

    #[inline] pub(crate) const fn auxiliary(self) -> u32 {
        self.auxiliary & SHAPE_MASK
    }

    /// The auxiliary word with its dictionary and exotic bits, without its storage form: equal
    /// to `auxiliary` exactly when the vector is neither in dictionary storage nor an exotic
    /// object's, so one comparison with a recorded shape proves all three.
    #[inline(always)] pub(crate) const fn exact_auxiliary(self) -> u32 {
        self.auxiliary & !STORAGE_FORM
    }

    #[inline] pub(crate) fn has_inline_property_storage(self) -> bool {
        self.start() as u32 == INLINE_PROPERTY_START
    }

    pub(crate) fn has_compact_range(self) -> bool {
        !self.has_inline_property_storage()
            && !self.is_dictionary()
            && self.start() != EMPTY_START as usize
    }

    #[inline] pub(crate) fn set_auxiliary(&mut self, value: u32) {
        // The last shape stays unused, so a dictionary vector's exact auxiliary
        // (`exact_auxiliary`) never equals the all-ones sentinel caches record as "none".
        assert!(value < SHAPE_LIMIT, "object shape table exhausted");
        self.auxiliary = (self.auxiliary & !SHAPE_MASK) | value;
    }

    /// Whether the vector holds no values at all: an empty compact range.
    #[inline(always)] pub(crate) fn holds_no_values(self) -> bool {
        !self.is_dictionary() && self.start() == EMPTY_START as usize
    }

    /// Whether its values are a narrow run (`NARROW_FORM`).
    #[inline(always)] pub(crate) fn is_narrow(self) -> bool {
        self.auxiliary & NARROW_FORM != 0
    }

    #[inline] pub(crate) fn is_dictionary(self) -> bool {
        self.auxiliary & DICTIONARY_STORAGE != 0
    }

    #[inline]
    fn dictionary_id(self) -> u32 {
        debug_assert!(self.is_dictionary());
        self.start & START_MASK
    }

    fn use_dictionary(&mut self, id: u32) {
        assert!(id <= START_MASK, "property dictionary table exhausted");
        self.start = (self.start & !START_MASK) | id;
        self.auxiliary |= DICTIONARY_STORAGE;
    }

    #[inline] pub(crate) fn is_extensible(self) -> bool {
        self.start & NON_EXTENSIBLE == 0
    }

    pub(crate) fn set_extensible(&mut self, value: bool) {
        if value {
            self.start &= !NON_EXTENSIBLE;
        } else {
            self.start |= NON_EXTENSIBLE;
        }
    }

    #[inline]
    pub(crate) fn is_frozen(self) -> bool {
        self.start & FROZEN != 0
    }

    pub(crate) fn set_frozen(&mut self, value: bool) {
        if value {
            self.start |= FROZEN;
        } else {
            self.start &= !FROZEN;
        }
    }

    #[inline] pub(crate) fn preserve_integrity_from(&mut self, previous: Self) {
        self.start = (self.start & !(NON_EXTENSIBLE | FROZEN))
            | (previous.start & (NON_EXTENSIBLE | FROZEN));
    }

    #[inline] fn start(self) -> usize {
        (self.start & START_MASK) as usize
    }


    pub(crate) fn start_offset(self) -> usize {
        debug_assert!(!self.is_dictionary());
        debug_assert!(!self.has_inline_property_storage());
        self.start()
    }

    fn relocate(&mut self, start: usize) {
        assert!(start < EMPTY_START as usize, "property arena exhausted");
        self.start = start as u32 | (self.start & !START_MASK);
    }
}

impl Default for ValueVec {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct ValueArena {
    values: PropertyBuffer,
    free: [FreeRuns; BUCKETS],
    /// Values held by the ranges on `free`.
    released_values: usize,
    /// The narrow-run words the current compaction wrote, for its `QUENCH_TRACE=gc` census.
    narrowed_words: usize,
    /// Shapes a narrow run of which widened for a value without a narrow form: their objects
    /// keep ordinary runs from then on, so a run is not narrowed and widened again each cycle.
    widened_shapes: rustc_hash::FxHashSet<u32>,
    shape_lengths: Vec<u32>,
    /// The slot owners of every shape (`ValueArena::slot_owner`): each shape's chain starts at
    /// its `shape_chains` index in `chain_owners`, which names, per slot, the shape that added
    /// it. A child that appends a slot at its parent's chain end shares that chain.
    shape_chains: Vec<u32>,
    chain_owners: Vec<u32>,
    /// Per owner shape, compact content types, maintained by every store: a host write joins its
    /// value's class before it lands (`note_slot_value`, `note_shape_value`, `note_plan_values`,
    /// publication's `join_object_slot_values`), a native raw write is admitted before it is
    /// exposed (`note_slot_type`), and a widening retires the readers first. While tracking, a
    /// slot holds a value of its owner's type; code subscribed to the observation
    /// (`subscribe_slot_observation`) is retired before any other value lands.
    owner_types: Vec<SlotType>,
    slot_readers: crate::code_dependencies::CodeReaders,
    /// Numeric shape departures only; heap admission, exotic access and physical
    /// storage remain separate proof obligations.
    shape_readers: crate::code_dependencies::CodeReaders,
    /// Whether writes join their values' classes (`ValueArena::track`): only a runtime whose
    /// native code reads representations keeps them.
    tracking: bool,
    /// Bumped whenever an owner's type or shape assignment changes: slots of two shapes encode
    /// alike only under one epoch, as `encodings_match` proved them.
    owner_epoch: u64,
    /// Owners that widened from Int32 to Number (`widen_owner`) and whose int-tagged words the heap
    /// has not rewritten yet (`Heap::rewrite_widened_numbers` drains it, once per observing call).
    int_to_number: Vec<u32>,
    dictionaries: FxHashMap<u32, DictionaryTable>,
    free_dictionaries: Vec<u32>,
    next_dictionary: u32,
    nursery: Nursery,
    /// Values the arena held, past its idle nursery and released runs, after the last full
    /// collection (`GenerationalPacing::arena_growth_reached`).
    settled_values: usize,
    /// Young runs reserved for constructors' receivers (`Reservation`).
    reservations: [Reservation; RESERVATIONS],
    /// The young cycle the valid `reservations` belong to (`Reservation::cycle`).
    reservation_cycle: u32,
}

impl Default for ValueArena {
    fn default() -> Self {
        Self {
            values: PropertyBuffer::default(),
            free: [FreeRuns::EMPTY; BUCKETS],
            released_values: 0,
            narrowed_words: 0,
            widened_shapes: rustc_hash::FxHashSet::default(),
            shape_lengths: vec![0],
            shape_chains: Vec::new(),
            chain_owners: Vec::new(),
            owner_types: Vec::new(),
            slot_readers: crate::code_dependencies::CodeReaders::default(),
            shape_readers: crate::code_dependencies::CodeReaders::default(),
            tracking: false,
            owner_epoch: 0,
            int_to_number: Vec::new(),
            dictionaries: FxHashMap::default(),
            free_dictionaries: Vec::new(),
            next_dictionary: 0,
            nursery: Nursery::default(),
            settled_values: 0,
            reservations: [Reservation::default(); RESERVATIONS],
            reservation_cycle: 0,
        }
    }
}

impl ValueArena {
    pub(crate) fn memory_bytes(&self) -> usize {
        self.values.capacity() * size_of::<Value>()
            + self.dictionaries.capacity() * size_of::<(u32, FxHashMap<u32, Value>)>()
            + self
                .dictionaries
                .values()
                .map(|values| values.capacity() * size_of::<(u32, Value)>())
                .sum::<usize>()
            + self.free_dictionaries.capacity() * size_of::<u32>()
            + self.dependency_bytes()
    }

    /// The bytes of the shape and slot readers: the facts this arena's values hold.
    pub(crate) fn dependency_bytes(&self) -> usize {
        self.slot_readers.memory_bytes() + self.shape_readers.memory_bytes()
    }

    pub(crate) fn memory_breakdown(&self) -> (usize, usize) {
        let values = self.values.capacity() * size_of::<Value>();
        (values, self.memory_bytes() - values)
    }

    pub(crate) fn stats(&self) -> (usize, usize, usize) {
        (
            self.values.len(),
            self.values.capacity(),
            self.free.iter().map(|list| list.len()).sum(),
        )
    }
}

#[cfg(test)]
mod tests;
