//! The header words of a lane window, which count down from its register 0 (`staged::frame` `CONTEXT_*`).

use crate::value::Value;

/// The header word `index` (a `staged::frame` `CONTEXT_*`) of the window at `registers`: the words count
/// down from register 0, so `sub(index)` words below it.
///
/// # Safety
/// `registers` is a live window, with the frame's context words below it.
#[inline(always)]
pub(in crate::vm) unsafe fn frame_word(registers: *const Value, index: usize) -> u64 {
    // SAFETY: as the caller promises.
    unsafe { *registers.cast::<u64>().sub(index) }
}

/// Writes the header word `index` of the window at `registers` (`frame_word`).
///
/// # Safety
/// As `frame_word`, and the caller owns the window's header.
#[inline(always)]
pub(in crate::vm) unsafe fn set_frame_word(registers: *mut Value, index: usize, word: u64) {
    // SAFETY: as the caller promises.
    unsafe { *registers.cast::<u64>().sub(index) = word }
}
