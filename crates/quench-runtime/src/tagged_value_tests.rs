use super::*;
use core::mem::{align_of, size_of};

#[test]
fn one_word_layout() {
    assert_eq!(size_of::<TaggedValue>(), 8);
    assert_eq!(align_of::<TaggedValue>(), 8);
    assert_eq!(TaggedValue::null().bits(), 0x7ff8_3000_0000_0000);
    assert_eq!(TaggedValue::undefined().bits(), 0x7ff8_4000_0000_0000);
}

#[test]
fn numbers_preserve_signed_zero_and_canonicalize_nan() {
    for value in [0.0, -0.0, 1.5, f64::INFINITY, f64::NEG_INFINITY] {
        let tagged = TaggedValue::number(value);
        match tagged.decode() {
            DecodedValue::Number(actual) => assert_eq!(actual.to_bits(), value.to_bits()),
            _ => panic!(),
        }
        assert_eq!(tagged.number_value().unwrap().to_bits(), value.to_bits());
    }
    for bits in [0x7ff0_0000_0000_0001, 0x7ff8_1234_5678_9abc] {
        assert_eq!(TaggedValue::number(f64::from_bits(bits)).bits(), TAG_PREFIX);
        assert!(matches!(
            TaggedValue::number(f64::from_bits(bits)).decode(),
            DecodedValue::Number(value) if value.is_nan()
        ));
    }
}

#[test]
fn every_tag_round_trips_at_boundaries() {
    for value in [I31_MIN, -1, 0, 1, I31_MAX] {
        assert_eq!(
            TaggedValue::i31(value).unwrap().decode(),
            DecodedValue::I31(value)
        );
    }
    for value in [false, true] {
        assert_eq!(TaggedValue::bool(value).decode(), DecodedValue::Bool(value));
    }
    assert_eq!(TaggedValue::null().decode(), DecodedValue::Null);
    assert_eq!(TaggedValue::undefined().decode(), DecodedValue::Undefined);
    for payload in [0, 0x1234, PAYLOAD_MASK] {
        assert_eq!(
            TaggedValue::builtin(payload).unwrap().decode(),
            DecodedValue::Builtin(payload)
        );
    }
    let reference = HeapRef {
        index: HEAP_INDEX_MASK as u32,
        generation: HEAP_GENERATION_MASK as u32,
    };
    assert_eq!(
        TaggedValue::heap_ref(reference).unwrap().decode(),
        DecodedValue::HeapRef(reference)
    );
}

#[test]
fn bounds_are_checked() {
    assert!(TaggedValue::i31(i32::MIN).is_none());
    assert!(TaggedValue::i31(i32::MAX).is_none());
    assert!(TaggedValue::builtin(1 << 44).is_none());
    assert!(TaggedValue::heap_ref(HeapRef {
        index: 1 << 24,
        generation: 0
    })
    .is_none());
    assert!(TaggedValue::heap_ref(HeapRef {
        index: 0,
        generation: 1 << 20
    })
    .is_none());
}

#[test]
fn forged_singleton_payloads_are_rejected() {
    for bits in [
        TAG_PREFIX | (2 << TAG_SHIFT) | 2,
        TAG_PREFIX | (3 << TAG_SHIFT) | 1,
        TAG_PREFIX | (4 << TAG_SHIFT) | PAYLOAD_MASK,
        TAG_PREFIX | (7 << TAG_SHIFT),
    ] {
        assert!(matches!(
            TaggedValue::from_bits(bits).decode(),
            DecodedValue::Number(value) if value.is_nan()
        ));
    }
}

#[test]
fn from_bits_preserves_non_tagged_nan_as_number() {
    for bits in [0x7ff0_0000_0000_0001, 0x7ff7_ffff_ffff_ffff] {
        assert!(matches!(
            TaggedValue::from_bits(bits).decode(),
            DecodedValue::Number(value) if value.is_nan()
        ));
    }
}

#[test]
fn value_adapter_has_explicit_scalar_boundary() {
    use crate::value::Value;

    for value in [
        Value::Number(-0.0),
        Value::Number(f64::NAN),
        Value::Boolean(true),
        Value::Null,
        Value::Undefined,
    ] {
        let tagged = value.to_tagged().expect("scalar must be representable");
        let round_trip = Value::from_tagged(tagged).expect("scalar must decode");
        match (value, round_trip) {
            (Value::Number(expected), Value::Number(actual)) => {
                assert_eq!(expected.to_bits(), actual.to_bits());
            }
            (Value::Boolean(expected), Value::Boolean(actual)) => assert_eq!(expected, actual),
            (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => {}
            _ => panic!("adapter changed scalar kind"),
        }
    }

    assert!(Value::BigInt("1".into()).to_tagged().is_none());
    assert!(Value::String("heap-owned".into()).to_tagged().is_none());
    assert!(Value::from_tagged(TaggedValue::i31(7).unwrap()).is_some());
    assert!(Value::from_tagged(TaggedValue::builtin(1).unwrap()).is_none());
}

#[test]
fn tagged_value_size_evidence_is_machine_word() {
    assert_eq!(size_of::<TaggedValue>(), size_of::<u64>());
    assert_eq!(align_of::<TaggedValue>(), align_of::<u64>());
}

#[test]
fn tagged_equality_is_bitwise_and_round_trips_scalars() {
    assert_eq!(
        TaggedValue::bool(true),
        TaggedValue::from_bits(TaggedValue::bool(true).bits())
    );
    assert_ne!(TaggedValue::bool(false), TaggedValue::bool(true));
    assert_eq!(TaggedValue::number(-0.0), TaggedValue::number(-0.0));
    assert_ne!(TaggedValue::number(0.0), TaggedValue::number(-0.0));

    use crate::value::Value;
    let scalars = [
        Value::Number(42.5),
        Value::Boolean(false),
        Value::Null,
        Value::Undefined,
    ];
    for original in scalars {
        let tagged = original
            .to_tagged()
            .expect("scalar must fit tagged representation");
        let decoded = Value::from_tagged(tagged).expect("tagged scalar must decode");
        assert_eq!(original, decoded);
    }
}
