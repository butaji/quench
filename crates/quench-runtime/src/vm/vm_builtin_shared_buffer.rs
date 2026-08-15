fn is_data_view_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::DataViewGetInt8
            | Builtin::DataViewGetUint8
            | Builtin::DataViewGetInt16
            | Builtin::DataViewGetUint16
            | Builtin::DataViewGetInt32
            | Builtin::DataViewGetUint32
            | Builtin::DataViewGetFloat16
            | Builtin::DataViewGetFloat32
            | Builtin::DataViewGetFloat64
            | Builtin::DataViewGetBigInt64
            | Builtin::DataViewGetBigUint64
            | Builtin::DataViewSetInt8
            | Builtin::DataViewSetUint8
            | Builtin::DataViewSetInt16
            | Builtin::DataViewSetUint16
            | Builtin::DataViewSetInt32
            | Builtin::DataViewSetUint32
            | Builtin::DataViewSetFloat16
            | Builtin::DataViewSetFloat32
            | Builtin::DataViewSetFloat64
            | Builtin::DataViewSetBigInt64
            | Builtin::DataViewSetBigUint64
            | Builtin::DataViewBufferGetter
            | Builtin::DataViewByteLengthGetter
            | Builtin::DataViewByteOffsetGetter
    )
}

pub(crate) fn execute_shared_array_buffer_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver.filter(|value| {
        matches!(value, Value::ArrayBuffer(data) if (data.shared == (builtin == Builtin::SharedArrayBufferSlice || builtin == Builtin::SharedArrayBufferGrow)) && !matches!(
            builtin,
            Builtin::ArrayBufferByteLengthGetter
                | Builtin::ArrayBufferDetachedGetter
                | Builtin::ArrayBufferImmutableGetter
                | Builtin::ArrayBufferMaxByteLengthGetter
                | Builtin::ArrayBufferResizableGetter
        )) || (builtin == Builtin::ArrayBufferByteLengthGetter
            && matches!(value, Value::ArrayBuffer(data) if !data.shared))
            || (builtin == Builtin::ArrayBufferDetachedGetter
                && matches!(value, Value::ArrayBuffer(data) if !data.shared))
            || (builtin == Builtin::ArrayBufferImmutableGetter
                && matches!(value, Value::ArrayBuffer(data) if !data.shared))
            || (builtin == Builtin::ArrayBufferMaxByteLengthGetter
                && matches!(value, Value::ArrayBuffer(data) if !data.shared))
            || (builtin == Builtin::ArrayBufferResizableGetter
                && matches!(value, Value::ArrayBuffer(data) if !data.shared))
    }) else {
        return Err(type_error(
            "SharedArrayBuffer method called on incompatible receiver",
        ));
    };
    match builtin {
        Builtin::ArrayBufferByteLengthGetter | Builtin::SharedArrayBufferByteLengthGetter => {
            Ok(Value::Number(buffer.byte_length() as f64))
        }
        Builtin::ArrayBufferDetachedGetter => Ok(Value::Boolean(*buffer.detached.borrow())),
        Builtin::ArrayBufferImmutableGetter => Ok(Value::Boolean(buffer.immutable)),
        Builtin::ArrayBufferMaxByteLengthGetter => Ok(Value::Number(
            buffer.max_byte_length.unwrap_or(buffer.byte_length()) as f64,
        )),
        Builtin::ArrayBufferResizableGetter => Ok(Value::Boolean(buffer.max_byte_length.is_some())),
        Builtin::SharedArrayBufferGrowableGetter => {
            Ok(Value::Boolean(buffer.max_byte_length.is_some()))
        }
        Builtin::SharedArrayBufferMaxByteLengthGetter => Ok(Value::Number(
            buffer.max_byte_length.unwrap_or(buffer.byte_length()) as f64,
        )),
        Builtin::SharedArrayBufferGrow => {
            if buffer.max_byte_length.is_none() {
                return Err(type_error("SharedArrayBuffer is not growable"));
            }
            let length = arguments
                .first()
                .ok_or_else(|| type_error("Missing length"))?;
            let length = crate::construct::to_index(crate::conversion::to_number(length)?)?;
            buffer
                .resize(length)
                .map_err(|_| crate::value::error::throw_range_error("Invalid grow length"))?;
            Ok(Value::Undefined)
        }
        Builtin::ArrayBufferSlice | Builtin::SharedArrayBufferSlice => {
            shared_array_buffer_slice(receiver, buffer, arguments)
        }
        _ => Err(type_error("Unknown SharedArrayBuffer builtin")),
    }
}

fn shared_array_buffer_slice(
    receiver: Option<&Value>,
    buffer: &crate::value::ArrayBufferData,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let length = buffer.byte_length() as i64;
    let start = slice_index(arguments.first(), length)?.unwrap_or(0);
    let end = slice_index(arguments.get(1), length)?
        .unwrap_or(length)
        .max(start);
    ensure_slice_source(buffer, end)?;
    let species = array_buffer_species(receiver)?;
    if !crate::conversion::is_callable(&species) {
        return Err(type_error(
            "SharedArrayBuffer species must be a constructor",
        ));
    }
    let result =
        crate::construct::construct_value(&species, &[Value::Number((end - start) as f64)])?;
    let Value::ArrayBuffer(result) = result else {
        return Err(type_error("SharedArrayBuffer species must return a buffer"));
    };
    if result.immutable {
        return Err(type_error(
            "ArrayBuffer species returned an immutable buffer",
        ));
    }
    ensure_slice_source(buffer, start)?;
    ensure_slice_source(buffer, end)?;
    let bytes = buffer.bytes.borrow()[start as usize..end as usize].to_vec();
    let same_buffer = matches!(receiver, Some(Value::ArrayBuffer(source)) if std::rc::Rc::ptr_eq(source, &result));
    if same_buffer || result.bytes.borrow().len() < bytes.len() {
        return Err(type_error(
            "SharedArrayBuffer species returned an invalid buffer",
        ));
    }
    let copy_length = result.bytes.borrow().len().min(bytes.len());
    result.bytes.borrow_mut()[..copy_length].copy_from_slice(&bytes[..copy_length]);
    Ok(Value::ArrayBuffer(result))
}

pub(crate) fn slice_to_immutable(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(type_error("sliceToImmutable requires an ArrayBuffer"));
    };
    if buffer.shared {
        return Err(type_error("sliceToImmutable requires an ArrayBuffer"));
    }
    if *buffer.detached.borrow() {
        return Err(type_error(
            "sliceToImmutable called on a detached ArrayBuffer",
        ));
    }
    let length = buffer.byte_length() as i64;
    let start = slice_index(arguments.first(), length)?.unwrap_or(0);
    let end = slice_index(arguments.get(1), length)?
        .unwrap_or(length)
        .max(start);
    ensure_slice_source(buffer, end)?;
    let bytes = buffer.bytes.borrow()[start as usize..end as usize].to_vec();
    let mut result = crate::value::ArrayBufferData::new(bytes.len());
    result.bytes.borrow_mut().copy_from_slice(&bytes);
    result.immutable = true;
    Ok(Value::ArrayBuffer(std::rc::Rc::new(result)))
}

fn ensure_slice_source(
    buffer: &crate::value::ArrayBufferData,
    required_length: i64,
) -> Result<(), VmError> {
    if *buffer.detached.borrow() {
        return Err(type_error("ArrayBuffer was detached during coercion"));
    }
    if buffer.byte_length() < required_length as usize {
        return Err(crate::value::error::throw_range_error(
            "ArrayBuffer was resized during coercion",
        ));
    }
    Ok(())
}

fn array_buffer_species(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("Missing SharedArrayBuffer receiver"))?;
    let constructor = crate::execute::get_property_result(receiver, "constructor")?;
    if matches!(constructor, Value::Null)
        || (!matches!(constructor, Value::Undefined) && !crate::value::is_object(&constructor))
    {
        return Err(type_error(
            "SharedArrayBuffer constructor must be an object",
        ));
    }
    let species = if matches!(constructor, Value::Undefined) {
        if receiver_is_shared(Some(receiver)) {
            Value::Builtin(Builtin::SharedArrayBuffer)
        } else {
            Value::Builtin(Builtin::ArrayBuffer)
        }
    } else {
        let value = crate::execute::get_property_result(&constructor, "Symbol.species")?;
        if matches!(value, Value::Undefined) {
            species_property(&constructor)
        } else {
            value
        }
    };
    Ok(match species {
        Value::Undefined | Value::Null if receiver_is_shared(Some(receiver)) => {
            Value::Builtin(Builtin::SharedArrayBuffer)
        }
        Value::Undefined | Value::Null => Value::Builtin(Builtin::ArrayBuffer),
        species => species,
    })
}

fn receiver_is_shared(receiver: Option<&Value>) -> bool {
    matches!(receiver, Some(Value::ArrayBuffer(buffer)) if buffer.shared)
}

fn species_property(constructor: &Value) -> Value {
    let Value::Object(properties) = constructor else {
        return Value::Undefined;
    };
    properties
        .iter()
        .rev()
        .find(|(key, _)| key.starts_with("Symbol.species\0"))
        .map_or(Value::Undefined, |(_, value)| value.clone())
}

fn slice_index(value: Option<&Value>, length: i64) -> Result<Option<i64>, VmError> {
    let Some(value) = value else { return Ok(None) };
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() {
        return Ok(Some(0));
    }
    let integer = number.trunc() as i64;
    Ok(Some(if integer < 0 {
        (length + integer).max(0)
    } else {
        integer.min(length)
    }))
}
fn is_number_receiver(receiver: Option<&Value>) -> bool {
    matches!(receiver, Some(Value::Builtin(Builtin::Number)))
}

fn is_nan_check(value: Option<&Value>, receiver: Option<&Value>) -> Result<bool, VmError> {
    if is_number_receiver(receiver) {
        return Ok(matches!(value, Some(Value::Number(number)) if number.is_nan()));
    }
    let value = value.cloned().unwrap_or(Value::Undefined);
    Ok(crate::conversion::to_number(&value)?.is_nan())
}

fn is_finite_check(value: Option<&Value>, receiver: Option<&Value>) -> Result<bool, VmError> {
    if is_number_receiver(receiver) {
        return Ok(is_finite(value));
    }
    let value = value.cloned().unwrap_or(Value::Undefined);
    Ok(crate::conversion::to_number(&value)?.is_finite())
}
