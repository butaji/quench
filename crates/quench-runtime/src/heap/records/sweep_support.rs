//! What a sweep does beside freeing: warming what its drops will free, and checking its result.

use crate::heap::ArrayObject;
use super::{BLOCK_WORDS, OCCUPANCY_WORD_BITS, RECORDS_PER_BLOCK, RecordId, RecordSpace};

impl<T, const CAPACITY: usize> RecordSpace<T, CAPACITY> {
    /// Sets what `warm_ahead` touches for each dead record: the memory its drop frees.
    pub(in crate::heap) fn warm_before_drop(&mut self, warm: fn(&T)) {
        self.warm = Some(warm);
    }

    /// Before the full sweep drops the dead records of word `word_index` of block
    /// `block_index`, warms those of the next word (and these, for the first), so their drops
    /// wait on cache misses together, not one after another.
    #[inline(always)]
    pub(super) fn warm_ahead(&self, block_index: usize, word_index: usize) {
        let Some(warm) = self.warm else { return };
        if block_index == 0 && word_index == 0 {
            self.warm_dead(0, 0, warm);
        }
        match word_index + 1 {
            BLOCK_WORDS => self.warm_dead(block_index + 1, 0, warm),
            next => self.warm_dead(block_index, next, warm),
        }
    }

    fn warm_dead(&self, block_index: usize, word_index: usize, warm: fn(&T)) {
        let Some(block) = self.blocks.get(block_index) else { return };
        let mut dead = block.occupied[word_index] & !block.marked[word_index];
        let id_base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
        while dead != 0 {
            let id = id_base + dead.trailing_zeros();
            // SAFETY: the occupied bit proved initialization; nothing has moved the record.
            warm(unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() });
            dead &= dead - 1;
        }
    }

    /// After a sweep every occupied record is marked or aged, never both: the invariant that
    /// lets a minor sweep skip the words no allocation reached and no aged record holds.
    #[cfg(debug_assertions)]
    pub(super) fn debug_assert_swept(&self) {
        for (block_index, block) in self.blocks.iter().enumerate() {
            for word_index in 0..BLOCK_WORDS {
                let (occupied, marked, aged) =
                    (block.occupied[word_index], block.marked[word_index], block.aged[word_index]);
                assert_eq!(
                    occupied & !marked & !aged,
                    0,
                    "an unswept young record survives in block {block_index} word {word_index}"
                );
                assert_eq!(
                    aged & (marked | !occupied),
                    0,
                    "an aged record is marked or free after a sweep in block {block_index} word {word_index}"
                );
            }
        }
    }
}

/// Starts loading the element backing a dead array's drop will free (`RecordSpace::warm`): a
/// hint, so it may name the shared empty backing or a young run.
#[inline(always)]
pub(in crate::heap) fn warm_array_elements(array: &ArrayObject) {
    let address = array.elements().as_ptr();
    #[cfg(target_arch = "x86_64")]
    // SAFETY: a prefetch hint reads nothing and never faults.
    unsafe {
        core::arch::x86_64::_mm_prefetch::<{ core::arch::x86_64::_MM_HINT_T0 }>(address.cast());
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = address;
}
