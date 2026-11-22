//! The lane stack's values: one allocation whose top is an address, as every activation's
//! window is, so a return truncates by storing the window's address.

use crate::Value;

/// The context words below each window's register 0 (`register_file::FRAME_CONTEXT`): `top`
/// always has them free beneath it, so a window starts at `top` and the next at its end plus
/// these, and a return stores the window's address back as the top.
pub(super) const CONTEXT: usize = super::register_file::FRAME_CONTEXT;

/// The low bit of an activation's `WINDOW` word when its window is not the lane stack's: a
/// suspended frame's register file (a generator's, `FrameRecord`), which a return leaves
/// alone. A `Value` is word aligned, so the bit is free in an address.
pub(crate) const HEAP_WINDOW: usize = 1;
const _: () = assert!(std::mem::align_of::<Value>() > HEAP_WINDOW);

/// The values of a lane stack's windows, `base..top`, in an allocation of room up to `end`.
/// They hold no drop glue, so truncation and growth only move words.
pub struct LaneValues {
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
        let mut owner = Vec::<Value>::with_capacity(capacity.max(2 * CONTEXT));
        let base = owner.as_mut_ptr();
        // SAFETY: the allocation holds at least `CONTEXT` values; the first window's context
        // words start initialized, as every later window's are written by its push.
        unsafe { for word in 0..CONTEXT { base.add(word).write(Value::UNDEFINED); } }
        // The context words of the first window sit at the base (`Self::top`).
        Self { base, top: base.wrapping_add(CONTEXT), end: Self::end_of(base, owner.capacity()), owner }
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
    /// The address of the top word, which emitted code bumps through a pointer as the lane does.
    #[inline(always)]
    pub(super) fn top_word(&mut self) -> *mut *mut Value { &raw mut self.top }
    /// The values that fit above the top.
    #[inline(always)]
    pub(super) fn room(&self) -> usize { self.end.addr().wrapping_sub(self.top.addr()) / std::mem::size_of::<Value>() }
    #[inline(always)]
    pub(super) fn clear(&mut self) { self.top = self.base.wrapping_add(CONTEXT); }

    /// The end `room` measures to: the next window's context words (`CONTEXT`) past the
    /// last window's registers stay within the allocation of `capacity` values at `base`.
    fn end_of(base: *mut Value, capacity: usize) -> *mut Value {
        base.wrapping_add(capacity.saturating_sub(CONTEXT).max(CONTEXT))
    }

    /// The addresses of the allocation, `base..end`: where the windows the stack allocated lie.
    #[inline(always)]
    pub(super) fn span(&self) -> std::ops::Range<usize> { self.base.addr()..self.end.addr() }

    /// Ends the values at a returned activation's `WINDOW` word, unless it marks a window of
    /// another register file (`HEAP_WINDOW`), which frees nothing here.
    ///
    /// # Safety
    /// As `set_top`, for a stack window's word.
    #[inline(always)]
    pub(super) unsafe fn release_window(&mut self, window: usize) {
        if window & HEAP_WINDOW == 0 { self.top = window as *mut Value; }
    }

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
        unsafe { self.top = self.base.add(len); }
        self.end = Self::end_of(self.base, grown.capacity());
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
