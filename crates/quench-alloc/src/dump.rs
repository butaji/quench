//! The dump arena: while a process dumps (`begin_dump`), every allocation bumps through one
//! range at a fixed address, so the memory its state reaches lies in that range and in the
//! fixed reservations (`fixed_space`), and a later run of the same binary can map it back at
//! the same addresses. A block in the range is never freed: a free of one is
//! dropped and a reallocation copies it out, so the arena needs no allocator state of its own
//! and the dumped heaps need not be adopted. What the run frees of the dump stays mapped,
//! bounded by the dump's size; a page of it the run never touches is never resident.

use std::alloc::Layout;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The arena's address: below the fixed reservations (`fixed_space`), above a no-PIE
/// executable and its heap.
pub const DUMP_BASE: usize = 0x0f00_0000_0000;
/// The arena's reservation; only what a dump uses is ever touched.
const DUMP_BYTES: usize = 1 << 34;

#[cfg(target_os = "linux")]
const MAP_FIXED_NOREPLACE: libc::c_int = 0x100000;

/// The next free byte while dumping; `DUMP_BASE` before.
static TOP: AtomicUsize = AtomicUsize::new(DUMP_BASE);
/// Whether allocations bump through the arena.
pub(crate) static DUMPING: AtomicBool = AtomicBool::new(false);

/// Whether `block` lies in the arena: a constant range compare.
#[inline(always)]
pub fn owns_dump(block: *const u8) -> bool {
    (block as usize).wrapping_sub(DUMP_BASE) < DUMP_BYTES
}

/// Starts serving every allocation from the arena, reserving it; false where its range cannot
/// be had. Call before the state to dump is made, on a process that has not enabled the
/// small-object allocator (`QuenchAlloc::enable`), whose blocks a dump could not hold.
pub fn begin_dump() -> bool {
    if super::ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    // SAFETY: `MAP_FIXED_NOREPLACE` maps only where nothing is mapped.
    let mapped = unsafe {
        libc::mmap(DUMP_BASE as *mut libc::c_void, DUMP_BYTES, libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | MAP_FIXED_NOREPLACE, -1, 0)
    };
    if mapped as usize != DUMP_BASE {
        return false;
    }
    TOP.store(DUMP_BASE, Ordering::Relaxed);
    DUMPING.store(true, Ordering::Release);
    true
}

/// Stops dumping; the arena's used span, from `DUMP_BASE`.
pub fn end_dump() -> (usize, usize) {
    DUMPING.store(false, Ordering::Release);
    (DUMP_BASE, TOP.load(Ordering::Relaxed) - DUMP_BASE)
}

/// A block of the arena for `layout`, or null once it is spent.
pub(crate) fn alloc(layout: Layout) -> *mut u8 {
    let align = layout.align().max(16);
    let mut top = TOP.load(Ordering::Relaxed);
    loop {
        let start = top.next_multiple_of(align);
        let end = start + layout.size().max(1);
        if end > DUMP_BASE + DUMP_BYTES {
            return std::ptr::null_mut();
        }
        match TOP.compare_exchange_weak(top, end, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return start as *mut u8,
            Err(seen) => top = seen,
        }
    }
}
