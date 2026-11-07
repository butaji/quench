//! The arenas function sources are parsed into, and returning their pages: an arena's
//! chunk is reserved whole, and its pages become resident as trees fill them.

use oxc_allocator::Allocator;

/// Arena bytes reserved per source byte for a parse: the whole program's parse reserves as
/// much (`Compiler::specialize_with_mode_and_private_names`), and pages it does not touch
/// cost no memory.
const ARENA_BYTES_PER_SOURCE_BYTE: usize = 32;

/// The arena for parsing `text` again.
pub(crate) fn arena(text: &str) -> Allocator {
    Allocator::with_capacity(text.len().saturating_mul(ARENA_BYTES_PER_SOURCE_BYTE))
}

/// `arena(text)`, with the capacity of its one chunk (`release_free_pages`).
pub(crate) fn sized_arena(text: &str) -> (Allocator, usize) {
    let allocator = arena(text);
    let chunk = allocator.capacity();
    (allocator, chunk)
}

/// Returns the pages of `allocator`'s free space to the system: a lazily parsed body's tree
/// is released from the arena (`ParseOptions::lazy_function_bodies`) and a reset arena drops
/// its trees, but the pages they touched stay resident until the arena is dropped. `chunk` is
/// the arena's capacity while it held one chunk; with more, its free space is not one range,
/// and nothing is returned.
pub(crate) fn release_free_pages(allocator: &Allocator, chunk: usize) {
    #[cfg(unix)]
    if allocator.capacity() == chunk {
        // The arena allocates downward from the end of its chunk: the free space lies below
        // the cursor, from the chunk's start.
        let cursor = std::ptr::from_mut(allocator.alloc(0u8));
        let start = (cursor.addr() + allocator.used_bytes()).saturating_sub(chunk);
        // SAFETY: sysconf only reads a system constant.
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap_or(4096);
        let (from, to) = (start.next_multiple_of(page), cursor.addr() / page * page);
        if to > from {
            // SAFETY: the pages lie in the chunk's free space, which holds no live allocation;
            // a later allocation there faults in zeroed pages.
            unsafe { libc::madvise(cursor.wrapping_sub(cursor.addr() - from).cast(), to - from, libc::MADV_DONTNEED) };
        }
    }
}

/// Tree bytes past which a pooled arena is reset and its pages returned once its function is
/// compiled. A smaller tree's pages stay for the next function, whose parse would fault them
/// in again.
const RETURNED_ARENA_BYTES: usize = 1 << 20;

/// Resets the pooled `allocator` once the function parsed into it is compiled, returning the
/// pages of a large tree (`RETURNED_ARENA_BYTES`), which would otherwise stay resident for the
/// rest of the program's compilation.
pub(crate) fn release_after(allocator: &mut Allocator) {
    let large = allocator.used_bytes() > RETURNED_ARENA_BYTES;
    allocator.reset();
    if large {
        let chunk = allocator.capacity();
        release_free_pages(allocator, chunk);
    }
}
