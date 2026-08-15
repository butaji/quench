fn construct_weak_ref(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(target) = arguments
        .first()
        .filter(|value| {
            crate::value::is_object(value)
                || matches!(value, crate::value::Value::String(text) if crate::conversion::is_symbol(value) && !text.starts_with("Symbol.for."))
        })
    else {
        return Err(crate::value::error::throw_type_error(
            "WeakRef target must be an object",
        ));
    };
    Ok(Value::Object(Rc::new(ObjectData::new(vec![
        ("\0weakref".to_string(), target.clone()),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::WeakRefPrototype),
        ),
    ]))))
}
fn construct_shared_array_buffer(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Value::ArrayBuffer(mut buffer) = construct_array_buffer(arguments)? else {
        return Err(crate::vm::not_callable());
    };
    Rc::make_mut(&mut buffer).shared = true;
    Ok(Value::ArrayBuffer(buffer))
}

fn construct_typed_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    use crate::ops::Builtin::*;
    Some(match builtin {
        Float64Array => construct_float64_array(arguments),
        Float32Array => construct_float32_array(arguments),
        Int8Array => construct_int8_array(arguments),
        Int16Array => construct_int16_array(arguments),
        Int32Array => construct_int32_array(arguments),
        Uint8Array => construct_uint8_array(arguments),
        Uint16Array => construct_uint16_array(arguments),
        Uint32Array => construct_uint32_array(arguments),
        Uint8ClampedArray => construct_uint8_clamped_array(arguments),
        BigInt64Array => construct_bigint64_array(arguments),
        BigUint64Array => construct_biguint64_array(arguments),
        _ => return None,
    })
}

fn is_intl_constructor(builtin: crate::ops::Builtin) -> bool {
    use crate::ops::Builtin;
    matches!(
        builtin,
        Builtin::IntlNumberFormat
            | Builtin::IntlDateTimeFormat
            | Builtin::IntlCollator
            | Builtin::IntlPluralRules
            | Builtin::IntlListFormat
            | Builtin::IntlRelativeTimeFormat
            | Builtin::IntlSegmenter
            | Builtin::IntlDisplayNames
            | Builtin::IntlLocale
    )
}

fn construct_regexp(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let source = arguments
        .first()
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    let flags = arguments
        .get(1)
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    let last_index = Value::BindingCell(Rc::new(RefCell::new(Value::Number(0.0))));
    let mut entries = vec![
        ("\0regexp".to_string(), Value::Boolean(true)),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::RegExpPrototype),
        ),
        ("source".to_string(), Value::String(source.clone())),
        (
            crate::builtins::descriptor_key("source"),
            regexp_data_descriptor(false, true, Value::String(source)),
        ),
        ("flags".to_string(), Value::String(flags.clone())),
        (
            crate::builtins::descriptor_key("flags"),
            regexp_data_descriptor(false, true, Value::String(flags.clone())),
        ),
        ("lastIndex".to_string(), last_index),
        (
            crate::builtins::descriptor_key("lastIndex"),
            regexp_data_descriptor(true, false, Value::Number(0.0)),
        ),
    ];
    entries.extend(regexp_flag_entries(&flags));
    Ok(Value::Object(Rc::new(ObjectData::new(entries))))
}

fn regexp_data_descriptor(writable: bool, configurable: bool, value: Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ])))
}

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
    let length = arguments.first().map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let length = to_index(length)?;
    let buffer = match arguments.get(1) {
        Some(options) => resizable_array_buffer(length, options)?,
        None => crate::value::ArrayBufferData::new(length),
    };
    Ok(Value::ArrayBuffer(Rc::new(buffer)))
}

fn resizable_array_buffer(
    length: usize,
    options: &Value,
) -> Result<crate::value::ArrayBufferData, crate::execute::VmError> {
    let maximum = crate::execute::get_property(options, "maxByteLength");
    let maximum = to_index(crate::intl::tolocale::value::to_number(Some(&maximum)))?;
    if maximum < length {
        return Err(range_error("maxByteLength is smaller than byteLength"));
    }
    Ok(crate::value::ArrayBufferData::new_resizable(
        length, maximum,
    ))
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
    let mut properties = vec![
        ("_value".to_string(), value),
        ("constructor".to_string(), Value::Builtin(constructor)),
    ];
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    Value::Object(std::rc::Rc::new(ObjectData::new(properties)))
}

fn construct_error(
    builtin: &crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if *builtin == crate::ops::Builtin::SuppressedError {
        return crate::builtins::suppressed_error(arguments);
    }

    let name = match builtin {
        crate::ops::Builtin::RangeError => "RangeError",
        crate::ops::Builtin::ReferenceError => "ReferenceError",
        crate::ops::Builtin::SyntaxError => "SyntaxError",
        crate::ops::Builtin::EvalError => "EvalError",
        crate::ops::Builtin::URIError => "URIError",
        crate::ops::Builtin::AggregateError => "AggregateError",
        crate::ops::Builtin::TypeError => "TypeError",
        _ => "Error",
    };

    let mut properties = vec![
        ("name".to_string(), Value::String(name.to_string())),
        (
            crate::builtins::ERROR_SLOT.to_string(),
            Value::Boolean(true),
        ),
        (
            "\0prototype".to_string(),
            Value::Builtin(
                crate::builtin_meta::instance_prototype(*builtin)
                    .unwrap_or(crate::ops::Builtin::ErrorPrototype),
            ),
        ),
    ];
    if let Some(message) = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
    {
        properties.push((
            "message".to_string(),
            Value::String(crate::conversion::to_string(message)?),
        ));
    }

    if let Some(cause_source) = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
    {
        let options = to_object(cause_source)?;
        if crate::with_scope::has_property(&options, "cause")? {
            let cause = crate::execute::get_property_result(&options, "cause")?;
            properties.push(("cause".to_string(), cause));
        }
    }

    for key in ["name", "message", "cause"] {
        if let Some((_, value)) = properties.iter().rev().find(|(current, _)| current == key) {
            properties.push((
                crate::builtins::descriptor_key(key),
                non_enumerable_descriptor(value),
            ));
        }
    }

    Ok(Value::Object(std::rc::Rc::new(ObjectData::new(properties))))
}

fn non_enumerable_descriptor(value: &Value) -> Value {
    Value::Object(std::rc::Rc::new(ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}

fn to_object(value: &Value) -> Result<Value, crate::execute::VmError> {
    match value {
        Value::Object(_)
        | Value::Array(_)
        | Value::ObjectAlias(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Builtin(_)
        | Value::Proxy(_)
        | Value::Promise(_)
        | Value::Map(_)
        | Value::Set(_)
        | Value::ArrayBuffer(_)
        | Value::DataView(_)
        | Value::Float32Array(_)
        | Value::Float64Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_)
        | Value::Uint32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Iterator(_)
        | Value::Generator(_)
        | Value::HostCapability(_) => Ok(value.clone()),

        Value::Number(value) => Ok(boxed_primitive(
            Value::Number(*value),
            crate::ops::Builtin::Number,
        )),
        Value::Boolean(value) => Ok(boxed_primitive(
            Value::Boolean(*value),
            crate::ops::Builtin::Boolean,
        )),
        Value::String(value) => Ok(boxed_primitive(
            Value::String(value.clone()),
            crate::ops::Builtin::String,
        )),
        Value::StringUnits(value) => Ok(boxed_primitive(
            Value::StringUnits(value.clone()),
            crate::ops::Builtin::String,
        )),
        Value::BigInt(value) => Ok(boxed_primitive(
            Value::BigInt(value.clone()),
            crate::ops::Builtin::BigInt,
        )),
        Value::BindingCell(_) | Value::Undefined | Value::Null => Err(
            crate::value::error::throw_type_error("Cannot convert undefined or null to object"),
        ),
    }
}

fn construct_function(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if !crate::functions::is_constructible(function) {
        return Err(crate::vm::not_callable());
    }
    if is_default_derived_constructor(function) {
        let super_constructor = derived_constructor(function)?;
        return construct_with_new_target(&super_constructor, target, arguments);
    }
    if derived_constructor(function).is_ok() {
        let _context = crate::super_scope::Guard::install(function, &Value::Undefined);
        let (result, final_this) =
            crate::functions::execute_construct(function, &Value::Undefined, target, arguments)?;
        if crate::value::is_object(&result) {
            return Ok(result);
        }
        if crate::value::is_object(&final_this) {
            return Ok(final_this);
        }
        return Err(crate::value::error::throw_reference_error(
            "Derived constructor did not initialize this",
        ));
    }
    let object = initialize_instance_fields(function, constructor_receiver(target))?;
    let (result, final_this) =
        crate::functions::execute_construct(function, &object, target, arguments)?;
    if crate::value::is_object(&result) {
        Ok(result)
    } else if crate::value::is_object(&final_this) {
        Ok(final_this)
    } else {
        Ok(object)
    }
}

pub(crate) fn initialize_instance_fields(
    function: &crate::value::FunctionValue,
    receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    initialize_instance_fields_impl(function, receiver)
}
include!("construct_instance_fields.rs");
pub(crate) fn derived_constructor(
    function: &crate::value::FunctionValue,
) -> Result<Value, crate::execute::VmError> {
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
        .ok_or_else(|| crate::value::error::throw_reference_error("super is unavailable"))
}

include!("construct_tail.rs");
