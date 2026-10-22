//! The VM's activation stack, which knows how much of itself a minor collection can skip.

use super::Frame;
#[cfg(debug_assertions)]
use crate::value::Value;
use std::ops::{Deref, Index, IndexMut};

/// The activation stack. Only the top frame executes, so a frame that stayed below the top
/// since the last collection still holds the values that collection traced, all of them old:
/// a minor collection scans the frames from `unchanged_below` up. Every way to change a frame
/// other than the top lowers it: `IndexMut`, `get_unchecked_mut`, `iter_mut`, and the pops
/// that make a lower frame the top.
#[derive(Default)]
pub(super) struct FrameStack {
    frames: Vec<Box<Frame>>,
    /// Frames below this index are unchanged since the last collection. Never above the top
    /// index: the top frame runs on after a collection.
    unchanged_below: usize,
    /// Each frame's values at the last collection, which the frames below `unchanged_below`
    /// must still hold at the next.
    #[cfg(debug_assertions)]
    traced: Vec<Vec<Value>>,
}

impl FrameStack {
    pub(super) fn push(&mut self, frame: Box<Frame>) {
        self.frames.push(frame);
    }

    pub(super) fn pop(&mut self) -> Option<Box<Frame>> {
        let frame = self.frames.pop();
        self.lower_to_top();
        frame
    }

    pub(super) fn clear(&mut self) {
        self.frames.clear();
        self.unchanged_below = 0;
    }

    pub(super) fn last_mut(&mut self) -> Option<&mut Box<Frame>> {
        debug_assert!(self.unchanged_below < self.frames.len().max(1));
        self.frames.last_mut()
    }

    /// # Safety
    /// `index` must be below the stack's length.
    pub(super) unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut Box<Frame> {
        self.unchanged_below = self.unchanged_below.min(index);
        // SAFETY: the caller keeps `index` in bounds.
        unsafe { self.frames.get_unchecked_mut(index) }
    }

    pub(super) fn iter_mut(&mut self) -> std::slice::IterMut<'_, Box<Frame>> {
        self.unchanged_below = 0;
        self.frames.iter_mut()
    }

    fn lower_to_top(&mut self) {
        self.unchanged_below = self
            .unchanged_below
            .min(self.frames.len().saturating_sub(1));
    }

    /// The frames a collection traces: every frame for a full one, the frames changed since
    /// the last collection for a minor one.
    pub(super) fn to_trace(&self, full: bool) -> &[Box<Frame>] {
        #[cfg(debug_assertions)]
        if !full {
            for (index, frame) in self.frames[..self.unchanged_below].iter().enumerate() {
                assert!(
                    frame.values().eq(self.traced[index].iter().copied()),
                    "frame {index} changed below the top without lowering the unchanged mark"
                );
            }
        }
        &self.frames[if full { 0 } else { self.unchanged_below }..]
    }

    /// Starts the next interval once a collection has traced the frames.
    pub(super) fn collected(&mut self) {
        self.unchanged_below = self.frames.len().saturating_sub(1);
        #[cfg(debug_assertions)]
        {
            self.traced = self
                .frames
                .iter()
                .map(|frame| frame.values().collect())
                .collect();
        }
    }
}

impl Deref for FrameStack {
    type Target = [Box<Frame>];

    fn deref(&self) -> &Self::Target {
        &self.frames
    }
}

impl Index<usize> for FrameStack {
    type Output = Box<Frame>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.frames[index]
    }
}

impl IndexMut<usize> for FrameStack {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.unchanged_below = self.unchanged_below.min(index);
        &mut self.frames[index]
    }
}

impl Frame {
    /// Every value the frame holds, whether or not its registers are live: what a frame left
    /// unchanged since a collection still holds.
    #[cfg(debug_assertions)]
    fn values(&self) -> impl Iterator<Item = Value> + '_ {
        [self.env, self.this]
            .into_iter()
            .chain(self.context.callee())
            .chain(self.original_arguments.iter().copied())
            .chain(self.locals.iter().copied())
            .chain(self.dynamic_bindings.iter().map(|(_, value)| *value))
            .chain(self.registers.iter().copied())
    }
}
