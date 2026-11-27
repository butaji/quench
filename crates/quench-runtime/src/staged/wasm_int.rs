//! The Wasm i32 and i64 register and immediate operators as staged rows (`Decoder::WasmInt`). The
//! operators are the staged Wasm rules every tier runs: the opcode's `WasmOperation` (its width,
//! operator and operands), `staged::wasm::integer_binary`'s traps, and `ConcreteWasm`'s primitives.
//! A trap, or an operand the frame does not hold as its word, leaves at the miss edge
//! (`Step::Unresolved`), where the general step raises it. The decode from a record, which reads the
//! record's fields, is in `wasm_int/glue.rs`.

use super::machine::MachineStage;
use super::register_frame::{Frame, Step, WasmRep, wasm_operand, wasm_result};
use super::wasm::{Width, binary_result, integer_binary, unary_result};
use super::wasm_concrete::ConcreteWasm;
use super::wasm_operation::{Operand, WasmOperation};
use super::Concrete;
use crate::bytecode::Register;
use crate::wasm::integer::I64BinaryOperator as Binary;

/// An integer operation of the family: the rule its record's opcode and operator tag name, with
/// the operands its record names already decoded (the operand registers, the right operand or
/// bit-field shift, the bit-field mask, and whether the i64 operands are raw words).
#[derive(Clone, Copy)]
pub struct WasmIntOperation {
    pub operation: WasmOperation,
    pub destination: Register,
    /// Whether the i64 operands are raw words (`staged::raw_representation`).
    pub raw: bool,
    /// The left operand register: a binary's `form.left`, else the unary's or bit-field's source.
    pub left: Register,
    /// A binary's right operand, or a bit-field's shift as a constant; unused by a unary.
    pub right: Operand,
    /// A bit-field's mask as a constant; zero for the other operations.
    pub mask: i64,
}

/// The representation a scalar of `width` takes in a register frame: an i64 is its raw word where
/// `raw` (`staged::raw_representation`), else boxed where untagged.
const fn rep_of(width: Width, raw: bool) -> WasmRep {
    match width {
        Width::I32 => WasmRep::Int32,
        Width::I64 if raw => WasmRep::RawBits64,
        Width::I64 => WasmRep::Bits64,
    }
}

/// The result of an operation before it is written: its destination, word and representation.
type Computed = (Register, u64, WasmRep);

/// Run `operation` on the interpreter's frame: the staged rules over machine words, read and
/// written in each operand's `WasmRep`. An operand the frame does not hold so, and a trap, leave
/// at the miss edge (`Step::Unresolved`).
#[inline(always)]
pub fn step<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmIntOperation) -> Step<u64, F::Control> {
    let Some((destination, word, rep)) = computed(stage, frame, operation) else { return Step::Unresolved };
    match wasm_result(stage, frame, destination, word, rep) {
        Some(tagged) => Step::Value(tagged),
        None => Step::Unresolved,
    }
}

/// The word `operation` computes, with its destination and representation; `None` where an operand
/// is not held so or the operator traps. A raw record reads and writes its i64 operands as raw words.
#[inline(always)]
fn computed<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmIntOperation) -> Option<Computed> {
    let WasmIntOperation { operation, destination, raw, left, right, mask } = operation;
    match operation {
        WasmOperation::Binary { operator, width, .. } => {
            let operand = rep_of(width, raw);
            let left = wasm_operand(stage, frame, left, operand)?;
            let right = match right {
                Operand::Register(register) => wasm_operand(stage, frame, register, operand)?,
                Operand::Constant(value) => ConcreteWasm.constant(width, value),
            };
            let word = integer_binary(&mut ConcreteWasm, operator, width, left, right).ok()?;
            Some((destination, word, rep_of(binary_result(operator, width), raw)))
        }
        WasmOperation::Unary { operator, width } => {
            let value = wasm_operand(stage, frame, left, rep_of(width, raw))?;
            let word = ConcreteWasm.operate_unary(operator, width, value);
            Some((destination, word, rep_of(unary_result(operator, width), raw)))
        }
        WasmOperation::BitField => {
            // The shift, then the mask, as the two i32 operators the extraction names.
            let Operand::Constant(shift) = right else { return None };
            let value = frame.wasm_read(stage, left, WasmRep::Int32)?;
            let shift = ConcreteWasm.constant(Width::I32, shift);
            let shifted = integer_binary(&mut ConcreteWasm, Binary::ShiftRightUnsigned, Width::I32, value, shift).ok()?;
            let mask = ConcreteWasm.constant(Width::I32, mask);
            let word = integer_binary(&mut ConcreteWasm, Binary::And, Width::I32, shifted, mask).ok()?;
            Some((destination, word, WasmRep::Int32))
        }
        _ => None,
    }
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
