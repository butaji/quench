/// Conservative cache-line width used for portable layout decisions.
///
/// The measured target host (arm64 Apple Silicon) reports
/// `sysctl hw.cachelinesize = 128`; this runtime intentionally keeps a
/// 64-byte baseline until measurements justify architecture-specific layout.
/// No alignment or padding is implied by this constant.
pub const CACHE_LINE_BYTES: usize = 64;
/// Canonical allocator page quantum.
///
/// `HeapArena` accounts each slot's reserved capacity in these units for its
/// entire lifetime. Zero and non-power-of-two quanta are invalid and excluded
/// by the compile-time contract below; this does not add object padding.
pub const ARENA_PAGE_BYTES: usize = 4096;
/// Bytes reserved for the frequently accessed portion of an object header.
pub const HOT_HEADER_BYTES: usize = 16;
/// Per-slot allocator metadata stored alongside the value vector.
///
/// `values` is the sole owner/liveness source for heap slots (`Option<T>`).
/// `sizes`, `generations`, and `free` are parallel indexes only: they may
/// describe a slot but never keep its value alive. External backing stores
/// are deliberately absent from this layout; their producer owns them and
/// reports their bytes through `charge_external`/`release_external`.
///
/// The three side vectors carry only allocation size, generation, and reusable
/// slot indices. This is 13 bytes of logical metadata per slot on 64-bit hosts
/// (8 + 1 + 4), before independent `Vec` capacity/allocator rounding.
pub const SLOT_METADATA_BYTES: usize =
    std::mem::size_of::<usize>() + std::mem::size_of::<Generation>() + std::mem::size_of::<u32>();

/// External allocation bookkeeping has no per-object slot metadata.
///
/// The only canonical external state is `HeapStats::external_bytes`; the
/// backing allocation and its lifetime remain owned by the producer.
pub const EXTERNAL_METADATA_BYTES: usize = std::mem::size_of::<u64>();
/// Return the logical heap-wide bytes occupied by per-slot metadata.
///
/// This deliberately uses the number of slots, not vector capacities: allocator
/// rounding is accounted for by `reserved_bytes`, while GC metadata has one
/// canonical record per slot. `values` remains the sole ownership/liveness
/// source; these side indexes never add a second object header.
#[inline]
pub const fn slot_metadata_bytes(slot_count: usize) -> usize {
    slot_count.saturating_mul(SLOT_METADATA_BYTES)
}

// Keep the portable layout assumptions enforced by the compiler, rather than
// relying only on focused tests. These are representation invariants, not
// hardware-detection claims.
const _: () = {
    assert!(CACHE_LINE_BYTES.is_power_of_two());
    assert!(ARENA_PAGE_BYTES.is_power_of_two());
    assert!(ARENA_PAGE_BYTES >= CACHE_LINE_BYTES);
    assert!(HOT_HEADER_BYTES.is_power_of_two());
    assert!(HOT_HEADER_BYTES <= CACHE_LINE_BYTES);
    assert!(SLOT_METADATA_BYTES <= HOT_HEADER_BYTES);
};

use std::collections::HashSet;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::identity::HeapRef;

/// A zero-sized marker that makes an arena explicitly isolate-local.
///
/// `Rc` is only used in the type-level marker: its !Send/!Sync auto-traits
/// prevent moving an arena across threads, which is the source invariant that
/// allows collection bookkeeping and write barriers to remain non-atomic.
#[derive(Debug, Default)]
struct IsolateOwner(PhantomData<Rc<()>>);

// `HeapRef` is only an index; this marker is the ownership/lifecycle boundary.

/// The frequently sampled allocator counters. This is a view into `HeapStats`,
/// not an additional backing store: `HeapStats` remains the sole snapshot
/// representation and `HeapArena` remains the owner of all mutable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeapHotStats {
    pub reserved_bytes: u64,
    pub committed_bytes: u64,
    pub live_bytes: u64,
    pub external_bytes: u64,
    pub allocated_bytes: u64,
    pub live_objects: usize,
}

/// Slow-path diagnostics and lifecycle counters. Like [`HeapHotStats`], this
/// is an observational view with no independent ownership or invalid state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeapColdStats {
    pub remembered_writes: u64,
    pub collections: u64,
    pub size_classes: [u64; 8],
    pub promoted_objects: u64,
    pub nursery_reclaimed: u64,
}

/// Canonical aggregate allocator snapshot. Mutable ownership stays in
/// `HeapArena`; this value is copied at the public API boundary.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeapStats {
    pub reserved_bytes: u64,
    pub committed_bytes: u64,
    pub live_bytes: u64,
    pub external_bytes: u64,
    pub allocated_bytes: u64,
    pub live_objects: usize,
    pub remembered_writes: u64,
    pub collections: u64,
    pub size_classes: [u64; 8],
    pub promoted_objects: u64,
    pub nursery_reclaimed: u64,
}

impl HeapStats {
    /// Project the canonical snapshot into its hot arithmetic fields.
    #[inline]
    pub const fn hot(self) -> HeapHotStats {
        HeapHotStats {
            reserved_bytes: self.reserved_bytes,
            committed_bytes: self.committed_bytes,
            live_bytes: self.live_bytes,
            external_bytes: self.external_bytes,
            allocated_bytes: self.allocated_bytes,
            live_objects: self.live_objects,
        }
    }

    /// Project the canonical snapshot into its cold diagnostics.
    #[inline]
    pub const fn cold(self) -> HeapColdStats {
        HeapColdStats {
            remembered_writes: self.remembered_writes,
            collections: self.collections,
            size_classes: self.size_classes,
            promoted_objects: self.promoted_objects,
            nursery_reclaimed: self.nursery_reclaimed,
        }
    }
}
/// Producer of bytes charged outside the arena slot store.
///
/// This is provenance, not a second counter: `HeapStats::external_bytes`
/// remains the sole authoritative total. Producers must charge once when a
/// backing store is acquired and release the same amount when it is detached
/// or destroyed. The arena never owns or traverses that storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalMemorySource {
    /// Bytes belonging to an ArrayBuffer backing store.
    ArrayBuffer,
    /// Bytes belonging to a host/native allocation.
    Native,
}

/// `HeapStats` is a value snapshot, so this is a size contract rather than a
/// false-sharing padding request. The arena's `!Send` owner marker prevents
/// concurrent mutation of these counters across isolates.
pub const HEAP_STATS_BYTES: usize = 5 * std::mem::size_of::<u64>()
    + std::mem::size_of::<usize>()
    + 2 * std::mem::size_of::<u64>()
    + std::mem::size_of::<[u64; 8]>()
    + 2 * std::mem::size_of::<u64>();
/// Bytes occupied by the contiguous, frequently read prefix of `HeapStats`.
///
/// The prefix contains allocator pressure and liveness counters only.  Cold
/// histogram and lifecycle counters follow it, so a hot allocation path never
/// needs to fetch the latter merely to read the former.  This is a layout
/// invariant over the canonical snapshot, not a second stored header.
pub const HEAP_HOT_PREFIX_BYTES: usize = std::mem::size_of::<u64>() * 8;
const _: () = {
    assert!(HEAP_HOT_PREFIX_BYTES <= CACHE_LINE_BYTES);
    assert!(std::mem::offset_of!(HeapStats, reserved_bytes) == 0);
    // `usize` is four bytes on wasm32 and eight bytes on the native hosts.
    // `repr(C)` inserts any required padding before the following `u64`s, so
    // the live-object counter may end before (but never after) the two cold
    // counters that terminate the hot prefix.
    assert!(
        std::mem::offset_of!(HeapStats, live_objects) + std::mem::size_of::<usize>()
            <= HEAP_HOT_PREFIX_BYTES - std::mem::size_of::<u64>() * 2
    );
    assert!(std::mem::offset_of!(HeapStats, size_classes) >= HEAP_HOT_PREFIX_BYTES);
};

/// Canonical per-slot allocator metadata snapshot. It is observational only;
/// liveness remains owned by the arena's value vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct AllocationMetadata {
    pub bytes: usize,
    pub class: usize,
    pub generation: Generation,
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

/// Isolate-local, non-moving arena.
///
/// The arena intentionally is not `Send` or `Sync`.  This is a compile-time
/// ownership boundary, not a convention: collection metadata and counters are
/// ordinary fields and are only mutated by the isolate that owns the arena.
///
/// ```compile_fail
/// use quench_runtime::heap::HeapArena;
///
/// let arena = HeapArena::<u8>::new();
/// std::thread::spawn(move || drop(arena));
/// ```
///
/// `HeapArena` is the canonical owner of heap slots and GC metadata. Its
/// `IsolateOwner` marker deliberately makes this type `!Send` and `!Sync`;
/// therefore no cross-thread mutation or observation can exist without an
/// explicit isolate boundary. This is the reason remembered-set writes,
/// generation updates, and collection counters use ordinary mutable fields
/// rather than atomic operations.
///
/// `HeapArena` currently has no compaction or relocation API: collection marks
/// unreachable slots empty and later reuses those slot indices, while every
/// live `T` stays at its original allocation address. Consequently native
/// objects and `ArrayBufferData` must not be modeled as pointers into a
/// relocatable arena. Their backing storage remains owned by their native
/// container, and its lifetime is independent of slot reuse.
///
/// External bytes are accounting-only (`charge_external` /
/// `release_external`); they are not traversed, moved, or reclaimed by this
/// arena. Any future moving collector must introduce an explicit pin/handle
/// contract before relocating objects or their external owners.
#[repr(C, align(4096))]
#[derive(Debug)]
pub struct HeapArena<T> {
    #[allow(dead_code)]
    owner: IsolateOwner,
    values: Vec<Option<T>>,
    sizes: Vec<usize>,
    generations: Vec<Generation>,
    pinned: HashSet<HeapRef>,
    free: Vec<u32>,
    stats: HeapStats,
    nursery_limit: usize,
    /// Old-generation owners and nursery targets recorded by the write barrier.
    remembered: Vec<(HeapRef, HeapRef)>,
    page_size: usize,
    gc_threshold: u64,
    bytes_since_gc: u64,
}
// Keep the representation and accounting contracts coupled: changing the
// canonical page quantum without changing the type-level alignment must fail
// at compile time rather than silently degrading page locality.
const _: () = {
    assert!(std::mem::align_of::<HeapArena<()>>() == ARENA_PAGE_BYTES);
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NurseryOverflow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationFailure {
    NurseryOverflow,
    Capacity,
}

impl AllocationFailure {
    /// Construct the failure returned when the nursery has no append capacity.
    ///
    /// Keep this constructor out of the straight-line allocator: overflow is a
    /// cold diagnostic path, not a normal allocation result.
    #[cold]
    #[inline(never)]
    fn nursery_overflow() -> Self {
        Self::NurseryOverflow
    }

    /// Construct the failure returned for an exhausted slot identity or a
    /// host allocator refusal. Both cases are unreachable on healthy bounded
    /// workloads and remain isolated from the hot allocation path.
    #[cold]
    #[inline(never)]
    fn capacity() -> Self {
        Self::Capacity
    }
}

/// Panic only after a fallible allocation has reached its explicitly cold
/// failure boundary. Keeping formatting and unwinding here prevents the
/// infallible convenience API from importing error construction into its hot
/// instruction path.
#[cold]
#[inline(never)]
fn panic_allocation_failure(operation: &'static str, failure: AllocationFailure) -> ! {
    panic!("{operation} failed: {failure:?}")
}

impl<T> Default for HeapArena<T> {
    fn default() -> Self {
        Self {
            owner: IsolateOwner::default(),
            values: Vec::new(),
            sizes: Vec::new(),
            generations: Vec::new(),
            pinned: HashSet::new(),
            free: Vec::new(),
            stats: HeapStats::default(),
            nursery_limit: ARENA_PAGE_BYTES,
            remembered: Vec::new(),
            page_size: ARENA_PAGE_BYTES,
            gc_threshold: ARENA_PAGE_BYTES as u64,
            bytes_since_gc: 0,
        }
    }
}
impl<T> HeapArena<T> {
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the allocation-pressure threshold that requests a collection.
    ///
    /// The threshold is owned by the arena for its lifetime. Zero is invalid
    /// (it would make every state collect before any useful work), so the
    /// builder clamps it to one byte. Pressure is compared inclusively:
    /// reaching the threshold is sufficient to request collection.
    pub fn with_gc_threshold(mut self, bytes: u64) -> Self {
        self.gc_threshold = bytes.max(1);
        self
    }

    /// Return the canonical threshold used by [`Self::should_collect`].
    #[inline]
    pub const fn gc_threshold(&self) -> u64 {
        self.gc_threshold
    }

    pub fn should_collect(&self) -> bool {
        self.bytes_since_gc >= self.gc_threshold
    }

    /// Bytes charged to the current allocation cohort since the last
    /// collection. Arena objects and externally owned backing stores share
    /// this one pressure interval; releasing an object does not erase the
    /// cohort's allocation history.
    #[inline]
    pub fn allocation_bytes_since_collection(&self) -> u64 {
        self.bytes_since_gc
    }

    /// Return the cumulative bytes charged by successful allocations.
    ///
    /// This is the canonical request-visible allocation total: it only
    /// increases when an allocation is committed, is independent of live
    /// object reclamation, and is never reset by collection. The per-interval
    /// pressure value is exposed separately by
    /// [`Self::allocation_bytes_since_collection`].
    #[inline]
    pub const fn allocation_bytes_total(&self) -> u64 {
        self.stats.allocated_bytes
    }
    pub fn gc_threshold_remaining(&self) -> u64 {
        self.gc_threshold.saturating_sub(self.bytes_since_gc)
    }

    /// Add allocation pressure without changing ownership counters.
    #[inline]
    fn record_pressure(&mut self, bytes: usize) {
        self.bytes_since_gc = self.bytes_since_gc.saturating_add(bytes as u64);
    }

    /// Charge bytes from a named external producer.
    ///
    /// `source` documents ownership at the boundary; totals intentionally
    /// remain unified so GC pressure cannot omit one producer.
    #[inline]
    pub fn charge_external_from(&mut self, _source: ExternalMemorySource, bytes: usize) {
        self.stats.external_bytes = self.stats.external_bytes.saturating_add(bytes as u64);
        self.record_pressure(bytes);
    }

    /// Release bytes previously charged by the named producer.
    #[inline]
    pub fn release_external_from(&mut self, _source: ExternalMemorySource, bytes: usize) {
        self.stats.external_bytes = self.stats.external_bytes.saturating_sub(bytes as u64);
    }

    #[inline]
    pub fn charge_external(&mut self, bytes: usize) {
        self.charge_external_from(ExternalMemorySource::Native, bytes);
    }

    #[inline]
    pub fn release_external(&mut self, bytes: usize) {
        self.release_external_from(ExternalMemorySource::Native, bytes);
    }

    /// Sweep every slot that is not reachable from the canonical root source.
    ///
    /// The source contract is intentionally concrete: `RootRegistry` owns the
    /// externally held roots, while `remembered` contributes only old-owner →
    /// nursery edges whose owner is itself reachable. `values` is the sole
    /// ownership/liveness store; all other vectors are derived metadata.
    /// References outside the current slot range, duplicate roots, and
    /// remembered edges from dead owners are invalid and have no effect.
    pub fn collect_unrooted(&mut self, roots: &RootRegistry) -> usize {
        let mut keep: std::collections::HashSet<_> =
            std::collections::HashSet::with_capacity(roots.root_count());
        keep.extend(roots.all_roots());
        // A remembered edge is authoritative only while its owner is a live
        // old-generation slot.  Stale roots/reused references must never
        // make a nursery object reachable.
        for &(owner, target) in &self.remembered {
            let owner_live = self
                .values
                .get(owner.0 as usize)
                .is_some_and(Option::is_some)
                && self.generations.get(owner.0 as usize) == Some(&Generation::Old);
            if owner_live && keep.contains(&owner) {
                keep.insert(target);
            }
        }
        let mut reclaimed = 0;
        for index in 0..self.values.len() {
            if self.values[index].is_some()
                && !keep.contains(&HeapRef(index as u32))
                && !self.pinned.contains(&HeapRef(index as u32))
            {
                let size = self.sizes[index];
                let class_index = Self::size_class_index(size);
                self.stats.size_classes[class_index] =
                    self.stats.size_classes[class_index].saturating_sub(1);
                if self.generations[index].is_nursery() {
                    self.stats.nursery_reclaimed = self.stats.nursery_reclaimed.saturating_add(1);
                }
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
        // Slots at the end of the arena cannot be reused by a later allocation
        // once they are reclaimed. Drop that dead suffix at collection
        // boundaries so bursty workloads do not retain their peak slot count.
        self.trim_trailing_slots();
        self.remembered.retain(|&(owner, target)| {
            self.values
                .get(owner.0 as usize)
                .is_some_and(Option::is_some)
                && self.generations.get(owner.0 as usize) == Some(&Generation::Old)
                && self
                    .values
                    .get(target.0 as usize)
                    .is_some_and(Option::is_some)
                && self.generations.get(target.0 as usize) == Some(&Generation::Nursery)
        });
        self.remembered.shrink_to_fit();
        self.stats.live_objects = self.live_len();
        self.stats.collections = self.stats.collections.saturating_add(1);
        self.bytes_since_gc = 0;

        self.recompute_committed();
        debug_assert!(self.bump_reuse_invariant());
        reclaimed
    }

    fn trim_trailing_slots(&mut self) {
        let new_len = self
            .values
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |index| index + 1);
        // Preserve stable slot numbers while any object remains live. A full
        // emptying is the lifecycle boundary where all old references are
        // dead, so dropping the backing vectors is unambiguously safe.
        if new_len != 0 {
            return;
        }
        self.values.clear();
        self.sizes.clear();
        self.generations.clear();
        self.free.clear();
        self.values.shrink_to_fit();
        self.sizes.shrink_to_fit();
        self.generations.shrink_to_fit();
        self.free.shrink_to_fit();
    }
    pub fn collect_if_needed(&mut self, roots: &RootRegistry) -> Option<usize> {
        self.should_collect().then(|| self.collect_unrooted(roots))
    }

    /// Configure the nursery slot budget. Allocation has one canonical bump
    /// frontier (`values.len()`): reclaimed slots are reused before the
    /// frontier grows, so overflow requires a full frontier and no free slot.
    pub fn with_nursery_limit(mut self, limit: usize) -> Self {
        self.nursery_limit = limit;
        self
    }
    /// Remaining append slots before nursery allocation reports overflow.
    pub fn nursery_remaining(&self) -> usize {
        self.nursery_limit.saturating_sub(self.values.len())
    }

    pub fn nursery_is_full(&self) -> bool {
        debug_assert!(self.bump_reuse_invariant());
        self.free.is_empty() && self.values.len() >= self.nursery_limit
    }
    pub fn allocate(&mut self, value: T) -> HeapRef {
        self.try_allocate(value)
            .unwrap_or_else(|failure| panic_allocation_failure("heap nursery allocation", failure))
    }
    pub fn try_allocate(&mut self, value: T) -> Result<HeapRef, AllocationFailure> {
        self.try_allocate_sized(value, std::mem::size_of::<T>().max(1))
    }
    pub fn try_allocate_sized(
        &mut self,
        value: T,
        bytes: usize,
    ) -> Result<HeapRef, AllocationFailure> {
        // Perform every fallible capacity check before touching logical state.
        // `allocate_sized` is then infallible: all three parallel vectors have
        // capacity for the append, or a reclaimed slot is already available.
        if self.nursery_is_full() {
            return Err(AllocationFailure::nursery_overflow());
        }
        // HeapRef is a u32 slot identity; never silently wrap the bump index.
        if self.free.is_empty() && self.values.len() >= u32::MAX as usize {
            return Err(AllocationFailure::capacity());
        }
        if self.free.is_empty() {
            if self.values.try_reserve(1).is_err()
                || self.sizes.try_reserve(1).is_err()
                || self.generations.try_reserve(1).is_err()
            {
                return Err(AllocationFailure::capacity());
            }
        }
        Ok(self.allocate_sized(value, bytes))
    }

    pub fn allocate_immutable(&mut self, value: T) -> HeapRef {
        self.try_allocate_immutable(value)
            .unwrap_or_else(|failure| {
                panic_allocation_failure("immutable metadata allocation", failure)
            })
    }

    /// Fallible counterpart used by callers that must turn OOM into isolate
    /// termination/reset rather than panic while preserving generation state.
    pub fn try_allocate_immutable(&mut self, value: T) -> Result<HeapRef, AllocationFailure> {
        let reference = self.try_allocate_sized(value, std::mem::size_of::<T>().max(1))?;
        if let Some(generation) = self.generations.get_mut(reference.0 as usize) {
            *generation = Generation::Old;
        }
        Ok(reference)
    }
    pub fn allocate_sized(&mut self, value: T, bytes: usize) -> HeapRef {
        let class_index = Self::size_class_index(bytes);
        self.stats.allocated_bytes = self.stats.allocated_bytes.saturating_add(bytes as u64);
        self.stats.live_bytes = self.stats.live_bytes.saturating_add(bytes as u64);
        self.stats.size_classes[class_index] =
            self.stats.size_classes[class_index].saturating_add(1);
        self.stats.live_objects = self.live_len().saturating_add(1);
        self.record_pressure(bytes);
        if let Some(index) = self.free.pop() {
            let reused = HeapRef(index);
            self.remembered
                .retain(|&(owner, target)| owner != reused && target != reused);
            self.values[index as usize] = Some(value);
            self.sizes[index as usize] = bytes;
            self.generations[index as usize] = Generation::Nursery;
            self.pinned.remove(&reused);
            self.recompute_committed();
            debug_assert!(self.bump_reuse_invariant());
            return reused;
        }
        self.values.push(Some(value));
        self.sizes.push(bytes);
        self.generations.push(Generation::Nursery);
        let index = (self.values.len() - 1) as u32;
        self.recompute_committed();
        debug_assert!(self.bump_reuse_invariant());
        HeapRef(index)
    }
    /// Validate the single-source slot invariant in debug builds:
    /// `values.len()` is the bump frontier, all side vectors have exactly the
    /// same frontier, and every reusable identity names one empty slot.
    ///
    /// The free list is deliberately not a second allocation frontier.  It is
    /// only a reuse queue; an append may occur exclusively when it is empty.
    #[inline]
    fn bump_reuse_invariant(&self) -> bool {
        self.values.len() == self.sizes.len()
            && self.values.len() == self.generations.len()
            && self.free.iter().enumerate().all(|(position, &index)| {
                (index as usize) < self.values.len()
                    && self.values[index as usize].is_none()
                    && !self.free[..position].contains(&index)
            })
    }

    fn size_class_index(bytes: usize) -> usize {
        let class = Self::size_class_bytes(bytes);
        (class.trailing_zeros() as usize).saturating_sub(3).min(7)
    }

    /// Return the slot capacity charged to an allocation of `bytes`.
    ///
    /// Classes are 8, 16, 32, 64, 128, 256, 512, and 1024 bytes;
    /// allocations larger than 1024 bytes use the final catch-all class,
    /// whose slot capacity is one arena page. Zero-byte values still occupy
    /// the smallest slot. This is the same canonical mapping used by both
    /// live counters and reserved-page accounting.
    pub fn size_class_capacity(bytes: usize) -> usize {
        Self::size_class_bytes(bytes)
    }

    /// Bytes reserved for reclaimed slots (internal fragmentation plus holes).
    pub fn fragmentation_bytes(&self) -> u64 {
        self.stats
            .reserved_bytes
            .saturating_sub(self.stats.committed_bytes)
    }

    /// Return the canonical power-of-two class capacity for an allocation.
    ///
    /// The largest class is a catch-all for objects larger than 1 KiB.  The
    /// checked operation is intentional: public callers may pass `usize::MAX`
    /// and class selection must remain total rather than overflow.
    fn size_class_bytes(bytes: usize) -> usize {
        bytes
            .max(1)
            .checked_next_power_of_two()
            .unwrap_or(usize::MAX)
            .min(ARENA_PAGE_BYTES)
    }

    pub fn size_class_for(bytes: usize) -> usize {
        Self::size_class_index(bytes)
    }

    fn recompute_committed(&mut self) {
        let page_size = self.page_size.max(1) as u64;
        let mut committed = 0u64;
        let mut reserved = 0u64;
        for (value, &size) in self.values.iter().zip(&self.sizes) {
            let class = Self::size_class_bytes(size).min(self.page_size.max(1)) as u64;
            let capacity = class.div_ceil(page_size) * page_size;
            reserved = reserved.saturating_add(capacity);
            if value.is_some() {
                committed = committed.saturating_add(capacity);
            }
        }
        self.stats.committed_bytes = committed;
        self.stats.reserved_bytes = reserved;
    }
    pub fn page_count(&self) -> usize {
        self.stats
            .reserved_bytes
            .div_ceil(self.page_size.max(1) as u64) as usize
    }

    /// Pin a live slot so a future moving collector must not relocate it.
    pub fn pin(&mut self, reference: HeapRef) -> bool {
        if self
            .values
            .get(reference.0 as usize)
            .is_some_and(Option::is_some)
        {
            self.pinned.insert(reference);
            true
        } else {
            false
        }
    }

    pub fn unpin(&mut self, reference: HeapRef) -> bool {
        self.pinned.remove(&reference)
    }

    #[inline]
    pub fn is_pinned(&self, reference: HeapRef) -> bool {
        self.pinned.contains(&reference)
    }
    /// Validate the page/slab accounting contract from canonical slot metadata.
    ///
    /// Every slot reserves one page-rounded size-class capacity, while only
    /// occupied slots contribute committed bytes. `page_count` is derived from
    /// that same reserved total; it is never an independent counter.
    pub fn page_accounting_consistent(&self) -> bool {
        let page = self.page_size.max(1) as u64;
        self.stats.reserved_bytes % page == 0
            && self.stats.committed_bytes <= self.stats.reserved_bytes
            && self.page_count() == (self.stats.reserved_bytes / page) as usize
            && self.counters_consistent()
    }

    pub fn stats(&self) -> HeapStats {
        self.stats
    }
    pub fn accounted_bytes(&self) -> u64 {
        self.stats
            .committed_bytes
            .saturating_add(self.stats.external_bytes)
    }
    /// Validate that each public counter has one authoritative source.
    ///
    /// Reserved/committed bytes come from slot metadata, live bytes and
    /// objects from occupied values, and external bytes remain independent of
    /// arena slots. `allocated_bytes` is the cumulative allocation counter.
    pub fn counters_consistent(&self) -> bool {
        let mut reserved = 0u64;
        let mut committed = 0u64;
        let mut live = 0u64;
        let mut objects = 0usize;
        let mut classes = [0u64; 8];
        for (value, &size) in self.values.iter().zip(&self.sizes) {
            let page = self.page_size.max(1);
            let capacity =
                Self::size_class_bytes(size).min(page).div_ceil(page) as u64 * page as u64;
            reserved = reserved.saturating_add(capacity);
            if value.is_some() {
                committed = committed.saturating_add(capacity);
                live = live.saturating_add(size as u64);
                objects = objects.saturating_add(1);
                let class = Self::size_class_index(size);
                classes[class] = classes[class].saturating_add(1);
            }
        }
        self.stats.reserved_bytes == reserved
            && self.stats.committed_bytes == committed
            && self.stats.live_bytes == live
            && self.stats.live_objects == objects
            && self.stats.size_classes == classes
    }

    /// Record a live old-to-young mutation without atomics (arenas are isolate-owned).
    ///
    /// The slot, generation, and value checks are one invariant: a stale
    /// `HeapRef` may retain its old generation after reclamation, but it is not
    /// a valid barrier edge. Duplicate edges are intentionally coalesced so
    /// repeated stores do not inflate remembered-set accounting. The return
    /// value reports whether this call added a new edge.
    pub fn record_write(&mut self, owner: HeapRef, target: HeapRef) -> bool {
        let owner_index = owner.0 as usize;
        let target_index = target.0 as usize;
        let owner_live = self.values.get(owner_index).is_some_and(Option::is_some);
        let target_live = self.values.get(target_index).is_some_and(Option::is_some);
        let owner_old = self.generations.get(owner_index) == Some(&Generation::Old);
        let target_nursery = self.generations.get(target_index) == Some(&Generation::Nursery);
        if !(owner_live && target_live && owner_old && target_nursery) {
            return false;
        }
        if self.remembered.contains(&(owner, target)) {
            return false;
        }
        self.remembered.push((owner, target));
        self.stats.remembered_writes = self.stats.remembered_writes.saturating_add(1);
        true
    }

    pub fn remembered_len(&self) -> usize {
        self.remembered
            .iter()
            .map(|(owner, _)| owner)
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    pub fn remembered(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.remembered.iter().map(|(owner, _)| *owner)
    }
    /// Iterate the canonical old-owner → nursery-target edges.
    ///
    /// The pair is the remembered-set record; [`Self::remembered`] is only the
    /// owner projection used by diagnostics.  Callers must not retain these
    /// references across collection because reclamation and slot reuse prune
    /// stale edges.
    pub fn remembered_edges(&self) -> impl Iterator<Item = (HeapRef, HeapRef)> + '_ {
        self.remembered.iter().copied()
    }

    /// Return metadata only for a live slot. Reclaimed and forged references
    /// are invalid states and therefore return `None`.
    ///
    /// The value vector is checked first and remains the sole ownership
    /// source; the parallel metadata vectors are only consulted after that
    /// proof and are required to contain the same slot. This keeps malformed
    /// metadata from becoming a panic or an alternate liveness model.
    #[inline]
    fn metadata_for_index(&self, index: usize) -> Option<AllocationMetadata> {
        self.values.get(index)?.as_ref()?;
        let bytes = *self.sizes.get(index)?;
        let generation = *self.generations.get(index)?;
        Some(AllocationMetadata {
            bytes,
            class: Self::size_class_index(bytes),
            generation,
        })
    }

    #[inline]
    pub fn allocation_metadata(&self, reference: HeapRef) -> Option<AllocationMetadata> {
        self.metadata_for_index(reference.0 as usize)
    }

    /// Resolve a live slot from the canonical value vector. Keeping the
    /// bounds check and occupancy check together prevents callers from
    /// building dependent chains through parallel metadata.
    #[inline]
    pub fn get(&self, reference: HeapRef) -> Option<&T> {
        let slot = self.values.get(reference.0 as usize)?;
        slot.as_ref()
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

    /// Release the value owned by `reference` and return it to the arena.
    ///
    /// `values` is the sole ownership source: a successful free takes the
    /// value from that vector exactly once, updates all derived accounting,
    /// and queues the slot for reuse. A stale or out-of-range identity is an
    /// invalid free and has no effect. The slot metadata remains in place
    /// until reuse (or an empty-arena trim), preserving stable identities.
    pub fn free(&mut self, reference: HeapRef) -> Option<T> {
        let index = reference.0 as usize;
        let value = self.values.get_mut(index)?.take()?;
        let size = self.sizes[index];
        let class_index = Self::size_class_index(size);
        self.stats.size_classes[class_index] =
            self.stats.size_classes[class_index].saturating_sub(1);
        if self.generations[index].is_nursery() {
            self.stats.nursery_reclaimed = self.stats.nursery_reclaimed.saturating_add(1);
        }
        self.stats.live_bytes = self.stats.live_bytes.saturating_sub(size as u64);
        self.free.push(reference.0);
        debug_assert!(self.bump_reuse_invariant());
        self.remembered.retain(|&(owner, target)| {
            self.values
                .get(owner.0 as usize)
                .is_some_and(Option::is_some)
                && self
                    .values
                    .get(target.0 as usize)
                    .is_some_and(Option::is_some)
        });
        self.stats.live_objects = self.live_len();
        self.recompute_committed();
        Some(value)
    }

    /// Compatibility spelling for collection code; ownership still flows
    /// through [`Self::free`].
    pub fn reclaim(&mut self, reference: HeapRef) -> Option<T> {
        self.free(reference)
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
        self.sets.push(RootSet::new(domain));
        let index = self.sets.len() - 1;
        &mut self.sets[index]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AllocationFailure, AllocationMetadata, ExternalMemorySource, Generation, HeapArena,
        HeapStats, LifetimeDomain, RootRegistry, ARENA_PAGE_BYTES, CACHE_LINE_BYTES,
        EXTERNAL_METADATA_BYTES, HEAP_HOT_PREFIX_BYTES, HEAP_STATS_BYTES, HOT_HEADER_BYTES,
        SLOT_METADATA_BYTES,
    };
    use crate::identity::HeapRef;
    use std::{
        cell::Cell,
        mem::{align_of, size_of},
        rc::Rc,
    };

    struct DropProbe(Rc<Cell<usize>>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn isolate_owner_is_zero_sized_and_non_thread_shareable_by_construction() {
        // The owner marker carries no runtime state: ownership is encoded only
        // in the type. Rc makes the marker (and therefore HeapArena) !Send and
        // !Sync, so these checks cannot be weakened to a runtime convention.
        assert_eq!(size_of::<super::IsolateOwner>(), 0);
    }
    #[test]
    fn isolate_local_allocator_keeps_lifecycle_and_counters_in_one_owner() {
        let drops = Rc::new(Cell::new(0));
        let mut arena = HeapArena::new();
        let reference = arena.allocate(DropProbe(drops.clone()));

        // Allocation, reclamation, and accounting all happen through the
        // unique mutable arena owner; no shared/atomic side counter exists.
        assert_eq!(arena.stats().live_objects, 1);
        assert_eq!(arena.get(reference).is_some(), true);
        assert_eq!(arena.reclaim(reference).is_some(), true);
        assert_eq!(drops.get(), 1);
        assert_eq!(arena.stats().live_objects, 0);
        assert!(arena.get(reference).is_none());
    }
    #[test]
    fn stats_are_snapshots_of_the_isolate_owner() {
        let mut arena = HeapArena::new();
        let mut snapshot = arena.stats();
        let reference = arena.allocate_sized(String::from("owned"), 5);

        // Reading a snapshot never creates a second mutable counter owner.
        // The only state transition is through the unique arena borrow.
        snapshot.live_objects = 99;
        snapshot.live_bytes = 99;
        assert_eq!((snapshot.live_objects, snapshot.live_bytes), (99, 99));
        assert_eq!(arena.stats().live_objects, 1);
        assert_eq!(arena.stats().live_bytes, 5);

        assert_eq!(arena.free(reference).as_deref(), Some("owned"));
        assert_eq!(arena.stats().live_objects, 0);
        assert_eq!(arena.stats().live_bytes, 0);
    }

    #[test]
    fn slot_metadata_has_no_hidden_gc_header() {
        assert_eq!(size_of::<Generation>(), 1);
        assert_eq!(
            SLOT_METADATA_BYTES,
            size_of::<usize>() + 1 + size_of::<u32>()
        );
        // On 64-bit hosts the three side vectors account for 13 logical bytes
        // per slot; values/Option<T> storage is intentionally excluded.
        assert_eq!(SLOT_METADATA_BYTES, 13);
    }

    #[test]
    fn cache_layout_matches_verified_arm64_baseline() {
        // Apple Silicon reports 128-byte hardware lines, but the runtime's
        // portable layout baseline is deliberately one 64-byte half-line.
        assert_eq!(CACHE_LINE_BYTES, 64);
        assert_eq!(HOT_HEADER_BYTES, 16);
        assert!(HOT_HEADER_BYTES.is_power_of_two());
        assert!(CACHE_LINE_BYTES.is_power_of_two());
        assert_eq!(size_of::<usize>(), 8);
        assert_eq!(size_of::<u32>(), 4);
        assert_eq!(size_of::<u64>(), 8);
    }

    #[test]
    fn slot_metadata_accounting_saturates_at_addressable_limit() {
        assert_eq!(super::slot_metadata_bytes(usize::MAX), usize::MAX);
        assert_eq!(super::slot_metadata_bytes(0), 0);
    }

    #[test]
    fn heap_wide_slot_metadata_scales_without_per_object_header() {
        let mut arena = HeapArena::new();
        for size in 0..256 {
            arena.allocate_sized(size as u8, size);
        }

        let slots = arena.values.len();
        assert_eq!(slots, 256);
        assert_eq!(super::slot_metadata_bytes(slots), slots * 13);
        assert_eq!(arena.values.len(), arena.sizes.len());
        assert_eq!(arena.values.len(), arena.generations.len());
        // Reclaiming values changes ownership/liveness, not metadata shape.
        let roots = RootRegistry::new();
        assert_eq!(arena.collect_unrooted(&roots), 256);
        assert_eq!(arena.values.len(), arena.sizes.len());
        assert_eq!(arena.values.len(), arena.generations.len());
        assert_eq!(super::slot_metadata_bytes(arena.values.len()), 0);
    }
    #[test]
    fn gc_slot_source_invariant_survives_collect_and_reuse() {
        let mut arena = HeapArena::new();
        let first = arena.allocate_sized("first", 24);
        let second = arena.allocate_sized("second", 48);
        let roots = RootRegistry::new();

        assert_eq!(arena.collect_unrooted(&roots), 2);
        assert!(arena.values.is_empty());
        assert!(arena.sizes.is_empty());
        assert!(arena.generations.is_empty());
        assert!(arena.free.is_empty());
        assert!(arena.allocation_metadata(first).is_none());
        assert!(arena.allocation_metadata(second).is_none());
        assert!(arena.bump_reuse_invariant());

        let reused = arena.allocate_sized("reused", 96);
        assert_eq!(reused, HeapRef(0));
        assert_eq!(arena.values.len(), 1);
        assert_eq!(arena.sizes.len(), 1);
        assert_eq!(arena.generations.len(), 1);
        assert_eq!(
            arena.allocation_metadata(reused),
            Some(AllocationMetadata {
                bytes: 96,
                class: 4,
                generation: Generation::Nursery,
            })
        );
        assert!(arena.bump_reuse_invariant());
    }

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
    fn heap_arena_is_sole_owner_until_reclaim_or_collection() {
        let drops = Rc::new(Cell::new(0));
        let mut arena = HeapArena::new();
        let reference = arena.allocate(DropProbe(drops.clone()));
        assert_eq!(drops.get(), 0);
        assert!(arena.get(reference).is_some());
        assert_eq!(arena.reclaim(reference).map(|_| ()), Some(()));
        assert_eq!(drops.get(), 1);
        assert!(arena.get(reference).is_none());
    }

    #[test]
    fn free_is_single_source_release_and_stale_free_is_inert() {
        let mut arena = HeapArena::new();
        let reference = arena.allocate_sized(String::from("owned"), 17);
        let before = arena.stats();

        assert_eq!(arena.free(reference).as_deref(), Some("owned"));
        assert_eq!(arena.stats().live_objects, 0);
        assert_eq!(arena.stats().live_bytes, 0);
        assert_eq!(arena.stats().allocated_bytes, before.allocated_bytes);
        assert_eq!(arena.free(reference), None);
        assert_eq!(arena.stats().live_objects, 0);
        assert!(arena.bump_reuse_invariant());

        let reused = arena.allocate_sized(String::from("replacement"), 9);
        assert_eq!(reused, reference);
        assert_eq!(arena.get(reused).map(String::as_str), Some("replacement"));
    }

    #[test]
    fn direct_reclaim_observes_generation_and_invalidates_ownership() {
        let mut arena = HeapArena::new();
        let reference = arena.allocate_sized(7u8, 3);

        assert_eq!(arena.generation_counts(), (1, 0));
        assert_eq!(arena.reclaim(reference), Some(7));
        assert_eq!(arena.stats().nursery_reclaimed, 1);
        assert_eq!(arena.live_len(), 0);
        assert!(arena.get(reference).is_none());
        // A stale identity is invalid until allocation explicitly reuses it.
        assert_eq!(arena.reclaim(reference), None);
        assert!(arena.counters_consistent());
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
        let mut arena = HeapArena::new().with_nursery_limit(3);
        let owner = arena.try_allocate(1u8).expect("owner nursery slot");
        let _discarded = arena.try_allocate(2u8).expect("discarded nursery slot");
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, owner);
        arena.collect_unrooted(&roots);
        let target = arena.try_allocate(3u8).expect("target nursery slot");
        let _filler = arena.try_allocate(4u8).expect("nursery limit filler");
        assert_eq!(arena.generation_counts(), (2, 1));
        assert_eq!(arena.nursery_remaining(), 0);
        assert!(arena.try_allocate(3u8).is_err());
        arena.charge_external(99);
        assert_eq!(arena.accounted_bytes(), arena.stats().committed_bytes + 99);
        arena.record_write(owner, target);
        assert_eq!(arena.stats().remembered_writes, 1);
        assert_eq!(arena.remembered().collect::<Vec<_>>(), vec![owner]);
        assert_eq!(arena.stats().allocated_bytes, 4);
    }
    #[test]
    fn nursery_bump_frontier_is_single_source_and_overflow_is_transactional() {
        let mut arena = HeapArena::new().with_nursery_limit(2);
        let first = arena
            .try_allocate_sized("first", 7)
            .expect("first nursery allocation");
        let second = arena
            .try_allocate_sized("second", 11)
            .expect("second nursery allocation");
        assert_eq!(first, HeapRef(0));
        assert_eq!(second, HeapRef(1));
        assert_eq!(arena.nursery_remaining(), 0);
        assert!(arena.bump_reuse_invariant());

        let before_stats = arena.stats();
        let before_slots = arena.values.len();
        let failure = arena
            .try_allocate_sized("overflow", 13)
            .expect_err("a full nursery must reject append allocation");
        assert_eq!(failure, AllocationFailure::NurseryOverflow);
        assert_eq!(arena.values.len(), before_slots);
        assert_eq!(arena.stats(), before_stats);
        assert_eq!(arena.get(first), Some(&"first"));
        assert_eq!(arena.get(second), Some(&"second"));
        assert!(arena.bump_reuse_invariant());
    }

    #[test]
    fn nursery_reuses_holes_without_moving_the_bump_frontier() {
        let mut arena = HeapArena::new().with_nursery_limit(2);
        let first = arena.allocate("first");
        let second = arena.allocate("second");
        assert_eq!(arena.values.len(), 2);
        assert_eq!(arena.reclaim(first), Some("first"));
        assert_eq!(arena.values.len(), 2);

        let reused = arena.allocate("reused");
        assert_eq!(reused, first);
        assert_eq!(arena.values.len(), 2);
        assert_eq!(arena.get(reused), Some(&"reused"));
        assert_eq!(arena.get(second), Some(&"second"));
        assert!(arena.bump_reuse_invariant());
    }
    #[test]
    fn remembered_old_owner_keeps_nursery_target_alive() {
        let mut arena = HeapArena::new();
        let owner = arena.allocate(1u8);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, owner);
        arena.collect_unrooted(&roots);
        let target = arena.allocate(2u8);
        arena.record_write(owner, target);
        assert_eq!(arena.collect_unrooted(&roots), 0);
        assert_eq!(arena.get(target), Some(&2));
    }
    #[test]
    fn allocation_counters_track_cumulative_live_and_gc_pressure() {
        let mut arena = HeapArena::new().with_gc_threshold(10);
        assert_eq!(arena.allocation_bytes_total(), 0);
        assert_eq!(arena.stats().live_bytes, 0);
        assert_eq!(arena.allocation_bytes_since_collection(), 0);

        let first = arena.allocate_sized(1u8, 4);
        assert_eq!(arena.allocation_bytes_total(), 4);
        assert_eq!(arena.stats().live_bytes, 4);
        assert_eq!(arena.allocation_bytes_since_collection(), 4);

        let second = arena.allocate_sized(2u8, 7);
        assert_eq!(arena.allocation_bytes_total(), 11);
        assert_eq!(arena.stats().live_bytes, 11);
        assert_eq!(arena.allocation_bytes_since_collection(), 11);

        assert_eq!(arena.reclaim(first), Some(1));
        assert_eq!(arena.allocation_bytes_total(), 11);
        assert_eq!(arena.stats().live_bytes, 7);
        assert_eq!(arena.gc_threshold_remaining(), 0);

        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Request, second);
        assert_eq!(arena.collect_unrooted(&roots), 0);
        assert_eq!(arena.allocation_bytes_total(), 11);
        assert_eq!(arena.allocation_bytes_since_collection(), 0);
        assert!(!arena.should_collect());
    }
    #[test]
    fn request_cohort_bytes_are_independent_of_root_lifetime() {
        let mut arena = HeapArena::new().with_gc_threshold(1_000);
        let realm_value = arena.allocate_sized((), 8);
        let request_value = arena.allocate_sized((), 24);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, realm_value);
        roots.add(LifetimeDomain::Request, request_value);

        assert_eq!(arena.stats().allocated_bytes, 32);
        assert_eq!(arena.stats().live_bytes, 32);
        assert_eq!(arena.allocation_bytes_since_collection(), 32);
        assert_eq!(arena.collect_unrooted(&roots), 0);
        assert_eq!(arena.stats().live_bytes, 32);

        roots.clear(LifetimeDomain::Request);
        assert_eq!(arena.collect_unrooted(&roots), 1);
        assert_eq!(arena.stats().live_bytes, 8);
        assert_eq!(arena.stats().allocated_bytes, 32);
        assert_eq!(arena.allocation_bytes_since_collection(), 0);
        assert!(arena.counters_consistent());
    }
    #[test]
    fn collector_uses_only_live_registry_roots_and_valid_remembered_edges() {
        let mut arena = HeapArena::new();
        let kept = arena.allocate(1u8);
        let dropped = arena.allocate(2u8);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, kept);
        roots.add(LifetimeDomain::Request, kept);
        roots.add(LifetimeDomain::Temporary, HeapRef(99_999));

        assert_eq!(arena.collect_unrooted(&roots), 1);
        assert_eq!(arena.get(kept), Some(&1));
        assert_eq!(arena.get(dropped), None);
        assert_eq!(arena.stats().live_objects, 1);

        // A rootless old owner cannot keep a nursery target alive through a
        // stale remembered edge.
        let mut arena = HeapArena::new();
        let owner = arena.allocate(3u8);
        let mut owner_roots = RootRegistry::new();
        owner_roots.add(LifetimeDomain::Realm, owner);
        arena.collect_unrooted(&owner_roots);
        let target = arena.allocate(4u8);
        arena.record_write(owner, target);
        owner_roots.clear(LifetimeDomain::Realm);
        assert_eq!(arena.collect_unrooted(&owner_roots), 2);
        assert!(arena.is_empty());
    }
    #[test]
    fn write_barrier_requires_live_old_owner_and_nursery_target() {
        let mut arena = HeapArena::new();
        let owner = arena.allocate(1u8);
        let discarded = arena.allocate(2u8);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, owner);
        arena.collect_unrooted(&roots);
        let target = arena.allocate(3u8);
        assert!(arena.record_write(owner, target));
        assert!(!arena.record_write(owner, target));
        assert_eq!(arena.remembered_len(), 1);
        assert_eq!(arena.stats().remembered_writes, 1);

        roots.remove(LifetimeDomain::Realm, owner);
        assert_eq!(arena.reclaim(owner), Some(1));
        assert!(!arena.record_write(owner, target));
        assert_eq!(arena.remembered_len(), 0);

        let live_owner = arena.allocate(4u8);
        arena.collect_unrooted(&RootRegistry::new());
        assert!(!arena.record_write(live_owner, discarded));
        assert_eq!(arena.remembered_len(), 0);
    }
    #[test]
    fn remembered_edges_are_canonical_and_pruned_at_collection_boundary() {
        let mut arena = HeapArena::new();
        let owner = arena.allocate(1u8);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, owner);
        assert_eq!(arena.collect_unrooted(&roots), 0);

        let target = arena.allocate(2u8);
        assert!(arena.record_write(owner, target));
        assert_eq!(
            arena.remembered_edges().collect::<Vec<_>>(),
            vec![(owner, target)]
        );

        roots.clear(LifetimeDomain::Realm);
        assert_eq!(arena.collect_unrooted(&roots), 2);
        assert!(arena.remembered_edges().next().is_none());
    }
    #[test]
    fn write_barrier_ignores_nursery_old_and_invalid_targets() {
        let mut arena = HeapArena::new();
        let owner = arena.allocate(1u8);
        let _discarded = arena.allocate(2u8);
        arena.record_write(owner, HeapRef(1));
        arena.record_write(owner, HeapRef(999));
        assert_eq!(arena.remembered_len(), 0);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, owner);
        arena.collect_unrooted(&roots);
        let target = arena.allocate(3u8);
        arena.record_write(owner, target);
        assert_eq!(arena.remembered_len(), 1);
    }

    #[test]
    fn bump_frontier_stays_at_peak_while_reuse_preserves_lifetime() {
        let mut arena = HeapArena::new().with_nursery_limit(2);
        let first = arena.try_allocate(String::from("request")).unwrap();
        let second = arena.try_allocate(String::from("live")).unwrap();
        assert_eq!(arena.nursery_remaining(), 0);
        assert_eq!(arena.reclaim(first).as_deref(), Some("request"));
        assert_eq!(arena.get(second).map(String::as_str), Some("live"));
        assert_eq!(arena.nursery_remaining(), 0);
        let reused = arena.try_allocate(String::from("next")).unwrap();
        assert_eq!(reused, first);
        assert_eq!(arena.get(second).map(String::as_str), Some("live"));
        assert!(matches!(
            arena.try_allocate(String::from("overflow")),
            Err(AllocationFailure::NurseryOverflow)
        ));
    }

    #[test]
    fn nursery_reuses_reclaimed_slot_after_append_limit() {
        let mut arena = HeapArena::new().with_nursery_limit(1);
        let first = arena.try_allocate(1u8).expect("initial nursery slot");
        assert_eq!(arena.nursery_remaining(), 0);
        assert_eq!(arena.reclaim(first), Some(1));

        // A full append frontier must still permit reuse of a reclaimed slot.
        let reused = arena
            .try_allocate(2u8)
            .expect("reclaimed slot bypasses append overflow");
        assert_eq!(reused, first);
        assert_eq!(arena.get(reused), Some(&2));
        assert_eq!(arena.nursery_remaining(), 0);
        assert!(matches!(
            arena.try_allocate(3u8),
            Err(AllocationFailure::NurseryOverflow)
        ));
    }

    #[test]
    fn reclaim_prunes_remembered_owner() {
        let mut arena = HeapArena::new();
        let owner = arena.allocate(1u8);
        let _discarded = arena.allocate(2u8);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, owner);
        arena.collect_unrooted(&roots);
        let target = arena.allocate(3u8);
        arena.record_write(owner, target);
        assert_eq!(arena.remembered().collect::<Vec<_>>(), vec![owner]);
        assert_eq!(arena.reclaim(owner), Some(1u8));
        assert_eq!(arena.remembered().count(), 0);
    }
    #[test]
    fn collection_prunes_dead_remembered_owners_and_reused_slots() {
        let mut arena = HeapArena::new();
        let owner = arena.allocate(1u8);
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, owner);
        arena.collect_unrooted(&roots);
        let target = arena.allocate(2u8);
        arena.record_write(owner, target);
        assert_eq!(arena.remembered().collect::<Vec<_>>(), vec![owner]);

        // The owner is no longer rooted. Collection must remove it from the
        // remembered set; a subsequent nursery reuse must not resurrect it.
        roots.remove(LifetimeDomain::Realm, owner);
        assert_eq!(arena.collect_unrooted(&roots), 2);
        assert_eq!(arena.remembered_len(), 0);
        let reused = arena.allocate(3u8);
        assert!(arena.get(reused).is_some());
        assert_eq!(arena.remembered_len(), 0);
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
    fn collection_releases_dead_trailing_slots() {
        let mut arena = HeapArena::new();
        let dropped = arena.allocate(1u8);
        arena.allocate(2u8);

        assert_eq!(arena.collect_unrooted(&RootRegistry::new()), 2);
        assert_eq!(arena.values.len(), 0);
        assert_eq!(arena.sizes.len(), 0);
        assert_eq!(arena.generations.len(), 0);
        assert_eq!(arena.values.capacity(), 0);
        assert!(arena.get(dropped).is_none());
        assert_eq!(arena.allocate(3u8), HeapRef(0));
    }
    /// Deterministic lifetime cohort measurement: short-lived nursery objects
    /// disappear, while rooted survivors promote exactly once per collection.
    /// This is the evidence needed before introducing additional generations.
    #[test]
    fn lifetime_cohorts_match_nursery_promotion_metrics() {
        let mut arena = HeapArena::new().with_gc_threshold(u64::MAX);
        let long_lived = arena.allocate_sized((), 16);
        let _short_lived = (0..7)
            .map(|_| arena.allocate_sized((), 8))
            .collect::<Vec<_>>();
        let mut roots = RootRegistry::new();
        roots.add(LifetimeDomain::Realm, long_lived);

        assert_eq!(arena.generation_counts(), (8, 0));
        assert_eq!(arena.stats().promoted_objects, 0);
        assert_eq!(arena.stats().nursery_reclaimed, 0);
        let reclaimed = arena.collect_unrooted(&roots);

        assert_eq!(reclaimed, 7);
        assert_eq!(arena.generation_counts(), (0, 1));
        assert_eq!(arena.stats().promoted_objects, 1);
        assert_eq!(arena.stats().nursery_reclaimed, 7);
        assert_eq!(arena.stats().live_objects, 1);
        assert_eq!(arena.stats().live_bytes, 16);
        assert_eq!(arena.stats().collections, 1);

        // A second collection measures the same survivor as old, rather than
        // counting a promotion again.
        assert_eq!(arena.collect_unrooted(&roots), 0);
        assert_eq!(arena.stats().promoted_objects, 1);
        assert_eq!(arena.stats().nursery_reclaimed, 7);
    }
    #[test]
    fn zero_gc_threshold_is_clamped_and_boundary_is_inclusive() {
        let mut arena = HeapArena::<u8>::new().with_gc_threshold(0);
        assert_eq!(arena.gc_threshold(), 1);
        assert!(!arena.should_collect());

        arena.allocate_sized(1, 1);
        assert_eq!(arena.allocation_bytes_since_collection(), 1);
        assert!(arena.should_collect());
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
    fn external_metadata_is_accounting_only_and_slot_independent() {
        let mut arena = HeapArena::<DropProbe>::new();
        let drops = Rc::new(Cell::new(0));
        let reference = arena.allocate(DropProbe(drops.clone()));
        let live_before = arena.stats().live_bytes;
        let slots_before = arena.values.len();

        arena.charge_external(256);
        assert_eq!(arena.stats().external_bytes, 256);
        assert_eq!(arena.stats().live_bytes, live_before);
        assert_eq!(arena.values.len(), slots_before);
        assert!(arena.get(reference).is_some());

        let roots = RootRegistry::new();
        assert_eq!(arena.collect_unrooted(&roots), 1);
        assert_eq!(drops.get(), 1);
        assert_eq!(arena.stats().external_bytes, 256);
        assert_eq!(arena.stats().live_bytes, 0);

        arena.release_external(256);
        assert_eq!(arena.stats().external_bytes, 0);
        assert_eq!(EXTERNAL_METADATA_BYTES, size_of::<u64>());
    }
    /// External storage is owned by its producer (for example an ArrayBuffer
    /// backing store or a native addon allocation), so the producer must
    /// charge at creation and release exactly once at destruction/detachment.
    #[test]
    fn external_accounting_tracks_array_buffer_and_native_lifecycles() {
        let mut arena = HeapArena::<u8>::new();

        // The isolate owns the accounting; the backing stores remain external.
        arena.charge_external(1024); // ArrayBuffer backing store.
        arena.charge_external(37); // Native allocation.
        assert_eq!(arena.stats().external_bytes, 1061);
        assert_eq!(
            arena.accounted_bytes(),
            arena.stats().committed_bytes + 1061
        );

        arena.release_external(1024);
        assert_eq!(arena.stats().external_bytes, 37);
        arena.release_external(37);
        assert_eq!(arena.stats().external_bytes, 0);
    }
    #[test]
    fn external_source_provenance_shares_one_pressure_account() {
        let mut arena = HeapArena::<u8>::new().with_gc_threshold(10);
        arena.charge_external_from(ExternalMemorySource::ArrayBuffer, 6);
        arena.charge_external_from(ExternalMemorySource::Native, 4);
        assert_eq!(arena.stats().external_bytes, 10);
        assert_eq!(arena.allocation_bytes_since_collection(), 10);
        assert!(arena.should_collect());

        arena.release_external_from(ExternalMemorySource::ArrayBuffer, 6);
        assert_eq!(arena.stats().external_bytes, 4);
        assert_eq!(arena.allocation_bytes_since_collection(), 10);
    }
    #[test]
    fn external_pressure_triggers_at_threshold_boundary() {
        let mut arena = HeapArena::<u8>::new().with_gc_threshold(100);
        arena.charge_external(99);
        assert!(!arena.should_collect());
        arena.charge_external(1);
        assert!(arena.should_collect());

        let roots = RootRegistry::new();
        assert_eq!(arena.collect_if_needed(&roots), Some(0));
        assert!(!arena.should_collect());
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
        const {
            assert!(CACHE_LINE_BYTES.is_power_of_two());
            assert!(HOT_HEADER_BYTES <= CACHE_LINE_BYTES);
            assert!(HOT_HEADER_BYTES <= 128);
            assert!(SLOT_METADATA_BYTES <= HOT_HEADER_BYTES);
        };
    }
    #[test]
    fn parallel_slot_vectors_remain_index_aligned_through_reuse() {
        let mut arena = HeapArena::new();
        let first = arena.allocate_sized(1u8, 8);
        let second = arena.allocate_sized(2u8, 16);
        assert_eq!(arena.values.len(), arena.sizes.len());
        assert_eq!(arena.values.len(), arena.generations.len());

        assert_eq!(arena.reclaim(first), Some(1));
        let reused = arena.allocate_sized(3u8, 32);
        assert_eq!(reused, first);
        assert_eq!(arena.values.len(), arena.sizes.len());
        assert_eq!(arena.values.len(), arena.generations.len());
        assert_eq!(arena.get(second), Some(&2));
    }
    #[test]
    fn hot_counters_have_one_canonical_snapshot_across_cold_collection_metadata() {
        let mut arena = HeapArena::new();
        let root = arena.allocate_sized(7u8, 24);
        let before = arena.stats();
        assert_eq!(before.live_objects, 1);
        assert_eq!(before.live_bytes, 24);
        assert_eq!(before.collections, 0);

        // Collection diagnostics are cold and evolve in place; arithmetic
        // facts remain sourced from the same arena-owned snapshot fields.
        let roots = RootRegistry::new();
        assert_eq!(arena.collect_unrooted(&roots), 1);
        let after = arena.stats();
        assert_eq!(arena.get(root), None);
        assert_eq!(after.live_objects, 0);
        assert_eq!(after.live_bytes, 0);
        assert_eq!(after.collections, 1);
        assert_eq!(after.nursery_reclaimed, 1);
        assert_eq!(arena.stats(), after);
    }

    #[test]
    fn heap_stats_layout_is_explicit_without_padding_for_sharing() {
        assert_eq!(size_of::<HeapStats>(), HEAP_STATS_BYTES);
        assert_eq!(size_of::<HeapStats>(), 144);
        assert_eq!(std::mem::align_of::<HeapStats>(), size_of::<u64>());
        assert!(size_of::<HeapStats>() < CACHE_LINE_BYTES * 3);
    }
    #[test]
    fn heap_stats_hot_prefix_is_contiguous_and_cache_line_bounded() {
        assert_eq!(std::mem::offset_of!(HeapStats, reserved_bytes), 0);
        assert_eq!(
            std::mem::offset_of!(HeapStats, size_classes),
            HEAP_HOT_PREFIX_BYTES
        );
        assert!(HEAP_HOT_PREFIX_BYTES <= CACHE_LINE_BYTES);
    }
    #[test]
    fn heap_stats_snapshots_are_isolate_owned_and_do_not_alias() {
        let mut left = HeapArena::new();
        let mut right = HeapArena::new();
        left.allocate_sized(1u8, 8);
        right.allocate_sized(2u8, 32);

        let left_snapshot = left.stats();
        let right_snapshot = right.stats();
        assert_ne!(left_snapshot.live_bytes, right_snapshot.live_bytes);

        // Returned snapshots are detached from both arena owners.
        let mut detached = left_snapshot;
        detached.live_bytes = 0;
        assert_eq!(left.stats().live_bytes, 8);
        assert_eq!(right.stats().live_bytes, 32);
    }

    #[test]
    fn immutable_metadata_starts_old() {
        let mut arena = HeapArena::new();
        let reference = arena.allocate_immutable(7u32);
        assert_eq!(arena.generation_counts(), (0, 1));
        assert_eq!(arena.get(reference), Some(&7));
    }

    #[test]
    fn failed_allocation_leaves_no_partial_logical_state() {
        let mut arena = HeapArena::new().with_nursery_limit(1);
        let first = arena.try_allocate_sized(7u8, 17).unwrap();
        let before = arena.stats();
        let before_lengths = (
            arena.values.len(),
            arena.sizes.len(),
            arena.generations.len(),
        );

        assert_eq!(
            arena.try_allocate_sized(8u8, 17),
            Err(AllocationFailure::NurseryOverflow)
        );
        assert_eq!(arena.stats(), before);
        assert_eq!(
            (
                arena.values.len(),
                arena.sizes.len(),
                arena.generations.len()
            ),
            before_lengths
        );
        assert_eq!(arena.get(first), Some(&7));
        assert!(arena.counters_consistent());
    }
    #[test]
    fn nursery_limit_is_checked_against_frontier_without_forging_heap_refs() {
        let mut arena = HeapArena::new().with_nursery_limit(2);
        let first = arena.try_allocate(1u8).unwrap();
        let second = arena.try_allocate(2u8).unwrap();
        let before = arena.stats();

        // Tightening the configured budget cannot invalidate existing slot
        // identities, and a failed append must not mutate any parallel vector.
        arena.nursery_limit = 1;
        assert_eq!(
            arena.try_allocate(3u8),
            Err(AllocationFailure::NurseryOverflow)
        );
        assert_eq!(arena.stats(), before);
        assert_eq!(arena.get(first), Some(&1));
        assert_eq!(arena.get(second), Some(&2));
        assert_eq!(arena.nursery_remaining(), 0);
        assert!(arena.counters_consistent());
    }

    #[test]
    fn fallible_immutable_metadata_respects_nursery_capacity() {
        let mut arena = HeapArena::new().with_nursery_limit(0);
        assert_eq!(
            arena.try_allocate_immutable(7u32),
            Err(AllocationFailure::NurseryOverflow)
        );
        assert_eq!(arena.live_len(), 0);
        assert_eq!(arena.generation_counts(), (0, 0));
    }

    #[test]
    fn infallible_allocation_reports_cold_failure_source() {
        let mut arena = HeapArena::new().with_nursery_limit(0);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            arena.allocate(7u32);
        }))
        .expect_err("infallible allocation must panic at the cold failure boundary");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .expect("panic payload should be a string");
        assert!(message.contains("heap nursery allocation failed: NurseryOverflow"));
        assert_eq!(arena.live_len(), 0);
        assert!(arena.counters_consistent());
    }
    #[test]
    fn nursery_allocations_are_contiguous_before_reuse() {
        let mut arena = HeapArena::new();
        let first = arena.allocate(1u8);
        let second = arena.allocate(2u8);
        assert_eq!(second.0, first.0 + 1);
        assert!(arena.bump_reuse_invariant());
        assert_eq!(arena.nursery_remaining(), 4094);
    }

    #[test]
    fn arena_page_quantum_is_deliberate_and_accounting_aligned() {
        assert!(ARENA_PAGE_BYTES.is_power_of_two());
        assert_eq!(ARENA_PAGE_BYTES, 4096);
        let mut arena = HeapArena::new();
        arena.allocate_sized((), ARENA_PAGE_BYTES + 1);
        arena.allocate_sized((), ARENA_PAGE_BYTES + 1);
        assert_eq!(arena.stats().reserved_bytes % ARENA_PAGE_BYTES as u64, 0);
        assert_eq!(arena.page_count(), 2);
    }
    #[test]
    fn size_class_catch_all_is_sourced_from_arena_page_quantum() {
        assert_eq!(
            HeapArena::<u8>::size_class_capacity(ARENA_PAGE_BYTES),
            ARENA_PAGE_BYTES
        );
        assert_eq!(
            HeapArena::<u8>::size_class_capacity(ARENA_PAGE_BYTES.saturating_add(1)),
            ARENA_PAGE_BYTES
        );
        assert_eq!(
            HeapArena::<u8>::size_class_capacity(usize::MAX),
            ARENA_PAGE_BYTES
        );
    }

    #[test]
    fn arena_storage_is_page_aligned_by_type_contract() {
        assert_eq!(align_of::<HeapArena<()>>(), ARENA_PAGE_BYTES);
        let arena = HeapArena::<()>::new();
        let address = std::ptr::addr_of!(arena) as usize;
        assert_eq!(address % ARENA_PAGE_BYTES, 0);
    }
    #[test]
    fn page_accounting_tracks_arena_commitment() {
        let mut arena = HeapArena::new();
        arena.allocate_sized([0u8; 128], 128);
        assert_eq!(arena.page_count(), 1);
        assert!(arena.stats().reserved_bytes >= arena.stats().committed_bytes);
    }
    #[test]
    fn page_accounting_consistency_survives_reclaim_and_reuse() {
        let mut arena = HeapArena::new();
        let first = arena.allocate_sized((), 9);
        let second = arena.allocate_sized((), 2049);
        assert!(arena.page_accounting_consistent());
        assert_eq!(arena.reclaim(first), Some(()));
        assert!(arena.page_accounting_consistent());
        arena.allocate_sized((), 65);
        assert!(arena.page_accounting_consistent());
        assert_eq!(arena.page_count(), 2);
        assert_eq!(arena.get(second), Some(&()));
    }
    #[test]
    fn reclaimed_page_slot_reuse_preserves_reserved_capacity_and_updates_class() {
        let mut arena = HeapArena::new();
        let first = arena.allocate_sized(1u8, 8);
        let second = arena.allocate_sized(2u8, 8);
        assert_eq!(arena.page_count(), 2);
        let committed = arena.stats().committed_bytes;
        let reserved = arena.stats().reserved_bytes;
        assert!(reserved >= committed);

        assert_eq!(arena.reclaim(first), Some(1u8));
        assert_eq!(arena.page_count(), 2);
        assert!(arena.stats().committed_bytes < committed);
        assert_eq!(arena.stats().reserved_bytes, reserved);
        assert!(arena.stats().reserved_bytes >= arena.stats().committed_bytes);
        let reused = arena.allocate_sized(3u8, 2048);
        assert_eq!(reused, first);
        assert_eq!(arena.get(second), Some(&2u8));
        assert_eq!(arena.stats().size_classes[0], 1);
        assert_eq!(arena.stats().size_classes[7], 1);
        assert_eq!(arena.page_count(), 2);
        assert_eq!(arena.stats().reserved_bytes, reserved);
    }
    #[test]
    fn free_slot_reuse_is_lifo_and_rewrites_all_slot_metadata() {
        let mut arena = HeapArena::new().with_nursery_limit(3);
        let first = arena.allocate_sized("first", 8);
        let second = arena.allocate_sized("second", 128);
        let third = arena.allocate_sized("third", 2048);
        assert_eq!(arena.nursery_remaining(), 0);

        assert_eq!(arena.reclaim(first), Some("first"));
        assert_eq!(arena.reclaim(second), Some("second"));
        // The free list is the only source of reused identities and is LIFO.
        let reused_second = arena.try_allocate_sized("replacement", 4096).unwrap();
        assert_eq!(reused_second, second);
        assert_eq!(arena.get(first), None);
        assert_eq!(arena.get(reused_second), Some(&"replacement"));
        assert_eq!(arena.stats().size_classes[0], 0);
        assert_eq!(arena.stats().size_classes[7], 2);
        assert_eq!(arena.get(third), Some(&"third"));
        assert_eq!(arena.nursery_remaining(), 0);
    }

    #[test]
    fn page_accounting_is_derived_from_live_slots_and_reserved_slot_classes() {
        let mut arena = HeapArena::new();
        let small = arena.allocate_sized((), 1);
        let medium = arena.allocate_sized((), 9);
        let large = arena.allocate_sized((), 4097);
        let reserved = arena.stats().reserved_bytes;
        assert_eq!(
            reserved,
            arena.page_count() as u64 * ARENA_PAGE_BYTES as u64
        );
        assert_eq!(arena.stats().committed_bytes, reserved);
        assert_eq!(arena.reclaim(medium), Some(()));
        assert_eq!(arena.stats().reserved_bytes, reserved);
        assert!(arena.stats().committed_bytes < reserved);
        assert_eq!(
            arena.page_count(),
            (reserved / ARENA_PAGE_BYTES as u64) as usize
        );
        let replacement = arena.allocate_sized((), 16);
        assert_eq!(replacement, medium);
        assert_eq!(arena.stats().reserved_bytes, reserved);
        assert_eq!(arena.stats().committed_bytes, reserved);
        assert!(arena.get(small).is_some());
        assert!(arena.get(large).is_some());
    }
    #[test]
    fn size_class_boundaries_are_total_and_canonical() {
        let boundaries = [
            0,
            1,
            8,
            9,
            16,
            17,
            32,
            33,
            64,
            65,
            128,
            129,
            256,
            257,
            512,
            513,
            1024,
            1025,
            4096,
            usize::MAX,
        ];
        let classes: Vec<_> = boundaries
            .iter()
            .map(|&size| HeapArena::<u8>::size_class_for(size))
            .collect();
        assert_eq!(
            classes,
            [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 7, 7, 7]
        );
        assert_eq!(HeapArena::<u8>::size_class_capacity(0), 1);
        assert_eq!(HeapArena::<u8>::size_class_capacity(1025), 2048);
        assert_eq!(HeapArena::<u8>::size_class_capacity(usize::MAX), 4096);
    }

    #[test]
    fn reclaimed_slots_measure_fragmentation_until_reused() {
        let mut arena = HeapArena::new();
        let first = arena.allocate_sized(1u8, 9);
        let second = arena.allocate_sized(2u8, 1025);
        let reserved = arena.stats().reserved_bytes;
        assert_eq!(arena.fragmentation_bytes(), 0);
        assert_eq!(arena.reclaim(first), Some(1));
        assert_eq!(arena.stats().reserved_bytes, reserved);
        assert!(arena.fragmentation_bytes() > 0);
        let reused = arena.allocate_sized(3u8, 9);
        assert_eq!(reused, first);
        assert_eq!(arena.fragmentation_bytes(), 0);
        assert_eq!(arena.stats().size_classes.iter().sum::<u64>(), 2);
        assert_eq!(arena.get(second), Some(&2));
    }
    #[test]
    fn counter_sources_remain_independent_across_transitions() {
        let mut arena = HeapArena::new();
        assert!(arena.counters_consistent());
        let first = arena.allocate_sized((), 9);
        let second = arena.allocate_sized((), 33);
        let allocated = arena.stats().allocated_bytes;
        let live = arena.stats().live_bytes;
        assert!(arena.counters_consistent());
        arena.charge_external(700);
        assert_eq!(arena.stats().live_bytes, live);
        assert_eq!(arena.stats().allocated_bytes, allocated);
        assert!(arena.counters_consistent());
        assert_eq!(arena.reclaim(first), Some(()));
        assert!(arena.counters_consistent());
        let reserved = arena.stats().reserved_bytes;
        assert!(arena.stats().committed_bytes < reserved);
        let replacement = arena.allocate_sized((), 65);
        assert_eq!(replacement, first);
        assert!(arena.counters_consistent());
        assert_eq!(arena.get(second), Some(&()));
        arena.release_external(700);
        assert_eq!(arena.stats().external_bytes, 0);
        assert!(arena.counters_consistent());
    }

    #[test]
    fn allocation_metadata_is_live_slot_source_contract() {
        let mut arena = HeapArena::new();
        let reference = arena.allocate_sized(7u8, 40);
        assert_eq!(
            arena.allocation_metadata(reference),
            Some(AllocationMetadata {
                bytes: 40,
                class: 3,
                generation: Generation::Nursery,
            })
        );
        assert_eq!(arena.allocation_metadata(HeapRef(u32::MAX)), None);
        assert_eq!(arena.reclaim(reference), Some(7));
        assert_eq!(arena.allocation_metadata(reference), None);
    }
    #[test]
    fn allocation_metadata_rejects_incomplete_parallel_indexes() {
        let mut arena = HeapArena::new();
        let reference = arena.allocate_sized(7u8, 40);
        arena.sizes.pop();
        assert_eq!(arena.allocation_metadata(reference), None);
    }

    #[test]
    fn pinned_slots_survive_collection_until_explicitly_unpinned() {
        let mut arena = HeapArena::new();
        let pinned = arena.allocate(7u8);
        let discarded = arena.allocate(9u8);
        assert!(arena.pin(pinned));
        assert!(arena.is_pinned(pinned));
        assert_eq!(arena.collect_unrooted(&RootRegistry::new()), 1);
        assert_eq!(arena.get(pinned), Some(&7));
        assert_eq!(arena.get(discarded), None);
        assert!(arena.unpin(pinned));
        assert_eq!(arena.collect_unrooted(&RootRegistry::new()), 1);
        assert_eq!(arena.get(pinned), None);
        assert!(!arena.pin(pinned));
    }

    #[test]
    fn stale_pin_cannot_keep_reused_slot_alive() {
        let mut arena = HeapArena::new();
        let old = arena.allocate(1u8);
        assert!(arena.pin(old));
        assert!(arena.unpin(old));
        assert_eq!(arena.collect_unrooted(&RootRegistry::new()), 1);
        let reused = arena.allocate(2u8);
        assert_eq!(old, reused);
        assert!(!arena.is_pinned(reused));
        assert_eq!(arena.collect_unrooted(&RootRegistry::new()), 1);
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
    #[test]
    fn hot_and_cold_views_partition_the_canonical_snapshot() {
        let mut arena = HeapArena::new();
        arena.allocate_sized(7u8, 24);
        let snapshot = arena.stats();
        let hot = snapshot.hot();
        let cold = snapshot.cold();

        assert_eq!(hot.live_bytes, snapshot.live_bytes);
        assert_eq!(hot.live_objects, snapshot.live_objects);
        assert_eq!(cold.collections, snapshot.collections);
        assert_eq!(cold.size_classes, snapshot.size_classes);
        assert_eq!(cold.promoted_objects, snapshot.promoted_objects);
        assert_eq!(cold.nursery_reclaimed, snapshot.nursery_reclaimed);
        assert_eq!(snapshot, arena.stats());
    }
}
