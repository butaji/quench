//! The GC operations of the b-jit's Wasm functions (`Step::Gc`), each a row over a VM callout:
//!
//! - the reference and array-length operations (`array.len`, `ref.is_null`, `ref.as_non_null`,
//!   `ref.eq`, `ref.test`, `ref.cast`) and the struct and array field accesses (`struct.get`,
//!   `array.set`, ...) call `wasm_gc_callout::<H>` (`wasm_gc_callout.rs`) through the kind-21 row
//!   `call_w_21_<flags>_1_0` (`quench-bjit/stencils/wasm_rows_callout.rs`);
//! - the allocating operations (`struct.new`, `array.new*`, `ref.i31`, ...) call
//!   `allocate_frame::<H>` (`wasm_gc_allocate.rs`) through `call_w_26_0_1_0`
//!   (`wasm_rows_callout.rs`, `gc_allocate`), which reads the operands from the frame.
//!
//! The callouts run the staged rules the interpreter runs, so both tiers share their checks and
//! traps. None of them reaches the VM's collector (an allocation only makes one due, and a
//! collection runs at an interpreter safepoint), so their rows root nothing (`wasm_stitch_roots.rs`).
//!
//! The callouts' addresses are the Host type's (`World`), and the rows are gated on
//! `World::memory_stable` as the other callout rows are.
//!
//! A child of `wasm_stitch` (`#[path]`), so it names `wasm_stitch`'s helpers through `super`.

use super::super::CONTEXT_MEMORY_LENGTH;
use super::super::wasm_gc_allocate::{allocating_reads, allocating_result, pack};
use super::super::wasm_gc_callout::GcCall;
use super::super::wasm_gc_stage::{field_registers, structure_access};
use super::{Planned, World, leak, slot};
use crate::bytecode::WideInstruction;
use crate::staged::wasm_gc::{GcOp, GcStep};
use crate::wasm::gc::{GcField, GcFieldAccess, GcFieldValue};
use quench_bjit::padded_holes;
use quench_bjit::wasm as codes;

/// Two 32-bit values in one hole: `low` in bits 0..32, `high` in 32..64.
fn pair(low: u32, high: u32) -> u64 {
    u64::from(low) | u64::from(high) << 32
}

/// The frame offset of `register`, which a 32-bit hole holds.
fn offset(register: u16) -> Option<u32> {
    u32::try_from(slot(register)).ok()
}

/// The frame offset of an operand, or the absent offset for none (the row passes the zero word).
fn operand(register: Option<u16>) -> Option<u32> {
    match register {
        None => Some(codes::CALLOUT_ABSENT),
        Some(register) => offset(register),
    }
}

/// The row of one callout of the GC operation `call`: its result (`flags`' `CALLOUT_RESULT`) to
/// `destination`, its operands `a`, `b` and `c` (`operands`, the `CALLOUT` row's layout), and the
/// instruction's immediate in the request.
fn callout(call: GcCall, flags: u32, destination: u16, operands: [Option<u16>; 3], imm: u32, world: &World<'_>) -> Option<Vec<Planned>> {
    let destination = offset(destination)?;
    let [first, second, third] = operands;
    let (first, second, third) = (operand(first)?, operand(second)?, operand(third)?);
    let length = u32::try_from(CONTEXT_MEMORY_LENGTH).ok()?;
    // Holes: the callout; `HOLE_A` (destination, a); `HOLE_B` (b, c); `HOLE_FOURTH` (absent value,
    // memory length offset); the request (tag, immediate).
    let request = call.tag() | u64::from(imm) << 8;
    let holes = [world.gc_callout, pair(destination, first), pair(second, third), pair(codes::CALLOUT_ABSENT, length), request];
    let name = leak(codes::wasm_name((codes::CALLOUT, flags, codes::W64, codes::REGISTER)));
    Some(vec![Planned { stencil: name, holes: padded_holes(holes), taken: None }])
}

/// The rows of the field access `instruction` (`struct.get`, `array.set`, ...): the reference, the
/// array index (`None` for a struct) and the data, as the u-jit's `field_registers` names them.
/// A read writes the data register of the field's value class (an i32 wrapped by the row); a write
/// stores its data word, whose class the callout converts.
fn field(instruction: WideInstruction, world: &World<'_>) -> Option<Vec<Planned>> {
    let op = instruction.op();
    let access = GcStep::of(op)?.field()?.1;
    let field = GcField::from_tag(instruction.imm())?;
    if matches!(field.value(), GcFieldValue::Vector) {
        return None;
    }
    let write = matches!(access, GcFieldAccess::Write);
    let structure = structure_access(op);
    let (reference, index, data) = field_registers(instruction, write, structure);
    let call = match (structure, access) {
        (true, GcFieldAccess::Write) => GcCall::StructSet,
        (false, GcFieldAccess::Write) => GcCall::ArraySet,
        (true, GcFieldAccess::ReadSigned) => GcCall::StructGetSigned,
        (true, _) => GcCall::StructGet,
        (false, GcFieldAccess::ReadSigned) => GcCall::ArrayGetSigned,
        (false, _) => GcCall::ArrayGet,
    };
    // A write's data is the callout's `c`; a read's result goes to the data register.
    let (flags, operands) = if write {
        (0, [Some(reference), index, Some(data)])
    } else {
        let flags = match field.value() {
            GcFieldValue::Narrow => codes::CALLOUT_RESULT | codes::CALLOUT_WRAP,
            _ => codes::CALLOUT_RESULT,
        };
        (flags, [Some(reference), index, None])
    };
    callout(call, flags, data, operands, instruction.imm(), world)
}

/// The row of an allocating step (`wasm_gc_allocate.rs`): the instruction packed, its immediate,
/// the operand descriptor (each read register and its kind, `Plan::gc_reads`), and whether the step
/// has a result, which the row writes to the instruction's `a`. The descriptor is leaked: the row
/// holds its address for the life of the code, as `leak` keeps the row names.
fn allocating(pc: usize, instruction: WideInstruction, world: &World<'_>) -> Option<Vec<Planned>> {
    let reads = allocating_reads(instruction)?;
    let kinds = (world.gc_reads)(pc);
    if kinds.len() != reads.len() || kinds.iter().zip(&reads).any(|((register, _), read)| register != read) {
        return None;
    }
    let count = u64::try_from(reads.len()).ok().filter(|count| *count <= 0xff)?;
    let descriptor: Vec<u64> = kinds.iter().map(|(register, kind)| u64::from(*register) | *kind << 16).collect();
    let descriptor: &'static [u64] = Box::leak(descriptor.into_boxed_slice());
    let result = u64::from(allocating_result(instruction).is_some());
    // Holes: the callout; the packed instruction; the immediate and count, the result flag at bit 40;
    // the descriptor's address; unused.
    let meta = u64::from(instruction.imm()) | count << 32 | result << 40;
    let holes = [world.allocate_callout, pack(instruction) as u64, meta, descriptor.as_ptr() as usize as u64, 0];
    let name = leak(codes::wasm_name((codes::GC_ALLOCATE, 0, codes::W64, codes::REGISTER)));
    Some(vec![Planned { stencil: name, holes: padded_holes(holes), taken: None }])
}

/// The rows of the GC operation `op` of `instruction` at `pc`, or `None` where it has none (a
/// binding or operand a 32-bit hole cannot hold, a vector field, or the callouts' memory gate).
pub(super) fn gc(pc: usize, instruction: WideInstruction, op: GcOp, world: &World<'_>) -> Option<Vec<Planned>> {
    // The callouts run no Wasm code, but a row that calls a VM callout shares the memory gate of
    // the others: the memory base must not move during one.
    if !world.memory_stable {
        return None;
    }
    let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
    let imm = instruction.imm();
    // A reference result is written whole (no wrap); an i32 result is wrapped by the row.
    let wrapped = codes::CALLOUT_RESULT | codes::CALLOUT_WRAP;
    match (op, GcStep::of(instruction.op())) {
        (GcOp::ArrayLength, Some(GcStep::ArrayLen)) => callout(GcCall::ArrayLength, wrapped, a, [Some(b), None, None], imm, world),
        (GcOp::Null, Some(GcStep::IsNull)) => callout(GcCall::IsNull, wrapped, a, [Some(b), None, None], imm, world),
        (GcOp::Null, Some(GcStep::AsNonNull)) => callout(GcCall::AsNonNull, codes::CALLOUT_RESULT, a, [Some(b), None, None], imm, world),
        (GcOp::Equal, Some(GcStep::Equal)) => callout(GcCall::RefEq, wrapped, a, [Some(b), Some(c), None], imm, world),
        (GcOp::Test, Some(GcStep::Test { cast: false, descriptor: false })) => callout(GcCall::RefTest, wrapped, a, [Some(b), None, None], imm, world),
        (GcOp::Test, Some(GcStep::Test { cast: true, descriptor: false })) => callout(GcCall::RefCast, codes::CALLOUT_RESULT, a, [Some(b), None, None], imm, world),
        (GcOp::Field, _) => field(instruction, world),
        (GcOp::Allocating, _) => allocating(pc, instruction, world),
        _ => None,
    }
}
