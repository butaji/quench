//! `Intl.DisplayNames`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_string, to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let properties = match arguments.get(1) {
        Some(Value::Object(properties)) => properties,
        _ => return Err(runtime_error("TypeError: options.type is required")),
    };
    let display_type = option_string(properties, "type", "TypeError: options.type is required")?;
    if !matches!(
        display_type.as_str(),
        "language" | "region" | "currency" | "script" | "calendar" | "dateTimeField"
    ) {
        return Err(runtime_error("RangeError: invalid type"));
    }
    let style = option_string(properties, "style", "")?;
    if !matches!(style.as_str(), "long" | "short" | "narrow") {
        return Err(runtime_error("RangeError: invalid style"));
    }
    let fallback = option_string(properties, "fallback", "")?;
    if !matches!(fallback.as_str(), "code" | "none") {
        return Err(runtime_error("RangeError: invalid fallback"));
    }
    let language_display = option_string(properties, "languageDisplay", "")?;
    if !matches!(language_display.as_str(), "dialect" | "standard") {
        return Err(runtime_error("RangeError: invalid languageDisplay"));
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
                ("fallback".to_string(), Value::String(fallback)),
                (
                    "languageDisplay".to_string(),
                    Value::String(language_display),
                ),
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
    let style = slot_string(&slots, "style").unwrap_or_else(|| "long".to_string());
    let fallback = slot_string(&slots, "fallback").unwrap_or_else(|| "code".to_string());
    let language_display =
        slot_string(&slots, "languageDisplay").unwrap_or_else(|| "dialect".to_string());
    match builtin {
        crate::ops::Builtin::IntlDisplayNamesOf => {
            let code = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            Ok(display_name(
                &code,
                &display_type,
                &locale,
                &style,
                &fallback,
            ))
        }
        crate::ops::Builtin::IntlDisplayNamesResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("style".to_string(), Value::String(style)),
            ("type".to_string(), Value::String(display_type)),
            ("fallback".to_string(), Value::String(fallback)),
            (
                "languageDisplay".to_string(),
                Value::String(language_display),
            ),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn option_string(
    properties: &[(String, Value)],
    name: &str,
    missing: &str,
) -> Result<String, VmError> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            Value::Undefined if name == "style" => Some("long".to_string()),
            Value::Undefined if name == "fallback" => Some("code".to_string()),
            Value::Undefined if name == "languageDisplay" => Some("dialect".to_string()),
            Value::Undefined => None,
            value => Some(to_string_value(value)),
        })
        .or_else(|| (name == "style").then(|| "long".to_string()))
        .or_else(|| (name == "fallback").then(|| "code".to_string()))
        .or_else(|| (name == "languageDisplay").then(|| "dialect".to_string()))
        .ok_or_else(|| runtime_error(missing))
}

fn display_name(
    code: &str,
    display_type: &str,
    locale: &str,
    style: &str,
    fallback: &str,
) -> Value {
    let _ = locale;
    let _ = style;
    match display_type {
        "language" => language_name(code, fallback),
        "region" => Value::String(code.to_string()),
        "currency" => Value::String(code.to_string()),
        "script" => Value::String(code.to_string()),
        "calendar" => Value::String(code.to_string()),
        "dateTimeField" => Value::String(code.to_string()),
        _ => Value::String(code.to_string()),
    }
}

fn language_name(code: &str, fallback: &str) -> Value {
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
        _ if fallback == "none" => return Value::Undefined,
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
