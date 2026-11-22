//! The native roots of a stitched frame's calls (`wasm_roots.rs` is the u-jit's): each row that can
//! reach the VM (a `call_` row: a known or indirect call, a VM callout of a table operation, a
//! `memory.grow`, a VM call of an unstitched callee) is bracketed by `ROOT_PUSH` rows holding the
//! instruction's reference registers (`World::references`, `Plan::references`), and a `ROOT_POP`
//! right after it, so the pop runs before any leave that follows the call (a tail call). The
//! registers go four to a push row. A call that exits with a status leaves its pushes held: the
//! driver that entered the frame (`enter_wasm`) restores the depth, as it does for the u-jit.
//!
//! A function with no reference registers at a call places nothing, so only the functions that
//! hold references change. The GC rows (`Step::Gc`) are not wrapped: no callout reaches the VM's
//! collector (`wasm_stitch_gc.rs`).
//!
//! A child of `wasm_stitch` (`#[path]`), so it names `wasm_stitch`'s helpers through `super`.

#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::compiled::{CONTEXT_NATIVE_ROOTS, NativeRoots};
use super::{Planned, Target, World, leak};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::wasm_step::Step;
use quench_bjit::padded_holes;
use quench_bjit::wasm as codes;

/// The registers one `ROOT_PUSH` row holds: the row's hole names at most four, 16 bits each.
const CHUNK: usize = 4;

/// The name prefix of a row that makes a call (`wasm_name`'s `call_`).
const CALL: &str = "call_";

/// Two 32-bit values in one hole: `low` in bits 0..32, `high` in 32..64.
fn pair(low: u32, high: u32) -> u64 {
    u64::from(low) | u64::from(high) << 32
}

/// The byte offsets of the native roots' fields a root row reads (`NativeRoots::DEPTH`,
/// `CAPACITY`, `SLOTS`) and of the context's pointer to them (`CONTEXT_NATIVE_ROOTS`), as holes.
fn root_offsets() -> Option<(u32, u32, u32, u32)> {
    Some((
        u32::try_from(CONTEXT_NATIVE_ROOTS).ok()?,
        u32::try_from(NativeRoots::DEPTH).ok()?,
        u32::try_from(NativeRoots::CAPACITY).ok()?,
        u32::try_from(NativeRoots::SLOTS).ok()?,
    ))
}

/// The `ROOT_PUSH` rows holding `registers`, four to a row.
fn pushes(registers: &[u16]) -> Option<Vec<Planned>> {
    let (context, depth, capacity, slots) = root_offsets()?;
    registers
        .chunks(CHUNK)
        .map(|chunk| {
            let mut packed = 0u64;
            for (index, register) in chunk.iter().enumerate() {
                packed |= u64::from(*register) << (16 * index as u32);
            }
            // Holes: the packed registers; the count and the context's pointer to the roots; the
            // depth's and the capacity's offsets; the slots' offset.
            let holes = [packed, pair(chunk.len() as u32, context), pair(depth, capacity), u64::from(slots)];
            let name = leak(codes::wasm_name((codes::ROOT_PUSH, 0, codes::W64, codes::REGISTER)));
            Some(Planned { stencil: name, holes: padded_holes(holes), taken: None })
        })
        .collect()
}

/// The `ROOT_POP` row releasing `count` roots.
fn pop(count: usize) -> Option<Planned> {
    let (context, depth, _, _) = root_offsets()?;
    let count = u32::try_from(count).ok()?;
    let holes = [0, pair(count, context), u64::from(depth), 0];
    let name = leak(codes::wasm_name((codes::ROOT_POP, 0, codes::W64, codes::REGISTER)));
    Some(Planned { stencil: name, holes: padded_holes(holes), taken: None })
}

/// Whether `planned` is a row that makes a call.
fn is_call(planned: &Planned) -> bool {
    planned.stencil.starts_with(CALL)
}

/// The rows of the instruction at `pc` (`instruction`), with the native roots around each of its
/// call rows (see the module's header). `None` where a root row has no encoding.
pub(crate) fn rooted(pc: usize, instruction: WideInstruction, rows: Vec<Planned>, world: &World<'_>) -> Option<Vec<Planned>> {
    // The GC rows make no call that reaches the VM's collector.
    if matches!(Step::of(instruction.op(), instruction.imm()), Some(Step::Gc(_))) {
        return Some(rows);
    }
    let registers = (world.references)(pc);
    if registers.is_empty() || !rows.iter().any(is_call) {
        return Some(rows);
    }
    let pushed = registers.len().div_ceil(CHUNK);
    // Each old row's new index: a call row starts with its pushes, then the call and its pop.
    let mut at = Vec::with_capacity(rows.len());
    let mut size = 0;
    for planned in &rows {
        at.push(size);
        size += if is_call(planned) { pushed + 2 } else { 1 };
    }
    let mut out = Vec::with_capacity(size);
    for mut planned in rows {
        // A row's target is an index into the instruction's rows; the rows moved, so it follows them.
        if let Some(Target::Row(row)) = planned.taken {
            planned.taken = Some(Target::Row(*at.get(row)?));
        }
        if is_call(&planned) {
            out.extend(pushes(&registers)?);
            out.push(planned);
            out.push(pop(registers.len())?);
        } else {
            out.push(planned);
        }
    }
    Some(out)
}
