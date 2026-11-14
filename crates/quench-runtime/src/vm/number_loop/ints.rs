//! The int32 nodes of a number loop's body: bitwise operators and ToInt32, whose values
//! stay raw int32s between operators, as C's `int` arithmetic does.

use super::nodes::{Ctx, Num};

/// `Int` as a trait object.
pub(super) trait IntNode {
    fn eval(&self, cx: &mut Ctx) -> Option<i32>;
}

impl<I: super::nodes::Int> IntNode for I {
    fn eval(&self, cx: &mut Ctx) -> Option<i32> {
        self.int(cx)
    }
}

use super::nodes::Int;

impl Int for Box<dyn IntNode> {
    #[inline(always)]
    fn int(&self, cx: &mut Ctx) -> Option<i32> {
        (**self).eval(cx)
    }
}

/// An int32 slot read as an int32.
pub(super) struct IntOf(pub(super) u16);
pub(super) struct IntConstant(pub(super) i32);
/// ToInt32 of a Number node.
pub(super) struct ToInt32<N>(pub(super) N);

impl Int for IntOf {
    #[inline(always)]
    fn int(&self, cx: &mut Ctx) -> Option<i32> {
        Some(cx.slot(self.0) as i32)
    }
}

impl Int for IntConstant {
    #[inline(always)]
    fn int(&self, _: &mut Ctx) -> Option<i32> {
        Some(self.0)
    }
}

impl<N: Num> Int for ToInt32<N> {
    #[inline(always)]
    fn int(&self, cx: &mut Ctx) -> Option<i32> {
        let number = self.0.num(cx)?;
        let truncated = number as i32;
        if f64::from(truncated) == number {
            return Some(truncated);
        }
        Some(crate::value::number_to_u32(number) as i32)
    }
}

/// The int32 bitwise operators a plan inlines, by code.
pub(super) const OR: u8 = 0;
pub(super) const AND: u8 = 1;
pub(super) const XOR: u8 = 2;
pub(super) const SHL: u8 = 3;
pub(super) const SAR: u8 = 4;
/// Shift counts are taken modulo the int32 width.
const SHIFT_MASK: i32 = i32::BITS as i32 - 1;

/// `dst = left OP right` on int32s, into an int32 slot.
pub(super) struct Bits<const OP: u8, L, R> {
    pub(super) dst: u16,
    pub(super) left: L,
    pub(super) right: R,
}

impl<const OP: u8, L: Int, R: Int> Int for Bits<OP, L, R> {
    #[inline(always)]
    fn int(&self, cx: &mut Ctx) -> Option<i32> {
        let left = self.left.int(cx)?;
        let right = self.right.int(cx)?;
        let value = match OP {
            OR => left | right,
            AND => left & right,
            XOR => left ^ right,
            SHL => left.wrapping_shl((right & SHIFT_MASK) as u32),
            _ => left.wrapping_shr((right & SHIFT_MASK) as u32),
        };
        cx.set(self.dst, value as i64 as u64);
        Some(value)
    }
}

/// An int32 node read as a Number.
pub(super) struct NumOf<I>(pub(super) I);

impl<I: Int> Num for NumOf<I> {
    #[inline(always)]
    fn num(&self, cx: &mut Ctx) -> Option<f64> {
        self.0.int(cx).map(f64::from)
    }
}

/// `dst = src` into an int32 slot.
pub(super) struct MoveInt<I> {
    pub(super) dst: u16,
    pub(super) src: I,
}

impl<I: Int> Int for MoveInt<I> {
    #[inline(always)]
    fn int(&self, cx: &mut Ctx) -> Option<i32> {
        let value = self.src.int(cx)?;
        cx.set(self.dst, value as i64 as u64);
        Some(value)
    }
}

/// `dst = src ± 1` of an int32 slot (`nodes::step_int`).
pub(super) struct IntStep<const DECREMENT: bool> {
    pub(super) dst: u16,
    pub(super) src: u16,
    pub(super) at: u32,
}

impl<const DECREMENT: bool> Int for IntStep<DECREMENT> {
    #[inline(always)]
    fn int(&self, cx: &mut Ctx) -> Option<i32> {
        super::nodes::step_int::<DECREMENT>(cx, self.dst, self.src, self.at)
    }
}
