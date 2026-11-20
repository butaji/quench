//! The `NumericStage` trait: the word-level primitives a numeric operation's staged step runs over, which each stage
//! (the concrete stencil stage, the u-jit emitter) implements. Split from `numeric.rs` for its size.

use super::*;

pub(crate) trait NumericStage: Stage {
    /// `relation` of two words in `domain`.
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: Self::Word, right: Self::Word) -> Self::Cond;
    /// `operation` of two binary64 words.
    fn float_arithmetic(&mut self, operation: NumberArithmetic,
        left: Self::Word, right: Self::Word) -> Self::Word;
    /// The binary64 negation of `value`: `-0` stays negative and NaN stays NaN.
    fn float_negate(&mut self, value: Self::Word) -> Self::Word;
    /// IEEE equality of two binary64 words: `-0` equals `+0`, and NaN equals nothing.
    fn float_eq(&mut self, left: Self::Word, right: Self::Word) -> Self::Cond;
    /// `operation` of two int32 words: the wrapped result and whether it overflowed.
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: Self::Word, right: Self::Word) -> (Self::Word, Self::Cond);
    /// The binary64 remainder of two binary64 words, keeping the dividend's sign.
    fn float_remainder(&mut self, left: Self::Word, right: Self::Word) -> Self::Word;
    /// The remainder of two binary64 words by the exact integer remainder where both are integral and below 2^53 in magnitude,
    /// a nonzero divisor and no negative zero: its result, and whether it misses (the operands are none of those, and only the
    /// interpreter's float remainder answers them). A stage whose words cannot hold a call answers the miss.
    fn integral_remainder(&mut self, left: Self::Word, right: Self::Word) -> (Self::Word, Self::Cond);
    /// The truncated remainder of two int32 words, `right` nonzero: the dividend's sign, and
    /// `i32::MIN % -1` is zero rather than a trap.
    fn int_remainder(&mut self, left: Self::Word, right: Self::Word) -> Self::Word;
    /// `operation` of a binary64 word (`staged::math::float_unary`).
    fn float_unary(&mut self, operation: super::super::math::FloatUnary, value: Self::Word) -> Self::Word;
    /// `float_unary` of a machine primitive (`Floor`, `Ceil`, `Trunc`, `Abs`, `Sqrt`, `Fround`): what the
    /// composite bodies (`math::round`, `math::sign`) build on, so the interpreter's instantiation of
    /// them calls no composite again and inlines whole (a stencil holds no call).
    #[inline(always)]
    fn float_primitive(&mut self, operation: super::super::math::FloatUnary, value: Self::Word) -> Self::Word {
        self.float_unary(operation, value)
    }
    /// `Math.max` (`maximum`) or `Math.min` of two binary64 words (`staged::math::float_extreme`).
    fn float_extreme(&mut self, maximum: bool, left: Self::Word, right: Self::Word) -> Self::Word;
    /// The wrapping product of two int32 words (`Math.imul`; `staged::math::int_multiply`).
    fn int_multiply(&mut self, left: Self::Word, right: Self::Word) -> Self::Word;
    /// The leading zero bits of the unsigned low 32 bits of an int32 word (`Math.clz32`; `staged::math::leading_zeros`).
    fn int_leading_zeros(&mut self, value: Self::Word) -> Self::Word;
    /// The int32 word holding `value`.
    fn i32_constant(&mut self, value: i32) -> Self::Word;
    /// The binary64 word holding `value`.
    fn f64_constant(&mut self, value: f64) -> Self::Word;
    /// The binary64 word whose IEEE bits are the 64-bit word `bits` (a double's value word).
    fn f64_from_bits(&mut self, bits: Self::Word) -> Self::Word;
    /// The int32 word holding the low 32 bits of the 64-bit word `value`.
    fn narrow(&mut self, value: Self::Word) -> Self::Word;
    fn i32_to_f64(&mut self, value: Self::Word) -> Self::Word;
    /// The binary64 of the unsigned low 32 bits of `value`.
    fn u32_to_f64(&mut self, value: Self::Word) -> Self::Word;
    /// ToInt32 of a binary64 word, as an int32 word; a specializer whose native range is narrower
    /// than ToInt32's leaves at `miss` for what it does not convert.
    fn to_int32(&mut self, number: Self::Word, miss: Self::Miss) -> Option<Self::Word>;
    /// The int32 word `operation` makes of two int32 words (a shift takes its count modulo 32).
    fn int_bitwise(&mut self, operation: NumberBitwise, left: Self::Word, right: Self::Word) -> Self::Word;
    fn i32_is_zero(&mut self, value: Self::Word) -> Self::Cond;
    fn i32_is_negative(&mut self, value: Self::Word) -> Self::Cond;
    fn both(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn either(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn cond_word(&mut self, cond: Self::Cond) -> Self::Word;
    /// Continue only where `cond` is false.
    fn guard_false(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
}
