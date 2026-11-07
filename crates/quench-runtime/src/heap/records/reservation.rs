//! A record space's address range: reserved on its first allocation, released with it.

use super::{OCCUPANCY_WORD_BITS, RECORDS_PER_BLOCK, RecordId, RecordSpace};
use std::alloc::{Layout, alloc, dealloc};
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// Whether a record space has its address range: reserved on its first allocation, once;
/// a failed reservation is not retried.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reservation {
    Unreserved,
    Reserved,
    Failed,
}

impl<T, const CAPACITY: usize> RecordSpace<T, CAPACITY> {
    /// Whether `base` names its address range.
    #[inline(always)]
    pub(super) fn is_reserved(&self) -> bool {
        self.reservation == Reservation::Reserved
    }

    /// Reserves its address range on the first call; `false` where that failed.
    pub(super) fn ensure_reserved(&mut self) -> bool {
        match self.reservation {
            Reservation::Reserved => return true,
            Reservation::Failed => return false,
            Reservation::Unreserved => {}
        }
        let Ok(layout) = Layout::array::<MaybeUninit<T>>(CAPACITY) else {
            self.reservation = Reservation::Failed;
            return false;
        };
        // SAFETY: the layout is a nonzero record range. The mapping is kept
        // for the arena lifetime and memory is initialized a block at a time.
        let Some(base) = NonNull::new(unsafe { alloc(layout) }.cast()) else {
            self.reservation = Reservation::Failed;
            return false;
        };
        self.base = base;
        self.reservation = Reservation::Reserved;
        true
    }
}

impl<T, const CAPACITY: usize> Drop for RecordSpace<T, CAPACITY> {
    fn drop(&mut self) {
        if !self.is_reserved() {
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
