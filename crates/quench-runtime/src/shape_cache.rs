//! Small, bounded inline-cache storage for object property lookups.
//!
//! This module stores only the compact physical key/state view. Object
//! semantics remain in the property gateway; the live quickening site owns
//! admission and invalidation around this storage.

/// The cache consumes the runtime's canonical identity keys. Keeping aliases
/// here exposes the cache vocabulary without creating a second shape/property
/// universe that could drift from `ObjectData` and its transition facts.
pub use crate::identity::{PropertyKeyId as PropertyId, ShapeId};

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
    entries: crate::quickening::GenericInlineCache<(ShapeId, PropertyId), u32, N>,
}

impl<const N: usize> ShapeCache<N> {
    pub fn new() -> Self {
        Self {
            entries: crate::quickening::GenericInlineCache::new(),
        }
    }

    pub fn lookup(&self, guard_shape: ShapeId, property: PropertyId) -> Option<u32> {
        self.entries.lookup(&(guard_shape, property))
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = ShapeCacheEntry> + '_ {
        self.entries.entries().map(|(key, slot)| ShapeCacheEntry {
            guard_shape: key.0,
            property: key.1,
            slot: *slot,
        })
    }

    /// Move a hit to the first probe position. The decision to call this is
    /// supplied by the generic execution profile; cache capacity is unchanged.
    pub fn promote(&mut self, guard_shape: ShapeId, property: PropertyId) {
        self.entries.promote(&(guard_shape, property));
    }

    pub fn insert(&mut self, guard_shape: ShapeId, property: PropertyId, slot: u32) {
        let key = (guard_shape, property);
        if self.entries.lookup(&key).is_some() {
            // A refreshed slot is an explicit physical update, not a cache
            // hit: preserve the generic key/state contract without consuming
            // a replacement turn.
            self.entries.insert_state(key, slot);
        } else {
            // New shape state goes through the generic idempotent
            // key→state admission path used by all IC views.
            let _ = self.entries.observe(key, |_| Some(slot));
        }
    }

    /// Invalidate every entry guarded by a changed shape.
    ///
    /// Invalidation is an explicit state transition: a miss after this call
    /// must use the complete property semantics and may repopulate the cache
    /// with a newly proven slot.
    pub fn invalidate_shape(&mut self, guard_shape: ShapeId) {
        self.entries.retain(|(shape, _), _| *shape != guard_shape);
    }

    /// Drop all physical cache state without changing the semantic owner.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        // GenericInlineCache is the sole physical storage; this method stays
        // a cheap view for callers that only need the bounded cardinality.
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
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
    fn canonical_identity_widths_are_used() {
        assert_eq!(std::mem::size_of::<ShapeId>(), std::mem::size_of::<u32>());
        assert_eq!(
            std::mem::size_of::<PropertyId>(),
            std::mem::size_of::<u32>()
        );
    }

    #[test]
    fn invalidation_returns_changed_shapes_to_the_fallback_state() {
        let mut cache = ShapeCache::<4>::new();
        cache.insert(ShapeId(1), PropertyId(1), 10);
        cache.insert(ShapeId(1), PropertyId(2), 11);
        cache.insert(ShapeId(2), PropertyId(1), 20);
        assert_eq!(cache.len(), 3);

        cache.invalidate_shape(ShapeId(1));
        assert_eq!(cache.lookup(ShapeId(1), PropertyId(1)), None);
        assert_eq!(cache.lookup(ShapeId(1), PropertyId(2)), None);
        assert_eq!(cache.lookup(ShapeId(2), PropertyId(1)), Some(20));

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        cache.insert(ShapeId(7), PropertyId(7), 70);
        cache.insert(ShapeId(8), PropertyId(8), 80);
        cache.insert(ShapeId(9), PropertyId(9), 90);
        cache.insert(ShapeId(10), PropertyId(10), 100);
        cache.insert(ShapeId(11), PropertyId(11), 110);
        assert_eq!(cache.lookup(ShapeId(7), PropertyId(7)), None);
        assert_eq!(cache.lookup(ShapeId(8), PropertyId(8)), Some(80));
        assert_eq!(cache.lookup(ShapeId(9), PropertyId(9)), Some(90));
        assert_eq!(cache.lookup(ShapeId(10), PropertyId(10)), Some(100));
        assert_eq!(cache.lookup(ShapeId(11), PropertyId(11)), Some(110));
    }
}
