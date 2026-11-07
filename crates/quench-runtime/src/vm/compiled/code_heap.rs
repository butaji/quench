//! The code zone: the one home of compiled code's executable memory, shared by every tier and
//! every VM in the process (Self's zone, HotSpot's code cache).
//!
//! Regions are reserved once and carved into chunks. Small bodies are bump-allocated from an
//! open chunk at `CODE_GRANULE` alignment, so they share pages and lie next to the code
//! compiled with them; a body larger than a chunk takes a run of whole chunks. A freed body's
//! block goes on its size class's free list for the next body of that size or smaller, and a
//! chunk whose last body is freed returns its pages to the system.

mod views;

use std::sync::{Mutex, MutexGuard, PoisonError};
use views::Views;

/// Every body starts on a cache line, which also satisfies each target's function alignment.
pub(in crate::vm) const CODE_GRANULE: usize = 64;
/// The unit pages are committed and returned in: a multiple of every supported page size.
const CHUNK_BYTES: usize = 64 * 1024;
/// A region's chunks: 64 MiB of address space, reserved without committing.
const REGION_CHUNKS: usize = 1024;
const SMALL_CLASSES: usize = CHUNK_BYTES / CODE_GRANULE;
const CLASS_WORD_BITS: usize = u64::BITS as usize;
const CLASS_WORDS: usize = SMALL_CLASSES.div_ceil(CLASS_WORD_BITS);

/// Where a body lives: its region and its byte offset there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    region: u32,
    offset: u32,
}

impl Block {
    fn chunk(self) -> usize {
        self.offset as usize / CHUNK_BYTES
    }

    fn after(self, bytes: usize) -> Self {
        Self {
            offset: self.offset + bytes as u32,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chunk {
    /// Holds no body; its pages are returned.
    Free,
    /// Small bodies, `live` bytes of them in use.
    Small { live: u32 },
    /// The first of a large body's `chunks` chunks.
    Head { chunks: u32 },
    /// A later chunk of a large body.
    Tail,
}

struct Region {
    views: Views,
    chunks: Box<[Chunk]>,
}

/// The chunk small bodies are bumped from, and its unused tail.
#[derive(Clone, Copy)]
struct Open {
    cursor: Block,
    end: u32,
}

struct CodeHeap {
    regions: Vec<Region>,
    /// Free small blocks by `granules - 1`.
    free: Vec<Vec<Block>>,
    /// Which `free` classes hold a block.
    nonempty: [u64; CLASS_WORDS],
    open: Option<Open>,
}

// SAFETY: the heap holds addresses of regions it owns; the mutex serializes every access.
unsafe impl Send for CodeHeap {}

static HEAP: Mutex<CodeHeap> = Mutex::new(CodeHeap::new());

fn heap() -> MutexGuard<'static, CodeHeap> {
    HEAP.lock().unwrap_or_else(PoisonError::into_inner)
}

impl CodeHeap {
    const fn new() -> Self {
        Self {
            regions: Vec::new(),
            free: Vec::new(),
            nonempty: [0; CLASS_WORDS],
            open: None,
        }
    }

    /// Places a body of `bytes`, returning its block and the bytes the zone holds for it.
    fn allocate(&mut self, bytes: usize) -> Result<(Block, usize), String> {
        let granules = bytes.max(1).div_ceil(CODE_GRANULE);
        if granules > SMALL_CLASSES {
            return self.allocate_span(bytes.div_ceil(CHUNK_BYTES));
        }
        let block = match self.reuse(granules) {
            Some(block) => block,
            None => self.bump(granules)?,
        };
        let granted = granules * CODE_GRANULE;
        let Chunk::Small { live } = &mut self.regions[block.region as usize].chunks[block.chunk()]
        else {
            unreachable!("small blocks lie in small chunks");
        };
        *live += granted as u32;
        Ok((block, granted))
    }

    /// Returns a body's block, as `allocate` granted it.
    fn free(&mut self, block: Block, granted: usize) {
        let state = &mut self.regions[block.region as usize].chunks[block.chunk()];
        match *state {
            Chunk::Head { chunks } => self.release(block.region, block.chunk(), chunks as usize),
            Chunk::Small { live } => {
                let live = live - granted as u32;
                *state = Chunk::Small { live };
                if live == 0 {
                    self.reclaim(block);
                } else {
                    self.push_free(block, granted / CODE_GRANULE);
                }
            }
            Chunk::Free | Chunk::Tail => unreachable!("a body's block starts in its own chunk"),
        }
    }

    /// A free block of at least `granules`, split to size.
    fn reuse(&mut self, granules: usize) -> Option<Block> {
        let class = self.nonempty_class_from(granules - 1)?;
        let block = self.free[class]
            .pop()
            .expect("nonempty class holds a block");
        if self.free[class].is_empty() {
            self.nonempty[class / CLASS_WORD_BITS] &= !(1 << (class % CLASS_WORD_BITS));
        }
        let held = class + 1;
        if held > granules {
            self.push_free(block.after(granules * CODE_GRANULE), held - granules);
        }
        Some(block)
    }

    fn nonempty_class_from(&self, class: usize) -> Option<usize> {
        let (first, bit) = (class / CLASS_WORD_BITS, class % CLASS_WORD_BITS);
        let masked = |word: usize| {
            if word == first {
                self.nonempty[word] & (u64::MAX << bit)
            } else {
                self.nonempty[word]
            }
        };
        (first..CLASS_WORDS)
            .find(|&word| masked(word) != 0)
            .map(|word| word * CLASS_WORD_BITS + masked(word).trailing_zeros() as usize)
    }

    fn push_free(&mut self, block: Block, granules: usize) {
        if self.free.is_empty() {
            self.free.resize_with(SMALL_CLASSES, Vec::new);
        }
        let class = granules - 1;
        self.free[class].push(block);
        self.nonempty[class / CLASS_WORD_BITS] |= 1 << (class % CLASS_WORD_BITS);
    }

    /// The next `granules` of the open chunk, opening another when it is full. The full
    /// chunk's tail becomes a free block.
    fn bump(&mut self, granules: usize) -> Result<Block, String> {
        let bytes = (granules * CODE_GRANULE) as u32;
        let open = match self.open {
            Some(open) if open.end - open.cursor.offset >= bytes => open,
            previous => {
                if let Some(full) = previous.filter(|full| full.end > full.cursor.offset) {
                    self.push_free(
                        full.cursor,
                        (full.end - full.cursor.offset) as usize / CODE_GRANULE,
                    );
                }
                self.open_chunk()?
            }
        };
        self.open = Some(Open {
            cursor: open.cursor.after(bytes as usize),
            ..open
        });
        Ok(open.cursor)
    }

    fn open_chunk(&mut self) -> Result<Open, String> {
        let (region, chunk) = self.free_run(1)?;
        self.regions[region as usize].chunks[chunk] = Chunk::Small { live: 0 };
        let start = (chunk * CHUNK_BYTES) as u32;
        Ok(Open {
            cursor: Block {
                region,
                offset: start,
            },
            end: start + CHUNK_BYTES as u32,
        })
    }

    fn allocate_span(&mut self, chunks: usize) -> Result<(Block, usize), String> {
        let (region, first) = self.free_run(chunks)?;
        let states = &mut self.regions[region as usize].chunks[first..first + chunks];
        states.fill(Chunk::Tail);
        states[0] = Chunk::Head {
            chunks: chunks as u32,
        };
        Ok((
            Block {
                region,
                offset: (first * CHUNK_BYTES) as u32,
            },
            chunks * CHUNK_BYTES,
        ))
    }

    /// The lowest run of `chunks` free chunks, reserving a region when none has one.
    fn free_run(&mut self, chunks: usize) -> Result<(u32, usize), String> {
        let found = self.regions.iter().enumerate().find_map(|(index, region)| {
            let mut run = 0;
            region
                .chunks
                .iter()
                .position(|chunk| {
                    run = if *chunk == Chunk::Free { run + 1 } else { 0 };
                    run == chunks
                })
                .map(|last| (index as u32, last + 1 - chunks))
        });
        if let Some(run) = found {
            return Ok(run);
        }
        let count = chunks.max(REGION_CHUNKS);
        let views = Views::reserve(count * CHUNK_BYTES)?;
        self.regions.push(Region {
            views,
            chunks: vec![Chunk::Free; count].into_boxed_slice(),
        });
        Ok(((self.regions.len() - 1) as u32, 0))
    }

    /// A small chunk with no live body: drops its free blocks and returns its pages, or
    /// rewinds it when it is the open chunk.
    fn reclaim(&mut self, block: Block) {
        let chunk = block.chunk();
        for class in 0..SMALL_CLASSES {
            if self.nonempty[class / CLASS_WORD_BITS] & (1 << (class % CLASS_WORD_BITS)) == 0 {
                continue;
            }
            self.free[class].retain(|free| free.region != block.region || free.chunk() != chunk);
            if self.free[class].is_empty() {
                self.nonempty[class / CLASS_WORD_BITS] &= !(1 << (class % CLASS_WORD_BITS));
            }
        }
        let start = Block {
            region: block.region,
            offset: (chunk * CHUNK_BYTES) as u32,
        };
        match &mut self.open {
            Some(open) if open.cursor.region == block.region && open.cursor.chunk() == chunk => {
                open.cursor = start;
            }
            _ => self.release(block.region, chunk, 1),
        }
    }

    fn release(&mut self, region: u32, first: usize, chunks: usize) {
        let region = &mut self.regions[region as usize];
        region
            .views
            .release(first * CHUNK_BYTES, chunks * CHUNK_BYTES);
        region.chunks[first..first + chunks].fill(Chunk::Free);
    }
}

/// A compiled body's executable memory in the code zone.
pub(in crate::vm) struct CodeMemory {
    pointer: *mut u8,
    length: usize,
    block: Block,
}

impl CodeMemory {
    /// The body's first executable byte.
    pub(in crate::vm) fn pointer(&self) -> *mut u8 {
        self.pointer
    }

    /// The bytes the zone holds for the body: its length rounded to the zone's granule.
    pub(in crate::vm) fn length(&self) -> usize {
        self.length
    }

    pub(in crate::vm) fn new(bytes: &[u8]) -> Option<Self> {
        Self::assemble(bytes.len(), |_, code| {
            code.copy_from_slice(bytes);
            Ok(())
        })
        .ok()
    }

    /// Places `length` bytes of code, lets `fill` write them knowing their final executable
    /// address, and publishes them. Copy-and-patch publication needs the address to resolve
    /// PC-relative continuations.
    pub(in crate::vm) fn assemble(
        length: usize,
        fill: impl FnOnce(*mut u8, &mut [u8]) -> Result<(), String>,
    ) -> Result<Self, String> {
        let (memory, views) = {
            let mut heap = heap();
            let (block, granted) = heap.allocate(length)?;
            let views = heap.regions[block.region as usize].views;
            let pointer = views.exec.wrapping_add(block.offset as usize);
            (
                Self {
                    pointer,
                    length: granted,
                    block,
                },
                views,
            )
        };
        let offset = memory.block.offset as usize;
        // SAFETY: `length <= granted` bytes of the writable view, owned by `memory`.
        let code = unsafe { std::slice::from_raw_parts_mut(views.writer(offset), length) };
        Views::writing(|| fill(memory.pointer, code))?;
        views.retire_writer(offset, length);
        views::flush_icache(memory.pointer, length);
        Ok(memory)
    }
}

impl Drop for CodeMemory {
    fn drop(&mut self) {
        heap().free(self.block, self.length);
    }
}

#[cfg(test)]
mod tests;
