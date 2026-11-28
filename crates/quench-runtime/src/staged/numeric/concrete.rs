//! The `NumericStage` of the concrete stage (`super::super::Concrete`): the words are the frame's own `u64` words and the
//! operations are the interpreter's, as the stencils run them. Split from `numeric.rs` for its size.

use super::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

#[inline(always)]
fn int32(word: u64) -> i32 { word as u32 as i32 }

/// The interpreter's wrapping int32 result, from the same rows as every arithmetic stage.
struct WrappingInt32Stage;

crate::projection!(WrappingInt32Stage => Concrete);

macro_rules! wrapping_int32_arithmetic {
    ($($operation:ident => $operator:ident, $method:ident, $instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        impl NumberArithmeticStage<i32> for WrappingInt32Stage {
            type Output = i32;
            $(
                #[inline(always)]
                fn $method(&mut self, left: i32, right: i32) -> Self::Output {
                    wrapping_int32_arithmetic!(@wrapped left, right, $symbol)
                }
            )+
        }
    };
    (@wrapped $left:ident, $right:ident, +) => { $left.wrapping_add($right) };
    (@wrapped $left:ident, $right:ident, -) => { $left.wrapping_sub($right) };
    (@wrapped $left:ident, $right:ident, *) => { $left.wrapping_mul($right) };
    (@wrapped $left:ident, $right:ident, /) => {
        { let _ = ($left, $right); unreachable!("division has a binary64 result") }
    };
}
crate::vm::operations::number_arithmetic_rows!(wrapping_int32_arithmetic);

/// The sign bit of a binary64 word.
const F64_SIGN_BIT: u64 = 1 << (u64::BITS - 1);

impl NumericStage for super::super::Concrete {
    #[inline(always)]
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: u64, right: u64) -> bool {
        match domain {
            Domain::Int32 => relation.apply(&mut PartialOrderStage, int32(left), int32(right)),
            Domain::Float64 => relation.apply(&mut PartialOrderStage,
                f64::from_bits(left), f64::from_bits(right)),
        }
    }
    #[inline(always)]
    fn float_arithmetic(&mut self, operation: NumberArithmetic, left: u64, right: u64) -> u64 {
        operation.apply(&mut NumberF64Stage,
            f64::from_bits(left), f64::from_bits(right)).to_bits()
    }
    #[inline(always)]
    fn float_negate(&mut self, value: u64) -> u64 {
        // The sign bit flipped. A stencil has no constant pool to hold a mask for a value that lives in a
        // vector register, so the mask is an opaque word it makes in a general register.
        #[cfg(stencil_build)]
        let mask = core::hint::black_box(F64_SIGN_BIT);
        #[cfg(not(stencil_build))]
        let mask = F64_SIGN_BIT;
        value ^ mask
    }
    #[inline(always)]
    fn float_eq(&mut self, left: u64, right: u64) -> bool {
        f64::from_bits(left) == f64::from_bits(right)
    }
    #[inline(always)]
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: u64, right: u64) -> (u64, bool) {
        let (left, right) = (int32(left), int32(right));
        let checked = operation.apply(&mut CheckedInt32Stage, left, right);
        let overflowed = checked.is_none();
        // A match rather than `unwrap_or_else`, whose closure instance stays out of line across crates.
        let result = match checked { Some(result) => result, None => operation.apply(&mut WrappingInt32Stage, left, right) };
        (u64::from(result as u32), overflowed)
    }
    #[inline(always)]
    fn float_remainder(&mut self, left: u64, right: u64) -> u64 {
        crate::vm::operations::remainder(f64::from_bits(left), f64::from_bits(right)).to_bits()
    }
    #[inline(always)]
    fn integral_remainder(&mut self, left: u64, right: u64) -> (u64, bool) {
        // A match rather than `map_or`, whose closure instance stays out of line across crates.
        match crate::vm::operations::integral_remainder(f64::from_bits(left), f64::from_bits(right)) {
            Some(remainder) => (remainder.to_bits(), false),
            None => (0, true),
        }
    }
    #[inline(always)]
    fn int_remainder(&mut self, left: u64, right: u64) -> u64 {
        // SAFETY: the trait's contract: every row guards the divisor nonzero before this step,
        // so no tier carries a divide-by-zero trap of its own.
        unsafe { core::hint::assert_unchecked(int32(right) != 0) };
        u64::from(int32(left).wrapping_rem(int32(right)) as u32)
    }
    #[inline(always)]
    fn float_unary(&mut self, operation: super::super::math::FloatUnary, value: u64) -> u64 {
        super::super::math::float_unary(operation, f64::from_bits(value)).to_bits()
    }
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn float_power(&mut self, base: u64, exponent: u64) -> u64 {
        super::super::power::power(f64::from_bits(base), f64::from_bits(exponent)).to_bits()
    }
    #[inline(always)]
    fn float_primitive(&mut self, operation: super::super::math::FloatUnary, value: u64) -> u64 {
        super::super::math::float_primitive(operation, f64::from_bits(value)).to_bits()
    }
    #[inline(always)]
    fn float_extreme(&mut self, maximum: bool, left: u64, right: u64) -> u64 {
        super::super::math::float_extreme(maximum, f64::from_bits(left), f64::from_bits(right)).to_bits()
    }
    #[inline(always)]
    fn int_multiply(&mut self, left: u64, right: u64) -> u64 {
        u64::from(super::super::math::int_multiply(int32(left), int32(right)) as u32)
    }
    #[inline(always)]
    fn int_leading_zeros(&mut self, value: u64) -> u64 {
        u64::from(super::super::math::leading_zeros(value as u32))
    }
    #[inline(always)]
    fn i32_constant(&mut self, value: i32) -> u64 { u64::from(value as u32) }
    /// The stencil build materializes the constant in an integer register (`words::register_bits`): a double
    /// LLVM folds into an instruction's memory operand would come from a constant pool, which a stencil may not use.
    #[inline(always)]
    fn f64_constant(&mut self, value: f64) -> u64 {
        #[cfg(stencil_build)]
        { crate::words::register_bits(value.to_bits()) }
        #[cfg(not(stencil_build))]
        { value.to_bits() }
    }
    #[inline(always)]
    fn f64_from_bits(&mut self, bits: u64) -> u64 { bits }
    #[inline(always)]
    fn narrow(&mut self, value: u64) -> u64 { u64::from(value as u32) }
    #[inline(always)]
    fn i32_to_f64(&mut self, value: u64) -> u64 { (i64::from(int32(value)) as f64).to_bits() }
    #[inline(always)]
    fn u32_to_f64(&mut self, value: u64) -> u64 { (i64::from(value as u32) as f64).to_bits() }
    #[inline(always)]
    fn to_int32(&mut self, number: u64, _miss: ()) -> Option<u64> {
        // The stencil's `units::to_int32`. The runtime's `units` module (not in quench-staged) re-exports
        // `value::number_to_i32` as `to_int32`, so the cargo flavor names that.
        #[cfg(stencil_build)]
        let int = crate::units::to_int32(f64::from_bits(number));
        #[cfg(not(stencil_build))]
        let int = crate::value::number_to_i32(f64::from_bits(number));
        Some(u64::from(int as u32))
    }
    #[inline(always)]
    fn int_bitwise(&mut self, operation: NumberBitwise, left: u64, right: u64) -> u64 {
        u64::from(int32_bitwise_bits(operation, int32(left), int32(right)) as u32)
    }
    #[inline(always)]
    fn i32_is_zero(&mut self, value: u64) -> bool { int32(value) == 0 }
    #[inline(always)]
    fn i32_is_negative(&mut self, value: u64) -> bool { int32(value) < 0 }
    #[inline(always)]
    fn both(&mut self, a: bool, b: bool) -> bool { a && b }
    #[inline(always)]
    fn either(&mut self, a: bool, b: bool) -> bool { a || b }
    #[inline(always)]
    fn cond_word(&mut self, cond: bool) -> u64 { u64::from(cond) }
    #[inline(always)]
    fn guard_false(&mut self, cond: bool, _miss: ()) -> Option<()> { (!cond).then_some(()) }
}
