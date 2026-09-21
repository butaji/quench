use std::fmt;

const TAG_MASK: u64 = 0xffff_0000_0000_0000;
const PAYLOAD_MASK: u64 = 0x0000_ffff_ffff_ffff;
const TAG_UNDEFINED: u64 = 0x7ff9_0000_0000_0000;
const TAG_NULL: u64 = 0x7ffa_0000_0000_0000;
const TAG_BOOL: u64 = 0x7ffb_0000_0000_0000;
const TAG_HEAP: u64 = 0x7ffc_0000_0000_0000;
const TAG_INT: u64 = 0x7ffd_0000_0000_0000;
const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Value(u64);

impl Value {
    pub const UNDEFINED: Self = Self(TAG_UNDEFINED);
    pub const NULL: Self = Self(TAG_NULL);
    pub const FALSE: Self = Self(TAG_BOOL);
    pub const TRUE: Self = Self(TAG_BOOL | 1);

    pub fn number(value: f64) -> Self {
        let integer = value as i32;
        if integer as f64 == value && !(value == 0.0 && value.is_sign_negative()) {
            return Self::integer(integer);
        }
        let bits = value.to_bits();
        Self(if value.is_nan() { CANONICAL_NAN } else { bits })
    }

    #[inline(always)]
    pub(crate) fn integer(value: i32) -> Self {
        Self(TAG_INT | u64::from(value as u32))
    }

    #[inline(always)]
    pub(crate) fn as_int(self) -> Option<i32> {
        ((self.0 & TAG_MASK) == TAG_INT).then_some(self.0 as u32 as i32)
    }

    #[inline(always)]
    pub(crate) fn int_pair(left: Self, right: Self) -> Option<(i32, i32)> {
        let tags = (left.0 ^ TAG_INT) | (right.0 ^ TAG_INT);
        (tags & TAG_MASK == 0).then_some((left.0 as u32 as i32, right.0 as u32 as i32))
    }

    pub(crate) fn heap(index: u32) -> Self {
        Self(TAG_HEAP | u64::from(index))
    }

    pub fn as_number(self) -> Option<f64> {
        if let Some(value) = self.as_int() {
            Some(value as f64)
        } else {
            (!self.is_tagged()).then(|| f64::from_bits(self.0))
        }
    }

    pub fn as_bool(self) -> Option<bool> {
        ((self.0 & TAG_MASK) == TAG_BOOL).then_some(self.0 & 1 != 0)
    }

    #[inline(always)]
    pub(crate) fn heap_index(self) -> Option<u32> {
        ((self.0 & TAG_MASK) == TAG_HEAP).then_some((self.0 & PAYLOAD_MASK) as u32)
    }

    pub fn is_undefined(self) -> bool {
        self.0 == TAG_UNDEFINED
    }

    pub fn is_null(self) -> bool {
        self.0 == TAG_NULL
    }

    pub fn is_heap(self) -> bool {
        (self.0 & TAG_MASK) == TAG_HEAP
    }

    fn is_tagged(self) -> bool {
        matches!(
            self.0 & TAG_MASK,
            TAG_UNDEFINED | TAG_NULL | TAG_BOOL | TAG_HEAP | TAG_INT
        )
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn profile_kind(self) -> usize {
        if self.is_undefined() {
            0
        } else if self.is_null() {
            1
        } else if self.as_bool().is_some() {
            2
        } else if self.as_int().is_some() {
            3
        } else if self.as_number().is_some() {
            4
        } else {
            5
        }
    }
}

#[inline]
pub(crate) fn number_to_u32(value: f64) -> u32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(4_294_967_296.0) as u32
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(number) = self.as_number() {
            write!(f, "{number}")
        } else if let Some(boolean) = self.as_bool() {
            write!(f, "{boolean}")
        } else if self.is_null() {
            f.write_str("null")
        } else if self.is_undefined() {
            f.write_str("undefined")
        } else {
            write!(f, "heap({})", self.heap_index().unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn values_round_trip() {
        for value in [0.0, -0.0, 1.5, f64::INFINITY, f64::NAN] {
            let round = Value::number(value).as_number().unwrap();
            assert!(round == value || round.is_nan() && value.is_nan());
        }
        assert_eq!(Value::TRUE.as_bool(), Some(true));
        assert_eq!(Value::heap(42).heap_index(), Some(42));
        assert_eq!(
            Value::int_pair(Value::integer(-2), Value::integer(3)),
            Some((-2, 3))
        );
        assert_eq!(Value::int_pair(Value::integer(1), Value::TRUE), None);
    }
}
