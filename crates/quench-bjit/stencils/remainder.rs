//! The in-place cases of the remainder row (`numeric::remainder`): `%` of two int32s, and of two integral Numbers below 2^53,
//! computed without the staged step. A case it does not take answers so, and the staged step (or the leave) decides.

use crate::places::{DEST, LEFT, Places, RIGHT};
use crate::staged::{Concrete, Frame, NumericFrame};
use crate::typed_ir::Representation;
use crate::words::truncate64;
use crate::bytecode::Operand;

/// `%` of two int32 operands in place: the int32 remainder, unless the divisor is zero, the quotient overflows, or the
/// remainder is the zero of a negative dividend (-0, which the staged step makes). Whether it wrote the result.
#[inline(always)]
pub(super) fn remainder_ints<const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>) -> bool {
    let mut stage = Concrete::from_hole(0);
    let (Some(left), Some(right)) = (frame.int_operand(&mut stage, Operand::register(LEFT), ()), frame.int_operand(&mut stage, Operand::register(RIGHT), ())) else {
        return false;
    };
    let (a, b) = (left as u32 as i32, right as u32 as i32);
    if b == 0 || (a == i32::MIN && b == -1) || (a < 0 && a % b == 0) {
        return false;
    }
    frame.write(&mut stage, DEST, u64::from((a % b) as u32), Representation::Int32);
    true
}

/// `%` of two integral Numbers in place, where both are exact in an i64 below 2^53: the exact integer remainder, which is the
/// f64 `fmod` of such values (a call a stencil cannot hold), with the dividend's sign and no negative zero. Whether it wrote
/// the Number result; any other operands leave to the staged step.
#[inline(always)]
pub(super) fn remainder_integral<const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>) -> bool {
    let mut stage = Concrete::from_hole(0);
    let (Some(left), Some(right)) = (frame.number_operand(&mut stage, Operand::register(LEFT), ()), frame.number_operand(&mut stage, Operand::register(RIGHT), ())) else {
        return false;
    };
    let (x, y) = (f64::from_bits(left), f64::from_bits(right));
    let (a, b) = (truncate64(x), truncate64(y));
    if !(exact_integer(a, x) && exact_integer(b, y)) || b == 0 || (a % b == 0 && x.is_sign_negative()) {
        return false;
    }
    frame.write(&mut stage, DEST, ((a % b) as f64).to_bits(), Representation::Float64);
    true
}

/// Whether `value`, the truncation of `number` (`i64::MIN` where out of range), is `number` exactly and below 2^53 in magnitude.
#[inline(always)]
fn exact_integer(value: i64, number: f64) -> bool {
    value != i64::MIN && value as f64 == number && value.unsigned_abs() < 1 << 53
}
