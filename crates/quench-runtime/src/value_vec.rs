use crate::Value;
use crate::staged::object::Rep;
use crate::typed_ir::SlotType;
use crate::vm::switches;
use rustc_hash::FxHashMap;

mod arena_snapshot; mod buffer; mod compaction; mod plan_values; mod shape_types;
mod subscribe; mod young;
use buffer::PropertyBuffer;
pub(crate) use buffer::NATIVE_PROPERTY_BUFFER_LAYOUT;

/// The word of an arena slot that holds `value`, where the slot's owner has type `owner`: the one
/// place a stored word is encoded. A slot of `Rep::Float64` holds its Number as the IEEE double (an
/// integer as the integral double; NaN is already canonical, so the word is a valid `Value` the
/// collector reads as a non-reference); any other slot holds the `Value` as it is.
#[inline(always)]
pub(crate) fn encode_slot(owner: SlotType, value: Value) -> Value {
    match owner.rep() {
        Rep::Float64 => value.as_int().map_or(value, |int| Value::from_bits(f64::from(int).to_bits())),
        Rep::Int32 | Rep::Tagged => value,
    }
}

/// The `Value` that the arena word `word` holds, where the slot's owner has type `owner`: the one
/// place a stored word is decoded, `encode_slot`'s inverse. A double word is a Number as
/// `Value::number` makes it (an integral double in int32 range is an int32, `-0` stays a double);
/// a word that is no Number (the deleted marker a removed property leaves) is itself.
#[inline(always)]
pub(crate) fn decode_slot(owner: SlotType, word: Value) -> Value {
    match owner.rep() {
        Rep::Float64 => word.as_number().map_or(word, Value::number),
        Rep::Int32 | Rep::Tagged => word,
    }
}

/// The `Value` of a word a staged read took from an arena run, whose owner the reader does not know
/// (`decode_slot` of `read_type`).
#[inline(always)]
pub(crate) fn decode_staged_word(word: u64) -> Value {
    decode_slot(read_type(), Value::from_bits(word))
}

/// The type a word of a slot is decoded as where its owner is unknown to the reader (`decode_slot`):
/// Number where slots of that type hold raw doubles (`switches::float_slots`), whose decoding is the
/// identity on every canonical `Value`, so it reads the word of any owner; else the type of tagged words.
#[inline(always)]
fn read_type() -> SlotType {
    if switches::float_slots() { SlotType::Number } else { SlotType::Tagged }
}

pub(crate) const START_MASK: u32 = 0x3fff_ffff;
const NON_EXTENSIBLE: u32 = 1 << 30;
const FROZEN: u32 = 1 << 31;
pub(crate) const DICTIONARY_STORAGE: u32 = 1 << 31;
/// An object whose property access is not the ordinary one its shape describes (a module
/// namespace, an arguments object): set once when it becomes so, and never cleared, it keeps
/// the object's exact auxiliary (`exact_auxiliary`) from every shape a cache records, so a
/// shape match proves the object ordinary with no further test.
pub(crate) const EXOTIC_ACCESS: u32 = 1 << 30;
/// An object record's storage form, kept in its auxiliary word beside the shape, so the word a
/// field hit loads to prove the shape also tells where the values live, with no load of the
/// header or test of the payload (`staged::object::storage_form`): its values are inline
/// (`Object::meta`'s inline flag) ...
pub(crate) const INLINE_FORM: u32 = 1 << 29;
/// ... or behind extras (`Object::payload`'s extras tag). Every writer of either keeps these
/// current (`Object::storage_form`); no shape a cache records carries them (`exact_auxiliary`
/// and `auxiliary` mask them off).
pub(crate) const EXTRAS_FORM: u32 = 1 << 28;
/// ... or in a narrow run (`ValueArena`): an arena run of narrow values (`Value::to_narrow`),
/// two to a word, slot `i` in the `i % 2` half (the lower first) of word `start + i / 2`,
/// which a plain object's values take where all have a narrow form (`ValueArena::compact_vector`).
/// Its payload word carries `NARROW_RUN_TAG` beside the start.
pub(crate) const NARROW_FORM: u32 = 1 << 27;
pub(crate) const STORAGE_FORM: u32 = INLINE_FORM | EXTRAS_FORM | NARROW_FORM;
const SHAPE_MASK: u32 = !(DICTIONARY_STORAGE | EXOTIC_ACCESS | STORAGE_FORM);
const _: () = assert!(STORAGE_FORM & (DICTIONARY_STORAGE | EXOTIC_ACCESS) == 0 && INLINE_FORM & EXTRAS_FORM == 0);
/// The narrow values one word of a narrow run holds: a `Value`'s two `u32` halves.
const NARROW_PER_WORD: usize = 2;
/// The factor of one capacity doubling, and the size classes each doubling splits into
/// (`capacity_class`): quarter steps.
const DOUBLING: usize = 2;
const CLASSES_PER_DOUBLING: usize = 4;
/// The free-list buckets below the first doubling: `MIN_CAPACITY` and its step.
const BUCKETS_BELOW_DOUBLING: usize = 2;
/// The words a narrow run of `len` values fills.
#[inline(always)] const fn narrow_words(len: usize) -> usize {
    len.div_ceil(NARROW_PER_WORD)
}
/// The narrow value at `slot` of the narrow run whose first word is `words`.
///
/// # Safety
/// `words` addresses a narrow run holding `slot`.
#[inline(always)] unsafe fn narrow_slot(words: *const Value, slot: usize) -> u32 {
    let half = if cfg!(target_endian = "little") { slot % NARROW_PER_WORD } else { 1 - slot % NARROW_PER_WORD };
    // SAFETY: the caller's run holds the word; a `Value` is a `u64`, two `u32` halves.
    unsafe { words.add(slot / NARROW_PER_WORD).cast::<u32>().add(half).read() }
}
const EMPTY_START: u32 = START_MASK;
const INLINE_PROPERTY_START: u32 = EMPTY_START - 1;
const MAX_ARENA_START: usize = INLINE_PROPERTY_START as usize;
/// Properties an object keeps in its record. The Score lane (`v2-cloud-c`, a2deff57a) runs
/// three for RayTrace's vectors at the cost of max RSS (Splay 162 -> 204 MB); the integration
/// branch keeps memory-first defaults (the owner's direction, 2026-10-17 and 2026-10-18).
pub(crate) const INLINE_PROPERTY_COUNT: usize = 2;
/// The smallest arena range. An object reaches the arena by outgrowing its inline slots, or
/// by storing a value they cannot hold before that; a range for its first values would regrow
/// as it grows past them.
const MIN_CAPACITY: usize = (INLINE_PROPERTY_COUNT + 1).next_power_of_two();
/// Range capacities are the size classes `capacity_class` derives, one free list each.
const BUCKETS: usize = 128;

/// A size class's released runs, linked through each run's first value: a released run's
/// values are dead, so its list costs no memory of its own. As a `Vec<u32>` per class, the
/// lists held 2.2 MB at TypeScript's peak, the full collection that releases millions of runs.
#[derive(Clone, Copy, Debug)]
struct FreeRuns {
    head: u32,
    len: u32,
}

impl FreeRuns {
    const EMPTY: Self = Self { head: u32::MAX, len: 0 };

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
#[inline] const fn capacity_class(len: usize) -> usize {
    if len <= MIN_CAPACITY {
        return MIN_CAPACITY;
    }
    if len <= DOUBLING * MIN_CAPACITY {
        let step = MIN_CAPACITY + MIN_CAPACITY / DOUBLING;
        return if len <= step { step } else { DOUBLING * MIN_CAPACITY };
    }
    let quarter = len.next_power_of_two() / DOUBLING / CLASSES_PER_DOUBLING;
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

/// Compact per-object shape, integrity, and property-storage metadata.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueVec {
    start: u32,
    auxiliary: u32,
}

/// `ValueVec::start` bits that hold the storage position: an arena offset, a dictionary id,
/// or one of the empty and inline markers.
pub(crate) const VALUE_VEC_START_BITS: u32 = START_MASK;
/// `ValueVec::start` bits that hold the object's integrity level.
pub(crate) const VALUE_VEC_INTEGRITY_BITS: u32 = NON_EXTENSIBLE | FROZEN;
/// The integrity bit of a frozen object, whose elements no store replaces.
pub(crate) const VALUE_VEC_FROZEN: u32 = FROZEN;

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
        assert!(value < SHAPE_MASK, "object shape table exhausted");
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

    /// The arena run a concurrent marker reads for this vector, as `(start, len)` from the
    /// shape lengths and arena length it snapshotted (`Heap::begin_cycle`): `(0, 0)` for no
    /// values or inline storage, `None` (the mutator traces it) for dictionary storage or a
    /// shape or run past the snapshot. The vector may be a torn read: the bounds keep the run
    /// inside the snapshot, whatever it holds.
    pub(crate) fn concurrent_run(self, shape_lengths: &[u32], arena_len: usize) -> Option<(usize, usize)> {
        // The marker reads runs as values; a narrow run's halves are not.
        if self.is_dictionary() || self.is_narrow() {
            return None;
        }
        if self.has_inline_property_storage() || self.start() == EMPTY_START as usize {
            return Some((0, 0));
        }
        let len = *shape_lengths.get(self.auxiliary() as usize)? as usize;
        let start = self.start();
        (start.checked_add(len)? <= arena_len).then_some((start, len))
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
    dictionaries: FxHashMap<u32, FxHashMap<u32, Value>>,
    free_dictionaries: Vec<u32>,
    next_dictionary: u32,
    nursery: Nursery,
    /// Values the arena held, past its idle nursery and released runs, after the last full
    /// collection (`GenerationalPacing::arena_growth_reached`).
    settled_values: usize,
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
            dictionaries: FxHashMap::default(),
            free_dictionaries: Vec::new(),
            next_dictionary: 0,
            nursery: Nursery::default(),
            settled_values: 0,
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

    /// What a concurrent marker reads of the arena: its values' base and length, and a copy
    /// of the shape lengths, all as of now. Valid while the values stay in place
    /// (`values_stay_in_place`) and nothing compacts them.
    pub(crate) fn concurrent_snapshot(&self) -> (*const Value, usize, Box<[u32]>) {
        (self.values.as_ptr(), self.values.len(), self.shape_lengths.clone().into_boxed_slice())
    }

    /// Whether the property values never move to another allocation (`PropertyBuffer::stays_in_place`).
    pub(crate) fn values_stay_in_place(&self) -> bool {
        self.values.stays_in_place()
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
    }

    pub(crate) fn register_shape(&mut self, shape: u32, length: usize) {
        self.shape_readers.retire(shape);
        assert!(shape <= SHAPE_MASK, "object shape table exhausted");
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
        self.slot_readers.retire(shape);
        self.owner_types[index] = SlotType::None;
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
    }

    /// Whether slots of Number owners hold raw doubles.
    #[inline(always)] pub(crate) fn float_slots(&self) -> bool {
        self.float
    }

    /// Whether the word of `slot` of an object of `shape` is a raw double, which only a native reader
    /// that knows its owner's type may read (`Rep::Float64`).
    #[inline(always)] pub(crate) fn holds_raw_doubles(&self, shape: u32, slot: usize) -> bool {
        self.word_type(shape, slot).rep() == Rep::Float64
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
        if previous != next {
            self.shape_readers.retire(previous);
        }
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
        let half = if cfg!(target_endian = "little") { index % NARROW_PER_WORD } else { 1 - index % NARROW_PER_WORD };
        // SAFETY: the caller's run was allocated from `values` and remains reserved.
        unsafe { self.values.as_mut_ptr().add(vector.start() + index / NARROW_PER_WORD).cast::<u32>().add(half).write(narrow) };
    }

    /// The values of the narrow vector `vector`, decoded: what a store that widens it copies.
    pub(crate) fn narrow_values(&self, vector: ValueVec) -> Vec<Value> {
        // SAFETY: each index is one of the live vector's values.
        (0..self.len(vector)).map(|index| unsafe { self.narrow_get(vector, index) }).collect()
    }

    /// `vector`, a narrow vector, widened in place of its owner: an ordinary run holding its
    /// values decoded (`run_of`), or a dictionary once the arena's positions are spent; its
    /// narrow run is released. The owner stores the result (`Object::set_properties`).
    pub(crate) fn widen(&mut self, vector: ValueVec) -> ValueVec {
        debug_assert!(vector.is_narrow() && !self.holds_young_run(vector));
        self.widened_shapes.insert(vector.auxiliary());
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
    #[inline(always)] pub(crate) fn push(&mut self, vector: &mut ValueVec, value: Value, shape: u32) {
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
            self.grow(vector, MAX_ARENA_START);
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

    /// The start of a new arena run holding `values`, of the capacity their count derives
    /// (`ordered_capacity`); `None` once the arena's positions are spent, where storage takes
    /// a dictionary.
    #[inline(always)] pub(crate) fn run_of(&mut self, values: &[Value]) -> Option<u32> {
        let start = self.allocate(capacity_class(values.len()), MAX_ARENA_START)?;
        self.values[start..start + values.len()].copy_from_slice(values);
        Some(start as u32)
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
            return self.duplicate_narrow(vector);
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

    /// The copy of the narrow vector `vector`: its values widen into a run of their own, or a
    /// dictionary where no run fits. Its owner may hold extras or be an array, which take wide
    /// runs only.
    fn duplicate_narrow(&mut self, vector: ValueVec) -> ValueVec {
        let mut copy = vector;
        let values = self.narrow_values(vector);
        copy.auxiliary &= !NARROW_FORM;
        copy.start = copy.start & !START_MASK | EMPTY_START;
        if !values.is_empty() {
            match self.run_of(&values) {
                Some(start) => copy.start = start | (copy.start & !START_MASK),
                None => copy.use_dictionary(self.allocate_dictionary(values)),
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
        *list = FreeRuns { head: start, len: list.len + 1 };
    }

    /// The last run released into `bucket`, unlinked.
    #[inline(always)]
    fn pop_free(&mut self, bucket: usize) -> Option<u32> {
        let list = self.free[bucket];
        if list.is_empty() {
            return None;
        }
        let next = self.values[list.head as usize].to_bits() as u32;
        self.free[bucket] = FreeRuns { head: next, len: list.len - 1 };
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

    /// Values held past the idle nursery and the released runs.
    pub(crate) fn occupied_values(&self) -> usize {
        let idle_nursery = self.nursery.end - self.nursery.top;
        (self.values.len() - idle_nursery).saturating_sub(self.released_values)
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
            + self.slot_readers.memory_bytes()
            + self.shape_readers.memory_bytes()
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
    #[inline(never)]
    fn grow(&mut self, vector: &mut ValueVec, max_start: usize) {
        assert!(!vector.is_narrow(), "growth widens a narrow vector first");
        let len = self.len(*vector);
        let capacity = self.capacity(*vector);
        let next = capacity_class(capacity + 1);
        let young = self.holds_young_run(*vector);
        if young && vector.start() + capacity == self.nursery.top && vector.start() + next <= self.nursery.end {
            self.nursery.top = vector.start() + next;
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
        if len != 0 {
            self.values
                .copy_within(vector.start()..vector.start() + len, start);
            self.release(*vector);
        }
        vector.start = start as u32 | (vector.start & !START_MASK);
    }

    #[cfg(test)]
    pub(crate) fn migrate_to_dictionary_for_test(&mut self, vector: &mut ValueVec) {
        self.grow(vector, 0);
    }

    pub(crate) fn vector_stats(&self, vector: ValueVec) -> (usize, usize) {
        (self.len(vector), self.capacity(vector))
    }

    /// The address of the first arena value, which a flat run's position offsets
    /// (`staged::Table::PropertyValues`). Valid until the arena grows or compacts.
    #[inline(always)] pub(crate) fn values_base(&self) -> *const Value {
        self.values.as_ptr()
    }

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
        let bucket = if capacity < DOUBLING * MIN_CAPACITY {
            usize::from(capacity != MIN_CAPACITY)
        } else {
            let octave = capacity.ilog2();
            let floor = 1usize << octave;
            BUCKETS_BELOW_DOUBLING
                + CLASSES_PER_DOUBLING * (octave - (DOUBLING * MIN_CAPACITY).ilog2()) as usize
                + (capacity - floor) / (floor / CLASSES_PER_DOUBLING)
        };
        assert!(bucket < BUCKETS, "property arena bucket overflow");
        bucket
    }
}

#[cfg(test)]
mod tests;
