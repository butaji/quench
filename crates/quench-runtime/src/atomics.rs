use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{PromiseData, PromiseState, Value},
};

struct Waiter {
    buffer: Rc<crate::value::ArrayBufferData>,
    index: usize,
    promise: Rc<PromiseData>,
}

thread_local! {
    static WAITERS: RefCell<Vec<Waiter>> = const { RefCell::new(Vec::new()) };
}

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
    if is_write_operation(builtin) && view_buffer(view).is_some_and(|buffer| buffer.immutable) {
        return Err(type_error("Cannot write to an immutable ArrayBuffer"));
    }
    let index = array_index(view, arguments.get(1))?;
    let old = crate::execute::get_property_result(view, &index.to_string())?;
    if builtin == Builtin::AtomicsLoad {
        return Ok(old);
    }
    if builtin == Builtin::AtomicsStore {
        let value = atomic_value(view, arguments.get(2))?;
        store(view, index, &value)?;
        return store_result(view, arguments.get(2));
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
    let next = arithmetic_value(builtin, view, &old, &value)?;
    store(view, index, &next)?;
    Ok(old)
}

fn is_write_operation(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::AtomicsAdd
            | Builtin::AtomicsAnd
            | Builtin::AtomicsCompareExchange
            | Builtin::AtomicsExchange
            | Builtin::AtomicsOr
            | Builtin::AtomicsStore
            | Builtin::AtomicsSub
            | Builtin::AtomicsXor
    )
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
    Ok(())
}

fn array_index(view: &Value, value: Option<&Value>) -> Result<usize, VmError> {
    let length = view_length(view);
    if let Some(Value::BigInt(raw)) = value {
        let index = raw
            .parse::<u128>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid index"))?;
        return check_index(length, usize::try_from(index).unwrap_or(usize::MAX));
    }
    let number = crate::conversion::to_number(value.unwrap_or(&Value::Undefined))?;
    let index = crate::construct::to_index(number)?;
    check_index(length, index)
}

fn view_length(view: &Value) -> usize {
    match view {
        Value::Int8Array(v) => v.logical_len(),
        Value::Uint8Array(v) => v.logical_len(),
        Value::Int16Array(v) => v.logical_len(),
        Value::Uint16Array(v) => v.logical_len(),
        Value::Int32Array(v) => v.logical_len(),
        Value::Uint32Array(v) => v.logical_len(),
        Value::BigInt64Array(v) => v.logical_len(),
        Value::BigUint64Array(v) => v.logical_len(),
        _ => 0,
    }
}

fn check_index(length: usize, index: usize) -> Result<usize, VmError> {
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
    let number = crate::conversion::to_number(value)?;
    let normalized = match view {
        Value::Int8Array(_) => crate::construct::to_int8(number) as f64,
        Value::Uint8Array(_) => crate::construct::to_uint8(number) as f64,
        Value::Int16Array(_) => crate::construct::to_int16(number) as f64,
        Value::Uint16Array(_) => crate::construct::to_uint16(number) as f64,
        Value::Int32Array(_) => crate::construct::to_int32(number) as f64,
        Value::Uint32Array(_) => crate::construct::to_uint32(number) as f64,
        _ => number,
    };
    Ok(Value::Number(normalized))
}

fn store_result(view: &Value, value: Option<&Value>) -> Result<Value, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    let primitive = crate::conversion::to_primitive(value, "number")?;
    if matches!(view, Value::BigInt64Array(_) | Value::BigUint64Array(_)) {
        let raw = match primitive {
            Value::BigInt(raw) | Value::String(raw) => raw,
            _ => return Err(type_error("Cannot convert a non-BigInt value to BigInt")),
        };
        raw.parse::<num_bigint::BigInt>()
            .map_err(|_| type_error("Invalid BigInt value"))?;
        return Ok(Value::BigInt(raw));
    }
    let number = crate::conversion::to_number(&primitive)?;
    let result = if number.is_nan() { 0.0 } else { number.trunc() };
    Ok(Value::Number(if result == 0.0 { 0.0 } else { result }))
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
    if builtin == Builtin::AtomicsNotify {
        validate_notify_view(view)?;
    } else {
        validate_wait_view(view)?;
    }
    let index = array_index(view, arguments.get(1))?;
    if builtin == Builtin::AtomicsNotify {
        let count = wait_count(arguments.get(2))?;
        if view_buffer(view).is_some_and(|buffer| !buffer.shared) {
            return Ok(Value::Number(0.0));
        }
        let buffer = view_buffer(view).ok_or_else(|| type_error("Invalid typed array"))?;
        return Ok(Value::Number(wake_waiters(buffer, index, count) as f64));
    }
    let current = crate::execute::get_property_result(view, &index.to_string())?;
    let expected = atomic_value(view, arguments.get(2))?;
    if !same_atomic(&current, &expected) {
        if builtin == Builtin::AtomicsWaitAsync {
            return Ok(Value::object(vec![
                ("async".into(), Value::Boolean(false)),
                ("value".into(), Value::String("not-equal".into())),
            ]));
        }
        return Ok(Value::String("not-equal".into()));
    }
    let timeout = crate::conversion::to_number(arguments.get(3).unwrap_or(&Value::Undefined))?;
    if builtin == Builtin::AtomicsWait {
        return Err(type_error("Atomics.wait cannot suspend this agent"));
    }
    if builtin == Builtin::AtomicsWaitAsync {
        return wait_async_result(view, index, timeout);
    }
    Ok(Value::String("timed-out".into()))
}

fn wait_count(value: Option<&Value>) -> Result<usize, VmError> {
    if value.is_none() || matches!(value, Some(Value::Undefined)) {
        return Ok(usize::MAX);
    }
    let number = crate::conversion::to_number(value.unwrap_or(&Value::Undefined))?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(if number.is_infinite() {
        usize::MAX
    } else {
        number.trunc() as usize
    })
}

fn wake_waiters(buffer: &Rc<crate::value::ArrayBufferData>, index: usize, count: usize) -> usize {
    let mut woken = 0;
    WAITERS.with(|waiters| {
        waiters.borrow_mut().retain(|waiter| {
            let matches = Rc::ptr_eq(&waiter.buffer, buffer) && waiter.index == index;
            if matches && woken < count {
                crate::promise::resolve_promise(&waiter.promise, Value::String("ok".into()));
                woken += 1;
                false
            } else {
                true
            }
        });
    });
    woken
}

fn wait_async_result(view: &Value, index: usize, timeout: f64) -> Result<Value, VmError> {
    if timeout.is_finite() && timeout <= 0.0 {
        return Ok(Value::object(vec![
            ("async".into(), Value::Boolean(false)),
            ("value".into(), Value::String("timed-out".into())),
        ]));
    }
    let buffer = view_buffer(view)
        .ok_or_else(|| type_error("Invalid typed array"))?
        .clone();
    let promise = Rc::new(PromiseData::new(PromiseState::Pending));
    WAITERS.with(|waiters| {
        waiters.borrow_mut().push(Waiter {
            buffer,
            index,
            promise: Rc::clone(&promise),
        })
    });
    Ok(Value::object(vec![
        ("async".into(), Value::Boolean(true)),
        ("value".into(), Value::Promise(promise)),
    ]))
}

fn validate_wait_view(view: &Value) -> Result<(), VmError> {
    let valid = matches!(view, Value::Int32Array(_) | Value::BigInt64Array(_));
    if !valid {
        return Err(type_error("Invalid typed array"));
    }
    if matches!(view, Value::Int32Array(v) if !v.buffer.shared)
        || matches!(view, Value::BigInt64Array(v) if !v.buffer.shared)
    {
        return Err(type_error("Atomics.wait requires a shared buffer"));
    }
    validate_view(view)
}

fn validate_notify_view(view: &Value) -> Result<(), VmError> {
    if !matches!(view, Value::Int32Array(_) | Value::BigInt64Array(_)) {
        return Err(type_error("Invalid typed array"));
    }
    if view_buffer(view).is_some_and(|buffer| *buffer.detached.borrow()) {
        return Err(type_error("Detached ArrayBuffer"));
    }
    validate_view(view)
}

fn arithmetic_value(
    builtin: Builtin,
    view: &Value,
    old: &Value,
    value: &Value,
) -> Result<Value, VmError> {
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
    let left = integer_bits(view, old);
    let right = integer_bits(view, value);
    let result = match builtin {
        Builtin::AtomicsAdd => left.wrapping_add(right),
        Builtin::AtomicsSub => left.wrapping_sub(right),
        Builtin::AtomicsAnd => left & right,
        Builtin::AtomicsOr => left | right,
        Builtin::AtomicsXor => left ^ right,
        _ => right,
    };
    Ok(Value::Number(if is_unsigned(view) {
        result as u32 as f64
    } else {
        result as i32 as f64
    }))
}

fn integer_bits(view: &Value, value: &Value) -> i32 {
    match value {
        Value::Number(value) if is_unsigned(view) => *value as u32 as i32,
        Value::Number(value) => *value as i32,
        _ => 0,
    }
}

fn is_unsigned(view: &Value) -> bool {
    matches!(
        view,
        Value::Uint8Array(_) | Value::Uint16Array(_) | Value::Uint32Array(_)
    )
}

fn same_atomic(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::BigInt(a), Value::BigInt(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => *a as i32 == *b as i32,
        _ => false,
    }
}

fn store(view: &Value, index: usize, value: &Value) -> Result<(), VmError> {
    if view_buffer(view).is_some_and(|buffer| buffer.immutable) {
        return Err(type_error("Cannot write to an immutable ArrayBuffer"));
    }
    crate::typed_array_ops::set_property(view, &index.to_string(), value)
        .ok_or_else(|| type_error("Invalid typed array"))??;
    Ok(())
}

fn view_buffer(view: &Value) -> Option<&std::rc::Rc<crate::value::ArrayBufferData>> {
    match view {
        Value::Int8Array(v) => Some(&v.buffer),
        Value::Uint8Array(v) => Some(&v.buffer),
        Value::Int16Array(v) => Some(&v.buffer),
        Value::Uint16Array(v) => Some(&v.buffer),
        Value::Int32Array(v) => Some(&v.buffer),
        Value::Uint32Array(v) => Some(&v.buffer),
        Value::BigInt64Array(v) => Some(&v.buffer),
        Value::BigUint64Array(v) => Some(&v.buffer),
        _ => None,
    }
}

fn type_error(message: &str) -> VmError {
    crate::value::error::throw_type_error(message)
}
