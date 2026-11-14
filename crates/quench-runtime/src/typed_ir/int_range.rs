/// Joins at one program point before a range that keeps growing widens (`IntRange::widen`).
pub(crate) const WIDENING_ROUNDS: u8 = 6;

/// A closed interval of integers a Number may hold, exact over `i64`: arithmetic on ranges is
/// mathematical, not wrapping, and a result `None` where it leaves even `i64`. Whether a range
/// fits int32 (`fits_int32`) is the caller's question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntRange {
    pub(crate) min: i64,
    pub(crate) max: i64,
}

impl IntRange {
    /// The span of int32: what an int32-represented value holds at widest, and where widening settles.
    pub(crate) const FULL: Self = Self { min: i32::MIN as i64, max: i32::MAX as i64 };

    pub(crate) const NON_NEGATIVE: Self = Self { min: 0, max: Self::FULL.max };

    /// What an int32 observed to be `value` gives a later entry to guard once: its sign.
    pub(crate) const fn entering(value: i32) -> Self { if value >= 0 { Self::NON_NEGATIVE } else { Self::FULL } }

    pub(crate) fn singleton(value: impl Into<i64>) -> Self {
        let value = value.into();
        Self { min: value, max: value }
    }

    pub(crate) fn packed() -> Self {
        let half = 1_i64 << (crate::value::NarrowLayout::INTEGER_BITS - 1);
        Self { min: -half, max: half - 1 }
    }

    pub(crate) fn bounds(min: i64, max: i64) -> Option<Self> { (min <= max).then_some(Self { min, max }) }

    pub(crate) fn fits_int32(&self) -> bool { Self::FULL.contains(*self) }

    pub(crate) fn join(self, other: Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    /// Open upward: the maximum up to int32's.
    pub(crate) fn widen_up(self) -> Self { Self { max: self.max.max(Self::FULL.max), ..self } }

    /// Open downward: the minimum down to int32's.
    pub(crate) fn widen_down(self) -> Self { Self { min: self.min.min(Self::FULL.min), ..self } }

    /// `self` joined with `next`, each bound `next` moved opened to int32's, so a chain of
    /// widenings is finite.
    pub(crate) fn widen(self, next: Self) -> Self {
        let hull = self.join(next);
        let up = if hull.max > self.max { hull.widen_up() } else { hull };
        if hull.min < self.min { up.widen_down() } else { up }
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        Self::bounds(self.min.max(other.min), self.max.min(other.max))
    }

    pub(crate) fn contains(self, other: Self) -> bool { self.min <= other.min && other.max <= self.max }

    pub(crate) fn add(self, other: Self) -> Option<Self> {
        Some(Self { min: self.min.checked_add(other.min)?, max: self.max.checked_add(other.max)? })
    }

    pub(crate) fn subtract(self, other: Self) -> Option<Self> {
        Some(Self { min: self.min.checked_sub(other.max)?, max: self.max.checked_sub(other.min)? })
    }

    pub(crate) fn multiply(self, other: Self) -> Option<Self> {
        let corners = [(self.min, other.min), (self.min, other.max), (self.max, other.min), (self.max, other.max)];
        let [a, b, c, d] = corners.map(|(left, right)| left.checked_mul(right));
        let products = [a?, b?, c?, d?];
        Some(Self { min: *products.iter().min()?, max: *products.iter().max()? })
    }

    /// Sign-extended N-bit operands remain sign-extended after &, | or ^.
    /// Unlike independent known bits, this retains the correlation of high
    /// bits with the sign even when the input interval crosses zero.
    pub(crate) fn bitwise_hull(self, other: Self) -> Self {
        let inputs = self.join(other);
        let magnitude = [inputs.min, inputs.max].map(|value| value.max(!value) as u64);
        let width = u64::BITS - (magnitude[0] | magnitude[1]).leading_zeros() + 1;
        let half = 1_i64 << (width - 1);
        Self { min: -half, max: half - 1 }
    }

    pub(crate) fn shift_right(self, count: Self) -> Option<Self> {
        if count.min != count.max { return None; }
        let shift = (count.min as u32) & crate::value::WORD32_SHIFT_MASK;
        Some(Self { min: self.min >> shift, max: self.max >> shift })
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
}
