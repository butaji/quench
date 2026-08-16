//! `Intl.DisplayNames`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_string, to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let options = match arguments.get(1) {
        Some(options @ Value::Object(_)) => options,
        _ => return Err(runtime_error("TypeError: options.type is required")),
    };
    let options = parse_options(options)?;
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
                ("type".to_string(), Value::String(options.display_type)),
                ("style".to_string(), Value::String(options.style)),
                ("fallback".to_string(), Value::String(options.fallback)),
                (
                    "languageDisplay".to_string(),
                    Value::String(options.language_display),
                ),
            ]),
        ),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlDisplayNamesPrototype),
        ),
    ]))
}

struct DisplayNamesOptions {
    display_type: String,
    style: String,
    fallback: String,
    language_display: String,
}

fn parse_options(options: &Value) -> Result<DisplayNamesOptions, VmError> {
    let locale_matcher = option_string(options, "localeMatcher", "")?;
    validate_value(&locale_matcher, &["lookup", "best fit"], "localeMatcher")?;
    let style = option_string(options, "style", "")?;
    validate_value(&style, &["long", "short", "narrow"], "style")?;
    let display_type = option_string(options, "type", "TypeError: options.type is required")?;
    validate_type(&display_type)?;
    let fallback = option_string(options, "fallback", "")?;
    validate_value(&fallback, &["code", "none"], "fallback")?;
    let language_display = option_string(options, "languageDisplay", "")?;
    validate_value(
        &language_display,
        &["dialect", "standard"],
        "languageDisplay",
    )?;
    Ok(DisplayNamesOptions {
        display_type,
        style,
        fallback,
        language_display,
    })
}

fn validate_type(value: &str) -> Result<(), VmError> {
    validate_value(
        value,
        &[
            "language",
            "region",
            "currency",
            "script",
            "calendar",
            "dateTimeField",
        ],
        "type",
    )
}

fn validate_value(value: &str, allowed: &[&str], name: &str) -> Result<(), VmError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(runtime_error(&format!("RangeError: invalid {name}")))
    }
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
        "region" => region_code_valid(code),
        "script" => fixed_alpha_code_valid(code, 4),
        "currency" => fixed_alpha_code_valid(code, 3),
        "calendar" => calendar_code_valid(code),
        "dateTimeField" => date_time_field_valid(code),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(runtime_error("RangeError: invalid code"))
    }
}

fn region_code_valid(code: &str) -> bool {
    (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
        || (code.len() == 3 && code.chars().all(|c| c.is_ascii_digit()))
}

fn fixed_alpha_code_valid(code: &str, length: usize) -> bool {
    code.len() == length && code.chars().all(|c| c.is_ascii_alphabetic())
}

fn calendar_code_valid(code: &str) -> bool {
    code.split('-').all(|part| {
        (3..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

fn date_time_field_valid(code: &str) -> bool {
    matches!(
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
            | "timeZoneName"
    )
}

fn language_code_valid(code: &str) -> bool {
    let mut parts = code.split('-');
    let language = parts.next().unwrap_or("");
    if !((2..=3).contains(&language.len()) || (5..=8).contains(&language.len()))
        || !language.chars().all(|c| c.is_ascii_alphabetic())
    {
        return false;
    }
    let mut script_seen = false;
    let mut region_seen = false;
    let mut variants = Vec::new();
    for part in parts {
        let alpha = part.chars().all(|c| c.is_ascii_alphabetic());
        let digit = part.chars().all(|c| c.is_ascii_digit());
        if part.len() == 1 || !(2..=8).contains(&part.len()) {
            return false;
        }
        if part.len() == 4 && alpha {
            if script_seen {
                return false;
            }
            script_seen = true;
        } else if (part.len() == 2 && alpha) || (part.len() == 3 && digit) {
            if region_seen {
                return false;
            }
            region_seen = true;
        } else if (5..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
            || part.len() == 4
                && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                && part[1..].chars().all(|c| c.is_ascii_alphanumeric())
        {
            if variants.iter().any(|variant| variant == &part) {
                return false;
            }
            variants.push(part);
        } else {
            return false;
        }
    }
    true
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    let slots = super::intl_slots(receiver)?;
    if slot_string(&slots, "type").is_none() {
        return Err(runtime_error("TypeError: not a DisplayNames object"));
    }
    Ok(slots)
}

fn option_string(options: &Value, name: &str, missing: &str) -> Result<String, VmError> {
    let value = crate::execute::get_property_result(options, name)?;
    match value {
        Value::Undefined if name == "localeMatcher" => Ok("best fit".to_string()),
        Value::Undefined if name == "style" => Ok("long".to_string()),
        Value::Undefined if name == "fallback" => Ok("code".to_string()),
        Value::Undefined if name == "languageDisplay" => Ok("dialect".to_string()),
        Value::Undefined => Err(runtime_error(missing)),
        value => Ok(crate::conversion::to_string(&value)?),
    }
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
