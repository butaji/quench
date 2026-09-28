use crate::Value;
use rustc_hash::FxHashMap;

const START_MASK: u32 = 0x3fff_ffff;
const NON_EXTENSIBLE: u32 = 1 << 30;
const FROZEN: u32 = 1 << 31;
const DICTIONARY_STORAGE: u32 = 1 << 31;
const SHAPE_MASK: u32 = !DICTIONARY_STORAGE;
const EMPTY_START: u32 = START_MASK;
const MAX_ARENA_START: usize = EMPTY_START as usize;
const MIN_CAPACITY: usize = 4;
const BUCKETS: usize = 32;

/// Compact metadata for values owned by the heap's canonical property arena.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueVec {
    start: u32,
    auxiliary: u32,
}

impl ValueVec {
    pub(crate) const fn new() -> Self {
        Self {
            start: EMPTY_START,
            auxiliary: 0,
        }
    }

    pub(crate) const fn auxiliary(self) -> u32 {
        self.auxiliary & SHAPE_MASK
    }

    pub(crate) fn set_auxiliary(&mut self, value: u32) {
        assert!(value <= SHAPE_MASK, "object shape table exhausted");
        self.auxiliary = (self.auxiliary & DICTIONARY_STORAGE) | value;
    }

    fn is_dictionary(self) -> bool {
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

    fn start(self) -> usize {
        (self.start & START_MASK) as usize
    }

    pub(crate) fn start_offset(self) -> usize {
        debug_assert!(!self.is_dictionary());
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
    shape_lengths: Vec<u32>,
    dictionaries: FxHashMap<u32, FxHashMap<u32, Value>>,
    free_dictionaries: Vec<u32>,
    next_dictionary: u32,
}

impl Default for ValueArena {
    fn default() -> Self {
        Self {
            values: vec![],
            free: Default::default(),
            shape_lengths: vec![0],
            dictionaries: FxHashMap::default(),
            free_dictionaries: Vec::new(),
            next_dictionary: 0,
        }
    }
}

impl ValueArena {
    pub(crate) fn reset(&mut self) {
        self.values.clear();
        for bucket in &mut self.free {
            bucket.clear();
        }
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
    pub(crate) unsafe fn get_unchecked(&self, vector: ValueVec, index: usize) -> Value {
        debug_assert!(index < self.len(vector));
        if vector.is_dictionary() {
            return self.dictionaries[&vector.dictionary_id()][&(index as u32)];
        }
        // SAFETY: caller provides the initialized-range invariant; every
        // vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked(vector.start() + index) }
    }

    pub(crate) fn set(&mut self, vector: ValueVec, index: usize, value: Value) {
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
    pub(crate) unsafe fn set_unchecked(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(index < self.len(vector));
        if vector.is_dictionary() {
            self.dictionaries
                .get_mut(&vector.dictionary_id())
                .expect("object dictionary exists")
                .insert(index as u32, value);
            return;
        }
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe {
            *self.values.get_unchecked_mut(vector.start() + index) = value;
        }
    }

    pub(crate) fn push(&mut self, vector: &mut ValueVec, value: Value) {
        let len = self.len(*vector);
        if vector.is_dictionary() {
            self.dictionaries
                .get_mut(&vector.dictionary_id())
                .expect("object dictionary exists")
                .insert(len as u32, value);
            return;
        }
        if len == self.capacity(*vector) {
            self.grow(vector);
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

    pub(crate) fn pair(&mut self, auxiliary: u32, first: Value, second: Value) -> ValueVec {
        let mut vector = ValueVec {
            start: EMPTY_START,
            auxiliary,
        };
        if let Some(start) = self.allocate(MIN_CAPACITY) {
            vector.start = start as u32;
            self.values[start] = first;
            self.values[start + 1] = second;
        } else {
            let mut values = vec![Value::UNDEFINED; self.len(vector)];
            if let Some(value) = values.get_mut(0) {
                *value = first;
            }
            if let Some(value) = values.get_mut(1) {
                *value = second;
            }
            let id = self.allocate_dictionary(values);
            vector.use_dictionary(id);
        }
        vector
    }

    pub(crate) fn append_live_values(&self, vector: ValueVec, output: &mut Vec<Value>) {
        let len = self.len(vector);
        if vector.is_dictionary() {
            let values = &self.dictionaries[&vector.dictionary_id()];
            output.extend(
                (0..len)
                    .filter_map(|slot| values.get(&(slot as u32)).copied())
                    .filter(|value| !value.is_deleted()),
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
                .filter(|value| !value.is_deleted()),
        );
    }

    pub(crate) fn release(&mut self, vector: ValueVec) {
        if vector.is_dictionary() {
            let id = vector.dictionary_id();
            self.dictionaries.remove(&id);
            self.free_dictionaries.push(id);
            return;
        }
        let capacity = self.capacity(vector);
        if capacity != 0 {
            self.free[Self::bucket(capacity)].push(vector.start & START_MASK);
        }
    }

    pub(crate) fn has_released_ranges(&self) -> bool {
        self.free.iter().any(|bucket| !bucket.is_empty())
    }

    pub(crate) fn has_compact_range(&self, vector: ValueVec) -> bool {
        !vector.is_dictionary() && vector.start() != EMPTY_START as usize
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
        for bucket in &mut self.free {
            bucket.clear();
        }
    }

    #[cfg(feature = "profile-memory")]
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

    #[cfg(feature = "profile-memory")]
    pub(crate) fn stats(&self) -> (usize, usize, usize) {
        (
            self.values.len(),
            self.values.capacity(),
            self.free.iter().map(Vec::len).sum(),
        )
    }

    fn grow(&mut self, vector: &mut ValueVec) {
        let len = self.len(*vector);
        let capacity = self.capacity(*vector);
        let next = if capacity == 0 {
            MIN_CAPACITY
        } else {
            capacity
                .checked_mul(2)
                .expect("property arena capacity overflow")
        };
        let Some(start) = self.allocate(next) else {
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

    #[cfg(any(test, feature = "profile-memory"))]
    pub(crate) fn vector_stats(&self, vector: ValueVec) -> (usize, usize) {
        (self.len(vector), self.capacity(vector))
    }

    fn len(&self, vector: ValueVec) -> usize {
        self.shape_lengths[vector.auxiliary() as usize] as usize
    }

    fn capacity(&self, vector: ValueVec) -> usize {
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

    fn allocate(&mut self, capacity: usize) -> Option<usize> {
        let bucket = Self::bucket(capacity);
        if let Some(start) = self.free[bucket].pop() {
            return Some(start as usize);
        }
        let start = self.values.len();
        if start >= MAX_ARENA_START {
            return None;
        }
        let required = start.checked_add(capacity)?;
        // Vec's 2x policy leaves a large unused tail once long-lived heaps
        // cross into millions of property slots. Preserve that policy for
        // small programs, then grow by one third so the arena remains dense
        // without turning every object allocation into a reallocation.
        if required > self.values.capacity() && self.values.capacity() >= 65_536 {
            let target = required.max(self.values.capacity() + self.values.capacity() / 3);
            self.values.reserve_exact(target - self.values.len());
        }
        self.values.resize(start + capacity, Value::UNDEFINED);
        Some(start)
    }

    fn allocate_dictionary(&mut self, values: Vec<Value>) -> u32 {
        let id = self.free_dictionaries.pop().unwrap_or_else(|| {
            let id = self.next_dictionary;
            self.next_dictionary = id
                .checked_add(1)
                .expect("property dictionary table exhausted");
            assert!(id <= START_MASK, "property dictionary table exhausted");
            id
        });
        self.dictionaries.insert(
            id,
            values
                .into_iter()
                .enumerate()
                .map(|(slot, value)| (slot as u32, value))
                .collect(),
        );
        id
    }

    fn bucket(capacity: usize) -> usize {
        debug_assert!(capacity >= MIN_CAPACITY && capacity.is_power_of_two());
        let bucket = capacity.trailing_zeros() as usize - MIN_CAPACITY.trailing_zeros() as usize;
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
}
