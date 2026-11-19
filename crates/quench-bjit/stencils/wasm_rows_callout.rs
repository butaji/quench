//! The VM callout row of the table, element-segment, data-segment and `memory.init` operations
//! (`TableOp`): one `call_w_31_<OP>_1_0` row per OP (`wasm_library.rs`, `CALLOUT`). It calls
//! `table_op_callout::<H>` (`vm/compiled/wasm_table_ops_stage.rs`), the callout the u-jit's
//! `Emitter::table_op` calls with the same request and words, so both tiers run one semantics.
//!
//! The callout's ABI, which the u-jit uses unchanged:
//!
//!     extern "C" fn(context: usize, request: u64, a: u64, b: u64, c: u64, value: u64, out: *mut u64) -> i64
//!
//! - `request`: the operation's tag in bits 0..8, its first binding's environment slot in bits
//!   8..24 and its second's in bits 24..40 (`wasm_stitch_table.rs`, `table_op`).
//! - `a`, `b`, `c`: the operation's word operands (`TableOp::words`), each a zero-extended frame
//!   word; an absent one is 0.
//! - `value`: the reference's bits (`TableOp::value`), an absent one is 0.
//! - `out`: always a valid pointer. On success the callout writes its result word there, so a row
//!   with no result passes a scratch word of its own, never a frame slot it would corrupt.
//! - the status: 0 when the operation ran, else the status a trap leaves with.
//!
//! Holes (all read with `hole64`; `CALLOUT_ABSENT` marks an operand a row does not read):
//! - hole 0: the callout's address, `table_op_callout::<H> as *const () as u64`.
//! - `HOLE_A`: low 32 bits, the result's frame offset (read only when `CALLOUT_RESULT`);
//!   high 32 bits, operand `a`'s frame offset.
//! - `HOLE_B`: low 32 bits, operand `b`'s frame offset; high 32 bits, operand `c`'s.
//! - `HOLE_FOURTH`: low 32 bits, the reference's frame offset; high 32 bits, the offset of
//!   `memory_length` in `CallContext`.
//! - `LAST_HOLE`: the request word.
//!
//! After the call the memory length is the context's and the pins are reloaded (`reload_memory`,
//! as `call()` does), whether or not the operation trapped. A `CALLOUT_WRAP` result is an i32
//! (`Size`, `Grow`), so its word is zero-extended from its low 32 bits (`-1` is `0xffff_ffff`).
//!
//! The callout's address is per compiled program's Host type: the stitched code is produced by
//! `Vm<H>` for its own `H`, so it names that `H`'s callout (`stitched_entry`, `compiled.rs`).

use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, LAST_HOLE, Out, S, hole64};
use crate::wasm_codes::{CALLOUT_ABSENT, CALLOUT_RESULT, CALLOUT_WRAP};

use super::reload_memory;

/// The table callout's shape (see the header).
type Callout = unsafe extern "C" fn(usize, u64, u64, u64, u64, u64, *mut u64) -> i64;

#[inline(always)]
fn low(word: u64) -> u32 {
    word as u32
}

#[inline(always)]
fn high(word: u64) -> u32 {
    (word >> 32) as u32
}

/// The word at frame offset `offset`, or zero where the operand is absent.
#[inline(always)]
unsafe fn operand(frame: usize, offset: u32) -> u64 {
    if offset == CALLOUT_ABSENT {
        0
    } else {
        unsafe { *((frame + offset as usize) as *const u64) }
    }
}

/// The table callout of `OP` (`CALLOUT_RESULT`, `CALLOUT_WRAP`): the operation's words from the
/// frame, its result to the destination when `CALLOUT_RESULT`, then the memory pins reloaded. A
/// nonzero status leaves.
#[inline(always)]
pub(crate) unsafe fn callout<const OP: u32>(mut s: S) -> Out {
    let (h_callout, h_a, h_b, h_fourth, h_request) =
        (hole64::<0>(), hole64::<HOLE_A>(), hole64::<HOLE_B>(), hole64::<HOLE_FOURTH>(), hole64::<LAST_HOLE>());
    let (frame, context) = (s.frame, s.ctx);
    let (destination, first) = (low(h_a), high(h_a));
    let (second, third) = (low(h_b), high(h_b));
    let (reference, length_at) = (low(h_fourth), high(h_fourth) as usize);
    let mut scratch = 0u64;
    unsafe {
        let a = operand(frame, first);
        let b = operand(frame, second);
        let c = operand(frame, third);
        let value = operand(frame, reference);
        let out = if OP & CALLOUT_RESULT != 0 {
            (frame + destination as usize) as *mut u64
        } else {
            core::ptr::addr_of_mut!(scratch)
        };
        let callout: Callout = core::mem::transmute(h_callout as usize);
        let status = callout(context, h_request, a, b, c, value, out);
        reload_memory(&mut s, length_at);
        if status != 0 {
            s.p[0] = status as usize;
            return Out::Exit(s);
        }
        if OP & CALLOUT_WRAP != 0 {
            *out = u64::from(*out as u32);
        }
    }
    Out::Next(s)
}

/// The allocating GC step's callout (`vm/compiled/wasm_gc_allocate.rs`, `allocate_frame`):
/// (context, the packed instruction, the immediate and count word, the frame, the operand
/// descriptor, the result word) -> status.
type Allocate = unsafe extern "C" fn(usize, u64, u64, usize, usize, *mut u64) -> i64;

/// The bit of the immediate-and-count word that says the step has a result, for the instruction's
/// destination `a` (`wasm_stitch_gc.rs`, `allocating`).
const RESULT_BIT: u32 = 40;

/// The allocating GC step of a stitched frame: the callout reads the operands from the frame, the
/// descriptor names each one's register and kind, and runs the interpreter's step. A step with a
/// result writes it to the destination register (the instruction's `a`). The callout allocates
/// but runs no interpreter code, so the frame holds nothing across it (`wasm_gc_allocate.rs`).
#[inline(always)]
pub(crate) unsafe fn gc_allocate(mut s: S) -> Out {
    unsafe {
        let (packed, meta, descriptor) = (hole64::<HOLE_A>(), hole64::<HOLE_B>(), hole64::<HOLE_FOURTH>());
        let callout: Allocate = core::mem::transmute::<usize, Allocate>(hole64::<0>() as usize);
        let mut word = 0u64;
        let status = callout(s.ctx, packed, meta, s.frame, descriptor as usize, core::ptr::addr_of_mut!(word));
        if status != 0 {
            s.p[0] = status as usize;
            return Out::Exit(s);
        }
        if (meta >> RESULT_BIT) & 1 != 0 {
            let destination = s.frame + 8 * (crate::wasm_codes::FRAME_HEADER_WORDS as usize + ((packed >> 16) & 0xffff) as usize);
            *(destination as *mut u64) = word;
        }
    }
    Out::Next(s)
}
