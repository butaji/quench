//! `Intl.ListFormat`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut style = "long".to_string();
    let mut list_type = "conjunction".to_string();
    if let Some(Value::Object(properties)) = arguments.get(1) {
        for (key, value) in properties.iter() {
            match key.as_str() {
                "style" => style = to_string_value(value),
                "type" => list_type = to_string_value(value),
                _ => {}
            }
        }
    }
    Ok(make_object(vec![
        (
            "format".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlListFormatFormat),
        ),
        (
            "formatToParts".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlListFormatFormatToParts),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlListFormatResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("style".to_string(), Value::String(style)),
                ("type".to_string(), Value::String(list_type)),
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
    let list_type = slot_string(&slots, "type").unwrap_or_else(|| "conjunction".to_string());
    match builtin {
        crate::ops::Builtin::IntlListFormatFormat => {
            let items = array_items(arguments.first());
            Ok(Value::String(format_list(
                &items, &locale, &style, &list_type,
            )))
        }
        crate::ops::Builtin::IntlListFormatFormatToParts => {
            let items = array_items(arguments.first());
            let joined = format_list(&items, &locale, &style, &list_type);
            Ok(make_array(vec![make_object(vec![
                ("type".to_string(), Value::String("element".to_string())),
                ("value".to_string(), Value::String(joined)),
            ])]))
        }
        crate::ops::Builtin::IntlListFormatResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("style".to_string(), Value::String(style)),
            ("type".to_string(), Value::String(list_type)),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn array_items(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values.iter().map(to_string_value).collect(),
        _ => Vec::new(),
    }
}

fn format_list(items: &[String], locale: &str, style: &str, list_type: &str) -> String {
    let _ = locale;
    if items.is_empty() {
        return String::new();
    }
    if items.len() == 1 {
        return items[0].clone();
    }
    if items.len() == 2 {
        let (and, or) = if style == "short" || style == "narrow" {
            (" & ", " or ")
        } else {
            (" and ", " or ")
        };
        let joiner = if list_type == "disjunction" { or } else { and };
        return format!("{}{joiner}{}", items[0], items[1]);
    }
    let last = items.last().unwrap();
    let head = &items[..items.len() - 1];
    let (and, or) = if style == "short" || style == "narrow" {
        (", & ", ", or ")
    } else {
        (", and ", ", or ")
    };
    let joiner = if list_type == "disjunction" { or } else { and };
    format!("{}{joiner}{last}", head.join(", "))
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlListFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlListFormatFormat
        | crate::ops::Builtin::IntlListFormatFormatToParts
        | crate::ops::Builtin::IntlListFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
