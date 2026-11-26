//! A program table that grows in place: entries a program adds after it was shared (the
//! sites a function's first compile makes, `ResidualProgram::function`) land in capacity the
//! load reserved, so the slices and pointers into the table that the running code holds stay
//! valid. The table never moves; only its length and the entries of a function that has not
//! run yet change.
use std::cell::UnsafeCell;
use std::ops::Deref;

pub struct AppendVec<T>(UnsafeCell<Storage<T>>);

/// Headroom of at least this many bytes is reserved as fresh zero pages of its own mapping,
/// which stay out of the resident set until an append writes them: a heap block of that
/// size may be one a past phase left resident (an indirect eval of a 2 MB bundle reserved
/// 7 MB of room for chunks its one stub would append, and kept it resident every eval).
const MAPPED_HEADROOM_BYTES: usize = 64 << 10;

enum Storage<T> {
    Heap(Vec<T>),
    /// `len` entries of `cap` at `ptr`, in a private anonymous mapping of `cap` entries.
    Mapped { ptr: std::ptr::NonNull<T>, len: usize, cap: usize },
}

impl<T> Storage<T> {
    #[inline(always)]
    fn as_slice(&self) -> &[T] {
        match self {
            Self::Heap(entries) => entries,
            // SAFETY: the first `len` entries of the mapping are initialized.
            Self::Mapped { ptr, len, .. } => unsafe { std::slice::from_raw_parts(ptr.as_ptr(), *len) },
        }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        match self {
            Self::Heap(entries) => entries,
            // SAFETY: as `as_slice`, through the unique borrow.
            Self::Mapped { ptr, len, .. } => unsafe { std::slice::from_raw_parts_mut(ptr.as_ptr(), *len) },
        }
    }

    fn capacity(&self) -> usize {
        match self {
            Self::Heap(entries) => entries.capacity(),
            Self::Mapped { cap, .. } => *cap,
        }
    }

    /// `entries` moved into a mapping of `cap` entries, when one can be had.
    #[cfg(unix)]
    fn mapped(entries: Vec<T>, cap: usize) -> Result<Self, Vec<T>> {
        let Some(bytes) = cap.checked_mul(size_of::<T>()).filter(|bytes| *bytes != 0) else { return Err(entries) };
        if align_of::<T>() > 4096 {
            return Err(entries);
        }
        // SAFETY: a fresh private anonymous mapping, owned by the storage until `Drop`.
        let raw = unsafe {
            quench_alloc::map_anonymous(bytes, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_NORESERVE)
        };
        if raw == libc::MAP_FAILED {
            return Err(entries);
        }
        let ptr = std::ptr::NonNull::new(raw.cast::<T>()).expect("mmap returned null");
        let len = entries.len();
        let mut entries = std::mem::ManuallyDrop::new(entries);
        // SAFETY: the mapping holds `cap >= len` entries; the moved entries leave the vector,
        // whose buffer alone is freed.
        unsafe {
            std::ptr::copy_nonoverlapping(entries.as_ptr(), ptr.as_ptr(), len);
            entries.set_len(0);
            std::mem::ManuallyDrop::drop(&mut entries);
        }
        Ok(Self::Mapped { ptr, len, cap })
    }

    #[cfg(not(unix))]
    fn mapped(entries: Vec<T>, _cap: usize) -> Result<Self, Vec<T>> {
        Err(entries)
    }
}

impl<T> Drop for Storage<T> {
    fn drop(&mut self) {
        if let Self::Mapped { ptr, len, cap } = *self {
            // SAFETY: the first `len` entries are initialized and dropped once; the mapping
            // was made with `cap` entries' bytes.
            unsafe {
                std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(ptr.as_ptr(), len));
                #[cfg(unix)]
                libc::munmap(ptr.as_ptr().cast(), cap * size_of::<T>());
            }
        }
    }
}

impl<T> AppendVec<T> {
    /// `entries`, with room for `headroom` more that `append` may add.
    pub(crate) fn with_headroom(mut entries: Vec<T>, headroom: usize) -> Self {
        if headroom.saturating_mul(size_of::<T>()) >= MAPPED_HEADROOM_BYTES {
            let cap = entries.len().saturating_add(headroom);
            match Storage::mapped(entries, cap) {
                Ok(storage) => return Self(UnsafeCell::new(storage)),
                Err(back) => entries = back,
            }
        }
        entries.shrink_to_fit();
        entries.reserve_exact(headroom);
        Self(UnsafeCell::new(Storage::Heap(entries)))
    }

    /// Adds `entries` after the current ones, within the capacity the table was made with.
    ///
    /// # Panics
    /// When they do not fit: the table cannot move, as shared slices point into it.
    pub(crate) fn append(&self, entries: impl ExactSizeIterator<Item = T>) {
        // SAFETY: the table is single-threaded (`UnsafeCell` makes it `!Sync`), and an append
        // within capacity writes only past the entries any slice covers, leaving the buffer
        // where it is.
        let storage = unsafe { &mut *self.0.get() };
        assert!(storage.capacity() - storage.as_slice().len() >= entries.len(), "an appended table outgrew its reserved capacity");
        match storage {
            Storage::Heap(vec) => vec.extend(entries),
            Storage::Mapped { ptr, len, cap } => {
                for entry in entries {
                    assert!(*len < *cap, "an appended table outgrew its reserved capacity");
                    // SAFETY: entry `len` lies within the mapping and is uninitialized.
                    unsafe { ptr.as_ptr().add(*len).write(entry) };
                    *len += 1;
                }
            }
        }
    }

    /// Entry `index`, which only the function that owns it reads until it has run.
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn entry_mut(&self, index: usize) -> &mut T {
        // SAFETY: as `append`; the caller names an entry no running code reads.
        let storage = unsafe { &mut *self.0.get() };
        &mut storage.as_mut_slice()[index]
    }

    /// Adds `entry` after the current ones, as `append`.
    #[cfg(test)]
    pub(crate) fn push(&mut self, entry: T) {
        match self.0.get_mut() {
            Storage::Heap(vec) => vec.push(entry),
            Storage::Mapped { .. } => self.append(std::iter::once(entry)),
        }
    }

    /// This table with room for `spare` more entries, where it is still unshared and on the
    /// heap; `false` where it is mapped (it then keeps its own room).
    pub(crate) fn reserve_unshared(&mut self, spare: usize) -> bool {
        match self.0.get_mut() {
            Storage::Heap(entries) => {
                let entries = std::mem::take(entries);
                *self = Self::with_headroom(entries, spare);
                true
            }
            Storage::Mapped { .. } => false,
        }
    }

    /// How many entries `append` can still add.
    pub(crate) fn spare(&self) -> usize {
        self.capacity() - self.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        // SAFETY: reads the capacity only.
        unsafe { (*self.0.get()).capacity() }
    }
}

impl<T> Deref for AppendVec<T> {
    type Target = [T];

    #[inline(always)]
    fn deref(&self) -> &[T] {
        // SAFETY: `append` never moves the buffer, and writes only beyond the length read here.
        unsafe { (*self.0.get()).as_slice() }
    }
}

impl<T> std::ops::DerefMut for AppendVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.0.get_mut().as_mut_slice()
    }
}

impl<T: PartialEq> PartialEq for AppendVec<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T> From<Vec<T>> for AppendVec<T> {
    fn from(entries: Vec<T>) -> Self {
        Self(UnsafeCell::new(Storage::Heap(entries)))
    }
}

impl<T> Default for AppendVec<T> {
    fn default() -> Self {
        Self(UnsafeCell::new(Storage::Heap(Vec::new())))
    }
}

impl<T: Clone> Clone for AppendVec<T> {
    /// A copy with the capacity of the original, so the copy's functions append as well.
    fn clone(&self) -> Self {
        Self::with_headroom(self.to_vec(), self.spare())
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

    /// `spare` is what a caller checks before appending, so production never reaches the panic.
    #[test]
    fn spare_counts_remaining_room() {
        let table = AppendVec::with_headroom(vec![1u8], 3);
        assert!(table.spare() >= 3);
        table.append([2, 3, 4].into_iter());
        assert!(table.spare() < 3);
    }

    #[test]
    #[should_panic(expected = "reserved capacity")]
    fn append_past_capacity_panics() {
        AppendVec::with_headroom(vec![1u8], 0).append([2].into_iter());
    }
}
