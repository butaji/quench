//! A direct-mapped cache table that allocates when its first write reaches it. Reads before
//! then answer from the table's shared empty entries, so a VM that never fills the cache (a
//! static image's minimal program) pays no page for it; once filled, a read is one predictable
//! branch and one indexed load.

use std::ops::{Deref, DerefMut};

pub struct LazyTable<T: Copy + 'static, const N: usize> {
    empty: &'static [T; N],
    table: Option<Box<[T; N]>>,
}

impl<T: Copy + 'static, const N: usize> LazyTable<T, N> {
    pub const fn new(empty: &'static [T; N]) -> Self {
        Self { empty, table: None }
    }

    /// The table's current entries: the shared empty ones until its first write.
    #[inline(always)]
    fn current(&self) -> &[T; N] {
        self.table.as_deref().unwrap_or(self.empty)
    }

    #[inline(always)]
    fn current_mut(&mut self) -> &mut [T; N] {
        let empty = self.empty;
        self.table.get_or_insert_with(|| Box::new(*empty))
    }

    /// Forgets every entry without allocating a table that was never filled.
    pub fn clear(&mut self) {
        self.table = None;
    }

    /// The first entry's address, for code that binds the table by pointer: the shared empty
    /// table until the first write, a stable heap table after it.
    pub fn as_ptr(&self) -> *const T {
        self.current().as_ptr()
    }
}

impl<T: Copy + 'static, const N: usize> Deref for LazyTable<T, N> {
    type Target = [T; N];

    #[inline(always)]
    fn deref(&self) -> &[T; N] {
        self.current()
    }
}

impl<T: Copy + 'static, const N: usize> DerefMut for LazyTable<T, N> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [T; N] {
        self.current_mut()
    }
}

/// A direct-mapped table whose unread entries are one shared value: for caches whose empty
/// entry is uniform, so a VM that never fills the cache keeps no table at all. One indexed
/// read is a predictable branch; the first write allocates the table.
pub struct LazyEntries<T: Copy + 'static, const N: usize> {
    empty: T,
    table: Option<Box<[T; N]>>,
}

impl<T: Copy + 'static, const N: usize> LazyEntries<T, N> {
    pub const fn new(empty: T) -> Self {
        Self { empty, table: None }
    }

    #[inline(always)]
    fn current_mut(&mut self) -> &mut [T; N] {
        let empty = self.empty;
        self.table.get_or_insert_with(|| Box::new([empty; N]))
    }

    /// Forgets every entry without allocating a table that was never filled.
    pub fn clear(&mut self) {
        self.table = None;
    }

    /// The entries, if the table was filled: for a pass that would otherwise allocate it.
    pub fn filled_mut(&mut self) -> Option<&mut [T; N]> {
        self.table.as_deref_mut()
    }

    /// The first entry's address: for a diagnostic that reads the table by pointer.
    pub fn as_ptr(&mut self) -> *const T {
        self.current_mut().as_ptr()
    }

    /// Every entry, allocating the table on first use: for diagnostics and tests.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.current_mut().iter_mut()
    }
}

impl<T: Copy + 'static, const N: usize> std::ops::Index<usize> for LazyEntries<T, N> {
    type Output = T;

    #[inline(always)]
    fn index(&self, index: usize) -> &T {
        self.table.as_deref().map_or(&self.empty, |table| &table[index])
    }
}

impl<T: Copy + 'static, const N: usize> std::ops::IndexMut<usize> for LazyEntries<T, N> {
    #[inline(always)]
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.current_mut()[index]
    }
}
