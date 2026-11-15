//! The elements of an array this array alone owns, written in place.

use super::*;

impl UniqueElements {
    #[inline] fn set_others(&mut self, others: u32) {
        if self.0.capacity() != 0 {
            // SAFETY: a unique backing with a capacity is this array's own allocation.
            unsafe { (*self.0.header.as_ptr()).others = others };
        }
    }

    /// Counts `added` other values in and `removed` out.
    #[inline(always)]
    pub(super) fn recount(&mut self, added: u32, removed: u32) {
        if added != removed {
            self.set_others(recounted(self.0.header().others, added, removed));
        }
    }

    /// Writes `value` at `index`, below the length.
    #[inline(always)]
    pub(crate) fn set(&mut self, index: usize, value: Value) {
        assert!(index < self.0.len(), "array store index out of bounds");
        self.recount(other(value), other(self[index]));
        // SAFETY: the index is below the length of this unique backing.
        unsafe { self.0.values().add(index).write(value) };
    }

    /// `value` at every index of `range`.
    pub(crate) fn fill(&mut self, range: std::ops::Range<usize>, value: Value) {
        assert!(range.start <= range.end && range.end <= self.0.len(), "array fill out of bounds");
        self.recount(other(value) * range.len() as u32, others(&self[range.clone()]));
        for index in range {
            // SAFETY: the range is within the length of this unique backing.
            unsafe { self.0.values().add(index).write(value) };
        }
    }

    /// `values` at the indices from `start`.
    pub(crate) fn copy_from(&mut self, start: usize, values: &[Value]) {
        assert!(start + values.len() <= self.0.len(), "array copy out of bounds");
        self.recount(others(values), others(&self[start..start + values.len()]));
        // SAFETY: the range is within the length; `values` cannot alias this unique backing.
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(start), values.len()) };
    }

    /// The first value, for native code that writes the backing and keeps its count of
    /// other values (`OTHERS_FROM_VALUES`).
    pub(crate) fn as_mut_ptr(&mut self) -> *mut Value {
        self.0.values()
    }

    #[inline] fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.0.capacity());
        if self.0.capacity() != 0 {
            // SAFETY: a unique backing with a capacity is this array's own allocation.
            unsafe { (*self.0.header.as_ptr()).len = len as u32 };
        }
    }

    /// Room for `additional` more values, growing by `GROWTH_SHIFT`. Inline: a backing with
    /// room, the common append, tests its capacity alone.
    #[inline(always)]
    pub(crate) fn reserve(&mut self, additional: usize) {
        let required = self.0.len().checked_add(additional).expect("array backing overflow");
        if required > self.0.capacity() {
            self.grow_for(required);
        }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn grow_for(&mut self, required: usize) {
        if self.0.bias() != 0 {
            self.unbias();
            if required <= self.0.capacity() && self.0.len() <= self.0.capacity() / COMPACT_LIVE_DIVISOR {
                return;
            }
        }
        let capacity = self.0.capacity();
        let grown = required.max(capacity + (capacity >> GROWTH_SHIFT)).max(MIN_GROWN_CAPACITY);
        self.grow_to(grown);
    }

    pub(super) fn grow_to(&mut self, capacity: usize) {
        let capacity_word = u32::try_from(capacity).ok().filter(|word| word & (BIASED | KEEP_WIDE) == 0).expect("array backing capacity overflow");
        let old = self.0.capacity();
        if old == 0 {
            // The shared empty header: a fresh backing of its own.
            self.0 = ArrayElements::allocate(capacity, 0);
            return;
        }
        let layout = ArrayElements::layout(capacity);
        let old_layout = ArrayElements::layout(old);
        let pointer = if quarantining() {
            // A marker may hold the old pointer: copy, and quarantine the old allocation.
            // SAFETY: the new allocation is larger than the old, whose bytes all initialized.
            unsafe {
                let pointer = alloc(layout);
                if !pointer.is_null() {
                    std::ptr::copy_nonoverlapping(self.0.header.as_ptr().cast::<u8>(), pointer, old_layout.size());
                    release(self.0.header.as_ptr().cast(), old_layout);
                }
                pointer
            }
        } else {
            // SAFETY: this unique backing was allocated with the layout of its old capacity.
            unsafe { realloc(self.0.header.as_ptr().cast(), old_layout, layout.size()) }
        }
        .cast::<Header>();
        let Some(header) = NonNull::new(pointer) else {
            handle_alloc_error(layout);
        };
        self.0.header = header;
        // SAFETY: the reallocated header is this array's own, its words copied from the old.
        unsafe { (*header.as_ptr()).capacity = capacity_word | ((*header.as_ptr()).capacity & KEEP_WIDE) };
    }

    #[inline(always)]
    pub(crate) fn push(&mut self, value: Value) {
        let len = self.0.len();
        self.reserve(1);
        self.recount(other(value), 0);
        // SAFETY: `reserve` made room for one more value.
        unsafe { self.0.values().add(len).write(value) };
        self.set_len(len + 1);
    }

    #[inline(always)]
    pub(crate) fn pop(&mut self) -> Option<Value> {
        let len = self.0.len().checked_sub(1)?;
        let value = self[len];
        self.recount(0, other(value));
        self.set_len(len);
        Some(value)
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        if len < self.0.len() {
            self.recount(0, others(&self[len..]));
            self.set_len(len);
        }
    }

    pub(crate) fn resize(&mut self, len: usize, value: Value) {
        let current = self.0.len();
        if len <= current {
            self.truncate(len);
            return;
        }
        self.reserve(len - current);
        self.recount(other(value) * (len - current) as u32, 0);
        // SAFETY: `reserve` made room for the values up to `len`.
        unsafe {
            for slot in current..len {
                self.0.values().add(slot).write(value);
            }
        }
        self.set_len(len);
    }

    pub(crate) fn extend_from_slice(&mut self, values: &[Value]) {
        let len = self.0.len();
        self.reserve(values.len());
        self.recount(others(values), 0);
        // SAFETY: `reserve` made room; `values` cannot alias this unique backing's spare room.
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(len), values.len()) };
        self.set_len(len + values.len());
    }

    pub(crate) fn remove(&mut self, index: usize) -> Value {
        let len = self.0.len();
        assert!(index < len, "array removal index out of bounds");
        let value = self[index];
        self.recount(0, other(value));
        if index == 0 && len >= TRIM_MIN_LEN && self.0.capacity() != 0 {
            self.trim_front();
            return value;
        }
        // SAFETY: the tail shifts down within the initialized values.
        unsafe {
            let at = self.0.values().add(index);
            std::ptr::copy(at.add(1), at, len - index - 1);
        }
        self.set_len(len - 1);
        value
    }

    /// Replaces the values in `range` with `values`, as `Vec::splice` does.
    pub(crate) fn splice(&mut self, range: std::ops::Range<usize>, values: &[Value]) {
        let len = self.0.len();
        assert!(range.start <= range.end && range.end <= len, "array splice out of bounds");
        if range.end == 0 && len >= TRIM_MIN_LEN {
            return self.prepend(values);
        }
        let removed = range.end - range.start;
        if values.len() > removed {
            self.reserve(values.len() - removed);
        }
        let new_len = len - removed + values.len();
        self.recount(others(values), others(&self[range.clone()]));
        // SAFETY: the tail moves within the reserved allocation, then `values` fills the gap.
        unsafe {
            let base = self.0.values();
            std::ptr::copy(base.add(range.end), base.add(range.start + values.len()), len - range.end);
            std::ptr::copy_nonoverlapping(values.as_ptr(), base.add(range.start), values.len());
        }
        self.set_len(new_len);
    }

    pub(crate) fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl std::ops::Deref for UniqueElements {
    type Target = [Value];

    #[inline(always)]
    pub(super) fn deref(&self) -> &[Value] {
        debug_assert!(!self.0.is_encoded());
        // SAFETY: a unique backing is wide; its first `len` values are initialized.
        unsafe { std::slice::from_raw_parts(self.0.values(), self.0.len()) }
    }
}

impl Extend<Value> for UniqueElements {
    pub(super) fn extend<I: IntoIterator<Item = Value>>(&mut self, values: I) {
        let values = values.into_iter();
        self.reserve(values.size_hint().0);
        for value in values {
            self.push(value);
        }
    }
}
