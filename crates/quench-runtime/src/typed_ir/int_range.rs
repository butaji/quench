/// A closed interval of semantic signed-int32 values, not wrapping arithmetic.
/// Unknown/overflowing operations are represented by the caller's `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntRange {
    pub(crate) min: i32,
    pub(crate) max: i32,
}

impl IntRange {
    pub(crate) const FULL: Self = Self { min: i32::MIN, max: i32::MAX };

    pub(crate) const fn singleton(value: i32) -> Self { Self { min: value, max: value } }

    pub(crate) fn packed() -> Self {
        let half = 1_i32 << (crate::value::NarrowLayout::INTEGER_BITS - 1);
        Self { min: -half, max: half - 1 }
    }

    pub(crate) fn bounds(min: i64, max: i64) -> Option<Self> {
        (min <= max).then_some(Self { min: i32::try_from(min).ok()?, max: i32::try_from(max).ok()? })
    }

    pub(crate) fn join(self, other: Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        Self::bounds(i64::from(self.min.max(other.min)), i64::from(self.max.min(other.max)))
    }

    pub(crate) fn contains(self, other: Self) -> bool { self.min <= other.min && other.max <= self.max }

    pub(crate) fn add(self, other: Self) -> Option<Self> {
        Self::bounds(i64::from(self.min) + i64::from(other.min), i64::from(self.max) + i64::from(other.max))
    }

    pub(crate) fn subtract(self, other: Self) -> Option<Self> {
        Self::bounds(i64::from(self.min) - i64::from(other.max), i64::from(self.max) - i64::from(other.min))
    }

    /// Sign-extended N-bit operands remain sign-extended after &, | or ^.
    /// Unlike independent known bits, this retains the correlation of high
    /// bits with the sign even when the input interval crosses zero.
    pub(crate) fn bitwise_hull(self, other: Self) -> Self {
        let inputs = self.join(other);
        let magnitude = [inputs.min, inputs.max].map(|value| if value < 0 { !value as u32 } else { value as u32 });
        let width = u32::BITS - (magnitude[0] | magnitude[1]).leading_zeros() + 1;
        let half = 1_i64 << (width - 1);
        Self { min: (-half) as i32, max: (half - 1) as i32 }
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
                        assert!(bound.contains(IntRange::singleton(value.wrapping_shr(count as u32))));
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
        let positive = packed.intersect(IntRange { min: 1, max: i32::MAX }).unwrap();
        assert!(packed.contains(positive.subtract(IntRange::singleton(1)).unwrap()));
        assert!(!packed.contains(packed.subtract(IntRange::singleton(1)).unwrap()));
        assert_eq!(IntRange::FULL.add(IntRange::singleton(1)), None);
        assert_eq!(IntRange::FULL.subtract(IntRange::singleton(1)), None);
        assert_eq!(IntRange::singleton(i32::MIN).subtract(IntRange::singleton(i32::MIN)), Some(IntRange::singleton(0)));
        for a in -8..=8 { for b in a..=8 { for c in -8..=8 { for d in c..=8 {
            let left = IntRange { min: a, max: b };
            let right = IntRange { min: c, max: d };
            for x in a..=b { for y in c..=d {
                assert!(left.add(right).unwrap().contains(IntRange::singleton(x + y)));
                assert!(left.subtract(right).unwrap().contains(IntRange::singleton(x - y)));
            }}
        }}}}
    }
}
