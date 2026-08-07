use super::*;

#[test]
fn strict_eq_positive_zero_negative_zero() {
    // +0 === -0 per strict equality
    assert!(strict_eq(&Value::Number(0.0), &Value::Number(-0.0)));
}
