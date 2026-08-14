//! `Intl.DisplayNames`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_string, to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let options = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
        .ok_or_else(|| runtime_error("TypeError: options.type is required"))?;
    let matcher = option_string(options, "localeMatcher", "best fit")?;
    if !matches!(matcher.as_str(), "lookup" | "best fit") {
        return Err(runtime_error("RangeError: invalid localeMatcher"));
    }
    let style = option_string(options, "style", "long")?;
    let display_type = option_string(options, "type", "language")?;
    if !matches!(
        display_type.as_str(),
        "language" | "region" | "currency" | "script" | "calendar" | "dateTimeField"
    ) {
        return Err(runtime_error("RangeError: invalid type"));
    }
    if !matches!(style.as_str(), "long" | "short" | "narrow") {
        return Err(runtime_error("RangeError: invalid style"));
    }
    let fallback = option_string(options, "fallback", "code")?;
    if !matches!(fallback.as_str(), "code" | "none") {
        return Err(runtime_error("RangeError: invalid fallback"));
    }
    let language_display = option_string(options, "languageDisplay", "dialect")?;
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
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlDisplayNamesPrototype),
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
            validate_code(&code, &display_type)?;
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

fn validate_code(code: &str, display_type: &str) -> Result<(), VmError> {
    let valid = match display_type {
        "language" => language_code_valid(code),
        "region" => {
            (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
                || (code.len() == 3 && code.chars().all(|c| c.is_ascii_digit()))
        }
        "script" => code.len() == 4 && code.chars().all(|c| c.is_ascii_alphabetic()),
        "currency" => code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()),
        "calendar" => code.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
        }),
        "dateTimeField" => matches!(
            code,
            "era"
                | "year"
                | "quarter"
                | "month"
                | "weekOfYear"
                | "weekday"
                | "day"
                | "dayPeriod"
                | "hour"
                | "minute"
                | "second"
                | "fractionalSecond"
                | "timeZoneName"
        ),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(runtime_error("RangeError: invalid code"))
    }
}

fn language_code_valid(code: &str) -> bool {
    let mut parts = code.split('-');
    let language = parts.next().unwrap_or("");
    if language == "root"
        || !((2..=3).contains(&language.len()) || (5..=8).contains(&language.len()))
        || !language.chars().all(|c| c.is_ascii_alphabetic())
    {
        return false;
    }
    let mut script = false;
    let mut region = false;
    let mut variants = Vec::<String>::new();
    for part in parts {
        if !valid_language_part(part, &mut script, &mut region, &mut variants) {
            return false;
        }
    }
    true
}

fn valid_language_part(
    part: &str,
    script: &mut bool,
    region: &mut bool,
    variants: &mut Vec<String>,
) -> bool {
    if !(2..=8).contains(&part.len()) || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
        let fresh = !*script;
        *script = true;
        return fresh;
    }
    if (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
        || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
    {
        let fresh = !*region;
        *region = true;
        return fresh;
    }
    if part.len() <= 3 {
        return false;
    }
    if part.len() == 1 || variants.iter().any(|variant| variant == part) {
        return false;
    }
    variants.push(part.to_string());
    true
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn option_string(options: &Value, name: &str, default: &str) -> Result<String, VmError> {
    let value = crate::execute::get_property_result(options, name)?;
    if matches!(value, Value::Undefined) {
        return Ok(default.to_string());
    }
    crate::conversion::to_string(&value)
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
    let language = code.split('-').next().unwrap_or(code);
    let name = match language {
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
