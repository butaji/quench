//! Runtime glue of `steps`: the part of an opcode's row (`row_step`, which names the runtime's `Op`
//! and `Decoder`), and the direct memory row's step (`step_wasm_access`), whose callee
//! `wasm_access::step` runs over the runtime's memory bytes.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Decoder, Op};
use crate::staged::{Concrete, Frame, Operation, Step};

#[cfg(not(stencil_build))]
/// The part of `step` opcode `op`'s row reaches.
pub const fn row_step(op: Op) -> RowStep {
    match op.staged() {
        Some(Decoder::Flow) => RowStep::Flow,
        Some(Decoder::Local) => RowStep::Local,
        Some(Decoder::WasmInt) => RowStep::WasmInt,
        Some(Decoder::Names) | Some(Decoder::Proved) => RowStep::Names,
        Some(Decoder::Property) => if matches!(op, Op::Delete) { RowStep::Keyed } else { RowStep::Property },
        Some(Decoder::Element) => RowStep::Keyed,
        Some(Decoder::WasmFloat) => RowStep::WasmFloat,
        // `IntCaseIndex` decodes to a Number operation (`NumberOperation::IntCase`), which the generic step does not run.
        Some(Decoder::Generic) => if matches!(op, Op::IntCaseIndex) { RowStep::Number }
            else if matches!(op, Op::Unary | Op::Binary | Op::NumericAdd | Op::NumericMultiply) { RowStep::Arith } else { RowStep::Generic },
        Some(Decoder::WasmJump) => RowStep::WasmJump,
        Some(Decoder::WasmState) => RowStep::WasmState,
        Some(Decoder::WasmMemory) => RowStep::WasmMemory,
        Some(Decoder::WasmVector) => RowStep::WasmVector,
        Some(Decoder::Lane) => match op {
            Op::TierPrologue | Op::BodyEntry => RowStep::Entry,
            _ => RowStep::Full,
        },
        Some(Decoder::Value) => match op {
            Op::LoadCapture | Op::StoreCapture => RowStep::Names,
            Op::IncDec | Op::ToNumeric => RowStep::Number,
            // A unary decodes to the operations the arithmetic step runs (`decode_unary`): a `typeof`
            // test or name, `!`, a Number operator or the VM's.
            Op::Unary => RowStep::Arith,
            Op::LoadThis | Op::LoadCallee | Op::RequireObjectCoercible | Op::RequireIteratorResult => RowStep::Value,
            _ => RowStep::Full,
        },
        _ => RowStep::Full,
    }
}

#[cfg(not(stencil_build))]
/// `step` of a direct memory row (`RowStep::WasmMemory`) on the interpreter's stage and frame.
#[inline(always)]
pub fn step_wasm_access<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: Operation<u64, ()>) -> Step<u64, F::Control> {
    match operation {
        Operation::WasmAccess(operation) => crate::staged::wasm_access::step(stage, frame, operation),
        _ => Step::Unresolved,
    }
}
