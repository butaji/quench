//! Runtime glue of `wasm_int`: the decode of an integer operation from its record, which reads the
//! record's opcode and fields (`WideInstruction`).
#![allow(unused_imports)]
use super::*;
use crate::bytecode::WideInstruction;
use crate::staged::raw_representation;
use crate::staged::wasm_operation::{Form, FormWasmOperationExt, Operand, WasmOperation, wasm_operation_of};

/// The operation `instruction` denotes: an i32 or i64 binary in its register or immediate form, an
/// i32 or i64 unary, or a bit-field extraction. `None` for any other opcode.
pub fn decode(instruction: WideInstruction) -> Option<WasmIntOperation> {
    let operation = wasm_operation_of(instruction.op(), instruction.imm())?;
    let (left, right, mask) = match operation {
        // A 16-bit constant right operand (`Form::Short`) is the i64 immediate form, raw or boxed.
        WasmOperation::Binary { form: form @ (Form::Register | Form::Immediate | Form::Short), .. } =>
            (form.left(instruction), form.right(instruction), 0),
        // A unary's operand is `b`; it has no right operand, so the placeholder is never read.
        WasmOperation::Unary { .. } => (instruction.b(), Operand::Constant(0), 0),
        WasmOperation::BitField => (
            instruction.b(),
            Operand::Constant(i64::from(instruction.c() as i16)),
            i64::from(instruction.imm() as i32),
        ),
        _ => return None,
    };
    Some(WasmIntOperation {
        operation,
        destination: instruction.a(),
        raw: raw_representation(instruction.op()),
        left,
        right,
        mask,
    })
}
