//! Wasm SIMD as data: the vector opcodes, the operators a machine computes lane-wise in its own
//! vector registers (`NATIVE`, a row each), and the one decision between a native row and the
//! operator's kernel (`simd`). The interpreter's stage computes the rows lane by lane
//! (`staged::wasm_simd_concrete`); the Wasm tier emits Cranelift vector instructions.

use super::register_frame::WasmRep;
use super::wasm_simd_concrete::{ConcreteSimd, execute};
use super::{Concrete, Constant, DecodeContext, Fields, Frame, Operation, Step};
use crate::bytecode::{Class, InstructionField, Op, Register, WideInstruction};
use crate::wasm::V128_BYTES;
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::wasm::simd::{SimdKernel, SimdOperator};
use crate::{WasmType, WasmValue};

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

/// The native row of `operator`, read from `NATIVE` laid out by operator once, so the
/// interpreter's decision per execution is one index.
pub(crate) fn native(operator: SimdOperator) -> Option<Native> {
    static ROWS: std::sync::LazyLock<Vec<Option<Native>>> = std::sync::LazyLock::new(|| {
        let length = NATIVE.iter().map(|(operator, _)| *operator as usize + 1).max();
        let mut rows = vec![None; length.unwrap_or(0)];
        for (operator, native) in NATIVE {
            rows[operator as usize] = Some(native);
        }
        rows
    });
    ROWS.get(operator as usize).copied().flatten()
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
