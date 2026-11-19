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
//! The operator code is the storage (`wasm_frame::storage_of`). Handles (references, vectors) have
//! no row: a store of one needs the write barrier the interpreter's heap applies.
//!
//! The rules are the staged `WasmInstance` primitives (`staged::wasm_instance`): `global_load`
//! and `global_store` on `GlobalStage` (`wasm_stage.rs`), whose cells are the context's area.

use super::wasm_frame::{global_area, storage_of};
use super::wasm_stage::GlobalStage;
use crate::staged::wasm_instance::WasmInstance;
use crate::words::{HOLE_A, HOLE_B, Out, S, hole64, slot};

/// `D = global`: the word of the global, narrowed to its storage.
#[inline(always)]
pub(crate) unsafe fn global_get<const OP: u32>(s: S) -> Out {
    let storage = const { storage_of(OP) };
    let mut instance = GlobalStage { area: global_area(&s) };
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
    let mut instance = GlobalStage { area: global_area(&s) };
    let value = unsafe { *(slot::<HOLE_A>(&s) as *const u64) };
    instance.global_store(hole64::<HOLE_B>() as u32, storage, value);
    Out::Next(s)
}
