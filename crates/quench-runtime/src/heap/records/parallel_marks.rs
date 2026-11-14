//! A record space as the parallel mark shares it.
use super::{OCCUPANCY_WORD_BITS, RecordBlock, RecordId, RecordSpace};
use std::sync::atomic::{AtomicU64, Ordering};

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
pub(in super::super) struct ParallelRecordMarks<'space, T> {
    pub(super) blocks: *mut RecordBlock,
    pub(super) block_count: usize,
    pub(super) records: *const T,
    pub(super) _space: std::marker::PhantomData<&'space mut [T]>,
}

impl<T> Clone for ParallelRecordMarks<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ParallelRecordMarks<'_, T> {}

impl<'space, T> ParallelRecordMarks<'space, T> {
    /// The mark word of record `id`'s block that holds its bit, and the bit; none for a block
    /// the space has not made.
    #[inline(always)]
    fn mark_word(&self, id: RecordId) -> Option<(&AtomicU64, u64, usize, usize)> {
        let (block_index, record_index) = RecordSpace::<T>::split_id(id);
        if block_index >= self.block_count {
            return None;
        }
        let word_index = record_index / OCCUPANCY_WORD_BITS;
        // SAFETY: the block is in bounds, and the word is in bounds and aligned for an atomic
        // (the layout assertion); while the view lives every access to mark words goes
        // through atomics.
        let word = unsafe {
            let block = self.blocks.add(block_index);
            AtomicU64::from_ptr(std::ptr::addr_of_mut!((*block).marked).cast::<u64>().add(word_index))
        };
        Some((word, 1_u64 << (record_index % OCCUPANCY_WORD_BITS), block_index, word_index))
    }

    /// Sets record `id`'s mark bit when the record is occupied and the bit is clear: `true`
    /// for the one caller that set it, as `RecordSpace::mark` answers serially.
    #[inline(always)]
    pub(in super::super) fn mark_if_new(&self, id: RecordId) -> bool {
        let Some((word, mask, block_index, word_index)) = self.mark_word(id) else {
            return false;
        };
        // SAFETY: the block is in bounds, and no occupancy word is written while marking.
        let occupied = unsafe {
            std::ptr::addr_of!((*self.blocks.add(block_index)).occupied).cast::<u64>().add(word_index).read()
        };
        occupied & mask != 0 && super::super::mark_word_if_new(word, mask)
    }

    /// Whether record `id`'s mark bit is set. A bit another worker sets after the read is
    /// found by `mark_if_new`.
    #[inline(always)]
    pub(in super::super) fn is_marked(&self, id: RecordId) -> bool {
        self.mark_word(id).is_some_and(|(word, mask, ..)| word.load(Ordering::Relaxed) & mask != 0)
    }

    /// The record `id`.
    ///
    /// # Safety
    /// `mark_if_new(id)` must have answered `true` (or the record be otherwise known to be
    /// occupied) through this view.
    #[inline(always)]
    pub(in super::super) unsafe fn get(&self, id: RecordId) -> &'space T {
        // SAFETY: an occupied id names an initialized record, which no one writes or frees
        // while the view lives.
        unsafe { &*self.records.add(id as usize) }
    }
}
