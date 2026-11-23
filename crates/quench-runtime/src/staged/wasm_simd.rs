//! Wasm SIMD steps as bytecode sees them: the vector opcodes' classes (`Simd`), each step's memory
//! and operand registers. The operator semantics are `wasm_simd_rules`, re-exported here for the
//! tiers' lowering.

#[allow(unused_imports)]
pub use super::wasm_simd_rules::{Lanes, Native, NativeOp, WasmSimd, simd, simd_scalar};

// The vector steps' classes and operands read bytecode and the Wasm kernels: runtime only.
#[cfg(not(stencil_build))]
use crate::bytecode::{Class, InstructionField, Op, WideInstruction};
#[cfg(not(stencil_build))]
use crate::wasm::memory::{MemoryLoad, MemoryStore};
#[cfg(not(stencil_build))]
use crate::wasm::simd::SimdKernel;

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
    /// The vector step `op` is, with `tag` its selector where the opcode takes one: a selector
    /// or access the tiers do not run is none.
    pub(crate) const fn of(op: Op, tag: u32) -> Option<Self> {
        match op.class() {
            Class::WasmSimd if SimdKernel::of(tag).is_some() => Some(Self::Operator { selector: tag }),
            Class::WasmSimdShuffle => Some(Self::Shuffle),
            Class::WasmMemoryAddress => Some(Self::Address),
            Class::WasmMemoryLoad => match MemoryLoad::from_tag(tag) {
                Some(MemoryLoad::V128Load) => Some(Self::Load),
                _ => None,
            },
            Class::WasmMemoryStore => match MemoryStore::from_tag(tag) {
                Some(MemoryStore::V128Store) => Some(Self::Store),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether the step reads a memory.
    pub(crate) const fn reads_memory(self) -> bool {
        matches!(self, Self::Address | Self::Load | Self::Store)
    }

    /// The register naming the memory the step reads, where it reads one.
    pub fn memory(self, instruction: WideInstruction) -> Option<u16> {
        match self {
            Self::Address => Some(instruction.c()),
            Self::Load | Self::Store => Some(instruction.b()),
            Self::Operator { .. } | Self::Shuffle => None,
        }
    }

    /// The register whose slot the plan records for the step: the address of an access, else
    /// the first operand.
    pub(crate) fn operand(self, instruction: WideInstruction) -> u16 {
        match self {
            Self::Load | Self::Store => instruction.c(),
            _ => instruction.b(),
        }
    }

    /// Whether `field` is the unused second operand of a one-operand operator, which lowering
    /// leaves naming register 0 and nothing reads.
    pub(crate) fn leaves_unread(self, field: InstructionField) -> bool {
        match self {
            Self::Operator { selector } => {
                field == InstructionField::C
                    && SimdKernel::of(selector).is_some_and(|(kernel, _)| kernel.right.is_none())
            }
            _ => false,
        }
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
use crate::staged::{Concrete, Constant, DecodeContext, Fields, Frame, Operation, Step};
use crate::staged::wasm_simd_rules::WasmSimd;
use crate::bytecode::{Op, Register};
use crate::wasm::simd::SimdKernel;
use crate::{WasmType, WasmValue};


/// A Wasm vector operation on the lane's registers: a SIMD operator on its operands, or a byte
/// shuffle of two vectors by a constant's lane indices.
#[derive(Clone, Copy)]
pub enum VectorOperation {
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
    pub fn decode_wasm_vector<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        VectorOperation::decode(instruction, context).map(Self::WasmVector)
    }
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
