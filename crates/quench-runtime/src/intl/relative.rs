//! `Intl.RelativeTimeFormat`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut style = "long".to_string();
    let mut numeric = "always".to_string();
    if let Some(Value::Object(properties)) = arguments.get(1) {
        for (key, value) in properties.iter() {
            match key.as_str() {
                "style" => style = to_string_value(value),
                "numeric" => numeric = to_string_value(value),
                _ => {}
            }
        }
    }
    Ok(make_object(vec![
        (
            "format".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlRelativeTimeFormatFormat),
        ),
        (
            "formatToParts".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("style".to_string(), Value::String(style)),
                ("numeric".to_string(), Value::String(numeric)),
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
    let style = slot_string(&slots, "style").unwrap_or_else(|| "long".to_string());
    let numeric = slot_string(&slots, "numeric").unwrap_or_else(|| "always".to_string());
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormatFormat => {
            let value = super::number::to_number(arguments.first());
            let unit = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            Ok(Value::String(format_relative(
                &value, &unit, &locale, &style, &numeric,
            )))
        }
        crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts => {
            let value = super::number::to_number(arguments.first());
            let unit = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            let text = format_relative(&value, &unit, &locale, &style, &numeric);
            Ok(make_array(vec![make_object(vec![
                ("type".to_string(), Value::String("literal".to_string())),
                ("value".to_string(), Value::String(text)),
            ])]))
        }
        crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("style".to_string(), Value::String(style)),
            ("numeric".to_string(), Value::String(numeric)),
            (
                "numberingSystem".to_string(),
                Value::String("latn".to_string()),
            ),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn format_relative(value: &f64, unit: &str, locale: &str, style: &str, numeric: &str) -> String {
    let _ = (locale, style, numeric);
    let value = value.round();
    let unit_label = match value.abs() {
        1.0 => unit.to_string(),
        _ => format!("{unit}s"),
    };
    if value == 0.0 {
        return format!("in 0 {unit}s");
    }
    if value < 0.0 {
        format!("{} {} ago", value.abs(), unit_label)
    } else {
        format!("in {value} {unit_label}")
    }
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlRelativeTimeFormatFormat
        | crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts
        | crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
