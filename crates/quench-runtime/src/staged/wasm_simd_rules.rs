//! Wasm SIMD semantics written once, for both tiers and without bytecode: the operator table's
//! kernels (`SimdKernel`: operand and result types, lane count, the bits body), the selector
//! (operator and lane immediate) decode, the native rows a machine computes in its own vector
//! registers (`NATIVE`), the lane-wise bits body of those rows (`native_bits`), and the one decision
//! between a native row and the kernel (`simd`). The interpreter's stage (`wasm_simd_concrete`) and
//! the b-jit's stencil stage (`stencils/wasm_stage.rs`) implement `WasmSimd`; the Wasm tier emits
//! Cranelift vector instructions. The bytecode-facing `Simd` steps are `wasm_simd`.

use super::wasm_simd_lanes::{SimdOperator, lane, replace};
use crate::WasmType;
#[cfg(not(stencil_build))]
use crate::WasmValue;
use crate::wasm::V128_BYTES;

pub const SELECTOR_LANE_BITS: u32 = u8::BITS;
const SELECTOR_LANE_MASK: u32 = u8::MAX as u32;

/// The selector of the operator with tag `tag` and lane immediate `lane`: the tag above the lane bits.
#[inline(always)]
pub const fn selector_of(tag: u32, lane: u8) -> u32 {
    (tag << SELECTOR_LANE_BITS) | lane as u32
}

impl SimdOperator {
    pub(crate) fn from_selector(selector: u32) -> Option<(Self, u8)> {
        let (_, lane) = SimdKernel::of(selector)?;
        Some((Self::ALL[(selector >> SELECTOR_LANE_BITS) as usize], lane))
    }

    pub(crate) fn selector(self, lane: u8) -> u32 {
        selector_of(self as u32, lane)
    }

    pub(crate) fn right_type(self) -> Option<WasmType> {
        Self::KERNELS[self as usize].right
    }

}

/// An operator as an execution needs it (`SimdKernel::of`): its operand and result types, its lane
/// count, and the operator whose bits body computes it.
pub struct SimdKernel {
    pub(crate) left: WasmType,
    pub(crate) right: Option<WasmType>,
    /// How many lanes a lane immediate may name: one for an operator without one.
    lanes: u32,
    pub(crate) result: WasmType,
    operator: SimdOperator,
}

impl SimdKernel {
    /// The kernel and lane immediate a selector names: none for an unknown operator or a
    /// lane past its count.
    #[inline(always)]
    pub(crate) const fn of(selector: u32) -> Option<(&'static Self, u8)> {
        let index = (selector >> SELECTOR_LANE_BITS) as usize;
        if index >= SimdOperator::KERNELS.len() {
            return None;
        }
        let kernel = &SimdOperator::KERNELS[index];
        let lane = (selector & SELECTOR_LANE_MASK) as u8;
        if (lane as u32) < kernel.lanes { Some((kernel, lane)) } else { None }
    }

    /// The type of the operator's result, from its table entry.
    pub(crate) fn result_type(&self, _lane: u8) -> WasmType {
        self.result
    }

    /// The operator applied to operands given as bits, its result as bits: a vector whole, a
    /// scalar in the low bits (`ScalarBits`), as compiled code holds them.
    #[inline(always)]
    pub(crate) fn apply_bits(&self, left: u128, right: u128, lane: u8) -> u128 {
        self.operator.bits(left, right, lane)
    }
}

/// The lanes a lane immediate may name: the width's count when the operator takes one (`indexed`),
/// one otherwise.
const fn lane_count(indexed: &[&str], width: u32) -> u32 {
    match indexed {
        [_] => 128 / width,
        _ => 1,
    }
}

/// The kernel table, one entry per operator in tag order, generated from the operator list.
macro_rules! simd_kernels {
    ($($name:ident $([$lane:ident])? : $width:expr, $left:ident, $right:expr, $result:ident, $family:ident => |$a:ident, $b:ident, $l:ident| $body:expr;)*) => {
        impl SimdOperator {
            pub(crate) const KERNELS: &'static [SimdKernel] = &[$(SimdKernel {
                left: WasmType::$left,
                right: $right,
                lanes: lane_count(&[$(stringify!($lane))?], $width),
                result: WasmType::$result,
                operator: SimdOperator::$name,
            },)*];
        }
    };
}
crate::staged::wasm_simd_lanes::simd_operator_list!(simd_kernels);


/// How the lanes of a vector are cut for a lane-wise operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lanes {
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
pub enum NativeOp {
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
pub struct Native {
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

/// The native row of each operator, by tag, built at compile time from `NATIVE` (no lazy cache, so the
/// stencil build compiles it too).
const NATIVE_BY_OPERATOR: [Option<Native>; SimdOperator::ALL.len()] = native_rows_by_operator();

const fn native_rows_by_operator() -> [Option<Native>; SimdOperator::ALL.len()] {
    let mut rows = [None; SimdOperator::ALL.len()];
    let mut index = 0;
    while index < NATIVE.len() {
        rows[NATIVE[index].0 as usize] = Some(NATIVE[index].1);
        index += 1;
    }
    rows
}

/// The native row of `operator`, or none where its kernel runs.
#[inline(always)]
pub fn native(operator: SimdOperator) -> Option<Native> {
    NATIVE_BY_OPERATOR[operator as usize]
}

/// The vector registers of a machine. A value is a 128-bit vector or a scalar in its low bits,
/// the representation of `SimdKernel::apply_bits`; a scalar result is the `Scalar` word.
pub trait WasmSimd {
    type Value: Copy;
    /// The word of an operator whose result is a scalar (i32, i64, f32 or f64).
    type Scalar: Copy;
    /// The vector whose bits are `bits`.
    fn constant_vector(&mut self, bits: u128) -> Self::Value;
    /// The scalar result of the operator `selector` names on its operands, by its kernel: no
    /// native row yields a scalar.
    fn scalar_kernel(&mut self, selector: u32, left: Self::Value, right: Option<Self::Value>) -> Self::Scalar;
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
pub fn simd<S: WasmSimd>(
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

/// The scalar result of the operator `selector` names (an i32 extract, bitmask or any/all test),
/// its word of the stage's `Scalar` type: the kernel, the one body of such an operator.
#[inline(always)]
pub fn simd_scalar<S: WasmSimd>(
    stage: &mut S,
    selector: u32,
    left: S::Value,
    right: Option<S::Value>,
) -> S::Scalar {
    stage.scalar_kernel(selector, left, right)
}

/// The v128 constant `bits`, the stage's vector.
#[inline(always)]
pub fn vector_constant<S: WasmSimd>(stage: &mut S, bits: u128) -> S::Value {
    stage.constant_vector(bits)
}

/// A vector moved from one bank to another: the value itself, so the move is the copy of the
/// stage's word and nothing else.
#[inline(always)]
pub fn vector_move<S: WasmSimd>(_: &mut S, source: S::Value) -> S::Value {
    source
}

/// `operation` on each lane of `bits`, cut at `width`.
fn each_lane(width: u32, bits: u128, operation: impl Fn(u8, u64) -> u64) -> u128 {
    (0..u128::BITS / width).fold(0, |result, index| {
        let index = index as u8;
        replace(
            result,
            width,
            index,
            operation(index, lane(bits, width, index)),
        )
    })
}

/// `operation` on the lane `value` of the signed lane type of `width`, as its sign-extended
/// 64-bit integer.
fn signed(width: u32, value: u64, operation: impl Fn(i64) -> i64) -> u64 {
    let unused = u64::BITS - width;
    operation(((value << unused) as i64) >> unused) as u64
}

/// A lane-wise `native` on its lanes; its shift count is the low bits of `right`.
fn lane_wise(native: Native, left: u128, right: u128) -> u128 {
    let width = native.lanes.width();
    let count = (right as u32) % width;
    let other = |index: u8| lane(right, width, index);
    each_lane(width, left, |index, value| match native.op {
        NativeOp::Add => value.wrapping_add(other(index)),
        NativeOp::Subtract => value.wrapping_sub(other(index)),
        NativeOp::ShiftLeft => value << count,
        NativeOp::ShiftRightUnsigned => value >> count,
        NativeOp::ShiftRightSigned => signed(width, value, |extended| extended >> count),
        _ => unreachable!("a whole-vector or widening row"),
    })
}

/// The bits of the native row `native` on `left` and `right` (a shift count, or a vector; zero
/// where it takes none): the whole-vector operators, the widening multiply, or a lane-wise row.
pub fn native_bits(native: Native, left: u128, right: u128) -> u128 {
    match (native.op, native.lanes) {
        (NativeOp::Not, _) => !left,
        (NativeOp::And, _) => left & right,
        (NativeOp::Or, _) => left | right,
        (NativeOp::Xor, _) => left ^ right,
        (NativeOp::AndNot, _) => left & !right,
        (NativeOp::ExtendMultiplyLowUnsigned, Lanes::I32x4) => {
            let (narrow, wide) = (native.lanes.width(), u64::BITS);
            (0..u128::BITS / wide).fold(0, |result, index| {
                let index = index as u8;
                let product = lane(left, narrow, index) * lane(right, narrow, index);
                replace(result, wide, index, product)
            })
        }
        _ => lane_wise(native, left, right),
    }
}

/// The bits of operator `selector` on `left` and `right` by its kernel, the scalar in the low bits
/// where its result is one.
#[inline(always)]
pub fn kernel_bits(selector: u32, left: u128, right: Option<u128>) -> u128 {
    // Validation admitted the selector, so its operator names a table entry and its lane is in
    // range; a lane immediate that reaches here as a row hole is taken as given, so no check
    // (and no panic path) is left in a stencil.
    let kernel = &SimdOperator::KERNELS[(selector >> SELECTOR_LANE_BITS) as usize];
    let lane = (selector & SELECTOR_LANE_MASK) as u8;
    debug_assert!(SimdKernel::of(selector).is_some(), "an admitted selector");
    kernel.apply_bits(left, right.unwrap_or(0), lane)
}

/// The value of type `ty` that `bits` hold: a vector whole, a scalar in the low bits.
#[inline(always)]
#[cfg(not(stencil_build))]
pub fn value_of(ty: WasmType, bits: u128) -> WasmValue {
    match ty {
        WasmType::V128 => WasmValue::V128(bits),
        ty => WasmValue::from_bits(ty, bits as u64),
    }
}

/// The bits of `value`: a vector whole, a scalar in the low bits (the inverse of `value_of`).
#[inline(always)]
#[cfg(not(stencil_build))]
pub fn bits_of(value: WasmValue) -> u128 {
    match value {
        WasmValue::V128(bits) => bits,
        WasmValue::I32(value) => u128::from(value as u32),
        WasmValue::F32(bits) => u128::from(bits),
        WasmValue::I64(value) => u128::from(value as u64),
        WasmValue::F64(bits) => u128::from(bits),
        _ => unreachable!("a scalar or vector value"),
    }
}

/// The operand of static type `ty` as the SIMD rules take it, from the representation the caller
/// holds: a v128 from its `bytes` (little-endian), any other type from its `decoded` scalar.
#[inline(always)]
#[cfg(not(stencil_build))]
pub fn simd_operand(
    ty: WasmType,
    bytes: impl FnOnce() -> Option<[u8; V128_BYTES]>,
    decoded: impl FnOnce() -> Option<WasmValue>,
) -> Option<WasmValue> {
    match ty {
        WasmType::V128 => bytes().map(|bytes| WasmValue::V128(u128::from_le_bytes(bytes))),
        _ => decoded(),
    }
}

/// The byte shuffle of `left` and `right` (the operands' bytes, `left` first) by `lanes`.
#[inline(always)]
pub fn shuffle_bits(left: u128, right: u128, lanes: [u8; V128_BYTES]) -> u128 {
    super::wasm_simd_lanes::shuffle(left, right, u128::from_le_bytes(lanes))
}
