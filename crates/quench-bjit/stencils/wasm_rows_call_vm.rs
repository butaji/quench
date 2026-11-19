//! The `call_vm` row: a direct call (`CallKnown`) of a defined function the stitcher did not
//! stitch, made through the VM's callout (`vm/compiled/wasm_call_vm.rs`). The callee runs in the
//! VM, as the interpreter runs it, and the stitched caller continues from the callout's status.
//!
//! It is the `call` row (`wasm_rows.rs`) with a callout in place of the callee's entry cell: the
//! operand is the defined-function index, not a cell, and the callout takes the index, the
//! arguments and the result word, as `reference_callout` does. The protocol is the staged rule
//! `staged::wasm_call::call_defined`: the callout, then the memory reload, then the status check.
//!
//! The VM may grow the memory, which can move its base, so the row is planned only where the base
//! is stable (`World::memory_stable`): the reload takes the length and never the base.
//!
//! Holes (all read with `hole64`, so each packs what it needs; `wasm_stitch.rs` packs them):
//! - H0: low 32 bits, the result's frame offset.
//! - HOLE_A: low 32 bits, the call window's frame offset; high 32 bits, the offset of
//!   `memory_length` in `CallContext` (`CONTEXT_MEMORY_LENGTH`).
//! - HOLE_B: the address of the callout (`World::vm_call`).
//! - HOLE_FOURTH: the callee's defined-function index.

use crate::staged::wasm_call::call_defined;
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64};

use super::wasm_call_stage::{CallStage, leave_with};
use super::wasm_frame::{high, low};

/// `D = call the defined function at hole FOURTH`, through the VM callout at hole B; the
/// arguments are the registers from the window on.
#[inline(always)]
pub(crate) unsafe fn call_vm(mut s: S) -> Out {
    unsafe {
        let (h0, h_a, h_b, h_fourth) = (hole64::<0>(), hole64::<HOLE_A>(), hole64::<HOLE_B>(), hole64::<HOLE_FOURTH>());
        let destination = s.frame + low(h0) as usize;
        let window = s.frame + low(h_a) as usize;
        let length_at = high(h_a) as usize;
        let result = {
            let mut stage = CallStage { s: &mut s };
            call_defined(&mut stage, h_b as usize, h_fourth, window, destination, length_at)
        };
        leave_with(s, result)
    }
}
