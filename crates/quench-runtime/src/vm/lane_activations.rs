//! The lane stack's activations: one allocation whose top is an address, as the values' top
//! is (`LaneValues`), so a push or a pop moves one word and no length becomes a division.

use super::dispatch_fast::Activation;

/// The activations of a lane stack, `base..top`, in an allocation of room up to `end`. An
/// activation holds no drop glue, so truncation only moves the top.
pub(super) struct LaneActivations {
    base: *mut Activation,
    top: *mut Activation,
    end: *mut Activation,
    owner: Vec<Activation>,
}

impl Default for LaneActivations {
    fn default() -> Self { Self::with_capacity(0) }
}

impl LaneActivations {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        let mut owner = Vec::<Activation>::with_capacity(capacity);
        let base = owner.as_mut_ptr();
        // SAFETY: the allocation holds `capacity` activations.
        Self { base, top: base, end: unsafe { base.add(owner.capacity()) }, owner }
    }

    #[inline(always)]
    fn distance(from: *mut Activation, to: *mut Activation) -> usize {
        to.addr().wrapping_sub(from.addr()) / std::mem::size_of::<Activation>()
    }

    #[inline(always)]
    pub(super) fn len(&self) -> usize { Self::distance(self.base, self.top) }
    #[inline(always)]
    pub(super) fn capacity(&self) -> usize { Self::distance(self.base, self.end) }
    /// The activations that fit above the top.
    #[inline(always)]
    pub(super) fn room(&self) -> usize { Self::distance(self.top, self.end) }
    /// The address the next activation is written at.
    #[inline(always)]
    pub(super) fn top_ptr(&self) -> *mut Activation { self.top }
    #[inline(always)]
    pub(super) fn is_empty(&self) -> bool { self.top == self.base }
    #[inline(always)]
    pub(super) fn is_full(&self) -> bool { self.top == self.end }

    /// The innermost activation, read from the top address: no length is derived.
    #[inline(always)]
    pub(super) fn last(&self) -> Option<&Activation> {
        // SAFETY: a non-empty region's activation below `top` is initialized.
        (!self.is_empty()).then(|| unsafe { &*self.top.sub(1) })
    }

    #[inline(always)]
    pub(super) fn last_mut(&mut self) -> Option<&mut Activation> {
        // SAFETY: as `last`, and `&mut self` is the only access.
        (!self.is_empty()).then(|| unsafe { &mut *self.top.sub(1) })
    }

    #[inline(always)]
    pub(super) fn get_mut(&mut self, depth: usize) -> Option<&mut Activation> {
        let at = self.base.wrapping_add(depth);
        // SAFETY: `at` is below `top`, so it is an initialized activation.
        (at < self.top).then(|| unsafe { &mut *at })
    }

    /// Drops the innermost activation, which holds no drop glue; its slot stays as it is,
    /// unread by the region, until the next push.
    ///
    /// # Safety
    /// The region is not empty.
    #[inline(always)]
    pub(super) unsafe fn discard_top(&mut self) {
        // SAFETY: a non-empty region has an activation below `top`.
        self.top = unsafe { self.top.sub(1) };
    }

    /// # Safety
    /// `len` activations below it are initialized, within the capacity.
    #[inline(always)]
    pub(super) unsafe fn set_len(&mut self, len: usize) { self.top = self.base.wrapping_add(len); }

    /// # Safety
    /// The activations' room holds one more.
    #[inline(always)]
    pub(super) unsafe fn push_unchecked(&mut self, activation: Activation) {
        // SAFETY: `top` lies below `end`, in the allocation.
        unsafe { self.top.write(activation); self.top = self.top.add(1); }
    }

    #[inline(always)]
    pub(super) fn pop(&mut self) -> Option<Activation> {
        if self.top == self.base { return None; }
        // SAFETY: the activation below `top` is initialized.
        unsafe { self.top = self.top.sub(1); Some(self.top.read()) }
    }

    pub(super) fn truncate(&mut self, len: usize) {
        if len < self.len() { self.top = self.base.wrapping_add(len); }
    }

    /// Empties the activations, yielding them outermost first. The slots stay in the
    /// allocation, which nothing pushes into while the caller holds the region.
    pub(super) fn drain(&mut self) -> impl Iterator<Item = Activation> + use<> {
        let (base, len) = (self.base, self.len());
        self.top = base;
        // SAFETY: the `len` slots were initialized and are read once each.
        (0..len).map(move |depth| unsafe { base.add(depth).read() })
    }

    /// Makes room for `additional` more activations; whether they moved.
    pub(super) fn reserve(&mut self, additional: usize) -> bool {
        if self.room() >= additional { return false; }
        let len = self.len();
        let mut grown = Vec::<Activation>::with_capacity(len + additional);
        // SAFETY: both allocations hold `len` activations and cannot overlap.
        unsafe { std::ptr::copy_nonoverlapping(self.base, grown.as_mut_ptr(), len); }
        self.base = grown.as_mut_ptr();
        // SAFETY: the new allocation holds `len + additional` activations.
        unsafe { self.top = self.base.add(len); self.end = self.base.add(grown.capacity()); }
        self.owner = grown;
        true
    }
}

impl std::ops::Deref for LaneActivations {
    type Target = [Activation];
    #[inline(always)]
    fn deref(&self) -> &[Activation] {
        // SAFETY: `base..top` holds initialized activations of the owned allocation.
        unsafe { std::slice::from_raw_parts(self.base, self.len()) }
    }
}

impl std::ops::DerefMut for LaneActivations {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [Activation] {
        // SAFETY: as `deref`, and `&mut self` is the only access.
        unsafe { std::slice::from_raw_parts_mut(self.base, self.len()) }
    }
}
