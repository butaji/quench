//! The Wasm rows of a global's access: `GLOBAL_GET` (`D = global`) and `GLOBAL_SET`
//! (`global = A`) over the instance's global area. A global's native slot is one raw word
//! (`staged::wasm_instance::Storage`): a 32-bit payload zero-extended, a 64-bit payload whole.
//! The area is the context's `globals` (`CallContext::globals`, `compiled.rs`): one pointer per
//! environment slot to the global's `Cell::WasmGlobal::bits`, which the heap never moves. The
//! context is the pinned `s.ctx` (r13, `wasm_entry.rs`), so the row reads the area at run time:
//! a compiled program is shared by every instance of its module, so no global's address can be
//! baked into a row.
//!
//! Holes (`wasm_stitch_global.rs` plans them):
//! - `GLOBAL_GET`: hole 0 = D's frame offset, hole B = the environment slot, hole FOURTH = the
//!   byte offset of `globals` in the context.
//! - `GLOBAL_SET`: hole A = A's frame offset, hole B = the environment slot, hole FOURTH = the
//!   same context offset.
//!
//! The operator code is the storage: `STORAGE_BITS32` narrows the word to 32 bits, as
//! `Storage::narrow` does; `STORAGE_BITS64` copies it whole. Handles (references, vectors) have
//! no row: a store of one needs the write barrier the interpreter's heap applies.
//!
//! ---------------------------------------------------------------------------------------------
//! stencil crate layout `wasm_rows.rs` already uses (`crate::words`, `super::STORAGE_*`).
//! ---------------------------------------------------------------------------------------------

use super::{STORAGE_BITS32};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64, slot};

/// The word a global's storage holds for `word`: a 32-bit payload zero-extended (`Storage::narrow`).
#[inline(always)]
const fn narrow<const OP: u32>(word: u64) -> u64 {
    if OP == STORAGE_BITS32 { word as u32 as u64 } else { word }
}

/// The native slot of the global bound at the environment slot in hole B: the context's global
/// area (`CallContext::globals`, at the byte offset in hole FOURTH) indexed by that slot.
#[inline(always)]
unsafe fn cell(s: &S) -> *mut u64 {
    unsafe {
        let area = s.ctx.wrapping_add(hole64::<HOLE_FOURTH>() as usize) as *const *const *mut u64;
        *(*area).add(hole64::<HOLE_B>() as usize)
    }
}

/// `D = global`: the word of the global, narrowed to its storage.
#[inline(always)]
pub(crate) unsafe fn global_get<const OP: u32>(s: S) -> Out {
    unsafe {
        let word = narrow::<OP>(*cell(&s));
        *(slot::<0>(&s) as *mut u64) = word;
    }
    Out::Next(s)
}

/// `global = A`: the register's word, narrowed to the global's storage.
#[inline(always)]
pub(crate) unsafe fn global_set<const OP: u32>(s: S) -> Out {
    unsafe {
        let word = narrow::<OP>(*(slot::<HOLE_A>(&s) as *const u64));
        *cell(&s) = word;
    }
    Out::Next(s)
}
