use super::*;

/// An integer operator's result, checked against the general number operator it must match.
fn check_integer_operator<const OP: u32>(left: i32, right: i32) {
    let actual = integer_operator::<OP>(Value::integer(left), Value::integer(right)).unwrap();
    let expected = Value::number(number_operator::<OP>(f64::from(left), f64::from(right)).unwrap());
    assert_eq!(actual, expected, "operator={OP} left={left} right={right}");
    if let Some(raw) = int32_operator::<OP>(left, right) {
        assert_eq!(Value::integer(raw), actual, "int32 operator={OP} left={left} right={right}");
    } else {
        assert!(actual.as_int().is_none(), "int32 declined operator={OP} left={left} right={right}");
    }
}

/// Every add, subtract and multiply of the pair.
fn check_integer_pair(left: i32, right: i32) {
    check_integer_operator::<ADDITION>(left, right);
    check_integer_operator::<SUBTRACTION>(left, right);
    check_integer_operator::<MULTIPLICATION>(left, right);
}

#[test]
fn checked_integer_results_match_general_number_arithmetic() {
    let square_boundary = i32::MAX.isqrt();
    let edges = [
        i32::MIN,
        i32::MIN + 1,
        -square_boundary - 1,
        -square_boundary,
        -1,
        0,
        1,
        square_boundary,
        square_boundary + 1,
        i32::MAX - 1,
        i32::MAX,
    ];
    for left in edges {
        for right in edges {
            check_integer_pair(left, right);
        }
    }
    const SAMPLE_COUNT: usize = 100_000;
    const SAMPLE_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut bits = 0_u64;
    for _ in 0..SAMPLE_COUNT {
        bits = bits.wrapping_add(SAMPLE_STEP);
        check_integer_pair(bits as i32, (bits >> i32::BITS) as i32);
    }
}
