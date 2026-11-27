#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Class, InstructionField, Op, WideInstruction};
use crate::staged::{DecodeContext, Fields, Operation};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::wasm::simd::SimdKernel;

/// The vector step `op` is, with `tag` its selector where the opcode takes one: a selector or
/// access the tiers do not run is none. A const fn, so a free fn: an inherent impl of `Simd`
/// cannot live outside quench-staged.
pub const fn simd_of(op: Op, tag: u32) -> Option<Simd> {
    match op.class() {
        Class::WasmSimd if SimdKernel::of(tag).is_some() => Some(Simd::Operator { selector: tag }),
        Class::WasmSimdShuffle => Some(Simd::Shuffle),
        Class::WasmMemoryAddress => Some(Simd::Address),
        Class::WasmMemoryLoad => match MemoryLoad::from_tag(tag) {
            Some(MemoryLoad::V128Load) => Some(Simd::Load),
            _ => None,
        },
        Class::WasmMemoryStore => match MemoryStore::from_tag(tag) {
            Some(MemoryStore::V128Store) => Some(Simd::Store),
            _ => None,
        },
        _ => None,
    }
}

/// The bytecode queries of a vector step: the memory it names, its operand, and the unused operand
/// of a one-operand operator.
pub trait SimdWasmSimdExt: Sized {
    /// The register naming the memory the step reads, where it reads one.
    fn memory(self, instruction: WideInstruction) -> Option<u16>;
    /// The register whose slot the plan records for the step: the address of an access, else
    /// the first operand.
    fn operand(self, instruction: WideInstruction) -> u16;
    /// Whether `field` is the unused second operand of a one-operand operator, which lowering
    /// leaves naming register 0 and nothing reads.
    fn leaves_unread(self, field: InstructionField) -> bool;
}

impl SimdWasmSimdExt for Simd {
    fn memory(self, instruction: WideInstruction) -> Option<u16> {
        match self {
            Self::Address => Some(instruction.c()),
            Self::Load | Self::Store => Some(instruction.b()),
            Self::Operator { .. } | Self::Shuffle => None,
        }
    }

    fn operand(self, instruction: WideInstruction) -> u16 {
        match self {
            Self::Load | Self::Store => instruction.c(),
            _ => instruction.b(),
        }
    }

    fn leaves_unread(self, field: InstructionField) -> bool {
        match self {
            Self::Operator { selector } => {
                field == InstructionField::C
                    && SimdKernel::of(selector).is_some_and(|(kernel, _)| kernel.right.is_none())
            }
            _ => false,
        }
    }
}

/// The operation `instruction` denotes (`Decoder::WasmVector`): an operator's selector is its
/// immediate, a shuffle's indices its constant (read through `context`, or the lane's own).
pub trait VectorOperationWasmSimdExt: Sized {
    fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self>;
}

impl VectorOperationWasmSimdExt for VectorOperation {
    fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
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

/// The `Operation` constructor for a vector record (`Decoder::WasmVector`): runtime only.
pub trait OperationWasmSimdExt: Sized {
    /// The Wasm vector operation `instruction` denotes (`Decoder::WasmVector`).
    fn decode_wasm_vector<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self>;
}

impl<W, P> OperationWasmSimdExt for Operation<W, P> {
    fn decode_wasm_vector<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        VectorOperation::decode(instruction, context).map(Self::WasmVector)
    }
}
