//! The lane stack's values: one allocation whose top is an address, as every activation's
//! window is, so a return truncates by storing the window's address.

use crate::Value;

/// The values of a lane stack's windows, `base..top`, in an allocation of room up to `end`.
/// They hold no drop glue, so truncation and growth only move words.
pub(super) struct LaneValues {
    base: *mut Value,
    top: *mut Value,
    end: *mut Value,
    owner: Vec<Value>,
}

impl Default for LaneValues {
    fn default() -> Self { Self::with_capacity(0) }
}

impl LaneValues {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        let mut owner = Vec::<Value>::with_capacity(capacity);
        let base = owner.as_mut_ptr();
        // SAFETY: the allocation holds `capacity` values.
        Self { base, top: base, end: unsafe { base.add(owner.capacity()) }, owner }
    }

    #[inline(always)]
    pub(super) fn as_ptr(&self) -> *const Value { self.base }
    #[inline(always)]
    pub(super) fn as_mut_ptr(&mut self) -> *mut Value { self.base }
    #[inline(always)]
    pub(super) fn len(&self) -> usize { self.top.addr().wrapping_sub(self.base.addr()) / std::mem::size_of::<Value>() }
    #[inline(always)]
    pub(super) fn capacity(&self) -> usize { self.end.addr().wrapping_sub(self.base.addr()) / std::mem::size_of::<Value>() }
    #[inline(always)]
    pub(super) fn top(&self) -> *mut Value { self.top }
    /// The values that fit above the top.
    #[inline(always)]
    pub(super) fn room(&self) -> usize { self.end.addr().wrapping_sub(self.top.addr()) / std::mem::size_of::<Value>() }
    #[inline(always)]
    pub(super) fn clear(&mut self) { self.top = self.base; }

    /// Ends the values at `top`, an address within the allocation.
    ///
    /// # Safety
    /// `top` lies in `base..=end`, and the values below it are initialized.
    #[inline(always)]
    pub(super) unsafe fn set_top(&mut self, top: *mut Value) { self.top = top; }

    /// # Safety
    /// As `Vec::set_len`.
    #[inline(always)]
    pub(super) unsafe fn set_len(&mut self, len: usize) { self.top = self.base.wrapping_add(len); }

    /// Makes room for `additional` more values; whether the values moved.
    pub(super) fn reserve(&mut self, additional: usize) -> bool {
        if self.room() >= additional { return false; }
        let len = self.len();
        let mut grown = Vec::<Value>::with_capacity(len + additional);
        // SAFETY: both allocations hold `len` values and cannot overlap.
        unsafe { std::ptr::copy_nonoverlapping(self.base, grown.as_mut_ptr(), len); }
        self.base = grown.as_mut_ptr();
        // SAFETY: the new allocation holds `len + additional` values.
        unsafe { self.top = self.base.add(len); self.end = self.base.add(grown.capacity()); }
        self.owner = grown;
        true
    }
}

impl std::ops::Deref for LaneValues {
    type Target = [Value];
    #[inline(always)]
    fn deref(&self) -> &[Value] {
        // SAFETY: `base..top` holds initialized values of the owned allocation.
        unsafe { std::slice::from_raw_parts(self.base, self.len()) }
    }
}

impl std::ops::DerefMut for LaneValues {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [Value] {
        // SAFETY: as `deref`, and `&mut self` is the only access.
        unsafe { std::slice::from_raw_parts_mut(self.base, self.len()) }
    }
}
