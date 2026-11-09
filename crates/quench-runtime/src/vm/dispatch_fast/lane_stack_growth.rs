//! When and how a lane stack grows.

use super::LaneStack;

/// How much a full lane stack grows by: it doubles, so its size follows the deepest window
/// and activation count a lane has needed in amortized constant time.
const LANE_STACK_GROWTH: usize = 2;

/// Which of a lane stack's two arrays lacks room: (values, activations).
#[derive(Clone, Copy)]
pub(super) struct LaneStackFull(bool, bool);

impl LaneStack {
    /// Which arrays lack room for a window of `room` values and one more activation.
    pub(super) fn full(&self, room: usize) -> LaneStackFull {
        LaneStackFull(
            self.values.capacity() - self.values.len() < room,
            self.activations.len() == self.activations.capacity(),
        )
    }

    /// Grows the arrays `full` names; only a lack of room grows the stack.
    pub(super) fn grow(&mut self, full: LaneStackFull, room: usize) {
        if full.0 {
            self.values.reserve((LANE_STACK_GROWTH * self.values.capacity()).max(room));
        }
        if full.1 {
            self.activations.reserve((LANE_STACK_GROWTH * self.activations.capacity()).max(1));
        }
    }
}
