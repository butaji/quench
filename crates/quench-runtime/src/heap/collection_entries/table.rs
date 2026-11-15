//! A collection's slots and their folded hashes in one block: the slots, then as many
//! 32-bit hashes. One allocation where a `Vec` of slots and a vector of hashes took two, and
//! a 16-byte handle where they took 64 (jsdom-d3-startup: 23K Maps of one to three
//! entries per document, each 176 bytes in two blocks before).

use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::marker::PhantomData;
use std::ptr::NonNull;

pub(super) struct Table<T> {
    block: NonNull<u8>,
    len: u32,
    capacity: u32,
    _slots: PhantomData<T>,
}

// SAFETY: a table owns its block, as a `Vec` owns its buffer.
unsafe impl<T: Send> Send for Table<T> {}

impl<T> Table<T> {
    pub(super) const fn new() -> Self {
        Self { block: NonNull::<u64>::dangling().cast(), len: 0, capacity: 0, _slots: PhantomData }
    }

    fn layout(capacity: usize) -> Layout {
        let bytes = capacity * (size_of::<T>() + size_of::<u32>());
        Layout::from_size_align(bytes, align_of::<T>().max(align_of::<u32>())).expect("a collection's table fits memory")
    }

    /// The most slots a block of the allocator's size class for `wanted` slots holds:
    /// what the class rounds up is given to slots instead of left unused.
    fn fitted(wanted: usize) -> usize {
        let per_slot = size_of::<T>() + size_of::<u32>();
        (wanted * per_slot).next_multiple_of(ALLOCATOR_GRANULE) / per_slot
    }

    #[inline(always)]
    pub(super) fn len(&self) -> usize {
        self.len as usize
    }

    pub(super) fn capacity(&self) -> usize {
        self.capacity as usize
    }

    pub(super) fn bytes(&self) -> usize {
        if self.capacity == 0 { 0 } else { Self::layout(self.capacity()).size() }
    }

    #[inline(always)]
    fn slot_ptr(&self) -> *mut T {
        self.block.as_ptr().cast()
    }

    #[inline(always)]
    fn hash_ptr(&self) -> *mut u32 {
        // SAFETY: the hashes start right after `capacity` slots, inside the block (or at the
        // dangling start of an empty one).
        unsafe { self.block.as_ptr().add(self.capacity() * size_of::<T>()).cast() }
    }

    #[inline(always)]
    pub(super) fn slots(&self) -> &[T] {
        // SAFETY: the first `len` slots are initialized.
        unsafe { std::slice::from_raw_parts(self.slot_ptr(), self.len()) }
    }

    #[inline(always)]
    pub(super) fn slots_mut(&mut self) -> &mut [T] {
        // SAFETY: as `slots`, borrowed uniquely.
        unsafe { std::slice::from_raw_parts_mut(self.slot_ptr(), self.len()) }
    }

    #[inline(always)]
    pub(super) fn hashes(&self) -> &[u32] {
        // SAFETY: the first `len` hashes are initialized.
        unsafe { std::slice::from_raw_parts(self.hash_ptr(), self.len()) }
    }

    pub(super) fn clear(&mut self) {
        self.len = 0;
    }
}

/// The allocator's step between small size classes.
const ALLOCATOR_GRANULE: usize = quench_alloc::GRANULE_BYTES;

/// How much a full table's capacity multiplies when it grows.
const GROWTH_FACTOR: usize = 2;

/// The fewest slots a table grows to from empty.
const MIN_GROWTH_CAPACITY: usize = 2;

impl<T> Table<T> {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        let mut table = Self::new();
        if capacity > 0 {
            table.reallocate(Self::fitted(capacity));
        }
        table
    }

    /// Moves the entries to a block of `capacity` slots, at least `len`.
    fn reallocate(&mut self, capacity: usize) {
        debug_assert!(capacity >= self.len());
        let capacity32 = u32::try_from(capacity).expect("a collection's slots fit 32 bits");
        let block = if capacity == 0 {
            NonNull::<u64>::dangling().cast()
        } else {
            let layout = Self::layout(capacity);
            // SAFETY: the layout has a nonzero size.
            NonNull::new(unsafe { alloc(layout) }).unwrap_or_else(|| handle_alloc_error(layout))
        };
        let len = self.len();
        // SAFETY: the new block holds `capacity >= len` slots and hashes; the old one's first
        // `len` of each are initialized, and the two blocks are distinct.
        unsafe {
            std::ptr::copy_nonoverlapping(self.slot_ptr(), block.as_ptr().cast::<T>(), len);
            let hashes = block.as_ptr().add(capacity * size_of::<T>()).cast::<u32>();
            std::ptr::copy_nonoverlapping(self.hash_ptr(), hashes, len);
        }
        self.release();
        self.block = block;
        self.capacity = capacity32;
    }

    fn release(&mut self) {
        if self.capacity > 0 {
            // SAFETY: the block was allocated with this layout.
            unsafe { dealloc(self.block.as_ptr(), Self::layout(self.capacity())) };
        }
    }

    #[inline]
    pub(super) fn push(&mut self, slot: T, hash: u32) {
        if self.len == self.capacity {
            self.grow();
        }
        // SAFETY: `len < capacity`, so both writes land inside the block.
        unsafe {
            self.slot_ptr().add(self.len()).write(slot);
            self.hash_ptr().add(self.len()).write(hash);
        }
        self.len += 1;
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self) {
        self.reallocate(Self::fitted((self.capacity() * GROWTH_FACTOR).max(MIN_GROWTH_CAPACITY)));
    }

}

impl<T: Copy> Table<T> {
    /// Keeps the entries `keep` accepts, in order, with their hashes.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut kept = 0;
        for slot in 0..self.len() {
            // SAFETY: `kept <= slot < len`.
            unsafe {
                let entry = *self.slot_ptr().add(slot);
                if keep(&entry) {
                    self.slot_ptr().add(kept).write(entry);
                    self.hash_ptr().add(kept).write(*self.hash_ptr().add(slot));
                    kept += 1;
                }
            }
        }
        self.len = u32::try_from(kept).expect("fewer than before");
    }
}

impl<T> Drop for Table<T> {
    fn drop(&mut self) {
        if self.capacity > 0 {
            // SAFETY: the block was allocated with this layout; slots are `Copy`, nothing to drop.
            unsafe { dealloc(self.block.as_ptr(), Self::layout(self.capacity())) };
        }
    }
}

impl<T: Clone> Clone for Table<T> {
    fn clone(&self) -> Self {
        let mut table = Self::with_capacity(self.len());
        self.slots().iter().zip(self.hashes()).for_each(|(slot, &hash)| table.push(slot.clone(), hash));
        table
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Table<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table").field("slots", &self.slots()).field("hashes", &self.hashes()).finish()
    }
}
