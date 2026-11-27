use super::{ArrayObject, AwaitState, CollectionRecord, FunctionRecord, IteratorRecord, OrdinaryRecord, PromiseCell};
use super::pacing::{AgedFates, PromotionAge};

#[cfg(all(test, feature = "bench-primitives"))]
mod primitive_bench;
#[cfg(test)]
mod vacancy_tests;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod reservation;
pub(super) mod sweep_support;
use reservation::{Reservation, page_bytes, return_pages};
#[allow(unused_imports)]
pub(super) use reservation::AddressRange;
use crate::vm::wtf16::JsString;
use crate::value::Value;
use std::mem::MaybeUninit;
#[cfg(feature = "profile-memory")]
use std::mem::size_of;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) type RecordId = u32;

/// What a record keeps alive, copied out for the reference-count barrier
/// (`RecordSpace::rc_note`): the values its trace would push, read before a write changes them.
/// Shape roots are left out.
pub(super) trait TraceRecord {
    fn extend_children(&self, properties: &crate::value_vec::ValueArena, out: &mut Vec<Value>);
}

pub(super) type ObjectRecordSpace = RecordSpace<OrdinaryRecord, OBJECT_RECORD_CAPACITY>;
pub(super) type ArrayRecordSpace = RecordSpace<ArrayObject, ARRAY_RECORD_CAPACITY>;
pub(super) type FunctionRecordSpace = RecordSpace<FunctionRecord, FUNCTION_RECORD_CAPACITY>;
pub(super) type PromiseRecordSpace = RecordSpace<PromiseCell, PROMISE_RECORD_CAPACITY>;
pub(super) type AwaitRecordSpace = RecordSpace<AwaitState, AWAIT_RECORD_CAPACITY>;
/// Iterators: records only, with no legacy-slot fallback. The space holds
/// `ITERATOR_RECORD_CAPACITY` records, the window its prototype links name (`cell.rs`).
pub(super) type IteratorRecordSpace = RecordSpace<IteratorRecord, ITERATOR_RECORD_CAPACITY>;
/// Maps and Sets: records of an object and its entries, with no legacy-slot fallback. The space
/// holds `COLLECTION_RECORD_CAPACITY` records, the window its prototype links name (`cell.rs`).
pub(super) type CollectionRecordSpace = RecordSpace<CollectionRecord, COLLECTION_RECORD_CAPACITY>;
/// Environments have no legacy-slot fallback, so their space holds as many records as the
/// legacy slots do (`MAX_HEAP_CELLS`).
pub(super) type EnvironmentRecordSpace = RecordSpace<super::EnvironmentRecord, { super::MAX_HEAP_CELLS }>;
/// Symbols are immutable and have no legacy-slot fallback, so their space holds as many records
/// as the legacy slots do (`MAX_HEAP_CELLS`).
pub(super) type SymbolRecordSpace = RecordSpace<super::SymbolData, { super::MAX_HEAP_CELLS }>;
/// Wasm vectors: immutable leaves, so a record's bytes are its whole state.
pub(super) type V128RecordSpace = RecordSpace<[u8; crate::wasm::V128_BYTES], V128_RECORD_CAPACITY>;
/// Wasm GC objects: a record holds its head (`WasmGcHead`) and its payload, read in place by compiled code.
pub(super) type WasmGcRecordSpace = RecordSpace<super::WasmGcRecord, WASM_GC_RECORD_CAPACITY>;
/// Strings have no legacy-slot fallback, so their space holds as many records as the legacy
/// slots do (`MAX_HEAP_CELLS`).
/// The ids `[STRING_RECORD_HOLE, 2 * STRING_RECORD_HOLE)` are never this space's: they name the
/// records of small strings (`super::small_strings`), which stay below the narrow index limit.
pub(super) type StringRecordSpace = RecordSpace<JsString, STRING_RECORD_CAPACITY, STRING_RECORD_HOLE>;
pub(super) const STRING_RECORD_CAPACITY: usize = super::MAX_HEAP_CELLS;
pub(super) const STRING_RECORD_HOLE: usize = crate::staged::layout_offsets::STRING_RECORD_HOLE as usize;
const _: () = assert!(STRING_RECORD_HOLE.is_multiple_of(RECORDS_PER_BLOCK) && 2 * STRING_RECORD_HOLE <= STRING_RECORD_CAPACITY);
const _: () = assert!(STRING_RECORD_CAPACITY.is_power_of_two());
const _: () = assert!(STRING_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(STRING_RECORD_CAPACITY <= u32::MAX as usize);

/// The `Value` of a record of one family, named by the family's element type. A family names its
/// records by id, except the ordinary object, whose `Value` is its stub granule, which its record
/// holds (`OrdinaryRecord::stub`).
pub(super) trait RecordValue: Sized {
    /// The `Value` of record `id` of `space`. The id names a live record of `space` (`RecordFamily::value_of`).
    fn value_of<const CAPACITY: usize, const HOLE: usize>(space: &RecordSpace<Self, CAPACITY, HOLE>, id: RecordId) -> Value;
}

/// What the collector does to every record family alike, whatever its record type: the
/// per-collection passes iterate `Heap::record_families` instead of naming each space.
pub(super) trait RecordFamily {
    /// The `Value` of record `id`, which must be live in this family (`RecordValue::value_of`).
    fn value_of(&self, id: RecordId) -> Value;
    /// Every record becomes young: a full collection's starting state.
    fn clear_marks(&mut self);
    /// The records the barrier logged since the last collection; each is unmarked.
    fn take_remembered(&mut self) -> Vec<RecordId>;
    /// Before a minor collection marks under `age`: which records are newborn
    /// (`RecordSpace::prepare_minor`), with the barrier's `logged` records, old, among them.
    fn prepare_minor(&mut self, logged: &[RecordId], age: PromotionAge);
    /// The records the last minor sweep promoted from aged, emptied.
    fn take_promoted(&mut self) -> Vec<RecordId>;
    /// Every aged record becomes old: a full collection promotes every survivor.
    fn forget_ages(&mut self);
    /// What minor sweeps found of the aged records since the last call, emptied.
    fn take_aged_fates(&mut self) -> AgedFates;
    fn live_count(&self) -> usize;
    /// Bytes of the occupied ids: live plus unswept garbage before a sweep, the survivors
    /// after it (pacing's resident measure).
    fn occupied_bytes(&self) -> usize;
    /// Visits every live record's id.
    fn for_each_live_id(&self, visit: &mut dyn FnMut(RecordId));
    #[cfg(feature = "profile-aggregate")]
    fn high_water(&self) -> usize;
    #[cfg(feature = "profile-aggregate")]
    fn remembered_count(&self) -> usize;
    #[cfg(debug_assertions)]
    fn snapshot_marks(&self) -> Vec<u64>;
    #[cfg(debug_assertions)]
    fn restore_marks(&mut self, marks: &[u64]);
    #[cfg(debug_assertions)]
    fn for_each_marked(&self, visit: &mut dyn FnMut(RecordId));

    /// After a full collection's sweep: returns the pages of the empty blocks to the kernel and
    /// lowers the high water mark past the empty top blocks (`RecordSpace::return_free_pages`).
    fn return_free_pages(&mut self);
}

impl<T: RecordValue, const CAPACITY: usize, const HOLE: usize> RecordFamily for RecordSpace<T, CAPACITY, HOLE> {
    fn value_of(&self, id: RecordId) -> Value {
        T::value_of(self, id)
    }
    fn clear_marks(&mut self) {
        RecordSpace::clear_marks(self);
    }
    fn take_remembered(&mut self) -> Vec<RecordId> {
        RecordSpace::take_remembered(self)
    }
    fn prepare_minor(&mut self, logged: &[RecordId], age: PromotionAge) {
        RecordSpace::prepare_minor(self, logged, age);
    }
    fn take_promoted(&mut self) -> Vec<RecordId> {
        std::mem::take(&mut self.promoted)
    }
    fn forget_ages(&mut self) {
        RecordSpace::forget_ages(self);
    }
    fn take_aged_fates(&mut self) -> AgedFates {
        std::mem::take(&mut self.aged_fates)
    }
    fn live_count(&self) -> usize {
        RecordSpace::live_count(self)
    }
    fn occupied_bytes(&self) -> usize {
        self.live.saturating_mul(size_of::<T>())
    }
    fn for_each_live_id(&self, visit: &mut dyn FnMut(RecordId)) {
        self.for_each_live(|id, _| visit(id));
    }
    #[cfg(feature = "profile-aggregate")]
    fn high_water(&self) -> usize {
        RecordSpace::high_water(self)
    }
    #[cfg(feature = "profile-aggregate")]
    fn remembered_count(&self) -> usize {
        RecordSpace::remembered_count(self)
    }
    #[cfg(debug_assertions)]
    fn snapshot_marks(&self) -> Vec<u64> {
        RecordSpace::snapshot_marks(self)
    }
    #[cfg(debug_assertions)]
    fn restore_marks(&mut self, marks: &[u64]) {
        RecordSpace::restore_marks(self, marks);
    }
    fn return_free_pages(&mut self) {
        RecordSpace::return_free_pages(self);
    }
    #[cfg(debug_assertions)]
    fn for_each_marked(&self, visit: &mut dyn FnMut(RecordId)) {
        RecordSpace::for_each_marked(self, visit);
    }
}

const RECORDS_PER_BLOCK: usize = 1_024;
/// The fewest empty blocks, on each side of the high water mark, that keep their pages when a
/// full collection returns the rest (`RecordSpace::return_free_pages`).
const RESERVE_MIN_BLOCKS: usize = 8;
const OCCUPANCY_WORD_BITS: usize = u64::BITS as usize;
const BLOCK_WORDS: usize = RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS;
/// Records each typed space holds, one constant per space. A capacity is no longer bounded by
/// the prototype link: a record inside its family's link window (`cell.rs`, `*_PROTO_WINDOW`)
/// is linked as `window base + id`, and a record past it that becomes a prototype takes an
/// overflow link (`heap::prototypes`). The spaces reserve address space only (`MAP_NORESERVE`,
/// `records/reservation.rs`), so a large capacity costs no memory until records are used.
/// Every capacity is a power of two and a multiple of `RECORDS_PER_BLOCK`.
/// The capacities live in `staged::object_encoding`, beside the link layout they fit in.
pub(super) use crate::staged::object_encoding::{
    ARRAY_RECORD_CAPACITY, AWAIT_RECORD_CAPACITY, COLLECTION_RECORD_CAPACITY, FUNCTION_RECORD_CAPACITY,
    ITERATOR_RECORD_CAPACITY, OBJECT_RECORD_CAPACITY, PROMISE_RECORD_CAPACITY, V128_RECORD_CAPACITY,
    WASM_GC_RECORD_CAPACITY,
};

#[cfg(test)]
pub(super) const TEST_RECORD_CAPACITY: usize = 1 << 24;

const _: () = assert!(OBJECT_RECORD_CAPACITY.is_power_of_two() && ARRAY_RECORD_CAPACITY.is_power_of_two());
const _: () = assert!(FUNCTION_RECORD_CAPACITY.is_power_of_two() && PROMISE_RECORD_CAPACITY.is_power_of_two());
const _: () = assert!(ITERATOR_RECORD_CAPACITY.is_power_of_two() && COLLECTION_RECORD_CAPACITY.is_power_of_two());
const _: () = assert!(OBJECT_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK) && ARRAY_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(AWAIT_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK) && V128_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(WASM_GC_RECORD_CAPACITY.is_power_of_two() && WASM_GC_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
// Ids are `u32`, and the id after a space's last one must still fit.
const _: () = assert!(OBJECT_RECORD_CAPACITY < u32::MAX as usize && FUNCTION_RECORD_CAPACITY < u32::MAX as usize);

const _: () = assert!(RECORDS_PER_BLOCK.is_power_of_two());
const _: () = assert!(OCCUPANCY_WORD_BITS.is_power_of_two());

/// A space's allocation cursor: the run `[next, limit)` of unallocated ids the allocator hands out
/// one by one (`RecordSpace::cursor_take`), a free run or the fresh ids past the high water mark.
/// Every id of the run is occupied from the refill that made it (`cursor_refill`), so an
/// allocation writes no occupancy bit; the ids past
/// `next` are freed again by `retire_cursor` before any collection reads occupancy. Generated
/// code reads and bumps `next` through `NATIVE_RECORD_CURSOR_LAYOUT`, so this is `repr(C)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RecordCursor {
    pub(super) next: RecordId,
    pub(super) limit: RecordId,
}

/// Each record family owns one fixed address range. Block metadata controls
/// allocation and sweep, but resolving a live locator never walks it. A nonzero `HOLE` keeps
/// the ids `[HOLE, 2 * HOLE)` out of use: their blocks exist, with every id withheld.
pub(super) struct RecordSpace<T, const CAPACITY: usize, const HOLE: usize = 0> {
    pub(super) base: NonNull<MaybeUninit<T>>,
    reservation: Reservation,
    /// The records its address range holds: `CAPACITY` where the whole range could be mapped,
    /// else the largest power of two down to `MIN_RESERVED_RECORDS` that could
    /// (`ensure_reserved`). An id at or past it is never handed out.
    reserved: usize,
    blocks: Vec<RecordBlock>,
    /// Per block, whether a record was written in it since its pages were last returned to the
    /// kernel (`return_free_pages`); a block not dirty holds no touched record page. Kept in step
    /// with `blocks`, outside the block so that `RecordBlock` keeps the layout native code reads.
    dirty: Vec<bool>,
    next_id: u32,
    /// The free ids below the high water mark, as runs `[start, end)` each within one block.
    /// The sweep lists the ids it frees, `release_live` and `retire_cursor` list theirs, and a
    /// snapshot or a release of withheld ids rebuilds the list. Allocation takes the last run
    /// first (`take_free_run`). An id withheld after its run was listed is skipped when taken.
    free_runs: Vec<(RecordId, RecordId)>,
    pub(super) live: usize,
    /// Old records written since the last collection, each logged once by the write barrier.
    remembered: Vec<RecordId>,
    /// Where young records can be. Every sweep frees each occupied, unmarked record it
    /// visits, so between collections only the ids taken since the last sweep are occupied
    /// and unmarked: fresh ids from `young_floor` up, and the runs taken since. A
    /// minor sweep visits those words alone, at a cost that follows allocation, not heap size.
    young_floor: RecordId,
    /// The first id of each occupancy word a cursor run was taken from since the last sweep.
    young_words: Vec<RecordId>,
    /// The first id of each occupancy word holding an aged record (`RecordBlock::aged`),
    /// which the next minor sweep visits beside the words young records were allocated in.
    aged_words: Vec<RecordId>,
    /// The words the coming minor sweep visits under `PromotionAge::Second`, each once, by
    /// first id, with the records newborn in each: occupied, unmarked, not aged and not
    /// logged by the barrier (`prepare_minor`). The mark cannot tell them from the old records
    /// it finds, so they are named before it runs.
    minor_words: Vec<(RecordId, u64)>,
    /// The aged records the last minor sweep promoted, which may point at what stays young:
    /// the collector carries them to the next minor collection (`Heap::carried_owners`).
    promoted: Vec<RecordId>,
    /// What minor sweeps found of the aged records since the collector last took it.
    aged_fates: AgedFates,
    /// How many ids `withhold` keeps from reuse.
    withheld: usize,
    /// Touches what a dead record's drop will free, before the drop (`release`): the drops of
    /// one occupancy word then wait on their cache misses together, not one after another.
    warm: Option<fn(&T)>,
    /// Whether counts are kept (`QUENCH_DEV=rc-gc`, `super::rc`): the barrier then copies an
    /// old record's children before its first write in an epoch (`rc_note`) and logs every
    /// store into an old record.
    rc: bool,
    /// The counts, a block of them per record block while `rc` (`rc_ensure`).
    rc_blocks: Vec<RcBlock>,
    /// The references past `RC_STUCK` of each record whose count bits hold it: the exact
    /// count is `RC_STUCK` plus its entry. Counts above the bits are rare and brief (a record
    /// held by its live holder and by dead ones not yet freed), so a table holds them.
    rc_overflow: std::sync::Mutex<rustc_hash::FxHashMap<RecordId, u32>>,
    /// The children the barrier copied this epoch, for decrements, and the records it logged,
    /// for increments of their children at the epoch's end.
    rc_decrements: Vec<Value>,
    rc_logged: Vec<RecordId>,
    /// The bump cursor the hot spaces allocate from (`RecordCursor`); empty elsewhere.
    pub(super) cursor: RecordCursor,
}

impl<T, const CAPACITY: usize, const HOLE: usize> Default for RecordSpace<T, CAPACITY, HOLE> {
    fn default() -> Self {
        Self {
            base: NonNull::dangling(),
            reservation: Reservation::Unreserved,
            reserved: 0,
            blocks: Vec::new(),
            dirty: Vec::new(),
            next_id: 0,
            free_runs: Vec::new(),
            cursor: RecordCursor::default(),
            live: 0,
            remembered: Vec::new(),
            young_floor: 0,
            young_words: Vec::new(),
            aged_words: Vec::new(),
            minor_words: Vec::new(),
            promoted: Vec::new(),
            aged_fates: AgedFates::default(),
            withheld: 0,
            warm: None,
            rc: false,
            rc_blocks: Vec::new(),
            rc_overflow: std::sync::Mutex::new(rustc_hash::FxHashMap::default()),
            rc_decrements: Vec::new(),
            rc_logged: Vec::new(),
        }
    }
}

impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// The first address in this record family's reserved range, if it has been reserved.
    ///
    /// The range is allocated once and kept for the lifetime of the record space. A native
    /// reader may resolve a live direct Value through this base and its record ID, but it must
    /// keep the Value rooted and must not carry a record or payload pointer across a VM
    /// safepoint. In particular, a record's owned suballocations may change when the VM runs.
    #[inline(always)]
    pub(super) fn base_ptr(&self) -> Option<*const T> {
        self.is_reserved()
            .then(|| self.base.as_ptr().cast::<T>().cast_const())
    }

    /// `base_ptr` untested: the table a live record id indexes, which exists wherever a value
    /// names one (`get_live`).
    #[inline(always)]
    pub(super) fn live_base(&self) -> *const T {
        self.base.as_ptr().cast::<T>().cast_const()
    }
}

// Native JIT/AOT guards read `marked` using offsets exported from this layout (`staged::object_encoding`).
use crate::staged::object_encoding::RecordBlock;

/// A block's reference counts under `QUENCH_DEV=rc-gc` (`super::rc`), beside its
/// `RecordBlock` (whose layout native code reads): two bits per record, a saturating count of
/// 0..=3 where 3 is stuck. Atomic only so a count changes through a shared view of the space
/// (one thread; relaxed loads and stores, no read-modify-write).
struct RcBlock {
    lo: [AtomicU64; BLOCK_WORDS],
    hi: [AtomicU64; BLOCK_WORDS],
    /// Records whose count passed `RC_STUCK + RC_OVERFLOW_LIMIT`: no longer counted, they are
    /// freed only by a trace (prototypes, shared constant strings).
    stuck: [AtomicU64; BLOCK_WORDS],
}

impl RcBlock {
    fn new() -> Self {
        Self {
            lo: [const { AtomicU64::new(0) }; BLOCK_WORDS],
            hi: [const { AtomicU64::new(0) }; BLOCK_WORDS],
            stuck: [const { AtomicU64::new(0) }; BLOCK_WORDS],
        }
    }

    /// Clears the counts of the ids of `word_index` that `kept` leaves out.
    #[inline(always)]
    fn keep(&mut self, word_index: usize, kept: u64) {
        *self.lo[word_index].get_mut() &= kept;
        *self.hi[word_index].get_mut() &= kept;
        *self.stuck[word_index].get_mut() &= kept;
    }
}

/// Authoritative record-mark layout for native write-barrier guards.
pub(super) const NATIVE_RECORDS_PER_BLOCK: usize = RECORDS_PER_BLOCK;
pub(super) const NATIVE_RECORD_MARK_WORD_BITS: usize = OCCUPANCY_WORD_BITS;
pub(super) const NATIVE_RECORD_BLOCK_SIZE: usize = size_of::<RecordBlock>();
pub(super) const NATIVE_RECORD_MARK_OFFSET: usize = std::mem::offset_of!(RecordBlock, marked);
const _: () = assert!(NATIVE_RECORD_MARK_OFFSET + size_of::<[u64; BLOCK_WORDS]>() <= NATIVE_RECORD_BLOCK_SIZE);

impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// The blocks never move once the range is reserved (`ensure_reserved`); the count grows
    /// as allocation makes blocks.
    pub(super) fn native_mark_blocks(&self) -> (*const u8, usize) {
        (self.blocks.as_ptr().cast(), self.blocks.len())
    }

    /// Blocks made so far: grows with every block `native_mark_blocks` may have moved.
    pub(super) fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

/// The parallel mark sets a record's bit in its block's `marked` word through an atomic view
/// of that same word, so both must share one layout.
const _: () = assert!(
    size_of::<AtomicU64>() == size_of::<u64>() && align_of::<AtomicU64>() == align_of::<u64>()
);

/// One record space during a parallel full mark: its occupancy words, read-only, and its
/// mark words, set atomically. These are the words the serial mark sets with `mark`; only
/// the access differs. Workers read records and set bits, nothing else: no record is
/// inserted, removed or written, and no block is added, until the marking scope joins
/// (the stopped-mutator invariant of `parallel_mark::ParallelMarkView`).
pub(super) struct ParallelRecordMarks<'space, T> {
    blocks: *mut RecordBlock,
    block_count: usize,
    records: *const T,
    _space: std::marker::PhantomData<&'space mut [T]>,
}

impl<T> Clone for ParallelRecordMarks<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ParallelRecordMarks<'_, T> {}

impl<'space, T> ParallelRecordMarks<'space, T> {
    /// Sets record `id`'s mark bit when the record is occupied and the bit is clear: `true`
    /// for the one caller that set it, as `RecordSpace::mark` answers serially.
    #[inline(always)]
    pub(super) fn mark_if_new(&self, id: RecordId) -> bool {
        let (block_index, record_index) = (id as usize / RECORDS_PER_BLOCK, id as usize % RECORDS_PER_BLOCK);
        if block_index >= self.block_count {
            return false;
        }
        let word_index = record_index / OCCUPANCY_WORD_BITS;
        let mask = 1_u64 << (record_index % OCCUPANCY_WORD_BITS);
        // SAFETY: the block is in bounds, and no occupancy word is written while marking.
        let occupied = unsafe {
            let block = self.blocks.add(block_index);
            std::ptr::addr_of!((*block).occupied).cast::<u64>().add(word_index).read()
        };
        if occupied & mask == 0 {
            return false;
        }
        // SAFETY: the word is in bounds and aligned for an atomic (the layout assertion), and
        // while the view lives every access to mark words goes through atomics.
        let word = unsafe {
            let block = self.blocks.add(block_index);
            AtomicU64::from_ptr(std::ptr::addr_of_mut!((*block).marked).cast::<u64>().add(word_index))
        };
        super::mark_word_if_new(word, mask)
    }

    /// Whether record `id` is occupied.
    #[inline(always)]
    pub(super) fn occupied(&self, id: RecordId) -> bool {
        let (block_index, record_index) = (id as usize / RECORDS_PER_BLOCK, id as usize % RECORDS_PER_BLOCK);
        // SAFETY: the block is in bounds, and no occupancy word is written while marking.
        block_index < self.block_count
            && unsafe {
                let block = self.blocks.add(block_index);
                std::ptr::addr_of!((*block).occupied).cast::<u64>().add(record_index / OCCUPANCY_WORD_BITS).read()
            } & (1_u64 << (record_index % OCCUPANCY_WORD_BITS))
                != 0
    }

    /// The record `id`.
    ///
    /// # Safety
    /// `mark_if_new(id)` must have answered `true` (or the record be otherwise known to be
    /// occupied) through this view.
    #[inline(always)]
    pub(super) unsafe fn get(&self, id: RecordId) -> &'space T {
        // SAFETY: an occupied id names an initialized record, which no one writes or frees
        // while the view lives.
        unsafe { &*self.records.add(id as usize) }
    }
}


pub(super) enum RecordSweep<'a, T> {
    Keep(RecordId, &'a T),
    Dead(RecordId, T),
}

/// A sweep's two callbacks (`RecordSpace::sweep_disposing`): what frees a dead record in
/// place, and what it shows the records left to drop and its survivors.
struct Sweeper<'v, D, V> {
    dispose: D,
    visit: &'v mut V,
}

impl RecordBlock {
    fn new() -> Self {
        Self {
            occupied: [0; BLOCK_WORDS],
            withheld: [0; BLOCK_WORDS],
            marked: [0; BLOCK_WORDS],
            aged: [0; BLOCK_WORDS],
        }
    }

    /// Whether any id of this block is occupied or withheld.
    fn in_use(&self) -> bool {
        self.occupied.iter().chain(self.withheld.iter()).any(|&word| word != 0)
    }

    #[inline(always)] fn claim(&mut self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        let mask = 1_u64 << bit;
        let word = &mut self.occupied[word_index];
        if *word & mask != 0 {
            return false;
        }
        *word |= mask;
        true
    }

    #[inline(always)] fn is_occupied(&self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        self.occupied[word_index] & (1_u64 << bit) != 0
    }

    /// The occupancy bits of record indices `from..to` of this block, word by word: the masks
    /// of one occupancy word at a time. `set` occupies them (none may be occupied yet), else frees them.
    fn set_occupancy_range(&mut self, from: usize, to: usize, set: bool) {
        let mut index = from;
        while index < to {
            let word_index = index / OCCUPANCY_WORD_BITS;
            let word_end = ((word_index + 1) * OCCUPANCY_WORD_BITS).min(to);
            let span = word_end - index;
            let mask = (u64::MAX >> (OCCUPANCY_WORD_BITS - span)) << (index % OCCUPANCY_WORD_BITS);
            let word = &mut self.occupied[word_index];
            if set {
                debug_assert_eq!(*word & mask, 0, "a cursor run's ids are unoccupied");
                *word |= mask;
            } else {
                *word &= !mask;
            }
            index = word_end;
        }
    }

    /// The first withheld id in `from..to` (block indices), or `to` when none is.
    fn first_withheld(&self, from: usize, to: usize) -> usize {
        let mut index = from;
        while index < to {
            let word_index = index / OCCUPANCY_WORD_BITS;
            let word_end = ((word_index + 1) * OCCUPANCY_WORD_BITS).min(to);
            let width = word_end - index;
            let window = if width == OCCUPANCY_WORD_BITS { u64::MAX } else { (1_u64 << width) - 1 };
            let hits = (self.withheld[word_index] >> (index % OCCUPANCY_WORD_BITS)) & window;
            if hits != 0 {
                return index + hits.trailing_zeros() as usize;
            }
            index = word_end;
        }
        to
    }

    fn release(&mut self, index: usize) {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        debug_assert!(self.is_occupied(index));
        self.occupied[word_index] &= !(1_u64 << bit);
    }

    #[inline(always)] fn mark(&mut self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        let mask = 1_u64 << bit;
        if self.marked[word_index] & mask != 0 {
            return false;
        }
        debug_assert!(self.occupied[word_index] & mask != 0);
        self.marked[word_index] |= mask;
        true
    }

    fn is_marked(&self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        self.marked[word_index] & (1_u64 << bit) != 0
    }
}

impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// A dense identity claimed with no value written (the Value tag carries it): its record is
    /// uninitialized, to be written before the next safepoint (`try_insert` writes it at once).
    #[inline(always)]
    pub(super) fn claim_uninit(&mut self) -> Option<RecordId> {
        match self.claim_uninit_quick() {
            Some(id) => Some(id),
            None => self.claim_slow(),
        }
    }

    /// `claim_uninit` without its slow path: `None`, with nothing claimed, wherever that would run.
    /// The cursor's next id, else the next fresh id once no free run is left and its block exists.
    #[inline(always)]
    pub(super) fn claim_uninit_quick(&mut self) -> Option<RecordId> {
        if let Some(id) = self.cursor_take() {
            return Some(id);
        }
        if !self.free_runs.is_empty()
            || !self.is_reserved()
            || self.next_id as usize >= self.reserved
            || Self::split_id(self.next_id).0 >= self.blocks.len()
        {
            return None;
        }
        let id = self.next_id;
        let (block_index, record_index) = Self::split_id(id);
        if !self.claim_block(block_index, record_index) {
            return None;
        }
        self.next_id += 1;
        self.live += 1;
        Some(id)
    }

    #[inline(always)]
    pub(super) fn record_ptr(&mut self, id: RecordId) -> *mut T { unsafe { self.base.as_ptr().add(id as usize).cast::<T>() } }

    #[inline(always)]
    pub(super) fn try_insert(&mut self, value: T) -> Result<RecordId, T> {
        let Some(id) = self.claim_uninit() else { return Err(value) };
        // SAFETY: the occupancy bit claims this previously uninitialized id, in the reserved
        // range (a free word and a block exist only after reservation).
        unsafe {
            self.base
                .as_ptr()
                .add(id as usize)
                .write(MaybeUninit::new(value));
        };
        Ok(id)
    }

    /// The next id of the cursor's run, bumped (the staged `bump::cursor_take` row): the inline
    /// allocation, with no occupancy write (the refill occupied the run). `None` once spent.
    #[inline(always)]
    pub(super) fn cursor_take(&mut self) -> Option<RecordId> {
        let mut stage = crate::staged::Concrete::pure();
        crate::staged::bump::cursor_take(&mut stage, (&raw mut self.cursor) as u64, ()).map(|id| id as RecordId)
    }

    /// Whether the cursor's run has an id left, which `cursor_take` would hand out.
    #[inline(always)]
    pub(super) fn cursor_open(&self) -> bool {
        self.cursor.next < self.cursor.limit
    }

    /// Makes the cursor's run the next free run (`take_free_run`), else the rest of the block the
    /// high water mark is in, occupied as a whole, and returns the run's length (the allocation
    /// charge it carries). `None` where no run can be made: a hole space, or a range not reserved or full.
    #[cold]
    #[inline(never)]
    pub(super) fn cursor_refill(&mut self) -> Option<usize> {
        debug_assert!(self.cursor.next == self.cursor.limit, "a run is refilled once spent");
        if let Some(run) = self.take_free_run() {
            return Some(run);
        }
        if HOLE != 0 || !self.ensure_reserved() {
            return None;
        }
        let start = self.next_id as usize;
        if start >= self.reserved {
            return None;
        }
        let (block_index, first) = Self::split_id(self.next_id);
        let end = ((block_index + 1) * RECORDS_PER_BLOCK).min(self.reserved);
        self.add_blocks_through(block_index);
        let run = end - start;
        self.blocks[block_index].set_occupancy_range(first, first + run, true);
        self.dirty[block_index] = true;
        self.cursor = RecordCursor { next: start as RecordId, limit: end as RecordId };
        self.next_id = end as RecordId;
        self.live += run;
        Some(run)
    }

    /// Frees the unallocated rest of the cursor's run and empties the cursor, returning how many
    /// ids that was. Every collection calls it first (`Heap::retire_record_cursors`), so occupancy
    /// names only initialized records when the sweep and the marks read it.
    pub(super) fn retire_cursor(&mut self) -> usize {
        let RecordCursor { next, limit } = std::mem::take(&mut self.cursor);
        if next >= limit {
            return 0;
        }
        // A run lies within one block: `cursor_refill` never crosses a block boundary.
        let (block_index, first) = Self::split_id(next);
        let count = (limit - next) as usize;
        self.blocks[block_index].set_occupancy_range(first, first + count, false);
        // A tail at the high water mark is never used: the high water falls back to it. Any other
        // tail is listed again as a free run. The high water never falls below `young_floor`, which
        // the minor sweep's word list is built from.
        if limit == self.next_id && next >= self.young_floor {
            self.next_id = next;
        } else {
            self.free_runs.push((next, limit));
        }
        self.live -= count;
        count
    }

    /// Whether `id` lies in the cursor's run and is not yet allocated (occupied, unallocated).
    #[inline(always)]
    fn in_unallocated_run(cursor: RecordCursor, id: RecordId) -> bool {
        cursor.next <= id && id < cursor.limit
    }

    /// A fresh id for a record the caller initializes before any safepoint: the cursor's next,
    /// else the first id of a new run (`cursor_refill`), else the per-id claim. Allocation charges
    /// `allocated` per run here (one unit per id a run holds) and per id on the per-id path, which
    /// is the pacing approximation: a run is charged when it is claimed, and the unallocated rest
    /// of it is refunded at the next collection (`retire_cursor`). `None` only where the space is full.
    #[inline(always)]
    pub(super) fn bump_id(&mut self, allocated: &mut usize) -> Option<RecordId> {
        match self.cursor_take() {
            Some(id) => Some(id),
            None => self.bump_id_slow(allocated),
        }
    }

    /// `bump_id` where the cursor is spent: a new run, or the per-id claim where a run cannot be made.
    #[cold]
    #[inline(never)]
    fn bump_id_slow(&mut self, allocated: &mut usize) -> Option<RecordId> {
        if let Some(run) = self.cursor_refill() {
            *allocated += run;
            return self.cursor_take();
        }
        let id = self.claim_uninit()?;
        *allocated += 1;
        Some(id)
    }

    /// Occupies the first free run that holds no withheld id as the cursor's run, and returns its
    /// length; `None` once no free run is left. A withheld id can sit in a listed run (`withhold`
    /// follows the sweep that listed it): it ends the run taken, and the rest of the run is listed
    /// again, so the next take skips it.
    fn take_free_run(&mut self) -> Option<usize> {
        while let Some((start, end)) = self.free_runs.pop() {
            let (block_index, first) = Self::split_id(start);
            let to = first + (end - start) as usize;
            let found = self.blocks[block_index].first_withheld(first, to);
            if found == first {
                if first + 1 < to {
                    self.free_runs.push((start + 1, end));
                }
                continue;
            }
            if found < to {
                self.free_runs.push((start + (found - first) as RecordId, end));
            }
            let run = found - first;
            self.blocks[block_index].set_occupancy_range(first, found, true);
            self.dirty[block_index] = true;
            let block_base = block_index * RECORDS_PER_BLOCK;
            for word in (first / OCCUPANCY_WORD_BITS)..=((found - 1) / OCCUPANCY_WORD_BITS) {
                self.young_words.push((block_base + word * OCCUPANCY_WORD_BITS) as RecordId);
            }
            self.cursor = RecordCursor { next: start, limit: start + run as RecordId };
            self.live += run;
            return Some(run);
        }
        None
    }

    /// `claim_uninit`'s claim where the quick path has none: a free run's id, a new run of fresh
    /// ids, or the single fresh id a hole or a block boundary leaves.
    #[inline(never)]
    fn claim_slow(&mut self) -> Option<RecordId> {
        if !self.ensure_reserved() {
            return None;
        }
        if self.cursor_refill().is_some() {
            return self.cursor_take();
        }
        if HOLE != 0 && self.next_id as usize == HOLE {
            self.next_id = 2 * HOLE as RecordId;
        }
        let id = self.next_id;
        if id as usize >= self.reserved {
            return None;
        }
        // The id after the last one a space may hold; `CAPACITY` is below `u32::MAX`, so it fits.
        let following = id.checked_add(1)?;
        let (block_index, record_index) = Self::split_id(id);
        self.add_blocks_through(block_index);
        if !self.claim_block(block_index, record_index) {
            return None;
        }
        self.next_id = following;
        self.live += 1;
        Some(id)
    }

    /// Lists the free ids of `bits`, the occupancy bits of the word at `base` (each set bit names a
    /// free id), as runs.
    fn push_free_bits(&mut self, base: RecordId, mut bits: u64) {
        while bits != 0 {
            let low = bits.trailing_zeros();
            let count = (bits >> low).trailing_ones();
            let span = if count as usize >= OCCUPANCY_WORD_BITS { u64::MAX } else { ((1_u64 << count) - 1) << low };
            bits &= !span;
            self.push_free_run(base + low, base + low + count);
        }
    }

    /// Lists the free ids `[start, end)`, which lie in one block, as a run. The last run is extended
    /// when it ends at `start` inside the same block.
    fn push_free_run(&mut self, start: RecordId, end: RecordId) {
        if let Some(last) = self.free_runs.last_mut() {
            if last.1 == start && start as usize % RECORDS_PER_BLOCK != 0 {
                last.1 = end;
                return;
            }
        }
        self.free_runs.push((start, end));
    }

    /// Lists every free id below the high water mark: no occupied, withheld or unreached id. Run by
    /// the paths that make ids free outside a sweep's word (a snapshot's skipped ids, released withheld ids).
    fn rebuild_free_runs(&mut self) {
        self.free_runs.clear();
        let high = self.next_id as usize;
        for block_index in 0..self.blocks.len() {
            for word_index in 0..BLOCK_WORDS {
                let base = block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS;
                if base >= high {
                    return;
                }
                let block = &self.blocks[block_index];
                let free = !(block.occupied[word_index] | block.withheld[word_index]);
                let reached = if base + OCCUPANCY_WORD_BITS <= high { u64::MAX } else { (1_u64 << (high - base)) - 1 };
                self.push_free_bits(base as RecordId, free & reached);
            }
        }
    }

    /// Whether `id` lies in the ids `HOLE` keeps out of use.
    #[inline(always)]
    pub(super) const fn in_hole(id: RecordId) -> bool {
        HOLE != 0 && id as usize >= HOLE && (id as usize) < 2 * HOLE
    }

    /// `RecordBlock::claim` on the block at `block_index`, which marks the block dirty.
    #[inline(always)]
    fn claim_block(&mut self, block_index: usize, record_index: usize) -> bool {
        let claimed = self.blocks[block_index].claim(record_index);
        self.dirty[block_index] |= claimed;
        claimed
    }

    /// Adds blocks until `block_index` exists; a block of the hole withholds every id.
    fn add_blocks_through(&mut self, block_index: usize) {
        while self.blocks.len() <= block_index {
            let mut block = RecordBlock::new();
            if Self::in_hole((self.blocks.len() * RECORDS_PER_BLOCK) as RecordId) {
                block.withheld = [u64::MAX; BLOCK_WORDS];
            }
            self.blocks.push(block);
            self.dirty.push(false);
        }
    }

    /// Whether the block at `block_index` may give its pages back: no id of it is in use, and a
    /// record was written in it since its pages were last returned.
    fn returnable(&self, block_index: usize) -> bool {
        self.dirty[block_index] && !self.blocks[block_index].in_use()
    }

    /// Places `value` at `id`, as a snapshot restores it (`crate::snapshot`): ids arrive in
    /// increasing order, each marked (old), as after a full collection; the ids skipped are
    /// reusable once `finish_snapshot_inserts` runs. `false` when the id cannot be placed.
    pub(super) fn insert_snapshot(&mut self, id: RecordId, value: T) -> bool {
        if !self.ensure_reserved() || id as usize >= self.reserved || id < self.next_id || Self::in_hole(id) {
            return false;
        }
        let (block_index, record_index) = Self::split_id(id);
        self.add_blocks_through(block_index);
        if !self.claim_block(block_index, record_index) {
            return false;
        }
        // SAFETY: the occupancy bit claims this previously uninitialized id.
        unsafe { self.base.as_ptr().add(id as usize).write(MaybeUninit::new(value)) };
        self.blocks[block_index].mark(record_index);
        self.next_id = id + 1;
        self.live += 1;
        true
    }

    /// Makes the ids `insert_snapshot` skipped reusable.
    pub(super) fn finish_snapshot_inserts(&mut self) {
        self.rebuild_free_runs();
        self.young_floor = self.next_id;
    }

    /// The first id never used: a snapshot restores the space up to it.
    pub(super) fn high_water_id(&self) -> RecordId {
        self.next_id
    }

    /// Starts loading record `id`'s first cache line, for a read soon after.
    #[inline(always)]
    pub(super) fn prefetch(&self, id: RecordId) {
        // SAFETY: a prefetch reads nothing; the address is within the space's reservation.
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
            _mm_prefetch::<_MM_HINT_T0>(self.base.as_ptr().add(id as usize).cast::<i8>());
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = id;
    }

    /// # Safety
    /// `id` must come from a live direct Value of this record family. Sweep removes the
    /// record only after all weak owners have been pruned.
    #[inline(always)]
    pub(super) unsafe fn get_live(&self, id: RecordId) -> &T {
        debug_assert!(
            self.is_occupied(id),
            "live slot names an initialized record"
        );
        // SAFETY: guaranteed by the caller's live-slot invariant.
        unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() }
    }

    /// # Safety
    /// `id` must come from a live direct Value of this record family, and
    /// the caller must have exclusive access to the heap.
    /// Mutable access for the mutator. This is the record write barrier: an old (marked)
    /// record is unmarked and logged once, so the next minor collection traces it again.
    ///
    /// # Safety
    /// As for `get_live_mut_unbarriered`.
    #[inline(always)]
    pub(super) unsafe fn get_live_mut(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena) -> &mut T
    where
        T: TraceRecord,
    {
        self.barrier(id, properties);
        // SAFETY: the caller's contract.
        unsafe { self.get_live_mut_unbarriered(id) }
    }

    /// Mutable access for a store of `stored` into the record. Only a heap reference can
    /// make an old record point at a young one, so other values skip the generational
    /// barrier.
    ///
    /// # Safety
    /// As for `get_live_mut_unbarriered`.
    #[inline(always)]
    pub(super) unsafe fn get_live_mut_storing(
        &mut self,
        id: RecordId,
        stored: Value,
        properties: &crate::value_vec::ValueArena,
    ) -> &mut T
    where
        T: TraceRecord,
    {
        if self.rc {
            self.rc_barrier(id, properties);
        } else if stored.heap_index().is_some() {
            self.remember(id);
        }
        // SAFETY: the caller's contract.
        unsafe { self.get_live_mut_unbarriered(id) }
    }

    /// The write barrier for a write into record `id`: the generational log (`remember`).
    #[inline(always)]
    pub(super) fn barrier(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena)
    where
        T: TraceRecord,
    {
        if self.rc {
            self.rc_barrier(id, properties);
        } else {
            self.remember(id);
        }
    }

    /// The barrier under counts: an old record's first write in the epoch unmarks and logs it,
    /// as `remember` does, and copies the children it holds before the write.
    #[inline(always)]
    fn rc_barrier(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena)
    where
        T: TraceRecord,
    {
        if self.barrier_marked(id) {
            self.rc_note(id, properties);
        }
    }

    #[cold]
    #[inline(never)]
    fn rc_note(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena)
    where
        T: TraceRecord,
    {
        // The mark is set: `rc_barrier` tested it.
        self.unmark_and_log(id);
        self.rc_logged.push(id);
        // SAFETY: a barrier runs only for a live record of this family.
        let record = unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() };
        record.extend_children(properties, &mut self.rc_decrements);
    }


    /// Mutable access that writes no reference into the record (the collector's own
    /// compaction and remapping). It does not pass the write barrier.
    ///
    /// # Safety
    /// `id` must come from a live direct Value of this record family, and
    /// the caller must have exclusive access to the heap.
    #[inline(always)]
    pub(super) unsafe fn get_live_mut_unbarriered(&mut self, id: RecordId) -> &mut T {
        debug_assert!(
            self.is_occupied(id),
            "live slot names an initialized record"
        );
        // SAFETY: guaranteed by the caller's live-slot and exclusive-access invariants.
        unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_mut() }
    }

    /// Whether storing `stored` into record `id` logs it (`remember`): a reference into a
    /// record the last collection left marked and no store has logged since. Any other
    /// store passes the barrier without effect.
    #[inline(always)]
    pub(super) fn store_logs(&self, id: RecordId, stored: Value) -> bool {
        // Under counts, overwriting a reference with any value deletes an edge.
        if stored.heap_index().is_none() && !self.rc {
            return false;
        }
        self.barrier_marked(id)
    }

    /// Whether record `id` is marked (left marked by the last collection and not yet logged): the
    /// staged mark-bit row (`staged::barrier::mark_bit`) over this space's block table.
    #[inline(always)]
    fn barrier_marked(&self, id: RecordId) -> bool {
        // A live id belongs to an initialized block, so the staged bit test reads within the table.
        let mut stage = crate::staged::Concrete::pure();
        crate::staged::barrier::mark_bit(&mut stage, self.blocks.as_ptr() as u64, u64::from(id)) != 0
    }

    #[inline(always)]
    pub(super) fn remember(&mut self, id: RecordId) {
        if self.barrier_marked(id) {
            self.unmark_and_log(id);
        }
    }

    /// Clears the mark of record `id` (set by the caller's `barrier_marked` test) and logs it.
    #[inline(always)]
    fn unmark_and_log(&mut self, id: RecordId) {
        let (block_index, record_index) = Self::split_id(id);
        // SAFETY: a live id belongs to an initialized block.
        let block = unsafe { self.blocks.get_unchecked_mut(block_index) };
        block.marked[record_index / OCCUPANCY_WORD_BITS] &= !(1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        self.log_remembered(id);
    }

    /// Logs a record the barrier just unmarked, out of line: the log grows only once per
    /// record per collection, so the stores that pass the barrier keep no call in their path.
    #[cold]
    #[inline(never)]
    fn log_remembered(&mut self, id: RecordId) {
        self.remembered.push(id);
    }

    /// The records the barrier logged since the last collection; each is unmarked.
    pub(super) fn take_remembered(&mut self) -> Vec<RecordId> {
        std::mem::take(&mut self.remembered)
    }

    #[cfg(any(test, feature = "profile-aggregate"))]
    pub(super) fn remembered_count(&self) -> usize {
        self.remembered.len()
    }

    /// Frees the occupied record `id` without reading it: its value holds nothing to drop, or the
    /// caller has taken it (`ClassSpace::release_stub`). The id is reusable as a swept dead record's is.
    pub(super) fn release_live(&mut self, id: RecordId) {
        debug_assert!(self.is_occupied(id), "a release frees an initialized record");
        let (block_index, record_index) = Self::split_id(id);
        self.blocks[block_index].release(record_index);
        self.blocks[block_index].marked[record_index / OCCUPANCY_WORD_BITS] &=
            !(1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        self.push_free_run(id, id + 1);
        self.live -= 1;
    }

    #[cfg(test)]
    pub(super) fn remove_live(&mut self, id: RecordId) -> T {
        debug_assert!(self.is_occupied(id), "sweep removes an initialized record");
        // SAFETY: the id is initialized (asserted above) and is read once, before it is released.
        let value = unsafe { self.base.as_ptr().add(id as usize).read().assume_init() };
        self.release_live(id);
        value
    }

    #[inline(always)] pub(super) fn mark(&mut self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get_mut(block_index)
            .is_some_and(|block| block.is_occupied(record_index) && block.mark(record_index))
    }

    /// Whether record `id` is aged: a newborn a minor collection under
    /// `PromotionAge::First` names (`age_newborns`), or a candidate counts unmarked
    /// (`rc_unmark`).
    #[inline(always)]
    pub(super) fn is_aged(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks[block_index].aged[record_index / OCCUPANCY_WORD_BITS] & (1_u64 << (record_index % OCCUPANCY_WORD_BITS)) != 0
    }

    pub(super) fn is_marked(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)
            .is_some_and(|block| block.is_occupied(record_index) && block.is_marked(record_index))
    }

    /// This space's records and mark words as the parallel full mark shares them
    /// (`ParallelRecordMarks`). The exclusive borrow is what lets workers set mark bits
    /// through it: nothing else reads or writes the space until the view's last use.
    pub(super) fn parallel_marks(&mut self) -> ParallelRecordMarks<'_, T> {
        ParallelRecordMarks {
            blocks: self.blocks.as_mut_ptr(),
            block_count: self.blocks.len(),
            records: self.base.as_ptr().cast::<T>().cast_const(),
            _space: std::marker::PhantomData,
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn snapshot_marks(&self) -> Vec<u64> {
        self.blocks
            .iter()
            .flat_map(|block| block.marked.iter().copied())
            .collect()
    }

    /// Visits every old (marked) record.
    #[cfg(debug_assertions)]
    pub(super) fn for_each_marked(&self, mut visit: impl FnMut(RecordId)) {
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.marked.iter().copied().enumerate() {
                let mut marked = word;
                while marked != 0 {
                    let bit = marked.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    visit((block_index * RECORDS_PER_BLOCK + record_index) as RecordId);
                    marked &= marked - 1;
                }
            }
        }
    }

    /// Every record becomes young: a full collection's starting state.
    pub(super) fn clear_marks(&mut self) {
        for block in &mut self.blocks {
            block.marked.fill(0);
        }
        self.remembered.clear();
    }

    #[cfg(debug_assertions)]
    pub(super) fn restore_marks(&mut self, marks: &[u64]) {
        let expected_len = self.blocks.len() * BLOCK_WORDS;
        assert_eq!(marks.len(), expected_len);
        for (block, block_marks) in self
            .blocks
            .iter_mut()
            .zip(marks.as_chunks::<BLOCK_WORDS>().0)
        {
            block.marked.copy_from_slice(block_marks);
        }
    }

    pub(super) fn live_count(&self) -> usize {
        self.live
    }

    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn high_water(&self) -> usize {
        self.next_id as usize
    }

    /// The first id of each word young records can occupy: the words cursor runs were taken
    /// from since the last sweep and the words of the fresh ids from `young_floor` up. A word may
    /// be listed twice (a run can start in the fresh ids' first word).
    fn young_word_bases(
        young_words: &[RecordId],
        young_floor: RecordId,
        next_id: RecordId,
    ) -> impl Iterator<Item = RecordId> + '_ {
        let word_bits = OCCUPANCY_WORD_BITS as RecordId;
        let fresh_words = (young_floor / word_bits)..next_id.div_ceil(word_bits);
        young_words
            .iter()
            .copied()
            .chain(fresh_words.map(move |word| word * word_bits))
    }

    /// The block and in-block word index of the occupancy word starting at `base`.
    fn word_position(base: RecordId) -> (usize, usize) {
        let (block_index, record_index) = Self::split_id(base);
        (block_index, record_index / OCCUPANCY_WORD_BITS)
    }

    /// Names the records a minor collection under `age` finds newborn, before its mark sets
    /// their mark bits as it does the old records'. The barrier's `logged` records are old
    /// though unmarked, and are not newborn. Under `PromotionAge::Second` the sweep's words
    /// and their newborn records are listed (`minor_words`). Under `PromotionAge::First`
    /// every survivor becomes old, as an aged one does, so the newborn records are marked
    /// aged in place: the sweep then frees or promotes them alike, and shows its young
    /// survivors as the marked aged ones, with no list. Listing them cost acorn-wtb a
    /// quarter of a megabyte of max RSS, the capacity those lists kept.
    pub(super) fn prepare_minor(&mut self, logged: &[RecordId], age: PromotionAge) {
        debug_assert!(self.minor_words.is_empty());
        // The newborns are named from occupancy: an unallocated run id is not one.
        self.retire_cursor();
        if !age.ages() {
            return self.age_newborns(logged);
        }
        let mut bases: Vec<RecordId> =
            Self::young_word_bases(&self.young_words, self.young_floor, self.next_id).collect();
        bases.append(&mut self.aged_words);
        bases.sort_unstable();
        bases.dedup();
        // The stopped mutator's log names old records whose barrier cleared their mark.
        // Restore those bits only while deriving newborn words: direct bit addressing
        // excludes each old record in O(1), without searching the sparse word list.
        // No observer runs until the bits are cleared again below.
        self.set_logged_marks(logged, true);
        self.minor_words.extend(bases.into_iter().map(|base| {
            let (block_index, word_index) = Self::word_position(base);
            let block = &self.blocks[block_index];
            (base, block.occupied[word_index] & !block.marked[word_index] & !block.aged[word_index])
        }));
        self.set_logged_marks(logged, false);
    }

    /// `prepare_minor` under `PromotionAge::First`: every unmarked young record is marked aged
    /// in place, except the logged old ones.
    fn age_newborns(&mut self, logged: &[RecordId]) {
        for base in Self::young_word_bases(&self.young_words, self.young_floor, self.next_id) {
            let (block_index, word_index) = Self::word_position(base);
            let block = &mut self.blocks[block_index];
            block.aged[word_index] |= block.occupied[word_index] & !block.marked[word_index];
        }
        for &id in logged {
            let (block_index, word_index) = Self::word_position(id);
            self.blocks[block_index].aged[word_index] &= !(1_u64 << (id as usize % OCCUPANCY_WORD_BITS));
        }
    }

    /// Sets (`marked`) or clears each logged record's mark bit, which the barrier cleared.
    fn set_logged_marks(&mut self, logged: &[RecordId], marked: bool) {
        for &id in logged {
            let (block_index, word_index) = Self::word_position(id);
            let mask = 1_u64 << (id as usize % OCCUPANCY_WORD_BITS);
            let word = &mut self.blocks[block_index].marked[word_index];
            debug_assert!(!marked || *word & mask == 0, "the barrier unmarked each logged record");
            if marked { *word |= mask } else { *word &= !mask }
        }
    }

    /// Every aged record becomes old, as a full collection leaves every survivor.
    pub(super) fn forget_ages(&mut self) {
        for base in self.aged_words.drain(..) {
            let (block_index, word_index) = Self::word_position(base);
            self.blocks[block_index].aged[word_index] = 0;
        }
        self.minor_words.clear();
        self.promoted.clear();
    }

    /// Frees every occupied, unmarked record through `visit`. Marked records survive and keep
    /// their mark bit, which makes them old for the next minor collection; they reach `visit`
    /// only when the sweep asks for them (`visit_live`): every one in a full sweep, the young
    /// ones (newborn or aged, `prepare_minor`) in a minor one. A full sweep visits every word; a
    /// minor one only the words young records can occupy (`young_word_bases`) and the aged
    /// ones, so it pays for its allocation, not for the old records it keeps. Under `age`
    /// `PromotionAge::Second` a minor sweep ages its newborn survivors instead and promotes its
    /// aged ones (`sweep_aging_word`). Every minor sweep frees or promotes the aged records
    /// it finds, whatever its age, so the age may change between collections.
    pub(super) fn sweep(
        &mut self,
        full: bool,
        age: PromotionAge,
        visit_live: bool,
        visit: impl FnMut(RecordSweep<'_, T>),
    ) -> (usize, usize) {
        self.sweep_disposing(full, age, visit_live, |_, _| true, visit)
    }

    /// `sweep`, which first hands each dead record in place to `dispose`, which frees what it
    /// owns and answers whether anything is left for its drop: a record it answers `false`
    /// for is forgotten, neither moved out of the space nor dropped, nor shown to `visit`.
    pub(super) fn sweep_disposing(
        &mut self,
        full: bool,
        age: PromotionAge,
        visit_live: bool,
        dispose: impl FnMut(RecordId, &T) -> bool,
        mut visit: impl FnMut(RecordSweep<'_, T>),
    ) -> (usize, usize) {
        // The sweep frees each occupied, unmarked record: an unallocated run id is not a record.
        self.retire_cursor();
        let mut visit = Sweeper { dispose, visit: &mut visit };
        let mut freed_count = 0;
        let live_count = if full {
            debug_assert!(self.aged_words.is_empty(), "a full collection forgets ages first");
            let mut live_count = 0;
            for block_index in 0..self.blocks.len() {
                for word_index in 0..BLOCK_WORDS {
                    self.warm_ahead(block_index, word_index);
                    let (live, freed) = self.sweep_word(block_index, word_index, if visit_live { u64::MAX } else { 0 }, &mut visit);
                    (live_count, freed_count) = (live_count + live, freed_count + freed);
                }
            }
            live_count
        } else if age.ages() {
            let words = std::mem::take(&mut self.minor_words);
            for &(base, newborn) in &words {
                freed_count += self.sweep_aging_word(base, newborn, visit_live, &mut visit);
            }
            self.minor_words = words;
            self.minor_words.clear();
            self.live - freed_count
        } else {
            // Newborn records are aged here (`prepare_minor`): the young survivors are the
            // marked aged ones, and the aged records the sweep kept are marked, so old.
            let young_words = std::mem::take(&mut self.young_words);
            let mut aged_words = std::mem::take(&mut self.aged_words);
            for base in Self::young_word_bases(&young_words, self.young_floor, self.next_id)
                .chain(aged_words.drain(..))
            {
                let (block_index, word_index) = Self::word_position(base);
                let young = if visit_live { self.blocks[block_index].aged[word_index] } else { 0 };
                freed_count += self.sweep_word(block_index, word_index, young, &mut visit).1;
                self.blocks[block_index].aged[word_index] = 0;
            }
            self.young_words = young_words;
            self.aged_words = aged_words;
            self.live - freed_count
        };
        self.live = live_count;
        self.young_floor = self.next_id;
        self.young_words.clear();
        #[cfg(debug_assertions)]
        self.debug_assert_swept();
        (live_count, freed_count)
    }

    /// Sweeps one occupancy word: frees its occupied, unmarked records and shows `visit` its
    /// marked ones among `shown`. Returns the live and freed counts it saw.
    #[inline(always)]
    fn sweep_word(
        &mut self,
        block_index: usize,
        word_index: usize,
        shown: u64,
        visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>,
    ) -> (usize, usize) {
        let block = &mut self.blocks[block_index];
        let occupied = block.occupied[word_index];
        let marked = block.marked[word_index];
        debug_assert_eq!(marked & !occupied, 0);
        let dead = occupied & !marked;
        let id_base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
        let mut live = marked & shown;
        while live != 0 {
            let id = id_base + live.trailing_zeros();
            // SAFETY: a mark bit is set only for an occupied record.
            (visit.visit)(RecordSweep::Keep(id, unsafe {
                (*self.base.as_ptr().add(id as usize)).assume_init_ref()
            }));
            live &= live - 1;
        }
        block.occupied[word_index] = marked;
        self.push_free_bits(id_base, dead);
        if self.rc {
            self.rc_keep(block_index, word_index, marked);
        }
        (marked.count_ones() as usize, self.release(id_base, dead, visit))
    }

    /// Sweeps one word a minor collection under `PromotionAge::Second` visits, whose `newborn`
    /// records `prepare_minor` named: the newborn and aged records the mark left unmarked die,
    /// the marked aged ones are promoted (kept marked, and listed in `promoted`), and the
    /// marked newborn ones stay young, aged and unmarked. Old records are left as they are.
    /// Not idempotent: `prepare_minor` lists each word once. Returns the freed count. Out of
    /// line, so the sweep's visitor keeps the two call sites of `sweep_word` it is inlined at.
    #[inline(never)]
    fn sweep_aging_word(
        &mut self,
        base: RecordId,
        newborn: u64,
        visit_live: bool,
        visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>,
    ) -> usize {
        let (block_index, word_index) = Self::word_position(base);
        let block = &mut self.blocks[block_index];
        let marked = block.marked[word_index];
        let aged = block.aged[word_index];
        debug_assert_eq!(newborn & aged, 0);
        let dead = (newborn | aged) & !marked;
        let mut promoted = aged & marked;
        self.aged_fates.add(AgedFates {
            died: (aged & !marked).count_ones() as usize,
            promoted: promoted.count_ones() as usize,
        });
        let stays = newborn & marked;
        block.aged[word_index] = stays;
        block.marked[word_index] = marked & !stays;
        block.occupied[word_index] &= !dead;
        self.push_free_bits(base, dead);
        if self.rc {
            self.rc_keep(block_index, word_index, !dead);
        }
        if stays != 0 {
            self.aged_words.push(base);
        }
        let mut kept = if visit_live { stays | promoted } else { 0 };
        while promoted != 0 {
            self.promoted.push(base + promoted.trailing_zeros());
            promoted &= promoted - 1;
        }
        while kept != 0 {
            let id = base + kept.trailing_zeros();
            // SAFETY: a kept record is marked or aged, so occupied.
            (visit.visit)(RecordSweep::Keep(id, unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() }));
            kept &= kept - 1;
        }
        self.release(base, dead, visit)
    }

    /// Hands `visit` the `dead` records of the word starting at `id_base`, whose occupancy
    /// bits the caller cleared, once its `dispose` leaves them something to drop; the others
    /// are forgotten. Returns how many there were.
    fn release(
        &mut self,
        id_base: RecordId,
        mut dead: u64,
        visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>,
    ) -> usize {
        let freed = dead.count_ones() as usize;
        while dead != 0 {
            let id = id_base + dead.trailing_zeros();
            // SAFETY: the occupied bit proved initialization, and clearing it transferred the
            // record to the sweep, which forgets one disposed of and moves any other out to
            // the drop callback.
            let record = unsafe { &mut *self.base.as_ptr().add(id as usize) };
            if (visit.dispose)(id, unsafe { record.assume_init_ref() }) {
                (visit.visit)(RecordSweep::Dead(id, unsafe { record.assume_init_read() }));
            }
            dead &= dead - 1;
        }
        freed
    }

    pub(super) fn for_each_live(&self, mut visit: impl FnMut(RecordId, &T)) {
        let cursor = self.cursor;
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    if !Self::in_unallocated_run(cursor, id) {
                        // SAFETY: every occupied bit outside the cursor's unallocated run names an initialized record.
                        visit(id, unsafe {
                            (*self.base.as_ptr().add(id as usize)).assume_init_ref()
                        });
                    }
                    live &= live - 1;
                }
            }
        }
    }

    pub(super) fn for_each_live_mut(&mut self, mut visit: impl FnMut(RecordId, &mut T)) {
        let base = self.base;
        let cursor = self.cursor;
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    if !Self::in_unallocated_run(cursor, id) {
                        // SAFETY: each occupied ID is visited once and `&mut self`
                        // excludes every other reference into this space.
                        visit(id, unsafe {
                            (*base.as_ptr().add(id as usize)).assume_init_mut()
                        });
                    }
                    live &= live - 1;
                }
            }
        }
    }

    #[inline(always)]
    fn is_occupied(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)
            .is_some_and(|block| block.is_occupied(record_index))
    }

    #[inline(always)]
    pub(super) fn is_live(&self, id: RecordId) -> bool {
        id < self.next_id && self.is_occupied(id)
    }

    #[inline(always)]
    fn split_id(id: RecordId) -> (usize, usize) {
        let id = id as usize;
        (id / RECORDS_PER_BLOCK, id % RECORDS_PER_BLOCK)
    }

    /// The bytes this space holds committed: every block's metadata, and the records of the
    /// blocks whose pages are still resident (`dirty`). `return_free_pages` gives a block's
    /// pages back and clears its `dirty` bit, so the ledger falls with them: the block count
    /// is a high-water mark that a full collection never lowers.
    pub(super) fn allocated_bytes(&self) -> usize {
        let resident_blocks = self.dirty.iter().filter(|&&dirty| dirty).count();
        self.blocks.len() * size_of::<RecordBlock>() + resident_blocks * RECORDS_PER_BLOCK * size_of::<T>()
    }

    /// Keeps the freed `id` from reuse until `release_withheld`, so a Value naming the
    /// record it held names no other record meanwhile; how many ids are withheld.
    pub(super) fn withhold(&mut self, id: RecordId) -> usize {
        let (block_index, record_index) = Self::split_id(id);
        let Some(block) = self.blocks.get_mut(block_index) else {
            return self.withheld;
        };
        debug_assert!(!block.is_occupied(record_index), "only a freed id is withheld");
        let word_index = record_index / OCCUPANCY_WORD_BITS;
        let mask = 1_u64 << (record_index % OCCUPANCY_WORD_BITS);
        if block.withheld[word_index] & mask == 0 {
            block.withheld[word_index] |= mask;
            self.withheld += 1;
        }
        self.withheld
    }

    /// Makes every withheld id reusable again, scanning down from the high water.
    pub(super) fn release_withheld(&mut self) {
        if self.withheld == 0 {
            return;
        }
        for (block_index, block) in self.blocks.iter_mut().enumerate() {
            let hole = Self::in_hole((block_index * RECORDS_PER_BLOCK) as RecordId);
            block.withheld = [if hole { u64::MAX } else { 0 }; BLOCK_WORDS];
        }
        self.withheld = 0;
        self.rebuild_free_runs();
    }

    /// After a full collection's sweep: gives the pages of the empty record blocks back to the
    /// kernel, and lowers the high water mark past the empty top blocks, so allocation reuses the
    /// low ids first. Returns the bytes returned.
    ///
    /// The block metadata stays where it is (`blocks` never shrinks, and the native mark pointers
    /// name it); only the pages of the records move. A block is returned when it is `returnable`, and the `RESERVE_MIN_BLOCKS` (or a quarter of the live
    /// blocks, if more) nearest the high water mark on each side stay resident: the ids that
    /// allocation reaches first. The high water mark falls to the end of the last block holding
    /// an occupied or withheld id; a cursor run is occupied, so no returned page holds one.
    ///
    /// A returned page reads back as zero. That is no record: a record is read only where its
    /// occupancy bit is set, and the sweep, the marks and the drop all read occupancy first.
    pub(super) fn return_free_pages(&mut self) -> usize {
        if !self.is_reserved() || self.cursor.next < self.cursor.limit {
            return 0;
        }
        let end_block = self.blocks.iter().rposition(RecordBlock::in_use).map_or(0, |index| index + 1);
        let lowered = (end_block * RECORDS_PER_BLOCK).min(self.next_id as usize) as RecordId;
        self.next_id = lowered;
        // The ids from `lowered` up are unreached now: no run names one.
        self.free_runs.retain_mut(|run| {
            run.1 = run.1.min(lowered);
            run.0 < run.1
        });
        // The sweep set `young_floor` to the old high water mark; the ids between are free now.
        self.young_floor = lowered;
        let pivot = self.next_id as usize / RECORDS_PER_BLOCK;
        let reserve = (self.live.div_ceil(RECORDS_PER_BLOCK) / 4).max(RESERVE_MIN_BLOCKS);
        let mut released = Vec::new();
        let mut kept = 0;
        for index in pivot..self.blocks.len() {
            if self.returnable(index) {
                if kept < reserve {
                    kept += 1;
                } else {
                    self.dirty[index] = false;
                    released.push(index);
                }
            }
        }
        kept = 0;
        for index in (0..pivot.min(self.blocks.len())).rev() {
            if self.returnable(index) {
                if kept < reserve {
                    kept += 1;
                } else {
                    self.dirty[index] = false;
                    released.push(index);
                }
            }
        }
        released.sort_unstable();
        let block_bytes = RECORDS_PER_BLOCK * size_of::<MaybeUninit<T>>();
        let mapped_bytes = self.reserved * size_of::<MaybeUninit<T>>();
        let page = page_bytes();
        let mut returned = 0;
        // Adjacent blocks form one run, returned by one call: whole pages inside the run, so a
        // page that also holds a block not returned stays resident.
        for run in released.chunk_by(|&first, &next| first + 1 == next) {
            let start = (run[0] * block_bytes).next_multiple_of(page);
            let end = ((run[run.len() - 1] + 1) * block_bytes).min(mapped_bytes);
            let end = end - end % page;
            if end > start {
                // SAFETY: `start` and the length are whole pages inside this space's mapping,
                // and the blocks named hold no occupied or withheld id, so no live record.
                unsafe { return_pages(self.base.as_ptr().cast::<u8>().add(start), end - start) };
                returned += end - start;
            }
        }
        returned
    }

    pub(super) fn reusable_ids(&self) -> usize {
        self.next_id as usize - self.live
    }

    pub(super) fn free_run_count(&self) -> usize {
        self.free_runs.len()
    }
}

#[cfg(test)]
mod tests;

/// Reference counts (`super::rc`).
impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// Turns counting on or off; off forgets the epoch's buffers.
    pub(super) fn set_rc(&mut self, on: bool) {
        self.rc = on;
        self.rc_decrements.clear();
        self.rc_logged.clear();
        if on {
            self.rc_ensure();
        } else {
            self.rc_blocks = Vec::new();
        }
    }

    /// Gives every record block its counts, zero for the blocks made since the last call: the
    /// collector calls it before it reads or changes counts.
    pub(super) fn rc_ensure(&mut self) {
        while self.rc_blocks.len() < self.blocks.len() {
            self.rc_blocks.push(RcBlock::new());
        }
    }

    /// The count of live record `id`: 0..=3, 3 stuck.
    #[inline(always)]
    pub(super) fn rc_count(&self, id: RecordId) -> u8 {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, bit) = (record_index / OCCUPANCY_WORD_BITS, record_index % OCCUPANCY_WORD_BITS);
        let lo = (block.lo[word].load(Ordering::Relaxed) >> bit) & 1;
        let hi = (block.hi[word].load(Ordering::Relaxed) >> bit) & 1;
        (hi << 1 | lo) as u8
    }

    /// The exact count of `id`, with its overflow entry; `u32::MAX` when stuck.
    #[cfg(test)]
    pub(super) fn rc_exact_count(&self, id: RecordId) -> u32 {
        if self.rc_stuck(id) {
            return u32::MAX;
        }
        let count = self.rc_count(id) as u32;
        let overflow = self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner());
        count + if count == RC_STUCK as u32 { overflow.get(&id).copied().unwrap_or(0) } else { 0 }
    }

    /// Adds a reference to live record `id`; past `RC_STUCK`, in the overflow table. The
    /// count's low bit is set first: 0 -> 1 and 2 -> 3 set it, 1 -> 2 carries into the high bit.
    #[inline(always)]
    pub(super) fn rc_increment(&self, id: RecordId) {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        let lo = block.lo[word].load(Ordering::Relaxed);
        if lo & mask == 0 {
            block.lo[word].store(lo | mask, Ordering::Relaxed);
            return;
        }
        let hi = block.hi[word].load(Ordering::Relaxed);
        if hi & mask == 0 {
            block.lo[word].store(lo & !mask, Ordering::Relaxed);
            block.hi[word].store(hi | mask, Ordering::Relaxed);
        } else if block.stuck[word].load(Ordering::Relaxed) & mask == 0 {
            self.rc_overflow_add(id);
        }
    }

    /// Makes `id`'s count stuck (`Heap::rc_pin`), dropping its overflow entry.
    pub(super) fn rc_pin(&self, id: RecordId) {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        if block.stuck[word].load(Ordering::Relaxed) & mask != 0 {
            return;
        }
        if block.lo[word].load(Ordering::Relaxed) & block.hi[word].load(Ordering::Relaxed) & mask != 0 {
            self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner()).remove(&id);
        }
        block.lo[word].fetch_or(mask, Ordering::Relaxed);
        block.hi[word].fetch_or(mask, Ordering::Relaxed);
        block.stuck[word].fetch_or(mask, Ordering::Relaxed);
    }

    /// Whether `id`'s count is stuck: past the overflow table's limit.
    #[cfg(test)]
    fn rc_stuck(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        let word = &self.rc_blocks[block_index].stuck[record_index / OCCUPANCY_WORD_BITS];
        word.load(Ordering::Relaxed) & (1_u64 << (record_index % OCCUPANCY_WORD_BITS)) != 0
    }

    #[cold]
    #[inline(never)]
    fn rc_overflow_add(&self, id: RecordId) {
        let mut overflow = self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner());
        let extra = overflow.entry(id).or_default();
        *extra += 1;
        if *extra > RC_OVERFLOW_LIMIT {
            overflow.remove(&id);
            let (block_index, record_index) = Self::split_id(id);
            let word = &self.rc_blocks[block_index].stuck[record_index / OCCUPANCY_WORD_BITS];
            word.fetch_or(1_u64 << (record_index % OCCUPANCY_WORD_BITS), Ordering::Relaxed);
        }
    }

    /// Drops a reference to live record `id`; whether its count reached 0. A count of 0 (an
    /// over-decrement, which a deletion the barrier missed cannot cause: those only
    /// over-count) stays 0.
    #[inline(always)]
    pub(super) fn rc_decrement(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        let lo = block.lo[word].load(Ordering::Relaxed);
        let hi = block.hi[word].load(Ordering::Relaxed);
        match (lo & mask != 0, hi & mask != 0) {
            (false, false) => false,
            (true, false) => {
                block.lo[word].store(lo & !mask, Ordering::Relaxed);
                true
            }
            (false, true) => {
                block.lo[word].store(lo | mask, Ordering::Relaxed);
                block.hi[word].store(hi & !mask, Ordering::Relaxed);
                false
            }
            (true, true) => {
                if block.stuck[word].load(Ordering::Relaxed) & mask == 0 && !self.rc_overflow_take(id) {
                    block.lo[word].store(lo & !mask, Ordering::Relaxed);
                }
                false
            }
        }
    }

    /// Takes one reference from `id`'s overflow entry; `false` when it has none.
    #[cold]
    #[inline(never)]
    fn rc_overflow_take(&self, id: RecordId) -> bool {
        let mut overflow = self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner());
        match overflow.get_mut(&id) {
            Some(extra) => {
                *extra -= 1;
                if *extra == 0 {
                    overflow.remove(&id);
                }
                true
            }
            None => false,
        }
    }

    /// Makes old record `id` a candidate for freeing: its mark bit is cleared, so a minor
    /// collection's mark re-marks it if anything it traces names it, and its word goes to the
    /// minor sweep's list (`aged_words`), which frees it otherwise. `false` when it is not
    /// marked: logged by the barrier this epoch, young, or a candidate already.
    pub(super) fn rc_unmark(&mut self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        let (word_index, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        let word = &mut self.blocks[block_index].marked[word_index];
        if *word & mask == 0 {
            return false;
        }
        *word &= !mask;
        // Aged, as a newborn is: if the mark re-marks it, it counts its children there. A
        // word aged already is listed already, among the young words or `aged_words`.
        let aged = &mut self.blocks[block_index].aged[word_index];
        if *aged == 0 {
            self.aged_words.push(id - (record_index % OCCUPANCY_WORD_BITS) as RecordId);
        }
        *aged |= mask;
        true
    }

    /// Clears the counts of the ids of a word that `kept` leaves out, as the sweep frees
    /// them, with their overflow entries.
    #[inline(never)]
    fn rc_keep(&mut self, block_index: usize, word_index: usize, kept: u64) {
        let Some(counts) = self.rc_blocks.get_mut(block_index) else { return };
        let mut overflowed = *counts.lo[word_index].get_mut() & *counts.hi[word_index].get_mut()
            & !*counts.stuck[word_index].get_mut() & !kept;
        counts.keep(word_index, kept);
        if overflowed != 0 {
            let overflow = self.rc_overflow.get_mut().unwrap_or_else(|poison| poison.into_inner());
            if !overflow.is_empty() {
                let base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
                while overflowed != 0 {
                    overflow.remove(&(base + overflowed.trailing_zeros()));
                    overflowed &= overflowed - 1;
                }
            }
        }
    }

    /// Every count becomes 0, as a recount starts.
    pub(super) fn rc_clear(&mut self) {
        self.rc_overflow.get_mut().unwrap_or_else(|poison| poison.into_inner()).clear();
        self.rc_ensure();
        for block in &mut self.rc_blocks {
            for word in 0..BLOCK_WORDS {
                block.keep(word, 0);
            }
        }
    }

    /// The records old as a full collection starts, one bit per id, `BLOCK_WORDS` words per
    /// block: the marked ones and the barrier's `logged` (old, though it unmarked them).
    pub(super) fn rc_old_bits(&self, logged: &[RecordId]) -> Vec<u64> {
        let mut bits = Vec::with_capacity(self.blocks.len() * BLOCK_WORDS);
        for block in &self.blocks {
            bits.extend_from_slice(&block.marked);
        }
        for &id in logged {
            let (block_index, record_index) = Self::split_id(id);
            bits[block_index * BLOCK_WORDS + record_index / OCCUPANCY_WORD_BITS] |= 1 << (record_index % OCCUPANCY_WORD_BITS);
        }
        bits
    }

    /// After a full collection's mark, before its sweep: shows `visit` each record old at its
    /// start (`old`, from `rc_old_bits`) the mark left (`false`), and each record young then
    /// the mark kept (`true`).
    pub(super) fn rc_full_changes(&self, old: &[u64], mut visit: impl FnMut(RecordId, bool)) {
        for (block_index, block) in self.blocks.iter().enumerate() {
            for word_index in 0..BLOCK_WORDS {
                let was = old.get(block_index * BLOCK_WORDS + word_index).copied().unwrap_or(0);
                let (occupied, marked) = (block.occupied[word_index], block.marked[word_index]);
                let base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
                let mut changed = occupied & (was ^ marked);
                while changed != 0 {
                    let bit = changed.trailing_zeros();
                    visit(base + bit, marked & (1 << bit) != 0);
                    changed &= changed - 1;
                }
            }
        }
    }

    /// The children the barrier copied and the records it logged this epoch, emptied.
    pub(super) fn take_rc_epoch(&mut self) -> (Vec<Value>, Vec<RecordId>) {
        self.rc_ensure();
        (std::mem::take(&mut self.rc_decrements), std::mem::take(&mut self.rc_logged))
    }

    /// Under `PromotionAge::First`, after a minor collection's mark and before its sweep: the
    /// young records the mark reached (the marked aged ones of the young words), which the
    /// sweep makes old.
    pub(super) fn for_each_young_survivor(&self, mut visit: impl FnMut(RecordId)) {
        self.for_each_young_survivor_word(|base, _block_index, _word_index, mut survivors| {
            while survivors != 0 {
                visit(base + survivors.trailing_zeros());
                survivors &= survivors - 1;
            }
        });
    }

    /// `for_each_young_survivor` a word at a time: each word's first id, block and word
    /// position, and its survivors' bits.
    fn for_each_young_survivor_word(&self, mut visit: impl FnMut(RecordId, usize, usize, u64)) {
        // A word reloaded for allocation is listed once per load; a candidate's word is in
        // `aged_words`.
        let mut bases: Vec<RecordId> = Self::young_word_bases(&self.young_words, self.young_floor, self.next_id).collect();
        bases.extend_from_slice(&self.aged_words);
        bases.sort_unstable();
        bases.dedup();
        for base in bases {
            let (block_index, word_index) = Self::word_position(base);
            let block = &self.blocks[block_index];
            let survivors = block.aged[word_index] & block.marked[word_index];
            if survivors != 0 {
                visit(base, block_index, word_index, survivors);
            }
        }
    }

    /// The young survivors (`for_each_young_survivor`) whose count is 0, a word of counts at
    /// a time; the counts cover every block (`rc_ensure`).
    pub(super) fn rc_zero_survivors(&self, mut visit: impl FnMut(RecordId)) {
        self.for_each_young_survivor_word(|base, block_index, word_index, survivors| {
            let counts = &self.rc_blocks[block_index];
            let mut zero = survivors
                & !(counts.lo[word_index].load(Ordering::Relaxed) | counts.hi[word_index].load(Ordering::Relaxed));
            while zero != 0 {
                visit(base + zero.trailing_zeros());
                zero &= zero - 1;
            }
        });
    }
}

/// The largest count the bits hold: references past it are counted in the overflow table.
#[cfg(test)]
pub(super) const RC_STUCK: u8 = 3;
/// The references past `RC_STUCK` the overflow table counts before the record's count sticks.
const RC_OVERFLOW_LIMIT: u32 = 64;
