fn regexp_flag_entries(flags: &str) -> Vec<(String, Value)> {
    let mut entries = Vec::new();
    for (flag, enabled) in [
        ("global", flags.contains('g')),
        ("ignoreCase", flags.contains('i')),
        ("multiline", flags.contains('m')),
        ("dotAll", flags.contains('s')),
        ("unicode", flags.contains('u')),
        ("unicodeSets", flags.contains('v')),
        ("sticky", flags.contains('y')),
    ] {
        let descriptor = regexp_data_descriptor(false, true, Value::Boolean(enabled));
        entries.push((flag.to_string(), Value::Boolean(enabled)));
        entries.push((crate::builtins::descriptor_key(flag), descriptor));
    }
    entries
}

fn is_error_builtin(builtin: crate::ops::Builtin) -> bool {
    matches!(
        builtin,
        crate::ops::Builtin::TypeError
            | crate::ops::Builtin::Error
            | crate::ops::Builtin::RangeError
            | crate::ops::Builtin::ReferenceError
            | crate::ops::Builtin::SyntaxError
            | crate::ops::Builtin::EvalError
            | crate::ops::Builtin::URIError
            | crate::ops::Builtin::AggregateError
            | crate::ops::Builtin::SuppressedError
    )
}

fn construct_promise(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let executor = arguments.first().ok_or_else(crate::vm::not_callable)?;
    crate::promise::construct_promise(executor)
}

fn construct_array_buffer(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let length = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let length = to_index(length)?;
    let buffer = match arguments
        .get(1)
        .filter(|options| crate::value::is_object(options))
    {
        Some(options) => resizable_array_buffer(length, options)?,
        None => crate::value::ArrayBufferData::try_new(length)
            .ok_or_else(|| range_error("ArrayBuffer length is too large"))?,
    };
    Ok(Value::ArrayBuffer(Rc::new(buffer)))
}

fn resizable_array_buffer(
    length: usize,
    options: &Value,
) -> Result<crate::value::ArrayBufferData, crate::execute::VmError> {
    let maximum = crate::execute::get_property_result(options, "maxByteLength")?;
    if matches!(maximum, Value::Undefined) {
        return crate::value::ArrayBufferData::try_new(length)
            .ok_or_else(|| range_error("ArrayBuffer length is too large"));
    }
    let maximum = to_index(crate::conversion::to_number(&maximum)?)?;
    if maximum < length {
        return Err(range_error("maxByteLength is smaller than byteLength"));
    }
    if !allocation_possible(maximum) {
        return Err(range_error("ArrayBuffer maxByteLength is too large"));
    }
    crate::value::ArrayBufferData::try_new_resizable(length, maximum)
        .ok_or_else(|| range_error("ArrayBuffer length is too large"))
}

fn allocation_possible(length: usize) -> bool {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.try_reserve_exact(length).is_ok()
}

fn view_length(buffer: &crate::value::ArrayBufferData, fixed_length: usize) -> usize {
    if buffer.max_byte_length.is_some() {
        usize::MAX
    } else {
        fixed_length
    }
}

fn construct_data_view(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(Value::ArrayBuffer(buffer)) = arguments.first() else {
        return Err(type_error("DataView buffer must be an ArrayBuffer"));
    };
    let offset = match arguments.get(1) {
        Some(value) => crate::conversion::to_number(value)?,
        None => 0.0,
    };
    let offset = to_index(offset)?;
    if *buffer.detached.borrow() {
        return Err(type_error("Cannot use a detached ArrayBuffer"));
    }
    let buffer_length = buffer.byte_length();
    if offset > buffer_length {
        return Err(range_error("Invalid DataView byte offset"));
    }
    let available = buffer_length - offset;
    let length = match arguments.get(2) {
        Some(Value::Undefined) | None => view_length(buffer, available),
        Some(value) => {
            let length = to_index(crate::conversion::to_number(value)?)?;
            if *buffer.detached.borrow() {
                return Err(type_error("Cannot use a detached ArrayBuffer"));
            }
            length
        }
    };
    if length != usize::MAX && length > buffer_length - offset {
        return Err(range_error("Invalid DataView byte length"));
    }
    Ok(Value::DataView(Rc::new(crate::value::DataViewData::new(
        buffer.clone(),
        offset,
        length,
    ))))
}

include!("construct_typed_length.rs");
include!("construct_typed_low.rs");
include!("construct_typed_high.rs");
include!("construct_typed_bigint.rs");

fn range_error(message: &str) -> crate::execute::VmError {
    crate::execute::VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::RangeError,
        &[Value::String(message.to_string())],
    ))
}

fn construct_number(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::vm::explicit_number(arguments.first())?;
    Ok(boxed_primitive(
        Value::Number(value),
        crate::ops::Builtin::Number,
    ))
}

fn construct_boolean(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::execute::execute_builtin_with_receiver(
        crate::ops::Builtin::Boolean,
        arguments,
        None,
    )?;
    Ok(boxed_primitive(value, crate::ops::Builtin::Boolean))
}

fn construct_string(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::execute::execute_builtin_with_receiver(
        crate::ops::Builtin::String,
        arguments,
        None,
    )?;
    Ok(boxed_primitive(value, crate::ops::Builtin::String))
}

fn boxed_primitive(value: Value, constructor: crate::ops::Builtin) -> Value {
    let mut properties = vec![("_value".to_string(), value.clone())];
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    let boxed = Value::Object(std::rc::Rc::new(ObjectData::new(properties)));
    define_string_length(&boxed, &value, constructor).unwrap_or(boxed)
}

include!("construct_string_box.rs");

fn construct_error(
    builtin: &crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if *builtin == crate::ops::Builtin::SuppressedError {
        return crate::builtins::suppressed_error(arguments);
    }
    if *builtin == crate::ops::Builtin::AggregateError {
        return construct_aggregate_error(arguments);
    }

    let message = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
        .map(crate::conversion::to_string)
        .transpose()?;
    let mut properties = error_properties(builtin, message);
    append_error_cause(&mut properties, arguments)?;
    Ok(Value::Object(std::rc::Rc::new(ObjectData::new(properties))))
}

fn construct_aggregate_error(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let message = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
        .map(crate::conversion::to_string)
        .transpose()?;
    let errors = crate::collections::iterator::collect_iterable(
        arguments.first().cloned().unwrap_or(Value::Undefined),
    )?;
    let mut properties = error_properties(&crate::ops::Builtin::AggregateError, message);
    append_error_cause_value(&mut properties, arguments.get(2))?;
    push_error_property(&mut properties, "errors", Value::array(errors));
    Ok(Value::Object(std::rc::Rc::new(ObjectData::new(properties))))
}

fn error_properties(
    builtin: &crate::ops::Builtin,
    message: Option<String>,
) -> Vec<(String, Value)> {
    let prototype_builtin = crate::builtin_meta::instance_prototype(*builtin)
        .unwrap_or(crate::ops::Builtin::ErrorPrototype);
    let prototype = crate::vm::realm_intrinsic(prototype_builtin);
    let mut properties = vec![
        (
            crate::builtins::ERROR_SLOT.to_string(),
            Value::Boolean(true),
        ),
        ("\0prototype".to_string(), prototype),
    ];
    if let Some(message) = message {
        push_error_property(&mut properties, "message", Value::String(message));
    }
    properties
}

fn append_error_cause(
    properties: &mut Vec<(String, Value)>,
    arguments: &[Value],
) -> Result<(), crate::execute::VmError> {
    append_error_cause_value(properties, arguments.get(1))
}

fn append_error_cause_value(
    properties: &mut Vec<(String, Value)>,
    cause_source: Option<&Value>,
) -> Result<(), crate::execute::VmError> {
    if let Some(cause_source) = cause_source.filter(|value| !matches!(value, Value::Undefined)) {
        let options = to_object(cause_source)?;
        if !crate::with_scope::has_property(&options, "cause")? {
            return Ok(());
        }
        let cause = crate::execute::get_property_result(&options, "cause")?;
        push_error_property(properties, "cause", cause);
    }
    Ok(())
}

fn push_error_property(properties: &mut Vec<(String, Value)>, key: &str, value: Value) {
    properties.push((
        crate::builtins::descriptor_key(key),
        crate::builtins::non_enumerable_descriptor(&value),
    ));
    properties.push((key.to_string(), value));
}

include!("construct_to_object.rs");
