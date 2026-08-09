//! `Intl.DisplayNames`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_string, to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut display_type = "language".to_string();
    let mut style = "long".to_string();
    if let Some(Value::Object(properties)) = arguments.get(1) {
        for (key, value) in properties.iter() {
            match key.as_str() {
                "type" => display_type = to_string_value(value),
                "style" => style = to_string_value(value),
                _ => {}
            }
        }
    }
    Ok(make_object(vec![
        (
            "of".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlDisplayNamesOf),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlDisplayNamesResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("type".to_string(), Value::String(display_type)),
                ("style".to_string(), Value::String(style)),
            ]),
        ),
    ]))
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    let display_type = slot_string(&slots, "type").unwrap_or_else(|| "language".to_string());
    match builtin {
        crate::ops::Builtin::IntlDisplayNamesOf => {
            let code = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            Ok(display_name(&code, &display_type, &locale))
        }
        crate::ops::Builtin::IntlDisplayNamesResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("style".to_string(), Value::String("long".to_string())),
            ("type".to_string(), Value::String(display_type)),
            ("fallback".to_string(), Value::String("code".to_string())),
            (
                "languageDisplay".to_string(),
                Value::String("dialect".to_string()),
            ),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn display_name(code: &str, display_type: &str, locale: &str) -> Value {
    let _ = locale;
    match display_type {
        "language" => language_name(code),
        "region" => Value::String(code.to_string()),
        "currency" => Value::String(code.to_string()),
        "script" => Value::String(code.to_string()),
        "calendar" => Value::String(code.to_string()),
        "dateTimeField" => Value::String(code.to_string()),
        _ => Value::String(code.to_string()),
    }
}

fn language_name(code: &str) -> Value {
    let name = match code {
        "en" => "English",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "zh" => "Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "ru" => "Russian",
        "ar" => "Arabic",
        "it" => "Italian",
        _ => return Value::String(code.to_string()),
    };
    Value::String(name.to_string())
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlDisplayNames => Some(construct(arguments)),
        crate::ops::Builtin::IntlDisplayNamesOf
        | crate::ops::Builtin::IntlDisplayNamesResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
