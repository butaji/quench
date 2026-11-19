//! The Wasm vector rows (`Decoder::WasmVector`): a SIMD operator or byte shuffle a record denotes
//! and its step on the interpreter's frame, over the lane rules of `wasm_simd_rules`.

use super::register_frame::WasmRep;
use super::wasm_simd::WasmSimd;
use super::wasm_simd_concrete::{ConcreteSimd, execute};
use super::{Concrete, Constant, DecodeContext, Fields, Frame, Operation, Step};
use crate::bytecode::{Op, Register};
use crate::wasm::simd::SimdKernel;
use crate::{WasmType, WasmValue};

/// A Wasm vector operation on the lane's registers: a SIMD operator on its operands, or a byte
/// shuffle of two vectors by a constant's lane indices.
#[derive(Clone, Copy)]
pub(crate) enum VectorOperation {
    /// `destination = operator(left, right)`, the operator and its lane immediate `selector`
    /// (`WasmSimd`); `right` is read only where the operator takes two operands.
    Operator { destination: Register, left: Register, right: Register, selector: u32 },
    /// `destination = shuffle(left, right)` by the sixteen lane indices of `indices`
    /// (`WasmSimdShuffle`).
    Shuffle { destination: Register, left: Register, right: Register, indices: Constant },
}

impl VectorOperation {
    /// The operation `instruction` denotes (`Decoder::WasmVector`): an operator's selector is its
    /// immediate, a shuffle's indices its constant (read through `context`, or the lane's own).
    pub(crate) fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        let op = instruction.op();
        let wide = instruction.wide();
        Some(match op {
            Op::WasmSimd => Self::Operator {
                destination: wide.a(),
                left: wide.b(),
                right: wide.c(),
                selector: wide.imm(),
            },
            Op::WasmSimdShuffle => Self::Shuffle {
                destination: wide.a(),
                left: wide.b(),
                right: wide.c(),
                indices: instruction.constant(context)?,
            },
            _ => return None,
        })
    }
}

impl<W, P> Operation<W, P> {
    /// The Wasm vector operation `instruction` denotes (`Decoder::WasmVector`).
    pub(crate) fn decode_wasm_vector<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        VectorOperation::decode(instruction, context).map(Self::WasmVector)
    }
}

/// Run `operation` on `frame`: its vector, or `Unresolved` where the frame leaves it. A scalar
/// result (an extract, a bitmask) and an operand of another representation are left to the
/// general step, which computes or raises them.
#[inline(always)]
pub(crate) fn step<F: Frame<Concrete>>(
    stage: &mut Concrete,
    frame: &mut F,
    operation: VectorOperation,
) -> Step<u64, F::Control> {
    match operation {
        VectorOperation::Operator { destination, left, right, selector } => {
            let Some((kernel, _)) = SimdKernel::of(selector) else {
                return Step::Unresolved;
            };
            let Some(left) = vector_operand(stage, frame, left, kernel.left) else {
                return Step::Unresolved;
            };
            let right = match kernel.right {
                None => None,
                Some(ty) => match vector_operand(stage, frame, right, ty) {
                    Some(right) => Some(right),
                    None => return Step::Unresolved,
                },
            };
            match execute(selector, left, right) {
                WasmValue::V128(bits) => store_vector(stage, frame, destination, bits),
                _ => Step::Unresolved,
            }
        }
        VectorOperation::Shuffle { destination, left, right, indices } => {
            let (Some(indices), Some(left), Some(right)) = (
                frame.vector_constant(stage, indices),
                frame.vector(stage, left),
                frame.vector(stage, right),
            ) else {
                return Step::Unresolved;
            };
            let shuffled = ConcreteSimd.vector_shuffle(
                indices,
                u128::from_le_bytes(left),
                u128::from_le_bytes(right),
            );
            store_vector(stage, frame, destination, shuffled)
        }
    }
}

/// A SIMD operand of static type `ty` read from `register`: a vector, or an i32 read as the frame
/// holds it; any other scalar type is `None`, which leaves the row to its miss.
fn vector_operand<F: Frame<Concrete>>(
    stage: &mut Concrete,
    frame: &mut F,
    register: Register,
    ty: WasmType,
) -> Option<WasmValue> {
    match ty {
        WasmType::V128 => frame
            .vector(stage, register)
            .map(|bytes| WasmValue::V128(u128::from_le_bytes(bytes))),
        WasmType::I32 => frame
            .wasm_read(stage, register, WasmRep::Int32)
            .map(|word| WasmValue::I32(word as u32 as i32)),
        _ => None,
    }
}

/// Define `destination` with the vector `bits` in a fresh cell, answering its word.
fn store_vector<F: Frame<Concrete>>(
    stage: &mut Concrete,
    frame: &mut F,
    destination: Register,
    bits: u128,
) -> Step<u64, F::Control> {
    match frame.set_vector(stage, destination, bits.to_le_bytes()) {
        Some(word) => Step::Value(word),
        None => Step::Unresolved,
    }
}
