/// Baseline cache-line width used for layout decisions.
///
/// The runtime does not assume this is the host's only cache size; it uses
/// 64-byte lines as the portable lower bound for hot metadata placement.
pub const CACHE_LINE_BYTES: usize = 64;
/// Bytes reserved for the frequently accessed portion of an object header.
pub const HOT_HEADER_BYTES: usize = 16;

use crate::identity::HeapRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeapStats {
    /// Bytes reserved from the host allocator (page capacity).
    pub reserved_bytes: u64,
    /// Bytes committed for object storage (size-class slots in use).
    pub committed_bytes: u64,
    /// Approximate bytes occupied by live objects.
    pub live_bytes: u64,
    /// Bytes owned by host/native allocations.
    pub external_bytes: u64,
    pub allocated_bytes: u64,
    pub live_objects: usize,
    pub remembered_writes: u64,
    pub collections: u64,
    /// Number of live allocations in each power-of-two size class.
    pub size_classes: [u64; 8],
    /// Objects promoted out of the nursery.
    pub promoted_objects: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Generation {
    Nursery,
    Old,
}
impl Generation {
    #[inline]
    pub fn is_nursery(self) -> bool {
        matches!(self, Self::Nursery)
    }

    #[inline]
    pub fn is_old(self) -> bool {
        matches!(self, Self::Old)
    }
}

#[derive(Debug)]
pub struct HeapArena<T> {
    values: Vec<Option<T>>,
    sizes: Vec<usize>,
    generations: Vec<Generation>,
    free: Vec<u32>,
    stats: HeapStats,
    nursery_limit: usize,
    remembered: Vec<HeapRef>,
    page_size: usize,
    gc_threshold: u64,
    bytes_since_gc: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NurseryOverflow;

impl<T> Default for HeapArena<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            sizes: Vec::new(),
            generations: Vec::new(),
            free: Vec::new(),
            stats: HeapStats::default(),
            nursery_limit: 4096,
            remembered: Vec::new(),
            page_size: 4096,
            gc_threshold: 4096,
            bytes_since_gc: 0,
        }
    }
}

impl<T> HeapArena<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_gc_threshold(mut self, bytes: u64) -> Self {
        self.gc_threshold = bytes.max(1);
        self
    }

    pub fn should_collect(&self) -> bool {
        self.bytes_since_gc >= self.gc_threshold
    }
    pub fn gc_threshold_remaining(&self) -> u64 {
        self.gc_threshold.saturating_sub(self.bytes_since_gc)
    }

    pub fn charge_external(&mut self, bytes: usize) {
        self.stats.external_bytes = self.stats.external_bytes.saturating_add(bytes as u64);
    }

    pub fn release_external(&mut self, bytes: usize) {
        self.stats.external_bytes = self.stats.external_bytes.saturating_sub(bytes as u64);
    }

    pub fn collect_unrooted(&mut self, roots: &RootRegistry) -> usize {
        let keep: std::collections::HashSet<_> =
            std::collections::HashSet::with_capacity(roots.root_count());
        let keep = roots.all_roots().fold(keep, |mut keep, root| {
            keep.insert(root);
            keep
        });
        let mut reclaimed = 0;
        for index in 0..self.values.len() {
            if self.values[index].is_some() && !keep.contains(&HeapRef(index as u32)) {
                let size = self.sizes[index];
                let class_index = Self::size_class_index(size);
                self.stats.size_classes[class_index] =
                    self.stats.size_classes[class_index].saturating_sub(1);
                self.values[index] = None;
                self.stats.live_bytes = self.stats.live_bytes.saturating_sub(size as u64);
                self.free.push(index as u32);
                reclaimed += 1;
            } else if self.values[index].is_some() && self.generations[index] == Generation::Nursery
            {
                self.generations[index] = Generation::Old;
                self.stats.promoted_objects = self.stats.promoted_objects.saturating_add(1);
            }
        }
        self.stats.live_objects = self.live_len();
        self.stats.collections = self.stats.collections.saturating_add(1);
        self.bytes_since_gc = 0;

        self.recompute_committed();
        reclaimed
    }
    pub fn collect_if_needed(&mut self, roots: &RootRegistry) -> Option<usize> {
        self.should_collect().then(|| self.collect_unrooted(roots))
    }

    /// Configure the nursery slot budget. Allocation itself remains a bump
    /// (append) operation; reclaimed slots are reused before growing it.
    pub fn with_nursery_limit(mut self, limit: usize) -> Self {
        self.nursery_limit = limit;
        self
    }
    /// Remaining append slots before nursery allocation reports overflow.
    pub fn nursery_remaining(&self) -> usize {
        self.nursery_limit.saturating_sub(self.values.len())
    }

    pub fn nursery_is_full(&self) -> bool {
        self.free.is_empty() && self.values.len() >= self.nursery_limit
    }

    pub fn allocate(&mut self, value: T) -> HeapRef {
        self.allocate_sized(value, std::mem::size_of::<T>().max(1))
    }

    pub fn try_allocate(&mut self, value: T) -> Result<HeapRef, NurseryOverflow> {
        self.try_allocate_sized(value, std::mem::size_of::<T>().max(1))
    }

    pub fn try_allocate_sized(
        &mut self,
        value: T,
        bytes: usize,
    ) -> Result<HeapRef, NurseryOverflow> {
        if self.nursery_is_full() {
            return Err(Self::nursery_overflow());
        }
        Ok(self.allocate_sized(value, bytes))
    }
    #[cold]
    fn nursery_overflow() -> NurseryOverflow {
        NurseryOverflow
    }

    /// Allocate immutable metadata directly in the old generation.
    pub fn allocate_immutable(&mut self, value: T) -> HeapRef {
        let reference = self.allocate_sized(value, std::mem::size_of::<T>().max(1));
        if let Some(generation) = self.generations.get_mut(reference.0 as usize) {
            *generation = Generation::Old;
        }
        reference
    }
    pub fn allocate_sized(&mut self, value: T, bytes: usize) -> HeapRef {
        let class_index = Self::size_class_index(bytes);
        self.stats.allocated_bytes = self.stats.allocated_bytes.saturating_add(bytes as u64);
        self.stats.live_bytes = self.stats.live_bytes.saturating_add(bytes as u64);
        self.stats.size_classes[class_index] =
            self.stats.size_classes[class_index].saturating_add(1);
        self.stats.live_objects = self.live_len().saturating_add(1);
        self.bytes_since_gc = self.bytes_since_gc.saturating_add(bytes as u64);
        if let Some(index) = self.free.pop() {
            self.values[index as usize] = Some(value);
            self.sizes[index as usize] = bytes;
            self.generations[index as usize] = Generation::Nursery;
            self.recompute_committed();
            return HeapRef(index);
        }
        let index = u32::try_from(self.values.len()).unwrap_or(u32::MAX);
        self.values.push(Some(value));
        self.sizes.push(bytes);
        self.generations.push(Generation::Nursery);
        self.recompute_committed();
        HeapRef(index)
    }

    fn size_class_index(bytes: usize) -> usize {
        let class = bytes.max(1).next_power_of_two().min(4096);
        (class.trailing_zeros() as usize).saturating_sub(3).min(7)
    }
    pub fn size_class_for(bytes: usize) -> usize {
        Self::size_class_index(bytes)
    }

    fn recompute_committed(&mut self) {
        let mut committed = 0u64;
        for (value, &size) in self.values.iter().zip(&self.sizes) {
            if value.is_some() {
                let class = size.max(1).next_power_of_two().min(self.page_size);
                committed += class.div_ceil(self.page_size) as u64 * self.page_size as u64;
            }
        }
        self.stats.committed_bytes = committed;
        self.stats.reserved_bytes = committed;
    }
    pub fn page_count(&self) -> usize {
        self.stats
            .committed_bytes
            .div_ceil(self.page_size.max(1) as u64) as usize
    }

    pub fn stats(&self) -> HeapStats {
        self.stats
    }
    pub fn accounted_bytes(&self) -> u64 {
        self.stats
            .committed_bytes
            .saturating_add(self.stats.external_bytes)
    }

    /// Record an old-to-young mutation without atomics (arenas are isolate-owned).
    pub fn record_write(&mut self, owner: HeapRef, target: HeapRef) {
        if owner != target && !self.remembered.contains(&owner) {
            self.remembered.push(owner);
            self.stats.remembered_writes = self.stats.remembered_writes.saturating_add(1);
        }
        let _ = target;
    }

    pub fn remembered_len(&self) -> usize {
        self.remembered.len()
    }

    pub fn remembered(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.remembered.iter().copied()
    }

    pub fn get(&self, reference: HeapRef) -> Option<&T> {
        self.values
            .get(reference.0 as usize)
            .and_then(Option::as_ref)
    }

    pub fn get_mut(&mut self, reference: HeapRef) -> Option<&mut T> {
        self.values
            .get_mut(reference.0 as usize)
            .and_then(Option::as_mut)
    }

    pub fn generation_counts(&self) -> (usize, usize) {
        self.generations
            .iter()
            .zip(&self.values)
            .filter(|(_, value)| value.is_some())
            .fold((0, 0), |(nursery, old), (generation, _)| match generation {
                Generation::Nursery => (nursery + 1, old),
                Generation::Old => (nursery, old + 1),
            })
    }

    pub fn reclaim(&mut self, reference: HeapRef) -> Option<T> {
        let index = reference.0 as usize;
        let value = self.values.get_mut(index)?.take()?;
        self.stats.live_bytes = self
            .stats
            .live_bytes
            .saturating_sub(self.sizes[index] as u64);
        self.free.push(reference.0);
        self.stats.live_objects = self.live_len();
        Some(value)
    }

    pub fn live_len(&self) -> usize {
        self.values.iter().filter(|value| value.is_some()).count()
    }
    pub fn is_empty(&self) -> bool {
        self.live_len() == 0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifetimeDomain {
    Realm,
    Module,
    Request,
    Temporary,
    Continuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSet {
    domain: LifetimeDomain,
    refs: Vec<HeapRef>,
}

impl RootSet {
    pub fn new(domain: LifetimeDomain) -> Self {
        Self {
            domain,
            refs: Vec::new(),
        }
    }

    pub fn domain(&self) -> LifetimeDomain {
        self.domain
    }

    pub fn insert(&mut self, reference: HeapRef) {
        if !self.refs.contains(&reference) {
            self.refs.push(reference);
        }
    }

    pub fn remove(&mut self, reference: HeapRef) {
        self.refs.retain(|candidate| *candidate != reference);
    }

    pub fn iter(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.refs.iter().copied()
    }
    pub fn len(&self) -> usize {
        self.refs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RootRegistry {
    sets: Vec<RootSet>,
}

impl RootRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn roots(&self, domain: LifetimeDomain) -> impl Iterator<Item = HeapRef> + '_ {
        self.sets
            .iter()
            .filter(move |set| set.domain() == domain)
            .flat_map(RootSet::iter)
    }

    pub fn all_roots(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.sets.iter().flat_map(RootSet::iter)
    }
    pub fn root_count(&self) -> usize {
        self.all_roots().count()
    }

    pub fn domain_count(&self) -> usize {
        self.sets.len()
    }

    pub fn contains(&self, domain: LifetimeDomain, reference: HeapRef) -> bool {
        self.sets
            .iter()
            .find(|set| set.domain() == domain)
            .is_some_and(|set| set.iter().any(|candidate| candidate == reference))
    }

    pub fn add(&mut self, domain: LifetimeDomain, reference: HeapRef) {
        self.set(domain).insert(reference);
    }

    pub fn remove(&mut self, domain: LifetimeDomain, reference: HeapRef) {
        let Some(index) = self.sets.iter().position(|set| set.domain() == domain) else {
            return;
        };
        self.sets[index].remove(reference);
        if self.sets[index].iter().next().is_none() {
            self.sets.remove(index);
        }
    }

    pub fn clear(&mut self, domain: LifetimeDomain) {
        self.sets.retain(|set| set.domain() != domain);
    }

    fn set(&mut self, domain: LifetimeDomain) -> &mut RootSet {
        if let Some(index) = self.sets.iter().position(|set| set.domain() == domain) {
            return &mut self.sets[index];
        }
        let index = self.sets.len();
        self.sets.push(RootSet::new(domain));
        &mut self.sets[index]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Generation, HeapArena, LifetimeDomain, RootRegistry, CACHE_LINE_BYTES, HOT_HEADER_BYTES,
    };
    use crate::identity::HeapRef;

    #[test]
    fn root_domains_are_enumerable_and_reclaimable() {
        let mut registry = RootRegistry::new();
        registry.add(LifetimeDomain::Realm, HeapRef(1));
        registry.add(LifetimeDomain::Request, HeapRef(2));
        registry.add(LifetimeDomain::Request, HeapRef(2));
        assert!(registry.contains(LifetimeDomain::Request, HeapRef(2)));
        assert_eq!(registry.all_roots().count(), 2);
        assert_eq!(registry.root_count(), 2);
        registry.clear(LifetimeDomain::Request);
        assert!(!registry.contains(LifetimeDomain::Request, HeapRef(2)));
        assert_eq!(registry.domain_count(), 1);
        assert_eq!(registry.all_roots().collect::<Vec<_>>(), vec![HeapRef(1)]);
        registry.remove(LifetimeDomain::Realm, HeapRef(1));
        assert_eq!(registry.all_roots().count(), 0);
    }

    #[test]
    fn arena_reuses_reclaimed_heap_references() {
        let mut arena = HeapArena::new();
        let first = arena.allocate(String::from("first"));
        assert!(arena.page_count() >= 1);
        assert_eq!(arena.get(first).map(String::as_str), Some("first"));
        assert_eq!(arena.reclaim(first).as_deref(), Some("first"));
        let second = arena.allocate(String::from("second"));
        assert_eq!(first, second);
        if let Some(value) = arena.get_mut(second) {
            value.push('!');
        }
        assert_eq!(arena.get(second).map(String::as_str), Some("second!"));
        assert_eq!(arena.live_len(), 1);
    }
    #[test]
    fn nursery_overflow_and_write_barrier_are_explicit() {
        let mut arena = HeapArena::new().with_nursery_limit(1);
        assert_eq!(arena.nursery_remaining(), 1);
        let owner = arena.try_allocate(1u8).expect("first nursery slot");
        assert_eq!(arena.nursery_remaining(), 0);
        assert!(arena.try_allocate(2u8).is_err());
        arena.charge_external(99);
        assert_eq!(arena.accounted_bytes(), arena.stats().committed_bytes + 99);
        arena.record_write(owner, HeapRef(9));
        assert_eq!(arena.stats().remembered_writes, 1);
        assert_eq!(arena.stats().allocated_bytes, 1);
    }

    #[test]
    fn collection_tracks_liveness_and_promotes_roots() {
        let mut arena = HeapArena::new().with_gc_threshold(1);
        let kept = arena.allocate_sized(1u8, 24);
        assert_eq!(arena.gc_threshold_remaining(), 0);
        let dropped = arena.allocate_sized(2u8, 40);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, kept);
        assert!(arena.should_collect());
        assert_eq!(arena.collect_unrooted(&roots), 1);
        assert!(arena.get(kept).is_some());
        assert!(arena.get(dropped).is_none());
        assert_eq!(arena.stats().live_bytes, 24);
        assert_eq!(arena.stats().promoted_objects, 1);
        assert_eq!(arena.stats().size_classes.iter().sum::<u64>(), 1);
        assert_eq!(HeapArena::<u8>::size_class_for(40), 3);
        assert!(arena.stats().committed_bytes >= 4096);
    }
    #[test]
    fn conditional_collection_resets_threshold_after_reclaim() {
        let mut arena = HeapArena::new().with_gc_threshold(1);
        arena.allocate_sized(1u8, 8);
        let roots = RootRegistry::new();
        assert_eq!(arena.collect_if_needed(&roots), Some(1));
        assert!(!arena.should_collect());
        assert!(Generation::Nursery.is_nursery());
        assert!(Generation::Old.is_old());
    }
    #[test]
    fn external_accounting_releases_without_underflow() {
        let mut arena = HeapArena::<u8>::new();
        arena.charge_external(64);
        arena.release_external(16);
        assert_eq!(arena.stats().external_bytes, 48);
        arena.release_external(usize::MAX);
        assert_eq!(arena.stats().external_bytes, 0);
    }
    #[test]
    fn hot_header_and_generation_layouts_are_compact() {
        assert_eq!(std::mem::size_of::<Generation>(), 1);
        const { assert!(HOT_HEADER_BYTES <= CACHE_LINE_BYTES) };
    }
    #[test]
    fn immutable_metadata_starts_old() {
        let mut arena = HeapArena::new();
        let reference = arena.allocate_immutable(7u32);
        assert_eq!(arena.generation_counts(), (0, 1));
        assert_eq!(arena.get(reference), Some(&7));
    }
    #[test]
    fn nursery_allocations_are_contiguous_before_reuse() {
        let mut arena = HeapArena::new();
        let first = arena.allocate(1u8);
        let second = arena.allocate(2u8);
        assert_eq!(second.0, first.0 + 1);
        assert_eq!(arena.nursery_remaining(), 4094);
    }
    #[test]
    fn page_accounting_tracks_arena_commitment() {
        let mut arena = HeapArena::new();
        arena.allocate_sized([0u8; 128], 128);
        assert_eq!(arena.page_count(), 1);
        assert!(arena.stats().committed_bytes >= 4096);
    }
    #[test]
    fn size_classes_grow_monotonically() {
        let sizes = [1, 8, 16, 32, 64, 128, 1024];
        let classes: Vec<_> = sizes
            .into_iter()
            .map(HeapArena::<u8>::size_class_for)
            .collect();
        assert!(classes.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
