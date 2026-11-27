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
    /// The range is a slice of another owner's (`RecordSpace::over`): this space neither maps
    /// nor unmaps it.
    #[allow(dead_code)]
    Borrowed,
}

/// An address range its owner maps and releases, lazily backed as a record space's own range is.
/// The class space maps one and cuts its regions from it (`RecordSpace::over`).
#[allow(dead_code)]
pub(in crate::heap) struct AddressRange {
    base: NonNull<u8>,
    layout: Layout,
}

#[allow(dead_code)]
impl AddressRange {
    /// Maps `bytes` bytes, page aligned; `None` where the mapping fails.
    pub(in crate::heap) fn map(bytes: usize) -> Option<Self> {
        let layout = Layout::from_size_align(bytes, page_bytes()).ok().filter(|layout| layout.size() != 0)?;
        // SAFETY: the layout is nonzero-sized, and `unmap_range` in `Drop` releases it with the same layout.
        let base = unsafe { map_range(layout) }?;
        Some(Self { base, layout })
    }

    pub(in crate::heap) fn base(&self) -> NonNull<u8> {
        self.base
    }
}

impl Drop for AddressRange {
    fn drop(&mut self) {
        // SAFETY: the pair is the one `map` made, and the regions cut from it were dropped first.
        unsafe { unmap_range(self.base, self.layout) };
    }
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
        quench_alloc::map_anonymous(layout.size(), libc::PROT_READ | libc::PROT_WRITE, libc::MAP_NORESERVE)
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

/// The kernel's page size: `return_pages` ranges are whole pages of it.
#[cfg(target_os = "linux")]
pub(super) fn page_bytes() -> usize {
    // SAFETY: `sysconf` reads a constant of the system.
    usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap_or(4096)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn page_bytes() -> usize {
    4096
}

/// Gives the pages of `[start, start + bytes)` back to the kernel (`MADV_DONTNEED`): they stay
/// mapped and read back as zero, and the next write faults them in again. Elsewhere it does
/// nothing, and the pages stay resident.
///
/// # Safety
/// `start` is page aligned, `bytes` is a multiple of the page size, and the range lies within a
/// mapping `map_range` made. It holds no live record: no reference into it may be used until a
/// record is written there again.
#[cfg(target_os = "linux")]
pub(super) unsafe fn return_pages(start: *mut u8, bytes: usize) {
    // SAFETY: the caller's contract names whole pages of this space's mapping, none of them live.
    unsafe { libc::madvise(start.cast(), bytes, libc::MADV_DONTNEED) };
}

/// # Safety
/// As `return_pages` (on this platform the call does nothing).
#[cfg(not(target_os = "linux"))]
pub(super) unsafe fn return_pages(_start: *mut u8, _bytes: usize) {}

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

/// The fewest records a space reserves where its whole range cannot be mapped: the prototype
/// link window (`staged::object_encoding::PROTO_RECORD_WINDOW`), which every space kept before
/// the spaces grew past it.
const MIN_RESERVED_RECORDS: usize = crate::staged::object_encoding::PROTO_RECORD_WINDOW;

impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// A space of `records` records at `base`, a slice of an address range its owner maps and
    /// releases (`ClassSpace`). The space neither maps nor unmaps the range, and no reservation
    /// step runs: its records are addressable at once.
    ///
    /// # Safety
    /// `base` addresses `records` records of `T` inside a range that outlives the space, and no
    /// other space uses those records.
    #[allow(dead_code)]
    pub(in crate::heap) unsafe fn over(base: NonNull<MaybeUninit<T>>, records: usize) -> Self {
        // Set in place: a `Drop` type cannot be built by moving fields out of a temporary.
        let mut space = Self::default();
        space.base = base;
        space.reservation = Reservation::Borrowed;
        space.reserved = records;
        space.blocks.reserve_exact(records.div_ceil(RECORDS_PER_BLOCK));
        space
    }

    /// Whether `base` names its address range.
    #[inline(always)]
    pub(super) fn is_reserved(&self) -> bool {
        matches!(self.reservation, Reservation::Reserved | Reservation::Borrowed)
    }

    /// Reserves its address range on the first call; `false` where that failed.
    pub(super) fn ensure_reserved(&mut self) -> bool {
        match self.reservation {
            Reservation::Reserved | Reservation::Borrowed => return true,
            Reservation::Failed => return false,
            Reservation::Unreserved => {}
        }
        // The mapping is page aligned, so it serves any record alignment up to a page.
        const { assert!(std::mem::align_of::<MaybeUninit<T>>() <= 4096) };
        const { assert!(CAPACITY < u32::MAX as usize) };
        // The whole range where the address space allows it, else half of it, and so on down to
        // `MIN_RESERVED_RECORDS`: a process whose address space is limited (an address-space
        // rlimit, an instrumenting tool's own map) runs with smaller spaces rather than none.
        // A space with a hole (`HOLE`) keeps the ids past it, so it never maps less than them.
        let floor = CAPACITY.min(MIN_RESERVED_RECORDS.max(2 * HOLE));
        let mut records = CAPACITY;
        let base = loop {
            let mapped = Layout::array::<MaybeUninit<T>>(records).ok()
                // SAFETY: the layout is the nonzero record range; `Drop` releases it with the same layout.
                .and_then(|layout| unsafe { map_range(layout) });
            match mapped {
                Some(base) => break base,
                None if records > floor => records /= 2,
                None => {
                    self.reservation = Reservation::Failed;
                    return false;
                }
            }
        };
        self.base = base.cast();
        self.reserved = records;
        self.reservation = Reservation::Reserved;
        // The block metadata never moves either: room for every block the range can hold
        // is taken now, as address space only, and a block's pages are touched when it is
        // made. A pointer into it (`native_mark_blocks`) stays valid for the space's life.
        self.blocks.reserve_exact(records.div_ceil(RECORDS_PER_BLOCK));
        true
    }
}

impl<T, const CAPACITY: usize, const HOLE: usize> Drop for RecordSpace<T, CAPACITY, HOLE> {
    fn drop(&mut self) {
        if !self.is_reserved() {
            return;
        }
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
        // A borrowed range belongs to its owner, which unmaps it after the space is dropped.
        if self.reservation == Reservation::Reserved {
            let layout = Layout::array::<MaybeUninit<T>>(self.reserved)
                .expect("record-space layout was validated at reservation");
            // SAFETY: this range was mapped by `ensure_reserved` with this layout.
            unsafe { unmap_range(self.base.cast(), layout) };
        }
    }
}
