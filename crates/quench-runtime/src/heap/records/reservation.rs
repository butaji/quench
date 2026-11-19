//! A record space's address range: reserved on its first allocation, released with it.

use super::{OCCUPANCY_WORD_BITS, RECORDS_PER_BLOCK, RecordId, RecordSpace};
use std::alloc::Layout;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// Whether a record space has its address range: reserved on its first allocation, once;
/// a failed reservation is not retried.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reservation {
    Unreserved,
    Reserved,
    Failed,
}

/// Maps the address range of `layout`, lazily backed. On Linux the mapping is `MAP_NORESERVE`, so
/// the kernel's overcommit heuristic does not charge the whole range against memory: only the
/// pages a block touches are backed. Elsewhere the global allocator stands in.
///
/// # Safety
/// `layout` is nonzero-sized, and the range is released only by `unmap_range` with the same layout.
#[cfg(target_os = "linux")]
unsafe fn map_range(layout: Layout) -> Option<NonNull<u8>> {
    // SAFETY: an anonymous private mapping at an address nothing else names.
    let mapped = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            layout.size(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
            -1,
            0,
        )
    };
    if mapped == libc::MAP_FAILED {
        return None;
    }
    NonNull::new(mapped.cast())
}

/// # Safety
/// `base` and `layout` are the pair `map_range` returned.
#[cfg(target_os = "linux")]
unsafe fn unmap_range(base: NonNull<u8>, layout: Layout) {
    // SAFETY: the pair names a mapping `map_range` made, which no reference still uses.
    unsafe { libc::munmap(base.as_ptr().cast(), layout.size()) };
}

/// # Safety
/// `layout` is nonzero-sized.
#[cfg(not(target_os = "linux"))]
unsafe fn map_range(layout: Layout) -> Option<NonNull<u8>> {
    // SAFETY: the layout is nonzero-sized (`ensure_reserved`), and `unmap_range` frees it.
    NonNull::new(unsafe { std::alloc::alloc(layout) })
}

/// # Safety
/// `base` and `layout` are the pair `map_range` returned.
#[cfg(not(target_os = "linux"))]
unsafe fn unmap_range(base: NonNull<u8>, layout: Layout) {
    // SAFETY: the pair names a block `map_range` allocated with this layout.
    unsafe { std::alloc::dealloc(base.as_ptr(), layout) };
}

impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// Whether `base` names its address range.
    #[inline(always)]
    pub(super) fn is_reserved(&self) -> bool {
        self.reservation == Reservation::Reserved
    }

    /// Reserves its address range on the first call; `false` where that failed.
    pub(super) fn ensure_reserved(&mut self) -> bool {
        match self.reservation {
            Reservation::Reserved => return true,
            Reservation::Failed => return false,
            Reservation::Unreserved => {}
        }
        // The mapping is page aligned, so it serves any record alignment up to a page.
        const { assert!(std::mem::align_of::<MaybeUninit<T>>() <= 4096) };
        const { assert!(CAPACITY < u32::MAX as usize) };
        let Ok(layout) = Layout::array::<MaybeUninit<T>>(CAPACITY) else {
            self.reservation = Reservation::Failed;
            return false;
        };
        // SAFETY: the layout is the nonzero record range; `Drop` releases it with the same layout.
        let Some(base) = (unsafe { map_range(layout) }) else {
            self.reservation = Reservation::Failed;
            return false;
        };
        self.base = base.cast();
        self.reservation = Reservation::Reserved;
        // The block metadata never moves either: room for every block the range can hold
        // is taken now, as address space only, and a block's pages are touched when it is
        // made. A pointer into it (`native_mark_blocks`) stays valid for the space's life.
        self.blocks.reserve_exact(CAPACITY / RECORDS_PER_BLOCK);
        true
    }
}

impl<T, const CAPACITY: usize, const HOLE: usize> Drop for RecordSpace<T, CAPACITY, HOLE> {
    fn drop(&mut self) {
        if !self.is_reserved() {
            return;
        }
        let layout = Layout::array::<MaybeUninit<T>>(CAPACITY)
            .expect("record-space layout was validated at reservation");
        // The cursor's unallocated run is occupied but uninitialized: its ids are not dropped.
        let cursor = self.cursor;
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let id = (block_index * RECORDS_PER_BLOCK
                        + word_index * OCCUPANCY_WORD_BITS
                        + bit) as RecordId;
                    if (cursor.next..cursor.limit).contains(&id) {
                        live &= live - 1;
                        continue;
                    }
                    // SAFETY: every set bit outside the cursor's unallocated run denotes exactly one initialized record.
                    unsafe {
                        self.base
                            .as_ptr()
                            .add(id as usize)
                            .as_mut()
                            .unwrap()
                            .assume_init_drop()
                    };
                    live &= live - 1;
                }
            }
        }
        // SAFETY: this range was mapped by `ensure_reserved` with this layout.
        unsafe { unmap_range(self.base.cast(), layout) };
    }
}
