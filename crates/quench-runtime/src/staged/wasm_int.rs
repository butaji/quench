//! The Wasm i32 and i64 register and immediate operators as staged rows (`Decoder::WasmInt`).
//! The semantics are the stage's (`Stage::wasm_i32_binary` and its siblings: the interpreter's
//! `staged::wasm` rules on machine words); a trap or a stage that does not run the operator
//! answers `None`, and the row leaves to its general step, which raises the trap.

use super::{Frame, NumericFrame, Stage, Step};
use crate::bytecode::{Op, Register, WideInstruction};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator};

/// The right operand of an i32 binary operator: a register, or the record's immediate.
#[derive(Clone, Copy)]
pub(crate) enum I32Right {
    Register(Register),
    Constant(i32),
}

/// An integer operation of the family, with the operands its record names.
#[derive(Clone, Copy)]
pub(crate) enum WasmIntOperation {
    /// `destination = left op right` on i32 words.
    I32Binary { operator: I32BinaryOperator, destination: Register, left: Register, right: I32Right },
    /// `destination = op source` on an i32 word.
    I32Unary { operator: I32UnaryOperator, destination: Register, source: Register },
    /// `destination = left op right` on i64 words (`raw` false: boxed registers, read and written
    /// as the i64 they hold; `raw` true: the machine words as they are).
    I64Binary { operator: I64BinaryOperator, destination: Register, left: Register, right: Register, raw: bool },
    /// `destination = op source` on an i64 word (`raw` as for `I64Binary`).
    I64Unary { operator: I64UnaryOperator, destination: Register, source: Register, raw: bool },
    /// `destination = (source >>> shift) & mask` on an i32 word: a bit-field extraction, whose
    /// shift is the record's signed constant and whose mask is its immediate.
    I32BitField { destination: Register, source: Register, shift: i32, mask: i32 },
}

/// Whether the i64 opcode `op` runs on raw words (its operands and result are the machine words,
/// never boxed). The raw opcodes of the Wasm thread join here; none is in this catalog yet.
const fn raw_i64(op: Op) -> bool {
    let _ = op;
    false
}

/// The operation `instruction` denotes, decoded from the record's operands: an i32 binary's register
/// or immediate form, an i32 unary, or an i64 binary or unary. `None` for any other opcode.
pub(crate) fn decode(instruction: WideInstruction) -> Option<WasmIntOperation> {
    let op = instruction.op();
    let (destination, b, c) = (instruction.a(), instruction.b(), instruction.c());
    if let Some(operator) = I32BinaryOperator::from_register_op(op) {
        return Some(WasmIntOperation::I32Binary { operator, destination, left: b, right: I32Right::Register(c) });
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
        let right = I32Right::Constant(instruction.imm() as i32);
        return Some(WasmIntOperation::I32Binary { operator, destination, left: b, right });
    }
    match op {
        Op::WasmI32Unary => Some(WasmIntOperation::I32Unary {
            operator: I32UnaryOperator::from_tag(instruction.imm())?, destination, source: b }),
        Op::WasmI32ShiftRightUnsignedAndImmediate => Some(WasmIntOperation::I32BitField {
            destination, source: b, shift: i32::from(c as i16), mask: instruction.imm() as i32 }),
        Op::WasmI64Binary => Some(WasmIntOperation::I64Binary {
            operator: I64BinaryOperator::from_tag(instruction.imm())?, destination, left: b, right: c, raw: raw_i64(op) }),
        Op::WasmI64Unary => Some(WasmIntOperation::I64Unary {
            operator: I64UnaryOperator::from_tag(instruction.imm())?, destination, source: b, raw: raw_i64(op) }),
        _ => None,
    }
}

/// Run `operation` on `frame`: its result word, or `Unresolved` where the stage leaves it.
#[inline(always)]
pub(crate) fn step<S: Stage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: WasmIntOperation) -> Step<S::Word, F::Control> {
    match operation {
        WasmIntOperation::I32Binary { operator, destination, left, right } => {
            let left = frame.read(stage, left);
            let right = match right {
                I32Right::Register(register) => frame.read(stage, register),
                I32Right::Constant(value) => stage.constant(u64::from(value as u32)),
            };
            match stage.wasm_i32_binary(operator, left, right) {
                Some(word) => define_i32(stage, frame, destination, word),
                None => Step::Unresolved,
            }
        }
        WasmIntOperation::I32Unary { operator, destination, source } => {
            let value = frame.read(stage, source);
            match stage.wasm_i32_unary(operator, value) {
                Some(word) => define_i32(stage, frame, destination, word),
                None => Step::Unresolved,
            }
        }
        WasmIntOperation::I64Binary { operator, destination, left, right, raw } => {
            let (Some(left), Some(right)) = (frame.wide_read(stage, left, raw), frame.wide_read(stage, right, raw)) else {
                return Step::Unresolved;
            };
            match stage.wasm_i64_binary(operator, left, right) {
                Some((word, is_i32)) => define_wide(stage, frame, destination, word, is_i32, raw),
                None => Step::Unresolved,
            }
        }
        WasmIntOperation::I64Unary { operator, destination, source, raw } => {
            let Some(value) = frame.wide_read(stage, source, raw) else { return Step::Unresolved };
            match stage.wasm_i64_unary(operator, value) {
                Some((word, is_i32)) => define_wide(stage, frame, destination, word, is_i32, raw),
                None => Step::Unresolved,
            }
        }
        WasmIntOperation::I32BitField { destination, source, shift, mask } => {
            // The shift, then the mask, as the two i32 operators the extraction names.
            let value = frame.read(stage, source);
            let shift = stage.constant(u64::from(shift as u32));
            let Some(shifted) = stage.wasm_i32_binary(I32BinaryOperator::ShiftRightUnsigned, value, shift) else {
                return Step::Unresolved;
            };
            let mask = stage.constant(u64::from(mask as u32));
            match stage.wasm_i32_binary(I32BinaryOperator::And, shifted, mask) {
                Some(word) => define_i32(stage, frame, destination, word),
                None => Step::Unresolved,
            }
        }
    }
}

/// Define `destination` with the i32 `word`, boxed as the frame keeps an int32.
#[inline(always)]
fn define_i32<S: Stage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register, word: S::Word) -> Step<S::Word, F::Control> {
    let word = frame.int_result(stage, word);
    let fact = frame.int_fact();
    frame.write(stage, destination, word, fact);
    Step::Value(word)
}

/// Define `destination` with an i64 `word`, or with an i32 flag where `is_i32`.
#[inline(always)]
fn define_wide<S: Stage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register, word: S::Word, is_i32: bool, raw: bool)
    -> Step<S::Word, F::Control> {
    if is_i32 {
        return define_i32(stage, frame, destination, word);
    }
    match frame.wide_write(stage, destination, word, raw) {
        Some(()) => Step::Value(word),
        None => Step::Unresolved,
    }
}
