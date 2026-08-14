use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) fn execute(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    if builtin == Builtin::AtomicsIsLockFree {
        return Some(is_lock_free(arguments));
    }
    if builtin == Builtin::AtomicsPause {
        return Some(Ok(Value::Undefined));
    }
    if matches!(
        builtin,
        Builtin::AtomicsNotify | Builtin::AtomicsWait | Builtin::AtomicsWaitAsync
    ) {
        return Some(wait_operation(builtin, arguments));
    }
    if !is_operation(builtin) {
        return None;
    }
    Some(operation(builtin, arguments))
}

fn is_operation(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::AtomicsAdd
            | Builtin::AtomicsAnd
            | Builtin::AtomicsCompareExchange
            | Builtin::AtomicsExchange
            | Builtin::AtomicsLoad
            | Builtin::AtomicsOr
            | Builtin::AtomicsStore
            | Builtin::AtomicsSub
            | Builtin::AtomicsXor
    )
}

fn is_lock_free(arguments: &[Value]) -> Result<Value, VmError> {
    let size = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    Ok(Value::Boolean(matches!(size, 1.0 | 2.0 | 4.0 | 8.0)))
}

fn operation(builtin: Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    let view = arguments
        .first()
        .ok_or_else(|| type_error("Invalid typed array"))?;
    validate_view(view)?;
    let index = array_index(view, arguments.get(1))?;
    let old = crate::execute::get_property_result(view, &index.to_string())?;
    if builtin == Builtin::AtomicsLoad {
        return Ok(old);
    }
    if builtin == Builtin::AtomicsStore {
        let value = atomic_value(view, arguments.get(2))?;
        store(view, index, &value)?;
        return Ok(value);
    }
    if builtin == Builtin::AtomicsCompareExchange {
        let expected = atomic_value(view, arguments.get(2))?;
        if same_atomic(&old, &expected) {
            let replacement = atomic_value(view, arguments.get(3))?;
            store(view, index, &replacement)?;
        }
        return Ok(old);
    }
    let value = atomic_value(view, arguments.get(2))?;
    let next = arithmetic_value(builtin, &old, &value)?;
    store(view, index, &next)?;
    Ok(old)
}

fn validate_view(view: &Value) -> Result<(), VmError> {
    let valid = matches!(
        view,
        Value::Int8Array(_)
            | Value::Uint8Array(_)
            | Value::Int16Array(_)
            | Value::Uint16Array(_)
            | Value::Int32Array(_)
            | Value::Uint32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
    );
    if !valid {
        return Err(type_error("Invalid typed array"));
    }
    let shared = match view {
        Value::Int8Array(v) => v.buffer.shared,
        Value::Uint8Array(v) => v.buffer.shared,
        Value::Int16Array(v) => v.buffer.shared,
        Value::Uint16Array(v) => v.buffer.shared,
        Value::Int32Array(v) => v.buffer.shared,
        Value::Uint32Array(v) => v.buffer.shared,
        Value::BigInt64Array(v) => v.buffer.shared,
        Value::BigUint64Array(v) => v.buffer.shared,
        _ => false,
    };
    if !shared {
        return Err(type_error("Atomics requires a shared buffer"));
    }
    Ok(())
}

fn array_index(view: &Value, value: Option<&Value>) -> Result<usize, VmError> {
    let number = crate::conversion::to_number(value.unwrap_or(&Value::Undefined))?;
    let index = crate::construct::to_index(number)?;
    let length = crate::execute::get_property_result(view, "length")?;
    let length = crate::conversion::to_number(&length)? as usize;
    if index >= length {
        return Err(crate::value::error::throw_range_error("Invalid index"));
    }
    Ok(index)
}

fn atomic_value(view: &Value, value: Option<&Value>) -> Result<Value, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    if matches!(view, Value::BigInt64Array(_) | Value::BigUint64Array(_)) {
        let bits = crate::construct::bigint_bits(value)?;
        return Ok(bigint_from_bits(view, bits));
    }
    Ok(Value::Number(crate::conversion::to_number(value)?))
}

fn bigint_from_bits(view: &Value, bits: u64) -> Value {
    if matches!(view, Value::BigInt64Array(_)) {
        Value::BigInt((bits as i64).to_string())
    } else {
        Value::BigInt(bits.to_string())
    }
}

fn wait_operation(builtin: Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    let view = arguments
        .first()
        .ok_or_else(|| type_error("Invalid typed array"))?;
    validate_wait_view(view)?;
    let index = array_index(view, arguments.get(1))?;
    if builtin == Builtin::AtomicsNotify {
        return Ok(Value::Number(0.0));
    }
    let current = crate::execute::get_property_result(view, &index.to_string())?;
    let expected = atomic_value(view, arguments.get(2))?;
    if !same_atomic(&current, &expected) {
        return Ok(Value::String("not-equal".into()));
    }
    if builtin == Builtin::AtomicsWaitAsync {
        return Ok(Value::object(vec![
            ("async".into(), Value::Boolean(false)),
            ("value".into(), Value::String("timed-out".into())),
        ]));
    }
    Ok(Value::String("timed-out".into()))
}

fn validate_wait_view(view: &Value) -> Result<(), VmError> {
    let valid = matches!(view, Value::Int32Array(_) | Value::BigInt64Array(_));
    if !valid {
        return Err(type_error("Invalid typed array"));
    }
    validate_view(view)
}

fn arithmetic_value(builtin: Builtin, old: &Value, value: &Value) -> Result<Value, VmError> {
    if let (Value::BigInt(left), Value::BigInt(right)) = (old, value) {
        let left = left
            .parse::<i128>()
            .map_err(|_| type_error("Invalid BigInt"))? as u64;
        let right = right
            .parse::<i128>()
            .map_err(|_| type_error("Invalid BigInt"))? as u64;
        let result = match builtin {
            Builtin::AtomicsAdd => left.wrapping_add(right),
            Builtin::AtomicsSub => left.wrapping_sub(right),
            Builtin::AtomicsAnd => left & right,
            Builtin::AtomicsOr => left | right,
            Builtin::AtomicsXor => left ^ right,
            _ => right,
        };
        return Ok(bigint_from_bits(old, result));
    }
    let left = number(old);
    let right = number(value);
    let result = match builtin {
        Builtin::AtomicsAdd => left.wrapping_add(right),
        Builtin::AtomicsSub => left.wrapping_sub(right),
        Builtin::AtomicsAnd => left & right,
        Builtin::AtomicsOr => left | right,
        Builtin::AtomicsXor => left ^ right,
        _ => right,
    };
    Ok(Value::Number(result as f64))
}

fn number(value: &Value) -> i32 {
    match value {
        Value::Number(value) => *value as i32,
        _ => 0,
    }
}

fn same_atomic(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::BigInt(a), Value::BigInt(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => *a as i32 == *b as i32,
        _ => false,
    }
}

fn store(view: &Value, index: usize, value: &Value) -> Result<(), VmError> {
    crate::typed_array_ops::set_property(view, &index.to_string(), value)
        .ok_or_else(|| type_error("Invalid typed array"))??;
    Ok(())
}

fn type_error(message: &str) -> VmError {
    crate::value::error::throw_type_error(message)
}
