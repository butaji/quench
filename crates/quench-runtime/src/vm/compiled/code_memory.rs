//! Executable memory for every native publisher (JS regions, lane bodies, stencils, Wasm).
//!
//! Bodies are spans of shared per-thread chunks, `FUNCTION_ALIGNMENT`-aligned, instead of one
//! mapping each: a small body no longer costs a whole page and a VMA of its own (most JS
//! regions are a few hundred bytes). Publication keeps W^X: the pages a new body touches are
//! writable only while it is written, then read-only and executable again. Code never leaves
//! the thread that published it (`CodeMemory` is neither `Send` nor `Sync`), so no other thread
//! can be running code on a page while it is writable, and this thread is inside the publisher.

use std::cell::RefCell;

/// Code alignment for each compiled function.
pub(super) const FUNCTION_ALIGNMENT: usize = 64;

/// Address space one chunk reserves. Pages are committed when first written, so a thread that
/// publishes little code holds little memory; larger bodies get a chunk of their own size.
const CHUNK_BYTES: usize = 256 * 1024;

fn page_size() -> usize {
    // SAFETY: `sysconf` has no preconditions.
    usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).expect("the host has a page size")
}

/// One reservation and its free spans (offset, length), sorted and coalesced.
struct Chunk {
    base: *mut u8,
    size: usize,
    free: Vec<(usize, usize)>,
    live: usize,
}

impl Chunk {
    fn reserve(size: usize) -> Option<Self> {
        // SAFETY: an anonymous private reservation; the result is checked.
        let base = unsafe {
            libc::mmap(std::ptr::null_mut(), size, libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE, -1, 0)
        };
        (base != libc::MAP_FAILED).then(|| Self { base: base.cast(), size, free: vec![(0, size)], live: 0 })
    }

    fn take(&mut self, length: usize) -> Option<usize> {
        let index = self.free.iter().position(|&(_, free)| free >= length)?;
        let (offset, free) = self.free[index];
        if free == length { self.free.remove(index); } else { self.free[index] = (offset + length, free - length); }
        self.live += 1;
        Some(offset)
    }

    /// Returns a span; its whole pages are released to the system (they read as zero later).
    fn give(&mut self, offset: usize, length: usize) {
        let index = self.free.partition_point(|&(start, _)| start < offset);
        self.free.insert(index, (offset, length));
        if index + 1 < self.free.len() && offset + length == self.free[index + 1].0 {
            self.free[index].1 += self.free.remove(index + 1).1;
        }
        let index = if index > 0 && self.free[index - 1].0 + self.free[index - 1].1 == offset {
            self.free[index - 1].1 += self.free.remove(index).1;
            index - 1
        } else { index };
        self.live -= 1;
        let (start, length) = self.free[index];
        let page = page_size();
        let (first, end) = (start.next_multiple_of(page), (start + length) / page * page);
        if first < end {
            // SAFETY: whole pages of this reservation that hold no live code.
            unsafe { libc::madvise(self.base.add(first).cast(), end - first, libc::MADV_DONTNEED) };
        }
    }

    fn contains(&self, pointer: *mut u8) -> bool {
        (self.base as usize..self.base as usize + self.size).contains(&(pointer as usize))
    }
}

/// This thread's chunks. A chunk outlives the thread only while its code does.
struct Chunks(Vec<Chunk>);

impl Drop for Chunks {
    fn drop(&mut self) {
        for chunk in self.0.iter().filter(|chunk| chunk.live == 0) {
            // SAFETY: a reservation with no live code.
            unsafe { libc::munmap(chunk.base.cast(), chunk.size) };
        }
    }
}

thread_local! {
    static CHUNKS: RefCell<Chunks> = const { RefCell::new(Chunks(Vec::new())) };
}

/// Executable memory holding a program's machine code.
pub(in crate::vm) struct CodeMemory {
    pointer: *mut u8,
    /// The span's length: the published bytes rounded up to `FUNCTION_ALIGNMENT`.
    pub(super) length: usize,
}

impl CodeMemory {
    /// The first byte of the executable code.
    pub(in crate::vm) fn pointer(&self) -> *mut u8 {
        self.pointer
    }

    pub(in crate::vm) fn new(bytes: &[u8]) -> Option<Self> {
        Self::assemble(bytes.len(), |_, code| {
            code.copy_from_slice(bytes);
            Ok(())
        })
        .ok()
    }

    /// Take `length` bytes of code memory, let `fill` write the code knowing its final
    /// address, then make it read-only and executable. Copy-and-patch publication needs
    /// the address to resolve PC-relative continuations.
    pub(in crate::vm) fn assemble(
        length: usize,
        fill: impl FnOnce(*mut u8, &mut [u8]) -> Result<(), String>,
    ) -> Result<Self, String> {
        let requested = length;
        let length = length.max(1).checked_next_multiple_of(FUNCTION_ALIGNMENT).ok_or("code size overflow")?;
        // The chunk list is not borrowed while `fill` runs, so a publisher may publish or drop
        // other code from inside it.
        let (pointer, writable) = CHUNKS.with_borrow_mut(|chunks| {
            let found = chunks.0.iter_mut().find_map(|chunk| Some((chunk.take(length)?, &*chunk)));
            let (offset, chunk) = match found {
                Some(found) => found,
                None => {
                    let size = length.next_multiple_of(page_size()).max(CHUNK_BYTES);
                    let mut chunk = Chunk::reserve(size).ok_or("code mapping failed")?;
                    let offset = chunk.take(length).ok_or("code mapping failed")?;
                    chunks.0.push(chunk);
                    (offset, &chunks.0[chunks.0.len() - 1])
                }
            };
            // SAFETY: `offset` lies inside the chunk.
            let pointer = unsafe { chunk.base.add(offset) };
            Ok::<_, String>((pointer, protect(pointer, length, libc::PROT_READ | libc::PROT_WRITE)))
        })?;
        let memory = Self { pointer, length };
        if !writable {
            return Err("code memory could not be made writable".into());
        }
        // SAFETY: the span is `length >= requested` writable bytes no other code uses; `memory`
        // returns it on failure.
        let filled = fill(pointer, unsafe { std::slice::from_raw_parts_mut(pointer, requested) });
        // Executable again even on failure: the pages may hold other bodies.
        if !protect(pointer, length, libc::PROT_READ | libc::PROT_EXEC) {
            return Err("code memory could not be made executable".into());
        }
        filled.map(|()| memory)
    }
}

/// Sets the protection of the whole pages `[pointer, pointer + length)` touches, all inside one
/// chunk (chunks are page-aligned and page-sized).
fn protect(pointer: *mut u8, length: usize, protection: libc::c_int) -> bool {
    let page = page_size();
    let first = pointer as usize / page * page;
    let end = (pointer as usize + length).next_multiple_of(page);
    // SAFETY: whole pages of a reservation this module owns.
    unsafe { libc::mprotect(first as *mut libc::c_void, end - first, protection) == 0 }
}

impl Drop for CodeMemory {
    fn drop(&mut self) {
        let (pointer, length) = (self.pointer, self.length);
        // During thread exit the chunks may already be gone; their memory then stays mapped.
        let _ = CHUNKS.try_with(|chunks| {
            let mut chunks = chunks.borrow_mut();
            let Some(index) = chunks.0.iter().position(|chunk| chunk.contains(pointer)) else { return };
            let chunk = &mut chunks.0[index];
            chunk.give(pointer as usize - chunk.base as usize, length);
            // Keep one chunk for the next body; unmap the others once empty.
            if chunk.live == 0 && chunks.0.len() > 1 {
                let chunk = chunks.0.swap_remove(index);
                // SAFETY: a reservation with no live code.
                unsafe { libc::munmap(chunk.base.cast(), chunk.size) };
            }
        });
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;

    #[test]
    fn small_bodies_share_pages_and_freed_spans_are_reused() {
        let first = CodeMemory::new(&[0xc3]).unwrap();
        let second = CodeMemory::new(&[0xc3; 100]).unwrap();
        assert_eq!(second.pointer() as usize - first.pointer() as usize, FUNCTION_ALIGNMENT);
        assert_eq!(second.length, 128);
        let address = second.pointer();
        drop(second);
        let third = CodeMemory::new(&[0xc3; 70]).unwrap();
        assert_eq!(third.pointer(), address);
        // SAFETY: both spans hold a published `ret`.
        unsafe {
            std::mem::transmute::<*mut u8, extern "C" fn()>(first.pointer())();
            std::mem::transmute::<*mut u8, extern "C" fn()>(third.pointer())();
        }
    }

    #[test]
    fn large_bodies_get_their_own_chunk() {
        let large = CodeMemory::new(&vec![0xc3; CHUNK_BYTES + 1]).unwrap();
        assert_eq!(large.length, (CHUNK_BYTES + 1).next_multiple_of(FUNCTION_ALIGNMENT));
        // SAFETY: a published `ret`.
        unsafe { std::mem::transmute::<*mut u8, extern "C" fn()>(large.pointer())() };
    }

    #[test]
    fn a_failed_fill_returns_its_span() {
        let before = CodeMemory::new(&[0xc3]).unwrap();
        assert!(CodeMemory::assemble(10, |_, _| Err("no".into())).is_err());
        let after = CodeMemory::new(&[0xc3]).unwrap();
        assert_eq!(after.pointer() as usize - before.pointer() as usize, FUNCTION_ALIGNMENT);
    }
}
