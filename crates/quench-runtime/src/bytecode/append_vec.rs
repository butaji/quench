//! A program table that grows in place: entries a program adds after it was shared (the
//! sites a function's first compile makes, `ResidualProgram::function`) land in capacity the
//! load reserved, so the slices and pointers into the table that the running code holds stay
//! valid. The table never moves; only its length and the entries of a function that has not
//! run yet change.
use std::cell::UnsafeCell;
use std::ops::Deref;

pub(crate) struct AppendVec<T>(UnsafeCell<Vec<T>>);

impl<T> AppendVec<T> {
    /// `entries`, with room for `headroom` more that `append` may add.
    pub(crate) fn with_headroom(mut entries: Vec<T>, headroom: usize) -> Self {
        entries.shrink_to_fit();
        entries.reserve_exact(headroom);
        Self(UnsafeCell::new(entries))
    }

    /// Adds `entries` after the current ones, within the capacity the table was made with.
    ///
    /// # Panics
    /// When they do not fit: the table cannot move, as shared slices point into it.
    pub(crate) fn append(&self, entries: impl ExactSizeIterator<Item = T>) {
        // SAFETY: the table is single-threaded (`UnsafeCell` makes it `!Sync`), and an append
        // within capacity writes only past the entries any slice covers, leaving the buffer
        // where it is.
        let vec = unsafe { &mut *self.0.get() };
        assert!(vec.capacity() - vec.len() >= entries.len(), "an appended table outgrew its reserved capacity");
        vec.extend(entries);
    }

    /// Entry `index`, which only the function that owns it reads until it has run.
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn entry_mut(&self, index: usize) -> &mut T {
        // SAFETY: as `append`; the caller names an entry no running code reads.
        let vec = unsafe { &mut *self.0.get() };
        &mut vec[index]
    }

    fn capacity(&self) -> usize {
        // SAFETY: reads the capacity field only.
        unsafe { (*self.0.get()).capacity() }
    }
}

impl<T> Deref for AppendVec<T> {
    type Target = [T];

    #[inline(always)]
    fn deref(&self) -> &[T] {
        // SAFETY: `append` never moves the buffer, and writes only beyond the length read here.
        unsafe { &*self.0.get() }
    }
}

impl<T> std::ops::DerefMut for AppendVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.0.get_mut()
    }
}

impl<T> From<Vec<T>> for AppendVec<T> {
    fn from(entries: Vec<T>) -> Self {
        Self(UnsafeCell::new(entries))
    }
}

impl<T: Clone> Clone for AppendVec<T> {
    /// A copy with the capacity of the original, so the copy's functions append as well.
    fn clone(&self) -> Self {
        let mut entries = Vec::with_capacity(self.capacity());
        entries.extend_from_slice(self);
        Self(UnsafeCell::new(entries))
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for AppendVec<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(formatter)
    }
}

impl<'a, T> IntoIterator for &'a AppendVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::AppendVec;

    /// Appending within the reserved capacity leaves the buffer, and so the slices into it, in place.
    #[test]
    fn append_keeps_earlier_slices_valid() {
        let table = AppendVec::with_headroom(vec![1u32, 2, 3], 4);
        let before = table.as_ptr();
        let held: &[u32] = &table;
        table.append([4, 5].into_iter());
        assert_eq!(table.as_ptr(), before);
        assert_eq!(held, [1, 2, 3]);
        assert_eq!(&*table, [1, 2, 3, 4, 5]);
        *table.entry_mut(0) = 9;
        assert_eq!(table[0], 9);
    }

    /// A clone keeps the room its original had.
    #[test]
    fn clone_keeps_headroom() {
        let table = AppendVec::with_headroom(vec![1u8], 2);
        let copy = table.clone();
        copy.append([2, 3].into_iter());
        assert_eq!(&*copy, [1, 2, 3]);
        assert_eq!(&*table, [1]);
    }

    #[test]
    #[should_panic(expected = "reserved capacity")]
    fn append_past_capacity_panics() {
        AppendVec::with_headroom(vec![1u8], 0).append([2].into_iter());
    }
}
