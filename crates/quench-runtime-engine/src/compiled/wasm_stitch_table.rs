//! The table, element-segment and data-segment operations of the b-jit's Wasm functions
//! (`Step::Table`, `TableOp`): each one a VM callout row (`call_w_21_<OP>_1_0`,
//! `quench-bjit/stencils/wasm_rows_callout.rs`). The callout is `table_op_callout`, the one
//! the u-jit's `Emitter::table_op` calls, with the same request encoding, the same word operands,
//! the same result handling and the same status, so the two tiers share one semantics.
//!
//! The bindings a table or segment names come from the function's full plan (`World::table_bindings`),
//! as the u-jit reads them (`Plan::operand`, `Plan::seconds`). No operation grows memory: the
//! callout runs no Wasm code, and `memory.init` writes without growing, so the rows need no
//! `World::memory_stable` gate. Every row reloads the memory length after its callout (`reload_memory`).
//!
//! A child of `wasm_stitch` (`#[path]`), so it names `wasm_stitch`'s helpers through `super`.

#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::compiled::CONTEXT_MEMORY_LENGTH;
use super::{Planned, World, leak, slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::wasm_table_ops::TableOp;
use quench_bjit::padded_holes;
use quench_bjit::wasm as codes;

/// The OP bits of the callout row an operation takes: whether it writes a result, and whether
/// that result is an i32 (`TableOp::Size`, `TableOp::Grow`, wrapped as the u-jit's `WrapI64`).
fn callout_flags(op: TableOp) -> u32 {
    match op {
        TableOp::Get => codes::CALLOUT_RESULT,
        TableOp::Size | TableOp::Grow => codes::CALLOUT_RESULT | codes::CALLOUT_WRAP,
        _ => 0,
    }
}

/// The frame offset of `register`, or the absent offset for none.
fn offset(register: Option<u16>) -> Option<u32> {
    match register {
        None => Some(codes::CALLOUT_ABSENT),
        Some(register) => u32::try_from(slot(register)).ok(),
    }
}

/// Two 32-bit values in one hole: `low` in bits 0..32, `high` in 32..64.
fn pack(low: u32, high: u32) -> u64 {
    u64::from(low) | u64::from(high) << 32
}

/// The rows of the table operation `op` at `pc` (`instruction`), or `None` where a binding the
/// plan does not name, or an offset a 32-bit hole cannot hold, has no row.
pub(crate) fn table(pc: usize, instruction: WideInstruction, op: TableOp, world: &World<'_>) -> Option<Vec<Planned>> {
    let (first, second) = match op {
        // The segment a drop names is its environment slot, the immediate.
        TableOp::SegmentDrop => (instruction.imm(), 0),
        _ => {
            let (first, second) = (world.table_bindings)(pc)?;
            (first?, second.unwrap_or(0))
        }
    };
    // The request's binding fields are 16 bits wide (`table_op_callout`).
    let (first, second) = (u16::try_from(first).ok()?, u16::try_from(second).ok()?);
    let request = op.tag() | u64::from(first) << 8 | u64::from(second) << 24;
    let [w0, w1, w2] = op.words(instruction);
    let (a, b, c) = (offset(w0)?, offset(w1)?, offset(w2)?);
    let value = offset(op.value(instruction))?;
    let flags = callout_flags(op);
    let destination = offset((flags & codes::CALLOUT_RESULT != 0).then_some(instruction.a()))?;
    let length = u32::try_from(CONTEXT_MEMORY_LENGTH).ok()?;
    let holes = [
        world.table_callout,
        pack(destination, a),
        pack(b, c),
        pack(value, length),
        request,
    ];
    let name = leak(codes::wasm_name((codes::CALLOUT, flags, codes::W64, codes::REGISTER)));
    Some(vec![Planned { stencil: name, holes: padded_holes(holes), taken: None }])
}
