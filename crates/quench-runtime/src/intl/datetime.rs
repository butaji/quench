//! `Intl.DateTimeFormat`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut properties = vec![
        (
            "format".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatFormat),
        ),
        (
            "formatToParts".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatFormatToParts),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("calendar".to_string(), Value::String("gregory".to_string())),
                (
                    "numberingSystem".to_string(),
                    Value::String("latn".to_string()),
                ),
                ("timeZone".to_string(), Value::String("UTC".to_string())),
                ("year".to_string(), Value::String("numeric".to_string())),
                ("month".to_string(), Value::String("numeric".to_string())),
                ("day".to_string(), Value::String("numeric".to_string())),
            ]),
        ),
    ];
    let _ = &mut properties;
    Ok(make_object(properties))
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormatFormat => {
            let value = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            Ok(Value::String(format_date(&value, &locale)))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatToParts => {
            let value = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            Ok(make_array(vec![make_object(vec![
                ("type".to_string(), Value::String("literal".to_string())),
                (
                    "value".to_string(),
                    Value::String(format_date(&value, &locale)),
                ),
            ])]))
        }
        crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("calendar".to_string(), Value::String("gregory".to_string())),
            (
                "numberingSystem".to_string(),
                Value::String("latn".to_string()),
            ),
            ("timeZone".to_string(), Value::String("UTC".to_string())),
            ("year".to_string(), Value::String("numeric".to_string())),
            ("month".to_string(), Value::String("numeric".to_string())),
            ("day".to_string(), Value::String("numeric".to_string())),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn format_date(value: &str, locale: &str) -> String {
    let _ = locale;
    if value.is_empty() {
        return value.to_string();
    }
    value.to_string()
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlDateTimeFormatFormat
        | crate::ops::Builtin::IntlDateTimeFormatFormatToParts
        | crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
