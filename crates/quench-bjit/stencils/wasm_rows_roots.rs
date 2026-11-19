//! The native-root rows of a stitched frame: `ROOT_PUSH` holds the frame's reference registers in
//! the context's native roots (`NativeRoots`, `vm/compiled/instance_context.rs`) before a call that
//! can reach the VM, and `ROOT_POP` releases them after the call returns. The layout is the u-jit's
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

use super::{FRAME_HEADER_WORDS, trap};
use crate::wasm::WasmTrap;
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64};

#[inline(always)]
fn low(word: u64) -> usize {
    word as u32 as usize
}

#[inline(always)]
fn high(word: u64) -> usize {
    (word >> 32) as usize
}

/// The `NativeRoots` the context names at byte offset `at` (`native_roots`).
#[inline(always)]
unsafe fn roots(s: &S, at: usize) -> *mut u8 {
    unsafe { *(s.ctx.wrapping_add(at) as *const usize) as *mut u8 }
}

/// Holds the registers named by hole 0 (`count` of them) in the native roots, in order after the
/// live ones. A push with no room for them traps `CallStackExhausted`, writing nothing.
#[inline(always)]
pub(crate) unsafe fn root_push(s: S) -> Out {
    unsafe {
        let (registers, count_and_context) = (hole64::<0>(), hole64::<HOLE_A>());
        let (depth_and_capacity, slots_at) = (hole64::<HOLE_B>(), low(hole64::<HOLE_FOURTH>()));
        let count = low(count_and_context);
        let header = roots(&s, high(count_and_context));
        let (depth_at, capacity_at) = (low(depth_and_capacity), high(depth_and_capacity));
        let depth = *(header.wrapping_add(depth_at) as *const usize);
        let capacity = *(header.wrapping_add(capacity_at) as *const usize);
        let needed = depth.wrapping_add(count);
        if capacity < needed {
            return trap(s, WasmTrap::CallStackExhausted);
        }
        let slots = *(header.wrapping_add(slots_at) as *const *mut u64);
        let mut index = 0;
        while index < count {
            let register = ((registers >> (16 * index)) & 0xffff) as usize;
            let word = *((s.frame + 8 * (FRAME_HEADER_WORDS as usize + register)) as *const u64);
            *slots.add(depth.wrapping_add(index)) = word;
            index += 1;
        }
        *(header.wrapping_add(depth_at) as *mut usize) = needed;
    }
    Out::Next(s)
}

/// Releases the count of roots hole `HOLE_A` names: the depth goes back down by it.
#[inline(always)]
pub(crate) unsafe fn root_pop(s: S) -> Out {
    unsafe {
        let count_and_context = hole64::<HOLE_A>();
        let header = roots(&s, high(count_and_context));
        let depth_at = low(hole64::<HOLE_B>());
        let depth = *(header.wrapping_add(depth_at) as *const usize);
        *(header.wrapping_add(depth_at) as *mut usize) = depth.wrapping_sub(low(count_and_context));
    }
    Out::Next(s)
}
