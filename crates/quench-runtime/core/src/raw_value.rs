use std::ptr::NonNull;

pub const VALUE_BYTES: usize = std::mem::size_of::<u64>();
pub const TAG_MASK: u64 = 0xffff_0000_0000_0000;
pub const PAYLOAD_MASK: u64 = 0x0000_ffff_ffff_ffff;
pub const CANONICAL_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;
pub const FIRST_TAG: u64 = 0xfff9_0000_0000_0000;
pub const UNDEFINED_TAG: u64 = FIRST_TAG;
pub const NULL_TAG: u64 = 0xfffa_0000_0000_0000;
pub const BOOL_TAG: u64 = 0xfffb_0000_0000_0000;
pub const STRING_TAG: u64 = 0xfffc_0000_0000_0000;
pub const OBJECT_TAG: u64 = 0xfffd_0000_0000_0000;
pub const FUNCTION_TAG: u64 = 0xfffe_0000_0000_0000;
pub const REGEXP_TAG: u64 = 0xffff_0000_0000_0000;
pub const FALSE_PAYLOAD: u64 = 0;
pub const TRUE_PAYLOAD: u64 = 1;
pub const HOLE_PAYLOAD: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct RawValue(u64);

impl RawValue {
    pub const UNDEFINED: Self = Self(UNDEFINED_TAG);
    pub const NULL: Self = Self(NULL_TAG);
    pub const FALSE: Self = Self(BOOL_TAG | FALSE_PAYLOAD);
    pub const TRUE: Self = Self(BOOL_TAG | TRUE_PAYLOAD);
    pub const HOLE: Self = Self(UNDEFINED_TAG | HOLE_PAYLOAD);

    pub fn number(value: f64) -> Self {
        if value.is_nan() {
            Self(CANONICAL_NAN_BITS)
        } else {
            Self(value.to_bits())
        }
    }

    pub const fn boolean(value: bool) -> Self {
        if value { Self::TRUE } else { Self::FALSE }
    }

    pub fn tagged_pointer<T>(tag: u64, pointer: NonNull<T>) -> Option<Self> {
        debug_assert!((STRING_TAG..=REGEXP_TAG).contains(&tag));
        let address = pointer.as_ptr() as usize as u64;
        (address & !PAYLOAD_MASK == 0).then_some(Self(tag | address))
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn is_number(self) -> bool {
        self.0 & TAG_MASK < FIRST_TAG
    }

    pub const fn is_heap(self) -> bool {
        self.tag() >= STRING_TAG
    }

    pub const fn tag(self) -> u64 {
        self.0 & TAG_MASK
    }

    pub const fn payload(self) -> u64 {
        self.0 & PAYLOAD_MASK
    }

    pub fn as_number(self) -> Option<f64> {
        self.is_number().then(|| f64::from_bits(self.0))
    }

    pub fn as_heap<T>(self) -> Option<NonNull<T>> {
        if !self.is_heap() {
            return None;
        }
        NonNull::new(self.payload() as usize as *mut T)
    }
}

const _: [(); VALUE_BYTES] = [(); std::mem::size_of::<RawValue>()];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_value_is_one_machine_word() {
        assert_eq!(std::mem::size_of::<RawValue>(), VALUE_BYTES);
    }

    #[test]
    fn numbers_round_trip_and_nan_is_canonical() {
        for value in [0.0, -0.0, 1.5, f64::INFINITY, f64::NEG_INFINITY] {
            let raw = RawValue::number(value);
            assert!(raw.is_number());
            assert_eq!(raw.as_number().unwrap().to_bits(), value.to_bits());
        }
        let negative_nan = f64::from_bits(0xfff8_1234_5678_9abc);
        let raw = RawValue::number(negative_nan);
        assert!(raw.is_number());
        assert_eq!(raw.bits(), CANONICAL_NAN_BITS);
        assert!(raw.as_number().unwrap().is_nan());
    }

    #[test]
    fn immediate_tags_are_disjoint_from_numbers() {
        for value in [
            RawValue::UNDEFINED,
            RawValue::NULL,
            RawValue::FALSE,
            RawValue::TRUE,
            RawValue::HOLE,
        ] {
            assert!(!value.is_number());
            assert!(!value.is_heap());
        }
    }

    #[test]
    fn heap_pointer_round_trips() {
        let pointer = NonNull::from(Box::leak(Box::new(42_u64)));
        let raw = RawValue::tagged_pointer(OBJECT_TAG, pointer)
            .expect("host heap pointer fits NaN-box payload");
        assert!(raw.is_heap());
        assert_eq!(raw.tag(), OBJECT_TAG);
        assert_eq!(raw.as_heap::<u64>(), Some(pointer));
        unsafe { drop(Box::from_raw(pointer.as_ptr())) };
    }
}
