use super::Value;
use std::mem::size_of;

#[test]
fn a_reference_narrowing_of_null_is_the_general_one() {
    assert_eq!(Value::NULL.to_narrow_reference(), Value::NULL.to_narrow());
    assert_eq!(Value::from_narrow(Value::NULL.to_narrow_reference().unwrap()), Value::NULL);
}

#[test]
fn to_uint32_of_large_magnitudes_is_the_value_modulo_two_to_the_32() {
    const MODULUS: f64 = 4_294_967_296.0;
    let reference = |value: f64| value.trunc().rem_euclid(MODULUS) as u32;
    let mut bits: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..100_000 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        let value = f64::from_bits(bits);
        if value.is_finite() && value.abs() >= 9_223_372_036_854_775_808.0 {
            assert_eq!(super::number_to_u32(value), reference(value), "{value:e}");
        }
    }
    for value in [9.223372036854776e18, -9.223372036854776e18, 1.8446744073709552e19, 3e19, -3e19, f64::MAX, -f64::MAX] {
        assert_eq!(super::number_to_u32(value), reference(value), "{value:e}");
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(super::number_to_u32(value), 0);
    }
}

#[test]
fn narrow_values_round_trip_exactly() {
    let narrow = [
        Value::integer(0),
        Value::integer(-1),
        Value::integer((1 << 29) - 1),
        Value::integer(-(1 << 29)),
        Value::UNDEFINED,
        Value::NULL,
        Value::TRUE,
        Value::FALSE,
        Value::DELETED,
        Value::heap(7),
        Value::direct_object(0),
        Value::direct_object((1 << 25) - 1),
        Value::direct_string(3),
    ];
    for value in narrow {
        let packed = value.to_narrow().expect("narrow");
        assert_eq!(Value::from_narrow(packed), value);
        assert!(packed >> 30 == 0 || packed >> 30 == 3, "top bits copy each other");
    }
    let wide = [
        Value::integer(1 << 30),
        Value::integer(i32::MIN),
        Value::number(1.5),
        Value::number(-0.0),
        Value::direct_object(1 << 25),
        Value::heap(1 << 26),
    ];
    for value in wide {
        assert_eq!(value.to_narrow(), None, "{value:?}");
    }
}

#[test]
fn value_is_one_machine_word() {
    assert_eq!(size_of::<Value>(), size_of::<usize>());
}

#[test]
fn values_round_trip() {
    for value in [0.0, -0.0, 1.5, f64::INFINITY, f64::NAN] {
        let round = Value::number(value).as_number().unwrap();
        assert!(round == value || round.is_nan() && value.is_nan());
    }
    assert_eq!(Value::TRUE.as_bool(), Some(true));
    assert_eq!(Value::heap(42).heap_index(), Some(42));
    assert_eq!(
        Value::heap(42).heap_reference(),
        Some(super::HeapReference::LegacySlot(42))
    );
    let direct_object = Value::direct_object(42);
    assert_eq!(direct_object.heap_index(), Some(42));
    assert_eq!(direct_object.direct_object_index(), Some(42));
    assert_eq!(
        direct_object.heap_reference(),
        Some(super::HeapReference::ObjectRecord(42))
    );
    assert_ne!(direct_object, Value::heap(42));
    let direct_array = Value::direct_array(42);
    assert_eq!(direct_array.heap_index(), Some(42));
    assert_eq!(direct_array.direct_array_index(), Some(42));
    assert_eq!(
        direct_array.heap_reference(),
        Some(super::HeapReference::ArrayRecord(42))
    );
    assert_ne!(direct_array, direct_object);
    let direct_function = Value::direct_function(42);
    assert_eq!(direct_function.direct_function_index(), Some(42));
    assert_eq!(direct_function.direct_object_index(), None);
    assert_eq!(direct_function.direct_array_index(), None);
    assert_eq!(direct_function.legacy_heap_index(), None);
    assert_eq!(
        direct_function.heap_reference(),
        Some(super::HeapReference::FunctionRecord(42))
    );
    // A function may be the IsHTMLDDA object, so truthiness never answers for one.
    assert!(!direct_function.is_record());
    assert!(direct_object.is_record() && direct_array.is_record());
    let direct_promise = Value::direct_promise(42);
    assert_eq!(direct_promise.direct_promise_index(), Some(42));
    assert_eq!(direct_promise.direct_object_index(), None);
    assert_eq!(direct_promise.direct_function_index(), None);
    assert_eq!(
        direct_promise.heap_reference(),
        Some(super::HeapReference::PromiseRecord(42))
    );
    let direct_iterator = Value::direct_iterator(42);
    assert_eq!(direct_iterator.direct_iterator_index(), Some(42));
    assert_eq!(direct_iterator.direct_object_index(), None);
    assert_eq!(direct_iterator.direct_promise_index(), None);
    assert!(!direct_iterator.is_record());
    let direct_await = Value::direct_await(42);
    assert_eq!(direct_await.direct_await_index(), Some(42));
    assert_eq!(direct_await.direct_promise_index(), None);
    assert_eq!(direct_await.direct_array_index(), None);
    assert_eq!(
        direct_await.heap_reference(),
        Some(super::HeapReference::AwaitRecord(42))
    );
    assert_eq!(
        Value::int_pair(Value::integer(-2), Value::integer(3)),
        Some((-2, 3))
    );
    assert_eq!(Value::int_pair(Value::integer(1), Value::TRUE), None);
}

#[test]
fn numbers_preserve_signed_zero_and_canonicalize_nan() {
    let negative_zero = Value::number(-0.0).as_number().unwrap();
    assert!(negative_zero == 0.0 && negative_zero.is_sign_negative());
    assert_eq!(
        Value::number(f64::NAN),
        Value::number(f64::from_bits(0x7ff0_0000_0000_0001))
    );
}

#[test]
fn length_encoding_matches_number_at_representation_boundaries() {
    fn check(value: usize) {
        let encoded = Value::length(value);
        assert_eq!(encoded, Value::number(value as f64));
        assert_eq!(
            encoded.as_number().unwrap().to_bits(),
            (value as f64).to_bits(),
        );
        assert_eq!(encoded.as_int().is_some(), i32::try_from(value).is_ok());
    }
    for value in [
        0,
        1,
        i32::MAX as usize,
        i32::MAX as usize + 1,
        u32::MAX as usize,
        usize::MAX,
    ] {
        check(value);
    }
    for exponent in 1..usize::BITS {
        let power = 1_usize << exponent;
        for value in [power - 1, power, power + 1] {
            check(value);
        }
    }
    const SAMPLE_COUNT: usize = 100_000;
    const SAMPLE_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut value = 0_u64;
    for _ in 0..SAMPLE_COUNT {
        value = value.wrapping_add(SAMPLE_STEP);
        check(value as usize);
        check(value as u32 as usize);
    }
}

#[test]
fn number_encoding_preserves_arbitrary_float_bits_and_integer_boundaries() {
    fn check(number: f64) {
        let encoded = Value::number(number);
        let decoded = encoded.as_number().unwrap();
        if number.is_nan() {
            assert_eq!(encoded.to_bits(), super::CANONICAL_NAN);
        } else {
            assert_eq!(decoded.to_bits(), number.to_bits());
            let integer = number.fract() == 0.0
                && number >= f64::from(i32::MIN)
                && number <= f64::from(i32::MAX)
                && !(number == 0.0 && number.is_sign_negative());
            assert_eq!(encoded.as_int().is_some(), integer, "{number:?}");
        }
    }
    for number in [
        0.0,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MAX,
        -f64::MAX,
    ] {
        check(number);
    }
    for exponent in 0..=f64::MANTISSA_DIGITS {
        let power = 2_f64.powi(exponent as i32);
        for sign in [-1.0, 1.0] {
            for offset in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let number = sign * power + offset;
                check(number);
                check(number.next_up());
                check(number.next_down());
            }
        }
    }
    const SAMPLE_COUNT: usize = 100_000;
    // An odd Weyl step covers the whole word space before repeating; bound the
    // sample count so representation checks stay cheap in the regular suite.
    const SAMPLE_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut bits = 0_u64;
    for _ in 0..SAMPLE_COUNT {
        bits = bits.wrapping_add(SAMPLE_STEP);
        check(f64::from_bits(bits));
        check(f64::from(bits as i32));
    }
}
