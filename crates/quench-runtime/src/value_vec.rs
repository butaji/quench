use crate::Value;
use rustc_hash::FxHashMap;

pub(crate) const START_MASK: u32 = 0x3fff_ffff;
const NON_EXTENSIBLE: u32 = 1 << 30;
const FROZEN: u32 = 1 << 31;
pub(crate) const DICTIONARY_STORAGE: u32 = 1 << 31;
const SHAPE_MASK: u32 = !DICTIONARY_STORAGE;
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
const DENSE_ARENA_RESERVE_THRESHOLD: usize = 65_536;
const DENSE_ARENA_GROWTH_DIVISOR: usize = 3;
/// Released values on the free lists, as a share (`1 / divisor`) of the arena's length, that
/// make a full collection compact the arena. Freed ranges are reused within their size class,
/// so compaction recovers only what other classes leave idle; rewriting every live object's
/// storage on each full collection cost source-map-wtb 3 s of 27 (TOD-137).
const COMPACTION_RELEASED_SHARE_DIVISOR: usize = 4;
/// The arena capacity a compaction keeps, as a multiple of the values it kept: the room one
/// doubling of the compacted arena would give. Before, a load burst's growth stayed resident
/// for good: a fastify server under 500 connections grew the arena to 654K values (5.2 MB)
/// while full collections kept 48-250K, and returning the rest lowered the board's final
/// peak 40.7 -> 37.8 MB (median of 8 rounds, bombardier, board phases, plain x86_64 build,
/// 2026-10-26); V8-v7 RSS and Score and the npm-perf fastify workload were unchanged.
const COMPACTED_CAPACITY_FACTOR: usize = 2;

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

    /// The auxiliary word with its dictionary bit: equal to `auxiliary` exactly when the
    /// vector is not in dictionary storage, so one comparison with a recorded shape, which
    /// is never a dictionary's, proves both.
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
        self.auxiliary = (self.auxiliary & DICTIONARY_STORAGE) | value;
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
    values: Vec<Value>,
    free: [Vec<u32>; BUCKETS],
    /// Values held by the ranges on `free`.
    released_values: usize,
    shape_lengths: Vec<u32>,
    dictionaries: FxHashMap<u32, FxHashMap<u32, Value>>,
    free_dictionaries: Vec<u32>,
    next_dictionary: u32,
}

impl Default for ValueArena {
    fn default() -> Self {
        Self {
            values: vec![],
            free: [const { Vec::new() }; BUCKETS],
            released_values: 0,
            shape_lengths: vec![0],
            dictionaries: FxHashMap::default(),
            free_dictionaries: Vec::new(),
            next_dictionary: 0,
        }
    }
}

impl ValueArena {
    /// The current storage for out-of-line object properties.
    ///
    /// A native reader may use this address only during synchronous no-safepoint execution:
    /// growth and compaction can move the vector. Every derived slot must be checked against
    /// `len` before it is read.
    pub(crate) fn native_values(&self) -> (*const Value, usize) {
        (self.values.as_ptr(), self.values.len())
    }

    /// The arena as a snapshot keeps it (`crate::snapshot`): storage positions name places in
    /// `values`, so the vector and its free ranges are written as they are.
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        let Self { values, free, released_values, shape_lengths, dictionaries, free_dictionaries, next_dictionary } = self;
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
            values,
            free,
            released_values,
            shape_lengths,
            dictionaries,
            free_dictionaries: reader.u32s()?,
            next_dictionary: reader.u32()?,
        })
    }

    pub(crate) fn reset(&mut self) {
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
    }

    pub(crate) fn register_shape(&mut self, shape: u32, length: usize) {
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
        self.shape_lengths.clear();
        self.shape_lengths.push(0);
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

    /// `get_unchecked` of a vector known to be outside dictionary mode (`exact_auxiliary`
    /// matched a recorded shape).
    ///
    /// # Safety
    /// As for `get_unchecked`, and the vector is not a dictionary.
    #[inline(always)]
    pub(crate) unsafe fn get_flat(&self, vector: ValueVec, index: usize) -> Value {
        debug_assert!(!vector.has_inline_property_storage() && !vector.is_dictionary());
        debug_assert!(index < self.len(vector));
        // SAFETY: as for `get_unchecked`.
        unsafe { *self.values.get_unchecked(vector.start() + index) }
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
    pub(crate) unsafe fn set_flat_unchecked(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(!vector.is_dictionary());
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe {
            *self.values.get_unchecked_mut(vector.start() + index) = value;
        }
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

    pub(crate) fn release(&mut self, vector: ValueVec) {
        if vector.has_inline_property_storage() {
            return;
        }
        if vector.is_dictionary() {
            let id = vector.dictionary_id();
            self.dictionaries.remove(&id);
            self.free_dictionaries.push(id);
            return;
        }
        let capacity = self.capacity(vector);
        if capacity != 0 {
            self.free[Self::bucket(capacity)].push(vector.start & START_MASK);
            self.released_values += capacity;
        }
    }

    /// Whether released ranges have grown enough that compaction is worth rewriting every
    /// live object's storage.
    pub(crate) fn compaction_due(&self) -> bool {
        self.released_values > 0
            && self.released_values >= self.values.len() / COMPACTION_RELEASED_SHARE_DIVISOR
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

    #[inline(never)]
    fn grow(&mut self, vector: &mut ValueVec, max_start: usize) {
        let len = self.len(*vector);
        let capacity = self.capacity(*vector);
        let next = capacity_class(capacity + 1);
        let Some(start) = self.allocate(next, max_start) else {
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

    pub(crate) fn len(&self, vector: ValueVec) -> usize {
        self.shape_lengths[vector.auxiliary() as usize] as usize
    }

    pub(crate) fn assert_shape_length(&self, shape: u32, length: usize) {
        let expected = self.shape_lengths[shape as usize] as usize;
        assert_eq!(length, expected, "property values must match shape");
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

    fn allocate(&mut self, capacity: usize, max_start: usize) -> Option<usize> {
        let bucket = Self::bucket(capacity);
        if let Some(start) = self.free[bucket].pop() {
            self.released_values -= capacity;
            return Some(start as usize);
        }
        let start = self.values.len();
        if start >= max_start {
            return None;
        }
        let required = start.checked_add(capacity)?;
        // Vec's 2x policy leaves a large unused tail once long-lived heaps
        // cross into millions of property slots. Preserve that policy for
        // small programs, then grow by one third so the arena remains dense
        // without turning every object allocation into a reallocation.
        if required > self.values.capacity()
            && self.values.capacity() >= DENSE_ARENA_RESERVE_THRESHOLD
        {
            let target = required
                .max(self.values.capacity() + self.values.capacity() / DENSE_ARENA_GROWTH_DIVISOR);
            self.values.reserve_exact(target - self.values.len());
        }
        self.values.resize(start + capacity, Value::UNDEFINED);
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
        assert_eq!(arena.vector_stats(values), (9, 16));
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
}
