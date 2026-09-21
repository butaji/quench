use crate::Value;

const EMPTY_START: u32 = u32::MAX;
const MIN_CAPACITY: usize = 4;
const BUCKETS: usize = 14;

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
        self.auxiliary
    }

    pub(crate) fn set_auxiliary(&mut self, value: u32) {
        self.auxiliary = value;
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
    shape_lengths: Vec<u16>,
}

impl Default for ValueArena {
    fn default() -> Self {
        Self {
            values: vec![],
            free: Default::default(),
            shape_lengths: vec![0],
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
    }

    pub(crate) fn register_shape(&mut self, shape: u32, length: usize) {
        assert!(
            length <= u16::MAX as usize,
            "object has too many properties"
        );
        let shape = shape as usize;
        if self.shape_lengths.len() <= shape {
            self.shape_lengths.resize(shape + 1, 0);
        }
        self.shape_lengths[shape] = length as u16;
    }

    pub(crate) fn get(&self, vector: ValueVec, index: usize) -> Option<Value> {
        (index < self.len(vector)).then(|| self.values[vector.start as usize + index])
    }

    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    pub(crate) unsafe fn get_unchecked(&self, vector: ValueVec, index: usize) -> Value {
        debug_assert!(index < self.len(vector));
        // SAFETY: caller provides the initialized-range invariant; every
        // vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked(vector.start as usize + index) }
    }

    pub(crate) fn set(&mut self, vector: ValueVec, index: usize, value: Value) {
        assert!(index < self.len(vector));
        self.values[vector.start as usize + index] = value;
    }

    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    pub(crate) unsafe fn set_unchecked(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(index < self.len(vector));
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe {
            *self.values.get_unchecked_mut(vector.start as usize + index) = value;
        }
    }

    pub(crate) fn push(&mut self, vector: &mut ValueVec, value: Value) {
        let len = self.len(*vector);
        if len == self.capacity(*vector) {
            self.grow(vector);
        }
        self.values[vector.start as usize + len] = value;
    }

    pub(crate) fn pair(&mut self, auxiliary: u32, first: Value, second: Value) -> ValueVec {
        let start = self.allocate(MIN_CAPACITY);
        self.values[start] = first;
        self.values[start + 1] = second;
        ValueVec {
            start: start as u32,
            auxiliary,
        }
    }

    pub(crate) fn values(&self, vector: ValueVec) -> &[Value] {
        let len = self.len(vector);
        if len == 0 {
            return &[];
        }
        &self.values[vector.start as usize..vector.start as usize + len]
    }

    pub(crate) fn release(&mut self, vector: ValueVec) {
        let capacity = self.capacity(vector);
        if capacity != 0 {
            self.free[Self::bucket(capacity)].push(vector.start);
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
            capacity * 2
        };
        assert!(next <= u16::MAX as usize, "value vector too large");
        let start = self.allocate(next);
        if len != 0 {
            self.values
                .copy_within(vector.start as usize..vector.start as usize + len, start);
            self.release(*vector);
        }
        vector.start = start as u32;
    }

    #[cfg(any(test, feature = "profile-memory"))]
    pub(crate) fn vector_stats(&self, vector: ValueVec) -> (usize, usize) {
        (self.len(vector), self.capacity(vector))
    }

    fn len(&self, vector: ValueVec) -> usize {
        self.shape_lengths[vector.auxiliary as usize] as usize
    }

    fn capacity(&self, vector: ValueVec) -> usize {
        let len = self.len(vector);
        if len == 0 {
            0
        } else {
            len.next_power_of_two().max(MIN_CAPACITY)
        }
    }

    fn allocate(&mut self, capacity: usize) -> usize {
        let bucket = Self::bucket(capacity);
        if let Some(start) = self.free[bucket].pop() {
            return start as usize;
        }
        let start = self.values.len();
        let required = start + capacity;
        // Vec's 2x policy leaves a large unused tail once long-lived heaps
        // cross into millions of property slots. Preserve that policy for
        // small programs, then grow by one third so the arena remains dense
        // without turning every object allocation into a reallocation.
        if required > self.values.capacity() && self.values.capacity() >= 65_536 {
            let target = required.max(self.values.capacity() + self.values.capacity() / 3);
            self.values.reserve_exact(target - self.values.len());
        }
        self.values.resize(start + capacity, Value::UNDEFINED);
        start
    }

    fn bucket(capacity: usize) -> usize {
        debug_assert!(capacity >= MIN_CAPACITY && capacity.is_power_of_two());
        capacity.trailing_zeros() as usize - MIN_CAPACITY.trailing_zeros() as usize
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
