//! Activation records live in one vector. Slots below `active` are the frame
//! stack; slots above it keep the storage of returned records, so activation
//! and return reuse a slot in place instead of moving a record in and out of
//! a separate pool.

use super::Frame;

#[derive(Default)]
pub(super) struct FrameStack {
    slots: Vec<Frame>,
    active: usize,
}

impl std::ops::Deref for FrameStack {
    type Target = [Frame];

    #[inline(always)]
    fn deref(&self) -> &[Frame] {
        // SAFETY: `active <= slots.len()` holds after every operation below.
        unsafe { self.slots.get_unchecked(..self.active) }
    }
}

impl std::ops::DerefMut for FrameStack {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [Frame] {
        // SAFETY: as for `deref`.
        unsafe { self.slots.get_unchecked_mut(..self.active) }
    }
}

impl FrameStack {
    /// Activate a record built off the stack. A spare in its slot moves
    /// above the stack, so its storage stays available.
    pub(super) fn push(&mut self, frame: Frame) {
        if self.active < self.slots.len() {
            let spare = std::mem::replace(&mut self.slots[self.active], frame);
            self.slots.push(spare);
        } else {
            self.slots.push(frame);
        }
        self.active += 1;
    }

    /// Deactivate the top record and hand it out by value.
    pub(super) fn pop(&mut self) -> Option<Frame> {
        self.active = self.active.checked_sub(1)?;
        Some(self.slots.swap_remove(self.active))
    }

    /// Activate the next slot in place, reusing a returned record's storage.
    /// Returns the caller record and the new record, which the caller must
    /// fully initialize.
    pub(super) fn activate_after(
        &mut self,
        caller: usize,
        empty: impl FnOnce() -> Frame,
    ) -> (&Frame, &mut Frame) {
        if self.active == self.slots.len() {
            self.slots.push(empty());
        }
        let index = self.active;
        self.active += 1;
        let (below, above) = self.slots.split_at_mut(index);
        (&below[caller], &mut above[0])
    }

    /// Deactivate the top record in place; its storage stays for reuse.
    pub(super) fn retire(&mut self) -> &mut Frame {
        self.active -= 1;
        &mut self.slots[self.active]
    }

    /// A returned record's storage for a later activation.
    pub(super) fn take_spare(&mut self) -> Option<Frame> {
        (self.slots.len() > self.active)
            .then(|| self.slots.pop())
            .flatten()
    }

    /// Keep a returned record's storage for a later activation.
    pub(super) fn recycle(&mut self, mut frame: Frame) {
        frame.reset_for_reuse();
        self.slots.push(frame);
    }

    /// Every record, active or spare, for storage accounting.
    #[cfg(feature = "profile-memory")]
    pub(super) fn slots(&self) -> &[Frame] {
        &self.slots
    }

    pub(super) fn clear(&mut self) {
        self.slots.clear();
        self.active = 0;
    }
}
