use crate::value::Value;

macro_rules! number_values {
    ($data:expr) => {
        collect_typed(
            checked_length(
                $data.length,
                $data.byte_offset,
                $data.byte_length(),
                $data.logical_len(),
                &$data.buffer,
            )?,
            |index| $data.get(index).map(|value| Value::Number(value.into())),
        )
    };
}
macro_rules! bigint_values {
    ($data:expr) => {
        collect_typed(
            checked_length(
                $data.length,
                $data.byte_offset,
                $data.byte_length(),
                $data.logical_len(),
                &$data.buffer,
            )?,
            |index| {
                $data
                    .get(index)
                    .map(|value| Value::BigInt(value.to_string()))
            },
        )
    };
}

// Keep the typed-array family as one semantic table.  Consumers provide the
// operation for numeric arrays, BigInt arrays, and the Uint16 lane (which has
// the only representation-specific float16 rule).
macro_rules! typed_array_dispatch {
    ($value:expr, $number:ident, $bigint:ident, $uint16:ident $(,)?) => {
        match $value {
            Value::Float64Array(data) => $number!(data),
            Value::Float32Array(data) => $number!(data),
            Value::Int8Array(data) => $number!(data),
            Value::Int16Array(data) => $number!(data),
            Value::Int32Array(data) => $number!(data),
            Value::Uint8Array(data) => $number!(data),
            Value::Uint8ClampedArray(data) => $number!(data),
            Value::Uint16Array(data) => $uint16!(data),
            Value::Uint32Array(data) => $number!(data),
            Value::BigInt64Array(data) => $bigint!(data),
            Value::BigUint64Array(data) => $bigint!(data),
            _ => Err(crate::collections::iterator::not_iterable()),
        }
    };
}

macro_rules! uint16_values {
    ($data:expr) => {{
        let length = checked_length(
            $data.length,
            $data.byte_offset,
            $data.byte_length(),
            $data.logical_len(),
            &$data.buffer,
        )?;
        collect_typed(length, |index| {
            $data.get(index).map(|value| {
                if $data.meta.property("\0float16_array").is_some()
                    || $data.meta.prototype().is_some_and(|prototype| {
                        matches!(
                            crate::execute::get_property(&prototype, "\0float16_constructor"),
                            Value::Boolean(true)
                        )
                    })
                {
                    Value::Number(crate::value::float16_to_float64(value))
                } else {
                    Value::Number(value.into())
                }
            })
        })
    }};
}

pub(crate) fn typed_values(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    if let Value::BindingCell(cell) = value {
        return typed_values(cell.load());
    }
    typed_array_dispatch!(value, number_values, bigint_values, uint16_values,)
}
fn checked_length(
    length: usize,
    byte_offset: usize,
    byte_length: usize,
    logical_len: usize,
    buffer: &crate::value::ArrayBufferData,
) -> Result<usize, crate::execute::VmError> {
    if length == usize::MAX {
        if *buffer.detached.borrow() {
            return Err(crate::collections::iterator::not_iterable());
        }
        if buffer.byte_length() < byte_offset {
            return Err(crate::collections::iterator::not_iterable());
        }
        return Ok(logical_len);
    }
    let required = byte_offset.saturating_add(byte_length);
    if *buffer.detached.borrow() || buffer.byte_length() < required {
        Err(crate::collections::iterator::not_iterable())
    } else {
        Ok(logical_len)
    }
}
pub(crate) fn typed_length(value: &Value) -> Result<usize, crate::execute::VmError> {
    if let Value::BindingCell(cell) = value {
        return typed_length(&cell.borrow());
    }
    macro_rules! length {
        ($data:expr) => {
            checked_length(
                $data.length,
                $data.byte_offset,
                $data.byte_length(),
                $data.logical_len(),
                &$data.buffer,
            )
        };
    }
    typed_array_dispatch!(value, length, length, length)
}
fn collect_typed<T>(
    length: usize,
    mut get: impl FnMut(usize) -> Option<T>,
) -> Result<Vec<T>, crate::execute::VmError> {
    (0..length)
        .map(|index| get(index).ok_or_else(crate::collections::iterator::not_iterable))
        .collect()
}
