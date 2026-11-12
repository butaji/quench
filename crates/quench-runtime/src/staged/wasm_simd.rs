//! Wasm SIMD as data: the vector opcodes, the operators a machine computes lane-wise in its own
//! vector registers (`NATIVE`, a row each), and the one decision between a native row and the
//! operator's kernel (`simd`). The interpreter's stage computes the rows lane by lane
//! (`staged::wasm_simd_concrete`); the Wasm tier emits Cranelift vector instructions.

use crate::bytecode::{Class, InstructionField, Op, WideInstruction};
use crate::wasm::V128_BYTES;
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::wasm::simd::{SimdKernel, SimdOperator};

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
    pub(crate) fn of(op: Op, tag: u32) -> Option<Self> {
        match op.class() {
            Class::WasmSimd => SimdKernel::of(tag).map(|_| Self::Operator { selector: tag }),
            Class::WasmSimdShuffle => Some(Self::Shuffle),
            Class::WasmMemoryAddress => Some(Self::Address),
            Class::WasmMemoryLoad => {
                (MemoryLoad::from_tag(tag) == Some(MemoryLoad::V128Load)).then_some(Self::Load)
            }
            Class::WasmMemoryStore => {
                (MemoryStore::from_tag(tag) == Some(MemoryStore::V128Store)).then_some(Self::Store)
            }
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

/// How the lanes of a vector are cut for a lane-wise operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Lanes {
    /// One lane of the whole vector: the bitwise operators.
    Bits,
    I8x16,
    I16x8,
    I32x4,
    I64x2,
}

impl Lanes {
    pub(crate) const fn width(self) -> u32 {
        match self {
            Self::Bits => u128::BITS,
            Self::I8x16 => u8::BITS,
            Self::I16x8 => u16::BITS,
            Self::I32x4 => u32::BITS,
            Self::I64x2 => u64::BITS,
        }
    }
}

/// A lane-wise operation a machine computes in its vector registers. A shift's count is a
/// scalar, reduced modulo the lane width as Wasm does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NativeOp {
    Not,
    And,
    Or,
    Xor,
    AndNot,
    Add,
    Subtract,
    ShiftLeft,
    ShiftRightUnsigned,
    ShiftRightSigned,
    /// The u64 products of the low two u32 lanes of each operand.
    ExtendMultiplyLowUnsigned,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Native {
    pub(crate) op: NativeOp,
    pub(crate) lanes: Lanes,
}

macro_rules! native_rows {
    ($($operator:ident => $op:ident, $lanes:ident;)+) => {
        /// The operators a machine computes natively, each exactly its kernel
        /// (`staged::wasm_simd_tests` checks every row against `SimdKernel`).
        pub(crate) const NATIVE: [(SimdOperator, Native); [$(SimdOperator::$operator),+].len()] = [$((
            SimdOperator::$operator,
            Native { op: NativeOp::$op, lanes: Lanes::$lanes },
        )),+];
    };
}

native_rows! {
    V128Not => Not, Bits;
    V128And => And, Bits;
    V128Or => Or, Bits;
    V128Xor => Xor, Bits;
    V128AndNot => AndNot, Bits;
    I8x16Add => Add, I8x16;
    I8x16Sub => Subtract, I8x16;
    I16x8Add => Add, I16x8;
    I16x8Sub => Subtract, I16x8;
    I16x8Shl => ShiftLeft, I16x8;
    I16x8ShrU => ShiftRightUnsigned, I16x8;
    I16x8ShrS => ShiftRightSigned, I16x8;
    I32x4Add => Add, I32x4;
    I32x4Sub => Subtract, I32x4;
    I32x4Shl => ShiftLeft, I32x4;
    I32x4ShrU => ShiftRightUnsigned, I32x4;
    I32x4ShrS => ShiftRightSigned, I32x4;
    I64x2Add => Add, I64x2;
    I64x2Sub => Subtract, I64x2;
    I64x2Shl => ShiftLeft, I64x2;
    I64x2ShrU => ShiftRightUnsigned, I64x2;
    I64x2ShrS => ShiftRightSigned, I64x2;
    I64x2ExtMulLowI32x4U => ExtendMultiplyLowUnsigned, I32x4;
}

/// The native row of `operator`.
pub(crate) fn native(operator: SimdOperator) -> Option<Native> {
    NATIVE
        .iter()
        .find(|(candidate, _)| *candidate == operator)
        .map(|(_, native)| *native)
}

/// The vector registers of a machine. A value is a 128-bit vector or a scalar in its low bits,
/// the representation of `SimdKernel::apply_bits`.
pub(crate) trait WasmSimd {
    type Value: Copy;
    /// `native` on `left` and, where it takes one, `right`: the shift count, or a vector.
    fn vector_native(
        &mut self,
        native: Native,
        left: Self::Value,
        right: Option<Self::Value>,
    ) -> Self::Value;
    /// The operator `selector` names, by its kernel.
    fn vector_kernel(
        &mut self,
        selector: u32,
        left: Self::Value,
        right: Option<Self::Value>,
    ) -> Self::Value;
    /// The byte shuffle of `left` and `right` by `lanes`.
    fn vector_shuffle(
        &mut self,
        lanes: [u8; V128_BYTES],
        left: Self::Value,
        right: Self::Value,
    ) -> Self::Value;
}

/// The operator `selector` names on its operands: the machine's own instruction where it has
/// one with exactly the operator's semantics, else the kernel. The one place that is decided.
#[inline(always)]
pub(crate) fn simd<S: WasmSimd>(
    stage: &mut S,
    selector: u32,
    left: S::Value,
    right: Option<S::Value>,
) -> S::Value {
    let (operator, _) = SimdOperator::from_selector(selector).expect("an admitted selector");
    match native(operator) {
        Some(native) => stage.vector_native(native, left, right),
        None => stage.vector_kernel(selector, left, right),
    }
}
