//! The instance rows that call the VM: `memory.grow` (`a = memory.grow b by c`). The u-jit lowers
//! it as a callout (`Memory::Grow`, `wasm_memory_stage.rs`): `memory_grow_callout::<H>(context,
//! pages)`, which grows the context's memory (`Vm::wasm_memory_grow`), refreshes the context's
//! `memory_length` (`refresh_memory_length`) and returns the size in pages before, or -1 where
//! the memory cannot grow. The row makes the same call through the context's `memory_grow` slot
//! (`GrowStage`, `wasm_stage.rs`, under the staged `WasmMemoryGrow` rule), then reloads the frame's
//! length word from the context, as the u-jit reads it again after a callout (`reload_memory`,
//! `wasm_frame.rs`).
//!
//! The base never moves under this row: the stitcher plans it only where the memory is a guarded
//! reservation (`World::memory_stable`, `wasm_stitch_instance.rs`), the case in which the u-jit
//! admits callouts too (`compiled.rs`, `analysis.callouts`), and a reservation grows in place.
//! So only the length changes.
//!
//! Holes (`wasm_stitch_instance.rs` plans them):
//! - hole 0: D's frame offset (the result, an i32 word).
//! - HOLE_A: the pages register's frame offset (c).
//! - HOLE_B: the offset of `memory_grow` in `CallContext`.
//! - HOLE_FOURTH: the offset of `memory_length` in `CallContext`.
//!
//! The row's name is `call_w_20_0_1_0` (kind `MEMORY_GROW`) (`wasm_library.rs` makes a `call_` name for it), since it
//! calls through a function pointer, which `build/checks.rs` requires of a `call_` row.

use super::wasm_frame::{read, reload_memory, write};
use super::wasm_stage::GrowStage;
use crate::staged::wasm_memory::WasmMemoryGrow;
use crate::words::{HOLE_A, HOLE_FOURTH, Out, S, hole64};

/// `a = memory.grow b by c`: the pages register's word (an i32, zero-extended), grown by the VM's
/// callout; the result is the i32 word of the size before, or -1.
#[inline(always)]
pub(crate) unsafe fn call_memory_grow(mut s: S) -> Out {
    unsafe {
        let before = GrowStage::of(&s).memory_grow(read::<HOLE_A>(&s));
        // The callout refreshed the context's length: the frame's length word takes it, and the
        // pins are reloaded from the header.
        reload_memory(&mut s, hole64::<HOLE_FOURTH>() as usize);
        write::<0>(&s, before);
    }
    Out::Next(s)
}
