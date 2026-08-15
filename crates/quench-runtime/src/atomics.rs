use crate::{execute::VmError, value::Value};

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if builtin == crate::ops::Builtin::AtomicsIsLockFree {
        return is_lock_free(arguments);
    }
    if builtin == crate::ops::Builtin::AtomicsPause {
        return Ok(Value::Undefined);
    }
    if builtin == crate::ops::Builtin::AtomicsStore {
        return store(arguments);
    }
    if builtin == crate::ops::Builtin::AtomicsSub {
        return read_modify_write(arguments, RmwOperation::Sub);
    }
    if builtin == crate::ops::Builtin::AtomicsExchange {
        return read_modify_write(arguments, RmwOperation::Exchange);
    }
    if builtin == crate::ops::Builtin::AtomicsLoad {
        return load(arguments);
    }
    if builtin == crate::ops::Builtin::AtomicsAnd {
        return read_modify_write(arguments, RmwOperation::And);
    }
    if builtin == crate::ops::Builtin::AtomicsOr {
        return read_modify_write(arguments, RmwOperation::Or);
    }
    if builtin == crate::ops::Builtin::AtomicsXor {
        return read_modify_write(arguments, RmwOperation::Xor);
    }
    if builtin == crate::ops::Builtin::AtomicsCompareExchange {
        return compare_exchange(arguments);
    }
    read_modify_write(arguments, RmwOperation::Add)
}

fn is_lock_free(arguments: &[Value]) -> Result<Value, VmError> {
    let size = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    let size = if size.is_nan() {
        0
    } else {
        size.trunc() as i64
    };
    Ok(Value::Boolean(matches!(size, 4)))
}

#[derive(Clone, Copy)]
enum RmwOperation {
    Add,
    Sub,
    Exchange,
    And,
    Or,
    Xor,
}

fn compare_exchange(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(view) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.compareExchange requires an integer typed array",
        ));
    };
    if !is_integer_typed_array(view) || !is_shared(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics.compareExchange requires a shared integer typed array",
        ));
    }
    let index = arguments
        .get(1)
        .map(crate::conversion::to_number)
        .transpose()?
        .map(crate::construct::to_index)
        .transpose()?
        .unwrap_or(0);
    macro_rules! compare_integer {
        ($variant:ident, $convert:ident) => {
            if let Some(Value::$variant(data)) = arguments.first() {
                let old = data.get(index).ok_or_else(|| {
                    crate::value::error::throw_range_error("Atomics index is out of bounds")
                })?;
                let expected = arguments
                    .get(2)
                    .map(crate::conversion::to_number)
                    .transpose()?
                    .unwrap_or(0.0);
                let expected = crate::construct::$convert(expected);
                let replacement = arguments
                    .get(3)
                    .map(crate::conversion::to_number)
                    .transpose()?
                    .unwrap_or(0.0);
                let replacement = crate::construct::$convert(replacement);
                if old == expected && !data.set(index, replacement) {
                    return Err(crate::value::error::throw_range_error(
                        "Atomics index is out of bounds",
                    ));
                }
                return Ok(Value::Number(old as f64));
            }
        };
    }
    compare_integer!(Int8Array, to_int8);
    compare_integer!(Uint8Array, to_uint8);
    compare_integer!(Int16Array, to_int16);
    compare_integer!(Uint16Array, to_uint16);
    compare_integer!(Int32Array, to_int32);
    compare_integer!(Uint32Array, to_uint32);
    if let Some(Value::BigInt64Array(data)) = arguments.first() {
        let old = data.get(index).ok_or_else(|| {
            crate::value::error::throw_range_error("Atomics index is out of bounds")
        })?;
        let expected = match arguments.get(2) {
            Some(Value::BigInt(value)) => value,
            _ => {
                return Err(crate::value::error::throw_type_error(
                    "Atomics BigInt expected value required",
                ))
            }
        };
        let replacement = match arguments.get(3) {
            Some(Value::BigInt(value)) => value,
            _ => {
                return Err(crate::value::error::throw_type_error(
                    "Atomics BigInt replacement required",
                ))
            }
        };
        if old.to_string() == *expected {
            let value = replacement
                .parse::<i64>()
                .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt replacement"))?;
            if !data.set(index, value) {
                return Err(crate::value::error::throw_range_error(
                    "Atomics index is out of bounds",
                ));
            }
        }
        return Ok(Value::BigInt(old.to_string()));
    }
    if let Some(Value::BigUint64Array(data)) = arguments.first() {
        let old = data.get(index).ok_or_else(|| {
            crate::value::error::throw_range_error("Atomics index is out of bounds")
        })?;
        let expected = match arguments.get(2) {
            Some(Value::BigInt(value)) => value,
            _ => {
                return Err(crate::value::error::throw_type_error(
                    "Atomics BigInt expected value required",
                ))
            }
        };
        let replacement = match arguments.get(3) {
            Some(Value::BigInt(value)) => value,
            _ => {
                return Err(crate::value::error::throw_type_error(
                    "Atomics BigInt replacement required",
                ))
            }
        };
        if old.to_string() == *expected {
            let value = replacement
                .parse::<u64>()
                .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt replacement"))?;
            if !data.set(index, value) {
                return Err(crate::value::error::throw_range_error(
                    "Atomics index is out of bounds",
                ));
            }
        }
        return Ok(Value::BigInt(old.to_string()));
    }
    Err(crate::value::error::throw_type_error(
        "Atomics.compareExchange requires an integer typed array",
    ))
}

fn load(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(view) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.load requires a view",
        ));
    };
    if !is_integer_typed_array(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics.load requires an integer typed array",
        ));
    }
    let index = arguments
        .get(1)
        .map(crate::conversion::to_number)
        .transpose()?
        .map(crate::construct::to_index)
        .transpose()?
        .unwrap_or(0);
    macro_rules! load_integer {
        ($variant:ident) => {
            if let Value::$variant(data) = view {
                let value = data.get(index).ok_or_else(|| {
                    crate::value::error::throw_range_error("Atomics index is out of bounds")
                })?;
                return Ok(Value::Number(value as f64));
            }
        };
    }
    load_integer!(Int8Array);
    load_integer!(Uint8Array);
    load_integer!(Int16Array);
    load_integer!(Uint16Array);
    load_integer!(Int32Array);
    load_integer!(Uint32Array);
    Ok(Value::Undefined)
}

fn read_modify_write(arguments: &[Value], operation: RmwOperation) -> Result<Value, VmError> {
    let Some(view) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics RMW requires a typed array",
        ));
    };
    if !is_integer_typed_array(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics RMW requires an integer typed array",
        ));
    }
    if !is_shared(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics RMW requires shared buffer data",
        ));
    }
    let index = arguments
        .get(1)
        .map(crate::conversion::to_number)
        .transpose()?
        .map(crate::construct::to_index)
        .transpose()?
        .unwrap_or(0);
    let length =
        crate::conversion::to_number(&crate::execute::get_property_result(view, "length")?)?
            as usize;
    if index >= length {
        return Err(crate::value::error::throw_range_error(
            "Atomics index is out of bounds",
        ));
    }
    macro_rules! rmw_integer {
        ($variant:ident, $convert:ident) => {
            if let Value::$variant(data) = view {
                let old = data.get(index).ok_or_else(|| {
                    crate::value::error::throw_range_error("Atomics index is out of bounds")
                })?;
                let value = arguments
                    .get(2)
                    .map(crate::conversion::to_number)
                    .transpose()?
                    .unwrap_or(0.0);
                let value = crate::construct::$convert(value);
                let replacement = match operation {
                    RmwOperation::Add => old.wrapping_add(value),
                    RmwOperation::Sub => old.wrapping_sub(value),
                    RmwOperation::Exchange => value,
                    RmwOperation::And => old & value,
                    RmwOperation::Or => old | value,
                    RmwOperation::Xor => old ^ value,
                };
                if !data.set(index, replacement) {
                    return Err(crate::value::error::throw_range_error(
                        "Atomics index is out of bounds",
                    ));
                }
                return Ok(Value::Number(old as f64));
            }
        };
    }
    rmw_integer!(Int8Array, to_int8);
    rmw_integer!(Uint8Array, to_uint8);
    rmw_integer!(Int16Array, to_int16);
    rmw_integer!(Uint16Array, to_uint16);
    rmw_integer!(Int32Array, to_int32);
    rmw_integer!(Uint32Array, to_uint32);
    Ok(Value::Undefined)
}

fn store(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(view) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.store requires a view",
        ));
    };
    if !is_integer_typed_array(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics.store requires an integer typed array",
        ));
    }
    let index = arguments
        .get(1)
        .map(crate::conversion::to_number)
        .transpose()?
        .map(crate::construct::to_index)
        .transpose()?
        .unwrap_or(0);
    let length =
        crate::conversion::to_number(&crate::execute::get_property_result(view, "length")?)?
            as usize;
    if index >= length {
        return Err(crate::value::error::throw_range_error(
            "Atomics index is out of bounds",
        ));
    }
    let value = arguments
        .get(2)
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    macro_rules! store_integer {
        ($variant:ident, $convert:ident) => {
            if let Value::$variant(data) = view {
                let converted = crate::construct::$convert(value);
                if !data.set(index, converted) {
                    return Err(crate::value::error::throw_range_error(
                        "Atomics index is out of bounds",
                    ));
                }
                return Ok(Value::Number(if value.is_nan() || value == 0.0 {
                    0.0
                } else {
                    value.trunc()
                }));
            }
        };
    }
    store_integer!(Int8Array, to_int8);
    store_integer!(Uint8Array, to_uint8);
    store_integer!(Int16Array, to_int16);
    store_integer!(Uint16Array, to_uint16);
    store_integer!(Int32Array, to_int32);
    store_integer!(Uint32Array, to_uint32);
    Ok(Value::Undefined)
}

fn is_integer_typed_array(value: &Value) -> bool {
    matches!(
        value,
        Value::Int8Array(_)
            | Value::Uint8Array(_)
            | Value::Int16Array(_)
            | Value::Uint16Array(_)
            | Value::Int32Array(_)
            | Value::Uint32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
    )
}

fn is_shared(value: &Value) -> bool {
    match value {
        Value::Int8Array(data) => data.buffer.shared,
        Value::Uint8Array(data) => data.buffer.shared,
        Value::Int16Array(data) => data.buffer.shared,
        Value::Uint16Array(data) => data.buffer.shared,
        Value::Int32Array(data) => data.buffer.shared,
        Value::Uint32Array(data) => data.buffer.shared,
        Value::BigInt64Array(data) => data.buffer.shared,
        Value::BigUint64Array(data) => data.buffer.shared,
        _ => false,
    }
}
