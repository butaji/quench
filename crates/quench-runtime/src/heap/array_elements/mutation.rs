//! `UniqueElements` writes: each keeps a raw-double backing's representation
//! (`DOUBLE_ELEMENTS`), so a store of a Number writes its double and any other value
//! clears the kind (`ArrayElements::clear_doubles`), and its count of non-int32 values.

use super::*;

impl UniqueElements {
    /// The word a store writes, keeping a `DOUBLE_ELEMENTS` backing's representation: a
    /// Number becomes the double it names (an int 2 becomes 2.0, equal by every Number
    /// operation), and any other value clears the kind, since its word is a tagged value.
    #[inline(always)]
    pub(super) fn storing(&mut self, value: Value) -> Value {
        if !self.0.is_doubles() {
            return value;
        }
        match value.as_number() {
            Some(number) => Value::from_bits(double_bits(number)),
            None => {
                self.0.clear_doubles();
                value
            }
        }
    }

    /// Clears a raw-double backing's kind, for a caller that writes one raw word itself;
    /// returns the same view.
    #[inline(always)]
    pub(crate) fn without_doubles(&mut self) -> &mut Self {
        self.0.clear_doubles();
        self
    }

    /// Makes every Number in this unique backing a raw double (`DOUBLE_ELEMENTS`), so native
    /// code reads and writes the elements without tag work. `false` (nothing changed) when
    /// the backing holds any non-Number. Representation only: every Number reads the same.
    pub(crate) fn promote_doubles(&mut self) -> bool {
        if self.0.is_doubles() {
            return true;
        }
        if !self.iter().all(|value| value.as_number().is_some()) {
            return false;
        }
        let len = self.0.len();
        for index in 0..len {
            let number = self.0[index].as_number().expect("every value is a Number");
            // SAFETY: every index is below the length of this unique backing.
            unsafe { self.0.values().add(index).write(Value::from_bits(double_bits(number))) };
        }
        // Every element is a non-int32 double now.
        self.set_others(len as u32);
        // SAFETY: a unique backing is this array's own allocation.
        unsafe { (*self.0.header.as_ptr()).capacity |= DOUBLE_ELEMENTS };
        true
    }

    /// `storing` of `values` written from `start`; returns how many written words are not
    /// int32, for `recount`.
    fn copy_storing(&mut self, start: usize, values: &[Value]) -> u32 {
        let mut added = 0;
        for (offset, &value) in values.iter().enumerate() {
            let word = self.storing(value);
            added += other(word);
            // SAFETY: the caller checked `start + values.len()` is within the length, and
            // `values` cannot alias this unique backing.
            unsafe { self.0.values().add(start + offset).write(word) };
        }
        added
    }

    pub(super) fn set_others(&mut self, others: u32) {
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
        let value = self.storing(value);
        self.recount(other(value), other(self.0[index]));
        // SAFETY: the index is below the length of this unique backing.
        unsafe { self.0.values().add(index).write(value) };
    }

    /// `value` at every index of `range`.
    pub(crate) fn fill(&mut self, range: std::ops::Range<usize>, value: Value) {
        assert!(range.start <= range.end && range.end <= self.0.len(), "array fill out of bounds");
        let removed = others(&self.0[range.clone()]);
        let value = self.storing(value);
        self.recount(other(value) * range.len() as u32, removed);
        for index in range {
            // SAFETY: the range is within the length of this unique backing.
            unsafe { self.0.values().add(index).write(value) };
        }
    }

    /// `values` at the indices from `start`.
    pub(crate) fn copy_from(&mut self, start: usize, values: &[Value]) {
        assert!(start + values.len() <= self.0.len(), "array copy out of bounds");
        let removed = others(&self.0[start..start + values.len()]);
        if self.0.is_doubles() {
            let added = self.copy_storing(start, values);
            self.recount(added, removed);
            return;
        }
        self.recount(others(values), removed);
        // SAFETY: the range is within the length; `values` cannot alias this unique backing.
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(start), values.len()) };
    }

    /// The first value, for native code that writes the backing and keeps its count of
    /// other values (`OTHERS_FROM_VALUES`).
    pub(crate) fn as_mut_ptr(&mut self) -> *mut Value {
        self.0.values()
    }

    fn set_len(&mut self, len: usize) {
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
    fn grow_for(&mut self, required: usize) {
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

    fn grow_to(&mut self, capacity: usize) {
        let capacity_word = u32::try_from(capacity).expect("array backing capacity overflow");
        let old = self.0.capacity();
        if old == 0 {
            // The shared empty header: a fresh backing of its own.
            self.0 = ArrayElements::allocate(capacity, 0);
            return;
        }
        let layout = ArrayElements::layout(capacity);
        let flags = self.0.header().capacity & CAPACITY_FLAGS;
        // SAFETY: this unique backing was allocated with the layout of its old capacity.
        let pointer = unsafe {
            realloc(self.0.header.as_ptr().cast(), ArrayElements::layout(old), layout.size())
        }
        .cast::<Header>();
        let Some(header) = NonNull::new(pointer) else {
            handle_alloc_error(layout);
        };
        self.0.header = header;
        // SAFETY: the reallocated header is this array's own.
        unsafe { (*header.as_ptr()).capacity = capacity_word | flags };
    }

    #[inline(always)]
    pub(crate) fn push(&mut self, value: Value) {
        let len = self.0.len();
        self.reserve(1);
        let value = self.storing(value);
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
            self.recount(0, others(&self.0[len..]));
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
        let value = self.storing(value);
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
        if self.0.is_doubles() {
            let added = self.copy_storing(len, values);
            self.recount(added, 0);
            self.set_len(len + values.len());
            return;
        }
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
        let removed_others = others(&self.0[range.clone()]);
        // SAFETY: the tail moves within the reserved allocation, then `values` fills the gap.
        unsafe {
            let base = self.0.values();
            std::ptr::copy(base.add(range.end), base.add(range.start + values.len()), len - range.end);
        }
        if self.0.is_doubles() {
            let added = self.copy_storing(range.start, values);
            self.recount(added, removed_others);
        } else {
            self.recount(others(values), removed_others);
            // SAFETY: as above; `values` cannot alias this unique backing.
            unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values().add(range.start), values.len()) };
        }
        self.set_len(new_len);
    }

    pub(crate) fn capacity(&self) -> usize {
        self.0.capacity()
    }
}


/// The raw f64 bits a promoted backing stores for `number`: a NaN is canonical, so its
/// word is never a tagged value.
#[inline(always)]
fn double_bits(number: f64) -> u64 {
    if number.is_nan() { Value::NAN.to_bits() } else { number.to_bits() }
}
