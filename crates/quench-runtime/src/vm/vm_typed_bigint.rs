use crate::{ops::Builtin, value::Value};

macro_rules! bigint_property {
    ($name:ident, $data:ty, $prototype:ident) => {
        fn $name(view: &$data, key: &str) -> Value {
            if let Ok(index) = key.parse::<usize>() {
                return view
                    .get(index)
                    .map(|value| Value::BigInt(value.to_string()))
                    .unwrap_or(Value::Undefined);
            }
            let out = view.length != usize::MAX
                && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
            match key {
                "buffer" => Value::ArrayBuffer(view.buffer.clone()),
                "byteLength" => Value::Number(if out { 0 } else { view.byte_length() } as f64),
                "byteOffset" => Value::Number(if out { 0 } else { view.byte_offset } as f64),
                "length" => Value::Number(if out { 0 } else { view.logical_len() } as f64),
                "BYTES_PER_ELEMENT" => Value::Number(8.0),
                _ => crate::builtins::property(Builtin::$prototype, key),
            }
        }
    };
}

bigint_property!(
    signed_property,
    crate::value::BigInt64ArrayData,
    BigInt64ArrayPrototype
);
bigint_property!(
    unsigned_property,
    crate::value::BigUint64ArrayData,
    BigUint64ArrayPrototype
);

pub(super) fn property(value: &Value, key: &str) -> Value {
    match value {
        Value::BigInt64Array(view) => signed_property(view, key),
        Value::BigUint64Array(view) => unsigned_property(view, key),
        _ => Value::Undefined,
    }
}
