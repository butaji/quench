//! Cache hints for the record allocator.

use super::{OCCUPANCY_WORD_BITS, RecordId};
use std::mem::MaybeUninit;

/// Brings in the record the allocation after this one takes: the highest id still free in
/// `free_word`, whose bits name ids from `free_word_base`. An empty word names a stray address,
/// which a prefetch hint ignores.
#[inline(always)]
pub(super) fn next_free_record<T>(base: *const MaybeUninit<T>, free_word_base: RecordId, free_word: u64) {
    let bit = (OCCUPANCY_WORD_BITS - 1).wrapping_sub(free_word.leading_zeros() as usize);
    let record = base.wrapping_add(free_word_base as usize + bit);
    #[cfg(target_arch = "x86_64")]
    // SAFETY: a prefetch hint reads nothing and never faults.
    unsafe {
        core::arch::x86_64::_mm_prefetch::<{ core::arch::x86_64::_MM_HINT_T0 }>(record.cast());
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = record;
}
