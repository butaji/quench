//! The native-root rows of a stitched frame: `ROOT_PUSH` holds the frame's reference registers in
//! the context's native roots (`NativeRoots`, `vm/compiled/instance_context.rs`) before a call that
//! can reach the VM, and `ROOT_POP` releases them after the call returns. The layout and the rules
//! are the staged ones (`staged::wasm_roots`: `push`, `pop`), which the u-jit runs too
//! (`vm/compiled/wasm_roots.rs`): the roots are a buffer of Value words of which `depth` are live,
//! and a push past `capacity` traps `CallStackExhausted`, as the native stack's budget would.
//!
//! A collection runs only at an interpreter safepoint, so only the calls that can reach the VM need
//! these rows (`wasm_stitch_roots.rs` places them). The stitched frame's `depth` restore on an error
//! path is the driver's (`enter_wasm`), as the u-jit's is: a row that exits leaves its pushes held.
//!
//! Holes (all read with `hole64`; the offsets are `CallContext`'s and `NativeRoots`' fields):
//! - hole 0 (`ROOT_PUSH`): the registers, 16 bits each from the low bits, as frame register numbers.
//! - `HOLE_A`: low 32 bits, the count (`ROOT_PUSH`: the registers in hole 0, at most four; `ROOT_POP`:
//!   the roots to release); high 32 bits, the byte offset of `native_roots` in `CallContext`.
//! - `HOLE_B`: low 32 bits, the byte offset of `depth` in `NativeRoots`; high 32 bits, that of
//!   `capacity` (`ROOT_PUSH`).
//! - `HOLE_FOURTH` (`ROOT_PUSH`): the byte offset of `slots` in `NativeRoots`.
//!
//! The frame's registers are `8 * (FRAME_HEADER_WORDS + register)` bytes past the frame, as the
//! stitcher places them (`wasm_stitch.rs`, `slot`).

use super::wasm_call_stage::{RootsStage, leave_with};
use super::wasm_frame::{high, low, native_roots};
use crate::staged::wasm_roots::{pop, push};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64};
use crate::wasm_codes::FRAME_HEADER_WORDS;

/// Holds the registers named by hole 0 (`count` of them, at most four: 16 bits each) in the native
/// roots, in order after the live ones. A push with no room for them traps `CallStackExhausted`,
/// writing nothing. Each of the four words is read from its register when the count holds it, so
/// the array is built whole by its initializer.
#[inline(always)]
pub(crate) unsafe fn root_push(s: S) -> Out {
    unsafe {
        let (registers, count_and_context) = (hole64::<0>(), hole64::<HOLE_A>());
        let (depth_and_capacity, slots_at) = (hole64::<HOLE_B>(), low(hole64::<HOLE_FOURTH>()) as usize);
        let count = low(count_and_context) as usize;
        let word = |index: usize| -> u64 {
            if index < count {
                let register = ((registers >> (16 * index)) & 0xffff) as usize;
                *((s.frame + 8 * (FRAME_HEADER_WORDS as usize + register)) as *const u64)
            } else {
                0
            }
        };
        let words = [word(0), word(1), word(2), word(3)];
        let held = words.get(..count).unwrap_or(&words[..]);
        let mut stage = RootsStage {
            header: native_roots(s.ctx, high(count_and_context) as usize),
            depth_at: low(depth_and_capacity) as usize,
            capacity_at: high(depth_and_capacity) as usize,
            slots_at,
        };
        leave_with(s, push(&mut stage, held).map(|_depth| ()))
    }
}

/// Releases the count of roots hole `HOLE_A` names: the depth goes back down by it.
#[inline(always)]
pub(crate) unsafe fn root_pop(s: S) -> Out {
    unsafe {
        let count_and_context = hole64::<HOLE_A>();
        let mut stage = RootsStage {
            header: native_roots(s.ctx, high(count_and_context) as usize),
            depth_at: low(hole64::<HOLE_B>()) as usize,
            capacity_at: 0,
            slots_at: 0,
        };
        pop(&mut stage, low(count_and_context) as usize);
    }
    Out::Next(s)
}
