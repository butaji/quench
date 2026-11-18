//! The Wasm i32 and i64 register and immediate operators as staged rows (`Decoder::WasmInt`). The
//! operators are the staged Wasm rules every tier runs: the opcode's `WasmOperation` (its width,
//! operator and operands), `staged::wasm::integer_binary`'s traps, and `ConcreteWasm`'s primitives.
//! A trap, or an operand the frame does not hold as its word, leaves at the miss edge
//! (`Step::Unresolved`), where the general step raises it.

use super::machine::MachineStage;
use super::register_frame::{Frame, Step, WasmRep};
use super::wasm::{Width, binary_result, integer_binary, unary_result};
use super::wasm_concrete::ConcreteWasm;
use super::wasm_operation::{Form, Operand, WasmOperation};
use super::Concrete;
use crate::bytecode::{Register, WideInstruction};
use crate::wasm::integer::I64BinaryOperator as Binary;

/// An integer operation of the family: the rule its record's opcode and operator tag name, with
/// the record the operands and destination come from.
#[derive(Clone, Copy)]
pub(crate) struct WasmIntOperation {
    operation: WasmOperation,
    instruction: WideInstruction,
}

/// The operation `instruction` denotes: an i32 or i64 binary in its register or immediate form, an
/// i32 or i64 unary, or a bit-field extraction. `None` for any other opcode.
pub(crate) fn decode(instruction: WideInstruction) -> Option<WasmIntOperation> {
    let operation = WasmOperation::of(instruction.op(), instruction.imm())?;
    let integer_row = match operation {
        WasmOperation::Binary { form, .. } => matches!(form, Form::Register | Form::Immediate),
        WasmOperation::Unary { .. } | WasmOperation::BitField => true,
        _ => false,
    };
    integer_row.then_some(WasmIntOperation { operation, instruction })
}

/// The representation a scalar of `width` takes in a register frame.
const fn rep_of(width: Width) -> WasmRep {
    match width {
        Width::I32 => WasmRep::Int32,
        Width::I64 => WasmRep::Bits64,
    }
}

/// The result of an operation before it is written: its destination, word and representation.
type Computed = (Register, u64, WasmRep);

/// Run `operation` on the interpreter's frame: the staged rules over machine words, read and
/// written in each operand's `WasmRep`. An operand the frame does not hold so, and a trap, leave
/// at the miss edge (`Step::Unresolved`).
#[inline(always)]
pub(crate) fn step<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmIntOperation) -> Step<u64, F::Control> {
    let Some((destination, word, rep)) = computed(stage, frame, operation) else { return Step::Unresolved };
    match frame.wasm_write(stage, destination, word, rep) {
        Some(tagged) => Step::Value(tagged),
        None => Step::Unresolved,
    }
}

/// The word `operation` computes, with its destination and representation; `None` where an operand
/// is not held so or the operator traps.
#[inline(always)]
fn computed<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmIntOperation) -> Option<Computed> {
    let WasmIntOperation { operation, instruction } = operation;
    let destination = instruction.a();
    match operation {
        WasmOperation::Binary { operator, width, form } => {
            let operand = rep_of(width);
            let left = frame.wasm_read(stage, form.left(instruction), operand)?;
            let right = match form.right(instruction) {
                Operand::Register(register) => frame.wasm_read(stage, register, operand)?,
                Operand::Constant(value) => ConcreteWasm.constant(width, value),
            };
            let word = integer_binary(&mut ConcreteWasm, operator, width, left, right).ok()?;
            Some((destination, word, rep_of(binary_result(operator, width))))
        }
        WasmOperation::Unary { operator, width } => {
            let value = frame.wasm_read(stage, instruction.b(), rep_of(width))?;
            let word = ConcreteWasm.operate_unary(operator, width, value);
            Some((destination, word, rep_of(unary_result(operator, width))))
        }
        WasmOperation::BitField => {
            // The shift, then the mask, as the two i32 operators the extraction names.
            let value = frame.wasm_read(stage, instruction.b(), WasmRep::Int32)?;
            let shift = ConcreteWasm.constant(Width::I32, i64::from(instruction.c() as i16));
            let shifted = integer_binary(&mut ConcreteWasm, Binary::ShiftRightUnsigned, Width::I32, value, shift).ok()?;
            let mask = ConcreteWasm.constant(Width::I32, i64::from(instruction.imm() as i32));
            let word = integer_binary(&mut ConcreteWasm, Binary::And, Width::I32, shifted, mask).ok()?;
            Some((destination, word, WasmRep::Int32))
        }
        _ => None,
    }
}
