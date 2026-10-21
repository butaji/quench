//! Per-site tables whose empty entry is all-zero bits, kept in an anonymous mapping that
//! the system fills with zero pages on first touch. A program's sites get their entries when
//! it loads, but only the pages holding sites that run become resident: most library code
//! never executes, so most of a table stays untouched.

/// A table entry stored as `Raw`, whose all-zero bits decode to the entry's empty value.
pub(super) trait ZeroCoded: Copy {
    type Raw: Copy;
    fn encode(self) -> Self::Raw;
    fn decode(raw: Self::Raw) -> Self;
}

/// `T` entries for `len` sites, from a reservation that grows by mapping more zero pages.
pub(super) struct ZeroTable<T: ZeroCoded> {
    storage: Storage<T::Raw>,
    len: usize,
}

impl<T: ZeroCoded> ZeroTable<T> {
    pub(super) fn new() -> Self {
        Self {
            storage: Storage::new(),
            len: 0,
        }
    }

    #[inline(always)]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// The entry at `site`.
    ///
    /// # Safety
    /// `site` is below `len`.
    #[inline(always)]
    pub(super) unsafe fn get_unchecked(&self, site: usize) -> T {
        debug_assert!(site < self.len);
        // SAFETY: the caller keeps `site` inside the entries this table holds.
        T::decode(unsafe { self.storage.read(site) })
    }

    #[inline(always)]
    pub(super) fn get(&self, site: usize) -> T {
        assert!(site < self.len, "cache site out of range");
        // SAFETY: checked above.
        unsafe { self.get_unchecked(site) }
    }

    #[inline(always)]
    pub(super) fn set(&mut self, site: usize, entry: T) {
        assert!(site < self.len, "cache site out of range");
        // SAFETY: checked above.
        unsafe { self.storage.write(site, entry.encode()) };
    }

    /// Adds `count` empty entries at the end.
    pub(super) fn extend_empty(&mut self, count: usize) {
        let len = self.len.checked_add(count).expect("cache site table overflow");
        self.storage.reserve(len);
        self.len = len;
    }

    /// Empties every entry and returns the pages that held them.
    pub(super) fn reset_all(&mut self) {
        self.storage.clear(self.len);
    }

    /// No entries at all, as a fresh program set starts.
    pub(super) fn clear(&mut self) {
        self.storage.clear(self.len);
        self.len = 0;
    }

    /// Bytes reserved for entries so far: an upper bound on the resident ones.
    #[cfg(feature = "profile-memory")]
    pub(super) fn reserved_bytes(&self) -> usize {
        self.len * size_of::<T::Raw>()
    }

    #[cfg(test)]
    pub(super) fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.len).map(|site| {
            // SAFETY: every site below `len` holds an entry.
            unsafe { self.get_unchecked(site) }
        })
    }
}

#[cfg(unix)]
mod storage {
    use std::ptr::NonNull;

    /// Entries a table may ever hold: address space only, since untouched pages cost
    /// nothing and the mapping reserves no swap.
    const RESERVED_ENTRIES: usize = 1 << 26;

    pub(super) struct Storage<R> {
        base: NonNull<R>,
        mapped: bool,
    }

    impl<R: Copy> Storage<R> {
        pub(super) fn new() -> Self {
            Self {
                base: NonNull::dangling(),
                mapped: false,
            }
        }

        fn bytes() -> usize {
            RESERVED_ENTRIES * size_of::<R>()
        }

        /// Maps the reservation on first use; entries up to `len` then read as zero.
        pub(super) fn reserve(&mut self, len: usize) {
            assert!(len <= RESERVED_ENTRIES, "cache site table exhausted");
            if self.mapped {
                return;
            }
            // SAFETY: an anonymous private mapping touches no existing memory.
            let base = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    Self::bytes(),
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                    -1,
                    0,
                )
            };
            assert_ne!(base, libc::MAP_FAILED, "cache site table reservation failed");
            self.base = NonNull::new(base.cast()).expect("mmap returned a mapping");
            self.mapped = true;
        }

        /// # Safety
        /// `site` is below a length passed to `reserve`.
        #[inline(always)]
        pub(super) unsafe fn read(&self, site: usize) -> R {
            // SAFETY: the mapping spans every reserved site, zero until written.
            unsafe { self.base.as_ptr().add(site).read() }
        }

        /// # Safety
        /// As for `read`.
        #[inline(always)]
        pub(super) unsafe fn write(&mut self, site: usize, raw: R) {
            // SAFETY: as for `read`.
            unsafe { self.base.as_ptr().add(site).write(raw) };
        }

        /// Zeroes the first `len` entries by dropping their pages.
        pub(super) fn clear(&mut self, len: usize) {
            if !self.mapped || len == 0 {
                return;
            }
            // SAFETY: the range lies inside the mapping; dropped private anonymous pages
            // read as zero again.
            let result = unsafe {
                libc::madvise(self.base.as_ptr().cast(), len * size_of::<R>(), libc::MADV_DONTNEED)
            };
            assert_eq!(result, 0, "cache site table reset failed");
        }
    }

    impl<R> Drop for Storage<R> {
        fn drop(&mut self) {
            if self.mapped {
                // SAFETY: the mapping is this table's own and no entry outlives it.
                unsafe { libc::munmap(self.base.as_ptr().cast(), RESERVED_ENTRIES * size_of::<R>()) };
            }
        }
    }
}

#[cfg(not(unix))]
mod storage {
    /// Entries in an ordinary vector, filled with the empty raw form.
    pub(super) struct Storage<R> {
        entries: Vec<R>,
    }

    impl<R: Copy> Storage<R> {
        pub(super) fn new() -> Self {
            Self { entries: Vec::new() }
        }

        pub(super) fn reserve(&mut self, len: usize) {
            // SAFETY: `R` is a plain raw form whose all-zero bits are its empty value.
            self.entries.resize(len, unsafe { std::mem::zeroed() });
        }

        #[inline(always)]
        pub(super) unsafe fn read(&self, site: usize) -> R {
            // SAFETY: the caller keeps `site` below the reserved length.
            unsafe { *self.entries.get_unchecked(site) }
        }

        #[inline(always)]
        pub(super) unsafe fn write(&mut self, site: usize, raw: R) {
            // SAFETY: as for `read`.
            unsafe { *self.entries.get_unchecked_mut(site) = raw };
        }

        pub(super) fn clear(&mut self, len: usize) {
            let len = len.min(self.entries.len());
            // SAFETY: as in `reserve`.
            self.entries[..len].fill(unsafe { std::mem::zeroed() });
        }
    }
}

use storage::Storage;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Entry(u32);

    impl ZeroCoded for Entry {
        type Raw = u32;
        fn encode(self) -> u32 {
            self.0 ^ u32::MAX
        }
        fn decode(raw: u32) -> Self {
            Self(raw ^ u32::MAX)
        }
    }

    #[test]
    fn entries_start_empty_and_reset_to_empty() {
        let mut table = ZeroTable::<Entry>::new();
        table.extend_empty(3);
        assert!(table.iter().all(|entry| entry == Entry(u32::MAX)));
        table.set(1, Entry(7));
        table.extend_empty(100_000);
        assert_eq!(table.get(1), Entry(7));
        assert_eq!(table.get(100_002), Entry(u32::MAX));
        table.reset_all();
        assert_eq!(table.get(1), Entry(u32::MAX));
        table.clear();
        assert_eq!(table.len(), 0);
    }
}
