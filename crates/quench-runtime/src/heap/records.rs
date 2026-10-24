use super::{ArrayObject, AwaitState, FunctionRecord, IteratorRecord, Object, PromiseCell};
use crate::vm::wtf16::JsString;
use crate::value::Value;
use std::alloc::Layout;
use std::alloc::{alloc, dealloc};
use std::mem::MaybeUninit;
#[cfg(feature = "profile-memory")]
use std::mem::size_of;
use std::ptr::NonNull;

pub(super) type RecordId = u32;

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
pub(super) type StringRecordSpace = RecordSpace<JsString, STRING_RECORD_CAPACITY>;
pub(super) const STRING_RECORD_CAPACITY: usize = super::MAX_HEAP_CELLS;
const _: () = assert!(STRING_RECORD_CAPACITY.is_power_of_two());
const _: () = assert!(STRING_RECORD_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(STRING_RECORD_CAPACITY <= u32::MAX as usize);

/// What the collector does to every record family alike, whatever its record type: the
/// per-collection passes iterate `Heap::record_families` instead of naming each space.
pub(super) trait RecordFamily {
    /// Every record becomes young: a full collection's starting state.
    fn clear_marks(&mut self);
    /// The records the barrier logged since the last collection; each is unmarked.
    fn take_remembered(&mut self) -> Vec<RecordId>;
    fn live_count(&self) -> usize;
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
}

impl<T, const CAPACITY: usize> RecordFamily for RecordSpace<T, CAPACITY> {
    fn clear_marks(&mut self) {
        RecordSpace::clear_marks(self);
    }
    fn take_remembered(&mut self) -> Vec<RecordId> {
        RecordSpace::take_remembered(self)
    }
    fn live_count(&self) -> usize {
        RecordSpace::live_count(self)
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
    #[cfg(debug_assertions)]
    fn for_each_marked(&self, visit: &mut dyn FnMut(RecordId)) {
        RecordSpace::for_each_marked(self, |id| visit(id));
    }
}

const RECORDS_PER_BLOCK: usize = 1_024;
const OCCUPANCY_WORD_BITS: usize = u64::BITS as usize;
pub(super) const RECORD_SPACE_CAPACITY: usize = 1 << 24;

const _: () = assert!(RECORDS_PER_BLOCK.is_power_of_two());
const _: () = assert!(RECORD_SPACE_CAPACITY.is_power_of_two());
const _: () = assert!(RECORD_SPACE_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(RECORD_SPACE_CAPACITY <= u32::MAX as usize);

/// Each record family owns one fixed address range. Block metadata controls
/// allocation and sweep, but resolving a live locator never walks it.
pub(super) struct RecordSpace<T, const CAPACITY: usize = RECORD_SPACE_CAPACITY> {
    base: NonNull<MaybeUninit<T>>,
    reserved: bool,
    reservation_failed: bool,
    blocks: Vec<RecordBlock>,
    next_id: u32,
    /// Where the downward scan for reusable IDs continues; 0 once none remain below it.
    free_cursor: RecordId,
    /// The reusable IDs of the occupancy word the scan last reached, starting at
    /// `free_word_base`; allocation takes them highest first before scanning further.
    free_word: u64,
    free_word_base: RecordId,
    live: usize,
    /// Old records written since the last collection, each logged once by the write barrier.
    remembered: Vec<RecordId>,
    /// Where young records can be. Every sweep frees each occupied, unmarked record it
    /// visits, so between collections only the ids taken since the last sweep are occupied
    /// and unmarked: fresh ids from `young_floor` up, and the free words loaded since. A
    /// minor sweep visits those words alone, at a cost that follows allocation, not heap size.
    young_floor: RecordId,
    /// The first id of each occupancy word `load_free_word` loaded since the last sweep.
    young_words: Vec<RecordId>,
}

impl<T, const CAPACITY: usize> Default for RecordSpace<T, CAPACITY> {
    fn default() -> Self {
        Self {
            base: NonNull::dangling(),
            reserved: false,
            reservation_failed: false,
            blocks: Vec::new(),
            next_id: 0,
            free_cursor: 0,
            free_word: 0,
            free_word_base: 0,
            live: 0,
            remembered: Vec::new(),
            young_floor: 0,
            young_words: Vec::new(),
        }
    }
}

impl<T, const CAPACITY: usize> RecordSpace<T, CAPACITY> {
    /// The first address in this record family's reserved range, if it has been reserved.
    ///
    /// The range is allocated once and kept for the lifetime of the record space. A native
    /// reader may resolve a live direct Value through this base and its record ID, but it must
    /// keep the Value rooted and must not carry a record or payload pointer across a VM
    /// safepoint. In particular, a record's owned suballocations may change when the VM runs.
    #[cfg(test)]
    #[inline]
    pub(super) fn base_ptr(&self) -> Option<*const T> {
        self.reserved
            .then(|| self.base.as_ptr().cast::<T>().cast_const())
    }
}

struct RecordBlock {
    occupied: [u64; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
    marked: [u64; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
}

pub(super) enum RecordSweep<'a, T> {
    Keep(RecordId, &'a T),
    Dead(RecordId, T),
}

impl RecordBlock {
    fn new() -> Self {
        Self {
            occupied: [0; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
            marked: [0; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
        }
    }

    fn claim(&mut self, index: usize) -> bool {
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

    fn is_occupied(&self, index: usize) -> bool {
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

    fn mark(&mut self, index: usize) -> bool {
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

impl<T, const CAPACITY: usize> RecordSpace<T, CAPACITY> {
    /// Allocates a dense identity in this record space. The family's Value
    /// tag carries this ID directly, independent of legacy Cell slot allocation.
    #[inline(always)]
    pub(super) fn try_insert(&mut self, value: T) -> Result<RecordId, T> {
        // The common allocation, inline: an id from the loaded free word, else the next fresh
        // id in a block that exists. Reservation, new blocks and loading the next free word
        // take `claim_slow`, which names no value: an allocation inlined into a lane handler
        // then passes it no address of the handler's stack, which would keep the handler's
        // dispatch from being a sibling call (`tools/check-lane-tail-calls.sh`).
        let id = if self.free_word != 0 {
            let bit = OCCUPANCY_WORD_BITS - 1 - self.free_word.leading_zeros() as usize;
            self.free_word &= !(1_u64 << bit);
            let id = self.free_word_base + bit as RecordId;
            let (block_index, record_index) = Self::split_id(id);
            let claimed = self.blocks[block_index].claim(record_index);
            debug_assert!(claimed, "a free-word bit names an unoccupied id");
            id
        } else if self.free_cursor == 0
            && self.reserved
            && (self.next_id as usize) < CAPACITY
            && Self::split_id(self.next_id).0 < self.blocks.len()
        {
            let id = self.next_id;
            let (block_index, record_index) = Self::split_id(id);
            if !self.blocks[block_index].claim(record_index) {
                return Err(value);
            }
            self.next_id += 1;
            id
        } else {
            let Some(id) = self.claim_slow() else {
                return Err(value);
            };
            id
        };
        // SAFETY: the occupancy bit claims this previously uninitialized id, in the reserved
        // range (a free word and a block exist only after reservation).
        unsafe {
            self.base
                .as_ptr()
                .add(id as usize)
                .write(MaybeUninit::new(value));
        };
        self.live += 1;
        Ok(id)
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
            let bit = OCCUPANCY_WORD_BITS - 1 - self.free_word.leading_zeros() as usize;
            self.free_word &= !(1_u64 << bit);
            let id = self.free_word_base + bit as RecordId;
            let (block_index, record_index) = Self::split_id(id);
            let claimed = self.blocks[block_index].claim(record_index);
            debug_assert!(claimed, "a free-word bit names an unoccupied id");
            return Some(id);
        }
        let id = self.next_id;
        if id as usize >= CAPACITY {
            return None;
        }
        let (block_index, record_index) = Self::split_id(id);
        while self.blocks.len() <= block_index {
            self.blocks.push(RecordBlock::new());
        }
        if !self.blocks[block_index].claim(record_index) {
            return None;
        }
        self.next_id += 1;
        Some(id)
    }

    /// Places `value` at `id`, as a snapshot restores it (`crate::snapshot`): ids arrive in
    /// increasing order, each marked (old), as after a full collection; the ids skipped are
    /// reusable once `finish_snapshot_inserts` runs. `false` when the id cannot be placed.
    pub(super) fn insert_snapshot(&mut self, id: RecordId, value: T) -> bool {
        if !self.ensure_reserved() || id as usize >= RECORD_SPACE_CAPACITY || id < self.next_id {
            return false;
        }
        let (block_index, record_index) = Self::split_id(id);
        while self.blocks.len() <= block_index {
            self.blocks.push(RecordBlock::new());
        }
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
        self.free_word = !self.blocks[block_index].occupied[word_index] & below_and_at;
        self.free_word_base = id - bit as RecordId;
        self.free_cursor = self.free_word_base;
        self.young_words.push(self.free_word_base);
    }

    fn ensure_reserved(&mut self) -> bool {
        if self.reserved {
            return true;
        }
        if self.reservation_failed {
            return false;
        }
        let Ok(layout) = Layout::array::<MaybeUninit<T>>(CAPACITY) else {
            self.reservation_failed = true;
            return false;
        };
        // SAFETY: the layout is a nonzero record range. The mapping is kept
        // for the arena lifetime and memory is initialized a block at a time.
        let Some(base) = NonNull::new(unsafe { alloc(layout) }.cast()) else {
            self.reservation_failed = true;
            return false;
        };
        self.base = base;
        self.reserved = true;
        true
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
    pub(super) unsafe fn get_live_mut(&mut self, id: RecordId) -> &mut T {
        self.remember(id);
        // SAFETY: the caller's contract.
        unsafe { self.get_live_mut_unbarriered(id) }
    }

    /// Mutable access for a store of `stored` into the record. Only a heap reference can
    /// make an old record point at a young one, so other values skip the write barrier.
    ///
    /// # Safety
    /// As for `get_live_mut_unbarriered`.
    #[inline(always)]
    pub(super) unsafe fn get_live_mut_storing(&mut self, id: RecordId, stored: Value) -> &mut T {
        if stored.heap_index().is_some() {
            self.remember(id);
        }
        // SAFETY: the caller's contract.
        unsafe { self.get_live_mut_unbarriered(id) }
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
        if stored.heap_index().is_none() {
            return false;
        }
        let (block_index, record_index) = Self::split_id(id);
        // SAFETY: a live id belongs to an initialized block.
        let block = unsafe { self.blocks.get_unchecked(block_index) };
        block.marked[record_index / OCCUPANCY_WORD_BITS] & (1_u64 << (record_index % OCCUPANCY_WORD_BITS)) != 0
    }

    #[inline(always)]
    pub(super) fn remember(&mut self, id: RecordId) {
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

    pub(super) fn mark(&mut self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get_mut(block_index)
            .is_some_and(|block| block.is_occupied(record_index) && block.mark(record_index))
    }

    pub(super) fn is_marked(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)
            .is_some_and(|block| block.is_occupied(record_index) && block.is_marked(record_index))
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
        let expected_len = self.blocks.len() * (RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS);
        assert_eq!(marks.len(), expected_len);
        for (block, block_marks) in self
            .blocks
            .iter_mut()
            .zip(marks.chunks_exact(RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS))
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

    /// Frees every occupied, unmarked record through `visit`. Marked records survive and keep
    /// their mark bit, which makes them old for the next minor collection; they reach `visit`
    /// only when a full sweep asks for them (`visit_live`). A full sweep visits every word; a
    /// minor one only the words young records can occupy, so it pays for its allocation, not
    /// for the old records it keeps.
    pub(super) fn sweep(
        &mut self,
        full: bool,
        visit_live: bool,
        mut visit: impl FnMut(RecordSweep<'_, T>),
    ) -> (usize, usize) {
        let mut freed_count = 0;
        let live_count = if full {
            let mut live_count = 0;
            for block_index in 0..self.blocks.len() {
                for word_index in 0..RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS {
                    let (live, freed) = self.sweep_word(block_index, word_index, visit_live, &mut visit);
                    live_count += live;
                    freed_count += freed;
                }
            }
            live_count
        } else {
            let young_words = std::mem::take(&mut self.young_words);
            let fresh_words = (self.young_floor / OCCUPANCY_WORD_BITS as RecordId)
                ..self.next_id.div_ceil(OCCUPANCY_WORD_BITS as RecordId);
            for word in young_words
                .iter()
                .map(|base| base / OCCUPANCY_WORD_BITS as RecordId)
                .chain(fresh_words)
            {
                let (block_index, record_index) =
                    Self::split_id(word * OCCUPANCY_WORD_BITS as RecordId);
                freed_count +=
                    self.sweep_word(block_index, record_index / OCCUPANCY_WORD_BITS, false, &mut visit).1;
            }
            self.young_words = young_words;
            self.live - freed_count
        };
        self.live = live_count;
        self.young_floor = self.next_id;
        self.young_words.clear();
        self.free_cursor = if live_count < self.next_id as usize {
            self.next_id
        } else {
            0
        };
        self.free_word = 0;
        #[cfg(debug_assertions)]
        self.debug_assert_swept();
        (live_count, freed_count)
    }

    /// Sweeps one occupancy word: frees its occupied, unmarked records and, when
    /// `visit_live`, shows `visit` its marked ones. Returns the live and freed counts it saw.
    #[inline(always)]
    fn sweep_word(
        &mut self,
        block_index: usize,
        word_index: usize,
        visit_live: bool,
        visit: &mut impl FnMut(RecordSweep<'_, T>),
    ) -> (usize, usize) {
        let block = &mut self.blocks[block_index];
        let occupied = block.occupied[word_index];
        let marked = block.marked[word_index];
        debug_assert_eq!(marked & !occupied, 0);
        let mut dead = occupied & !marked;
        let id_base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
        let mut live = if visit_live { marked } else { 0 };
        while live != 0 {
            let id = id_base + live.trailing_zeros();
            // SAFETY: a mark bit is set only for an occupied record.
            visit(RecordSweep::Keep(id, unsafe {
                (*self.base.as_ptr().add(id as usize)).assume_init_ref()
            }));
            live &= live - 1;
        }
        block.occupied[word_index] = marked;
        let freed = dead.count_ones() as usize;
        while dead != 0 {
            let id = id_base + dead.trailing_zeros();
            // SAFETY: the occupied bit proved initialization and clearing it above transferred
            // this record to the drop callback.
            let value = unsafe { self.base.as_ptr().add(id as usize).read().assume_init() };
            visit(RecordSweep::Dead(id, value));
            dead &= dead - 1;
        }
        (marked.count_ones() as usize, freed)
    }

    /// After a sweep every occupied record is marked: the invariant that lets a minor sweep
    /// skip the words no allocation reached.
    #[cfg(debug_assertions)]
    fn debug_assert_swept(&self) {
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, (occupied, marked)) in
                block.occupied.iter().zip(block.marked.iter()).enumerate()
            {
                assert_eq!(
                    occupied & !marked,
                    0,
                    "an unswept young record survives in block {block_index} word {word_index}"
                );
            }
        }
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
            let occupied = self
                .blocks
                .get(block_index)
                .expect("every allocated ID belongs to an initialized record block")
                .occupied[word_index];
            let free_bits = !occupied & valid_bits;
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
        self.blocks.len() * RECORDS_PER_BLOCK * size_of::<T>()
            + self.blocks.capacity() * size_of::<RecordBlock>()
    }

    pub(super) fn reusable_ids(&self) -> usize {
        self.next_id as usize - self.live
    }

    pub(super) fn allocation_scan_cursor(&self) -> usize {
        self.free_cursor as usize
    }
}

impl<T, const CAPACITY: usize> Drop for RecordSpace<T, CAPACITY> {
    fn drop(&mut self) {
        if !self.reserved {
            return;
        }
        let layout = Layout::array::<MaybeUninit<T>>(CAPACITY)
            .expect("record-space layout was validated at reservation");
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let id = (block_index * RECORDS_PER_BLOCK
                        + word_index * OCCUPANCY_WORD_BITS
                        + bit) as RecordId;
                    // SAFETY: every set bit denotes exactly one initialized record.
                    unsafe {
                        self.base
                            .as_ptr()
                            .add(id as usize)
                            .as_mut()
                            .unwrap()
                            .assume_init_drop()
                    };
                    live &= live - 1;
                }
            }
        }
        // SAFETY: this was allocated with the layout recomputed above.
        unsafe { dealloc(self.base.as_ptr().cast(), layout) };
    }
}

#[cfg(test)]
mod tests {
    use super::{RECORD_SPACE_CAPACITY, RECORDS_PER_BLOCK, RecordSpace};
    use std::mem::size_of;

    #[test]
    fn reserved_record_base_is_absent_until_reserved_and_then_stable() {
        let mut arena: RecordSpace<u64, RECORDS_PER_BLOCK> = RecordSpace::default();
        assert!(arena.base_ptr().is_none());

        let first = arena.try_insert(11).unwrap();
        let base = arena.base_ptr().expect("insertion reserves the record range");
        // SAFETY: `first` is the live ID returned by the insertion above.
        assert_eq!(unsafe { *base.add(first as usize) }, 11);

        let second = arena.try_insert(22).unwrap();
        assert_eq!(arena.base_ptr(), Some(base));
        // SAFETY: `second` is the live ID returned by the insertion above.
        assert_eq!(unsafe { *base.add(second as usize) }, 22);
    }

    #[test]
    fn object_record_addresses_are_dense_and_reuse_vacancies() {
        let mut arena: RecordSpace<_> = RecordSpace::default();
        let first = arena.try_insert(String::from("first")).unwrap();
        let second = arena.try_insert(String::from("second")).unwrap();
        // SAFETY: both ids were returned by successful insertions and remain live.
        let first_address = unsafe { arena.get_live(first) } as *const String as usize;
        // SAFETY: both ids were returned by successful insertions and remain live.
        let second_address = unsafe { arena.get_live(second) } as *const String as usize;
        assert_eq!(second_address - first_address, size_of::<String>());

        let mut last = second;
        for value in 2..=RECORDS_PER_BLOCK {
            last = arena
                .try_insert(format!("record-{value}"))
                .expect("record-space reservation has spare capacity");
        }
        assert_eq!(last as usize, RECORDS_PER_BLOCK);
        // SAFETY: the first id remains live after the second block is added.
        assert_eq!(unsafe { arena.get_live(first) }, "first");
        assert!(RECORD_SPACE_CAPACITY > RECORDS_PER_BLOCK);
        assert_eq!(arena.remove_live(first), "first");
        assert_eq!(arena.try_insert(String::from("reused")).unwrap(), first);
        // SAFETY: the slot-index id was reinitialized by the insertion above.
        assert_eq!(unsafe { arena.get_live(first) }, "reused");
    }

    #[test]
    fn object_record_sweep_uses_marks_and_recycles_dead_ids() {
        use super::RecordSweep;

        let mut arena: RecordSpace<_> = RecordSpace::default();
        let dead = arena.try_insert(String::from("dead")).unwrap();
        let live = arena.try_insert(String::from("live")).unwrap();
        assert!(arena.mark(live));
        assert!(!arena.mark(live));

        let mut kept = Vec::new();
        let mut dropped = Vec::new();
        let (live_count, freed_count) = arena.sweep(true, true, |record| match record {
            RecordSweep::Keep(id, value) => kept.push((id, value.clone())),
            RecordSweep::Dead(id, value) => dropped.push((id, value)),
        });
        assert_eq!((live_count, freed_count), (1, 1));
        assert_eq!(kept, [(live, String::from("live"))]);
        assert_eq!(dropped, [(dead, String::from("dead"))]);
        assert!(arena.is_live(live));
        assert!(!arena.is_live(dead));
        assert_eq!(arena.try_insert(String::from("reused")).unwrap(), dead);
    }

    #[test]
    fn bitmap_reuse_preserves_descending_free_id_order_across_sweeps() {
        use super::RecordSweep;

        let mut arena: RecordSpace<_> = RecordSpace::default();
        let ids: Vec<_> = (0..6)
            .map(|value| arena.try_insert(value).unwrap())
            .collect();
        for &live in &[ids[1], ids[4]] {
            assert!(arena.mark(live));
        }

        let mut dead_ids = Vec::new();
        let (live_count, freed_count) = arena.sweep(true, true, |record| match record {
            RecordSweep::Keep(_, _) => {}
            RecordSweep::Dead(id, _) => dead_ids.push(id),
        });
        assert_eq!((live_count, freed_count), (2, 4));
        assert_eq!(dead_ids, [ids[0], ids[2], ids[3], ids[5]]);

        assert_eq!(arena.try_insert(10).unwrap(), ids[5]);
        arena.clear_marks();
        for &live in &[ids[1], ids[4], ids[5]] {
            assert!(arena.mark(live));
        }
        assert_eq!(arena.sweep(true, true, |_| {}), (3, 0));
        assert_eq!(arena.try_insert(11).unwrap(), ids[3]);
        assert_eq!(arena.try_insert(12).unwrap(), ids[2]);
        assert_eq!(arena.try_insert(13).unwrap(), ids[0]);
        assert_eq!(arena.try_insert(14).unwrap(), 6);
    }
}
