use crate::Value;
use crate::fact_stats::{self, FactKind};
use crate::heap::growth::{GrowthPolicy, grown_capacity};
use crate::typed_ir::Representation;
use crate::typed_ir::SlotType;
use crate::vm::switches;
use rustc_hash::FxHashMap;

mod aux_form; mod buffer; mod plan_values;
mod subscribe;
use buffer::PropertyBuffer;
#[allow(unused_imports)]
pub use buffer::NATIVE_PROPERTY_BUFFER_LAYOUT;

/// The word of an arena slot that holds `value`, where the slot's owner has type `owner`: the one
/// place a stored word is encoded. A slot of `Representation::Float64` holds its Number as the IEEE double (an
/// integer as the integral double; NaN is already canonical, so the word is a valid `Value` the
/// collector reads as a non-reference); any other slot holds the `Value` as it is.
#[inline(always)]
pub(crate) fn encode_slot(owner: SlotType, value: Value) -> Value {
    match owner.rep() {
        Representation::Float64 => value.as_int().map_or(value, |int| Value::from_bits(f64::from(int).to_bits())),
        // Slots never hold Int64 words: `SlotType::rep` yields only Int32, Float64 or Tagged.
        Representation::Int64 => unreachable!("a slot word is never an Int64"),
        Representation::Int32 | Representation::Tagged => value,
    }
}

/// The `Value` that the arena word `word` holds, where the slot's owner has type `owner`: the one
/// place a stored word is decoded, `encode_slot`'s inverse. A double word is a Number as
/// `Value::number` makes it (an integral double in int32 range is an int32, `-0` stays a double);
/// a word that is no Number (the deleted marker a removed property leaves) is itself.
#[inline(always)]
pub(crate) fn decode_slot(owner: SlotType, word: Value) -> Value {
    match owner.rep() {
        // A tagged word (an int32, a reference, the deleted marker, a word a packed or narrow form
        // decoded) is canonical already: one tag test passes it. Only a raw double's bits take
        // `Value::number`'s canonical form.
        Representation::Float64 if word.is_tagged() => word,
        Representation::Float64 => Value::number(f64::from_bits(word.to_bits())),
        // Slots never hold Int64 words: `SlotType::rep` yields only Int32, Float64 or Tagged.
        Representation::Int64 => unreachable!("a slot word is never an Int64"),
        Representation::Int32 | Representation::Tagged => word,
    }
}

/// The `Value` of a word a staged read took from an arena run, whose owner the reader does not know
/// (`decode_slot` of `read_type`).
#[inline(always)]
pub fn decode_staged_word(word: u64) -> Value {
    decode_slot(read_type(), Value::from_bits(word))
}

/// `decode_staged_word` of a read whose row runs only where slots hold raw doubles (an own read
/// of a form not tagged, `dispatch_fast::own_formed_handler`): the Number decoding, which passes
/// any tagged word too, without testing the switch again.
#[inline(always)]
pub fn decode_float_word(word: u64) -> Value {
    decode_slot(SlotType::Number, Value::from_bits(word))
}

/// The type a word of a slot is decoded as where its owner is unknown to the reader (`decode_slot`):
/// Number where slots of that type hold raw doubles (`switches::float_slots`), whose decoding is the
/// identity on every canonical `Value`, so it reads the word of any owner; else the type of tagged words.
#[inline(always)]
fn read_type() -> SlotType {
    if switches::float_slots() { SlotType::Number } else { SlotType::Tagged }
}

// The storage-form and integrity bits are the shared object encoding (`staged::object_encoding`).
pub use crate::staged::object_encoding::{DICTIONARY_STORAGE, EXOTIC_ACCESS, EXTRAS_FORM, FROZEN, INLINE_FORM, NARROW_FORM, NON_EXTENSIBLE, START_MASK};
pub const STORAGE_FORM: u32 = INLINE_FORM | EXTRAS_FORM | NARROW_FORM;
pub(crate) use aux_form::CONSTANT_FORM;
const SHAPE_MASK: u32 = !(DICTIONARY_STORAGE | EXOTIC_ACCESS | STORAGE_FORM | CONSTANT_FORM | SHAPE_LIMIT_BITS);
/// The auxiliary bits above `CONSTANT_FORM` no storage form uses (26): no shape reaches them.
const SHAPE_LIMIT_BITS: u32 = 1 << 26;
/// Shapes are numbered below `CONSTANT_FORM`.
const SHAPE_LIMIT: u32 = CONSTANT_FORM - 1;
const _: () = assert!(CONSTANT_FORM & (STORAGE_FORM | DICTIONARY_STORAGE | EXOTIC_ACCESS) == 0 && SHAPE_MASK == SHAPE_LIMIT);
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

/// A size class's released runs, linked through each run's first value: a released run's
/// values are dead, so its list costs no memory of its own. As a `Vec<u32>` per class, the
/// lists held 2.2 MB at TypeScript's peak, the full collection that releases millions of runs.
///
/// The list is `unreleased` runs at the head, then released ones: a run is pushed at the head
/// unreleased, `pop_free` takes the head, and `return_free_pages` gives back the pages of the
/// first `unreleased` runs and then counts them released. So a run whose pages are already
/// given back is never walked again, and every link stays the one `push_free` wrote.
#[derive(Clone, Copy, Debug)]
struct FreeRuns {
    head: u32,
    len: u32,
    unreleased: u32,
}

impl FreeRuns {
    const EMPTY: Self = Self { head: u32::MAX, len: 0, unreleased: 0 };

    fn is_empty(self) -> bool {
        self.len == 0
    }

    fn len(self) -> usize {
        self.len as usize
    }
}
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

/// The runs of new objects (`ValueArena::young_run_padded`), bumped from one range of the
/// arena, `[floor, end)`: those taken since the last collection are `[floor, top)`. A dead
/// young object releases nothing; its collection moves the few young runs whose owners
/// survive (`ValueArena::evacuate_young`) and takes back the rest of `[floor, top)` at once,
/// so a dead young object's storage costs nothing to reclaim and the collection's cost follows
/// its survivors (a scavenger's bump allocation and evacuation, over immix's line reclaim).
///
/// Only an object's creation takes a young run: any other range comes from `allocate`, so
/// every owner of a young run is a young object, which the sweep shows (`RecordSweep::Keep`).
/// Young runs are ordinary size-class runs, so forgetting the nursery (`Nursery::default`) is
/// always sound: its dead runs then wait for a compaction.
///
/// The range stays where it is: once something else ends the arena it no longer grows, and
/// young runs past its end are ordinary ones. Retiring it to start again at the arena's end
/// handed its unused values to the free lists as runs of classes few allocations asked for:
/// on acorn-wtb they held 15K idle values more than the old path's at the peak, 0.2 MB of
/// max RSS.
#[derive(Clone, Copy, Debug, Default)]
struct Nursery {
    floor: usize,
    top: usize,
    end: usize,
}

/// The nursery's young range, `[floor, top)`, which a collection's releases leave as it is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct YoungRuns {
    pub(crate) floor: usize,
    pub(crate) top: usize,
}

impl YoungRuns {
    /// Whether `vector` holds one of these runs.
    #[inline(always)] pub(crate) fn hold(self, vector: ValueVec) -> bool {
        !vector.is_dictionary() && vector.start().wrapping_sub(self.floor) < self.top - self.floor
    }
}

/// Entries in the direct-mapped `ValueArena::reservations` table; a power of two.
const RESERVATIONS: usize = 16;

/// A young run taken past the class of its length for a constructor's receiver
/// (`ValueArena::young_reserved_run`): `[start, end)` was bumped from the nursery whole, so no
/// other run holds any of it until the young cycle ends, and the run's owner may grow into it
/// without moving (`ValueArena::reserved_room`). A run's capacity is otherwise a fact of its
/// length (`ordered_capacity`), which every other reader still sees: the words past it are
/// only the owner's to append into, and a release, an evacuation or a compaction moves or
/// frees no more than that capacity, leaving the rest as dead nursery words. Valid while
/// `cycle` is the arena's `reservation_cycle`, which every event that frees or moves young
/// runs advances; an entry another reservation evicts only loses the room, never a value.
#[derive(Clone, Copy, Debug, Default)]
struct Reservation {
    start: u32,
    end: u32,
    cycle: u32,
}

impl Reservation {
    #[inline(always)]
    fn index(start: usize) -> usize {
        // Fibonacci hashing: reserved runs lie a few classes apart.
        ((start as u32).wrapping_mul(0x9E37_79B9) >> (32 - RESERVATIONS.trailing_zeros())) as usize
    }
}

#[quench_macros::test_visible]
/// Compact per-object shape, integrity, and property-storage metadata.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueVec {
    start: u32,
    auxiliary: u32,
}

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
        self.auxiliary & !(STORAGE_FORM | CONSTANT_FORM)
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
    /// Whether a slot whose owner's type is Number holds its Number as a raw double
    /// (`encode_slot`): tracking under `switches::float_slots`. The words of an owner that left or
    /// entered that type wait in `widened` for the heap to recode them (`recode_owned_run`).
    float: bool,
    widened: Vec<u32>,
    /// Bumped whenever an owner's type or shape assignment changes: slots of two shapes encode
    /// alike only under one epoch, as `encodings_match` proved them.
    owner_epoch: u64,
    dictionaries: FxHashMap<u32, FxHashMap<u32, Value>>,
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
            float: false,
            widened: Vec::new(),
            owner_epoch: 0,
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
    pub(crate) const BUFFER_OFFSET: usize = std::mem::offset_of!(Self, values);
    /// The current storage for out-of-line object properties.
    ///
    /// Native readers remain confined to no-safepoint execution: reserved storage
    /// keeps its base, but compaction relocates ranges and allocation fallback can
    /// move the buffer. Every derived slot must be checked against `len`.
    pub(crate) fn native_values(&self) -> (*const Value, usize) {
        (self.values.as_ptr(), self.values.len())
    }



    /// The arena as a snapshot keeps it (`crate::snapshot`): storage positions name places in
    /// `values`, so the vector and its free ranges are written as they are.
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer, runs: impl IntoIterator<Item = (usize, u32, usize)>) {
        // Slot owners and representations are derived where they are kept (`Heap::track_slot_representations`).
        let Self { values: _, free, released_values, shape_lengths, shape_chains: _, chain_owners: _, owner_types: _, slot_readers: _, shape_readers: _, tracking: _, float: _, widened: _, dictionaries, free_dictionaries, next_dictionary, nursery: _, narrowed_words: _, settled_values: _, widened_shapes: _, reservations: _, reservation_cycle: _, owner_epoch: _ } = self;
        writer.values(&self.tagged_values(runs));
        for bucket in 0..BUCKETS {
            writer.u32s(&self.free_starts(bucket));
        }
        let _ = free;
        writer.len(*released_values);
        writer.u32s(shape_lengths);
        writer.len(dictionaries.len());
        for (id, entries) in dictionaries {
            writer.u32(*id);
            writer.len(entries.len());
            for (key, value) in entries {
                writer.u32(*key);
                writer.value(*value);
            }
        }
        writer.u32s(free_dictionaries);
        writer.u32(*next_dictionary);
    }

    /// The arena's words with every word of `runs` (the tracked runs of live objects) in the
    /// encoding of tagged values: a snapshot keeps no owner types, so its restoring runtime reads
    /// every word as tagged until it joins the owners anew and recodes (`Heap::recode_all_slots`).
    fn tagged_values(&self, runs: impl IntoIterator<Item = (usize, u32, usize)>) -> std::borrow::Cow<'_, [Value]> {
        if !self.float { return std::borrow::Cow::Borrowed(&self.values); }
        let mut values = self.values.to_vec();
        for (start, shape, len) in runs {
            for slot in 0..len {
                values[start + slot] = decode_slot(self.word_type(shape, slot), values[start + slot]);
            }
        }
        std::borrow::Cow::Owned(values)
    }

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let values = reader.values()?;
        let mut free: [Vec<u32>; BUCKETS] = [const { Vec::new() }; BUCKETS];
        for bucket in &mut free {
            *bucket = reader.u32s()?;
        }
        let released_values = reader.len()?;
        let mut values: PropertyBuffer = values.into();
        let mut lists = [FreeRuns::EMPTY; BUCKETS];
        for (list, starts) in lists.iter_mut().zip(&free) {
            // Pushed last to first, so the list pops in the order it was written.
            for &start in starts.iter().rev() {
                if start as usize >= values.len() {
                    return Err(crate::snapshot::SnapshotError("free run past the arena".into()));
                }
                values[start as usize] = Value::from_bits(u64::from(list.head));
                *list = FreeRuns { head: start, len: list.len + 1, unreleased: list.unreleased + 1 };
            }
        }
        let shape_lengths = reader.u32s()?;
        let dictionaries = crate::snapshot::read_list(reader, |reader| {
            let id = reader.u32()?;
            let entries = crate::snapshot::read_list(reader, |reader| Ok((reader.u32()?, reader.value()?)))?;
            Ok((id, entries.into_iter().collect()))
        })?
        .into_iter()
        .collect();
        Ok(Self {
            values,
            free: lists,
            released_values,
            shape_lengths,
            dictionaries,
            free_dictionaries: reader.u32s()?,
            next_dictionary: reader.u32()?,
            ..Self::default()
        })
    }

    pub(crate) fn reset(&mut self) {
        self.widened_shapes.clear();
        self.shape_readers.reset();
        self.values.clear();
        self.free = [FreeRuns::EMPTY; BUCKETS];
        self.released_values = 0;
        self.settled_values = 0;
        self.shape_lengths.truncate(1);
        self.shape_lengths[0] = 0;
        self.dictionaries.clear();
        self.free_dictionaries.clear();
        self.next_dictionary = 0;
        self.nursery = Nursery::default();
        self.forget_reservations();
    }

    pub(crate) fn register_shape(&mut self, shape: u32, length: usize) {
        if self.shape_readers.retire(shape) {
            fact_stats::retired(FactKind::Shape, u64::from(shape));
        }
        assert!(shape < SHAPE_LIMIT, "object shape table exhausted");
        assert!(
            length <= u32::MAX as usize,
            "object has too many properties"
        );
        let shape = shape as usize;
        if self.shape_lengths.len() <= shape {
            self.shape_lengths.resize(shape + 1, 0);
        }
        self.shape_lengths[shape] = length as u32;
    }

    pub(crate) fn reset_shapes(&mut self) {
        self.widened_shapes.clear();
        self.shape_readers.reset();
        self.slot_readers.reset();
        self.tracking = false;
        self.float = false;
        self.widened.clear();
        self.shape_lengths.clear();
        self.shape_lengths.push(0);
        self.shape_chains.clear();
        self.chain_owners.clear();
        self.owner_types.clear();
        self.owner_epoch += 1;
    }

    /// Registers `shape`, of `length` slots, made from `parent` (of `parent_length`): one slot
    /// longer extends the parent's chain, owning the new slot, sharing the chain where the
    /// parent's is its end; as long shares it; anything else owns every slot itself.
    pub(crate) fn register_derived_shape(&mut self, shape: u32, length: usize, parent: Option<(u32, usize)>) {
        self.register_shape(shape, length);
        // Owners serve only kept representations; tracking's start registers every shape.
        if !self.tracking { return; }
        let index = shape as usize;
        if self.shape_chains.len() <= index {
            self.shape_chains.resize(index + 1, 0);
            self.owner_types.resize(index + 1, SlotType::None);
        }
        if self.slot_readers.retire(shape) {
            fact_stats::retired(FactKind::Slot, u64::from(shape));
        }
        self.owner_types[index] = SlotType::None;
        self.owner_epoch += 1;
        // A parent's chain covers its length once registered; one that does not (a parent the
        // heap never registered at that length) lends no owners, and the child owns its slots.
        let covered = parent.filter(|(parent, parent_length)| self.shape_chains.get(*parent as usize)
            .is_some_and(|base| *base as usize + parent_length <= self.chain_owners.len()));
        let chain = match covered {
            Some((parent, parent_length)) if length == parent_length => self.shape_chains[parent as usize] as usize,
            Some((parent, parent_length)) if length == parent_length + 1 => {
                let base = self.shape_chains[parent as usize] as usize;
                if self.chain_owners.len() != base + parent_length {
                    let start = self.chain_owners.len();
                    self.chain_owners.extend_from_within(base..base + parent_length);
                    self.chain_owners.push(shape);
                    start
                } else {
                    self.chain_owners.push(shape);
                    base
                }
            }
            _ => {
                let start = self.chain_owners.len();
                self.chain_owners.extend(std::iter::repeat_n(shape, length));
                start
            }
        };
        self.shape_chains[index] = u32::try_from(chain).expect("slot owner chains fit u32");
    }

    /// Observes a write of `value` to `slot` of an object of `shape`, where the
    /// shape holds the slot (an ordinary shape's; a dictionary's holds none of its own).
    #[inline(always)] pub(crate) fn note_shape_value(&mut self, shape: u32, slot: usize, value: Value) {
        if self.tracking && slot < self.shape_length(shape) && !value.is_deleted() {
            self.note_slot_value(shape, slot, value);
        }
    }

    /// Observes an object's initial values, slot by slot.
    pub(crate) fn note_shape_values(&mut self, shape: u32, values: &[Value]) {
        if !self.tracking { return; }
        for (slot, value) in values.iter().copied().enumerate() {
            self.note_shape_value(shape, slot, value);
        }
    }

    /// Starts joining writes' classes from none: the caller joins what live objects hold.
    pub(crate) fn track(&mut self) {
        self.slot_readers.reset();
        self.tracking = true;
        self.float = switches::float_slots();
        self.widened.clear();
        self.owner_types.fill(SlotType::None);
        self.owner_epoch += 1;
    }

    /// Whether slots of Number owners hold raw doubles.
    #[inline(always)] pub(crate) fn float_slots(&self) -> bool {
        self.float
    }

    /// The epoch of the owners' types (`owner_epoch`): an `encodings_match` proof holds while it is current.
    #[inline(always)] pub(crate) fn owner_epoch(&self) -> u64 {
        self.owner_epoch
    }

    /// Whether the first `len` slots of an object of `source` shape and of `target` shape each have
    /// one encoding (`word_type`), so a word of one shape's slot is the word of the other's. Both
    /// shapes must hold `len` slots; the proof holds under the current `owner_epoch`.
    pub(crate) fn encodings_match(&self, source: u32, target: u32, len: usize) -> bool {
        self.shape_length(source) == len
            && self.shape_length(target) == len
            && (0..len).all(|slot| self.word_type(source, slot) == self.word_type(target, slot))
    }

    /// Whether the word of `slot` of an object of `shape` is a raw double, which only a native reader
    /// that knows its owner's type may read (`Representation::Float64`).
    #[inline(always)] pub(crate) fn holds_raw_doubles(&self, shape: u32, slot: usize) -> bool {
        self.word_type(shape, slot).rep() == Representation::Float64
    }

    /// The type whose encoding the word of `slot` of an object of `shape` has (`encode_slot`): its
    /// owner's where Number slots hold doubles, else the type of tagged words. Defined for the
    /// slots of a registered shape; any other slot is tagged.
    #[inline(always)] pub(crate) fn word_type(&self, shape: u32, slot: usize) -> SlotType {
        if self.float && slot < self.shape_length(shape) && (shape as usize) < self.shape_chains.len() {
            self.slot_observation(shape, slot)
        } else {
            SlotType::Tagged
        }
    }

    /// Whether writes join their values' classes.
    #[inline] pub(crate) fn tracks(&self) -> bool {
        self.tracking
    }

    /// The slots `shape`'s objects store.
    #[inline]
    pub(crate) fn shape_length(&self, shape: u32) -> usize {
        self.shape_lengths.get(shape as usize).map_or(0, |length| *length as usize)
    }

    /// The shape that added `slot` of `shape`'s objects, whose representation it shares.
    #[inline(always)] pub(crate) fn slot_owner(&self, shape: u32, slot: usize) -> u32 {
        self.chain_owners[self.shape_chains[shape as usize] as usize + slot]
    }

    /// The type every value `slot` of `shape` holds is of, while tracking (`owner_types`).
    #[inline(always)] pub(crate) fn slot_observation(&self, shape: u32, slot: usize) -> SlotType {
        self.owner_types[self.slot_owner(shape, slot) as usize]
    }

    /// Observe the next departure from this shape, not the contents of its slots.
    /// The caller must establish its target-shape assumption before subscribing.
    pub(crate) fn subscribe_shape(&mut self, shape: u32,
        code: &std::rc::Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
        if shape as usize >= self.shape_lengths.len() { return false; }
        self.shape_readers.subscribe(shape, code)
    }

    #[inline(always)] pub(crate) fn note_shape_transition(&mut self, previous: u32, next: u32) {
        if previous != next && self.shape_readers.retire(previous) {
            fact_stats::retired(FactKind::Shape, u64::from(previous));
        }
    }

    /// Joins `value`'s class into `slot` of `shape`'s representation; whether it widened. Only a
    /// widening is out of line (`widen_owner`): an untracked store is the test alone.
    #[inline(always)] pub(crate) fn note_slot_value(&mut self, shape: u32, slot: usize, value: Value) -> bool {
        self.tracking && { let owner = self.slot_owner(shape, slot); self.join_slot_owner(owner, value) }
    }

    #[inline(never)]
    fn widen_owner(&mut self, owner: u32, class: SlotType) -> bool {
        self.owner_epoch += 1;
        if self.slot_readers.retire(owner) {
            fact_stats::retired(FactKind::Slot, u64::from(owner));
        }
        let held = std::mem::replace(&mut self.owner_types[owner as usize], class);
        if self.float && (held == SlotType::Number) != (class == SlotType::Number) {
            self.widened.push(owner);
        }
        true
    }

    /// Whether an owner changed the encoding of its words since the heap last recoded them.
    #[inline(always)] pub(crate) fn has_widened(&self) -> bool {
        !self.widened.is_empty()
    }

    /// The owners whose words wait for recoding (`recode_owned_run`), sorted, each once.
    pub(crate) fn take_widened(&mut self) -> Vec<u32> {
        let mut owners = std::mem::take(&mut self.widened);
        owners.sort_unstable();
        owners.dedup();
        owners
    }

    /// Recodes the `len` slots of the run at `start`, the values of an object of `shape`, whose
    /// owners are in `owners` (sorted), to the encodings the owners hold now. A word is decoded as
    /// any owner's, then encoded as its own, so a recoding of a word already in its encoding
    /// changes nothing.
    pub(crate) fn recode_owned_run(&mut self, (start, shape, len): (usize, u32, usize), owners: &[u32]) {
        if shape as usize >= self.shape_chains.len() { return; }
        for slot in 0..len {
            if owners.binary_search(&self.slot_owner(shape, slot)).is_ok() {
                self.recode_word(start + slot, self.word_type(shape, slot));
            }
        }
    }

    /// `recode_owned_run` of every slot, for a run written in the encoding of tagged values
    /// (`with_values`, `young_run_padded`), which the heap recodes once the values are admitted.
    #[inline(always)] pub(crate) fn recode_run(&mut self, start: usize, shape: u32, len: usize) {
        if self.float { self.recode_run_words(start, shape, len); }
    }

    #[inline(never)]
    fn recode_run_words(&mut self, start: usize, shape: u32, len: usize) {
        for slot in 0..len {
            self.recode_word(start + slot, self.word_type(shape, slot));
        }
    }

    fn recode_word(&mut self, at: usize, ty: SlotType) {
        self.values[at] = encode_slot(ty, decode_slot(SlotType::Number, self.values[at]));
    }

    /// `len` values of the run at `start` as values (`decode_slot`), for storage that holds them so.
    fn decoded_run(&self, start: usize, len: usize) -> Vec<Value> {
        self.values[start..start + len].iter().map(|word| decode_slot(read_type(), *word)).collect()
    }

    /// Admits the possible writes of a no-reentry raw slot capability before exposing it.
    pub(crate) fn note_slot_type(&mut self, shape: u32, slot: usize, ty: crate::typed_ir::Type) -> bool {
        if !self.tracking { return false; }
        let owner = self.slot_owner(shape, slot);
        self.join_owner_type(owner, || SlotType::from_type(ty))
    }

    /// Joins `incoming` into the slot type `owner` holds; whether it widened.
    #[inline(always)]
    fn join_owner_type(&mut self, owner: u32, incoming: impl FnOnce() -> SlotType) -> bool {
        debug_assert!((owner as usize) < self.owner_types.len());
        // SAFETY: owners come from `chain_owners`, which only names shapes `owner_types` covers.
        let held = unsafe { *self.owner_types.get_unchecked(owner as usize) };
        if held == SlotType::Tagged { return false; }
        let class = held.join(incoming());
        class != held && self.widen_owner(owner, class)
    }

    #[inline(always)] pub(crate) fn get(&self, vector: ValueVec, index: usize) -> Option<Value> {
        debug_assert!(!vector.has_inline_property_storage());
        if index >= self.len(vector) {
            return None;
        }
        if vector.is_dictionary() {
            return self
                .dictionaries
                .get(&vector.dictionary_id())
                .and_then(|values| values.get(&(index as u32)))
                .copied();
        }
        if vector.is_narrow() {
            // SAFETY: a narrow vector's run holds its length's values.
            return Some(unsafe { self.narrow_get(vector, index) });
        }
        Some(decode_slot(read_type(), self.values[vector.start() + index]))
    }

    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    #[inline(always)]
    pub(crate) unsafe fn get_unchecked(&self, vector: ValueVec, index: usize) -> Value {
        debug_assert!(!vector.has_inline_property_storage());
        debug_assert!(index < self.len(vector));
        if vector.is_dictionary() {
            return self.dictionary_get(vector, index);
        }
        if vector.is_narrow() {
            // SAFETY: as below.
            return unsafe { self.narrow_get(vector, index) };
        }
        // SAFETY: caller provides the initialized-range invariant; every
        // vector range was allocated from `values` and remains reserved.
        unsafe { self.get_flat_at(vector.start(), index) }
    }

    /// `get_flat` of the vector whose storage starts at `start` (`ValueVec::start`).
    ///
    /// # Safety
    /// As for `get_flat`: `start` is a live vector's start and `index` one of its values.
    #[inline(always)]
    pub(crate) unsafe fn get_flat_at(&self, start: usize, index: usize) -> Value {
        // SAFETY: the caller's vector range was allocated from `values` and remains reserved.
        decode_slot(read_type(), unsafe { *self.values.get_unchecked(start + index) })
    }

    /// `get_unchecked` of a vector outside dictionary mode; `None` for a dictionary, whose
    /// out-of-line read a caller may leave to its slower path to keep its own code call-free.
    ///
    /// # Safety
    /// As for `get_unchecked`.
    #[inline(always)]
    pub(crate) unsafe fn get_shaped(&self, vector: ValueVec, index: usize) -> Option<Value> {
        debug_assert!(!vector.has_inline_property_storage());
        if vector.is_dictionary() {
            return None;
        }
        debug_assert!(index < self.len(vector));
        if vector.is_narrow() {
            // SAFETY: as for `get_unchecked`.
            return Some(unsafe { self.narrow_get(vector, index) });
        }
        // SAFETY: as for `get_unchecked`.
        Some(unsafe { self.get_flat_at(vector.start(), index) })
    }

    /// The value at `index` of the narrow vector `vector` (`NARROW_FORM`).
    ///
    /// # Safety
    /// `vector` is a live narrow vector and `index` one of its values.
    #[inline(always)]
    pub(crate) unsafe fn narrow_get(&self, vector: ValueVec, index: usize) -> Value {
        debug_assert!(vector.is_narrow() && index < self.len(vector));
        // SAFETY: the caller's run was allocated from `values` and remains reserved.
        Value::from_narrow(unsafe { narrow_slot(self.values.as_ptr().add(vector.start()), index) })
    }

    /// Stores the narrow value `narrow` at `index` of the narrow vector `vector`.
    ///
    /// # Safety
    /// As for `narrow_get`.
    #[inline(always)]
    pub(crate) unsafe fn narrow_set(&mut self, vector: ValueVec, index: usize, narrow: u32) {
        debug_assert!(vector.is_narrow() && index < self.len(vector));
        let half = if cfg!(target_endian = "little") { index % 2 } else { 1 - index % 2 };
        // SAFETY: the caller's run was allocated from `values` and remains reserved.
        unsafe { self.values.as_mut_ptr().add(vector.start() + index / 2).cast::<u32>().add(half).write(narrow) };
    }

    /// The values of the narrow vector `vector`, decoded: what a store that widens it copies.
    pub(crate) fn narrow_values(&self, vector: ValueVec) -> Vec<Value> {
        // SAFETY: each index is one of the live vector's values.
        (0..self.len(vector)).map(|index| unsafe { self.narrow_get(vector, index) }).collect()
    }

    /// Records that narrow `vector` widens for a value without a narrow form: its shape's
    /// objects keep ordinary runs from then on (`widened_shapes`). A structural widen (an
    /// added property, extras) says nothing of the shape's values and leaves it narrowable.
    pub(crate) fn keep_shape_wide(&mut self, vector: ValueVec) {
        self.widened_shapes.insert(vector.auxiliary());
    }

    /// `widen` for a value without a narrow form (`keep_shape_wide`).
    pub(crate) fn widen_for_value(&mut self, vector: ValueVec) -> ValueVec {
        self.keep_shape_wide(vector);
        self.widen(vector)
    }

    /// `vector`, a narrow vector, widened in place of its owner: an ordinary run holding its
    /// values decoded (`run_of`), or a dictionary once the arena's positions are spent; its
    /// narrow run is released. The owner stores the result (`Object::set_properties`).
    pub(crate) fn widen(&mut self, vector: ValueVec) -> ValueVec {
        debug_assert!(vector.is_narrow() && !self.holds_young_run(vector));
        let values = self.narrow_values(vector);
        self.release_held(vector);
        let mut wide = ValueVec { start: vector.start & !START_MASK | EMPTY_START, auxiliary: vector.auxiliary & !NARROW_FORM };
        if values.is_empty() {
            return wide;
        }
        match self.run_of(&values) {
            Some(start) => wide.start = start | (wide.start & !START_MASK),
            None => wide.use_dictionary(self.allocate_dictionary(values)),
        }
        wide
    }

    #[cold]
    #[inline(never)]
    fn dictionary_get(&self, vector: ValueVec, index: usize) -> Value {
        self.dictionaries[&vector.dictionary_id()][&(index as u32)]
    }

    #[cold]
    #[inline(never)]
    fn dictionary_set(&mut self, vector: ValueVec, index: usize, value: Value) {
        self.dictionaries
            .get_mut(&vector.dictionary_id())
            .expect("object dictionary exists")
            .insert(index as u32, value);
    }

    pub(crate) fn set(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(!vector.has_inline_property_storage());
        assert!(!vector.is_narrow(), "a store widens a narrow vector first");
        assert!(index < self.len(vector));
        if vector.is_dictionary() {
            self.dictionaries
                .get_mut(&vector.dictionary_id())
                .expect("object dictionary exists")
                .insert(index as u32, value);
            return;
        }
        let ty = self.word_type(vector.auxiliary(), index);
        self.values[vector.start() + index] = encode_slot(ty, value);
    }

    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    #[inline(always)]
    pub(crate) unsafe fn set_unchecked(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(!vector.has_inline_property_storage());
        debug_assert!(!vector.is_narrow(), "a store widens a narrow vector first");
        debug_assert!(index < self.len(vector));
        if vector.is_dictionary() {
            self.dictionary_set(vector, index, value);
            return;
        }
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.set_flat_unchecked(vector, index, value, self.word_type(vector.auxiliary(), index)) };
    }

    /// `set_unchecked` of a vector known not to be a dictionary, whose slot's words have the
    /// encoding of `ty` (`word_type`).
    ///
    /// # Safety
    /// As for `set_unchecked`, and `vector` is not a dictionary.
    #[inline(always)]
    pub(crate) unsafe fn set_flat_unchecked(
        &mut self,
        vector: ValueVec,
        index: usize,
        value: Value,
        ty: SlotType,
    ) {
        debug_assert!(!vector.is_dictionary() && !vector.is_narrow());
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.set_flat_at(vector.start(), index, value, ty) }
    }

    /// `set_flat_unchecked` of the vector whose storage starts at `start` (`ValueVec::start`).
    ///
    /// # Safety
    /// As for `get_flat_at`.
    #[inline(always)]
    pub(crate) unsafe fn set_flat_at(&mut self, start: usize, index: usize, value: Value, ty: SlotType) {
        // SAFETY: the caller's vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked_mut(start + index) = encode_slot(ty, value) }
    }

    /// The address of an initialized flat slot, for a native no-allocation region that holds
    /// the owning object rooted and stores only non-reference values until it exits.
    ///
    /// # Safety
    /// `vector` is a non-inline, non-dictionary vector and `index` is initialized. The caller
    /// must prevent any arena reallocation, collection, or competing access until the pointer
    /// is no longer used.
    #[inline(always)]
    pub(crate) unsafe fn flat_mut_ptr_unchecked(&mut self, vector: ValueVec, index: usize) -> *mut Value {
        debug_assert!(!vector.has_inline_property_storage() && !vector.is_dictionary() && !vector.is_narrow());
        debug_assert!(index < self.len(vector));
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.values.as_mut_ptr().add(vector.start() + index) }
    }

    /// Append `value` past `vector`'s values, which an add of one property makes, moving to
    /// `shape`. Inline: an append that fits its range is the store alone; growing is out of line.
    /// `want` is the capacity the run takes if the add moves it (`grow`), or zero for the next class.
    #[inline(always)] pub(crate) fn push(&mut self, vector: &mut ValueVec, value: Value, shape: u32, want: usize) {
        debug_assert!(!vector.has_inline_property_storage());
        assert!(!vector.is_narrow(), "an add widens a narrow vector first");
        let len = self.len(*vector);
        if vector.is_dictionary() {
            self.dictionaries
                .get_mut(&vector.dictionary_id())
                .expect("object dictionary exists")
                .insert(len as u32, value);
            return;
        }
        if len == Self::ordered_capacity(*vector, len) {
            self.grow(vector, MAX_ARENA_START, want);
            if vector.is_dictionary() {
                self.dictionaries
                    .get_mut(&vector.dictionary_id())
                    .expect("object dictionary exists")
                    .insert(len as u32, value);
                return;
            }
        }
        let ty = self.word_type(shape, len);
        self.values[vector.start() + len] = encode_slot(ty, value);
    }

    pub(crate) fn with_values(&mut self, shape: u32, values: &[Value]) -> ValueVec {
        self.assert_shape_length(shape, values.len());
        let mut vector = ValueVec {
            start: EMPTY_START,
            auxiliary: shape,
        };
        if values.is_empty() {
            return vector;
        }
        if let Some(start) = self.run_of(values) {
            vector.start = start;
            self.recode_run(start as usize, shape, values.len());
        } else {
            vector.use_dictionary(self.allocate_dictionary(values.to_vec()));
        }
        vector
    }

    /// A new arena run for an object of `shape`, holding the `len` words of `source`, an arena
    /// vector outside dictionary storage and not narrow. Its words are copied as they are: no value
    /// is decoded or recoded, so the caller proved `encodings_match` for the source and `shape`
    /// under the current `owner_epoch`. `None` once the arena's positions are spent.
    pub(crate) fn copy_run(&mut self, source: ValueVec, len: usize, shape: u32) -> Option<ValueVec> {
        debug_assert!(len > 0 && source.has_compact_range() && !source.is_narrow());
        debug_assert_eq!(self.len(source), len);
        let target = self.allocate(capacity_class(len), MAX_ARENA_START)?;
        let from = source.start();
        self.values.copy_within(from..from + len, target);
        Some(ValueVec { start: target as u32, auxiliary: shape })
    }

    /// The start of a new arena run holding `values`, of the capacity their count derives
    /// (`ordered_capacity`); `None` once the arena's positions are spent, where storage takes
    /// a dictionary.
    #[inline(always)] pub(crate) fn run_of(&mut self, values: &[Value]) -> Option<u32> {
        let start = self.allocate(capacity_class(values.len()), MAX_ARENA_START)?;
        self.values[start..start + values.len()].copy_from_slice(values);
        Some(start as u32)
    }

    /// A young run (`Nursery`) for a new object, holding the first `len` of `values`, whose
    /// values past `len` are `undefined`, as a run's unused values start; `None` once the
    /// arena's positions are spent. Its owner must be a direct object record, whose survival
    /// the sweep shows. The run's whole capacity is copied, as the smallest class and then the
    /// pairs of values each larger class adds (`capacity_class`), moves of fixed size rather
    /// than a copy of variable length, which is a call.
    #[inline(always)] pub(crate) fn young_run_padded<const N: usize>(&mut self, values: &[Value; N], len: usize, shape: u32) -> Option<u32> {
        let capacity = capacity_class(len);
        debug_assert!(len <= N && capacity <= N && values[len..].iter().all(|value| *value == Value::UNDEFINED));
        let start = self.young_run(capacity)?;
        let run = &mut self.values[start..start + capacity];
        let (first, mut rest) = run.split_at_mut(MIN_CAPACITY);
        <&mut [Value; MIN_CAPACITY]>::try_from(first).unwrap().copy_from_slice(&values[..MIN_CAPACITY]);
        let mut copied = MIN_CAPACITY;
        while let Some((pair, tail)) = rest.split_first_chunk_mut::<CAPACITY_CLASS_STEP>() {
            *pair = *values[copied..].first_chunk::<CAPACITY_CLASS_STEP>().unwrap();
            copied += CAPACITY_CLASS_STEP;
            rest = tail;
        }
        self.recode_run(start, shape, len);
        Some(start as u32)
    }

    /// The start of an empty young run reserved for `slots` values (`Reservation`): the run
    /// of a constructor's receiver whose fields its learned count says will reach `slots`,
    /// bumped from the nursery's range whole, so the adds that follow append in place up to
    /// that count (`reserved_room`) where the class of each length would move the run. Its
    /// owner must be a direct object record, as for every young run. `None`, taking nothing,
    /// where the nursery's range lacks the room: a reservation never grows the arena.
    #[inline(always)]
    pub(crate) fn young_reserved_run(&mut self, slots: usize) -> Option<u32> {
        let capacity = capacity_class(slots);
        let start = self.nursery.top;
        if capacity > self.nursery.end - start {
            return None;
        }
        self.nursery.top = start + capacity;
        self.reservations[Reservation::index(start)] =
            Reservation { start: start as u32, end: (start + capacity) as u32, cycle: self.reservation_cycle };
        Some(start as u32)
    }

    /// Whether `vector`, a young run outside dictionary and narrow storage, may hold `len`
    /// values in place, past the capacity its current length gives it: its reservation
    /// (`young_reserved_run`) still holds, from its start, the whole capacity of `len` values
    /// (`ordered_capacity`), which every later reader takes as the run's.
    #[inline(always)]
    pub(crate) fn reserved_room(&self, vector: ValueVec, len: usize) -> bool {
        if vector.is_narrow() || vector.is_dictionary() || vector.has_inline_property_storage() || !self.holds_young_run(vector) {
            return false;
        }
        let start = vector.start();
        let entry = self.reservations[Reservation::index(start)];
        entry.cycle == self.reservation_cycle && entry.start as usize == start && start + capacity_class(len) <= entry.end as usize
    }

    /// Ends every reservation (`Reservation`): the young runs they name may move or be reused.
    #[inline(always)]
    fn forget_reservations(&mut self) {
        self.reservation_cycle = self.reservation_cycle.wrapping_add(1);
    }

    /// A young run of `capacity`, bumped from the nursery's range.
    #[inline(always)]
    fn young_run(&mut self, capacity: usize) -> Option<usize> {
        let start = self.nursery.top;
        if capacity <= self.nursery.end - start {
            self.nursery.top = start + capacity;
            return Some(start);
        }
        self.young_run_slow(capacity)
    }

    /// A young run for the elements of a new array of `len` values (`ArrayElements::young`):
    /// a header and the values, bumped from the nursery, which grows as `young_run_slow`
    /// grows it; `None` for an empty array or one longer than `YOUNG_ELEMENTS_MAX`, once the
    /// nursery cannot take the run, or where the arena's values may move as it grows, which
    /// would move the run beneath its array. Never an ordinary run: a young array's death
    /// releases nothing, and the collection it survives copies its elements out
    /// (`ArrayElements::leave_nursery`).
    #[inline(always)] pub(crate) fn young_elements(&mut self, len: usize) -> Option<std::ptr::NonNull<Value>> {
        // An empty array holds the shared empty header.
        if len.wrapping_sub(1) >= YOUNG_ELEMENTS_MAX || !self.values.keeps_addresses() {
            return None;
        }
        let capacity = crate::heap::ARRAY_HEADER_VALUES + len;
        let start = self.nursery.top;
        let start = if capacity <= self.nursery.end - start {
            self.nursery.top = start + capacity;
            start
        } else {
            self.grow_nursery(capacity)?
        };
        // SAFETY: the run lies within the arena's initialized values.
        Some(unsafe { std::ptr::NonNull::new_unchecked(self.values.as_mut_ptr().add(start)) })
    }

    /// `young_run` past the nursery's end. A released run of its class comes first: it is
    /// resident already, where the nursery's growth extends the arena. While the free lists
    /// held what a full collection released, a nursery that kept growing extended the arena
    /// past them (acorn-wtb: 43K values more at the peak, 0.4 MB of max RSS). Its owner's
    /// death releases it, as before. Within its range the nursery bumps whatever the free
    /// lists hold: preferring them there let the dead young objects' releases refill them,
    /// and kept new objects off the nursery for good (construct 416 -> 436 Ir). Else a range that ends the arena grows by
    /// `NURSERY_GROWTH`, and an empty one, as a compaction leaves it, starts at the arena's
    /// end; else the run is an ordinary one (`allocate`).
    #[inline(never)]
    fn young_run_slow(&mut self, capacity: usize) -> Option<usize> {
        if !self.free[Self::bucket(capacity)].is_empty() {
            return self.allocate(capacity, MAX_ARENA_START);
        }
        self.grow_nursery(capacity).or_else(|| self.allocate(capacity, MAX_ARENA_START))
    }

    /// A young run of `capacity` past the nursery's end, where its range ends the arena and
    /// can grow by `NURSERY_GROWTH`; an empty range, as a compaction leaves it, starts again
    /// at the arena's end.
    #[inline(never)]
    fn grow_nursery(&mut self, capacity: usize) -> Option<usize> {
        let tail = self.values.len();
        if self.nursery.end == self.nursery.floor {
            self.nursery = Nursery { floor: tail, top: tail, end: tail };
            self.forget_reservations();
        }
        let end = self.nursery.top + capacity.max(NURSERY_GROWTH);
        // Every young run is written whole before it is read, as a range reused after a young
        // cycle holds the dead runs' values: the range's values are not filled, so only the
        // runs taken make its pages resident (1 MB of `undefined` before, for any program
        // that took one young run: V8-v7 RegExp's harness check).
        if self.nursery.end == tail && end <= MAX_ARENA_START && self.values.extend_unfilled(end) {
            self.nursery.end = end;
            let start = self.nursery.top;
            self.nursery.top = start + capacity;
            return Some(start);
        }
        None
    }

    /// The young runs taken since the last collection, as a sweep reads them beside its
    /// releases.
    #[inline(always)] pub(crate) fn young_runs(&self) -> YoungRuns {
        YoungRuns { floor: self.nursery.floor, top: self.nursery.top }
    }

    /// Whether `vector` holds a young run (`Nursery`).
    #[inline(always)] pub(crate) fn holds_young_run(&self, vector: ValueVec) -> bool {
        self.young_runs().hold(vector)
    }

    /// Moves the young run of `vector`, whose owner survives the collection under way, out of
    /// the young range: to a released run of its size class, else down to the nursery's
    /// floor, which rises past it. Survivors must come in the order of their starts, so each
    /// one moved to the floor lands at or below its source.
    pub(crate) fn evacuate_young(&mut self, vector: &mut ValueVec, narrows: bool) {
        debug_assert!(self.holds_young_run(*vector));
        // Survivors land below their sources, perhaps at a reserved run's start.
        self.forget_reservations();
        let length = self.len(*vector);
        let source = vector.start();
        if narrows && length > 0 && self.narrows_to_fewer_words(*vector, length) {
            let mut narrow = *vector;
            narrow.auxiliary |= NARROW_FORM;
            let capacity = Self::ordered_capacity(narrow, length);
            let target = self.young_target(capacity, source);
            // The target lies before the source (a free range or the nursery floor below it),
            // so word `k` is written after values `2k` and `2k + 1` are read only when apart:
            // read the values first.
            let words = self.narrow_words_of(source, length);
            self.values[target..target + words.len()].copy_from_slice(&words);
            self.values[target + words.len()..target + capacity].fill(Value::UNDEFINED);
            narrow.relocate(target);
            *vector = narrow;
            return;
        }
        let capacity = Self::ordered_capacity(*vector, length);
        let target = self.young_target(capacity, source);
        self.values.copy_within(source..source + capacity, target);
        vector.relocate(target);
    }

    /// `vector`, an ordinary run of a plain object whose values all have a narrow form and
    /// fill a smaller class narrow, moved into a released range of the narrow class, its own
    /// range released in turn: what a full collection without compaction does for the runs
    /// it keeps (`Heap::narrow_live_runs`). Without such a range the run narrows in place and
    /// its unused tail is released, where the tail is a class of its own. The arena never
    /// grows for it. `None` changes nothing.
    pub(crate) fn narrow_into_released(&mut self, vector: ValueVec) -> Option<ValueVec> {
        debug_assert!(!vector.is_narrow() && !vector.is_dictionary() && !vector.has_inline_property_storage());
        let length = self.len(vector);
        if length == 0 || !self.narrows_to_fewer_words(vector, length) {
            return None;
        }
        let mut narrow = vector;
        narrow.auxiliary |= NARROW_FORM;
        let capacity = Self::ordered_capacity(narrow, length);
        let tail = Self::ordered_capacity(vector, length) - capacity;
        if self.free[Self::bucket(capacity)].is_empty() && capacity_class(tail) != tail {
            return None;
        }
        let words = self.narrow_words_of(vector.start(), length);
        let target = match self.pop_free(Self::bucket(capacity)) {
            Some(target) => {
                self.released_values -= capacity;
                self.release_held(vector);
                target as usize
            }
            None => {
                // No released range of the narrow class: the run narrows where it is, and the
                // words it no longer fills go free as a range of their own class.
                let source = vector.start();
                self.push_free(Self::bucket(tail), (source + capacity) as u32);
                self.released_values += tail;
                source
            }
        };
        self.values[target..target + words.len()].copy_from_slice(&words);
        self.values[target + words.len()..target + capacity].fill(Value::UNDEFINED);
        narrow.relocate(target);
        Some(narrow)
    }

    /// Where a surviving young run of `capacity` words at `source` moves: a free range of its
    /// class, or the nursery floor, which never passes the source.
    fn young_target(&mut self, capacity: usize, source: usize) -> usize {
        match self.pop_free(Self::bucket(capacity)) {
            Some(target) => {
                self.released_values -= capacity;
                target as usize
            }
            None => {
                let target = self.nursery.floor;
                debug_assert!(target <= source);
                self.nursery.floor += capacity;
                target
            }
        }
    }

    /// The narrow words of the `length` values at `source`, each of which has a narrow form.
    fn narrow_words_of(&self, source: usize, length: usize) -> smallvec::SmallVec<[Value; 16]> {
        (0..narrow_words(length)).map(|word| {
            let low = self.values[source + 2 * word].to_narrow().expect("checked narrow");
            let high = if 2 * word + 1 < length { self.values[source + 2 * word + 1].to_narrow().expect("checked narrow") } else { 0 };
            let (first, second) = if cfg!(target_endian = "little") { (low, high) } else { (high, low) };
            Value::from_bits(u64::from(first) | u64::from(second) << 32)
        }).collect()
    }

    /// Ends a collection's young cycle once its survivors have left (`evacuate_young`): the
    /// dead young runs are free at once.
    pub(crate) fn finish_young_cycle(&mut self) {
        self.nursery.top = self.nursery.floor;
        self.forget_reservations();
    }

    /// The heap references among a vector's values: the edges a collector traces. Numbers,
    /// other immediates and deleted slots are not references. An empty vector without a range,
    /// as every array without named properties has, is answered inline: a mark visit makes
    /// this call per array (V8-v7 Splay: 2.75M calls a run, ~31 Ir each).
    #[inline(always)]
    pub(crate) fn append_heap_references(&self, vector: ValueVec, output: &mut Vec<Value>) {
        if vector.holds_no_values() {
            return;
        }
        self.append_held_heap_references(vector, output);
    }

    #[inline(never)]
    fn append_held_heap_references(&self, vector: ValueVec, output: &mut Vec<Value>) {
        let len = self.len(vector);
        if vector.is_dictionary() {
            let values = &self.dictionaries[&vector.dictionary_id()];
            output.extend(
                (0..len)
                    .filter_map(|slot| values.get(&(slot as u32)).copied())
                    .filter(|value| value.is_heap()),
            );
            return;
        }
        if !self.has_compact_range(vector) {
            return;
        }
        if vector.is_narrow() {
            // SAFETY: each index is one of the live vector's values.
            output.extend((0..len).map(|index| unsafe { self.narrow_get(vector, index) }).filter(|value| value.is_heap()));
            return;
        }
        output.extend(
            self.values[vector.start()..vector.start() + len]
                .iter()
                .copied()
                .filter(|value| value.is_heap()),
        );
    }

    /// An independent copy of `vector`'s storage: the same shape, integrity and values in a
    /// range or dictionary of its own, so neither owner's release, growth or writes reach the
    /// other. Inline storage lives in its owner's header and an empty vector has no range, so
    /// both are returned as they are.
    pub(crate) fn duplicate(&mut self, vector: ValueVec) -> ValueVec {
        let mut copy = vector;
        if vector.has_inline_property_storage() {
            return copy;
        }
        if vector.is_dictionary() {
            let slots = self.dictionaries[&vector.dictionary_id()].clone();
            copy.use_dictionary(self.insert_dictionary(slots));
            return copy;
        }
        if vector.is_narrow() {
            // The copy's owner may hold extras or be an array, which take wide runs only.
            let values = self.narrow_values(vector);
            copy.auxiliary &= !NARROW_FORM;
            copy.start = copy.start & !START_MASK | EMPTY_START;
            if !values.is_empty() {
                match self.run_of(&values) {
                    Some(start) => copy.start = start | (copy.start & !START_MASK),
                    None => copy.use_dictionary(self.allocate_dictionary(values)),
                }
            }
            return copy;
        }
        let capacity = self.capacity(vector);
        if capacity == 0 {
            return copy;
        }
        let len = self.len(vector);
        match self.allocate(capacity, MAX_ARENA_START) {
            Some(start) => {
                self.values.copy_within(vector.start()..vector.start() + len, start);
                copy.relocate(start);
            }
            None => {
                let values = self.decoded_run(vector.start(), len);
                copy.use_dictionary(self.allocate_dictionary(values));
            }
        }
        copy
    }

    /// Frees `vector`'s storage. A young run (`Nursery`) returns with the rest of the young
    /// range when its collection ends, so its release is the range test alone.
    /// The arena's words in use, released ranges included (`QUENCH_TRACE=storage-census`).
    pub(crate) fn resident_words(&self) -> usize {
        self.values.len()
    }

    /// Its length, capacity, released values, held values against the last full collection's
    /// (`GenerationalPacing::arena_growth_reached`) and nursery range, for `QUENCH_TRACE=gc`.
    pub(crate) fn trace_summary(&self) -> String {
        let n = self.nursery;
        let free_classes = self.free.iter().enumerate().filter(|(_, list)| !list.is_empty())
            .map(|(class, list)| format!("{class}:{}", list.len())).collect::<Vec<_>>().join(",");
        format!("arena_len={} arena_cap={} released={} arena_held={} arena_settled={} nursery={}..{}..{} free=[{free_classes}]",
            self.values.len(), self.values.capacity(), self.released_values, self.occupied_values(), self.settled_values,
            n.floor, n.top, n.end)
    }

    #[inline(always)] pub(crate) fn release(&mut self, vector: ValueVec) {
        if !self.young_runs().hold(vector) {
            self.release_ordinary(vector);
        }
    }

    /// `release` of storage that is no young run: that of any record but a direct object
    /// (`young_run_padded`), whose sweeps skip the young range test. Storage that holds no
    /// arena words, as most arrays' and functions' does, is the inlined test alone, where the
    /// call cost a dead array 20 Ir (array-callback); a range or dictionary takes the call
    /// (`release_held`).
    #[inline(always)] pub(crate) fn release_ordinary(&mut self, vector: ValueVec) {
        debug_assert!(!self.holds_young_run(vector));
        // Inline storage and an empty vector without a range hold no arena words.
        if vector.has_inline_property_storage() || (vector.start == EMPTY_START && !vector.is_dictionary()) {
            return;
        }
        self.release_held(vector);
    }

    /// Links the released run at `start` into `bucket`'s list (`FreeRuns`).
    #[inline(always)]
    fn push_free(&mut self, bucket: usize, start: u32) {
        let list = &mut self.free[bucket];
        self.values[start as usize] = Value::from_bits(u64::from(list.head));
        *list = FreeRuns { head: start, len: list.len + 1, unreleased: list.unreleased + 1 };
    }

    /// The last run released into `bucket`, unlinked.
    #[inline(always)]
    fn pop_free(&mut self, bucket: usize) -> Option<u32> {
        let list = self.free[bucket];
        if list.is_empty() {
            return None;
        }
        let next = self.values[list.head as usize].to_bits() as u32;
        // The head is one of the `unreleased` runs at the front while there are any (`FreeRuns`).
        self.free[bucket] = FreeRuns { head: next, len: list.len - 1, unreleased: list.unreleased.saturating_sub(1) };
        Some(list.head)
    }

    /// `bucket`'s released runs, last released first.
    fn free_starts(&self, bucket: usize) -> Vec<u32> {
        let mut starts = Vec::with_capacity(self.free[bucket].len());
        let mut start = self.free[bucket].head;
        for _ in 0..self.free[bucket].len {
            starts.push(start);
            start = self.values[start as usize].to_bits() as u32;
        }
        starts
    }

    #[inline(never)]
    fn release_held(&mut self, vector: ValueVec) {
        if vector.is_dictionary() {
            let id = vector.dictionary_id();
            self.dictionaries.remove(&id);
            self.free_dictionaries.push(id);
            return;
        }
        let capacity = Self::ordered_capacity(vector, self.len(vector));
        if capacity != 0 {
            self.push_free(Self::bucket(capacity), vector.start & START_MASK);
            self.released_values += capacity;
        }
    }

    /// Whether released ranges have grown enough that compaction is worth rewriting every
    /// live object's storage.
    pub(crate) fn compaction_due(&self) -> bool {
        let idle_nursery = self.nursery.end - self.nursery.top;
        self.released_values > 0
            && self.released_values >= (self.values.len() - idle_nursery) / COMPACTION_RELEASED_SHARE_DIVISOR
    }

    /// Values held past the idle nursery and the released runs.
    pub(crate) fn occupied_values(&self) -> usize {
        let idle_nursery = self.nursery.end - self.nursery.top;
        (self.values.len() - idle_nursery).saturating_sub(self.released_values)
    }

    /// Records what a full collection left (`GenerationalPacing::arena_growth_reached`).
    pub(crate) fn settle_after_full(&mut self) {
        self.settled_values = self.occupied_values();
    }

    /// The values the last full collection left held (`settle_after_full`).
    pub(crate) fn settled_values(&self) -> usize {
        self.settled_values
    }

    pub(crate) fn has_compact_range(&self, vector: ValueVec) -> bool {
        vector.has_compact_range()
    }

    /// Slides `vector`'s run down to `target` and answers the words it now takes. Where
    /// `narrows` (a plain object's storage) and every value has a narrow form, the run becomes
    /// a narrow one (`NARROW_FORM`), half the words; its owner stores the vector back.
    pub(crate) fn compact_vector(&mut self, vector: &mut ValueVec, target: usize, narrows: bool) -> usize {
        assert!(
            !vector.is_dictionary(),
            "dictionary storage is not arena-backed"
        );
        let length = self.len(*vector);
        let source = vector.start();
        assert!(
            target <= source,
            "property ranges must compact in source order"
        );
        if narrows && !vector.is_narrow() && length > 0 && self.narrows_to_fewer_words(*vector, length) {
            let mut narrow = *vector;
            narrow.auxiliary |= NARROW_FORM;
            let capacity = Self::ordered_capacity(narrow, length);
            // Words are written in order, each from values at or past it: word `k` reads
            // values `2k` and `2k + 1` of a source at or past the target.
            for word in 0..narrow_words(length) {
                let low = self.values[source + 2 * word].to_narrow().expect("checked narrow");
                let high = if 2 * word + 1 < length { self.values[source + 2 * word + 1].to_narrow().expect("checked narrow") } else { 0 };
                let (first, second) = if cfg!(target_endian = "little") { (low, high) } else { (high, low) };
                self.values[target + word] = Value::from_bits(u64::from(first) | u64::from(second) << 32);
            }
            self.values[target + narrow_words(length)..target + capacity].fill(Value::UNDEFINED);
            narrow.relocate(target);
            *vector = narrow;
            self.narrowed_words += capacity;
            return capacity;
        }
        let capacity = self.capacity(*vector);
        let words = if vector.is_narrow() { narrow_words(length) } else { length };
        if vector.is_narrow() { self.narrowed_words += capacity; }
        self.values.copy_within(source..source + words, target);
        self.values[target + words..target + capacity].fill(Value::UNDEFINED);
        vector.relocate(target);
        capacity
    }

    /// Whether `length` values fill a smaller class as a narrow run (census).
    pub(crate) fn narrow_class_smaller(&self, length: usize) -> bool {
        capacity_class(narrow_words(length)) < capacity_class(length)
    }

    /// Whether the `length` values at `source` all have a narrow form and fill a smaller
    /// class as a narrow run than as values.
    fn narrows_to_fewer_words(&self, vector: ValueVec, length: usize) -> bool {
        let source = vector.start();
        capacity_class(narrow_words(length)) < capacity_class(length)
            && !self.widened_shapes.contains(&vector.auxiliary())
            && self.values[source..source + length].iter().all(|value| value.to_narrow().is_some())
    }

    pub(crate) fn finish_compaction(&mut self, length: usize) {
        self.values.truncate(length);
        if crate::trace::Channel::Gc.on() {
            // Narrow-run words hold value pairs, not values: the census counts them apart and
            // leaves them out of the wide counts only in sum (they read as numbers).
            let wide = self.values[..length].iter().filter(|value| value.to_narrow().is_none()).count();
            let numbers = self.values[..length].iter().filter(|value| value.to_narrow().is_none() && value.as_number().is_some()).count();
            eprintln!("arena census: compacted={length} narrow_run_words={} wide={wide} wide_numbers={numbers} (narrow-run words included)", self.narrowed_words);
        }
        self.narrowed_words = 0;
        // A burst's growth stays resident past the compaction that frees it: keep the room
        // the next doubling would give the live values, and return the rest.
        self.values.shrink_to(length.saturating_mul(COMPACTED_CAPACITY_FACTOR));
        self.free = [FreeRuns::EMPTY; BUCKETS];
        self.released_values = 0;
        // Compaction moved every live run below `length`, young ones too.
        self.nursery = Nursery::default();
        self.forget_reservations();
    }

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

    /// Moves `vector` to a range of the next size class. A young run's owner is a newborn
    /// object (`Nursery`), so its next range is a young run too: grown in place when it ends
    /// the young range, as a new object's runs mostly do while its constructor adds
    /// properties, else bumped. An ordinary range for it left the young one dead in the
    /// nursery until the collection, and pushed the arena's end past the nursery's.
    ///
    /// `want` is the capacity an add that moves the run predicts for it. A young run takes it
    /// in one jump where the nursery has the room, and reserves the tail past the class
    /// (`Reservation`), so the adds up to `want` append in place. An ordinary run takes the
    /// next class alone: its capacity is a fact of its length (`ordered_capacity`), so a tail
    /// it could not release would be lost to the free lists.
    #[inline(never)]
    fn grow(&mut self, vector: &mut ValueVec, max_start: usize, want: usize) {
        assert!(!vector.is_narrow(), "growth widens a narrow vector first");
        let len = self.len(*vector);
        let capacity = self.capacity(*vector);
        let class = grown_capacity(capacity, capacity + 1, GrowthPolicy::SizeClass);
        let young = self.holds_young_run(*vector);
        // A reserved run (`Reservation`) takes the next class from its reservation.
        if young && self.reserved_room(*vector, capacity + 1) {
            return;
        }
        // The predicted tail is bumped from the nursery's room only, so it never grows the arena.
        let in_place = young && vector.start() + capacity == self.nursery.top;
        let room = if in_place { vector.start() } else { self.nursery.top };
        let predicted = young && want > class && room + want <= self.nursery.end;
        let next = if predicted { want } else { class };
        if in_place && vector.start() + next <= self.nursery.end {
            self.nursery.top = vector.start() + next;
            self.reserve_predicted_run(vector.start(), next, class);
            return;
        }
        let start = if young { self.young_run(next) } else { self.allocate(next, max_start) };
        let Some(start) = start else {
            let values = if self.has_compact_range(*vector) {
                self.decoded_run(vector.start(), len)
            } else {
                vec![Value::UNDEFINED; len]
            };
            self.release(*vector);
            let id = self.allocate_dictionary(values);
            vector.use_dictionary(id);
            return;
        };
        if predicted {
            self.reserve_predicted_run(start, next, class);
        }
        if len != 0 {
            self.values
                .copy_within(vector.start()..vector.start() + len, start);
            self.release(*vector);
        }
        vector.start = start as u32 | (vector.start & !START_MASK);
    }

    /// Records the young run at `start` as `capacity` values long, past its `class`, so its owner
    /// appends in place up to that length (`reserved_room`). The tail is the owner's alone until
    /// the young cycle ends, which `forget_reservations` marks, and an entry another run evicts
    /// only loses that room.
    #[inline(always)]
    fn reserve_predicted_run(&mut self, start: usize, capacity: usize, class: usize) {
        if capacity > class {
            self.reservations[Reservation::index(start)] =
                Reservation { start: start as u32, end: (start + capacity) as u32, cycle: self.reservation_cycle };
        }
    }

    #[cfg(test)]
    pub(crate) fn migrate_to_dictionary_for_test(&mut self, vector: &mut ValueVec) {
        self.grow(vector, 0, 0);
    }

    pub(crate) fn vector_stats(&self, vector: ValueVec) -> (usize, usize) {
        (self.len(vector), self.capacity(vector))
    }

    /// The address of the first arena value, which a flat run's position offsets
    /// (`staged::Table::PropertyValues`). Valid until the arena grows or compacts.
    #[inline(always)] pub(crate) fn values_base(&self) -> *const Value {
        self.values.as_ptr()
    }

    #[inline]
    pub(crate) fn len(&self, vector: ValueVec) -> usize {
        self.shape_lengths[vector.auxiliary() as usize] as usize
    }

    #[inline] pub(crate) fn assert_shape_length(&self, shape: u32, length: usize) {
        debug_assert_eq!(length, self.shape_lengths[shape as usize] as usize, "property values must match shape");
    }

    pub(crate) fn capacity(&self, vector: ValueVec) -> usize {
        if vector.has_inline_property_storage() {
            return 0;
        }
        if vector.is_dictionary() {
            return self.dictionaries[&vector.dictionary_id()].capacity();
        }
        Self::ordered_capacity(vector, self.len(vector))
    }

    /// The capacity of the range of an ordered (neither inline nor dictionary) `vector` of
    /// `len` values: ranges are allocated at the size classes of `capacity_class`, and an
    /// empty vector without a range has none.
    #[inline(always)]
    fn ordered_capacity(vector: ValueVec, len: usize) -> usize {
        // A narrow run's capacity is in words, the class of the words its values fill.
        let len = if vector.is_narrow() { narrow_words(len) } else { len };
        match (len, vector.start() != EMPTY_START as usize) {
            (0, false) => 0,
            (0, true) => MIN_CAPACITY,
            (length, _) => capacity_class(length),
        }
    }

    /// Whether `vector` of `len` values still names its storage once its last value is
    /// dropped by a shorter shape: inline values, or an ordered range whose capacity, which
    /// derives from the length, is the same for one value fewer.
    pub(crate) fn drops_last_in_place(vector: ValueVec, len: usize) -> bool {
        vector.has_inline_property_storage()
            || (!vector.is_dictionary() && !vector.is_narrow()
                && len > 0
                && Self::ordered_capacity(vector, len - 1) == Self::ordered_capacity(vector, len))
    }

    /// Whether `vector`, an arena vector outside dictionary storage of `len` values, has room
    /// for one more value without moving its range (`push`'s `grow`).
    #[inline(always)] pub(crate) fn appends_in_place(vector: ValueVec, len: usize) -> bool {
        !vector.is_narrow() && len < Self::ordered_capacity(vector, len)
    }

    /// `push` of `value` to `vector` of `len` values, moving to `shape`, where `appends_in_place`
    /// holds: the store alone.
    ///
    /// # Safety
    /// `vector` is an arena vector outside dictionary storage of `len` values, whose range has
    /// room for one more (`appends_in_place`).
    #[inline(always)]
    pub(crate) unsafe fn push_in_place(&mut self, vector: ValueVec, len: usize, value: Value, shape: u32) {
        debug_assert!(!vector.has_inline_property_storage() && !vector.is_dictionary());
        debug_assert!(len == self.len(vector) && Self::appends_in_place(vector, len));
        // SAFETY: the range from `start` holds its capacity, past `len`, reserved in `values`.
        unsafe { self.set_flat_at(vector.start(), len, value, self.word_type(shape, len)) };
    }

    /// `push` of `value` to `vector` of `len` values, moving to `shape`, where its reservation
    /// has the room (`reserved_room` of `len + 1`): the store alone.
    ///
    /// # Safety
    /// `vector` is a young arena run outside dictionary storage of `len` values whose
    /// reservation holds the capacity of `len + 1`.
    #[inline(always)]
    pub(crate) unsafe fn push_reserved(&mut self, vector: ValueVec, len: usize, value: Value, shape: u32) {
        debug_assert!(len == self.len(vector) && self.reserved_room(vector, len + 1));
        // SAFETY: the reservation holds the range from `start` past `len`, reserved in `values`.
        unsafe { self.set_flat_at(vector.start(), len, value, self.word_type(shape, len)) };
    }

    /// A range of `capacity`: a released one of its size class, else, out of line, one past
    /// the arena's end (`allocate_fresh`).
    #[inline(always)]
    fn allocate(&mut self, capacity: usize, max_start: usize) -> Option<usize> {
        let bucket = Self::bucket(capacity);
        if let Some(start) = self.pop_free(bucket) {
            self.released_values -= capacity;
            return Some(start as usize);
        }
        self.allocate_fresh(capacity, max_start)
    }

    #[inline(never)]
    fn allocate_fresh(&mut self, capacity: usize, max_start: usize) -> Option<usize> {
        let start = self.values.len();
        if start >= max_start {
            return None;
        }
        let required = start.checked_add(capacity)?;
        if !self.values.resize(required) { return None; }
        Some(start)
    }

    fn allocate_dictionary(&mut self, values: Vec<Value>) -> u32 {
        self.insert_dictionary(
            values
                .into_iter()
                .enumerate()
                .map(|(slot, value)| (slot as u32, value))
                .collect(),
        )
    }

    fn insert_dictionary(&mut self, slots: FxHashMap<u32, Value>) -> u32 {
        let id = self.free_dictionaries.pop().unwrap_or_else(|| {
            let id = self.next_dictionary;
            self.next_dictionary = id
                .checked_add(1)
                .expect("property dictionary table exhausted");
            assert!(id <= START_MASK, "property dictionary table exhausted");
            id
        });
        self.dictionaries.insert(id, slots);
        id
    }

    /// The free list of a size class (`capacity_class`): `MIN_CAPACITY` and its step first,
    /// then four per doubling, the power of two and the quarter steps above it.
    #[inline(always)]
    fn bucket(capacity: usize) -> usize {
        debug_assert_eq!(capacity_class(capacity), capacity, "a range capacity is a size class");
        let bucket = if capacity < 2 * MIN_CAPACITY {
            usize::from(capacity != MIN_CAPACITY)
        } else {
            let octave = capacity.ilog2();
            let floor = 1usize << octave;
            2 + 4 * (octave - (2 * MIN_CAPACITY).ilog2()) as usize + (capacity - floor) / (floor / 4)
        };
        assert!(bucket < BUCKETS, "property arena bucket overflow");
        bucket
    }

    /// The size class `bucket` holds: the inverse of `bucket`, so a released run's length
    /// follows from its list alone.
    #[inline] fn bucket_capacity(bucket: usize) -> usize {
        match bucket {
            0 => MIN_CAPACITY,
            1 => MIN_CAPACITY + MIN_CAPACITY / 2,
            _ => {
                let octave = 3 + (bucket - 2) / 4;
                let floor = 1usize << octave;
                floor + (bucket - 2) % 4 * (floor / 4)
            }
        }
    }

    /// Gives back the whole pages inside each released run (`return_free_pages`'s contract).
    /// A run's first value links its free list and stays; the values after it are dead, so
    /// every page wholly inside the run is released, and a page that shares a value with a
    /// live range or another run is kept. A run already given back is not given back again
    /// (`FreeRuns::unreleased`).
    pub(crate) fn return_free_pages(&mut self) {
        let bytes = self.values.capacity() * size_of::<Value>();
        if bytes < FREE_PAGES_MIN_BYTES || self.released_values * 2 <= self.values.len() {
            return;
        }
        for bucket in 0..BUCKETS {
            let list = self.free[bucket];
            if list.unreleased == 0 {
                continue;
            }
            let capacity = Self::bucket_capacity(bucket);
            let mut start = list.head;
            for _ in 0..list.unreleased {
                let next = self.values[start as usize].to_bits() as u32;
                let first = start as usize + 1;
                self.values.release_pages(first, start as usize + capacity);
                start = next;
            }
            self.free[bucket].unreleased = 0;
        }
    }
}

#[cfg(test)]
mod tests;
