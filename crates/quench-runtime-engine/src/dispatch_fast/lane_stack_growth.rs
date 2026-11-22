//! When and how a lane stack grows.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::LaneStack;

/// How much a full lane stack grows by: it doubles, so its size follows the deepest window
/// and activation count a lane has needed in amortized constant time.
const LANE_STACK_GROWTH: usize = 2;

/// Which of a lane stack's two arrays lacks room: (values, activations).
#[derive(Clone, Copy)]
pub(crate) struct LaneStackFull(bool, bool);

#[quench_macros::ext(LaneStackOps4)]
impl LaneStack {
    /// Which arrays lack room for a window of `room` values and one more activation.
    pub(crate) fn full(&self, room: usize) -> LaneStackFull {
        LaneStackFull(
            self.values.room() < room,
            // Every frame counts against the records' capacity, headers too (`spill_plain`).
            self.activation_count() >= self.frame_capacity(),
        )
    }

    /// Makes room for `additional` more values, re-pointing every frame's window where the
    /// values move: a window is an address, which only this moves, and only for the windows the
    /// values hold (a window in another register file stays). A record's window is its prefix's
    /// word; a plain header's is where the walk finds it, so the words naming one move instead
    /// of the headers spilling (one-frame-one-call.md F4 (3)): each header's caller registers,
    /// the innermost window and each sealed segment's (`relocate_plain`).
    pub(crate) fn reserve_values(&mut self, additional: usize) {
        let before = self.values.span();
        if !self.values.reserve(additional) { return; }
        let after = self.values.as_ptr().addr();
        let moved = |address: usize| if before.contains(&address) { address.wrapping_sub(before.start).wrapping_add(after) } else { address };
        for activation in self.records_mut().iter_mut() {
            activation.window = moved(activation.window);
        }
        self.relocate_plain(moved);
    }

    /// Grows the arrays `full` names; only a lack of room grows the stack.
    pub(crate) fn grow(&mut self, full: LaneStackFull, room: usize) {
        if full.0 {
            self.reserve_values((LANE_STACK_GROWTH * self.values.capacity()).max(room));
        }
        if full.1 {
            let capacity = self.frame_capacity(); self.records_mut().reserve((LANE_STACK_GROWTH * capacity).max(1));
        }
    }
}
