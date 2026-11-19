//! The `call_indirect` row: a Wasm `call_indirect` stitched as one row. It makes the checks the
//! u-jit's `Call::IndirectTarget` makes (`staged::wasm_call::indirect_target`, emitted by
//! `quench-runtime/src/vm/compiled/wasm_table_stage.rs`) and then the call `Call::Reference` makes
//! (`call_reference` there), in the same order and with the same traps. A child of `wasm_rows`
//! (`#[path]`), so the row names `wasm_rows`'s own helpers.
//!
//! Fast path: the element is in bounds, holds the expected canonical signature, has an entry in
//! the running instance, and the row's body (the body the instance publishes for that entry,
//! `TableTarget::body`) is compiled. It calls the body exactly as `call()` does (`wasm_rows.rs`).
//! Every other case (null or uninitialized element, a related signature, another instance, a
//! body that is not compiled) goes through the VM's callout, which resolves the element again,
//! checks it and runs it, as the u-jit's `call_through_vm` does. So the row never needs a relate
//! step of its own: the bounds trap is the only one it raises.
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

use crate::wasm::WasmTrap;
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole64};

use super::{reload_memory, trap};

use crate::wasm_codes::{
    CONTEXT_CALL_REFERENCE_AT, CONTEXT_INSTANCE_AT, CONTEXT_MEMORY_LENGTH_AT, CONTEXT_TABLE_WORDS_AT, TARGET_BODY_AT, TARGET_BYTES,
    TARGET_INSTANCE_AT, TARGET_SIGNATURE_AT,
};

// The offsets and sizes the row reads are defined once, in `wasm_codes.rs`; the runtime asserts
// each equal to its own layout (`quench-runtime/src/vm/compiled/wasm_codes_check.rs`).
// The row does not test `NO_ENTRY`: a row's body is 0 unless it names a compiled function of its
// instance (`InstanceContext::published_body` publishes 0 for `NO_ENTRY`), so `body != 0` covers it.

/// `TableWords`: the first row of the table's mirror and its length in elements.
#[repr(C)]
struct Words {
    base: usize,
    length: u64,
}

/// A compiled body, `WasmEntry`'s ABI (`wasm_rows.rs`).
type Body = unsafe extern "C" fn(usize, u64, u64, *const u64, *mut u64) -> u32;
/// The VM's `reference_callout` (`vm/compiled.rs`): (context, table binding, element index,
/// type index, site id, arguments, result) -> status, 0 when the call returned.
type Callout = unsafe extern "C" fn(usize, u64, u64, u64, u64, *const u64, *mut u64) -> i64;

#[inline(always)]
fn low(word: u64) -> u32 {
    word as u32
}

#[inline(always)]
fn high(word: u64) -> u32 {
    (word >> 32) as u32
}

/// Leaves with `status` after a call: the memory's length is the context's and the pins are
/// reloaded (`reload_memory`, as `call()` does), and a nonzero status leaves.
#[inline(always)]
fn finish(mut s: S, status: u64, length_at: usize) -> Out {
    unsafe { reload_memory(&mut s, length_at) };
    if status != 0 {
        s.p[0] = status as usize;
        return Out::Exit(s);
    }
    Out::Next(s)
}

/// `D = call_indirect(index B) of the signature the holes name`, as the module's table `T`
/// holds it (see the header for the holes). The arguments are the registers from the window on.
#[inline(always)]
pub(crate) unsafe fn call_indirect(s: S) -> Out {
    let (h0, h1, h2, h3) = (hole64::<0>(), hole64::<HOLE_A>(), hole64::<HOLE_B>(), hole64::<HOLE_FOURTH>());
    let frame = s.frame;
    let context = s.ctx;
    let destination = frame + low(h0) as usize;
    let window = frame + high(h0) as usize;
    let index_at = frame + low(h1) as usize;
    let table = high(h1) as usize;
    let expected = low(h2);
    unsafe {
        // The index is an i32, zero-extended in its frame word.
        let index = *(index_at as *const u64);
        // `table_words` is the array of the words per binding; the table's words are its `table`th.
        let array = *((context + CONTEXT_TABLE_WORDS_AT) as *const *const *const Words);
        let words = *array.add(table);
        // The target's first check (`indirect_target`): the bound. The rest are the slow path's.
        if index >= (*words).length {
            return trap(s, WasmTrap::UndefinedElement);
        }
        let row = (*words).base + index as usize * TARGET_BYTES;
        // The fast path, one compare and branch each: the expected id (the rule's first test,
        // `unless_equal`), a compiled body (0 also for an element with no entry), this instance.
        if *((row + TARGET_SIGNATURE_AT) as *const u32) == expected {
            let body = *((row + TARGET_BODY_AT) as *const usize);
            if body != 0 && *((row + TARGET_INSTANCE_AT) as *const usize) == *((context + CONTEXT_INSTANCE_AT) as *const usize) {
                let body: Body = core::mem::transmute(body);
                let status = body(context, s.p[1] as u64, s.p[2] as u64, window as *const u64, destination as *mut u64);
                return finish(s, u64::from(status), CONTEXT_MEMORY_LENGTH_AT);
            }
        }
        slow(s, (table, index), (high(h2), low(h3)), (window, destination))
    }
}

/// Every case but the fast one: the VM resolves the element, checks it (null, uninitialized, a
/// related signature or a mismatch trap there) and calls it.
#[inline(always)]
unsafe fn slow(s: S, (table, index): (usize, u64), (signature, site): (u32, u32), (window, destination): (usize, usize)) -> Out {
    let context = s.ctx;
    unsafe {
        let callout: Callout = core::mem::transmute(*((context + CONTEXT_CALL_REFERENCE_AT) as *const usize));
        let status = callout(context, table as u64, index, u64::from(signature), u64::from(site), window as *const u64, destination as *mut u64);
        finish(s, status as u64, CONTEXT_MEMORY_LENGTH_AT)
    }
}
