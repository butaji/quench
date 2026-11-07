use crate::Value;
use crate::typed_ir::SlotType;
use rustc_hash::FxHashMap;

mod buffer;
mod target_shapes;
pub(crate) use target_shapes::TargetShape;
use buffer::PropertyBuffer;
pub(crate) use buffer::NATIVE_PROPERTY_BUFFER_LAYOUT;

pub(crate) const START_MASK: u32 = 0x3fff_ffff;
const NON_EXTENSIBLE: u32 = 1 << 30;
const FROZEN: u32 = 1 << 31;
pub(crate) const DICTIONARY_STORAGE: u32 = 1 << 31;
/// An object whose property access is not the ordinary one its shape describes (a module
/// namespace, an arguments object): set once when it becomes so, and never cleared, it keeps
/// the object's exact auxiliary (`exact_auxiliary`) from every shape a cache records, so a
/// shape match proves the object ordinary with no further test.
pub(crate) const EXOTIC_ACCESS: u32 = 1 << 30;
const SHAPE_MASK: u32 = !(DICTIONARY_STORAGE | EXOTIC_ACCESS);
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
/// Range capacities are the size classes `capacity_class` derives: powers of two from
/// `MIN_CAPACITY` and the three-quarter steps between them, one free list each.
const BUCKETS: usize = 64;
/// The capacity of a range holding `len` values: the smallest power of two from
/// `MIN_CAPACITY`, or three-quarter step between two (6, 12, 24, ...), that holds them. The
/// half steps leave a range at most a third unused where powers of two left it half unused
/// (typescript-lib: 1.6M of 6.7M live slots idle).
const fn capacity_class(len: usize) -> usize {
    if len <= MIN_CAPACITY {
        return MIN_CAPACITY;
    }
    let power = len.next_power_of_two();
    let step = power / 2 + power / 4;
    if len <= step { step } else { power }
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
/// cycle of new runs.
const NURSERY_GROWTH: usize = 1 << 10;
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
    floor: usize,
    top: usize,
}

impl YoungRuns {
    /// Whether `vector` holds one of these runs.
    #[inline(always)]
    pub(crate) fn hold(self, vector: ValueVec) -> bool {
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
    pub(crate) const fn into_parts(self) -> (u32, u32) {
        (self.start, self.auxiliary)
    }

    pub(crate) const fn from_parts(start: u32, auxiliary: u32) -> Self {
        Self { start, auxiliary }
    }

    pub(crate) const fn new() -> Self {
        Self {
            start: EMPTY_START,
            auxiliary: 0,
        }
    }

    pub(crate) const fn inline_property_storage(shape: u32) -> Self {
        Self {
            start: INLINE_PROPERTY_START,
            auxiliary: shape,
        }
    }

    pub(crate) const fn auxiliary(self) -> u32 {
        self.auxiliary & SHAPE_MASK
    }

    /// The auxiliary word with its dictionary and exotic bits: equal to `auxiliary` exactly
    /// when the vector is neither in dictionary storage nor an exotic object's, so one
    /// comparison with a recorded shape proves all three.
    pub(crate) const fn exact_auxiliary(self) -> u32 {
        self.auxiliary
    }

    pub(crate) fn has_inline_property_storage(self) -> bool {
        self.start() as u32 == INLINE_PROPERTY_START
    }

    pub(crate) fn has_compact_range(self) -> bool {
        !self.has_inline_property_storage()
            && !self.is_dictionary()
            && self.start() != EMPTY_START as usize
    }

    pub(crate) fn set_auxiliary(&mut self, value: u32) {
        // The last shape stays unused, so a dictionary vector's exact auxiliary
        // (`exact_auxiliary`) never equals the all-ones sentinel caches record as "none".
        assert!(value < SHAPE_MASK, "object shape table exhausted");
        self.auxiliary = (self.auxiliary & !SHAPE_MASK) | value;
    }

    pub(crate) fn is_dictionary(self) -> bool {
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

    pub(crate) fn is_extensible(self) -> bool {
        self.start & NON_EXTENSIBLE == 0
    }

    pub(crate) fn set_extensible(&mut self, value: bool) {
        if value {
            self.start &= !NON_EXTENSIBLE;
        } else {
            self.start |= NON_EXTENSIBLE;
        }
    }

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

    pub(crate) fn preserve_integrity_from(&mut self, previous: Self) {
        self.start = (self.start & !(NON_EXTENSIBLE | FROZEN))
            | (previous.start & (NON_EXTENSIBLE | FROZEN));
    }

    fn start(self) -> usize {
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
    free: [Vec<u32>; BUCKETS],
    /// Values held by the ranges on `free`.
    released_values: usize,
    shape_lengths: Vec<u32>,
    /// The slot owners of every shape (`ValueArena::slot_owner`): each shape's chain starts at
    /// its `shape_chains` index in `chain_owners`, which names, per slot, the shape that added
    /// it. A child that appends a slot at its parent's chain end shares that chain.
    shape_chains: Vec<u32>,
    chain_owners: Vec<u32>,
    /// Per owner shape, compact observed content types. Recording writes joins
    /// them; native raw writes can bypass recording, so this is not a proof.
    owner_types: Vec<SlotType>,
    slot_readers: crate::code_dependencies::CodeReaders,
    targets: target_shapes::TargetShapes,
    /// Numeric shape departures only; heap admission, exotic access and physical
    /// storage remain separate proof obligations.
    shape_readers: crate::code_dependencies::CodeReaders,
    /// Whether writes join their values' classes (`ValueArena::track`): only a runtime whose
    /// native code reads representations keeps them.
    tracking: bool,
    dictionaries: FxHashMap<u32, FxHashMap<u32, Value>>,
    free_dictionaries: Vec<u32>,
    next_dictionary: u32,
    nursery: Nursery,
}

impl Default for ValueArena {
    fn default() -> Self {
        Self {
            values: PropertyBuffer::default(),
            free: [const { Vec::new() }; BUCKETS],
            released_values: 0,
            shape_lengths: vec![0],
            shape_chains: Vec::new(),
            chain_owners: Vec::new(),
            owner_types: Vec::new(),
            slot_readers: crate::code_dependencies::CodeReaders::default(),
            targets: target_shapes::TargetShapes::default(),
            shape_readers: crate::code_dependencies::CodeReaders::default(),
            tracking: false,
            dictionaries: FxHashMap::default(),
            free_dictionaries: Vec::new(),
            next_dictionary: 0,
            nursery: Nursery::default(),
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
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        // Slot owners and representations are derived where they are kept (`Heap::track_slot_representations`).
        let Self { values, free, released_values, shape_lengths, shape_chains: _, chain_owners: _, owner_types: _, slot_readers: _, targets: _, shape_readers: _, tracking: _, dictionaries, free_dictionaries, next_dictionary, nursery: _ } = self;
        writer.values(values);
        for bucket in free {
            writer.u32s(bucket);
        }
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

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let values = reader.values()?;
        let mut free: [Vec<u32>; BUCKETS] = [const { Vec::new() }; BUCKETS];
        for bucket in &mut free {
            *bucket = reader.u32s()?;
        }
        let released_values = reader.len()?;
        let shape_lengths = reader.u32s()?;
        let dictionaries = crate::snapshot::read_list(reader, |reader| {
            let id = reader.u32()?;
            let entries = crate::snapshot::read_list(reader, |reader| Ok((reader.u32()?, reader.value()?)))?;
            Ok((id, entries.into_iter().collect()))
        })?
        .into_iter()
        .collect();
        Ok(Self {
            values: values.into(),
            free,
            released_values,
            shape_lengths,
            dictionaries,
            free_dictionaries: reader.u32s()?,
            next_dictionary: reader.u32()?,
            ..Self::default()
        })
    }

    pub(crate) fn reset(&mut self) {
        self.targets.reset();
        self.shape_readers.reset();
        self.values.clear();
        for bucket in &mut self.free {
            bucket.clear();
        }
        self.released_values = 0;
        self.shape_lengths.truncate(1);
        self.shape_lengths[0] = 0;
        self.dictionaries.clear();
        self.free_dictionaries.clear();
        self.next_dictionary = 0;
        self.nursery = Nursery::default();
    }

    pub(crate) fn register_shape(&mut self, shape: u32, length: usize) {
        self.targets.depart(shape, |owner| self.slot_readers.retire(owner));
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
        self.targets.reset();
        self.shape_readers.reset();
        self.slot_readers.reset();
        self.tracking = false;
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
        self.targets.reset_owner(shape);
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
    #[inline(always)]
    pub(crate) fn note_shape_target(&mut self, shape: u32, slot: usize, value: Value, target: TargetShape) {
        if self.tracking && slot < self.shape_length(shape) && !value.is_deleted() {
            self.note_slot_target(shape, slot, value, target);
        }
    }

    /// Observes an object's initial values and pointee shapes, slot by slot.
    pub(crate) fn note_shape_values(&mut self, shape: u32, values: &[Value], target: impl Fn(Value) -> TargetShape) {
        if !self.tracking { return; }
        for (slot, value) in values.iter().copied().enumerate() {
            self.note_shape_target(shape, slot, value, target(value));
        }
    }

    /// Starts joining writes' classes from none: the caller joins what live objects hold.
    pub(crate) fn track(&mut self) {
        self.targets.reset();
        self.slot_readers.reset();
        self.tracking = true;
        self.owner_types.fill(SlotType::None);
    }

    /// Whether writes join their values' classes.
    pub(crate) fn tracks(&self) -> bool {
        self.tracking
    }

    /// The slots `shape`'s objects store.
    pub(crate) fn shape_length(&self, shape: u32) -> usize {
        self.shape_lengths.get(shape as usize).map_or(0, |length| *length as usize)
    }

    /// The shape that added `slot` of `shape`'s objects, whose representation it shares.
    #[inline(always)]
    pub(crate) fn slot_owner(&self, shape: u32, slot: usize) -> u32 {
        self.chain_owners[self.shape_chains[shape as usize] as usize + slot]
    }

    /// The recorded type of `slot` of `shape`, not a write-maintained invariant.
    #[inline(always)]
    pub(crate) fn slot_observation(&self, shape: u32, slot: usize) -> SlotType {
        self.owner_types[self.slot_owner(shape, slot) as usize]
    }

    pub(crate) fn subscribe_slot(&mut self, dependency: crate::code_dependencies::SlotDependency,
        code: &std::rc::Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
        let crate::code_dependencies::SlotDependency { shape, slot, ty, target_shape } = dependency;
        let slot = usize::from(slot);
        if !self.tracking || slot >= self.shape_length(shape)
            || self.slot_observation(shape, slot).ty() != ty
            || target_shape.is_some_and(|target| self.slot_target_shape(shape, slot) != Some(target)) { return false; }
        self.subscribe_shape(shape, code)
            && self.slot_readers.subscribe(self.slot_owner(shape, slot), code)
    }

    /// Observe the next departure from this shape, not the contents of its slots.
    /// The caller must establish its target-shape assumption before subscribing.
    pub(crate) fn subscribe_shape(&mut self, shape: u32,
        code: &std::rc::Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
        if shape as usize >= self.shape_lengths.len() { return false; }
        self.shape_readers.subscribe(shape, code)
    }

    #[inline(always)]
    pub(crate) fn note_shape_transition(&mut self, previous: u32, next: u32) {
        if previous != next {
            self.targets.depart(previous, |owner| self.slot_readers.retire(owner));
            self.shape_readers.retire(previous);
        }
    }

    /// Shape identifiers will be remapped; retire assumptions before any object moves.
    pub(crate) fn retire_shape_readers(&mut self) {
        self.shape_readers.reset();
    }

    /// Joins `value`'s class into `slot` of `shape`'s representation; whether it widened.
    #[inline(always)]
    #[cfg(test)]
    pub(crate) fn note_slot_value(&mut self, shape: u32, slot: usize, value: Value) -> bool {
        self.note_slot_target(shape, slot, value, TargetShape::unclassified(value))
    }

    /// The untracked case, every store of a runtime without lane bodies, is the test alone:
    /// the joins are out of line (`note_tracked_slot_target`).
    #[inline(always)]
    pub(crate) fn note_slot_target(&mut self, shape: u32, slot: usize, value: Value, target: TargetShape) -> bool {
        self.tracking && self.note_tracked_slot_target(shape, slot, value, target)
    }

    #[inline(never)]
    fn note_tracked_slot_target(&mut self, shape: u32, slot: usize, value: Value, target: TargetShape) -> bool {
        let owner = self.slot_owner(shape, slot);
        let changed = self.targets.join(owner, target);
        if changed { self.slot_readers.retire(owner); }
        self.join_owner_type(owner, SlotType::of(value)) || changed
    }

    pub(crate) fn slot_target_shape(&self, shape: u32, slot: usize) -> Option<u32> {
        if !self.tracking || slot >= self.shape_length(shape) { return None; }
        self.targets.get(self.slot_owner(shape, slot)).shape()
    }

    /// Admits the possible writes of a no-reentry raw slot capability before exposing it.
    pub(crate) fn note_slot_type(&mut self, shape: u32, slot: usize, ty: crate::typed_ir::Type) -> bool {
        if !self.tracking { return false; }
        let owner = self.slot_owner(shape, slot);
        let target = if ty.is_subtype(crate::typed_ir::Type::NULLISH) { TargetShape::NONE } else { TargetShape::ANY };
        let changed = self.targets.join(owner, target);
        if changed { self.slot_readers.retire(owner); }
        self.join_owner_type(owner, SlotType::from_type(ty)) || changed
    }

    /// Joins `incoming` into the slot type `owner` holds; whether it widened.
    #[inline(always)]
    fn join_owner_type(&mut self, owner: u32, incoming: SlotType) -> bool {
        let held = self.owner_types[owner as usize];
        // Writes mostly repeat a class the slot already holds: one compare.
        if held == incoming || held == SlotType::Tagged { return false; }
        let class = held.join(incoming);
        if class == held { return false; }
        self.slot_readers.retire(owner);
        self.owner_types[owner as usize] = class;
        true
    }

    pub(crate) fn get(&self, vector: ValueVec, index: usize) -> Option<Value> {
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
        Some(self.values[vector.start() + index])
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
        // SAFETY: caller provides the initialized-range invariant; every
        // vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked(vector.start() + index) }
    }

    /// `get_flat` of the vector whose storage starts at `start` (`ValueVec::start`).
    ///
    /// # Safety
    /// As for `get_flat`: `start` is a live vector's start and `index` one of its values.
    #[inline(always)]
    pub(crate) unsafe fn get_flat_at(&self, start: usize, index: usize) -> Value {
        // SAFETY: the caller's vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked(start + index) }
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
        // SAFETY: as for `get_unchecked`.
        Some(unsafe { *self.values.get_unchecked(vector.start() + index) })
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
        assert!(index < self.len(vector));
        if vector.is_dictionary() {
            self.dictionaries
                .get_mut(&vector.dictionary_id())
                .expect("object dictionary exists")
                .insert(index as u32, value);
            return;
        }
        self.values[vector.start() + index] = value;
    }

    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    #[inline(always)]
    pub(crate) unsafe fn set_unchecked(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(!vector.has_inline_property_storage());
        debug_assert!(index < self.len(vector));
        if vector.is_dictionary() {
            self.dictionary_set(vector, index, value);
            return;
        }
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.set_flat_unchecked(vector, index, value) };
    }

    /// `set_unchecked` of a vector known not to be a dictionary.
    ///
    /// # Safety
    /// As for `set_unchecked`, and `vector` is not a dictionary.
    #[inline(always)]
    pub(crate) unsafe fn set_flat_unchecked(
        &mut self,
        vector: ValueVec,
        index: usize,
        value: Value,
    ) {
        debug_assert!(!vector.is_dictionary());
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.set_flat_at(vector.start(), index, value) }
    }

    /// `set_flat_unchecked` of the vector whose storage starts at `start` (`ValueVec::start`).
    ///
    /// # Safety
    /// As for `get_flat_at`.
    #[inline(always)]
    pub(crate) unsafe fn set_flat_at(&mut self, start: usize, index: usize, value: Value) {
        // SAFETY: the caller's vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked_mut(start + index) = value }
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
        debug_assert!(!vector.has_inline_property_storage() && !vector.is_dictionary());
        debug_assert!(index < self.len(vector));
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.values.as_mut_ptr().add(vector.start() + index) }
    }

    /// Append `value` past `vector`'s values, which an add of one property makes. Inline:
    /// an append that fits its range is the store alone; growing is out of line.
    #[inline(always)]
    pub(crate) fn push(&mut self, vector: &mut ValueVec, value: Value) {
        debug_assert!(!vector.has_inline_property_storage());
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
        self.values[vector.start() + len] = value;
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
        } else {
            vector.use_dictionary(self.allocate_dictionary(values.to_vec()));
        }
        vector
    }

    /// The start of a new arena run holding `values`, of the capacity their count derives
    /// (`ordered_capacity`); `None` once the arena's positions are spent, where storage takes
    /// a dictionary.
    #[inline(always)]
    pub(crate) fn run_of(&mut self, values: &[Value]) -> Option<u32> {
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
    #[inline(always)]
    pub(crate) fn young_run_padded<const N: usize>(&mut self, values: &[Value; N], len: usize) -> Option<u32> {
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
        Some(start as u32)
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
    #[inline(always)]
    pub(crate) fn young_elements(&mut self, len: usize) -> Option<std::ptr::NonNull<Value>> {
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
        }
        let end = self.nursery.top + capacity.max(NURSERY_GROWTH);
        if self.nursery.end == tail && end <= MAX_ARENA_START && self.values.resize(end) {
            self.nursery.end = end;
            let start = self.nursery.top;
            self.nursery.top = start + capacity;
            return Some(start);
        }
        None
    }

    /// The young runs taken since the last collection, as a sweep reads them beside its
    /// releases.
    #[inline(always)]
    pub(crate) fn young_runs(&self) -> YoungRuns {
        YoungRuns { floor: self.nursery.floor, top: self.nursery.top }
    }

    /// Whether `vector` holds a young run (`Nursery`).
    #[inline(always)]
    pub(crate) fn holds_young_run(&self, vector: ValueVec) -> bool {
        self.young_runs().hold(vector)
    }

    /// Moves the young run of `vector`, whose owner survives the collection under way, out of
    /// the young range: to a released run of its size class, else down to the nursery's
    /// floor, which rises past it. Survivors must come in the order of their starts, so each
    /// one moved to the floor lands at or below its source.
    pub(crate) fn evacuate_young(&mut self, vector: &mut ValueVec) {
        debug_assert!(self.holds_young_run(*vector));
        let capacity = Self::ordered_capacity(*vector, self.len(*vector));
        let source = vector.start();
        let target = match self.free[Self::bucket(capacity)].pop() {
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
        };
        self.values.copy_within(source..source + capacity, target);
        vector.relocate(target);
    }

    /// Ends a collection's young cycle once its survivors have left (`evacuate_young`): the
    /// dead young runs are free at once.
    pub(crate) fn finish_young_cycle(&mut self) {
        self.nursery.top = self.nursery.floor;
    }

    /// The heap references among a vector's values: the edges a collector traces. Numbers,
    /// other immediates and deleted slots are not references.
    pub(crate) fn append_heap_references(&self, vector: ValueVec, output: &mut Vec<Value>) {
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
                let values = self.values[vector.start()..vector.start() + len].to_vec();
                copy.use_dictionary(self.allocate_dictionary(values));
            }
        }
        copy
    }

    /// Frees `vector`'s storage. A young run (`Nursery`) returns with the rest of the young
    /// range when its collection ends, so its release is the range test alone.
    #[inline(always)]
    pub(crate) fn release(&mut self, vector: ValueVec) {
        if !self.young_runs().hold(vector) {
            self.release_ordinary(vector);
        }
    }

    /// `release` of storage that is no young run: that of any record but a direct object
    /// (`young_run_padded`), whose sweeps skip the young range test. Storage that holds no
    /// arena words, as most arrays' and functions' does, is the inlined test alone, where the
    /// call cost a dead array 20 Ir (array-callback); a range or dictionary takes the call
    /// (`release_held`).
    #[inline(always)]
    pub(crate) fn release_ordinary(&mut self, vector: ValueVec) {
        debug_assert!(!self.holds_young_run(vector));
        // Inline storage and an empty vector without a range hold no arena words.
        if vector.has_inline_property_storage() || (vector.start == EMPTY_START && !vector.is_dictionary()) {
            return;
        }
        self.release_held(vector);
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
            self.free[Self::bucket(capacity)].push(vector.start & START_MASK);
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

    pub(crate) fn has_compact_range(&self, vector: ValueVec) -> bool {
        vector.has_compact_range()
    }

    pub(crate) fn compact_vector(&mut self, vector: &mut ValueVec, target: usize) -> usize {
        assert!(
            !vector.is_dictionary(),
            "dictionary storage is not arena-backed"
        );
        let length = self.len(*vector);
        let capacity = self.capacity(*vector);
        let source = vector.start();
        assert!(
            target <= source,
            "property ranges must compact in source order"
        );
        self.values.copy_within(source..source + length, target);
        self.values[target + length..target + capacity].fill(Value::UNDEFINED);
        vector.relocate(target);
        capacity
    }

    pub(crate) fn finish_compaction(&mut self, length: usize) {
        self.values.truncate(length);
        // A burst's growth stays resident past the compaction that frees it: keep the room
        // the next doubling would give the live values, and return the rest.
        self.values.shrink_to(length.saturating_mul(COMPACTED_CAPACITY_FACTOR));
        for bucket in &mut self.free {
            bucket.clear();
        }
        self.released_values = 0;
        // Compaction moved every live run below `length`, young ones too.
        self.nursery = Nursery::default();
    }

    pub(crate) fn memory_bytes(&self) -> usize {
        self.values.capacity() * size_of::<Value>()
            + self
                .free
                .iter()
                .map(|bucket| bucket.capacity() * size_of::<u32>())
                .sum::<usize>()
            + self.dictionaries.capacity() * size_of::<(u32, FxHashMap<u32, Value>)>()
            + self
                .dictionaries
                .values()
                .map(|values| values.capacity() * size_of::<(u32, Value)>())
                .sum::<usize>()
            + self.free_dictionaries.capacity() * size_of::<u32>()
            + self.slot_readers.memory_bytes()
            + self.targets.memory_bytes()
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
            self.free.iter().map(Vec::len).sum(),
        )
    }

    /// Moves `vector` to a range of the next size class. A young run's owner is a newborn
    /// object (`Nursery`), so its next range is a young run too: grown in place when it ends
    /// the young range, as a new object's runs mostly do while its constructor adds
    /// properties, else bumped. An ordinary range for it left the young one dead in the
    /// nursery until the collection, and pushed the arena's end past the nursery's.
    #[inline(never)]
    fn grow(&mut self, vector: &mut ValueVec, max_start: usize) {
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
                self.values[vector.start()..vector.start() + len].to_vec()
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
    #[inline(always)]
    pub(crate) fn values_base(&self) -> *const Value {
        self.values.as_ptr()
    }

    pub(crate) fn len(&self, vector: ValueVec) -> usize {
        self.shape_lengths[vector.auxiliary() as usize] as usize
    }

    pub(crate) fn assert_shape_length(&self, shape: u32, length: usize) {
        debug_assert_eq!(length, self.shape_lengths[shape as usize] as usize, "property values must match shape");
    }

    fn capacity(&self, vector: ValueVec) -> usize {
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
            || (!vector.is_dictionary()
                && len > 0
                && Self::ordered_capacity(vector, len - 1) == Self::ordered_capacity(vector, len))
    }

    /// Whether `vector`, an arena vector outside dictionary storage of `len` values, has room
    /// for one more value without moving its range (`push`'s `grow`).
    #[inline(always)]
    pub(crate) fn appends_in_place(vector: ValueVec, len: usize) -> bool {
        len < Self::ordered_capacity(vector, len)
    }

    /// `push` of `value` to `vector` of `len` values where `appends_in_place` holds: the
    /// store alone.
    ///
    /// # Safety
    /// `vector` is an arena vector outside dictionary storage of `len` values, whose range has
    /// room for one more (`appends_in_place`).
    #[inline(always)]
    pub(crate) unsafe fn push_in_place(&mut self, vector: ValueVec, len: usize, value: Value) {
        debug_assert!(!vector.has_inline_property_storage() && !vector.is_dictionary());
        debug_assert!(len == self.len(vector) && Self::appends_in_place(vector, len));
        // SAFETY: the range from `start` holds its capacity, past `len`, reserved in `values`.
        unsafe { *self.values.get_unchecked_mut(vector.start() + len) = value };
    }

    /// A range of `capacity`: a released one of its size class, else, out of line, one past
    /// the arena's end (`allocate_fresh`).
    #[inline(always)]
    fn allocate(&mut self, capacity: usize, max_start: usize) -> Option<usize> {
        let bucket = Self::bucket(capacity);
        if let Some(start) = self.free[bucket].pop() {
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

    /// The free list of a size class (`capacity_class`): two per doubling, the power of two
    /// and the three-quarter step below the next one.
    #[inline(always)]
    fn bucket(capacity: usize) -> usize {
        debug_assert_eq!(capacity_class(capacity), capacity, "a range capacity is a size class");
        let octave = (capacity.ilog2() - MIN_CAPACITY.ilog2()) as usize;
        let bucket = 2 * octave + usize::from(!capacity.is_power_of_two());
        assert!(bucket < BUCKETS, "property arena bucket overflow");
        bucket
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn slot_observations_share_owners_and_widen_semantically() {
        let mut arena = ValueArena::default();
        assert!(!arena.note_slot_value(u32::MAX, usize::MAX, Value::NULL));
        arena.track();
        arena.register_derived_shape(1, 1, None);
        arena.register_derived_shape(2, 2, Some((1, 1)));
        assert_eq!(arena.slot_owner(1, 0), arena.slot_owner(2, 0));
        assert!(arena.note_slot_value(1, 0, Value::NULL));
        assert!(arena.note_slot_value(2, 0, Value::direct_object(7)));
        assert_eq!(arena.slot_observation(1, 0), SlotType::DirectObjectOrNull);
        assert!(!arena.note_slot_value(2, 0, Value::direct_object(8)));
        assert!(arena.note_slot_value(2, 1, Value::number(0.5)));
        assert_eq!(arena.slot_observation(2, 1), SlotType::Number);
        assert!(arena.note_slot_value(1, 0, Value::UNDEFINED));
        assert_eq!(arena.slot_observation(2, 0), SlotType::DirectObjectOrNullish);
        assert!(arena.note_slot_value(2, 0, Value::integer(1)));
        assert_eq!(arena.slot_observation(1, 0), SlotType::Tagged);
        assert_eq!(arena.slot_observation(2, 1), SlotType::Number);
        arena.track();
        assert_eq!(arena.slot_observation(2, 0), SlotType::None);
        assert_eq!(arena.slot_observation(2, 1), SlotType::None);
    }
    use super::*;

    #[test]
    fn metadata_is_compact_and_ranges_preserve_values() {
        assert_eq!(size_of::<ValueVec>(), 8);
        let mut arena = ValueArena::default();
        let mut values = ValueVec::new();
        for value in 0..9 {
            arena.register_shape(value + 1, value as usize + 1);
            arena.push(&mut values, Value::number(f64::from(value)));
            values.set_auxiliary(value + 1);
        }
        assert_eq!(arena.get(values, 8).unwrap().as_number(), Some(8.0));
        assert_eq!(arena.vector_stats(values), (9, 12));
    }

    #[test]
    fn size_classes_step_by_powers_of_two_and_three_quarters() {
        let classes: Vec<_> = (0..=33).map(capacity_class).collect();
        assert_eq!(&classes[..=MIN_CAPACITY], &[MIN_CAPACITY; MIN_CAPACITY + 1]);
        assert_eq!([classes[5], classes[6], classes[7], classes[9], classes[13], classes[17], classes[25], classes[33]], [6, 6, 8, 12, 16, 24, 32, 48]);
        // Each class has its own free list, in increasing order.
        let buckets: Vec<_> = [4, 6, 8, 12, 16, 24, 32, 48].into_iter().map(ValueArena::bucket).collect();
        assert_eq!(buckets, (0..8).collect::<Vec<_>>());
        // A range grows to the next class.
        assert_eq!(capacity_class(12 + 1), 16);
    }

    #[test]
    fn young_runs_reclaim_the_dead_at_once_and_move_survivors_below_the_floor() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 3);
        let values = |n: f64| [Value::number(n), Value::number(n + 0.5), Value::number(n + 0.25), Value::UNDEFINED];
        let young = |arena: &mut ValueArena, n: f64| ValueVec { start: arena.young_run_padded(&values(n), 3).unwrap(), auxiliary: 1 };
        let ordinary = arena.with_values(1, &values(9.0)[..3]);
        let mut runs: Vec<ValueVec> = (0..8).map(|n| young(&mut arena, f64::from(n))).collect();
        assert!(runs.iter().all(|run| arena.holds_young_run(*run)));
        assert!(!arena.holds_young_run(ordinary));
        let floor = runs[0].start();
        assert_eq!(floor, MIN_CAPACITY, "the nursery starts at the arena's end");
        // A dead young run releases nothing.
        arena.release(runs[1]);
        assert_eq!(arena.stats().2, 0);
        // Survivors 3 and 6 move down to the floor in start order; the rest is reclaimed.
        for index in [3, 6] {
            arena.evacuate_young(&mut runs[index]);
        }
        arena.finish_young_cycle();
        assert_eq!((runs[3].start(), runs[6].start()), (floor, floor + MIN_CAPACITY));
        for index in [3, 6] {
            assert!(!arena.holds_young_run(runs[index]));
            assert_eq!(arena.get(runs[index], 2).unwrap().as_number(), Some(index as f64 + 0.25));
        }
        // The next cycle's runs reuse the reclaimed range above the survivors.
        assert_eq!(young(&mut arena, 20.0).start(), floor + 2 * MIN_CAPACITY);
        // A survivor takes a released run of its class before the floor.
        arena.release(runs[3]);
        let mut survivor = young(&mut arena, 30.0);
        arena.evacuate_young(&mut survivor);
        assert_eq!(survivor.start(), floor);
        assert_eq!(arena.get(survivor, 1).unwrap().as_number(), Some(30.5));
        // A range that no longer ends the arena serves young runs up to its end, then ordinary
        // runs take over; one a compaction emptied starts again at the arena's end.
        let _ = arena.with_values(1, &[Value::number(1.0); 5][..3]);
        arena.finish_young_cycle();
        let kept = young(&mut arena, 50.0);
        assert!(arena.holds_young_run(kept), "the range stays");
        let overflows = (0..NURSERY_GROWTH).any(|n| {
            let run = young(&mut arena, n as f64);
            !arena.holds_young_run(run)
        });
        assert!(overflows, "a range that cannot grow overflows into ordinary runs");
        let length = arena.stats().0;
        arena.finish_compaction(length);
        assert_eq!(young(&mut arena, 60.0).start(), length);
    }

    #[test]
    fn duplicated_storage_survives_the_release_and_reuse_of_its_source() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 2);
        let mut source = arena.with_values(1, &[Value::number(1.0), Value::number(2.0)]);
        source.set_frozen(true);
        let copy = arena.duplicate(source);
        assert_ne!(copy.start(), source.start());
        assert!(copy.is_frozen());
        assert_eq!(copy.auxiliary(), source.auxiliary());
        arena.release(source);
        let reused = arena.with_values(1, &[Value::number(7.0), Value::number(8.0)]);
        assert_eq!(reused.start(), source.start(), "the released range is reused");
        assert_eq!(arena.get(copy, 0).unwrap().as_number(), Some(1.0));
        assert_eq!(arena.get(copy, 1).unwrap().as_number(), Some(2.0));

        arena.migrate_to_dictionary_for_test(&mut source);
        arena.set(source, 0, Value::number(3.0));
        let dictionary_copy = arena.duplicate(source);
        assert_ne!(dictionary_copy.dictionary_id(), source.dictionary_id());
        arena.set(source, 0, Value::number(4.0));
        assert_eq!(arena.get(dictionary_copy, 0).unwrap().as_number(), Some(3.0));
    }

    #[test]
    fn compaction_is_due_once_released_ranges_hold_a_share_of_the_arena() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 2);
        let pair = [Value::number(1.0), Value::number(2.0)];
        let vectors: Vec<_> = (0..2 * COMPACTION_RELEASED_SHARE_DIVISOR)
            .map(|_| arena.with_values(1, &pair))
            .collect();
        assert!(!arena.compaction_due());
        arena.release(vectors[0]);
        assert!(!arena.compaction_due(), "one range of eight stays for reuse");
        arena.release(vectors[1]);
        assert!(arena.compaction_due(), "a quarter of the arena is released");
        // Reusing a released range takes it off the free lists.
        arena.with_values(1, &pair);
        assert!(!arena.compaction_due());
        arena.finish_compaction(arena.stats().0);
        assert!(!arena.compaction_due());
    }

    #[test]
    fn initialized_vector_uses_the_shape_length_and_preserves_values() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 3);
        let expected = [Value::heap(11), Value::heap(12), Value::heap(13)];

        let vector = arena.with_values(1, &expected);

        assert_eq!(arena.len(vector), expected.len());
        assert_eq!(
            (0..expected.len())
                .map(|slot| arena.get(vector, slot).unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(vector.auxiliary(), 1);
    }

    #[test]
    fn arena_exhaustion_migrates_live_slots_to_dictionary_storage() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 2);
        let mut vector = arena.with_values(1, &[Value::number(11.0), Value::number(22.0)]);

        let start = vector.start();
        let range_count = arena.values.len();
        // A zero address limit exercises the same checked exhaustion branch
        // without allocating billions of arena entries in a unit test.
        arena.migrate_to_dictionary_for_test(&mut vector);
        let dictionary_id = vector.dictionary_id();

        assert!(vector.is_dictionary());
        assert_eq!(arena.get(vector, 0).unwrap().as_number(), Some(11.0));
        assert_eq!(arena.get(vector, 1).unwrap().as_number(), Some(22.0));
        arena.set(vector, 1, Value::number(33.0));
        assert_eq!(arena.get(vector, 1).unwrap().as_number(), Some(33.0));

        let mut references = Vec::new();
        arena.append_heap_references(vector, &mut references);
        assert!(references.is_empty(), "numbers are not heap references");
        assert_eq!(arena.values.len(), range_count);
        assert!(
            arena
                .free
                .iter()
                .any(|bucket| bucket.contains(&(start as u32)))
        );

        arena.release(vector);
        assert!(arena.dictionaries.is_empty());
        assert_eq!(arena.free_dictionaries, vec![dictionary_id]);
    }

    #[test]
    fn dictionary_roots_skip_deleted_slots_after_arena_migration() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 2);
        let mut vector = arena.with_values(1, &[Value::heap(11), Value::heap(12)]);
        arena.set(vector, 1, Value::DELETED);

        arena.migrate_to_dictionary_for_test(&mut vector);

        let mut roots = Vec::new();
        arena.append_heap_references(vector, &mut roots);
        assert_eq!(roots, [Value::heap(11)]);
    }

    #[test]
    fn snapshot_preserves_offsets_free_ranges_and_value_bits_after_compaction() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 3);
        let dead = arena.with_values(1, &[Value::NULL; 3]);
        let expected = [Value::number(-0.0), Value::number(f64::NAN), Value::heap(17)];
        let mut live = arena.with_values(1, &expected);
        arena.release(dead);
        let capacity = arena.compact_vector(&mut live, 0);
        arena.finish_compaction(capacity);
        let mut writer = crate::snapshot::Writer::new();
        arena.write_snapshot(&mut writer);
        let bytes = writer.finish();
        let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
        let mut restored = ValueArena::read_snapshot(&mut reader).unwrap();
        reader.finish().unwrap();
        for (slot, value) in expected.iter().enumerate() {
            assert_eq!(restored.get(live, slot).unwrap().to_bits(), value.to_bits());
        }
        let released = restored.with_values(1, &[Value::UNDEFINED; 3]);
        restored.release(released);
        let mut writer = crate::snapshot::Writer::new();
        restored.write_snapshot(&mut writer);
        let bytes = writer.finish();
        let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
        let mut restored = ValueArena::read_snapshot(&mut reader).unwrap();
        let replacement = restored.with_values(1, &[Value::TRUE; 3]);
        assert_eq!(replacement.start(), released.start());
        assert_eq!(restored.get(live, 2), Some(expected[2]));
    }
}
