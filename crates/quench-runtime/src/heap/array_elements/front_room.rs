//! Room before a backing's header: front removals move the header past the values they
//! drop (`BIASED`), and front insertions move it back, so a queue or deque that works at
//! its front moves no values per operation.

use super::*;

impl UniqueElements {
    /// Drops the first value by moving the header onto it: O(1) where `remove` would move
    /// every later value, so draining an array from its front (a queue) is linear.
    pub(super) fn trim_front(&mut self) {
        let bias = self.0.bias() + 1;
        let old = self.0.header.as_ptr();
        // SAFETY: this unique allocation holds the header and at least one value after it; the
        // header moves one value forward, onto that dropped value, and the bias word takes the
        // word before it, which the old header held.
        unsafe {
            let mut header = old.read();
            header.len -= 1;
            header.capacity = ((header.capacity & !BIASED) - 1) | BIASED;
            let moved = old.cast::<Value>().add(1).cast::<Header>();
            moved.write(header);
            moved.cast::<usize>().sub(1).write(bias);
            self.0.header = NonNull::new_unchecked(moved);
        }
    }

    /// Inserts `values` before the first value by moving the header back into the room front
    /// removals left, or a new backing leaves (`regrow_front`): unshifting is amortized O(1).
    pub(super) fn prepend(&mut self, values: &[Value]) {
        let count = values.len();
        if self.0.bias() < count {
            self.regrow_front(count.max(self.0.len()));
        }
        let bias = self.0.bias() - count;
        self.recount(others(values), 0);
        // SAFETY: the header moves back `count` values within the front room, its own copy read
        // first; `values` fill the room between it and the old first value.
        unsafe {
            let mut header = self.0.header.as_ptr().read();
            header.len += count as u32;
            header.capacity = ((header.capacity & !BIASED) + count as u32) | if bias == 0 { 0 } else { BIASED };
            let moved = self.0.header.as_ptr().cast::<Value>().sub(count).cast::<Header>();
            moved.write(header);
            if bias != 0 {
                moved.cast::<usize>().sub(1).write(bias);
            }
            self.0.header = NonNull::new_unchecked(moved);
            std::ptr::copy_nonoverlapping(values.as_ptr(), self.0.values(), count);
        }
    }

    /// Moves the values to a new backing with `front` values of room before them.
    fn regrow_front(&mut self, front: usize) {
        let (len, capacity) = (self.0.len(), self.0.capacity());
        let layout = ArrayElements::layout(front + capacity);
        assert!(u32::try_from(capacity + front).is_ok_and(|word| word & BIASED == 0), "array backing capacity overflow");
        // SAFETY: the new allocation holds `front` values, a header and `capacity` values; the
        // old one, this array's own, is freed with the layout it was allocated with.
        unsafe {
            let Some(start) = NonNull::new(alloc(layout)) else {
                handle_alloc_error(layout);
            };
            let moved = start.as_ptr().cast::<Value>().add(front).cast::<Header>();
            let mut header = self.0.header.as_ptr().read();
            header.capacity = capacity as u32 | BIASED;
            moved.write(header);
            moved.cast::<usize>().sub(1).write(front);
            std::ptr::copy_nonoverlapping(self.0.values(), moved.cast::<u8>().add(VALUES_OFFSET).cast(), len);
            let (old, old_layout) = self.0.allocation();
            dealloc(old, old_layout);
            self.0.header = NonNull::new_unchecked(moved);
        }
    }

    /// Moves the header and values back to the start of the allocation, clearing `BIASED`.
    pub(super) fn unbias(&mut self) {
        let (start, _) = self.0.allocation();
        let (bias, len) = (self.0.bias(), self.0.len());
        // SAFETY: the header and its `len` values move back `bias` values within their own
        // allocation; the regions may overlap.
        unsafe {
            let mut header = self.0.header.as_ptr().read();
            header.capacity = (header.capacity & !BIASED) + bias as u32;
            std::ptr::copy(self.0.values(), start.add(VALUES_OFFSET).cast::<Value>(), len);
            let moved = start.cast::<Header>();
            moved.write(header);
            self.0.header = NonNull::new_unchecked(moved);
        }
    }
}
