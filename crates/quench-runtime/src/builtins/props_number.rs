use crate::value::Value;

pub(super) fn constant(key: &str) -> Option<Value> {
    let number = match key {
        "EPSILON" => f64::EPSILON,
        "MAX_SAFE_INTEGER" => 9_007_199_254_740_991.0,
        "MAX_VALUE" => f64::MAX,
        "MIN_SAFE_INTEGER" => -9_007_199_254_740_991.0,
        "MIN_VALUE" => f64::from_bits(1),
        "NaN" => f64::NAN,
        "NEGATIVE_INFINITY" => f64::NEG_INFINITY,
        "POSITIVE_INFINITY" => f64::INFINITY,
        _ => return None,
    };
    Some(Value::Number(number))
}
