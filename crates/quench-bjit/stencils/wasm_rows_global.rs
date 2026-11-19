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
//! The rules are the staged `WasmInstance` primitives (`staged::wasm_instance`): `global_load`
//! and `global_store` on a stage whose cells are the context's area.

use super::STORAGE_BITS32;
use crate::staged::wasm_concrete::scalar_stage;
use crate::staged::wasm_instance::{Storage, WasmInstance};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64, slot};

/// The storage a global's operator code names.
#[inline(always)]
const fn storage_of(op: u32) -> Storage {
    if op == STORAGE_BITS32 { Storage::Bits32 } else { Storage::Bits64 }
}

/// The instance's globals as the row sees them: the context's global area, at `area` (the
/// context plus hole FOURTH's offset of `globals`). Its cells are indexed by environment slot.
struct GlobalStage {
    area: usize,
}

scalar_stage!(GlobalStage);

impl GlobalStage {
    #[inline(always)]
    fn of(s: &S) -> Self {
        Self { area: s.ctx.wrapping_add(hole64::<HOLE_FOURTH>() as usize) }
    }

    /// The native slot of the global bound at environment slot `slot`.
    #[inline(always)]
    unsafe fn cell(&self, slot: u32) -> *mut u64 {
        // SAFETY: the context's area holds one cell pointer per environment slot the program binds.
        unsafe {
            let area = self.area as *const *const *mut u64;
            *(*area).add(slot as usize)
        }
    }
}

impl WasmInstance for GlobalStage {
    #[inline(always)]
    fn global_load(&mut self, slot: u32, storage: Storage) -> u64 {
        // SAFETY: `cell` is the global's live slot, which the heap never moves.
        storage.narrow(unsafe { *self.cell(slot) })
    }

    #[inline(always)]
    fn global_store(&mut self, slot: u32, storage: Storage, value: u64) {
        // SAFETY: as `global_load`.
        unsafe { *self.cell(slot) = storage.narrow(value) }
    }
}

/// `D = global`: the word of the global, narrowed to its storage.
#[inline(always)]
pub(crate) unsafe fn global_get<const OP: u32>(s: S) -> Out {
    let storage = const { storage_of(OP) };
    let mut instance = GlobalStage::of(&s);
    let word = instance.global_load(hole64::<HOLE_B>() as u32, storage);
    unsafe {
        *(slot::<0>(&s) as *mut u64) = word;
    }
    Out::Next(s)
}

/// `global = A`: the register's word, narrowed to the global's storage.
#[inline(always)]
pub(crate) unsafe fn global_set<const OP: u32>(s: S) -> Out {
    let storage = const { storage_of(OP) };
    let mut instance = GlobalStage::of(&s);
    let value = unsafe { *(slot::<HOLE_A>(&s) as *const u64) };
    instance.global_store(hole64::<HOLE_B>() as u32, storage, value);
    Out::Next(s)
}
