//! The `call_indirect` row: a Wasm `call_indirect` stitched as one row. Its target is the staged
//! rule `staged::wasm_call::indirect_target_word`, the one the u-jit's `Call::IndirectTarget` runs
//! (`quench-runtime/src/vm/compiled/wasm_table_stage.rs`), over the stencil's `WasmTable`
//! (`wasm_call_stage.rs`): the bound traps, then the expected id is accepted at once. Its call is
//! the rule `call_entry` (the body) or `call_reference` (the slow path). A child of `wasm_rows`
//! (`#[path]`), so the row names `wasm_rows`'s own helpers.
//!
//! Fast path: the element is in bounds, holds the expected canonical signature (the rule's
//! `unless_equal`, one compare), has a compiled body (the body the instance publishes for that
//! entry, `TableTarget::body`; 0 also for an element with no entry), in the running instance. It
//! calls the body exactly as `call()` does (`wasm_rows.rs`). Every other case (null or
//! uninitialized element, a related signature, another instance, a body that is not compiled) is
//! deferred by the rule to the VM's callout, which resolves the element again, checks it and runs
//! it, as the u-jit's `call_through_vm` does. The slow path is the row's cold part.
//!
//! Holes (all read with `hole64`, so each packs several values; `wasm_stitch_indirect.rs` packs):
//! - H0: low 32 bits, the result's frame slot; high 32 bits, the call window's frame slot.
//! - H1: low 32 bits, the element index's frame slot (register `b`, written by the target row);
//!   high 32 bits, the table binding (the environment slot of its words, `CONTEXT_TABLE_WORDS`).
//! - H2: low 32 bits, the expected canonical signature word; high 32 bits, the program's type index
//!   (the callout's `signature_index`).
//! - H3: the call site id (the artifact's `reference_site_ids`).
//! - H4 (`LAST_HOLE`): unused.
//! The context offsets are `CallContext`'s (`vm/compiled.rs`, `CONTEXT_*`), constants here
//! (`CONTEXT_*_AT`) that the stitcher asserts equal to the runtime's, so the row unpacks no
//! hole for them. No table address is a hole: the table's words live behind the instance's
//! binding, which differs per instance, so the row reads them from the context each time, and a
//! body comes from the row itself, so the code table is not read.

use crate::staged::wasm_call::{call_entry, call_reference, indirect_target_word};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64};

use super::wasm_call_stage::{CallStage, Leave, NULL_SIGNATURE, TableStage, leave_with};
use super::wasm_frame::{high, low, trap};

use crate::wasm_layout::{
    CONTEXT_CALL_REFERENCE_AT, CONTEXT_INSTANCE_AT, CONTEXT_MEMORY_LENGTH_AT, CONTEXT_TABLE_WORDS_AT, TARGET_BODY_AT,
    TARGET_INSTANCE_AT,
};

// The offsets and sizes the row reads are defined once, in `wasm_layout.rs`; the runtime asserts
// each equal to its own layout (`quench-runtime/src/vm/compiled/wasm_codes_check.rs`).
// The row does not test `NO_ENTRY`: a row's body is 0 unless it names a compiled function of its
// instance (`InstanceContext::published_body` publishes 0 for `NO_ENTRY`), so `body != 0` covers it.

/// `TableWords`: the first row of the table's mirror and its length in elements.
#[repr(C)]
struct Words {
    base: usize,
    length: u64,
}

/// `D = call_indirect(index B) of the signature the holes name`, as the module's table `T`
/// holds it (see the header for the holes). The arguments are the registers from the window on.
#[inline(always)]
pub(crate) unsafe fn call_indirect(mut s: S) -> Out {
    unsafe {
        let (h0, h1, h2, h3) = (hole64::<0>(), hole64::<HOLE_A>(), hole64::<HOLE_B>(), hole64::<HOLE_FOURTH>());
        let frame = s.frame;
        let destination = frame + low(h0) as usize;
        let window = frame + high(h0) as usize;
        let index_at = frame + low(h1) as usize;
        let table = high(h1) as usize;
        let expected = u64::from(low(h2));
        // The index is an i32, zero-extended in its frame word.
        let index = *(index_at as *const u64);
        // `table_words` is the array of the words per binding; the table's words are its `table`th.
        let array = *((s.ctx + CONTEXT_TABLE_WORDS_AT) as *const *const *const Words);
        let words = *array.add(table);
        let mut stage = TableStage { base: (*words).base, length: (*words).length };
        let row = match indirect_target_word(&mut stage, index, expected, u64::from(NULL_SIGNATURE)) {
            Ok((_, row)) => row as usize,
            Err(Leave::Trap(error)) => return trap(s, error),
            // A related or uninitialized element, or a target the rule defers: the VM's callout.
            Err(_) => return slow(s, (table, index), (high(h2), low(h3)), (window, destination)),
        };
        // The fast path, one compare and branch each: a compiled body (0 also for an element with
        // no entry), and this instance.
        let body = *((row + TARGET_BODY_AT) as *const usize);
        if body != 0 && *((row + TARGET_INSTANCE_AT) as *const usize) == *((s.ctx + CONTEXT_INSTANCE_AT) as *const usize) {
            let result = {
                let mut stage = CallStage { s: &mut s };
                call_entry::<_, true>(&mut stage, body, window, destination, CONTEXT_MEMORY_LENGTH_AT)
            };
            return leave_with(s, result);
        }
        slow(s, (table, index), (high(h2), low(h3)), (window, destination))
    }
}

/// Every case but the fast one: the VM resolves the element, checks it (null, uninitialized, a
/// related signature or a mismatch trap there) and calls it, then the staged protocol
/// (`call_reference`). The cold path of the row: inlined, since a stencil may name no symbol but
/// its holes, and laid out after the hot part by the build's layout pass.
#[inline(always)]
unsafe fn slow(mut s: S, (table, index): (usize, u64), (signature, site): (u32, u32), (window, destination): (usize, usize)) -> Out {
    unsafe {
        let callout = *((s.ctx + CONTEXT_CALL_REFERENCE_AT) as *const usize);
        let reference = (table as u64, index, u64::from(signature), u64::from(site));
        let result = {
            let mut stage = CallStage { s: &mut s };
            call_reference(&mut stage, callout, reference, window, destination, CONTEXT_MEMORY_LENGTH_AT)
        };
        leave_with(s, result)
    }
}
