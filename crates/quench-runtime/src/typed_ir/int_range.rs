/// Joins at one program point before a range that keeps growing widens (`IntRange::widen`).
pub const WIDENING_ROUNDS: u8 = 6;

/// The magnitude bits within which a signed 64-bit word holds an integer Number and the sum,
/// difference or product of two int32s without wrapping: the span of `IntRange::WORD`.
pub(crate) const WORD_BITS: u32 = i64::BITS - 2;

/// The significand bits of a binary64: the integers within `IntRange::EXACT` are the Numbers
/// that hold them exactly, so a 64-bit word of that span and the Number are the same value.
pub(crate) const EXACT_BITS: u32 = f64::MANTISSA_DIGITS;

/// A closed interval of integers a Number may hold, exact over `i64`: arithmetic on ranges is
/// mathematical, not wrapping, and a result `None` where it leaves even `i64`. Whether a range
/// fits int32 (`fits_int32`) is the caller's question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntRange {
    pub min: i64,
    pub max: i64,
}

impl IntRange {
    /// The span of int32: what an int32-represented value holds at widest, and where widening settles.
    pub const FULL: Self = Self { min: i32::MIN as i64, max: i32::MAX as i64 };

    pub(crate) const NON_NEGATIVE: Self = Self { min: 0, max: Self::FULL.max };

    /// The range of the one integer zero: a divisor or a sign test's operand that may be zero is `contains(ZERO)`.
    pub const ZERO: Self = Self { min: 0, max: 0 };

    /// The widest span integer arithmetic over int32s stays within without wrapping an i64,
    /// where widening settles a range that left int32: a result outside it has no range.
    pub const WORD: Self = Self { min: -(1 << WORD_BITS), max: 1 << WORD_BITS };

    /// The integers a Number holds exactly.
    pub const EXACT: Self = Self { min: -(1 << EXACT_BITS), max: 1 << EXACT_BITS };

    pub fn singleton(value: impl Into<i64>) -> Self {
        let value = value.into();
        Self { min: value, max: value }
    }

    pub fn packed() -> Self {
        let half = 1_i64 << (crate::value::NarrowLayout::INTEGER_BITS - 1);
        Self { min: -half, max: half - 1 }
    }

    pub fn bounds(min: i64, max: i64) -> Option<Self> { (min <= max).then_some(Self { min, max }) }

    pub fn fits_int32(&self) -> bool { Self::FULL.contains(*self) }

    /// The range of the binary64 Numbers the integers of `self` round to. Rounding to nearest
    /// is monotone, so the bounds' own are the range's: `self` within `EXACT`, wider beyond.
    pub fn rounded(self) -> Self {
        Self { min: self.min as f64 as i64, max: self.max as f64 as i64 }
    }

    pub fn join(self, other: Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    /// Open upward: the maximum to int32's, or from there to `WORD`'s.
    pub(crate) fn widen_up(self) -> Self {
        Self { max: if self.max <= Self::FULL.max { Self::FULL.max } else { Self::WORD.max }, ..self }
    }

    /// Open downward: the minimum to int32's, or from there to `WORD`'s.
    pub(crate) fn widen_down(self) -> Self {
        Self { min: if self.min >= Self::FULL.min { Self::FULL.min } else { Self::WORD.min }, ..self }
    }

    /// `self` joined with `next`, each bound `next` moved opened a step (int32's, then
    /// `WORD`'s), so a chain of widenings is finite.
    pub(crate) fn widen(self, next: Self) -> Self {
        let hull = self.join(next);
        let up = if hull.max > self.max { hull.widen_up() } else { hull };
        if hull.min < self.min { up.widen_down() } else { up }
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        Self::bounds(self.min.max(other.min), self.max.min(other.max))
    }

    pub fn contains(self, other: Self) -> bool { self.min <= other.min && other.max <= self.max }

    pub fn add(self, other: Self) -> Option<Self> {
        Some(Self { min: self.min.checked_add(other.min)?, max: self.max.checked_add(other.max)? })
    }

    pub fn subtract(self, other: Self) -> Option<Self> {
        Some(Self { min: self.min.checked_sub(other.max)?, max: self.max.checked_sub(other.min)? })
    }

    pub fn multiply(self, other: Self) -> Option<Self> {
        let corners = [(self.min, other.min), (self.min, other.max), (self.max, other.min), (self.max, other.max)];
        let [a, b, c, d] = corners.map(|(left, right)| left.checked_mul(right));
        let products = [a?, b?, c?, d?];
        Some(Self { min: *products.iter().min()?, max: *products.iter().max()? })
    }

    /// Sign-extended N-bit operands remain sign-extended after &, | or ^.
    /// Unlike independent known bits, this retains the correlation of high
    /// bits with the sign even when the input interval crosses zero.
    pub fn bitwise_hull(self, other: Self) -> Self {
        let inputs = self.join(other);
        let magnitude = [inputs.min, inputs.max].map(|value| value.max(!value) as u64);
        let width = u64::BITS - (magnitude[0] | magnitude[1]).leading_zeros() + 1;
        let half = 1_i64 << (width - 1);
        Self { min: -half, max: half - 1 }
    }

    pub fn shift_right(self, count: Self) -> Option<Self> {
        if count.min != count.max { return None; }
        let shift = (count.min as u32) & crate::value::WORD32_SHIFT_MASK;
        Some(Self { min: self.min >> shift, max: self.max >> shift })
    }

    /// The range of `self >>> count` where `count` is one known shift of nonzero amount. A
    /// shift of 1 or more leaves [0, 2^31); a non-negative operand shifts its bounds. Any other
    /// operand is reinterpreted as a uint32 first, so only the upper bound of the shifted uint32
    /// is known. A zero shift keeps the uint32, which is not an int32: `None`.
    pub fn shift_right_unsigned(self, count: Self) -> Option<Self> {
        if count.min != count.max { return None; }
        let shift = (count.min as u32) & crate::value::WORD32_SHIFT_MASK;
        if shift == 0 { return None; }
        if self.min >= 0 { return Some(Self { min: self.min >> shift, max: self.max >> shift }); }
        Some(Self { min: 0, max: i64::from(u32::MAX >> shift) })
    }
}

/// The transfer rules of integer ranges per operator, pure over `IntRange` (the stencil build includes this file, so
/// it names nothing outside the lattice). The solve's operator dispatch (`staged::typing`) picks one.
impl IntRange {
    /// The integer a Number operation over integer operands defines where a word holds it: the binary64 Number of the
    /// result (`rounded`), within `WORD`. Beyond `WORD` the result has no integer range.
    pub fn as_number(self) -> Option<Self> {
        let number = self.rounded();
        Self::WORD.contains(number).then_some(number)
    }

    /// Whether a product of integers in `self` and `other` may be a negative zero, which no integer type holds: a zero
    /// product of an operand that may be zero, negative where the other may be.
    pub fn product_may_be_negative_zero(self, other: Self) -> bool {
        let zero = Self::singleton(0);
        (self.contains(zero) && other.min < 0) || (other.contains(zero) && self.min < 0)
    }

    /// The int32 a bitwise operator reads an operand as: an operand outside int32 is any int32, since ToInt32 of any
    /// Number lies in it.
    pub fn int32_operand(self) -> Self {
        if self.fits_int32() { self } else { Self::FULL }
    }

    /// `self & other`: a non-negative operand clears the bits the other has above its own maximum, so the result lies
    /// in `0..=` the least such maximum (`i & 255` lies in `0..=255`). Two negative operands give the hull.
    pub fn and(self, other: Self) -> Self {
        let ceiling = [self, other].iter().filter(|range| range.min >= 0).map(|range| range.max).min();
        match ceiling {
            Some(max) => Self { min: 0, max },
            None => self.bitwise_hull(other),
        }
    }

    /// `self | other`: an operand that is zero leaves the other as it is; otherwise the bitwise hull.
    pub fn or(self, other: Self) -> Self {
        let zero = Self::singleton(0);
        if other == zero { self } else if self == zero { other } else { self.bitwise_hull(other) }
    }

    /// `self % divisor` in JavaScript, whose result takes the dividend's sign. A dividend of no negative value and a
    /// divisor that is no zero give an integer in `0..=` the lesser of the dividend's maximum and the divisor's
    /// magnitude less one. `None` otherwise: a zero divisor's NaN, a negative dividend's possible `-0`.
    pub fn remainder(self, divisor: Self) -> Option<Self> {
        if self.min < 0 || divisor.contains(Self::singleton(0)) { return None; }
        let magnitude = divisor.min.unsigned_abs().max(divisor.max.unsigned_abs());
        let ceiling = (magnitude - 1).min(self.max.unsigned_abs());
        Self::bounds(0, ceiling as i64)
    }

    /// The ranges of `self` and `other` on an edge where `self <= other` (`self < other` when `strict`): each keeps the
    /// bounds the other allows. `None` where no pair of values reaches the edge.
    pub fn ordered_edge(self, other: Self, strict: bool) -> Option<[Self; 2]> {
        let strict = i64::from(strict);
        Some([Self::bounds(self.min, self.max.min(other.max - strict))?,
            Self::bounds(other.min.max(self.min + strict), other.max)?])
    }

    /// `self` on an edge where it differs from `other`: an interval drops a singleton `other` only at its end, so
    /// `self` = [v, max] with v = `other` is [v + 1, max]. `None` where no value is left.
    pub fn without(self, other: Self) -> Option<Self> {
        if other.min != other.max { return Some(self); }
        let value = other.min;
        if self.min == value && self.max == value { None }
        else if self.min == value { Self::bounds(value + 1, self.max) }
        else if self.max == value { Self::bounds(self.min, value - 1) }
        else { Some(self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_width_encloses_bitwise_results_across_zero_and_word_boundaries() {
        for a in -4..=4 { for b in a..=4 { for c in -4..=4 { for d in c..=4 {
            let bound = IntRange { min: a, max: b }.bitwise_hull(IntRange { min: c, max: d });
            for x in a..=b { for y in c..=d {
                for result in [x & y, x | y, x ^ y] { assert!(bound.contains(IntRange::singleton(result))); }
            }}
        }}}}
        for width in 1..=i32::BITS {
            let half = 1_i64 << (width - 1);
            let range = IntRange::bounds(-half, half - 1).unwrap();
            assert_eq!(range.bitwise_hull(range), range);
            for x in [range.min, -1, 0, range.max] { for y in [range.min, -1, 0, range.max] {
                for result in [x & y, x | y, x ^ y] { assert!(range.contains(IntRange::singleton(result))); }
            }}
        }
    }

    #[test]
    fn arithmetic_shift_uses_word32_counts_and_closes_packed_bitwise_updates() {
        for range in [IntRange::FULL, IntRange::packed(), IntRange { min: -1, max: 1 }] {
            for count in [-33, -32, -1, 0, 1, 31, 32, 33, 64] {
                let bound = range.shift_right(IntRange::singleton(count)).unwrap();
                for value in [range.min, range.max, -1, 0, 1] {
                    if range.contains(IntRange::singleton(value)) {
                        assert!(bound.contains(IntRange::singleton((value as i32).wrapping_shr(count as u32))));
                    }
                }
            }
        }
        assert_eq!(IntRange::FULL.shift_right(IntRange { min: 0, max: 1 }), None);
        let packed = IntRange::packed();
        let shifted = packed.shift_right(IntRange::singleton(1)).unwrap();
        for mask in [shifted.min, -1, 0, 1, shifted.max] {
            assert!(packed.contains(shifted.bitwise_hull(IntRange::singleton(mask))));
        }
    }

    #[test]
    fn mathematical_bounds_reject_overflow_and_close_positive_packed_decrement() {
        let packed = IntRange::packed();
        let positive = packed.intersect(IntRange { min: 1, max: IntRange::FULL.max }).unwrap();
        assert!(packed.contains(positive.subtract(IntRange::singleton(1)).unwrap()));
        assert!(!packed.contains(packed.subtract(IntRange::singleton(1)).unwrap()));
        let beyond = |range: Option<IntRange>| !range.unwrap().fits_int32();
        assert!(beyond(IntRange::FULL.add(IntRange::singleton(1))));
        assert!(beyond(IntRange::FULL.subtract(IntRange::singleton(1))));
        assert_eq!(IntRange::singleton(i32::MIN).subtract(IntRange::singleton(i32::MIN)), Some(IntRange::singleton(0)));
        assert!(beyond(IntRange::FULL.multiply(IntRange::singleton(2))));
        assert!(beyond(IntRange::singleton(i32::MIN).multiply(IntRange::singleton(-1))));
        assert_eq!(IntRange::singleton(i64::MAX).add(IntRange::singleton(1)), None);
        assert_eq!(IntRange::singleton(i64::MAX).multiply(IntRange::singleton(2)), None);
        assert_eq!(IntRange { min: -3, max: 2 }.multiply(IntRange { min: -4, max: 5 }), Some(IntRange { min: -15, max: 12 }));
        for a in -8..=8 { for b in a..=8 { for c in -8..=8 { for d in c..=8 {
            let left = IntRange { min: a, max: b };
            let right = IntRange { min: c, max: d };
            for x in a..=b { for y in c..=d {
                assert!(left.multiply(right).unwrap().contains(IntRange::singleton(x * y)));
                assert!(left.add(right).unwrap().contains(IntRange::singleton(x + y)));
                assert!(left.subtract(right).unwrap().contains(IntRange::singleton(x - y)));
            }}
        }}}}
    }

    #[test]
    fn widening_contains_the_join_and_settles_at_int32_in_finitely_many_steps() {
        let start = IntRange::singleton(0);
        let mut range = start;
        for step in 1..=i64::from(WIDENING_ROUNDS) * 2 {
            let next = range.widen(IntRange { min: 0, max: step });
            assert!(next.contains(range.join(IntRange { min: 0, max: step })));
            range = next;
        }
        assert_eq!(range, IntRange::NON_NEGATIVE);
        assert_eq!(range.widen(IntRange { min: -1, max: 0 }), IntRange::FULL);
        assert_eq!(IntRange::FULL.widen(start), IntRange::FULL);
    }

    #[test]
    fn an_integer_word_and_its_number_agree_within_the_exact_span_and_rounding_is_monotone() {
        assert_eq!(IntRange::EXACT.rounded(), IntRange::EXACT);
        assert!(IntRange::EXACT.contains(IntRange::singleton(crate::bytecode::MAX_ARRAY_LENGTH as i64)));
        assert!(IntRange::WORD.contains(IntRange::WORD.rounded()));
        let beyond = IntRange { min: -(1 << 60) - 1, max: (1 << 60) + 1 };
        assert!(beyond.rounded().contains(IntRange::singleton((1_i64 << 59) + 3)));
        for value in [beyond.min, -(1 << 57) - 5, 1 << 53 | 1, (1 << 55) + 3, beyond.max] {
            let number = value as f64 as i64;
            assert!(IntRange { min: value.min(number), max: value.max(number) }.rounded().contains(IntRange::singleton(number)));
        }
        let odd = IntRange::singleton((1_i64 << 53) + 1);
        assert_eq!(odd.rounded(), IntRange::singleton(1_i64 << 53), "ties round to even");
    }

    #[test]
    fn a_range_that_leaves_int32_widens_to_the_word_span_and_no_further() {
        let past = IntRange { min: 0, max: i64::from(i32::MAX) + 1 };
        assert_eq!(IntRange::FULL.widen(past), IntRange { min: IntRange::FULL.min, max: IntRange::WORD.max });
        let beyond = IntRange { min: IntRange::FULL.min - 1, max: 0 };
        assert_eq!(IntRange::FULL.widen(beyond), IntRange { min: IntRange::WORD.min, max: IntRange::FULL.max });
        let word = IntRange { min: IntRange::WORD.min, max: IntRange::WORD.max };
        assert_eq!(word.widen(IntRange::singleton(0)), word);
    }

    #[test]
    fn zero_fill_shift_by_a_known_count_encloses_every_uint32_outcome() {
        let ranges = [IntRange { min: -3, max: 2 }, IntRange { min: 0, max: 9 }, IntRange { min: -40, max: -1 }];
        for left in ranges {
            for shift in 1..32_i64 {
                let range = left.shift_right_unsigned(IntRange::singleton(shift)).expect("nonzero shift");
                assert!(range.fits_int32());
                for x in left.min.max(i32::MIN.into())..=left.max.min(i32::MAX.into()) {
                    let result = (x as i32 as u32 >> shift) as i64;
                    assert!(range.contains(IntRange::singleton(result)), "{x} >>> {shift}");
                }
            }
        }
        assert!(IntRange::FULL.shift_right_unsigned(IntRange::singleton(0)).is_none());
        assert!(IntRange::FULL.shift_right_unsigned(IntRange { min: 1, max: 2 }).is_none());
    }
}
