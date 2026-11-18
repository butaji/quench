//! The instance rows that call the VM: `memory.grow` (`a = memory.grow b by c`). The u-jit lowers
//! it as a callout (`Memory::Grow`, `wasm_memory_stage.rs`): `memory_grow_callout::<H>(context,
//! pages)`, which grows the context's memory (`Vm::wasm_memory_grow`), refreshes the context's
//! `memory_length` (`refresh_memory_length`) and returns the size in pages before, or -1 where
//! the memory cannot grow. The row makes the same call through the context's `memory_grow` slot,
//! then reloads the frame's length word from the context, as the u-jit reads it again after a
//! callout (`reload_memory` in `wasm_rows.rs`).
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
//! The row's name is `call_w_30_0_1_0` (`wasm_library.rs` makes a `call_` name for it), since it
//! calls through a function pointer, which `build/checks.rs` requires of a `call_` row.

use super::{read, reload_memory, write};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64};

/// `memory_grow_callout::<H>`: (context, pages) -> the size in pages before as a word, or -1
/// (all ones) where the memory cannot grow. Trap-free.
type Grow = unsafe extern "C" fn(usize, u64) -> u64;

/// `a = memory.grow b by c`: the pages register's word (an i32, zero-extended), grown by the VM's
/// callout; the result is the i32 word of the size before, or -1.
#[inline(always)]
pub(crate) unsafe fn memory_grow(mut s: S) -> Out {
    unsafe {
        let (grow_at, length_at) = (hole64::<HOLE_B>() as usize, hole64::<HOLE_FOURTH>() as usize);
        // The pages operand is an i32 word, zero-extended in its frame word.
        let pages = u64::from(read::<HOLE_A>(&s) as u32);
        let grow: Grow = core::mem::transmute(*((s.ctx + grow_at) as *const usize));
        let before = grow(s.ctx, pages);
        // The callout refreshed the context's length: the frame's length word takes it, and the
        // pins are reloaded from the header.
        reload_memory(&mut s, length_at);
        // The result is an i32 word (`WrapI64` in the u-jit), zero-extended as every i32 register.
        write::<0>(&s, u64::from(before as u32));
    }
    Out::Next(s)
}
