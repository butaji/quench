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
}

#[cfg(test)]
mod tests {
    use super::*;

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
