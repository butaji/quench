//! Wasm SIMD steps as bytecode sees them: the vector opcodes' classes (`Simd`), each step's memory
//! and operand registers. The operator semantics are `wasm_simd_rules`, re-exported here for the
//! tiers' lowering.

#[allow(unused_imports)]
pub use super::wasm_simd_rules::{Lanes, Native, NativeOp, WasmSimd, simd, simd_scalar};

#[cfg(not(stencil_build))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Simd {
    /// `a = operator(b, c)`; the selector (the operator and its lane immediate) is `imm`.
    Operator { selector: u32 },
    /// `a = shuffle(b, c)` by the sixteen lane indices of the constant.
    Shuffle,
    /// `a = b + offset`, the effective address of a vector access: the 32-bit `b` plus the
    /// constant's static offset, as 64 bits; the memory is in `c`.
    Address,
    /// `a = v128.load` of the memory in `b` at the 64-bit address `c`.
    Load,
    /// `v128.store a` to the memory in `b` at the 64-bit address `c`.
    Store,
}
#[cfg(not(stencil_build))]
impl Simd {
    /// Whether the step reads a memory.
    pub const fn reads_memory(self) -> bool {
        matches!(self, Self::Address | Self::Load | Self::Store)
    }
}

// The interpreter's vector records as a staged operation (`Decoder::WasmVector`): runtime only.
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
pub use operation::{VectorOperation, step};
#[cfg(not(stencil_build))]
pub mod operation {
use crate::staged::register_frame::WasmRep;
use crate::staged::wasm_simd_concrete::{ConcreteSimd, execute};
use crate::staged::{Concrete, Constant, Frame, Step};
use crate::staged::wasm_simd_rules::WasmSimd;
use crate::bytecode::Register;
use crate::wasm::simd::SimdKernel;
use crate::{WasmType, WasmValue};


/// A Wasm vector operation on the lane's registers: a SIMD operator on its operands, or a byte
/// shuffle of two vectors by a constant's lane indices. Its decoders are `glue.rs`.
#[derive(Clone, Copy)]
pub enum VectorOperation {
    /// `destination = operator(left, right)`, the operator and its lane immediate `selector`
    /// (`WasmSimd`); `right` is read only where the operator takes two operands.
    Operator { destination: Register, left: Register, right: Register, selector: u32 },
    /// `destination = shuffle(left, right)` by the sixteen lane indices of `indices`
    /// (`WasmSimdShuffle`).
    Shuffle { destination: Register, left: Register, right: Register, indices: Constant },
}

/// Run `operation` on `frame`: its vector, or `Unresolved` where the frame leaves it. A scalar
/// result (an extract, a bitmask) and an operand of another representation are left to the
/// general step, which computes or raises them.
#[inline(always)]
pub fn step<F: Frame<Concrete>>(
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
            match execute(kernel, selector, left, right) {
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
}

// The decoders read bytecode and sit in `glue.rs`: the `Simd` step queries and the vector records.
#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
