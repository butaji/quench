use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) fn is_lock_free(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments.first().unwrap_or(&Value::Undefined);
    let size = crate::conversion::to_number(value)?;
    Ok(Value::Boolean(matches!(size, 1.0 | 2.0 | 4.0 | 8.0)))
}

pub(crate) fn notify(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(view) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.notify requires a typed array",
        ));
    };
    match view {
        Value::Int32Array(view) => {
            let length = view.logical_len();
            if *view.buffer.detached.borrow() {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.notify requires an attached buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.notify index is out of range",
                ));
            }
        }
        Value::BigInt64Array(view) => {
            let length = view.logical_len();
            if *view.buffer.detached.borrow() {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.notify requires an attached buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.notify index is out of range",
                ));
            }
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Atomics.notify requires an Int32Array or BigInt64Array",
            ))
        }
    }
    let _count = arguments
        .get(2)
        .map(crate::conversion::to_number)
        .transpose()?;
    Ok(Value::Number(0.0))
}

pub(crate) fn wait(arguments: &[Value]) -> Result<Value, VmError> {
    let (current, expected) = match arguments.first() {
        Some(Value::Int32Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.wait requires a shared buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            (
                view.get(index)
                    .ok_or_else(|| {
                        crate::value::error::throw_range_error("Atomics.wait index is out of range")
                    })?
                    .to_string(),
                atomic_value(arguments.get(2))?.to_string(),
            )
        }
        Some(Value::BigInt64Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.wait requires a shared buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            (
                view.get(index)
                    .ok_or_else(|| {
                        crate::value::error::throw_range_error("Atomics.wait index is out of range")
                    })?
                    .to_string(),
                bigint_argument(arguments.get(2))?.to_string(),
            )
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Atomics.wait requires an Int32Array or BigInt64Array",
            ))
        }
    };
    if current != expected {
        return Ok(Value::String("not-equal".into()));
    }
    let _timeout = arguments
        .get(3)
        .map(crate::conversion::to_number)
        .transpose()?;
    Ok(Value::String("timed-out".into()))
}

pub(crate) fn load_store(builtin: Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(
        arguments.first(),
        Some(Value::BigInt64Array(_) | Value::BigUint64Array(_))
    ) {
        let view = bigint_view(arguments.first())?;
        let index = atomic_index(arguments.get(1))?;
        if builtin == Builtin::AtomicsLoad {
            return Ok(Value::BigInt(bigint_old(view, index)?));
        }
        if bigint_view_immutable(view) {
            return Err(crate::value::error::throw_type_error(
                "Atomics operation requires a writable buffer",
            ));
        }
        let value = bigint_argument(arguments.get(2))?;
        let bits = crate::construct::bigint_bits(&Value::BigInt(value.to_string()))?;
        let ok = match view {
            Value::BigInt64Array(v) => v.set(index, bits as i64),
            Value::BigUint64Array(v) => v.set(index, bits),
            _ => false,
        };
        if !ok {
            return Err(crate::value::error::throw_range_error(
                "Atomics index is out of range",
            ));
        }
        return Ok(Value::BigInt(value.to_string()));
    }
    let Some(view) = atomic_view(arguments.first()) else {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires an Int32Array",
        ));
    };
    let index = atomic_index(arguments.get(1))?;
    if builtin == Builtin::AtomicsLoad {
        return view.get_number(index).map(Value::Number).ok_or_else(|| {
            crate::value::error::throw_range_error("Atomics index is out of range")
        });
    }
    if view.immutable() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let value = atomic_value(arguments.get(2))?;
    if !view.set(index, value) {
        return Err(crate::value::error::throw_range_error(
            "Atomics index is out of range",
        ));
    }
    Ok(Value::Number(value as f64))
}

pub(crate) fn exchange(arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(
        arguments.first(),
        Some(Value::BigInt64Array(_) | Value::BigUint64Array(_))
    ) {
        let view = bigint_view(arguments.first())?;
        if bigint_view_immutable(view) {
            return Err(crate::value::error::throw_type_error(
                "Atomics operation requires a writable buffer",
            ));
        }
        let index = atomic_index(arguments.get(1))?;
        let old = bigint_old(view, index)?;
        let value = bigint_argument(arguments.get(2))?;
        let bits = crate::construct::bigint_bits(&Value::BigInt(value.to_string()))?;
        match view {
            Value::BigInt64Array(v) if v.set(index, bits as i64) => {}
            Value::BigUint64Array(v) if v.set(index, bits) => {}
            Value::BigInt64Array(_) | Value::BigUint64Array(_) => {
                return Err(crate::value::error::throw_range_error(
                    "Atomics index is out of range",
                ));
            }
            _ => {}
        }
        return Ok(Value::BigInt(old));
    }
    let Some(view) = atomic_view(arguments.first()) else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.exchange requires an integer typed array",
        ));
    };
    if view.immutable() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let index = atomic_index(arguments.get(1))?;
    let old = view.get(index).ok_or_else(|| {
        crate::value::error::throw_range_error("Atomics.exchange index is out of range")
    })?;
    let old_number = view.get_number(index).unwrap_or(old as f64);
    let value = atomic_value(arguments.get(2))?;
    view.set(index, value);
    Ok(Value::Number(old_number))
}

pub(crate) fn wait_async(arguments: &[Value]) -> Result<Value, VmError> {
    let result = match arguments.first() {
        Some(Value::BigInt64Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.waitAsync requires a shared buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            let expected = bigint_argument(arguments.get(2))?.to_string();
            let current = view
                .get(index)
                .ok_or_else(|| {
                    crate::value::error::throw_range_error(
                        "Atomics.waitAsync index is out of range",
                    )
                })?
                .to_string();
            if current != expected {
                "not-equal"
            } else {
                "timed-out"
            }
        }
        Some(Value::Int32Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.waitAsync requires a shared buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            let expected = atomic_value(arguments.get(2))?;
            let current = view.get(index).ok_or_else(|| {
                crate::value::error::throw_range_error("Atomics.waitAsync index is out of range")
            })?;
            if current != expected {
                "not-equal"
            } else {
                "timed-out"
            }
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Atomics.waitAsync requires an Int32Array or BigInt64Array",
            ))
        }
    };
    Ok(crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("async".into(), Value::Boolean(false)),
            ("value".into(), Value::String(result.into())),
        ]),
    )))
}

pub(crate) fn execute(
    builtin: Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if matches!(
        arguments.first(),
        Some(Value::BigInt64Array(_) | Value::BigUint64Array(_))
    ) {
        return execute_bigint(builtin, arguments);
    }
    let Some(view) = atomic_view(arguments.first()) else {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires an Int32Array",
        ));
    };
    if view.immutable() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let index = atomic_index(arguments.get(1))?;
    let old = view
        .get(index)
        .ok_or_else(|| crate::value::error::throw_range_error("Atomics index is out of range"))?;
    let old_number = view.get_number(index).unwrap_or(old as f64);
    if builtin == Builtin::AtomicsCompareExchange {
        let expected = atomic_value(arguments.get(2))?;
        if old == expected {
            view.set(index, atomic_value(arguments.get(3))?);
        }
        return Ok(Value::Number(old_number));
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
    Ok(Value::Number(old_number))
}

fn execute_bigint(builtin: Builtin, args: &[Value]) -> Result<Value, VmError> {
    let view = bigint_view(args.first())?;
    if bigint_view_immutable(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let index = atomic_index(args.get(1))?;
    let old = bigint_old(view, index)?;
    let replacement = bigint_result(builtin, args, &old)?;
    let replacement = match view {
        Value::BigInt64Array(_) => {
            let bits = crate::construct::bigint_bits(&Value::BigInt(replacement))?;
            (bits as i64).to_string()
        }
        Value::BigUint64Array(_) => {
            let bits = crate::construct::bigint_bits(&Value::BigInt(replacement))?;
            bits.to_string()
        }
        _ => replacement,
    };
    bigint_write(
        view,
        index,
        &replacement,
        builtin == Builtin::AtomicsCompareExchange && replacement == old,
    )?;
    Ok(Value::BigInt(old))
}

fn bigint_view(value: Option<&Value>) -> Result<&Value, VmError> {
    match value {
        Some(value @ (Value::BigInt64Array(_) | Value::BigUint64Array(_))) => Ok(value),
        _ => Err(crate::value::error::throw_type_error(
            "Atomics requires a BigInt typed array",
        )),
    }
}

fn bigint_old(view: &Value, index: usize) -> Result<String, VmError> {
    let value = match view {
        Value::BigInt64Array(v) => v.get(index).map(|x| x.to_string()),
        Value::BigUint64Array(v) => v.get(index).map(|x| x.to_string()),
        _ => None,
    };
    value.ok_or_else(|| crate::value::error::throw_range_error("Atomics index is out of range"))
}

fn bigint_result(builtin: Builtin, args: &[Value], old: &str) -> Result<String, VmError> {
    let old = old
        .parse::<num_bigint::BigInt>()
        .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt"))?;
    let value = bigint_argument(args.get(2))?;
    let result = if builtin == Builtin::AtomicsCompareExchange {
        if old == value {
            bigint_argument(args.get(3))?
        } else {
            old
        }
    } else {
        match builtin {
            Builtin::AtomicsAdd => old + value,
            Builtin::AtomicsAnd => old & value,
            Builtin::AtomicsOr => old | value,
            Builtin::AtomicsSub => old - value,
            Builtin::AtomicsXor => old ^ value,
            _ => return Err(crate::vm::not_callable()),
        }
    };
    Ok(result.to_string())
}

fn bigint_write(view: &Value, index: usize, value: &str, unchanged: bool) -> Result<(), VmError> {
    if unchanged {
        return Ok(());
    }
    match view {
        Value::BigInt64Array(v) => v.set(
            index,
            value
                .parse::<i64>()
                .map_err(|_| crate::value::error::throw_type_error("BigInt64 out of range"))?,
        ),
        Value::BigUint64Array(v) => v.set(
            index,
            value
                .parse::<u64>()
                .map_err(|_| crate::value::error::throw_type_error("BigUint64 out of range"))?,
        ),
        _ => false,
    };
    Ok(())
}

fn bigint_argument(value: Option<&Value>) -> Result<num_bigint::BigInt, VmError> {
    let Some(Value::BigInt(value)) = value else {
        return Err(crate::value::error::throw_type_error(
            "Atomics requires BigInt values",
        ));
    };
    value
        .parse()
        .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt"))
}

enum AtomicView<'a> {
    Int8(&'a crate::value::Int8ArrayData),
    Int16(&'a crate::value::Int16ArrayData),
    Int32(&'a crate::value::Int32ArrayData),
    Uint8(&'a crate::value::Uint8ArrayData),
    Uint16(&'a crate::value::Uint16ArrayData),
    Uint32(&'a crate::value::Uint32ArrayData),
}

impl AtomicView<'_> {
    fn immutable(&self) -> bool {
        match self {
            Self::Int8(v) => v.buffer.immutable,
            Self::Int16(v) => v.buffer.immutable,
            Self::Int32(v) => v.buffer.immutable,
            Self::Uint8(v) => v.buffer.immutable,
            Self::Uint16(v) => v.buffer.immutable,
            Self::Uint32(v) => v.buffer.immutable,
        }
    }
    fn get(&self, index: usize) -> Option<i32> {
        match self {
            Self::Int8(v) => v.get(index).map(i32::from),
            Self::Int16(v) => v.get(index).map(i32::from),
            Self::Int32(v) => v.get(index),
            Self::Uint8(v) => v.get(index).map(i32::from),
            Self::Uint16(v) => v.get(index).map(i32::from),
            Self::Uint32(v) => v.get(index).map(|x| x as i32),
        }
    }

    fn get_number(&self, index: usize) -> Option<f64> {
        match self {
            Self::Uint32(v) => v.get(index).map(|value| value as f64),
            _ => self.get(index).map(|value| value as f64),
        }
    }
    fn set(&self, index: usize, value: i32) -> bool {
        match self {
            Self::Int8(v) => v.set(index, value as i8),
            Self::Int16(v) => v.set(index, value as i16),
            Self::Int32(v) => v.set(index, value),
            Self::Uint8(v) => v.set(index, value as u8),
            Self::Uint16(v) => v.set(index, value as u16),
            Self::Uint32(v) => v.set(index, value as u32),
        }
    }
}

fn atomic_view(value: Option<&Value>) -> Option<AtomicView<'_>> {
    match value? {
        Value::Int8Array(v) => Some(AtomicView::Int8(v)),
        Value::Int16Array(v) => Some(AtomicView::Int16(v)),
        Value::Int32Array(v) => Some(AtomicView::Int32(v)),
        Value::Uint8Array(v) => Some(AtomicView::Uint8(v)),
        Value::Uint16Array(v) => Some(AtomicView::Uint16(v)),
        Value::Uint32Array(v) => Some(AtomicView::Uint32(v)),
        _ => None,
    }
}

fn bigint_view_immutable(view: &Value) -> bool {
    match view {
        Value::BigInt64Array(v) => v.buffer.immutable,
        Value::BigUint64Array(v) => v.buffer.immutable,
        _ => false,
    }
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
