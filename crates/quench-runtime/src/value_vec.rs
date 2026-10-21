use crate::Value;
use rustc_hash::FxHashMap;

/// `ValueVec::start` bit of a range in the narrow lane (`ValueArena`).
const NARROW_STORAGE: u32 = 1 << 29;
const START_MASK: u32 = NARROW_STORAGE - 1;
const NON_EXTENSIBLE: u32 = 1 << 30;
const FROZEN: u32 = 1 << 31;
const DICTIONARY_STORAGE: u32 = 1 << 31;
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
const BUCKETS: usize = 32;
const DENSE_ARENA_RESERVE_THRESHOLD: usize = 65_536;
const DENSE_ARENA_GROWTH_DIVISOR: usize = 3;
/// Released values on the free lists, as a share (`1 / divisor`) of the arena's length, that
/// make a full collection compact the arena. Freed ranges are reused within their size class,
/// so compaction recovers only what other classes leave idle; rewriting every live object's
/// storage on each full collection cost source-map-wtb 3 s of 27 (TOD-137).
const COMPACTION_RELEASED_SHARE_DIVISOR: usize = 4;

/// Compact per-object shape, integrity, and property-storage metadata.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueVec {
    start: u32,
    auxiliary: u32,
}

/// `ValueVec::start` bits that hold the storage position: an arena offset, a dictionary id,
/// or one of the empty and inline markers.
pub(crate) const VALUE_VEC_START_BITS: u32 = START_MASK | NARROW_STORAGE;
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
        self.start = (self.start & !VALUE_VEC_START_BITS) | id;
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

    /// The range's place for compaction order: its lane, then its offset in that lane.
    pub(crate) fn start_offset(self) -> usize {
        debug_assert!(!self.is_dictionary());
        debug_assert!(!self.has_inline_property_storage());
        (self.start & VALUE_VEC_START_BITS) as usize
    }

    /// Whether the range lives in the narrow lane.
    #[inline(always)]
    fn is_narrow(self) -> bool {
        self.start & NARROW_STORAGE != 0
    }

    /// Names the range at `start` of the narrow lane or the wide one.
    fn place(&mut self, start: usize, narrow: bool) {
        assert!(start < INLINE_PROPERTY_START as usize, "property arena exhausted");
        let lane = if narrow { NARROW_STORAGE } else { 0 };
        self.start = start as u32 | lane | (self.start & !VALUE_VEC_START_BITS);
    }
}

impl Default for ValueVec {
    fn default() -> Self {
        Self::new()
    }
}

/// One store of arena ranges: `slots` with free ranges by power-of-two size class.
struct Lane<T> {
    slots: Vec<T>,
    free: [Vec<u32>; BUCKETS],
    /// Slots held by the ranges on `free`.
    released: usize,
}

impl<T> Default for Lane<T> {
    fn default() -> Self {
        Self { slots: Vec::new(), free: Default::default(), released: 0 }
    }
}

impl<T: Copy> Lane<T> {
    fn write_snapshot(&self, writer: &mut crate::snapshot::Writer, slots: impl FnOnce(&mut crate::snapshot::Writer, &[T])) {
        slots(writer, &self.slots);
        for bucket in &self.free {
            writer.u32s(bucket);
        }
        writer.len(self.released);
    }

    fn read_snapshot(
        reader: &mut crate::snapshot::Reader<'_>,
        slots: impl FnOnce(&mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Vec<T>>,
    ) -> crate::snapshot::SnapshotResult<Self> {
        let slots = slots(reader)?;
        let mut free: [Vec<u32>; BUCKETS] = Default::default();
        for bucket in &mut free {
            *bucket = reader.u32s()?;
        }
        Ok(Self { slots, free, released: reader.len()? })
    }

    fn reset(&mut self) {
        self.slots.clear();
        for bucket in &mut self.free {
            bucket.clear();
        }
        self.released = 0;
    }

    fn allocate(&mut self, capacity: usize, max_start: usize, fill: T) -> Option<usize> {
        let bucket = ValueArena::bucket(capacity);
        if let Some(start) = self.free[bucket].pop() {
            self.released -= capacity;
            return Some(start as usize);
        }
        let start = self.slots.len();
        if start >= max_start {
            return None;
        }
        let required = start.checked_add(capacity)?;
        // Vec's 2x policy leaves a large unused tail once long-lived heaps
        // cross into millions of property slots. Preserve that policy for
        // small programs, then grow by one third so the arena remains dense
        // without turning every object allocation into a reallocation.
        if required > self.slots.capacity() && self.slots.capacity() >= DENSE_ARENA_RESERVE_THRESHOLD {
            let target = required.max(self.slots.capacity() + self.slots.capacity() / DENSE_ARENA_GROWTH_DIVISOR);
            self.slots.reserve_exact(target - self.slots.len());
        }
        self.slots.resize(start + capacity, fill);
        Some(start)
    }

    fn release(&mut self, start: usize, capacity: usize) {
        self.free[ValueArena::bucket(capacity)].push(start as u32);
        self.released += capacity;
    }

    fn compaction_due(&self) -> bool {
        self.released > 0 && self.released >= self.slots.len() / COMPACTION_RELEASED_SHARE_DIVISOR
    }

    fn compact(&mut self, source: usize, length: usize, capacity: usize, target: usize, fill: T) {
        assert!(target <= source, "property ranges must compact in source order");
        self.slots.copy_within(source..source + length, target);
        self.slots[target + length..target + capacity].fill(fill);
    }

    fn finish_compaction(&mut self, length: usize) {
        self.slots.truncate(length);
        for bucket in &mut self.free {
            bucket.clear();
        }
        self.released = 0;
    }

    fn memory_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<T>() + self.free.iter().map(|bucket| bucket.capacity() * size_of::<u32>()).sum::<usize>()
    }
}

/// Property values of every object outside inline and dictionary storage. A range lives in
/// the narrow lane, as narrow values (`Value::to_narrow`), while each of its values has a
/// narrow form, and in the wide lane otherwise; a value without a narrow form moves its range
/// to the wide lane for good.
pub(crate) struct ValueArena {
    values: Lane<Value>,
    narrow: Lane<u32>,
    shape_lengths: Vec<u32>,
    dictionaries: FxHashMap<u32, FxHashMap<u32, Value>>,
    free_dictionaries: Vec<u32>,
    next_dictionary: u32,
}

impl Default for ValueArena {
    fn default() -> Self {
        Self {
            values: Lane::default(),
            narrow: Lane::default(),
            shape_lengths: vec![0],
            dictionaries: FxHashMap::default(),
            free_dictionaries: Vec::new(),
            next_dictionary: 0,
        }
    }
}

/// The narrow form of undefined, which fills unused narrow slots.
fn narrow_undefined() -> u32 {
    Value::UNDEFINED.to_narrow().expect("undefined has a narrow form")
}

impl ValueArena {
    /// The arena as a snapshot keeps it (`crate::snapshot`): storage positions name places in
    /// the lanes, so the lanes and their free ranges are written as they are.
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        let Self { values, narrow, shape_lengths, dictionaries, free_dictionaries, next_dictionary } = self;
        values.write_snapshot(writer, |writer, slots| writer.values(slots));
        narrow.write_snapshot(writer, |writer, slots| writer.u32s(slots));
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
        let values = Lane::read_snapshot(reader, |reader| reader.values())?;
        let narrow = Lane::read_snapshot(reader, |reader| reader.u32s())?;
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
            narrow,
            shape_lengths,
            dictionaries,
            free_dictionaries: reader.u32s()?,
            next_dictionary: reader.u32()?,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.values.reset();
        self.narrow.reset();
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

    /// The value at `index` of `vector`'s range, in whichever lane holds it.
    ///
    /// # Safety
    /// `vector` has a range and `index` is within its length.
    #[inline(always)]
    unsafe fn range_get(&self, vector: ValueVec, index: usize) -> Value {
        let at = vector.start() + index;
        // SAFETY: the caller proves the slot is inside the vector's reserved range.
        unsafe {
            if vector.is_narrow() {
                Value::from_narrow(*self.narrow.slots.get_unchecked(at))
            } else {
                *self.values.slots.get_unchecked(at)
            }
        }
    }

    /// Stores `value` at `index` of `vector`'s range. `true` when the range had to move to
    /// the wide lane (or, with the arena exhausted, to a dictionary), so `vector` changed and
    /// its owner must store it again.
    ///
    /// # Safety
    /// As for `range_get`.
    #[inline(always)]
    unsafe fn range_set(&mut self, vector: &mut ValueVec, index: usize, value: Value) -> bool {
        if vector.is_narrow() {
            if let Some(narrow) = value.to_narrow() {
                // SAFETY: as for `range_get`.
                unsafe { *self.narrow.slots.get_unchecked_mut(vector.start() + index) = narrow };
                return false;
            }
            self.widen(vector);
            if vector.is_dictionary() {
                self.dictionary_set(*vector, index, value);
                return true;
            }
            // SAFETY: widening keeps the length and capacity.
            unsafe { *self.values.slots.get_unchecked_mut(vector.start() + index) = value };
            return true;
        }
        // SAFETY: as for `range_get`.
        unsafe { *self.values.slots.get_unchecked_mut(vector.start() + index) = value };
        false
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
        // SAFETY: the index is within the vector's length, so inside its range.
        Some(unsafe { self.range_get(vector, index) })
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
        // vector range was allocated from its lane and remains reserved.
        unsafe { self.range_get(vector, index) }
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
        unsafe { self.range_get(vector, index) }
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
        Some(unsafe { self.range_get(vector, index) })
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

    /// Stores `value` at `index`; `true` when `vector` changed (`range_set`).
    pub(crate) fn set(&mut self, vector: &mut ValueVec, index: usize, value: Value) -> bool {
        debug_assert!(!vector.has_inline_property_storage());
        assert!(index < self.len(*vector));
        if vector.is_dictionary() {
            self.dictionary_set(*vector, index, value);
            return false;
        }
        // SAFETY: the index is within the vector's length, so inside its range.
        unsafe { self.range_set(vector, index, value) }
    }

    /// Stores `value` at `index`; `true` when `vector` changed (`range_set`).
    ///
    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    #[inline(always)]
    pub(crate) unsafe fn set_unchecked(&mut self, vector: &mut ValueVec, index: usize, value: Value) -> bool {
        debug_assert!(!vector.has_inline_property_storage());
        debug_assert!(index < self.len(*vector));
        if vector.is_dictionary() {
            self.dictionary_set(*vector, index, value);
            return false;
        }
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.range_set(vector, index, value) }
    }

    /// `set_unchecked` of a vector known not to be a dictionary.
    ///
    /// # Safety
    /// As for `set_unchecked`, and `vector` is not a dictionary.
    #[inline(always)]
    pub(crate) unsafe fn set_flat_unchecked(&mut self, vector: &mut ValueVec, index: usize, value: Value) -> bool {
        debug_assert!(!vector.is_dictionary());
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.range_set(vector, index, value) }
    }

    pub(crate) fn push(&mut self, vector: &mut ValueVec, value: Value) {
        debug_assert!(!vector.has_inline_property_storage());
        let len = self.len(*vector);
        if !vector.is_dictionary() {
            let narrow = value.to_narrow();
            if vector.is_narrow() && narrow.is_none() {
                self.widen(vector);
            }
            if !vector.is_dictionary() && len == self.capacity(*vector) {
                // A vector without a range starts in the lane its first value fits.
                let lane = if self.capacity(*vector) == 0 { narrow.is_some() } else { vector.is_narrow() };
                self.grow(vector, MAX_ARENA_START, lane);
            }
            if !vector.is_dictionary() {
                let at = vector.start() + len;
                match narrow.filter(|_| vector.is_narrow()) {
                    Some(narrow) => self.narrow.slots[at] = narrow,
                    None => self.values.slots[at] = value,
                }
                return;
            }
        }
        self.dictionaries
            .get_mut(&vector.dictionary_id())
            .expect("object dictionary exists")
            .insert(len as u32, value);
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
        let capacity = values
            .len()
            .max(MIN_CAPACITY)
            .checked_next_power_of_two()
            .expect("property vector capacity overflow");
        if values.iter().all(|value| value.to_narrow().is_some())
            && let Some(start) = self.narrow.allocate(capacity, MAX_ARENA_START, narrow_undefined())
        {
            vector.place(start, true);
            for (slot, value) in self.narrow.slots[start..start + values.len()].iter_mut().zip(values) {
                *slot = value.to_narrow().expect("checked narrow");
            }
        } else if let Some(start) = self.values.allocate(capacity, MAX_ARENA_START, Value::UNDEFINED) {
            vector.place(start, false);
            self.values.slots[start..start + values.len()].copy_from_slice(values);
        } else {
            vector.use_dictionary(self.allocate_dictionary(values.to_vec()));
        }
        vector
    }

    /// `vector`'s range values in order, decoded from whichever lane holds them.
    fn range_values(&self, vector: ValueVec) -> Vec<Value> {
        let (start, len) = (vector.start(), self.len(vector));
        if vector.is_narrow() {
            self.narrow.slots[start..start + len].iter().map(|narrow| Value::from_narrow(*narrow)).collect()
        } else {
            self.values.slots[start..start + len].to_vec()
        }
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
        let start = vector.start();
        if vector.is_narrow() {
            output.extend(
                self.narrow.slots[start..start + len]
                    .iter()
                    .map(|narrow| Value::from_narrow(*narrow))
                    .filter(|value| value.is_heap()),
            );
        } else {
            output.extend(self.values.slots[start..start + len].iter().copied().filter(|value| value.is_heap()));
        }
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
        let (source, len) = (vector.start(), self.len(vector));
        let start = if vector.is_narrow() {
            self.narrow.allocate(capacity, MAX_ARENA_START, narrow_undefined()).inspect(|start| {
                self.narrow.slots.copy_within(source..source + len, *start);
            })
        } else {
            self.values.allocate(capacity, MAX_ARENA_START, Value::UNDEFINED).inspect(|start| {
                self.values.slots.copy_within(source..source + len, *start);
            })
        };
        match start {
            Some(start) => copy.place(start, vector.is_narrow()),
            None => {
                let values = self.range_values(vector);
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
            if vector.is_narrow() {
                self.narrow.release(vector.start(), capacity);
            } else {
                self.values.release(vector.start(), capacity);
            }
        }
    }

    /// Whether released ranges have grown enough, in either lane, that compaction is worth
    /// rewriting every live object's storage.
    pub(crate) fn compaction_due(&self) -> bool {
        self.values.compaction_due() || self.narrow.compaction_due()
    }

    pub(crate) fn has_compact_range(&self, vector: ValueVec) -> bool {
        vector.has_compact_range()
    }

    /// Slides `vector`'s range down to its lane's next target, which advances past it. Owners
    /// come in `ValueVec::start_offset` order: each lane in offset order.
    pub(crate) fn compact_vector(&mut self, vector: &mut ValueVec, targets: &mut CompactionTargets) {
        assert!(
            !vector.is_dictionary(),
            "dictionary storage is not arena-backed"
        );
        let length = self.len(*vector);
        let capacity = self.capacity(*vector);
        let source = vector.start();
        let narrow = vector.is_narrow();
        let target = if narrow { &mut targets.narrow } else { &mut targets.values };
        if narrow {
            self.narrow.compact(source, length, capacity, *target, narrow_undefined());
        } else {
            self.values.compact(source, length, capacity, *target, Value::UNDEFINED);
        }
        vector.place(*target, narrow);
        *target += capacity;
    }

    pub(crate) fn finish_compaction(&mut self, targets: CompactionTargets) {
        self.values.finish_compaction(targets.values);
        self.narrow.finish_compaction(targets.narrow);
    }

    pub(crate) fn memory_bytes(&self) -> usize {
        self.values.memory_bytes()
            + self.narrow.memory_bytes()
            + self.dictionaries.capacity() * size_of::<(u32, FxHashMap<u32, Value>)>()
            + self
                .dictionaries
                .values()
                .map(|values| values.capacity() * size_of::<(u32, Value)>())
                .sum::<usize>()
            + self.free_dictionaries.capacity() * size_of::<u32>()
    }

    pub(crate) fn memory_breakdown(&self) -> (usize, usize) {
        let values = self.values.slots.capacity() * size_of::<Value>()
            + self.narrow.slots.capacity() * size_of::<u32>();
        (values, self.memory_bytes() - values)
    }

    /// Slots in use, slots reserved and free ranges, over both lanes.
    pub(crate) fn stats(&self) -> (usize, usize, usize) {
        let free = |lane_free: &[Vec<u32>; BUCKETS]| lane_free.iter().map(Vec::len).sum::<usize>();
        (
            self.values.slots.len() + self.narrow.slots.len(),
            self.values.slots.capacity() + self.narrow.slots.capacity(),
            free(&self.values.free) + free(&self.narrow.free),
        )
    }

    /// Moves `vector`'s narrow range to the wide lane, or to a dictionary when that lane is
    /// exhausted. The capacity stays.
    #[cold]
    fn widen(&mut self, vector: &mut ValueVec) {
        debug_assert!(vector.is_narrow());
        let (source, len, capacity) = (vector.start(), self.len(*vector), self.capacity(*vector));
        match self.values.allocate(capacity, MAX_ARENA_START, Value::UNDEFINED) {
            Some(start) => {
                for slot in 0..len {
                    self.values.slots[start + slot] = Value::from_narrow(self.narrow.slots[source + slot]);
                }
                self.narrow.release(source, capacity);
                vector.place(start, false);
            }
            None => {
                let values = self.range_values(*vector);
                self.narrow.release(source, capacity);
                vector.use_dictionary(self.allocate_dictionary(values));
            }
        }
    }

    /// Doubles `vector`'s range in its lane (`narrow`, for a vector without one). With the
    /// lane exhausted below `max_start`, the values move to a dictionary.
    fn grow(&mut self, vector: &mut ValueVec, max_start: usize, narrow: bool) {
        let len = self.len(*vector);
        let capacity = self.capacity(*vector);
        let next = if capacity == 0 {
            MIN_CAPACITY
        } else {
            capacity
                .checked_mul(2)
                .expect("property arena capacity overflow")
        };
        let start = if narrow {
            self.narrow.allocate(next, max_start, narrow_undefined())
        } else {
            self.values.allocate(next, max_start, Value::UNDEFINED)
        };
        let Some(start) = start else {
            let values = if self.has_compact_range(*vector) {
                self.range_values(*vector)
            } else {
                vec![Value::UNDEFINED; len]
            };
            self.release(*vector);
            let id = self.allocate_dictionary(values);
            vector.use_dictionary(id);
            return;
        };
        if len != 0 {
            let source = vector.start();
            if narrow {
                self.narrow.slots.copy_within(source..source + len, start);
            } else {
                self.values.slots.copy_within(source..source + len, start);
            }
            self.release(*vector);
        }
        vector.place(start, narrow);
    }

    #[cfg(test)]
    pub(crate) fn migrate_to_dictionary_for_test(&mut self, vector: &mut ValueVec) {
        let narrow = vector.is_narrow();
        self.grow(vector, 0, narrow);
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
        let len = self.len(vector);
        match (len, vector.start() != EMPTY_START as usize) {
            (0, false) => 0,
            (0, true) => MIN_CAPACITY,
            (length, _) => length.next_power_of_two().max(MIN_CAPACITY),
        }
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

    fn bucket(capacity: usize) -> usize {
        debug_assert!(capacity >= MIN_CAPACITY && capacity.is_power_of_two());
        let bucket = capacity.trailing_zeros() as usize - MIN_CAPACITY.trailing_zeros() as usize;
        assert!(bucket < BUCKETS, "property arena bucket overflow");
        bucket
    }
}

/// Where compaction places the next range of each lane (`ValueArena::compact_vector`).
#[derive(Default)]
pub(crate) struct CompactionTargets {
    values: usize,
    narrow: usize,
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
        arena.set(&mut source, 0, Value::number(3.0));
        let dictionary_copy = arena.duplicate(source);
        assert_ne!(dictionary_copy.dictionary_id(), source.dictionary_id());
        arena.set(&mut source, 0, Value::number(4.0));
        assert_eq!(arena.get(dictionary_copy, 0).unwrap().as_number(), Some(3.0));
    }

    #[test]
    fn narrow_ranges_widen_on_a_value_without_a_narrow_form() {
        let mut arena = ValueArena::default();
        arena.register_shape(1, 3);
        arena.register_shape(2, 4);
        let mut vector = arena.with_values(1, &[Value::integer(7), Value::heap(11), Value::TRUE]);
        assert!(vector.is_narrow());
        assert_eq!(arena.values.slots.len(), 0, "every value is narrow");
        let mut references = Vec::new();
        arena.append_heap_references(vector, &mut references);
        assert_eq!(references, [Value::heap(11)]);

        assert!(!arena.set(&mut vector, 0, Value::integer(8)), "a narrow store keeps the range");
        let copy = arena.duplicate(vector);
        assert!(copy.is_narrow());
        assert!(arena.set(&mut vector, 2, Value::number(0.5)), "a double moves the range");
        assert!(!vector.is_narrow());
        assert_eq!(
            (0..3).map(|slot| arena.get(vector, slot).unwrap()).collect::<Vec<_>>(),
            [Value::integer(8), Value::heap(11), Value::number(0.5)]
        );
        assert_eq!(arena.get(copy, 2), Some(Value::TRUE), "the copy keeps its own range");

        let mut grown = copy;
        arena.push(&mut grown, Value::number(1.5));
        grown.set_auxiliary(2);
        assert!(!grown.is_narrow());
        assert_eq!(arena.get(grown, 3).unwrap().as_number(), Some(1.5));
        assert_eq!(arena.get(grown, 1), Some(Value::heap(11)));
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
        arena.finish_compaction(CompactionTargets {
            values: arena.values.slots.len(),
            narrow: arena.narrow.slots.len(),
        });
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
        let mut vector = arena.with_values(1, &[Value::number(11.5), Value::number(22.5)]);

        let start = vector.start();
        let range_count = arena.values.slots.len();
        // A zero address limit exercises the same checked exhaustion branch
        // without allocating billions of arena entries in a unit test.
        arena.migrate_to_dictionary_for_test(&mut vector);
        let dictionary_id = vector.dictionary_id();

        assert!(vector.is_dictionary());
        assert_eq!(arena.get(vector, 0).unwrap().as_number(), Some(11.5));
        assert_eq!(arena.get(vector, 1).unwrap().as_number(), Some(22.5));
        arena.set(&mut vector, 1, Value::number(33.5));
        assert_eq!(arena.get(vector, 1).unwrap().as_number(), Some(33.5));

        let mut references = Vec::new();
        arena.append_heap_references(vector, &mut references);
        assert!(references.is_empty(), "numbers are not heap references");
        assert_eq!(arena.values.slots.len(), range_count);
        assert!(
            arena
                .values
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
        arena.set(&mut vector, 1, Value::DELETED);

        arena.migrate_to_dictionary_for_test(&mut vector);

        let mut roots = Vec::new();
        arena.append_heap_references(vector, &mut roots);
        assert_eq!(roots, [Value::heap(11)]);
    }
}
