//! The frame answers the tier guards read (`staged/guard.rs`): the code's invalidation word and the
//! active realm's globals word, each read from the row's holes. `places.rs` answers `Frame`'s
//! `invalidation` and `realm_globals` hooks with these.

use crate::heap_rows::{payload, word_at};
use crate::layout::JS_STEP_VM;
use crate::numeric::{HOLE_A, HOLE_B, S};
use crate::words::{hole, hole64};

/// The code's invalidation word: the word at the address in hole 1 (`LIVE_ROW`).
#[inline(always)]
pub(crate) fn invalidation(s: &S) -> u64 {
    word_at(hole64::<HOLE_A>() as usize, 0)
}

/// The active realm's global object word: the word at hole 1's byte offset in the VM the payload names
/// (`REALM_ROW`).
#[inline(always)]
pub(crate) fn realm_globals(s: &S) -> u64 {
    word_at(word_at(payload(s), JS_STEP_VM) as usize, hole::<HOLE_A>())
}

/// A taken backward edge counts in the run's counter, word 0 of the loop entry record: hole 1
/// and 2 are the offsets of the context's payload pointer and of the entry pointer in the payload
/// (`BackEdge`, after its poll).
#[inline(always)]
pub(crate) fn count_edge(s: &S) {
    // SAFETY: the payload and entry pointers the holes name are the activation's live records.
    unsafe {
        let payload = ((s.ctx + hole::<1>()) as *const usize).read();
        let entry = ((payload + hole::<HOLE_B>()) as *const usize).read();
        let count = (entry as *const u64).read();
        (entry as *mut u64).write(count.wrapping_add(1));
    }
}
