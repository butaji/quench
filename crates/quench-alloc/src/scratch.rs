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
/// The system page, the unit `release` returns.
const PAGE_BYTES: usize = 4096;
/// Address space reserved for the arena, faulted by first touch: a compile's scratch peaked
/// near 100 MB on the largest bodies; past this the system allocator serves the rest.
const ARENA_BYTES: usize = 1 << 30;
/// Free blocks of at least this class (64 KiB) are returned one by one while another thread
/// still holds a live block (a reset is impossible then); smaller ones stay, a few pages each.
const PARTIAL_RELEASE_SHIFT: u32 = 16;

const _: () = assert!(MAX_SMALL_BYTES.is_power_of_two() && MIN_SHIFT > MAX_SMALL_BYTES.trailing_zeros());
// A block of a page or more starts on a page: every block starts at a multiple of its size.
const _: () = assert!(PAGE_BYTES == 2 << MIN_SHIFT);
const _: () = assert!(ARENA_BYTES.is_multiple_of(1 << MAX_SHIFT));

/// The arena's state, behind one lock: the adopting thread allocates and any thread frees.
/// Blocks are buddies: a block of class `c` starts at an offset that is a multiple of its size,
/// so its buddy is the offset with that size's bit flipped. A freed block whose buddy is free
/// joins it into the next class, and a request with no free block of its class splits a larger
/// one: a compile's growing vectors reuse the pages of their earlier, smaller blocks, so the
/// resident span follows the compile's live bytes, not every size it ever asked for. Which
/// blocks are free lives beside the arena (`free_bits`), never in the blocks: a free block's
/// pages stay untouched until a request takes it.
struct Arena {
    /// Start of the used span's unused remainder, as an offset from `BASE`.
    used: usize,
    /// Which blocks of each class are free, by offset over the class size (`free_word`).
    free_bits: [u64; FREE_WORDS],
    /// Blocks handed out and not freed back.
    live: usize,
}

/// The words of class `class`'s free bits start at `bits_base(class)`.
const fn bits_base(class: usize) -> usize {
    let mut base = 0;
    let mut below = 0;
    while below < class {
        base += (ARENA_BYTES >> (below as u32 + MIN_SHIFT)).div_ceil(64);
        below += 1;
    }
    base
}
const FREE_WORDS: usize = bits_base(CLASSES);

/// The word and bit of the block at `offset` of class `class` in `free_bits`.
fn free_word(class: usize, offset: usize) -> (usize, u64) {
    let index = offset >> (class as u32 + MIN_SHIFT);
    (bits_base(class) + index / 64, 1 << (index % 64))
}

const fn class_bytes(class: usize) -> usize { 1 << (class as u32 + MIN_SHIFT) }

impl Arena {
    /// The words of class `class`'s free bits that cover the used span.
    fn class_words(&self, class: usize) -> std::ops::Range<usize> {
        bits_base(class)..bits_base(class) + (self.used >> (class as u32 + MIN_SHIFT)).div_ceil(64)
    }

    fn is_free(&self, class: usize, offset: usize) -> bool {
        let (word, bit) = free_word(class, offset);
        self.free_bits[word] & bit != 0
    }

    fn set_free(&mut self, class: usize, offset: usize, free: bool) {
        let (word, bit) = free_word(class, offset);
        if free { self.free_bits[word] |= bit } else { self.free_bits[word] &= !bit }
    }

    /// The offset of a free block of class `class`, taken: the lowest, so blocks pack low.
    fn take(&mut self, class: usize) -> Option<usize> {
        let words = self.class_words(class);
        let first = words.start;
        let (word, bits) = words.map(|word| (word, self.free_bits[word])).find(|(_, bits)| *bits != 0)?;
        let offset = ((word - first) * 64 + bits.trailing_zeros() as usize) << (class as u32 + MIN_SHIFT);
        self.set_free(class, offset, false);
        Some(offset)
    }

    /// Frees the block at `offset` of class `class`, joined with its free buddies.
    fn release_block(&mut self, mut class: usize, mut offset: usize) {
        while class + 1 < CLASSES {
            let buddy = offset ^ class_bytes(class);
            if buddy >= self.used || !self.is_free(class, buddy) { break; }
            self.set_free(class, buddy, false);
            offset = offset.min(buddy);
            class += 1;
        }
        self.set_free(class, offset, true);
    }

    /// Every free block of class `class`, as offsets.
    fn free_blocks(&self, class: usize) -> impl Iterator<Item = usize> + '_ {
        let first = self.class_words(class).start;
        self.class_words(class).flat_map(move |word| {
            let bits = self.free_bits[word];
            (0..64).filter(move |bit| bits & (1 << bit) != 0)
                .map(move |bit| ((word - first) * 64 + bit) << (class as u32 + MIN_SHIFT))
        })
    }
}

static ARENA: Mutex<Arena> = Mutex::new(Arena { used: 0, free_bits: [0; FREE_WORDS], live: 0 });
static BASE: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ADOPTED: Cell<bool> = const { Cell::new(false) };
}

/// The power-of-two scratch class of `size` bytes: the index of the smallest power of two that
/// holds it, counted from `MIN_SHIFT`. A request below the smallest class takes class zero.
fn pow2_class_of(size: usize) -> usize {
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

/// Ends the calling thread's adoption: its later large blocks come from the system allocator
/// again, and blocks it took stay the arena's until freed.
pub fn leave() {
    let _ = ADOPTED.try_with(|adopted| adopted.set(false));
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
    let class = pow2_class_of(layout.size());
    let base = BASE.load(Ordering::Relaxed);
    let mut arena = lock();
    let split = (class..CLASSES).find_map(|larger| arena.take(larger).map(|offset| (larger, offset)));
    let offset = match split {
        Some((mut larger, offset)) => {
            // The halves above the request stay free, one per class.
            while larger > class {
                larger -= 1;
                arena.set_free(larger, offset + class_bytes(larger), true);
            }
            offset
        }
        None => {
            let bytes = class_bytes(class);
            // The block starts at a multiple of its size; the gap below goes to the free blocks.
            while !arena.used.is_multiple_of(bytes) {
                let gap = (arena.used.trailing_zeros() - MIN_SHIFT) as usize;
                let used = arena.used;
                arena.used += class_bytes(gap);
                arena.release_block(gap, used);
            }
            if arena.used + bytes > ARENA_BYTES {
                return ptr::null_mut();
            }
            arena.used += bytes;
            arena.used - bytes
        }
    };
    arena.live += 1;
    (base + offset) as *mut u8
}

/// Returns `block` (the arena's, per `owns`) to its class, joined with its free buddies. The
/// size is the request's, which rounds to the class the block came from.
pub(crate) fn free(block: *mut u8, layout: Layout) {
    let class = pow2_class_of(layout.size());
    let base = BASE.load(Ordering::Relaxed);
    let mut arena = lock();
    arena.release_block(class, block as usize - base);
    arena.live -= 1;
}

/// Returns the arena's pages to the system. With no live block the arena resets and its used
/// span goes in one call; otherwise only the free blocks of the largest classes go.
pub fn release() {
    release_from(PARTIAL_RELEASE_SHIFT);
}

/// `release`, returning every free block of a page or more while blocks stay live: a phase
/// that ends (`leave`) with a few of its blocks kept returns the rest of its pages.
pub fn release_phase() {
    release_from(12);
}

fn release_from(partial_release_shift: u32) {
    let mut arena = lock();
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 || arena.used == 0 {
        return;
    }
    if arena.live == 0 {
        // SAFETY: no block is live, so the span holds nothing in use; it reads zero after.
        unsafe { libc::madvise(base as *mut libc::c_void, arena.used, libc::MADV_DONTNEED) };
        for class in 0..CLASSES {
            let words = arena.class_words(class);
            arena.free_bits[words].fill(0);
        }
        arena.used = 0;
        return;
    }
    for class in (partial_release_shift.max(MIN_SHIFT) - MIN_SHIFT) as usize..CLASSES {
        let bytes = class_bytes(class);
        if bytes < PAGE_BYTES { continue; }
        for offset in arena.free_blocks(class) {
            // SAFETY: a free block of whole pages holds nothing in use.
            unsafe { libc::madvise((base + offset) as *mut libc::c_void, bytes, libc::MADV_DONTNEED) };
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
        assert_eq!(pow2_class_of(MAX_SMALL_BYTES + 1), 0);
        assert_eq!(pow2_class_of(1 << MIN_SHIFT), 0);
        assert_eq!(pow2_class_of((1 << MIN_SHIFT) + 1), 1);
        assert_eq!(pow2_class_of(1 << MAX_SHIFT), CLASSES - 1);
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
            if size > PAGE_BYTES {
                assert_eq!(block as usize % PAGE_BYTES, 0, "release returns whole pages only");
            }
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
    fn freed_buddies_join_and_serve_a_larger_request() {
        let _guard = serial();
        adopt();
        let small = 1usize << MIN_SHIFT;
        let (first, second) = (alloc(layout(small)), alloc(layout(small)));
        free(first, layout(small));
        free(second, layout(small));
        let joined = alloc(layout(2 * small));
        assert_eq!(joined, first.min(second), "two free buddies make one block of the next class");
        free(joined, layout(2 * small));
        release();
    }

    #[test]
    fn a_growing_vector_reuses_its_earlier_blocks() {
        let _guard = serial();
        adopt();
        let mut size = 2 * MAX_SMALL_BYTES;
        let mut block = alloc(layout(size));
        while size < 1 << MAX_SHIFT {
            let grown = alloc(layout(2 * size));
            free(block, layout(size));
            (block, size) = (grown, 2 * size);
        }
        assert!(lock().used <= 4 << MAX_SHIFT, "the span stays near twice the largest block: {}", lock().used);
        free(block, layout(size));
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
