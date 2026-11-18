//! The bound on a search's backtracking stack, and the report of a search that outgrew it.
use super::Frame;

/// The most frames a search's backtracking stack may hold: 64 MB of the interpreter's 32-byte
/// frames, V8's backtracking budget. A search that needs more is exhausted
/// (`take_backtrack_exhausted`).
pub(super) const MAXIMUM_STACK_FRAMES: usize = 1 << 21;

thread_local! {
    /// Set where a search on this thread outgrew `MAXIMUM_STACK_FRAMES`.
    pub(super) static EXHAUSTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a search on this thread since the last call ran out of backtracking stack, in
/// which case its result is no answer: the caller raises (as V8 does, a stack RangeError).
#[inline]
pub fn take_backtrack_exhausted() -> bool {
    EXHAUSTED.get() && EXHAUSTED.replace(false)
}

/// Makes room for more frames in a full `stack`, up to `MAXIMUM_STACK_FRAMES`. Past it the
/// search is exhausted (`take_backtrack_exhausted`): every choice is dropped, so each run ends
/// at its next failure, and the search reports the exhaustion in place of its result.
#[cold]
#[inline(never)]
pub(in crate::backtrack) fn reserve_frames(stack: &mut Vec<Frame>) -> bool {
    if stack.capacity() < MAXIMUM_STACK_FRAMES {
        stack.reserve(stack.capacity().max(1));
        return true;
    }
    stack.clear();
    EXHAUSTED.set(true);
    false
}
