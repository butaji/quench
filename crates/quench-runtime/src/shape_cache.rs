//! Small, bounded inline-cache storage for object property lookups.
//!
//! This module intentionally has no connection to object semantics yet.  It is
//! the compact storage primitive that a future `ObjectData` implementation can
//! use once shapes and slots are migrated.

/// A stable identifier for an object shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId(pub u64);

/// A stable identifier for a property within a shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PropertyId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeCacheEntry {
    pub guard_shape: ShapeId,
    pub property: PropertyId,
    pub slot: u32,
}

/// A fixed-capacity, allocation-free monomorphic shape cache.
///
/// On overflow, entries are replaced in round-robin order. Inserting an
/// existing `(shape, property)` pair updates its slot without consuming a
/// replacement turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeCache<const N: usize = 4> {
    entries: [Option<ShapeCacheEntry>; N],
    next_replacement: usize,
}

impl<const N: usize> ShapeCache<N> {
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            next_replacement: 0,
        }
    }

    pub fn lookup(&self, guard_shape: ShapeId, property: PropertyId) -> Option<u32> {
        self.entries.iter().flatten().find_map(|entry| {
            (entry.guard_shape == guard_shape && entry.property == property).then_some(entry.slot)
        })
    }

    pub fn insert(&mut self, guard_shape: ShapeId, property: PropertyId, slot: u32) {
        let entry = ShapeCacheEntry {
            guard_shape,
            property,
            slot,
        };
        if let Some(existing) =
            self.entries.iter_mut().flatten().find(|existing| {
                existing.guard_shape == guard_shape && existing.property == property
            })
        {
            *existing = entry;
            return;
        }

        if N == 0 {
            return;
        }
        self.entries[self.next_replacement] = Some(entry);
        self.next_replacement = (self.next_replacement + 1) % N;
    }

    pub const fn is_empty(&self) -> bool {
        let mut index = 0;
        while index < N {
            if self.entries[index].is_some() {
                return false;
            }
            index += 1;
        }
        true
    }
}

impl<const N: usize> Default for ShapeCache<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_hits_and_misses() {
        let mut cache = ShapeCache::<2>::new();
        let shape = ShapeId(1);
        let property = PropertyId(2);
        assert_eq!(cache.lookup(shape, property), None);
        cache.insert(shape, property, 7);
        assert_eq!(cache.lookup(shape, property), Some(7));
        assert_eq!(cache.lookup(ShapeId(9), property), None);
        assert_eq!(cache.lookup(shape, PropertyId(9)), None);
    }

    #[test]
    fn inserting_existing_entry_updates_without_replacement() {
        let mut cache = ShapeCache::<2>::new();
        cache.insert(ShapeId(1), PropertyId(1), 10);
        cache.insert(ShapeId(1), PropertyId(1), 11);
        cache.insert(ShapeId(2), PropertyId(2), 20);
        assert_eq!(cache.lookup(ShapeId(1), PropertyId(1)), Some(11));
        assert_eq!(cache.lookup(ShapeId(2), PropertyId(2)), Some(20));
    }

    #[test]
    fn overflow_replaces_entries_round_robin() {
        let mut cache = ShapeCache::<2>::new();
        cache.insert(ShapeId(1), PropertyId(1), 10);
        cache.insert(ShapeId(2), PropertyId(2), 20);
        cache.insert(ShapeId(3), PropertyId(3), 30);
        assert_eq!(cache.lookup(ShapeId(1), PropertyId(1)), None);
        assert_eq!(cache.lookup(ShapeId(2), PropertyId(2)), Some(20));
        assert_eq!(cache.lookup(ShapeId(3), PropertyId(3)), Some(30));
    }

    #[test]
    fn full_width_guards_do_not_alias() {
        let mut cache = ShapeCache::<2>::new();
        let low = ShapeId(1);
        let high = ShapeId(1u64 << 32 | 1);
        let property = PropertyId(7);
        cache.insert(low, property, 10);
        cache.insert(high, property, 20);
        assert_eq!(cache.lookup(low, property), Some(10));
        assert_eq!(cache.lookup(high, property), Some(20));

        let low_property = PropertyId(2);
        let high_property = PropertyId(1u64 << 32 | 2);
        cache.insert(low, low_property, 30);
        cache.insert(low, high_property, 40);
        assert_eq!(cache.lookup(low, low_property), Some(30));
        assert_eq!(cache.lookup(low, high_property), Some(40));
    }
}
