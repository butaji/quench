use super::{ArrayObject, AwaitState, FunctionRecord, IteratorRecord, Object, PromiseCell};
use super::pacing::{AgedFates, PromotionAge};

#[cfg(all(test, feature = "bench-primitives"))]
mod primitive_bench;
#[cfg(test)]
mod vacancy_tests;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod prefetch;
mod reservation;
pub(super) mod sweep_support;
mod family;
mod marks;
mod reference_counts;
mod sweep;
pub(super) use family::RecordFamily;
pub(super) use marks::{ConcurrentRecordMarks, ParallelRecordMarks};
pub(super) use sweep::RecordSweep;
use reservation::Reservation;
use crate::vm::wtf16::JsString;
use crate::value::Value;
use std::mem::MaybeUninit;
#[cfg(feature = "profile-memory")]
use std::mem::size_of;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) type RecordId = u32;

/// What a record keeps alive, copied out for the snapshot barrier (`RecordSpace::satb_note`):
/// the values its trace would push, read before the mutator's first write in a concurrent
/// cycle changes them. Shape roots are left out: a cycle pushes them per shape when it starts.
pub(super) trait SatbSnapshot {
    fn snapshot(&self, properties: &crate::value_vec::ValueArena, out: &mut Vec<Value>);
}

pub(super) type ObjectRecordSpace = RecordSpace<Object>;
pub(super) type ArrayRecordSpace = RecordSpace<ArrayObject>;
pub(super) type FunctionRecordSpace = RecordSpace<FunctionRecord>;
pub(super) type PromiseRecordSpace = RecordSpace<PromiseCell>;
pub(super) type AwaitRecordSpace = RecordSpace<AwaitState>;
pub(super) type IteratorRecordSpace = RecordSpace<IteratorRecord>;
/// Wasm vectors: immutable leaves, so a record's bytes are its whole state.
pub(super) type V128RecordSpace = RecordSpace<[u8; crate::wasm::V128_BYTES]>;
/// Strings have no legacy-slot fallback, so their space holds as many records as the legacy
/// slots do (`MAX_HEAP_CELLS`).
/// The ids `[STRING_RECORD_HOLE, 2 * STRING_RECORD_HOLE)` are never this space's: they name the
/// records of small strings (`super::small_strings`), which stay below the narrow index limit.
pub(super) type StringRecordSpace = RecordSpace<JsString, STRING_RECORD_CAPACITY, STRING_RECORD_HOLE>;
pub(super) const STRING_RECORD_CAPACITY: usize = super::MAX_HEAP_CELLS;
pub(super) const STRING_RECORD_HOLE: usize = 1 << 24;
const _: () = assert!(STRING_RECORD_HOLE.is_multiple_of(RECORDS_PER_BLOCK) && 2 * STRING_RECORD_HOLE <= STRING_RECORD_CAPACITY);
const _: () = assert!(STRING_RECORD_CAPACITY.is_power_of_two());
const _: () = assert!(STRING_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(STRING_RECORD_CAPACITY <= u32::MAX as usize);

/// The ids a hole keeps out of use end at this multiple of its start: `[HOLE, HOLE_END_MULTIPLE * HOLE)`.
const HOLE_END_MULTIPLE: usize = 2;
const RECORDS_PER_BLOCK: usize = 1_024;
const OCCUPANCY_WORD_BITS: usize = u64::BITS as usize;
const BLOCK_WORDS: usize = RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS;
pub(super) const RECORD_SPACE_CAPACITY: usize = 1 << 24;

const _: () = assert!(RECORDS_PER_BLOCK.is_power_of_two());
const _: () = assert!(OCCUPANCY_WORD_BITS.is_power_of_two());
const _: () = assert!(RECORD_SPACE_CAPACITY.is_power_of_two());
const _: () = assert!(RECORD_SPACE_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(RECORD_SPACE_CAPACITY <= u32::MAX as usize);

/// Each record family owns one fixed address range. Block metadata controls
/// allocation and sweep, but resolving a live locator never walks it. A nonzero `HOLE` keeps
/// the ids `[HOLE, 2 * HOLE)` out of use: their blocks exist, with every id withheld.
pub(super) struct RecordSpace<T, const CAPACITY: usize = RECORD_SPACE_CAPACITY, const HOLE: usize = 0> {
    pub(super) base: NonNull<MaybeUninit<T>>,
    reservation: Reservation,
    blocks: Vec<RecordBlock>,
    next_id: u32,
    /// Where the downward scan for reusable IDs continues; 0 once none remain below it.
    free_cursor: RecordId,
    /// The reusable IDs of the occupancy word the scan last reached, starting at
    /// `free_word_base`; allocation takes them highest first before scanning further.
    pub(super) free_word: u64,
    pub(super) free_word_base: RecordId,
    /// The occupancy word `free_word` was loaded from: a claim sets its bit through this
    /// pointer, with no block or word index. Valid while `free_word` is nonzero, as blocks are
    /// added only once no free word is left (`claim_slow`), and set by `load_free_word` alone.
    pub(super) free_word_occupied: *mut u64,
    pub(super) live: usize,
    /// Old records written since the last collection, each logged once by the write barrier.
    remembered: Vec<RecordId>,
    /// Where young records can be. Every sweep frees each occupied, unmarked record it
    /// visits, so between collections only the ids taken since the last sweep are occupied
    /// and unmarked: fresh ids from `young_floor` up, and the free words loaded since. A
    /// minor sweep visits those words alone, at a cost that follows allocation, not heap size.
    young_floor: RecordId,
    /// The first id of each occupancy word `load_free_word` loaded since the last sweep.
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
    /// Whether a concurrent cycle is marking (`Heap::satb_active`): the write barrier then
    /// snapshots a record's children before its first write instead of logging it for the
    /// next minor collection, and the mark bits belong to the cycle (`satb_note`).
    satb: bool,
    /// The first id the cycle's allocation hands out fresh: records from here up are newer
    /// than the snapshot, so the barrier skips them.
    satb_frontier: RecordId,
    /// The children the barrier snapshotted, for the cycle to mark.
    satb_values: Vec<Value>,
    /// The first id of each occupancy word whose free ids the cycle's allocation pre-marked
    /// (`satb_blacken_free_word`), which `end_satb` unmarks where no claim took them.
    satb_reused_words: Vec<RecordId>,
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
}

impl<T, const CAPACITY: usize, const HOLE: usize> Default for RecordSpace<T, CAPACITY, HOLE> {
    fn default() -> Self {
        Self {
            base: NonNull::dangling(),
            reservation: Reservation::Unreserved,
            blocks: Vec::new(),
            next_id: 0,
            free_cursor: 0,
            free_word: 0,
            free_word_base: 0,
            free_word_occupied: std::ptr::null_mut(),
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
            satb: false,
            satb_frontier: 0,
            satb_values: Vec::new(),
            satb_reused_words: Vec::new(),
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
    #[inline]
    pub(super) fn base_ptr(&self) -> Option<*const T> {
        self.is_reserved()
            .then(|| self.base.as_ptr().cast::<T>().cast_const())
    }
}

// Native JIT/AOT guards read `marked` using offsets exported from this layout.
#[repr(C)]
struct RecordBlock {
    occupied: [u64; BLOCK_WORDS],
    marked: [u64; BLOCK_WORDS],
    /// Young records that survived one minor collection (`PromotionAge::Second`): unmarked,
    /// swept by the next minor collection, which frees or promotes them.
    aged: [u64; BLOCK_WORDS],
    /// Freed ids no allocation takes until `RecordSpace::release_withheld`
    /// (`RecordSpace::withhold`).
    withheld: [u64; BLOCK_WORDS],
}

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

impl RecordBlock {
    fn new() -> Self {
        Self {
            occupied: [0; BLOCK_WORDS],
            withheld: [0; BLOCK_WORDS],
            marked: [0; BLOCK_WORDS],
            aged: [0; BLOCK_WORDS],
        }
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

    #[cfg(test)]
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
        // The common allocation, inline: an id from the loaded free word, else the next fresh id
        // in a block that exists; `claim_slow` names no value (no address of the caller's stack).
        let id = if self.free_word != 0 {
            self.take_free_word_id()
        } else if self.free_cursor == 0
            && self.is_reserved()
            && (self.next_id as usize) < CAPACITY
            && Self::split_id(self.next_id).0 < self.blocks.len()
        {
            let id = self.next_id;
            let (block_index, record_index) = Self::split_id(id);
            if !self.blocks[block_index].claim(record_index) {
                return None;
            }
            self.next_id += 1;
            id
        } else {
            self.claim_slow()?
        };
        self.live += 1;
        Some(id)
    }

    /// `claim_uninit` without its slow path: `None`, with nothing claimed, wherever that would run.
    #[inline(always)]
    pub(super) fn claim_uninit_quick(&mut self) -> Option<RecordId> {
        let id = if self.free_word != 0 {
            self.take_free_word_id()
        } else if self.free_cursor == 0
            && self.is_reserved()
            && (self.next_id as usize) < CAPACITY
            && Self::split_id(self.next_id).0 < self.blocks.len()
        {
            let id = self.next_id;
            let (block_index, record_index) = Self::split_id(id);
            if !self.blocks[block_index].claim(record_index) {
                return None;
            }
            self.next_id += 1;
            id
        } else {
            return None;
        };
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

    /// Claims the highest id of the loaded free word, whose bits name unoccupied ids of one
    /// occupancy word, with no test of the id it already proved free; hints the next one.
    #[inline(always)]
    fn take_free_word_id(&mut self) -> RecordId {
        debug_assert_ne!(self.free_word, 0);
        let bit = OCCUPANCY_WORD_BITS - 1 - self.free_word.leading_zeros() as usize;
        let mask = 1_u64 << bit;
        self.free_word &= !mask;
        prefetch::next_free_record(self.base.as_ptr(), self.free_word_base, self.free_word);
        // SAFETY: `free_word_occupied` names the occupancy word the nonzero free word came from,
        // in a block that has not moved since (see the field).
        let word = unsafe { &mut *self.free_word_occupied };
        debug_assert_eq!(*word & mask, 0, "a free-word bit names an unoccupied id");
        *word |= mask;
        self.free_word_base + bit as RecordId
    }

    /// `try_insert`'s claim of an unoccupied id when it reserves the range, adds a block or
    /// loads a free word; `None` when no id is free.
    #[inline(never)]
    fn claim_slow(&mut self) -> Option<RecordId> {
        if !self.ensure_reserved() {
            return None;
        }
        if self.free_word == 0 && self.free_cursor != 0 {
            self.load_free_word();
        }
        if self.free_word != 0 {
            return Some(self.take_free_word_id());
        }
        if HOLE != 0 && self.next_id as usize == HOLE {
            self.next_id = (HOLE_END_MULTIPLE * HOLE) as RecordId;
        }
        let id = self.next_id;
        if id as usize >= CAPACITY {
            return None;
        }
        let (block_index, record_index) = Self::split_id(id);
        self.add_blocks_through(block_index);
        if !self.blocks[block_index].claim(record_index) {
            return None;
        }
        self.next_id += 1;
        Some(id)
    }

    /// Whether `id` lies in the ids `HOLE` keeps out of use.
    #[inline(always)]
    pub(super) const fn in_hole(id: RecordId) -> bool {
        HOLE != 0 && id as usize >= HOLE && (id as usize) < HOLE_END_MULTIPLE * HOLE
    }

    /// Adds blocks until `block_index` exists; a block of the hole withholds every id.
    fn add_blocks_through(&mut self, block_index: usize) {
        while self.blocks.len() <= block_index {
            let mut block = RecordBlock::new();
            if Self::in_hole((self.blocks.len() * RECORDS_PER_BLOCK) as RecordId) {
                block.withheld = [u64::MAX; BLOCK_WORDS];
            }
            self.blocks.push(block);
        }
    }

    /// Places `value` at `id`, as a snapshot restores it (`crate::snapshot`): ids arrive in
    /// increasing order, each marked (old), as after a full collection; the ids skipped are
    /// reusable once `finish_snapshot_inserts` runs. `false` when the id cannot be placed.
    pub(super) fn insert_snapshot(&mut self, id: RecordId, value: T) -> bool {
        if !self.ensure_reserved() || id as usize >= CAPACITY || id < self.next_id || Self::in_hole(id) {
            return false;
        }
        let (block_index, record_index) = Self::split_id(id);
        self.add_blocks_through(block_index);
        if !self.blocks[block_index].claim(record_index) {
            return false;
        }
        // SAFETY: the occupancy bit claims this previously uninitialized id.
        unsafe { self.base.as_ptr().add(id as usize).write(MaybeUninit::new(value)) };
        self.blocks[block_index].mark(record_index);
        self.next_id = id + 1;
        self.live += 1;
        true
    }

    /// Makes the ids `insert_snapshot` skipped reusable, scanning down from the high water.
    pub(super) fn finish_snapshot_inserts(&mut self) {
        self.free_cursor = self.next_id;
        self.free_word = 0;
        self.young_floor = self.next_id;
    }

    /// The first id never used: a snapshot restores the space up to it.
    pub(super) fn high_water_id(&self) -> RecordId {
        self.next_id
    }

    /// Loads the occupancy word holding the highest reusable ID below `free_cursor` and moves
    /// the cursor to that word's start, or to 0 when no reusable ID remains.
    #[inline(never)]
    fn load_free_word(&mut self) {
        let Some(id) = self.highest_free_before(self.free_cursor) else {
            self.free_cursor = 0;
            return;
        };
        let (block_index, record_index) = Self::split_id(id);
        let word_index = record_index / OCCUPANCY_WORD_BITS;
        let bit = record_index % OCCUPANCY_WORD_BITS;
        let below_and_at = u64::MAX >> (OCCUPANCY_WORD_BITS - 1 - bit);
        let block = &mut self.blocks[block_index];
        self.free_word = !(block.occupied[word_index] | block.withheld[word_index]) & below_and_at;
        self.free_word_occupied = &raw mut block.occupied[word_index];
        self.free_word_base = id - bit as RecordId;
        self.free_cursor = self.free_word_base;
        self.young_words.push(self.free_word_base);
        if self.satb {
            self.satb_blacken_free_word();
        }
    }

    /// Allocation is black while a cycle marks: the free ids of the loaded word are marked
    /// before any is claimed, as newer than the snapshot (their snapshotted bits too, so the
    /// barrier skips them), and the claims then need no work of their own. Ids from the
    /// frontier up are marked at the cycle's end instead (`end_satb`).
    #[cold]
    #[inline(never)]
    fn satb_blacken_free_word(&mut self) {
        let (block_index, word_index) = Self::word_position(self.free_word_base);
        let block = &mut self.blocks[block_index];
        // Atomic: a concurrent marker may set other bits of the word meanwhile.
        AtomicU64::from_mut(&mut block.marked[word_index]).fetch_or(self.free_word, std::sync::atomic::Ordering::Relaxed);
        block.aged[word_index] |= self.free_word;
        self.satb_reused_words.push(self.free_word_base);
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
    /// While a concurrent cycle marks, the barrier snapshots the record instead (`satb_note`),
    /// reading arena-held children from `properties`.
    #[inline(always)]
    pub(super) unsafe fn get_live_mut(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena) -> &mut T
    where
        T: SatbSnapshot,
    {
        self.barrier(id, properties);
        // SAFETY: the caller's contract.
        unsafe { self.get_live_mut_unbarriered(id) }
    }

    /// Mutable access for a store of `stored` into the record. Only a heap reference can
    /// make an old record point at a young one, so other values skip the generational
    /// barrier; while a concurrent cycle marks, every store passes the snapshot barrier, as
    /// overwriting a reference with any value deletes an edge.
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
        T: SatbSnapshot,
    {
        if self.satb {
            self.satb_note(id, properties);
        } else if self.rc {
            self.rc_barrier(id, properties);
        } else if stored.heap_index().is_some() {
            self.remember(id);
        }
        // SAFETY: the caller's contract.
        unsafe { self.get_live_mut_unbarriered(id) }
    }

    /// The write barrier for a write into record `id`: the generational log (`remember`), or
    /// the snapshot while a concurrent cycle marks (`satb_note`).
    #[inline(always)]
    pub(super) fn barrier(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena)
    where
        T: SatbSnapshot,
    {
        if self.satb {
            self.satb_note(id, properties);
        } else if self.rc {
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
        T: SatbSnapshot,
    {
        let (block_index, record_index) = Self::split_id(id);
        // SAFETY: a live id belongs to an initialized block.
        let block = unsafe { self.blocks.get_unchecked(block_index) };
        if block.marked[record_index / OCCUPANCY_WORD_BITS] & (1_u64 << (record_index % OCCUPANCY_WORD_BITS)) != 0 {
            self.rc_note(id, properties);
        }
    }

    #[cold]
    #[inline(never)]
    fn rc_note(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena)
    where
        T: SatbSnapshot,
    {
        self.remember(id);
        self.rc_logged.push(id);
        // SAFETY: a barrier runs only for a live record of this family.
        let record = unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() };
        record.snapshot(properties, &mut self.rc_decrements);
    }

    /// The snapshot barrier: before the first write in this cycle to a record older than the
    /// cycle, copies the values it keeps alive into `satb_values` and sets its snapshotted bit
    /// (the `aged` word, which a full collection emptied and no minor collection reads while
    /// a cycle runs). Later writes to it, and writes to newer records, add nothing: whatever
    /// the cycle's snapshot reached through the record is kept.
    #[cold]
    #[inline(never)]
    fn satb_note(&mut self, id: RecordId, properties: &crate::value_vec::ValueArena)
    where
        T: SatbSnapshot,
    {
        if id >= self.satb_frontier {
            return;
        }
        let (block_index, record_index) = Self::split_id(id);
        let word = &mut self.blocks[block_index].aged[record_index / OCCUPANCY_WORD_BITS];
        let mask = 1_u64 << (record_index % OCCUPANCY_WORD_BITS);
        if *word & mask != 0 {
            return;
        }
        *word |= mask;
        // SAFETY: a barrier runs only for a live record of this family; the record lies in the
        // reserved range, apart from the space's own fields.
        let record = unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() };
        record.snapshot(properties, &mut self.satb_values);
    }

    /// Starts a concurrent cycle's snapshot barrier: the records existing now are its
    /// snapshot. Follows a full collection's `forget_ages`, so no snapshotted bit is set.
    pub(super) fn begin_satb(&mut self) {
        debug_assert!(self.aged_words.is_empty() && self.remembered.is_empty());
        self.satb = true;
        self.satb_frontier = self.next_id;
        // The word allocation already holds is black from now on, as the next one loaded.
        if self.free_word != 0 {
            self.satb_blacken_free_word();
        }
    }

    /// Ends the snapshot barrier, as the cycle's remark does before its sweep: every record
    /// claimed from a fresh id is marked, a pre-marked id no claim took is unmarked again (a
    /// sweep keeps only occupied, marked ids), the snapshotted bits are forgotten, and the
    /// generational barrier logs writes again.
    pub(super) fn end_satb(&mut self) {
        for base in std::mem::take(&mut self.satb_reused_words) {
            let (block_index, word_index) = Self::word_position(base);
            let block = &mut self.blocks[block_index];
            block.marked[word_index] &= block.occupied[word_index];
        }
        let mut id = self.satb_frontier;
        while id < self.next_id {
            let (block_index, record_index) = Self::split_id(id);
            let (word_index, bit) = (record_index / OCCUPANCY_WORD_BITS, record_index % OCCUPANCY_WORD_BITS);
            let block = &mut self.blocks[block_index];
            block.marked[word_index] |= block.occupied[word_index] & (u64::MAX << bit);
            id += (OCCUPANCY_WORD_BITS - bit) as RecordId;
        }
        self.satb = false;
        for block in &mut self.blocks {
            block.aged = [0; BLOCK_WORDS];
        }
    }

    /// Whether every mark bit names an occupied id, as a sweep requires (`sweep_word`).
    #[cfg(test)]
    pub(super) fn marks_within_occupancy(&self) -> bool {
        self.blocks.iter().all(|block| (0..BLOCK_WORDS).all(|word| block.marked[word] & !block.occupied[word] == 0))
    }

    /// The children the snapshot barrier copied since the last call.
    pub(super) fn take_satb_values(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.satb_values)
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
    /// While a concurrent cycle marks, every store logs: the caller takes the barriered path.
    #[inline(always)]
    pub(super) fn store_logs(&self, id: RecordId, stored: Value) -> bool {
        if self.satb {
            return true;
        }
        // Under counts, overwriting a reference with any value deletes an edge.
        if stored.heap_index().is_none() && !self.rc {
            return false;
        }
        let (block_index, record_index) = Self::split_id(id);
        // SAFETY: a live id belongs to an initialized block.
        let block = unsafe { self.blocks.get_unchecked(block_index) };
        block.marked[record_index / OCCUPANCY_WORD_BITS] & (1_u64 << (record_index % OCCUPANCY_WORD_BITS)) != 0
    }

    #[inline(always)]
    pub(super) fn remember(&mut self, id: RecordId) {
        // A cycle's marks are not old bits: clearing one would unmark what the cycle marked.
        debug_assert!(!self.satb, "the generational barrier is off while a concurrent cycle marks");
        let (block_index, record_index) = Self::split_id(id);
        // SAFETY: a live id belongs to an initialized block.
        let block = unsafe { self.blocks.get_unchecked_mut(block_index) };
        let word = &mut block.marked[record_index / OCCUPANCY_WORD_BITS];
        let mask = 1_u64 << (record_index % OCCUPANCY_WORD_BITS);
        if *word & mask != 0 {
            *word &= !mask;
            self.log_remembered(id);
        }
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

    #[cfg(test)]
    pub(super) fn remove_live(&mut self, id: RecordId) -> T {
        debug_assert!(self.is_occupied(id), "sweep removes an initialized record");
        let (block_index, record_index) = Self::split_id(id);
        self.blocks[block_index].release(record_index);
        self.blocks[block_index].marked[record_index / OCCUPANCY_WORD_BITS] &=
            !(1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        self.free_cursor = self.next_id;
        self.free_word = 0;
        self.live -= 1;
        // SAFETY: release cleared the bit and this id was initialized exactly once.
        unsafe { self.base.as_ptr().add(id as usize).read().assume_init() }
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
    /// This space as a concurrent marker reads it (`ConcurrentRecordMarks`), from the barrier's
    /// frontier. Valid until the space next frees or sweeps a record.
    pub(super) fn concurrent_marks(&mut self) -> ConcurrentRecordMarks<T> {
        debug_assert!(self.satb, "a concurrent marker reads a space under the snapshot barrier");
        ConcurrentRecordMarks {
            blocks: self.blocks.as_mut_ptr(),
            block_count: self.blocks.len(),
            records: self.base.as_ptr().cast::<T>().cast_const(),
            frontier: self.satb_frontier,
        }
    }

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

    /// The first id of each word young records can occupy: the free words loaded since the
    /// last sweep and the words of the fresh ids from `young_floor` up. A word may be listed
    /// twice (the free scan can reach the fresh ids' first word).
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

    pub(super) fn for_each_live(&self, mut visit: impl FnMut(RecordId, &T)) {
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    // SAFETY: every occupied bit names an initialized record.
                    visit(id, unsafe {
                        (*self.base.as_ptr().add(id as usize)).assume_init_ref()
                    });
                    live &= live - 1;
                }
            }
        }
    }

    pub(super) fn for_each_live_mut(&mut self, mut visit: impl FnMut(RecordId, &mut T)) {
        let base = self.base;
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    // SAFETY: each occupied ID is visited once and `&mut self`
                    // excludes every other reference into this space.
                    visit(id, unsafe {
                        (*base.as_ptr().add(id as usize)).assume_init_mut()
                    });
                    live &= live - 1;
                }
            }
        }
    }

    fn is_occupied(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)
            .is_some_and(|block| block.is_occupied(record_index))
    }

    pub(super) fn is_live(&self, id: RecordId) -> bool {
        id < self.next_id && self.is_occupied(id)
    }

    fn split_id(id: RecordId) -> (usize, usize) {
        let id = id as usize;
        (id / RECORDS_PER_BLOCK, id % RECORDS_PER_BLOCK)
    }

    fn highest_free_before(&self, mut end: RecordId) -> Option<RecordId> {
        while end != 0 {
            let candidate = end - 1;
            let (block_index, record_index) = Self::split_id(candidate);
            let word_index = record_index / OCCUPANCY_WORD_BITS;
            let bit_count = record_index % OCCUPANCY_WORD_BITS + 1;
            let valid_bits = u64::MAX >> (OCCUPANCY_WORD_BITS - bit_count);
            let block = self
                .blocks
                .get(block_index)
                .expect("every allocated ID belongs to an initialized record block");
            let free_bits = !(block.occupied[word_index] | block.withheld[word_index]) & valid_bits;
            if free_bits != 0 {
                let bit = OCCUPANCY_WORD_BITS - 1 - free_bits.leading_zeros() as usize;
                return Some(
                    (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS + bit)
                        as RecordId,
                );
            }
            end = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
        }
        None
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        // The blocks' capacity is reserved address space; only the blocks made are touched.
        self.blocks.len() * (RECORDS_PER_BLOCK * size_of::<T>() + size_of::<RecordBlock>())
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
        self.free_cursor = self.next_id;
        self.free_word = 0;
    }

    pub(super) fn reusable_ids(&self) -> usize {
        self.next_id as usize - self.live
    }

    pub(super) fn allocation_scan_cursor(&self) -> usize {
        self.free_cursor as usize
    }
}

#[cfg(test)]
mod tests;

/// The largest count the bits hold: references past it are counted in the overflow table.
#[cfg(test)]
pub(super) const RC_STUCK: u8 = 3;
/// The references past `RC_STUCK` the overflow table counts before the record's count sticks.
const RC_OVERFLOW_LIMIT: u32 = 64;
