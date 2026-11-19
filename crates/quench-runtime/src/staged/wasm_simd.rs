//! Wasm SIMD steps as bytecode sees them: the vector opcodes' classes (`Simd`), each step's memory
//! and operand registers. The operator semantics are `wasm_simd_rules`, re-exported here for the
//! tiers' lowering.

use crate::bytecode::{Class, InstructionField, Op, WideInstruction};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::wasm::simd::SimdKernel;
pub(crate) use super::wasm_simd_rules::{Lanes, Native, NativeOp, WasmSimd, simd, simd_scalar};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Simd {
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
    pub(crate) fn memory(self, instruction: WideInstruction) -> Option<u16> {
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

