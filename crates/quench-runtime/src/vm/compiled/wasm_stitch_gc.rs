//! The GC operations of the b-jit's Wasm functions (`Step::Gc`), each a VM callout row
//! (`call_w_31_<flags>_1_0`, `quench-bjit/stencils/wasm_rows_callout.rs`) that calls
//! `wasm_gc_callout::<H>` (`wasm_gc_callout.rs`). The callout runs the staged rule the interpreter
//! runs, so both tiers share its checks and traps.
//!
//! Stitched here: `array.len`, `ref.is_null`, `ref.as_non_null`, `ref.eq`, `ref.test` and
//! `ref.cast`. Not stitched, so the function stays interpreted:
//!
//! - `GcOp::Field` (`struct.get`, `array.set`, ...): the frame holds Wasm bits without types, and a
//!   stored field value cannot be converted back to a frame word, since an i32 and an `i31`
//!   reference are both `Value::integer`. Converting needs the field's declared Wasm type.
//! - `GcOp::Allocating`: an allocation can make a collection due, and a collection would free the
//!   references a stitched frame holds, since the frame roots none yet.
//!
//! The callout's address is the one of the Host type the stitched code belongs to (`World`), and
//! the rows are gated on `World::memory_stable` as the other callout rows are.
//!
//! A child of `wasm_stitch` (`#[path]`), so it names `wasm_stitch`'s helpers through `super`.

use super::super::wasm_gc_callout::GcCall;
use super::{Planned, World, leak, slot};
use crate::bytecode::{Op, WideInstruction};
use crate::staged::wasm_gc::GcOp;
use quench_bjit::padded_holes;
use quench_bjit::wasm as codes;

/// Two 32-bit values in one hole: `low` in bits 0..32, `high` in 32..64.
fn pack(low: u32, high: u32) -> u64 {
    u64::from(low) | u64::from(high) << 32
}

/// The frame offset of `register`, which a 32-bit hole holds.
fn offset(register: u16) -> Option<u32> {
    u32::try_from(slot(register)).ok()
}

/// The rows of the GC operation `op` of `instruction`, or `None` where it has no callout row (a
/// field operation or an allocation, see the module's header), or a binding the row cannot hold.
pub(super) fn gc(instruction: WideInstruction, op: GcOp, world: &World<'_>) -> Option<Vec<Planned>> {
    // The callout runs no Wasm code, but a row that calls a VM callout shares the memory gate of
    // the others: the memory base must not move during one.
    if !world.memory_stable {
        return None;
    }
    // `(call, result flags, second reference)`: a reference result is written whole (no wrap),
    // an i32 result is wrapped by the row (`CALLOUT_WRAP`).
    let (call, flags, second) = match (op, instruction.op()) {
        (GcOp::ArrayLength, Op::WasmArrayLen) => (GcCall::ArrayLength, codes::CALLOUT_RESULT | codes::CALLOUT_WRAP, None),
        (GcOp::Null, Op::WasmRefIsNull) => (GcCall::IsNull, codes::CALLOUT_RESULT | codes::CALLOUT_WRAP, None),
        (GcOp::Null, Op::WasmRefAsNonNull) => (GcCall::AsNonNull, codes::CALLOUT_RESULT, None),
        (GcOp::Equal, Op::WasmRefEq) => (GcCall::RefEq, codes::CALLOUT_RESULT | codes::CALLOUT_WRAP, Some(instruction.c())),
        (GcOp::Test, Op::WasmRefTest) => (GcCall::RefTest, codes::CALLOUT_RESULT | codes::CALLOUT_WRAP, None),
        (GcOp::Test, Op::WasmRefCast) => (GcCall::RefCast, codes::CALLOUT_RESULT, None),
        _ => return None,
    };
    let destination = offset(instruction.a())?;
    let reference = offset(instruction.b())?;
    let second = match second {
        Some(register) => offset(register)?,
        None => codes::CALLOUT_ABSENT,
    };
    let length = u32::try_from(super::super::CONTEXT_MEMORY_LENGTH).ok()?;
    // Holes: the callout; `HOLE_A` (destination, reference); `HOLE_B` (second, absent `c`);
    // `HOLE_FOURTH` (absent value, memory length offset); the request (tag, immediate).
    let request = call.tag() | u64::from(instruction.imm()) << 8;
    let holes = [
        world.gc_callout,
        pack(destination, reference),
        pack(second, codes::CALLOUT_ABSENT),
        pack(codes::CALLOUT_ABSENT, length),
        request,
    ];
    let name = leak(codes::wasm_name((codes::CALLOUT, flags, codes::W64, codes::REGISTER)));
    Some(vec![Planned { stencil: name, holes: padded_holes(holes), taken: None }])
}
