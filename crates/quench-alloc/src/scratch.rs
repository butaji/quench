//! The code generator thread's scratch arena: large blocks that never touch glibc's arenas.
//!
//! A compile allocates megabytes of Cranelift scratch in blocks above `MAX_SMALL_BYTES`, which
//! the system allocator serves from the thread's malloc arena. Those blocks fragment it, and
//! `malloc_trim` (or glibc's own trimming) then advises every free run with its own
//! `madvise(MADV_DONTNEED)`: each one a cross-CPU TLB shootdown that interrupts the program's
//! thread (V8-v7 earley-boyer, JIT, x86_64: ~7,500 calls against ~385 without the generator,
//! about 2,300 of them from the generator's per-compile release).
//!
//! The thread adopts one reserved region (`adopt`) and takes power-of-two blocks from it:
//! a freed block joins its class's list and the next request of the class pops it, so a
//! recurring compile reuses the same pages with no system call. When nothing is live the
//! region resets and the used span returns with one call (`release`), which the thread makes
//! once per idle period rather than once per compile. A bump arena reset per compile was
//! rejected: results the program keeps (a code vector moved to it) would pin a compile's
//! whole arena; per-block size classes let them pin only themselves, and `release` then
//! returns the free blocks of the largest classes one by one.
//!
//! Any thread may free a block (`owns`); only adopting threads allocate.

use std::alloc::Layout;
use std::cell::Cell;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use super::MAX_SMALL_BYTES;

/// log2 of the smallest scratch class: the first power of two above `MAX_SMALL_BYTES`.
const MIN_SHIFT: u32 = MAX_SMALL_BYTES.trailing_zeros() + 1;
/// log2 of the largest scratch class, 1 MiB: where the system allocator maps blocks itself
/// and `large` takes over.
const MAX_SHIFT: u32 = 20;
const CLASSES: usize = (MAX_SHIFT - MIN_SHIFT + 1) as usize;
/// The largest request served: its alignment is the smallest class's, which every block has.
const MAX_ALIGN: usize = 1 << MIN_SHIFT;
/// Address space reserved for the arena, faulted by first touch: a compile's scratch peaked
/// near 100 MB on the largest bodies; past this the system allocator serves the rest.
const ARENA_BYTES: usize = 1 << 30;
/// Free blocks of at least this class (64 KiB) are returned one by one while another thread
/// still holds a live block (a reset is impossible then); smaller ones stay, a few pages each.
const PARTIAL_RELEASE_SHIFT: u32 = 16;

const _: () = assert!(MAX_SMALL_BYTES.is_power_of_two() && MIN_SHIFT > MAX_SMALL_BYTES.trailing_zeros());
const _: () = assert!(ARENA_BYTES.is_multiple_of(1 << MAX_SHIFT));

struct FreeBlock {
    next: *mut FreeBlock,
}

/// The arena's state, behind one lock: the adopting thread allocates and any thread frees.
struct Arena {
    /// Start of the used span's unused remainder, as an offset from `BASE`.
    used: usize,
    /// Free blocks per class, as addresses.
    free: [usize; CLASSES],
    /// Blocks handed out and not freed back.
    live: usize,
}

static ARENA: Mutex<Arena> = Mutex::new(Arena { used: 0, free: [0; CLASSES], live: 0 });
/// The reservation's address, zero until a thread adopts it or when the host refused it.
static BASE: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ADOPTED: Cell<bool> = const { Cell::new(false) };
}

fn class_of(size: usize) -> usize {
    (size.next_power_of_two().trailing_zeros().max(MIN_SHIFT) - MIN_SHIFT) as usize
}

fn lock() -> std::sync::MutexGuard<'static, Arena> {
    ARENA.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Makes the calling thread's large blocks come from the arena, reserving it on first use.
pub fn adopt() {
    if BASE.load(Ordering::Relaxed) == 0 {
        // SAFETY: a fresh anonymous mapping, faulted on first touch; the result is checked.
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                ARENA_BYTES,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if base != libc::MAP_FAILED {
            let _ = BASE.compare_exchange(0, base as usize, Ordering::AcqRel, Ordering::Relaxed);
            if BASE.load(Ordering::Relaxed) != base as usize {
                // SAFETY: lost the race; the mapping is unused.
                unsafe { libc::munmap(base, ARENA_BYTES) };
            }
        }
    }
    let _ = ADOPTED.try_with(|adopted| adopted.set(true));
}

/// Whether `layout` is served from the arena on this thread.
pub(crate) fn applies(layout: Layout) -> bool {
    layout.size() > MAX_SMALL_BYTES
        && layout.size() <= 1 << MAX_SHIFT
        && layout.align() <= MAX_ALIGN
        && BASE.load(Ordering::Relaxed) != 0
        && ADOPTED.try_with(Cell::get).unwrap_or(false)
}

/// Whether `block` lies in the arena.
pub(crate) fn owns(block: *const u8) -> bool {
    let base = BASE.load(Ordering::Relaxed);
    base != 0 && (block as usize).wrapping_sub(base) < ARENA_BYTES
}

/// A block for `layout` when this thread's requests of it come from the arena, else null.
pub(crate) fn alloc_for(layout: Layout) -> *mut u8 {
    if applies(layout) { alloc(layout) } else { ptr::null_mut() }
}

/// A block for `layout` (one `applies` accepted), or null when the arena is full.
fn alloc(layout: Layout) -> *mut u8 {
    let class = class_of(layout.size());
    let mut arena = lock();
    let head = arena.free[class];
    let block = if head != 0 {
        // SAFETY: a free block of this class, linked through its first word.
        arena.free[class] = unsafe { (*(head as *mut FreeBlock)).next } as usize;
        head
    } else {
        let bytes = 1usize << (class as u32 + MIN_SHIFT);
        if arena.used + bytes > ARENA_BYTES {
            return ptr::null_mut();
        }
        let block = BASE.load(Ordering::Relaxed) + arena.used;
        arena.used += bytes;
        block
    };
    arena.live += 1;
    block as *mut u8
}

/// Returns `block` (the arena's, per `owns`) to its class. The size is the request's, which
/// rounds to the class the block came from.
pub(crate) fn free(block: *mut u8, layout: Layout) {
    let class = class_of(layout.size());
    let mut arena = lock();
    // SAFETY: a block of at least the class size, no longer in use.
    unsafe { (*block.cast::<FreeBlock>()).next = arena.free[class] as *mut FreeBlock };
    arena.free[class] = block as usize;
    arena.live -= 1;
}

/// Returns the arena's pages to the system. With no live block the arena resets and its used
/// span goes in one call; otherwise only the free blocks of the largest classes go.
pub fn release() {
    let mut arena = lock();
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 || arena.used == 0 {
        return;
    }
    if arena.live == 0 {
        // SAFETY: no block is live, so the span holds nothing in use; it reads zero after.
        unsafe { libc::madvise(base as *mut libc::c_void, arena.used, libc::MADV_DONTNEED) };
        arena.free = [0; CLASSES];
        arena.used = 0;
        return;
    }
    for class in (PARTIAL_RELEASE_SHIFT - MIN_SHIFT) as usize..CLASSES {
        let bytes = 1usize << (class as u32 + MIN_SHIFT);
        let mut block = arena.free[class];
        while block != 0 {
            // SAFETY: free blocks are page-aligned multiples of the class and hold nothing in
            // use. The link in the first word is saved before it is zeroed, and rewritten after.
            unsafe {
                let next = (*(block as *mut FreeBlock)).next;
                libc::madvise(block as *mut libc::c_void, bytes, libc::MADV_DONTNEED);
                (*(block as *mut FreeBlock)).next = next;
                block = next as usize;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(size: usize) -> Layout {
        Layout::from_size_align(size, 16).unwrap()
    }

    /// The arena is process-wide; tests that read its state take this lock.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[test]
    fn classes_round_up_to_powers_of_two() {
        assert_eq!(class_of(MAX_SMALL_BYTES + 1), 0);
        assert_eq!(class_of(1 << MIN_SHIFT), 0);
        assert_eq!(class_of((1 << MIN_SHIFT) + 1), 1);
        assert_eq!(class_of(1 << MAX_SHIFT), CLASSES - 1);
    }

    #[test]
    fn only_adopting_threads_and_large_sizes_apply() {
        let _guard = serial();
        assert!(!applies(layout(MAX_SMALL_BYTES + 1)), "this test thread has not adopted");
        adopt();
        assert!(applies(layout(MAX_SMALL_BYTES + 1)));
        assert!(!applies(layout(MAX_SMALL_BYTES)));
        assert!(!applies(layout((1 << MAX_SHIFT) + 1)));
        assert!(!applies(Layout::from_size_align(4096, MAX_ALIGN * 2).unwrap()));
        let _ = ADOPTED.try_with(|adopted| adopted.set(false));
    }

    #[test]
    fn a_freed_block_serves_the_next_request_of_its_class() {
        let _guard = serial();
        adopt();
        let first = alloc(layout(3000));
        assert!(owns(first) && !first.is_null());
        free(first, layout(3000));
        assert_eq!(alloc(layout(4096)), first, "3000 and 4096 share a class");
        free(first, layout(4096));
        release();
    }

    #[test]
    fn blocks_are_aligned_and_do_not_overlap() {
        let _guard = serial();
        adopt();
        let sizes = [2000usize, 5000, 70_000, 2000];
        let blocks: Vec<_> = sizes.iter().map(|&size| alloc(layout(size))).collect();
        for (&block, &size) in blocks.iter().zip(&sizes) {
            assert_eq!(block as usize % MAX_ALIGN, 0);
            // SAFETY: the block holds `size` bytes.
            unsafe { block.write_bytes(0xAB, size) };
        }
        let mut spans: Vec<_> = blocks.iter().zip(&sizes).map(|(&b, &s)| (b as usize, s)).collect();
        spans.sort_unstable();
        assert!(spans.windows(2).all(|pair| pair[0].0 + pair[0].1 <= pair[1].0));
        for (&block, &size) in blocks.iter().zip(&sizes) {
            free(block, layout(size));
        }
        release();
    }

    #[test]
    fn release_with_nothing_live_resets_and_zeroes_the_span() {
        let _guard = serial();
        adopt();
        let block = alloc(layout(8192));
        // SAFETY: the block holds 8192 bytes.
        unsafe { block.write_bytes(0xCD, 8192) };
        free(block, layout(8192));
        release();
        assert_eq!(lock().used, 0);
        let again = alloc(layout(8192));
        assert_eq!(again, block, "the reset arena hands out its start again");
        // SAFETY: as above.
        assert!(unsafe { std::slice::from_raw_parts(again, 8192) }.iter().all(|&byte| byte == 0));
        free(again, layout(8192));
        release();
    }

    #[test]
    fn a_live_block_survives_a_release_and_free_blocks_stay_usable() {
        let _guard = serial();
        adopt();
        let size = 1usize << PARTIAL_RELEASE_SHIFT;
        let (live, spare) = (alloc(layout(size)), alloc(layout(size)));
        // SAFETY: both hold `size` bytes.
        unsafe { live.write_bytes(0x5A, size) };
        unsafe { spare.write_bytes(0x5A, size) };
        free(spare, layout(size));
        release();
        // SAFETY: `live` was not freed.
        assert!(unsafe { std::slice::from_raw_parts(live, size) }.iter().all(|&byte| byte == 0x5A));
        assert_eq!(alloc(layout(size)), spare, "the released block is still in its list");
        free(spare, layout(size));
        free(live, layout(size));
        release();
    }

    #[test]
    fn another_thread_may_free_a_block() {
        let _guard = serial();
        adopt();
        let block = alloc(layout(10_000)) as usize;
        std::thread::spawn(move || free(block as *mut u8, layout(10_000))).join().unwrap();
        assert_eq!(lock().live, 0);
        release();
    }
}
