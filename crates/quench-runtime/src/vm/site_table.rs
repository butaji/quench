//! Per-cache-site tables whose empty entries are zero bits.
//!
//! Every installed program appends one entry per cache site to each table, and most sites
//! of a large bundle never run. Encoding an empty entry as zero bits lets a table grow over
//! fresh zero pages without writing them, so a page becomes resident only once a site on it
//! records a cache.

/// An entry kind a `SiteTable` holds: its value is stored as `Stored`, and its empty value
/// stores as zero words.
pub(super) trait SiteEntry: Copy {
    /// The stored form: plain `u32` words, so every byte is initialized.
    type Stored: StoredWords;
    fn store(self) -> Self::Stored;
    fn load(stored: Self::Stored) -> Self;
}

/// A padding-free run of `u32` words whose all-zero value is valid.
///
/// # Safety
/// The type is `u32` or an array of `u32`: no padding, and every bit pattern valid.
pub(super) unsafe trait StoredWords: Copy {}
// SAFETY: plain words.
unsafe impl StoredWords for u32 {}
// SAFETY: plain words.
unsafe impl<const N: usize> StoredWords for [u32; N] {}

/// An index entry whose empty value is `u32::MAX`, stored complemented.
impl SiteEntry for u32 {
    type Stored = u32;
    #[inline(always)]
    fn store(self) -> u32 {
        !self
    }
    #[inline(always)]
    fn load(stored: u32) -> Self {
        !stored
    }
}

/// The granule a table's mapping is sized by: the smallest page size of the supported
/// hosts.
const MAPPING_GRANULE_BYTES: usize = 4096;

#[cfg(unix)]
mod storage {
    use std::ptr::NonNull;

    /// A private anonymous mapping of `bytes` zero bytes.
    pub(super) fn map_zeroed(bytes: usize) -> NonNull<u8> {
        // SAFETY: a fresh anonymous private mapping aliases no existing memory.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert!(base != libc::MAP_FAILED, "cache site table mapping failed");
        NonNull::new(base.cast()).expect("mmap returned a mapping")
    }

    /// Returns a mapping from `map_zeroed`.
    ///
    /// # Safety
    /// `base` and `bytes` name a live mapping from `map_zeroed` that nothing reads again.
    pub(super) unsafe fn unmap(base: NonNull<u8>, bytes: usize) {
        // SAFETY: per the contract.
        unsafe { libc::munmap(base.as_ptr().cast(), bytes) };
    }
}

#[cfg(not(unix))]
mod storage {
    use std::alloc::{Layout, alloc_zeroed, dealloc};
    use std::ptr::NonNull;

    fn layout(bytes: usize) -> Layout {
        Layout::from_size_align(bytes, super::MAPPING_GRANULE_BYTES).expect("cache site table layout")
    }

    pub(super) fn map_zeroed(bytes: usize) -> NonNull<u8> {
        // SAFETY: a nonzero size.
        NonNull::new(unsafe { alloc_zeroed(layout(bytes)) })
            .unwrap_or_else(|| std::alloc::handle_alloc_error(layout(bytes)))
    }

    /// # Safety
    /// `base` and `bytes` name a live allocation from `map_zeroed`.
    pub(super) unsafe fn unmap(base: NonNull<u8>, bytes: usize) {
        // SAFETY: per the contract.
        unsafe { dealloc(base.as_ptr(), layout(bytes)) };
    }
}

/// One entry per cache site. Entries at and past `len` are zero bits, so growth only
/// moves the length.
pub(super) struct SiteTable<T: SiteEntry> {
    base: std::ptr::NonNull<T::Stored>,
    len: usize,
    /// Entries the mapping holds; zero while there is no mapping.
    capacity: usize,
}

// SAFETY: the table owns its mapping as a `Vec` owns its buffer.
unsafe impl<T: SiteEntry + Send> Send for SiteTable<T> {}

impl<T: SiteEntry> Default for SiteTable<T> {
    fn default() -> Self {
        Self { base: std::ptr::NonNull::dangling(), len: 0, capacity: 0 }
    }
}

impl<T: SiteEntry> Drop for SiteTable<T> {
    fn drop(&mut self) {
        self.release();
    }
}

impl<T: SiteEntry> SiteTable<T> {
    const ENTRY_BYTES: usize = size_of::<T::Stored>();

    /// A table of `len` empty entries.
    pub(super) fn with_empty(len: usize) -> Self {
        let mut table = Self::default();
        table.extend_empty(len);
        table
    }

    #[inline(always)]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// Entries the table holds before it next moves.
    #[cfg(feature = "profile-memory")]
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }

    /// Mapped bytes, resident or not.
    pub(super) fn capacity_bytes(&self) -> usize {
        self.capacity * Self::ENTRY_BYTES
    }

    /// # Safety
    /// `index < self.len()`.
    #[inline(always)]
    pub(super) unsafe fn get_unchecked(&self, index: usize) -> T {
        debug_assert!(index < self.len);
        // SAFETY: an index inside the table reads an initialized entry.
        T::load(unsafe { *self.base.as_ptr().add(index) })
    }

    #[inline(always)]
    /// The entries from `index` on, as a pointer `load_from` reads: a caller that holds it
    /// re-derives it whenever the table grows or is replaced, which may move the mapping.
    pub(super) fn entries_from(&self, index: usize) -> *const T::Stored {
        self.base.as_ptr().wrapping_add(index)
    }

    /// Entry `offset` of `entries` (`entries_from`).
    ///
    /// # Safety
    /// `entries` comes from `entries_from` of a table that has not grown or been replaced
    /// since, and `offset` names one of its entries.
    #[inline(always)]
    pub(super) unsafe fn load_from(entries: *const T::Stored, offset: usize) -> T {
        // SAFETY: per the contract.
        T::load(unsafe { *entries.add(offset) })
    }

    pub(super) fn get(&self, index: usize) -> T {
        assert!(index < self.len, "cache site out of range");
        // SAFETY: checked above.
        unsafe { self.get_unchecked(index) }
    }

    /// # Safety
    /// `index < self.len()`.
    #[inline(always)]
    pub(super) unsafe fn set_unchecked(&mut self, index: usize, value: T) {
        debug_assert!(index < self.len);
        // SAFETY: an index inside the table names an entry of the mapping.
        unsafe { *self.base.as_ptr().add(index) = value.store() };
    }

    #[inline(always)]
    pub(super) fn set(&mut self, index: usize, value: T) {
        assert!(index < self.len, "cache site out of range");
        // SAFETY: checked above.
        unsafe { self.set_unchecked(index, value) }
    }

    #[cfg(test)]
    pub(super) fn push(&mut self, value: T) {
        self.extend_empty(1);
        self.set(self.len - 1, value);
    }

    #[cfg(any(test, feature = "profile-memory"))]
    pub(super) fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.len).map(|index| self.get(index))
    }

    /// Appends `additional` empty entries.
    pub(super) fn extend_empty(&mut self, additional: usize) {
        let len = self
            .len
            .checked_add(additional)
            .expect("cache site count overflow");
        if len > self.capacity {
            self.grow(len);
        }
        self.len = len;
    }

    /// Empties every entry: the kernel drops the mapping's pages, which read as zero again.
    #[cfg(target_os = "linux")]
    pub(super) fn reset(&mut self) {
        if self.capacity != 0 {
            // SAFETY: the table's own private anonymous mapping, whose every entry empties.
            let result = unsafe {
                libc::madvise(
                    self.base.as_ptr().cast(),
                    self.capacity_bytes(),
                    libc::MADV_DONTNEED,
                )
            };
            assert_eq!(result, 0, "cache site table reset failed");
        }
    }

    /// Empties every entry.
    #[cfg(not(target_os = "linux"))]
    pub(super) fn reset(&mut self) {
        // SAFETY: the first `len` entries lie inside the mapping.
        unsafe {
            self.base
                .as_ptr()
                .cast::<u8>()
                .write_bytes(0, self.len * Self::ENTRY_BYTES)
        };
    }

    /// Drops every entry and the mapping.
    pub(super) fn clear(&mut self) {
        self.release();
        *self = Self::default();
    }

    /// Moves the entries to a mapping of at least `wanted` entries.
    fn grow(&mut self, wanted: usize) {
        let capacity = wanted.max(self.capacity * 2);
        let bytes = (capacity * Self::ENTRY_BYTES).next_multiple_of(MAPPING_GRANULE_BYTES);
        let base = match self.capacity {
            0 => storage::map_zeroed(bytes),
            // SAFETY: the table's own mapping, which it leaves for the moved one.
            _ => unsafe { self.moved(bytes) },
        };
        self.base = base.cast();
        self.capacity = bytes / Self::ENTRY_BYTES;
    }

    /// The mapping grown to `bytes` in place or moved by the kernel, its pages kept and
    /// its new tail zero.
    ///
    /// # Safety
    /// The table has a mapping, and adopts the returned one.
    #[cfg(target_os = "linux")]
    unsafe fn moved(&mut self, bytes: usize) -> std::ptr::NonNull<u8> {
        // SAFETY: the table's own anonymous mapping.
        let base = unsafe {
            libc::mremap(
                self.base.as_ptr().cast(),
                self.capacity_bytes(),
                bytes,
                libc::MREMAP_MAYMOVE,
            )
        };
        assert!(base != libc::MAP_FAILED, "cache site table mapping failed");
        std::ptr::NonNull::new(base.cast()).expect("mremap returned a mapping")
    }

    /// A fresh mapping of `bytes` holding the entries, the old one returned.
    ///
    /// # Safety
    /// The table has a mapping, and adopts the returned one.
    #[cfg(not(target_os = "linux"))]
    unsafe fn moved(&mut self, bytes: usize) -> std::ptr::NonNull<u8> {
        let base = storage::map_zeroed(bytes);
        // SAFETY: the old mapping holds `len` entries, and the new one is larger.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.base.as_ptr().cast::<u8>(),
                base.as_ptr(),
                self.len * Self::ENTRY_BYTES,
            )
        };
        self.release();
        base
    }

    fn release(&mut self) {
        if self.capacity != 0 {
            // SAFETY: a nonzero capacity names this table's own mapping, which it drops.
            unsafe { storage::unmap(self.base.cast(), self.capacity_bytes()) };
            self.capacity = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_entries_read_back_and_growth_keeps_recorded_entries() {
        let mut table = SiteTable::<u32>::with_empty(3);
        assert!(table.iter().all(|entry| entry == u32::MAX));
        table.set(1, 7);
        table.extend_empty(MAPPING_GRANULE_BYTES);
        assert_eq!(table.get(1), 7);
        assert_eq!(table.get(MAPPING_GRANULE_BYTES), u32::MAX);
        table.set(MAPPING_GRANULE_BYTES, 0);
        table.reset();
        assert!(table.iter().all(|entry| entry == u32::MAX));
        table.clear();
        assert_eq!(table.len(), 0);
    }
}
