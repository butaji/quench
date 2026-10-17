//! Quench's small-object allocator.
//!
//! The runtime frees most of its small allocations in bulk when the collector sweeps dead
//! cells (element vectors, string units, environment slots, boxed cell payloads) and
//! allocates the same sizes again right after. A general-purpose allocator pays for size
//! search, coalescing and consolidation on each of those calls. Here every small size
//! class has its own pages, and a free is one push onto its page's free list.
//!
//! Layout: address space is reserved in `CHUNK_BYTES` chunks and faulted page by page, so
//! untouched pages are never resident. A `PAGE_BYTES` page holds one size class behind a
//! header. A page that becomes empty returns to a pool any class can take, so freed memory
//! is not stranded in one class. Pages are small (one base page) to keep the per-class
//! floor small: a heap touches at most one partly used page per class. Sizes above
//! `MAX_SMALL_BYTES`, alignments above `GRANULE_BYTES`, and any request after reservation
//! fails go to the system allocator. The size class is derived from the `Layout` passed to
//! `dealloc`, so no pointer is ever looked up.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

/// One allocator page: the unit a size class takes and returns.
pub const PAGE_BYTES: usize = 4096;
/// Block alignment and the step between the smallest size classes.
pub const GRANULE_BYTES: usize = 16;
/// The largest request served from size classes.
pub const MAX_SMALL_BYTES: usize = 1024;
/// Address space reserved at a time; pages are faulted in only when first used.
const CHUNK_BYTES: usize = 64 << 20;
/// Space before a page's first block, a multiple of the granule.
const PAGE_HEADER_BYTES: usize = 64;

/// Block sizes: every granule up to 256 bytes, then four steps per doubling.
pub const CLASS_SIZES: [usize; CLASS_COUNT] = [
    16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 320, 384, 448, 512,
    640, 768, 896, 1024,
];
pub const CLASS_COUNT: usize = 24;
/// The largest size served by granule-step classes.
const GRANULE_CLASSES_MAX_BYTES: usize = 256;

const _: () = assert!(PAGE_HEADER_BYTES >= std::mem::size_of::<Page>());
const _: () = assert!(PAGE_HEADER_BYTES % GRANULE_BYTES == 0);
const _: () = assert!(CLASS_SIZES[CLASS_COUNT - 1] == MAX_SMALL_BYTES);
const _: () = assert!(
    CLASS_SIZES[GRANULE_CLASSES_MAX_BYTES / GRANULE_BYTES - 1] == GRANULE_CLASSES_MAX_BYTES
);
const _: () = assert!(CHUNK_BYTES % PAGE_BYTES == 0);

/// The size class serving `size` bytes, or `None` above `MAX_SMALL_BYTES`.
pub fn class_of(size: usize) -> Option<usize> {
    if size <= GRANULE_CLASSES_MAX_BYTES {
        return Some(size.max(1).div_ceil(GRANULE_BYTES) - 1);
    }
    if size > MAX_SMALL_BYTES {
        return None;
    }
    let first = GRANULE_CLASSES_MAX_BYTES / GRANULE_BYTES;
    CLASS_SIZES[first..]
        .iter()
        .position(|class_size| size <= *class_size)
        .map(|offset| first + offset)
}

/// A freed block, linked through its first word.
struct FreeBlock {
    next: *mut FreeBlock,
}

/// The header at the start of every page.
#[repr(C)]
struct Page {
    /// Blocks freed back to this page.
    free: *mut FreeBlock,
    /// Offset of the first never-used block.
    bump: u32,
    /// Blocks handed out and not freed.
    used: u32,
    class: u32,
    /// Linked into its class's list of pages with free blocks.
    in_partial: bool,
    prev: *mut Page,
    next: *mut Page,
}

struct State {
    /// The page each class allocates from.
    current: [*mut Page; CLASS_COUNT],
    /// Pages other than `current` that have free blocks, per class.
    partial: [*mut Page; CLASS_COUNT],
    /// Empty pages, any class may take them; linked through `next`.
    empty: *mut Page,
    /// The reserved, never-used remainder of the current chunk.
    chunk_next: usize,
    chunk_end: usize,
}

/// The allocator. One instance serves a process as `#[global_allocator]`.
pub struct QuenchAlloc {
    locked: AtomicBool,
    state: UnsafeCell<State>,
}

// SAFETY: `state` is only accessed while `locked` is held.
unsafe impl Sync for QuenchAlloc {}

impl QuenchAlloc {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            state: UnsafeCell::new(State {
                current: [ptr::null_mut(); CLASS_COUNT],
                partial: [ptr::null_mut(); CLASS_COUNT],
                empty: ptr::null_mut(),
                chunk_next: 0,
                chunk_end: 0,
            }),
        }
    }

    #[inline(always)]
    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        // SAFETY: the lock gives exclusive access to the state.
        let result = f(unsafe { &mut *self.state.get() });
        self.locked.store(false, Ordering::Release);
        result
    }

    fn small_class(layout: Layout) -> Option<usize> {
        if layout.align() > GRANULE_BYTES {
            return None;
        }
        class_of(layout.size())
    }
}

impl Default for QuenchAlloc {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    /// A block of `class`, or null when no page can be had.
    #[inline(always)]
    unsafe fn allocate(&mut self, class: usize) -> *mut u8 {
        let page = self.current[class];
        if !page.is_null() {
            // SAFETY: `current` holds initialized pages of this class.
            let block = unsafe { take_block(page, class) };
            if !block.is_null() {
                return block;
            }
        }
        unsafe { self.allocate_slow(class) }
    }

    #[cold]
    unsafe fn allocate_slow(&mut self, class: usize) -> *mut u8 {
        let page = if !self.partial[class].is_null() {
            let page = self.partial[class];
            // SAFETY: partial pages are initialized and linked.
            unsafe { self.unlink_partial(page) };
            page
        } else {
            // SAFETY: a new page is reserved, writable memory.
            let page = unsafe { self.new_page() };
            if page.is_null() {
                return ptr::null_mut();
            }
            unsafe { init_page(page, class) };
            page
        };
        self.current[class] = page;
        // SAFETY: the page was just made current for this class and has a free block.
        unsafe { take_block(page, class) }
    }

    unsafe fn free(&mut self, block: *mut u8, class: usize) {
        let page = page_of(block);
        // SAFETY: the block came from `allocate` for this class, so its page is ours.
        unsafe {
            let free = block.cast::<FreeBlock>();
            (*free).next = (*page).free;
            (*page).free = free;
            (*page).used -= 1;
            if page == self.current[class] {
                return;
            }
            if (*page).used == 0 {
                if (*page).in_partial {
                    self.unlink_partial(page);
                }
                (*page).next = self.empty;
                self.empty = page;
            } else if !(*page).in_partial {
                self.link_partial(page, class);
            }
        }
    }

    unsafe fn new_page(&mut self) -> *mut Page {
        if !self.empty.is_null() {
            let page = self.empty;
            // SAFETY: empty pages are linked through `next`.
            self.empty = unsafe { (*page).next };
            return page;
        }
        if self.chunk_next == self.chunk_end {
            // SAFETY: an anonymous private mapping; pages fault in on first touch.
            let chunk = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    CHUNK_BYTES,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                    -1,
                    0,
                )
            };
            if chunk == libc::MAP_FAILED {
                return ptr::null_mut();
            }
            // mmap returns page-aligned addresses; PAGE_BYTES is one base page.
            self.chunk_next = chunk as usize;
            self.chunk_end = chunk as usize + CHUNK_BYTES;
        }
        let page = self.chunk_next as *mut Page;
        self.chunk_next += PAGE_BYTES;
        page
    }

    unsafe fn link_partial(&mut self, page: *mut Page, class: usize) {
        // SAFETY: `page` is an initialized page of `class` not in any list.
        unsafe {
            (*page).in_partial = true;
            (*page).prev = ptr::null_mut();
            (*page).next = self.partial[class];
            if !self.partial[class].is_null() {
                (*self.partial[class]).prev = page;
            }
        }
        self.partial[class] = page;
    }

    unsafe fn unlink_partial(&mut self, page: *mut Page) {
        // SAFETY: `page` is linked in its class's partial list.
        unsafe {
            let class = (*page).class as usize;
            if (*page).prev.is_null() {
                self.partial[class] = (*page).next;
            } else {
                (*(*page).prev).next = (*page).next;
            }
            if !(*page).next.is_null() {
                (*(*page).next).prev = (*page).prev;
            }
            (*page).in_partial = false;
            (*page).prev = ptr::null_mut();
            (*page).next = ptr::null_mut();
        }
    }
}

fn page_of(block: *mut u8) -> *mut Page {
    (block as usize & !(PAGE_BYTES - 1)) as *mut Page
}

unsafe fn init_page(page: *mut Page, class: usize) {
    // SAFETY: the caller owns `page`, a writable page.
    unsafe {
        page.write(Page {
            free: ptr::null_mut(),
            bump: PAGE_HEADER_BYTES as u32,
            used: 0,
            class: class as u32,
            in_partial: false,
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
        });
    }
}

/// A free block of the page, or null when it is full.
#[inline(always)]
unsafe fn take_block(page: *mut Page, class: usize) -> *mut u8 {
    // SAFETY: the caller passes an initialized page of `class`.
    unsafe {
        let free = (*page).free;
        if !free.is_null() {
            (*page).free = (*free).next;
            (*page).used += 1;
            return free.cast();
        }
        let size = CLASS_SIZES[class];
        let bump = (*page).bump as usize;
        if bump + size > PAGE_BYTES {
            return ptr::null_mut();
        }
        (*page).bump = (bump + size) as u32;
        (*page).used += 1;
        page.cast::<u8>().add(bump)
    }
}

unsafe impl GlobalAlloc for QuenchAlloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(class) = Self::small_class(layout) {
            // SAFETY: the state is accessed under the lock.
            let block = self.with_state(|state| unsafe { state.allocate(class) });
            // Null only when address space cannot be reserved: an allocation failure. The
            // system allocator cannot stand in, since `dealloc` routes by layout alone.
            return block;
        }
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    #[inline]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        if let Some(class) = Self::small_class(layout) {
            // SAFETY: a small layout's block came from `alloc` for the same class.
            self.with_state(|state| unsafe { state.free(block, class) });
            return;
        }
        // SAFETY: a large layout's block came from the system allocator.
        unsafe { System.dealloc(block, layout) }
    }

    #[inline]
    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_class = Self::small_class(layout);
        let new_layout = match Layout::from_size_align(new_size, layout.align()) {
            Ok(new_layout) => new_layout,
            Err(_) => return ptr::null_mut(),
        };
        let new_class = Self::small_class(new_layout);
        if old_class.is_some() && old_class == new_class {
            return block;
        }
        if old_class.is_none() && new_class.is_none() {
            // SAFETY: both sides are the system allocator's.
            return unsafe { System.realloc(block, layout, new_size) };
        }
        // SAFETY: a fresh block of the new layout; the old one is valid for its size.
        unsafe {
            let new_block = self.alloc(new_layout);
            if !new_block.is_null() {
                ptr::copy_nonoverlapping(block, new_block, layout.size().min(new_size));
                self.dealloc(block, layout);
            }
            new_block
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_every_small_size_exactly_once() {
        let mut previous = 0;
        for size in 1..=MAX_SMALL_BYTES {
            let class = class_of(size).unwrap();
            assert!(CLASS_SIZES[class] >= size);
            assert!(class == 0 || CLASS_SIZES[class - 1] < size);
            assert!(class >= previous);
            previous = class;
        }
        assert_eq!(class_of(MAX_SMALL_BYTES + 1), None);
        assert!(CLASS_SIZES.iter().all(|size| size % GRANULE_BYTES == 0));
    }

    #[test]
    fn blocks_are_reused_and_empty_pages_change_class() {
        let allocator = QuenchAlloc::new();
        let small = Layout::from_size_align(24, 8).unwrap();
        let blocks: Vec<*mut u8> = (0..1000)
            .map(|index| unsafe {
                let block = allocator.alloc(small);
                assert!(!block.is_null());
                assert_eq!(block as usize % GRANULE_BYTES, 0);
                block.write_bytes(index as u8, small.size());
                block
            })
            .collect();
        for (index, block) in blocks.iter().enumerate() {
            assert!(unsafe { std::slice::from_raw_parts(*block, small.size()) }
                .iter()
                .all(|byte| *byte == index as u8));
        }
        for block in &blocks {
            unsafe { allocator.dealloc(*block, small) };
        }
        let state = unsafe { &*allocator.state.get() };
        let reserved_before = state.chunk_next;
        // Every page but the current one went back to the empty pool, so another class
        // takes them without reserving more.
        let large = Layout::from_size_align(1000, 16).unwrap();
        let pages_freed = blocks.len() / ((PAGE_BYTES - PAGE_HEADER_BYTES) / 32) - 1;
        let reused: Vec<*mut u8> = (0..pages_freed * ((PAGE_BYTES - PAGE_HEADER_BYTES) / 1024))
            .map(|_| unsafe { allocator.alloc(large) })
            .collect();
        let state = unsafe { &*allocator.state.get() };
        assert_eq!(state.chunk_next, reserved_before);
        for block in reused {
            unsafe { allocator.dealloc(block, large) };
        }
    }

    #[test]
    fn realloc_moves_between_classes_and_the_system_allocator() {
        let allocator = QuenchAlloc::new();
        let layout = Layout::from_size_align(40, 8).unwrap();
        unsafe {
            let block = allocator.alloc(layout);
            block.write_bytes(7, 40);
            let same = allocator.realloc(block, layout, 48);
            assert_eq!(same, block);
            let grown = allocator.realloc(same, Layout::from_size_align(48, 8).unwrap(), 4000);
            assert!(std::slice::from_raw_parts(grown, 40)
                .iter()
                .all(|byte| *byte == 7));
            let shrunk = allocator.realloc(grown, Layout::from_size_align(4000, 8).unwrap(), 100);
            assert!(std::slice::from_raw_parts(shrunk, 40)
                .iter()
                .all(|byte| *byte == 7));
            allocator.dealloc(shrunk, Layout::from_size_align(100, 8).unwrap());
        }
    }
}
