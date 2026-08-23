use crate::value::Value;

pub(super) fn array_like_length(value: &Value) -> usize {
    let length = match value {
        Value::Array(values) => values.len() as f64,
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(key, _)| key == "length")
            .map_or(0.0, |(_, value)| value_to_number(value)),
        _ => 0.0,
    };
    length.max(0.0).min(usize::MAX as f64) as usize
}

pub(super) fn array_like_value(value: &Value, index: usize) -> Value {
    match value {
        Value::Array(values) => values.get(index).cloned().unwrap_or(Value::Undefined),
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(key, _)| key == &index.to_string())
            .map_or(Value::Undefined, |(_, value)| value.clone()),
        _ => Value::Undefined,
    }
}

fn value_to_number(value: &Value) -> f64 {
    match value {
        Value::Number(value) => *value,
        Value::String(value) => value.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}
