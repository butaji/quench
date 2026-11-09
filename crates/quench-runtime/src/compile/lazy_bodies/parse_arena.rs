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

thread_local! {
    /// The largest whole-program parse arena so far, reset, with the pages its parses touched
    /// still resident: the next parse writes over them instead of faulting in fresh pages, as
    /// V8 reuses a zone's segments. A code-load benchmark parses its 2 MB source anew each
    /// iteration; fresh arenas, and returning a parse's free pages after it, were half of its
    /// minor faults (JetStream first-inspector-code-load, x86_64, 2026-11-08: 52K -> 27K,
    /// max RSS 45 -> 47 MB). Memory-first returns it at collections (`release_warm_arena`).
    static WARM: std::cell::Cell<Option<Allocator>> = const { std::cell::Cell::new(None) };
}

/// A whole program's parse arena (`sized_arena`): the warm one when it is large enough,
/// handed back when dropped, warm if no larger one is.
pub(crate) struct ParseArena(std::mem::ManuallyDrop<Allocator>);

impl std::ops::Deref for ParseArena {
    type Target = Allocator;
    fn deref(&self) -> &Allocator {
        &self.0
    }
}

impl Drop for ParseArena {
    fn drop(&mut self) {
        // SAFETY: the allocator is taken once, here, and not used after.
        let mut allocator = unsafe { std::mem::ManuallyDrop::take(&mut self.0) };
        allocator.reset();
        // The larger arena stays warm: it serves every parse the smaller one does.
        let warm = WARM.take();
        WARM.set(Some(match warm {
            Some(warm) if warm.capacity() > allocator.capacity() => warm,
            _ => allocator,
        }));
    }
}

/// The arena for parsing a whole program of `text`: the warm arena, when its chunk holds as
/// much as `arena(text)` reserves.
pub(crate) fn sized_arena(text: &str) -> ParseArena {
    let reserved = text.len().saturating_mul(ARENA_BYTES_PER_SOURCE_BYTE);
    let warm = WARM.take();
    let allocator = match warm {
        Some(warm) if warm.capacity() >= reserved => warm,
        warm => {
            WARM.set(warm);
            arena(text)
        }
    };
    ParseArena(std::mem::ManuallyDrop::new(allocator))
}

/// Returns the warm arena's pages to the system (`WARM`).
pub(crate) fn release_warm_arena() {
    drop(WARM.take());
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
