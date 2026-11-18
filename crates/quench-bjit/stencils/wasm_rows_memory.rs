//! The bulk memory rows of a Wasm function: `memory.size`, `memory.fill` and `memory.copy` on the
//! frame's memory, run in the row with no call into the runtime (so none is a `call_` row). The
//! semantics are the staged rules' (`staged::wasm_memory`: `fill`, `copy`, the bounds checks), so a
//! row traps exactly where the u-jit's bulk step does, with the same `WasmTrap`.
//!
//! Operands are register byte offsets in holes, as `wasm_rows.rs` reads them:
//! - `memory.size`: the result in hole 0 (`write::<0>`); the memory register is not read.
//! - `memory.fill`: destination in `HOLE_A`, byte in `HOLE_B`, length in `HOLE_FOURTH`.
//! - `memory.copy`: destination in `HOLE_A`, source in `HOLE_B`, length in `HOLE_FOURTH`.
//!
//! Only memory index 0, the frame's memory, has rows: the base and length are the frame header's
//! words (`FRAME_HEADER_WORDS`), as `call()` reads them. A memory of any other index has no row,
//! and `bulk` in `wasm_stitch_memory.rs` plans none for one.
//!
//! The rows are built with the rest of the library (`wasm_library.rs`), so each has its symbol
//! `w_<kind>_<op>_<width>_<shape>` and is dispatched from `wasm_rows::run`.

use super::{StencilMemory, read, trap, write};
use crate::staged::wasm_concrete::scalar_stage;
use crate::staged::wasm_memory::{Access, WasmMemory, WasmMemoryWrite, copy, fill};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S};

/// The frame header's memory base and length words (`call()`), after the result pointer.
const BASE_WORD: usize = 1;
const LENGTH_WORD: usize = 2;
const _: () = assert!(crate::wasm_codes::FRAME_HEADER_WORDS as usize > LENGTH_WORD);

/// The log2 of a Wasm page, `crate::wasm::memory::WASM_PAGE_BITS` (16). Written out because the
/// stencil build does not compile `crate::wasm::memory`.
const PAGE_BITS: u32 = 16;

/// The frame's memory for the bulk rows: `StencilMemory`'s base and length, with the bulk movers
/// it leaves out. Its accesses delegate to `StencilMemory`.
struct BulkMemory {
    base: *mut u8,
    length: u64,
}

scalar_stage!(BulkMemory);

impl WasmMemory for BulkMemory {
    fn guarded(&self) -> bool {
        false
    }

    #[inline(always)]
    fn memory_length(&mut self) -> u64 {
        self.length
    }

    /// A 32-bit address word is already its zero-extended index.
    #[inline(always)]
    fn memory_index(&mut self, address: u64) -> u64 {
        address
    }

    #[inline(always)]
    fn memory_load(&mut self, access: Access, index: u64, offset: u32) -> u64 {
        StencilMemory { base: self.base, length: self.length }.memory_load(access, index, offset)
    }
}

impl WasmMemoryWrite for BulkMemory {
    #[inline(always)]
    fn memory_store(&mut self, access: Access, index: u64, offset: u32, value: u64) {
        StencilMemory { base: self.base, length: self.length }.memory_store(access, index, offset, value)
    }

    /// `length` copies of the low byte of `byte` at `index`; `fill` has checked the range.
    #[inline(always)]
    fn memory_fill(&mut self, index: u64, byte: u64, length: u64) {
        // SAFETY: `fill` checked `index + length` inside the memory, whose base is `self.base`
        // for `self.length` bytes.
        unsafe { fill_bytes(self.base.wrapping_add(index as usize), byte as u8, length as usize) }
    }

    /// `length` bytes from `source` to `destination` (memmove semantics); `copy` has checked both
    /// ranges.
    #[inline(always)]
    fn memory_copy(&mut self, destination: u64, source: u64, length: u64) {
        // SAFETY: `copy` checked both ranges inside the memory, whose base is `self.base`; `copy`
        // allows the ranges to overlap.
        unsafe {
            copy_bytes(self.base.wrapping_add(source as usize), self.base.wrapping_add(destination as usize), length as usize)
        }
    }
}

// A stencil may call nothing outside itself, so the movers cannot be `memset`/`memmove`, which
// LLVM lowers `write_bytes`/`copy` to: x86-64 moves with `rep stosb`/`rep movsb` (fast for any
// length on current cores), and a copy whose destination overlaps past its source runs backward
// one byte at a time; other targets move bytes through volatile accesses, which LLVM keeps.
#[inline(always)]
unsafe fn fill_bytes(destination: *mut u8, byte: u8, length: usize) {
    unsafe {
        #[cfg(target_arch = "x86_64")]
        core::arch::asm!("rep stosb", inout("rdi") destination => _, inout("rcx") length => _, in("al") byte, options(nostack, preserves_flags));
        #[cfg(not(target_arch = "x86_64"))]
        for at in 0..length {
            core::ptr::write_volatile(destination.add(at), byte);
        }
    }
}

#[inline(always)]
unsafe fn copy_bytes(source: *const u8, destination: *mut u8, length: usize) {
    unsafe {
        let overlapping_after = (destination as usize).wrapping_sub(source as usize) < length;
        #[cfg(target_arch = "x86_64")]
        if !overlapping_after {
            core::arch::asm!("rep movsb", inout("rsi") source => _, inout("rdi") destination => _, inout("rcx") length => _, options(nostack, preserves_flags));
            return;
        }
        if overlapping_after {
            for at in (0..length).rev() {
                core::ptr::write_volatile(destination.add(at), core::ptr::read_volatile(source.add(at)));
            }
        } else {
            for at in 0..length {
                core::ptr::write_volatile(destination.add(at), core::ptr::read_volatile(source.add(at)));
            }
        }
    }
}

/// The frame's memory, its base and length read from the header words as `call()` reads them.
#[inline(always)]
unsafe fn frame_memory(s: &S) -> BulkMemory {
    let header = s.frame as *const usize;
    // SAFETY: the header's words are the entry's and live for the row.
    unsafe {
        BulkMemory {
            base: *header.add(BASE_WORD) as *mut u8,
            length: *header.add(LENGTH_WORD) as u64,
        }
    }
}

/// `D = memory.size`: the length in pages, an i32 word (zero-extended, as every i32 register).
#[inline(always)]
pub(crate) unsafe fn memory_size(s: S) -> Out {
    unsafe {
        let pages = frame_memory(&s).length >> PAGE_BITS;
        write::<0>(&s, u64::from(pages as u32));
    }
    Out::Next(s)
}

/// `memory.fill`: traps out of bounds before any byte is written (the staged `fill`).
#[inline(always)]
pub(crate) unsafe fn memory_fill(s: S) -> Out {
    unsafe {
        let mut memory = frame_memory(&s);
        match fill(&mut memory, read::<HOLE_A>(&s), read::<HOLE_B>(&s), read::<HOLE_FOURTH>(&s)) {
            Ok(()) => Out::Next(s),
            Err(error) => trap(s, error),
        }
    }
}

/// `memory.copy`: the destination range is checked first, then the source, both before any byte
/// moves (the staged `copy`).
#[inline(always)]
pub(crate) unsafe fn memory_copy(s: S) -> Out {
    unsafe {
        let mut memory = frame_memory(&s);
        match copy(&mut memory, read::<HOLE_A>(&s), read::<HOLE_B>(&s), read::<HOLE_FOURTH>(&s)) {
            Ok(()) => Out::Next(s),
            Err(error) => trap(s, error),
        }
    }
}
