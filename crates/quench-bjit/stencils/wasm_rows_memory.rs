//! The bulk memory rows of a Wasm function: `memory.size`, `memory.fill` and `memory.copy` on the
//! frame's memory, run in the row with no call into the runtime (so none is a `call_` row). The
//! semantics are the staged rules' (`staged::wasm_memory`: `fill`, `copy`, the bounds checks), so a
//! row traps exactly where the u-jit's bulk step does, with the same `WasmTrap`. The movers the
//! rules call are `StencilMemory`'s (`wasm_stage.rs`).
//!
//! Operands are register byte offsets in holes, as `wasm_rows.rs` reads them:
//! - `memory.size`: the result in hole 0 (`write::<0>`); the memory register is not read.
//! - `memory.fill`: destination in `HOLE_A`, byte in `HOLE_B`, length in `HOLE_FOURTH`.
//! - `memory.copy`: destination in `HOLE_A`, source in `HOLE_B`, length in `HOLE_FOURTH`.
//!
//! Only memory index 0, the frame's memory, has rows: the base and length are the pins
//! `s.p[1]` and `s.p[2]`, as the load and store rows read them. A memory of any other index has no row,
//! and `bulk` in `wasm_stitch_memory.rs` plans none for one.
//!
//! The rows are built with the rest of the library (`wasm_library.rs`), so each has its symbol
//! `w_<kind>_<op>_<width>_<shape>` and is dispatched from `wasm_rows::run`.

use super::wasm_frame::{read, trap, write};
use super::StencilMemory;
use super::WASM_PAGE_BITS;
use crate::staged::wasm_memory::{copy, fill, size};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S};

/// `D = memory.size`: the staged `size` rule, the length in pages as an i32 word.
#[inline(always)]
pub(crate) unsafe fn memory_size(s: S) -> Out {
    let mut memory = StencilMemory::of(&s);
    let pages = size(&mut memory, WASM_PAGE_BITS);
    unsafe { write::<0>(&s, pages) };
    Out::Next(s)
}

/// `memory.fill`: traps out of bounds before any byte is written (the staged `fill`).
#[inline(always)]
pub(crate) unsafe fn memory_fill(s: S) -> Out {
    unsafe {
        let mut memory = StencilMemory::of(&s);
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
        let mut memory = StencilMemory::of(&s);
        match copy(&mut memory, read::<HOLE_A>(&s), read::<HOLE_B>(&s), read::<HOLE_FOURTH>(&s)) {
            Ok(()) => Out::Next(s),
            Err(error) => trap(s, error),
        }
    }
}
