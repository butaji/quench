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
/// No fixed no-replace mapping outside Linux (`begin_dump` refuses there): the dump reads and
/// writes ELF state, which only a Linux image has.
#[cfg(not(target_os = "linux"))]
const MAP_FIXED_NOREPLACE: libc::c_int = 0;

/// One bit per `GRANULE` of the arena, set where the run freed the block that held it: the
/// dump leaves those bytes out (`freed_granules`), which no live state reads. Mapped beside the
/// arena while dumping.
const FREED_BASE: usize = DUMP_BASE + DUMP_BYTES;
const GRANULE: usize = 16;

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
    // The dump's maps and readers are Linux's (a fixed arena, an ELF image).
    if !cfg!(target_os = "linux") || super::ENABLED.load(Ordering::Relaxed) {
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
    // SAFETY: as above, beside the arena.
    let freed = unsafe {
        libc::mmap(FREED_BASE as *mut libc::c_void, DUMP_BYTES / GRANULE / 8, libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | MAP_FIXED_NOREPLACE, -1, 0)
    };
    if freed as usize != FREED_BASE {
        // SAFETY: the arena just mapped, which nothing names yet.
        unsafe { libc::munmap(mapped, DUMP_BYTES) };
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

/// Notes that the run freed the arena's `[block, block + size)` (`freed_granules`).
pub(crate) fn note_free(block: *const u8, size: usize) {
    if !DUMPING.load(Ordering::Relaxed) {
        return;
    }
    // Blocks start on a granule (`alloc` aligns to 16); a partial last granule stays.
    let first = (block as usize - DUMP_BASE) / GRANULE;
    let last = first + size / GRANULE;
    let bits = FREED_BASE as *mut u8;
    for granule in first..last {
        // SAFETY: the bitmap covers the whole arena and only this thread writes it while it
        // frees (the dumping process's allocations are serialized by its single worker).
        unsafe { *bits.add(granule / 8) |= 1 << (granule % 8) };
    }
}

/// Which of the 256 granules of the arena page at `page` the run freed, one bit each; all
/// clear once the dump has ended without a bitmap.
pub fn freed_granules(page: usize) -> [u64; 4] {
    let mut out = [0u64; 4];
    if !owns_dump(page as *const u8) || TOP.load(Ordering::Relaxed) == DUMP_BASE {
        return out;
    }
    let first = (page - DUMP_BASE) / GRANULE / 8;
    for (index, word) in out.iter_mut().enumerate() {
        // SAFETY: inside the bitmap `begin_dump` mapped, which stays mapped for the process.
        *word = unsafe { std::ptr::read_unaligned((FREED_BASE + first + index * 8) as *const u64) };
    }
    out
}

/// Starts the arena's next allocation on a fresh page while dumping: a phase of the dumped
/// run (a section of a restored snapshot) then starts on the same page offset in every
/// dump of the binary, whatever the phases before it allocated, so its pages are the same
/// bytes in two dumps whose earlier phases differ and are stored once (`aot::image_dump`).
pub fn align_dump_phase() {
    if DUMPING.load(Ordering::Relaxed) {
        let top = TOP.load(Ordering::Relaxed);
        TOP.store(top.next_multiple_of(PHASE_ALIGN), Ordering::Relaxed);
    }
}

/// A phase's alignment in the arena (`align_dump_phase`).
const PHASE_ALIGN: usize = 4096;

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
