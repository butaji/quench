fn construct_builtin_match(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::Array => Ok(crate::builtins::array(arguments)),
        crate::ops::Builtin::ArrayBuffer => construct_array_buffer(arguments),
        crate::ops::Builtin::SharedArrayBuffer => construct_shared_array_buffer(arguments),
        crate::ops::Builtin::DataView => construct_data_view(arguments),
        crate::ops::Builtin::Object => Ok(crate::builtins::object(arguments)),
        crate::ops::Builtin::Number => construct_number(arguments),
        crate::ops::Builtin::Boolean => construct_boolean(arguments),
        crate::ops::Builtin::String => construct_string(arguments),
        crate::ops::Builtin::Promise => construct_promise(arguments),
        crate::ops::Builtin::Proxy => crate::proxy::proxy_new(arguments),
        crate::ops::Builtin::Map => crate::collections::map::map_new(arguments),
        crate::ops::Builtin::Set => crate::collections::set::set_new(arguments),
        crate::ops::Builtin::WeakMap => crate::collections::map::weak_map_new(arguments),
        crate::ops::Builtin::WeakSet => crate::collections::set::weak_set_new(arguments),
        crate::ops::Builtin::WeakRef => construct_weak_ref(arguments),
        crate::ops::Builtin::Date => crate::date::execute(builtin, None, arguments)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        crate::ops::Builtin::DisposableStack => crate::disposable_stack::construct(),
        crate::ops::Builtin::AsyncDisposableStack => crate::disposable_stack::construct_async(),
        crate::ops::Builtin::FinalizationRegistry => {
            crate::finalization_registry::construct(arguments)
        }
        crate::ops::Builtin::RegExp => construct_regexp(arguments),
        _ => construct_builtin_tail(builtin, arguments),
    }
}

fn construct_builtin_tail(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::AbstractModuleSource => Err(crate::value::error::throw_type_error(
            "AbstractModuleSource cannot be constructed",
        )),
        crate::ops::Builtin::TemporalDuration => crate::temporal::duration::construct(arguments),
        crate::ops::Builtin::TemporalPlainDate => crate::temporal::plain_date::construct(arguments),
        crate::ops::Builtin::ShadowRealm => {
            let realm = crate::vm::create_shadow_realm_value();
            Ok(crate::builtins::set_property(
                realm,
                "\0prototype",
                Value::Builtin(crate::ops::Builtin::ShadowRealmPrototype),
            ))
        }
        _ if is_intl_constructor(builtin) => crate::intl::execute(builtin, arguments, None)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        _ => Err(crate::vm::not_callable()),
    }
}

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
    let (source, observable_source, flags) = regexp_source_and_flags(arguments)?;
    crate::regexp::compile(&source, &flags)
        .map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    let last_index = Value::BindingCell(Rc::new(RefCell::new(Value::Number(0.0))));
    let mut entries = vec![
        ("\0regexp".to_string(), Value::Boolean(true)),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::RegExpPrototype),
        ),
        (
            "source".to_string(),
            Value::BindingCell(Rc::new(RefCell::new(observable_source.clone()))),
        ),
        (
            crate::builtins::descriptor_key("source"),
            regexp_data_descriptor(false, true, observable_source),
        ),
        (
            "flags".to_string(),
            Value::BindingCell(Rc::new(RefCell::new(Value::String(flags.clone())))),
        ),
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

fn regexp_source_and_flags(
    arguments: &[Value],
) -> Result<(String, Value, String), crate::execute::VmError> {
    let source_value = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let flags = arguments
        .get(1)
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    let source = crate::strings::source_text(&source_value)
        .or_else(|| crate::conversion::to_string(&source_value).ok())
        .unwrap_or_default();
    let observable_source = crate::strings::source_value(&source);
    Ok((source, observable_source, flags))
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
    let mut properties = vec![("_value".to_string(), value)];
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
    let name = error_name(builtin);
    let constructor = crate::vm::realm_intrinsic(*builtin);
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
    if *builtin != crate::ops::Builtin::AggregateError {
        properties.insert(0, ("constructor".to_string(), constructor));
        properties.insert(0, ("name".to_string(), Value::String(name.to_string())));
    }
    if let Some(message) = message {
        push_error_property(&mut properties, "message", Value::String(message));
    }
    properties
}

fn error_name(builtin: &crate::ops::Builtin) -> &'static str {
    match builtin {
        crate::ops::Builtin::RangeError => "RangeError",
        crate::ops::Builtin::ReferenceError => "ReferenceError",
        crate::ops::Builtin::SyntaxError => "SyntaxError",
        crate::ops::Builtin::EvalError => "EvalError",
        crate::ops::Builtin::URIError => "URIError",
        crate::ops::Builtin::AggregateError => "AggregateError",
        crate::ops::Builtin::TypeError => "TypeError",
        _ => "Error",
    }
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

pub(crate) fn to_object(value: &Value) -> Result<Value, crate::execute::VmError> {
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
