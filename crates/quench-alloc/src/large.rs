//! A small per-thread cache of large blocks that a program frees and requests again.
//!
//! glibc maps every block from `LARGE_MIN_BYTES` up on its own and unmaps it when freed, so
//! a multi-megabyte result string rebuilt each cycle page-faults the whole block each time.
//! Keeping such a block for a collection or two lets the next request reuse it, while a block
//! that does not recur goes back to the system within `MAX_IDLE_COLLECTIONS` collections.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr;
use std::sync::atomic::Ordering;

use super::ENABLED;

/// The process's mmap threshold: smaller blocks are the arena's and are already reused.
const LARGE_MIN_BYTES: usize = 1 << 20;
/// Larger blocks are rare one-offs such as Wasm memories; keeping one resident would cost
/// more than the faults it saves.
const LARGE_MAX_BYTES: usize = 32 << 20;
/// Blocks kept at once per thread; a cycle rarely recurs with more distinct large blocks.
const SLOTS: usize = 4;
/// Collections a kept block may go unused before it is returned to the system, so only a
/// block that recurs stays resident, and only briefly.
const MAX_IDLE_COLLECTIONS: u32 = 2;
/// A kept block serves a request at most `1 / 2^FIT_SHIFT` smaller than itself, so it is not
/// held resident for much smaller requests.
const FIT_SHIFT: u32 = 3;
/// The largest alignment served: what system malloc guarantees on x86_64 and aarch64, so a
/// kept block's layout can be rebuilt from its size alone.
const MAX_ALIGN: usize = std::mem::align_of::<u128>();

/// One kept block; a null `block` is an empty slot.
struct Slot {
    block: Cell<*mut u8>,
    bytes: Cell<usize>,
    idle: Cell<u32>,
}

impl Slot {
    const fn empty() -> Self {
        Self { block: Cell::new(ptr::null_mut()), bytes: Cell::new(0), idle: Cell::new(0) }
    }

    fn is_empty(&self) -> bool {
        self.block.get().is_null()
    }

    fn fits(&self, size: usize) -> bool {
        let bytes = self.bytes.get();
        !self.is_empty() && bytes >= size && bytes - size <= bytes >> FIT_SHIFT
    }

    fn fill(&self, block: *mut u8, bytes: usize) {
        self.block.set(block);
        self.bytes.set(bytes);
        self.idle.set(0);
    }

    /// Returns the block to the system and empties the slot.
    fn release(&self) {
        // SAFETY: a kept block came from the system allocator with this size and an alignment
        // of at most `MAX_ALIGN`, which the system serves identically to any smaller one.
        unsafe { System.dealloc(self.block.get(), Layout::from_size_align_unchecked(self.bytes.get(), MAX_ALIGN)) };
        self.block.set(ptr::null_mut());
    }
}

thread_local! {
    static KEPT: [Slot; SLOTS] = const { [const { Slot::empty() }; SLOTS] };
    /// Whether this thread's collections age its kept blocks (`age`). A thread that never
    /// collects (a code generator's) would keep its blocks for good, so it keeps none.
    static AGES: Cell<bool> = const { Cell::new(false) };
}

fn eligible(layout: Layout) -> bool {
    (LARGE_MIN_BYTES..=LARGE_MAX_BYTES).contains(&layout.size()) && layout.align() <= MAX_ALIGN
}

/// A kept block that serves `layout`, else the system allocator's.
///
/// # Safety
/// As `GlobalAlloc::alloc`.
pub(crate) unsafe fn alloc(layout: Layout) -> *mut u8 {
    let block = take(layout);
    if !block.is_null() {
        return block;
    }
    // SAFETY: forwarded unchanged.
    unsafe { System.alloc(layout) }
}

/// Keeps a system block for a later `alloc` once the switch is on, else frees it.
///
/// # Safety
/// As `GlobalAlloc::dealloc`, for a block of the system allocator.
pub(crate) unsafe fn free(block: *mut u8, layout: Layout) {
    if ENABLED.load(Ordering::Relaxed) && keep(block, layout) {
        return;
    }
    // SAFETY: forwarded unchanged.
    unsafe { System.dealloc(block, layout) }
}

/// A kept block that serves `layout`, or null. The cache is empty during thread teardown.
fn take(layout: Layout) -> *mut u8 {
    if !eligible(layout) {
        return ptr::null_mut();
    }
    KEPT
        .try_with(|slots| match slots.iter().find(|slot| slot.fits(layout.size())) {
            Some(slot) => slot.block.replace(ptr::null_mut()),
            None => ptr::null_mut(),
        })
        .unwrap_or(ptr::null_mut())
}

/// Keeps `block` for a later `take`; false when it stays the caller's to free: ineligible,
/// or every slot holds a block not yet idle through a collection.
fn keep(block: *mut u8, layout: Layout) -> bool {
    if !eligible(layout) || !AGES.try_with(Cell::get).unwrap_or(false) {
        return false;
    }
    KEPT
        .try_with(|slots| {
            let slot = slots
                .iter()
                .find(|slot| slot.is_empty())
                .or_else(|| slots.iter().max_by_key(|slot| slot.idle.get()).filter(|slot| slot.idle.get() > 0));
            let Some(slot) = slot else { return false };
            if !slot.is_empty() {
                slot.release();
            }
            slot.fill(block, layout.size());
            true
        })
        .unwrap_or(false)
}

/// Ages every kept block by one collection and returns those idle for too long.
pub(crate) fn age() {
    // A failed access is thread teardown, which has nothing kept to age.
    let _ = AGES.try_with(|ages| ages.set(true));
    let _ = KEPT.try_with(|slots| {
        for slot in slots.iter().filter(|slot| !slot.is_empty()) {
            slot.idle.set(slot.idle.get() + 1);
            if slot.idle.get() >= MAX_IDLE_COLLECTIONS {
                slot.release();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(size: usize) -> Layout {
        Layout::from_size_align(size, MAX_ALIGN).unwrap()
    }

    fn system_block(size: usize) -> *mut u8 {
        // SAFETY: a non-zero layout.
        let block = unsafe { System.alloc(layout(size)) };
        assert!(!block.is_null());
        block
    }

    fn free(block: *mut u8, size: usize) {
        // SAFETY: a block of `system_block` with this size.
        unsafe { System.dealloc(block, layout(size)) };
    }

    fn drain() {
        for _ in 0..MAX_IDLE_COLLECTIONS {
            age();
        }
    }

    #[test]
    fn a_kept_block_is_taken_back_for_an_equal_request() {
        age();
        let block = system_block(LARGE_MIN_BYTES);
        assert!(keep(block, layout(LARGE_MIN_BYTES)));
        assert_eq!(take(layout(LARGE_MIN_BYTES)), block);
        assert!(take(layout(LARGE_MIN_BYTES)).is_null());
        free(block, LARGE_MIN_BYTES);
    }

    #[test]
    fn a_much_smaller_request_does_not_take_the_block() {
        age();
        let size = LARGE_MIN_BYTES * 2;
        let block = system_block(size);
        assert!(keep(block, layout(size)));
        assert!(take(layout(size - (size >> FIT_SHIFT) - 1)).is_null());
        assert_eq!(take(layout(size - (size >> FIT_SHIFT))), block);
        free(block, size);
    }

    #[test]
    fn a_block_ages_out_after_the_idle_limit() {
        age();
        let block = system_block(LARGE_MIN_BYTES);
        assert!(keep(block, layout(LARGE_MIN_BYTES)));
        for _ in 1..MAX_IDLE_COLLECTIONS {
            age();
        }
        assert_eq!(take(layout(LARGE_MIN_BYTES)), block);
        assert!(keep(block, layout(LARGE_MIN_BYTES)));
        drain();
        assert!(take(layout(LARGE_MIN_BYTES)).is_null());
    }

    #[test]
    fn sizes_outside_the_range_are_never_kept() {
        age();
        for size in [LARGE_MIN_BYTES - 1, LARGE_MAX_BYTES + 1] {
            let block = system_block(size);
            assert!(!keep(block, layout(size)));
            free(block, size);
        }
    }

    #[test]
    fn full_slots_of_fresh_blocks_refuse_another() {
        age();
        let blocks: Vec<_> = (0..SLOTS).map(|_| system_block(LARGE_MIN_BYTES)).collect();
        assert!(blocks.iter().all(|&block| keep(block, layout(LARGE_MIN_BYTES))));
        let extra = system_block(LARGE_MIN_BYTES);
        assert!(!keep(extra, layout(LARGE_MIN_BYTES)));
        free(extra, LARGE_MIN_BYTES);
        drain();
    }

    #[test]
    fn a_full_cache_evicts_its_oldest_block() {
        age();
        let blocks: Vec<_> = (0..SLOTS).map(|_| system_block(LARGE_MIN_BYTES)).collect();
        assert!(blocks.iter().all(|&block| keep(block, layout(LARGE_MIN_BYTES))));
        age();
        let extra = system_block(LARGE_MIN_BYTES);
        assert!(keep(extra, layout(LARGE_MIN_BYTES)));
        drain();
    }

    #[test]
    fn a_thread_that_has_not_collected_keeps_nothing() {
        let block = system_block(LARGE_MIN_BYTES);
        assert!(!keep(block, layout(LARGE_MIN_BYTES)));
        free(block, LARGE_MIN_BYTES);
    }
}
