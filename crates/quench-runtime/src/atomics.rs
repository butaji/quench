use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) fn is_lock_free(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments.first().unwrap_or(&Value::Undefined);
    let size = crate::conversion::to_number(value)?;
    Ok(Value::Boolean(matches!(size, 1.0 | 2.0 | 4.0)))
}

pub(crate) fn notify(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Int32Array(view)) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.notify requires an Int32Array",
        ));
    };
    if !view.buffer.shared {
        return Err(crate::value::error::throw_type_error(
            "Atomics.notify requires a shared buffer",
        ));
    }
    let index = atomic_index(arguments.get(1))?;
    if view.get(index).is_none() {
        return Err(crate::value::error::throw_range_error(
            "Atomics.notify index is out of range",
        ));
    }
    let _count = arguments
        .get(2)
        .map(crate::conversion::to_number)
        .transpose()?;
    Ok(Value::Number(0.0))
}

pub(crate) fn execute(
    builtin: Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Int32Array(view)) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires an Int32Array",
        ));
    };
    if !view.buffer.shared {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a shared buffer",
        ));
    }
    let index = atomic_index(arguments.get(1))?;
    let old = view
        .get(index)
        .ok_or_else(|| crate::value::error::throw_range_error("Atomics index is out of range"))?;
    if builtin == Builtin::AtomicsCompareExchange {
        let expected = atomic_value(arguments.get(2))?;
        if old == expected {
            view.set(index, atomic_value(arguments.get(3))?);
        }
        return Ok(Value::Number(old as f64));
    }
    let value = atomic_value(arguments.get(2))?;
    let updated = match builtin {
        Builtin::AtomicsAdd => old.wrapping_add(value),
        Builtin::AtomicsAnd => old & value,
        Builtin::AtomicsOr => old | value,
        Builtin::AtomicsSub => old.wrapping_sub(value),
        Builtin::AtomicsXor => old ^ value,
        _ => return Err(crate::vm::not_callable()),
    };
    view.set(index, updated);
    Ok(Value::Number(old as f64))
}

fn atomic_index(value: Option<&Value>) -> Result<usize, VmError> {
    let value = value.ok_or_else(|| crate::value::error::throw_type_error("Missing index"))?;
    crate::construct::to_index(crate::conversion::to_number(value)?)
}

fn atomic_value(value: Option<&Value>) -> Result<i32, VmError> {
    let value = value.ok_or_else(|| crate::value::error::throw_type_error("Missing value"))?;
    Ok(crate::construct::to_int32(crate::conversion::to_number(
        value,
    )?))
}
