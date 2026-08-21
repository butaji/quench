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
        crate::ops::Builtin::Iterator => Ok(crate::value::Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![(
                "\0prototype".to_string(),
                crate::value::Value::Builtin(crate::ops::Builtin::IteratorPrototype),
            )]),
        ))),
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
            | Builtin::IntlDurationFormat
            | Builtin::IntlLocale
    )
}

fn construct_regexp(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    // RegExp(pattern) returns the pattern when it is RegExp-like and its
    // constructor is this intrinsic, before attempting to read/compile flags.
    if let Some(pattern) = arguments.first() {
        let flags_omitted = arguments
            .get(1)
            .is_none_or(|value| matches!(value, Value::Undefined));
        if flags_omitted && is_regexp_pattern(pattern)? {
            let constructor = crate::execute::get_property_result(pattern, "constructor")?;
            if matches!(constructor, Value::Builtin(crate::ops::Builtin::RegExp)) {
                return Ok(pattern.clone());
            }
        }
    }
    let (source, observable_source, flags) = regexp_source_and_flags(arguments)?;
    let visible_flags = crate::regexp::canonical_flags(&flags);
    crate::regexp::compile(&source, &flags)
        .map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    let mut entries = regexp_entries(&source, &observable_source, &flags, &visible_flags);
    entries.extend(regexp_flag_entries(&flags));
    Ok(Value::Object(Rc::new(ObjectData::new(entries))))
}

fn regexp_entries(
    source: &str,
    observable_source: &Value,
    flags: &str,
    visible_flags: &str,
) -> Vec<(String, Value)> {
    let last_index = Value::BindingCell(Rc::new(RefCell::new(Value::Number(0.0))));
    vec![
        (
            "\0realm".to_string(),
            Value::Number(crate::vm::current_context_or_default().realm().get() as f64),
        ),
        ("\0regexp".to_string(), Value::Boolean(true)),
        (
            "\0regexp_source".to_string(),
            Value::String(source.to_string()),
        ),
        (
            "\0regexp_flags".to_string(),
            Value::String(flags.to_string()),
        ),
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
            regexp_data_descriptor(false, true, observable_source.clone()),
        ),
        (
            "flags".to_string(),
            Value::BindingCell(Rc::new(RefCell::new(Value::String(
                visible_flags.to_string(),
            )))),
        ),
        (
            crate::builtins::descriptor_key("flags"),
            regexp_data_descriptor(false, true, Value::String(visible_flags.to_string())),
        ),
        ("lastIndex".to_string(), last_index),
        (
            crate::builtins::descriptor_key("lastIndex"),
            regexp_data_descriptor(true, false, Value::Number(0.0)),
        ),
    ]
}

fn regexp_source_and_flags(
    arguments: &[Value],
) -> Result<(String, Value, String), crate::execute::VmError> {
    let source_value = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let source_value = regexp_constructor_source(&source_value)?;
    let flags = regexp_constructor_flags(arguments)?;
    let source = crate::strings::source_text(&source_value)
        .map(Ok)
        .unwrap_or_else(|| crate::conversion::to_string(&source_value))?;
    let observable_source = crate::strings::source_value(&source);
    Ok((source, observable_source, flags))
}

fn regexp_constructor_source(value: &Value) -> Result<Value, crate::execute::VmError> {
    if is_regexp_pattern(value)? {
        return crate::execute::get_property_result(value, "source");
    }
    Ok(value.clone())
}

fn regexp_constructor_flags(arguments: &[Value]) -> Result<String, crate::execute::VmError> {
    let inherits_flags = arguments
        .get(1)
        .map_or(true, |flags| matches!(flags, Value::Undefined));
    if let Some(pattern) = arguments.first() {
        if inherits_flags && is_regexp_pattern(pattern)? {
            return crate::execute::get_property_result(pattern, "flags")
                .and_then(|value| crate::conversion::to_string(&value));
        }
    }
    arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)
}

fn is_regexp_pattern(value: &Value) -> Result<bool, crate::execute::VmError> {
    if crate::regexp::has_regexp_internal_slot(value) {
        return Ok(true);
    }
    if !crate::value::is_object(value) {
        return Ok(false);
    }
    Ok(crate::execute::is_truthy(
        &crate::execute::get_property_result(value, "Symbol.match")?,
    ))
}

fn regexp_data_descriptor(writable: bool, configurable: bool, value: Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ])))
}

include!("construct_builtins_tail.rs");
