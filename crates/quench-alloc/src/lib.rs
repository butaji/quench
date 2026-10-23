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
//! fails go to the system allocator. Every chunk comes from one reservation made once per
//! process (at most `REGION_BYTES`, halved until the host grants it), so whether a block is
//! this allocator's is one address-range compare
//! (`owns`): a free goes to whichever allocator holds the block. That lets a process serve
//! everything from the system allocator until it chooses this one (`QuenchAlloc::enable`),
//! and lets an allocation fall back to the system allocator when no page can be had.
//!
//! Threads: each thread allocates from its own heap, so neither path takes a lock or an
//! atomic read-modify-write. A page belongs to the heap that took it (`Page::owner`); a
//! free on another thread pushes the block onto the page's `remote_free` list and queues the
//! page on its owner's `remote_pages` once, and the owner merges those blocks in its slow
//! path. A thread's heap outlives the thread: at exit it is abandoned, pages and all, and
//! the next new thread adopts it. Allocation on a thread whose heap is already gone (late
//! thread-exit destructors) uses one shared heap behind a lock.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

/// One allocator page: the unit a size class takes and returns.
pub const PAGE_BYTES: usize = 4096;
/// Block alignment and the step between the smallest size classes.
pub const GRANULE_BYTES: usize = 16;
/// The largest request served from size classes.
pub const MAX_SMALL_BYTES: usize = 1024;
/// Address space a heap takes at a time; pages are faulted in only when first used.
const CHUNK_BYTES: usize = 64 << 20;
/// Address space reserved once, inaccessible until carved into chunks: every page this
/// allocator hands out lies inside it (`owns`). It reserves addresses, not memory; a process
/// that needs more than this many chunks serves the rest from the system allocator. A host
/// that refuses this much address space (an address-space limit, or an instrumenting
/// emulator such as Valgrind) is asked for half as much, down to one chunk.
const REGION_BYTES: usize = 1 << 36;
const _: () = assert!(REGION_BYTES.is_multiple_of(CHUNK_BYTES));
const _: () = assert!(REGION_BYTES.is_power_of_two() && CHUNK_BYTES.is_power_of_two());
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
    /// Blocks freed back to this page by its owner.
    free: *mut FreeBlock,
    /// Offset of the first never-used block.
    bump: u32,
    /// Blocks handed out and not freed back to the owner (remote frees count until merged).
    used: u32,
    class: u8,
    /// Linked into its class's list of pages with free blocks.
    in_partial: bool,
    /// Linked into the owner's `remote_pages`, so another remote free need not link it.
    remote_queued: AtomicBool,
    prev: *mut Page,
    next: *mut Page,
    /// The heap whose thread allocates from this page.
    owner: *mut Heap,
    /// Blocks other threads freed, pushed atomically; the owner merges them.
    remote_free: AtomicPtr<FreeBlock>,
    /// The next page in the owner's `remote_pages`.
    remote_next: AtomicPtr<Page>,
}

/// One thread's allocation state.
struct Heap {
    /// The page each class allocates from.
    current: [*mut Page; CLASS_COUNT],
    /// Pages other than `current` that have free blocks, per class.
    partial: [*mut Page; CLASS_COUNT],
    /// Empty pages, any class may take them; linked through `next`.
    empty: *mut Page,
    /// The reserved, never-used remainder of the current chunk.
    chunk_next: usize,
    chunk_end: usize,
    /// Pages of this heap with blocks in `remote_free`, linked through `remote_next`.
    remote_pages: AtomicPtr<Page>,
    /// The next heap in `ABANDONED`.
    next_abandoned: *mut Heap,
}

/// A heap and the flag that serializes its use, for heaps no single thread owns.
struct LockedHeap {
    locked: AtomicBool,
    heap: UnsafeCell<Heap>,
}

// SAFETY: the heap is only accessed while `locked` is held.
unsafe impl Sync for LockedHeap {}

impl LockedHeap {
    fn with<R>(&self, f: impl FnOnce(*mut Heap) -> R) -> R {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        let result = f(self.heap.get());
        self.locked.store(false, Ordering::Release);
        result
    }
}

/// Serves threads whose own heap is already abandoned.
static ORPHAN: LockedHeap = LockedHeap {
    locked: AtomicBool::new(false),
    heap: UnsafeCell::new(Heap::EMPTY),
};

/// Heaps of exited threads, linked through `next_abandoned`; the lock flag and the head.
static ABANDONED_LOCK: AtomicBool = AtomicBool::new(false);
static ABANDONED: AtomicPtr<Heap> = AtomicPtr::new(ptr::null_mut());

thread_local! {
    /// This thread's heap; null before its first allocation and after it exits.
    static HEAP: Cell<*mut Heap> = const { Cell::new(ptr::null_mut()) };
    /// Abandons the thread's heap when the thread exits.
    static EXIT: ThreadExit = const { ThreadExit };
}

struct ThreadExit;

impl Drop for ThreadExit {
    fn drop(&mut self) {
        let heap = HEAP.replace(ptr::null_mut());
        if !heap.is_null() {
            with_abandoned(|head| {
                // SAFETY: the exiting thread no longer uses its heap.
                unsafe { (*heap).next_abandoned = *head };
                *head = heap;
            });
        }
    }
}

fn with_abandoned<R>(f: impl FnOnce(&mut *mut Heap) -> R) -> R {
    while ABANDONED_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
    let mut head = ABANDONED.load(Ordering::Relaxed);
    let result = f(&mut head);
    ABANDONED.store(head, Ordering::Relaxed);
    ABANDONED_LOCK.store(false, Ordering::Release);
    result
}

/// This thread's heap, adopting an abandoned one or making one on first use; null once the
/// thread's exit has abandoned it.
#[inline(always)]
fn thread_heap() -> *mut Heap {
    let heap = HEAP.get();
    if !heap.is_null() {
        return heap;
    }
    thread_heap_slow()
}

#[cold]
fn thread_heap_slow() -> *mut Heap {
    // A thread past its exit guard keeps no heap of its own: one made now would never be
    // abandoned.
    if EXIT.try_with(|_| ()).is_err() {
        return ptr::null_mut();
    }
    let adopted = with_abandoned(|head| {
        let heap = *head;
        if !heap.is_null() {
            // SAFETY: abandoned heaps are linked through `next_abandoned`.
            *head = unsafe { (*heap).next_abandoned };
        }
        heap
    });
    let heap = if adopted.is_null() {
        // SAFETY: an anonymous private mapping sized for one heap, owned from now on.
        let mapped = unsafe {
            libc::mmap(
                ptr::null_mut(),
                std::mem::size_of::<Heap>(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if mapped == libc::MAP_FAILED {
            return ptr::null_mut();
        }
        let heap = mapped.cast::<Heap>();
        // SAFETY: fresh writable memory sized and aligned (page-aligned) for a heap.
        unsafe { heap.write(Heap::EMPTY) };
        heap
    } else {
        adopted
    };
    HEAP.set(heap);
    heap
}

/// The reservation's first address, or zero before it is made (or after it failed).
static REGION_BASE: AtomicUsize = AtomicUsize::new(0);
/// The reservation's length, stored before its base is published; zero before it is made.
static REGION_LEN: AtomicUsize = AtomicUsize::new(0);
/// The one attempt at the reservation, so a failure is not retried on every page. Threads
/// that ask while it runs wait for it: one that read "attempted" before the base was stored
/// had taken no chunk and served its request from the system allocator (seen as a parallel
/// test's heap reserving a page its own frees should have covered).
static REGION_RESERVED: std::sync::Once = std::sync::Once::new();
/// The offset in the reservation of the next chunk any heap takes.
static REGION_NEXT: AtomicUsize = AtomicUsize::new(0);
/// Whether small requests are served here (`QuenchAlloc::enable`); before, every request
/// goes to the system allocator.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// The reservation's base, reserving it on first use; zero if it failed.
#[cold]
fn region_base() -> usize {
    // `Once` waits on a futex and never allocates, so the allocator may use it.
    REGION_RESERVED.call_once(reserve_region);
    REGION_BASE.load(Ordering::Acquire)
}

/// Reserves the region, halving the request down to one chunk until the kernel grants it.
fn reserve_region() {
    let mut length = REGION_BYTES;
    loop {
        // SAFETY: a fresh inaccessible anonymous reservation; nothing else uses its addresses.
        let reserved = unsafe {
            libc::mmap(
                ptr::null_mut(),
                length,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if reserved != libc::MAP_FAILED {
            REGION_LEN.store(length, Ordering::Relaxed);
            REGION_BASE.store(reserved as usize, Ordering::Release);
            return;
        }
        if length == CHUNK_BYTES {
            return;
        }
        length /= 2;
    }
}

/// A fresh chunk of the reservation, made writable, or `None` once it is spent.
#[cold]
fn take_chunk() -> Option<usize> {
    let base = region_base();
    if base == 0 {
        return None;
    }
    let offset = REGION_NEXT.fetch_add(CHUNK_BYTES, Ordering::Relaxed);
    if offset >= REGION_LEN.load(Ordering::Relaxed) {
        return None;
    }
    let chunk = base + offset;
    // SAFETY: a chunk of the reservation no heap has taken; pages fault in on first touch.
    let protected = unsafe {
        libc::mprotect(chunk as *mut libc::c_void, CHUNK_BYTES, libc::PROT_READ | libc::PROT_WRITE)
    };
    (protected == 0).then_some(chunk)
}

/// Whether `block` lies in this allocator's pages.
#[inline(always)]
pub fn owns(block: *const u8) -> bool {
    // A block of the reservation reached this thread after its length was stored.
    let base = REGION_BASE.load(Ordering::Relaxed);
    base != 0 && (block as usize).wrapping_sub(base) < REGION_LEN.load(Ordering::Relaxed)
}

/// The allocator. One instance serves a process as `#[global_allocator]`; its state lives in
/// per-thread heaps, so every instance shares it.
pub struct QuenchAlloc;

impl QuenchAlloc {
    pub const fn new() -> Self {
        Self
    }

    /// Serves small requests from now on; until called, every request goes to the system
    /// allocator. Blocks either allocator handed out are freed by it (`owns`).
    pub fn enable() {
        ENABLED.store(true, Ordering::Relaxed);
    }

    fn small_class(layout: Layout) -> Option<usize> {
        if layout.align() > GRANULE_BYTES {
            return None;
        }
        class_of(layout.size())
    }

    /// `alloc` once the switch is on: a small request from its class, any other (or one no
    /// page can serve) from the system allocator, which then frees it (`owns`).
    #[inline(never)]
    unsafe fn alloc_enabled(layout: Layout) -> *mut u8 {
        if let Some(class) = Self::small_class(layout) {
            // SAFETY: a small class.
            let block = unsafe { allocate(class) };
            if !block.is_null() {
                return block;
            }
        }
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc(layout) }
    }
}

impl Default for QuenchAlloc {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    const EMPTY: Self = Self {
        current: [ptr::null_mut(); CLASS_COUNT],
        partial: [ptr::null_mut(); CLASS_COUNT],
        empty: ptr::null_mut(),
        chunk_next: 0,
        chunk_end: 0,
        remote_pages: AtomicPtr::new(ptr::null_mut()),
        next_abandoned: ptr::null_mut(),
    };

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
        // SAFETY: called by the heap's owner.
        unsafe { self.collect_remote() };
        let current = self.current[class];
        if !current.is_null() {
            // SAFETY: `current` holds initialized pages of this class.
            let block = unsafe { take_block(current, class) };
            if !block.is_null() {
                return block;
            }
        }
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
            unsafe { init_page(page, class, self) };
            page
        };
        self.current[class] = page;
        // SAFETY: the page was just made current for this class and has a free block.
        unsafe { take_block(page, class) }
    }

    /// Frees a block of one of this heap's pages, on its owner's thread.
    unsafe fn free(&mut self, page: *mut Page, block: *mut u8) {
        // SAFETY: the block came from `allocate` for the page's class, and the page is ours.
        unsafe {
            let free = block.cast::<FreeBlock>();
            (*free).next = (*page).free;
            (*page).free = free;
            (*page).used -= 1;
            self.settle(page);
        }
    }

    /// Merges the blocks other threads freed to this heap's pages.
    unsafe fn collect_remote(&mut self) {
        let mut page = self.remote_pages.swap(ptr::null_mut(), Ordering::Acquire);
        while !page.is_null() {
            // SAFETY: queued pages are this heap's; the queue link is read before the page
            // may be queued again.
            unsafe {
                let next = (*page).remote_next.load(Ordering::Relaxed);
                // Cleared before the blocks are taken: a free pushed after the swap below
                // queues the page again.
                (*page).remote_queued.store(false, Ordering::SeqCst);
                let mut block = (*page).remote_free.swap(ptr::null_mut(), Ordering::SeqCst);
                if !block.is_null() {
                    while !block.is_null() {
                        let following = (*block).next;
                        (*block).next = (*page).free;
                        (*page).free = block;
                        (*page).used -= 1;
                        block = following;
                    }
                    self.settle(page);
                }
                page = next;
            }
        }
    }

    /// Files a page that just gained free blocks: the empty pool when it has none in use,
    /// else its class's partial list, unless it is the class's current page.
    unsafe fn settle(&mut self, page: *mut Page) {
        // SAFETY: an initialized page of this heap.
        unsafe {
            let class = usize::from((*page).class);
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
            let Some(chunk) = take_chunk() else {
                return ptr::null_mut();
            };
            // The reservation is page-aligned and chunks are whole pages.
            self.chunk_next = chunk;
            self.chunk_end = chunk + CHUNK_BYTES;
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
            let class = usize::from((*page).class);
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

unsafe fn init_page(page: *mut Page, class: usize, owner: *mut Heap) {
    // SAFETY: the caller owns `page`, a writable page.
    unsafe {
        page.write(Page {
            free: ptr::null_mut(),
            bump: PAGE_HEADER_BYTES as u32,
            used: 0,
            class: class as u8,
            in_partial: false,
            remote_queued: AtomicBool::new(false),
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
            owner,
            remote_free: AtomicPtr::new(ptr::null_mut()),
            remote_next: AtomicPtr::new(ptr::null_mut()),
        });
    }
}

/// Frees a block of a page another heap owns: the block joins the page's `remote_free`, and
/// the page its owner's `remote_pages` unless it is queued already.
unsafe fn free_remote(page: *mut Page, block: *mut u8) {
    // SAFETY: the block is allocated from `page`, whose owner heap outlives its pages.
    unsafe {
        let free = block.cast::<FreeBlock>();
        let mut head = (*page).remote_free.load(Ordering::Relaxed);
        loop {
            (*free).next = head;
            match (*page).remote_free.compare_exchange_weak(
                head,
                free,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => head = actual,
            }
        }
        if (*page).remote_queued.swap(true, Ordering::SeqCst) {
            return;
        }
        let pages = &(*(*page).owner).remote_pages;
        let mut head = pages.load(Ordering::Relaxed);
        loop {
            (*page).remote_next.store(head, Ordering::Relaxed);
            match pages.compare_exchange_weak(head, page, Ordering::Release, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => head = actual,
            }
        }
    }
}

/// Allocates a block of `class` from this thread's heap, or the shared one past its exit.
#[inline(always)]
unsafe fn allocate(class: usize) -> *mut u8 {
    let heap = thread_heap();
    if heap.is_null() {
        // SAFETY: the shared heap is used under its lock.
        return ORPHAN.with(|heap| unsafe { (*heap).allocate(class) });
    }
    // SAFETY: the thread's own heap, used only by this thread.
    unsafe { (*heap).allocate(class) }
}

/// Frees a small block: on the owner's thread directly, else through the page's remote list.
#[inline(always)]
unsafe fn free(block: *mut u8) {
    let page = page_of(block);
    let heap = HEAP.get();
    // SAFETY: the block is allocated from `page`; a heap the thread owns is used only by it.
    unsafe {
        if !heap.is_null() && (*page).owner == heap {
            (*heap).free(page, block);
        } else {
            // Including the shared heap's pages: whoever holds its lock next merges them.
            free_remote(page, block);
        }
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
        // The switch first, and the enabled path out of line: until the switch is on, a
        // request costs one load and a jump to the system allocator, with no frame of its own.
        if ENABLED.load(Ordering::Relaxed) {
            // SAFETY: forwarded unchanged.
            return unsafe { Self::alloc_enabled(layout) };
        }
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ENABLED.load(Ordering::Relaxed) {
            if let Some(class) = Self::small_class(layout) {
                // SAFETY: a small class.
                let block = unsafe { allocate(class) };
                if !block.is_null() {
                    // SAFETY: the block holds at least `layout.size()` bytes.
                    unsafe { block.write_bytes(0, layout.size()) };
                    return block;
                }
            }
        }
        // The system allocator's zeroed allocation: a large one maps fresh zero pages
        // instead of writing every byte resident.
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    #[inline]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // Until the switch is on, no block is this allocator's.
        if ENABLED.load(Ordering::Relaxed) && owns(block) {
            // SAFETY: a block in this allocator's pages came from `alloc`, which took it
            // from its page.
            unsafe { free(block) };
            return;
        }
        // SAFETY: any other block came from the system allocator.
        unsafe { System.dealloc(block, layout) }
    }

    #[inline]
    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let owned = owns(block);
        if !owned && !ENABLED.load(Ordering::Relaxed) {
            // SAFETY: the system allocator's block, and it serves every request.
            return unsafe { System.realloc(block, layout, new_size) };
        }
        let new_layout = match Layout::from_size_align(new_size, layout.align()) {
            Ok(new_layout) => new_layout,
            Err(_) => return ptr::null_mut(),
        };
        let stays_here = ENABLED.load(Ordering::Relaxed) && Self::small_class(new_layout).is_some();
        if owned && Self::small_class(layout) == Self::small_class(new_layout) {
            return block;
        }
        if !owned && !stays_here {
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

    /// The allocator under test, serving small requests.
    fn enabled() -> QuenchAlloc {
        QuenchAlloc::enable();
        QuenchAlloc::new()
    }

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
        let allocator = enabled();
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
        let state = unsafe { &*thread_heap() };
        let reserved_before = state.chunk_next;
        // Every page but the current one went back to the empty pool, so another class
        // takes them without reserving more.
        let large = Layout::from_size_align(1000, 16).unwrap();
        let pages_freed = blocks.len() / ((PAGE_BYTES - PAGE_HEADER_BYTES) / 32) - 1;
        let reused: Vec<*mut u8> = (0..pages_freed * ((PAGE_BYTES - PAGE_HEADER_BYTES) / 1024))
            .map(|_| unsafe { allocator.alloc(large) })
            .collect();
        let state = unsafe { &*thread_heap() };
        assert_eq!(state.chunk_next, reserved_before);
        for block in reused {
            unsafe { allocator.dealloc(block, large) };
        }
    }

    #[test]
    fn blocks_freed_on_other_threads_return_to_their_owner() {
        let allocator = enabled();
        let layout = Layout::from_size_align(64, 8).unwrap();
        let blocks: Vec<usize> = (0..2000)
            .map(|_| unsafe { allocator.alloc(layout) } as usize)
            .collect();
        // Served by this thread's heap, not the system allocator: otherwise their frees could
        // not come back to it.
        assert!(blocks.iter().all(|block| owns(*block as *const u8)));
        let reserved = unsafe { (*thread_heap()).chunk_next };
        std::thread::scope(|scope| {
            for part in blocks.chunks(500) {
                scope.spawn(move || {
                    for block in part {
                        unsafe { QuenchAlloc::new().dealloc(*block as *mut u8, layout) };
                    }
                });
            }
        });
        // Every block came back through the remote lists, so the owner reuses them without
        // reserving more.
        let again: Vec<*mut u8> = (0..2000).map(|_| unsafe { allocator.alloc(layout) }).collect();
        assert_eq!(unsafe { (*thread_heap()).chunk_next }, reserved);
        for block in again {
            unsafe { allocator.dealloc(block, layout) };
        }
    }

    #[test]
    fn exited_threads_hand_their_heap_on() {
        QuenchAlloc::enable();
        let layout = Layout::from_size_align(32, 8).unwrap();
        let kept = std::thread::spawn(move || unsafe { QuenchAlloc::new().alloc(layout) } as usize)
            .join()
            .unwrap();
        // The block outlives its thread; freeing it here goes to the abandoned heap.
        unsafe { QuenchAlloc::new().dealloc(kept as *mut u8, layout) };
        let reused = std::thread::spawn(move || unsafe {
            let block = QuenchAlloc::new().alloc(layout);
            QuenchAlloc::new().dealloc(block, layout);
            block as usize
        })
        .join()
        .unwrap();
        assert_ne!(reused, 0);
    }

    #[test]
    fn realloc_moves_between_classes_and_the_system_allocator() {
        let allocator = enabled();
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

    #[test]
    fn zeroed_blocks_are_zero_in_either_allocator() {
        let allocator = enabled();
        for size in [24, 4000] {
            let layout = Layout::from_size_align(size, 8).unwrap();
            unsafe {
                let block = allocator.alloc(layout);
                block.write_bytes(9, size);
                allocator.dealloc(block, layout);
                let zeroed = allocator.alloc_zeroed(layout);
                assert!(std::slice::from_raw_parts(zeroed, size).iter().all(|byte| *byte == 0));
                allocator.dealloc(zeroed, layout);
            }
        }
    }

    #[test]
    fn frees_go_to_the_allocator_that_holds_the_block() {
        let layout = Layout::from_size_align(48, 8).unwrap();
        unsafe {
            // A system block, as a request made before `enable` gets.
            let system = System.alloc(layout);
            assert!(!owns(system));
            let allocator = enabled();
            let here = allocator.alloc(layout);
            assert!(owns(here));
            // Grown in place of the system block: it moves here, keeping its bytes.
            system.write_bytes(5, 48);
            let moved = allocator.realloc(system, layout, 64);
            assert!(owns(moved));
            assert!(std::slice::from_raw_parts(moved, 48).iter().all(|byte| *byte == 5));
            allocator.dealloc(moved, Layout::from_size_align(64, 8).unwrap());
            allocator.dealloc(here, layout);
        }
    }
}
