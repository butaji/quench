//! The Wasm float rows that need a float constant, as stencils with no constant pool: the
//! float-to-integer conversions (trapping and saturating), the unsigned 64-bit integer-to-float
//! conversions, and copysign. LLVM loads every float bound or mask those rows use from a
//! RIP-relative constant-pool entry, which a row cannot relocate (`wasm_codes.rs`). Here each
//! float constant is an integer immediate turned into a float (`float_of_bits`), so LLVM emits a
//! `movabs` and a `movq`.
//!
//! `convert_row` runs the staged rule, `staged::wasm_float::convert`, which holds the trap order
//! (the NaN trap, then the overflow trap) once. It runs over `ConstantFree`: `ConcreteWasm` for
//! every primitive, except the two that touch a float constant (`truncation_overflows`, and
//! `convert` for the float-to-integer and unsigned 64-bit conversions), which compute the same
//! values here. So the semantics stay the staged ones; only the constants change.
//!
//! `copysign_row` has no staged rule to run: `staged::wasm_concrete` computes CopySign with
//! `f64::copysign`, which LLVM lowers with a sign-mask constant. It is written here as the same
//! bit operation (`(A & !SIGN) | (B & SIGN)`, the `float_width!` arm), with integer masks.
//!
//! A row continues (`Out::Next`) or leaves through exit 0 with the trap's status in the first
//! pinned register, as `wasm_rows.rs` rows do.

use crate::staged::machine::{Machine, MachineStage};
use crate::staged::wasm_concrete::{ConcreteWasm, delegated};
use crate::staged::wasm_float::{Float, Truncation, convert, float_binary};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::F64BinaryOperator as FloatBinary;
use crate::wasm::WasmTrap;
use crate::words::{HOLE_A, HOLE_B, Out, S, slot};

/// The `CopySign` tag of `F64BinaryOperator` (`FLOAT_BINARY_NEEDS_CONSTANTS`'s only entry).
pub(crate) const COPYSIGN_TAG: u32 = 6;
const _: () = assert!(matches!(FloatBinary::from_tag(COPYSIGN_TAG), Some(FloatBinary::CopySign)));

const SIGN32: u64 = 0x8000_0000;
const SIGN64: u64 = 1 << 63;

/// `2^exponent` as f64 bits: the biased exponent in its field, no mantissa.
const fn pow2_bits(exponent: u32) -> u64 {
    (1023 + exponent as u64) << 52
}

/// The f64 with IEEE bits `bits`. The black box keeps LLVM from folding the bits back into a
/// float constant, so the value is an integer immediate in a register (`movabs`, `movq`).
#[inline(always)]
fn float_of_bits(bits: u64) -> f64 {
    f64::from_bits(core::hint::black_box(bits))
}

/// `truncation_bounds` (`staged::wasm_concrete`) from integer bits: `lower <= value < limit`.
#[inline(always)]
fn bounds(bits: u32, signed: bool) -> (f64, f64) {
    if signed {
        let limit = pow2_bits(bits - 1);
        (float_of_bits(SIGN64 | limit), float_of_bits(limit))
    } else {
        (float_of_bits(0), float_of_bits(pow2_bits(bits)))
    }
}

/// A float operand widened to f64, exactly (`float_value` in `staged::wasm_concrete`).
#[inline(always)]
fn widen(float: Float, word: u64) -> f64 {
    match float {
        Float::F32 => f64::from(f32::from_bits(word as u32)),
        Float::F64 => f64::from_bits(word),
    }
}

/// `saturate` (`staged::wasm_concrete`) as the word a conversion leaves: a NaN is 0, the value
/// truncates toward zero and clamps to the `bits`-wide integer, and a 32-bit result is its low
/// word, zero-extended. The trapping truncations call it too, once their range check has passed,
/// where it is the same truncation.
#[inline(always)]
fn saturated(value: f64, bits: u32, signed: bool) -> u64 {
    if value.is_nan() {
        return 0;
    }
    let (lower, limit) = bounds(bits, signed);
    let (min, max, mask) = match (bits, signed) {
        (32, true) => (SIGN32, 0x7fff_ffff, 0xffff_ffff),
        (32, false) => (0, 0xffff_ffff, 0xffff_ffff),
        (_, true) => (SIGN64, i64::MAX as u64, u64::MAX),
        (_, false) => (0, u64::MAX, u64::MAX),
    };
    if value <= lower {
        return min;
    }
    if value >= limit {
        return max;
    }
    let integer = if !signed && bits == 64 && value >= float_of_bits(pow2_bits(63)) {
        let above = value - float_of_bits(pow2_bits(63));
        // SAFETY: `above` is in [0, 2^63), and its truncation fits an i64.
        (unsafe { above.to_int_unchecked::<i64>() } as u64) | SIGN64
    } else {
        // SAFETY: the value is in (lower, limit), so its truncation fits an i64.
        unsafe { value.to_int_unchecked::<i64>() as u64 }
    };
    integer & mask
}

/// `value as f64` for an unsigned 64-bit `value`, correctly rounded, without the expansion LLVM
/// uses for an unsigned conversion, which loads a constant. A value under 2^63 converts as
/// signed. A larger one halves with its low bit kept as a sticky bit, converts as signed, and
/// adds to itself: the same rounding, exact.
#[inline(always)]
fn unsigned_to_f64(value: u64) -> f64 {
    if (value as i64) >= 0 {
        value as i64 as f64
    } else {
        let half = core::hint::black_box((value >> 1) | (value & 1)) as i64 as f64;
        half + half
    }
}

/// `value as f32` for an unsigned 64-bit `value`: `unsigned_to_f64`'s halving, at f32. The
/// sticky bit keeps the single rounding correct for the 24-bit result.
#[inline(always)]
fn unsigned_to_f32(value: u64) -> f32 {
    if (value as i64) >= 0 {
        value as i64 as f32
    } else {
        let half = core::hint::black_box((value >> 1) | (value & 1)) as i64 as f32;
        half + half
    }
}

/// The stage of a constant-free row: `ConcreteWasm`, with the primitives that compare or convert
/// against a float constant computing from integer bits instead.
pub(crate) struct ConstantFree;

impl Machine for ConstantFree {
    type Word = u64;
}

impl MachineStage for ConstantFree {
    type Exit = WasmTrap;

    delegated! {
        constant(width: crate::staged::wasm::Width, value: i64) -> u64;
        operate(operator: crate::wasm::integer::I64BinaryOperator, width: crate::staged::wasm::Width, left: u64, right: u64) -> u64;
        operate_unary(operator: crate::wasm::integer::I64UnaryOperator, width: crate::staged::wasm::Width, value: u64) -> u64;
        operate_float_unary(operator: crate::wasm::float::F64UnaryOperator, float: Float, value: u64) -> u64;
        float_unordered(float: Float, left: u64, right: u64) -> u64;
        float_constant(float: Float, bits: u64) -> u64;
        select(flag: u64, taken: u64, other: u64) -> u64;
        trap_when(flag: u64, trap: WasmTrap) -> Result<(), WasmTrap>;
    }

    /// `ConcreteWasm::operate_float`, with CopySign as the sign-bit operation: `f64::copysign`
    /// lowers to a sign-mask constant from the constant pool.
    #[inline(always)]
    fn operate_float(&mut self, operator: FloatBinary, float: Float, left: u64, right: u64) -> u64 {
        match operator {
            FloatBinary::CopySign => copysign_bits(float, left, right),
            _ => ConcreteWasm.operate_float(operator, float, left, right),
        }
    }

    /// `ConcreteWasm::truncation_overflows`, with the bounds from bits. `trunc` is `roundsd` on the
    /// stencil targets (x86-64-v2 and up), so it loads no constant.
    #[inline(always)]
    fn truncation_overflows(&mut self, truncation: Truncation, value: u64) -> u64 {
        let truncated = core::f64::math::trunc(widen(truncation.float, value));
        let (lower, limit) = bounds(truncation.width.bits(), truncation.signed);
        u64::from(truncated < lower || truncated >= limit)
    }

    /// `ConcreteWasm::convert`: the float-to-integer conversions through `saturated`, the unsigned
    /// 64-bit conversions through the halving forms, the rest as `ConcreteWasm`.
    #[inline(always)]
    fn convert(&mut self, operator: ScalarConversionOperator, value: u64) -> u64 {
        if let Some((truncation, _)) = operator.float_to_integer() {
            return saturated(widen(truncation.float, value), truncation.width.bits(), truncation.signed);
        }
        match operator {
            ScalarConversionOperator::ConvertI64UnsignedF32 => u64::from(unsigned_to_f32(value).to_bits()),
            ScalarConversionOperator::ConvertI64UnsignedF64 => unsigned_to_f64(value).to_bits(),
            _ => ConcreteWasm.convert(operator, value),
        }
    }
}

const fn scalar_conversion(tag: u32) -> ScalarConversionOperator {
    match ScalarConversionOperator::from_tag(tag) {
        Some(operator) => operator,
        None => ScalarConversionOperator::WrapI64,
    }
}

/// Leaves with `trap`'s status (as `wasm_rows.rs`'s `trap`).
#[inline(always)]
fn leave(mut s: S, trap: WasmTrap) -> Out {
    s.p[0] = match trap.compiled_status() {
        Some(status) => status as usize,
        None => 0,
    };
    Out::Exit(s)
}

/// `D = op A` for the conversion tag `OP` (`ScalarConversionOperator`), any tag: the staged
/// `staged::wasm_float::convert` over `ConstantFree`, so the NaN trap comes before the overflow
/// trap and both leave with `WasmTrap`'s compiled status, as the interpreter's rule does.
#[inline(always)]
pub(crate) unsafe fn convert_row<const OP: u32>(s: S) -> Out {
    let operator = const { scalar_conversion(OP) };
    unsafe {
        let value = *(slot::<HOLE_A>(&s) as *const u64);
        match convert(&mut ConstantFree, operator, value) {
            Ok(word) => {
                *(slot::<0>(&s) as *mut u64) = word;
                Out::Next(s)
            }
            Err(error) => leave(s, error),
        }
    }
}

/// CopySign at `float`: A's bits below the sign, B's sign bit. An f32 word is zero-extended, as
/// the interpreter's is. `ConstantFree::operate_float` computes it.
#[inline(always)]
fn copysign_bits(float: Float, left: u64, right: u64) -> u64 {
    match float {
        Float::F32 => u64::from(((left as u32) & !(SIGN32 as u32)) | ((right as u32) & (SIGN32 as u32))),
        Float::F64 => (left & !SIGN64) | (right & SIGN64),
    }
}

/// `D = copysign(A, B)` at width `W` (`W32` is f32): the staged `float_binary` rule over
/// `ConstantFree`, which returns CopySign's word unchanged (no NaN rule).
#[inline(always)]
pub(crate) unsafe fn copysign_row<const W: u8>(s: S) -> Out {
    unsafe {
        let (left, right) = (*(slot::<HOLE_A>(&s) as *const u64), *(slot::<HOLE_B>(&s) as *const u64));
        let word = float_binary(&mut ConstantFree, FloatBinary::CopySign, super::float_of(W), left, right);
        *(slot::<0>(&s) as *mut u64) = word;
    }
    Out::Next(s)
}
