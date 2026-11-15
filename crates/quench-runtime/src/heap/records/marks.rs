use super::*;

/// One record space during a parallel full mark: its occupancy words, read-only, and its
/// mark words, set atomically. These are the words the serial mark sets with `mark`; only
/// the access differs. Workers read records and set bits, nothing else: no record is
/// inserted, removed or written, and no block is added, until the marking scope joins
/// (the stopped-mutator invariant of `parallel_mark::ParallelMarkView`).
pub(in crate::heap) struct ParallelRecordMarks<'space, T> {
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
    pub(in crate::heap) fn mark_if_new(&self, id: RecordId) -> bool {
        let (block_index, record_index) = RecordSpace::<T>::split_id(id);
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

    /// The record `id`.
    ///
    /// # Safety
    /// `mark_if_new(id)` must have answered `true` (or the record be otherwise known to be
    /// occupied) through this view.
    #[inline(always)]
    pub(in crate::heap) unsafe fn get(&self, id: RecordId) -> &'space T {
        // SAFETY: an occupied id names an initialized record, which no one writes or frees
        // while the view lives.
        unsafe { &*self.records.add(id as usize) }
    }
}

/// A record space as a concurrent marker reads it while the mutator runs
/// (`RecordSpace::concurrent_marks`): the blocks and ids of the cycle's snapshot. Ids from the
/// frontier up are newer than the snapshot, black, as are the reused ids allocation marks
/// before claiming them, so the marker visits only records the snapshot holds; it reads
/// occupancy and records racily and sets mark bits atomically, as the mutator does for the
/// cycle's black allocation.
pub(in crate::heap) struct ConcurrentRecordMarks<T> {
    blocks: *mut RecordBlock,
    block_count: usize,
    records: *const T,
    frontier: RecordId,
}

impl<T> Clone for ConcurrentRecordMarks<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ConcurrentRecordMarks<T> {}
// SAFETY: blocks and records sit in fixed reservations the heap keeps until the cycle ends;
// mark words are shared through atomics; records are only read, without touching their
// payloads' reference counts.
unsafe impl<T> Send for ConcurrentRecordMarks<T> {}
// SAFETY: as above.
unsafe impl<T> Sync for ConcurrentRecordMarks<T> {}

impl<T> ConcurrentRecordMarks<T> {
    /// The mark word and bit of `id`, when it is below the frontier.
    #[inline(always)]
    fn position(&self, id: RecordId) -> Option<(*mut RecordBlock, usize, u64)> {
        let (block_index, record_index) = RecordSpace::<T>::split_id(id);
        if id >= self.frontier || block_index >= self.block_count {
            return None;
        }
        // SAFETY: the block index is below the snapshot's block count.
        Some((unsafe { self.blocks.add(block_index) }, record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS)))
    }

    /// Whether `id` names an occupied record of the snapshot that no one has marked.
    #[inline(always)]
    pub(in crate::heap) fn unmarked(&self, id: RecordId) -> bool {
        let Some((block, word, mask)) = self.position(id) else { return false };
        // SAFETY: the words are in the block, aligned for atomics.
        unsafe {
            let occupied = AtomicU64::from_ptr(std::ptr::addr_of_mut!((*block).occupied).cast::<u64>().add(word));
            let marked = AtomicU64::from_ptr(std::ptr::addr_of_mut!((*block).marked).cast::<u64>().add(word));
            occupied.load(std::sync::atomic::Ordering::Relaxed) & mask != 0
                && marked.load(std::sync::atomic::Ordering::Relaxed) & mask == 0
        }
    }

    /// Sets `id`'s mark bit; `true` for the one caller that set it. `id` passed `unmarked`.
    #[inline(always)]
    pub(in crate::heap) fn mark_if_new(&self, id: RecordId) -> bool {
        let Some((block, word, mask)) = self.position(id) else { return false };
        // SAFETY: as in `unmarked`.
        let marked = unsafe { AtomicU64::from_ptr(std::ptr::addr_of_mut!((*block).marked).cast::<u64>().add(word)) };
        super::mark_word_if_new(marked, mask)
    }

    /// The record `id`'s address. `id` passed `unmarked`, so it is initialized.
    #[inline(always)]
    pub(in crate::heap) fn record(&self, id: RecordId) -> *const T {
        // SAFETY: below the frontier, inside the reservation.
        unsafe { self.records.add(id as usize) }
    }
}
