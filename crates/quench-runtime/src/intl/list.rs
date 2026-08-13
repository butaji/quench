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
    if let Some(options) = arguments.get(1) {
        if matches!(options, Value::Null) {
            return Err(crate::value::error::throw_type_error(
                "Cannot convert null or undefined to object",
            ));
        }
        let style_value = crate::execute::get_property_result(options, "style")?;
        if !matches!(style_value, Value::Undefined) {
            style = crate::conversion::to_string(&style_value)?;
        }
        let type_value = crate::execute::get_property_result(options, "type")?;
        if !matches!(type_value, Value::Undefined) {
            list_type = crate::conversion::to_string(&type_value)?;
        }
    }
    if !matches!(style.as_str(), "long" | "short" | "narrow") {
        return Err(runtime_error("RangeError: invalid style"));
    }
    if !matches!(list_type.as_str(), "conjunction" | "disjunction" | "unit") {
        return Err(runtime_error("RangeError: invalid type"));
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
            let items = iterable_items(arguments.first())?;
            Ok(Value::String(format_list(
                &items, &locale, &style, &list_type,
            )))
        }
        crate::ops::Builtin::IntlListFormatFormatToParts => {
            let items = iterable_items(arguments.first())?;
            Ok(make_array(format_parts(
                &items, &locale, &style, &list_type,
            )))
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

fn iterable_items(value: Option<&Value>) -> Result<Vec<String>, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    let iterator = crate::collections::iterator::open(value.clone())?;
    crate::collections::iterator::collect(&iterator)
        .map(|values| values.iter().map(to_string_value).collect())
}

fn format_parts(items: &[String], locale: &str, style: &str, list_type: &str) -> Vec<Value> {
    if items.is_empty() {
        return Vec::new();
    }
    if items.len() == 1 {
        return vec![part("element", &items[0])];
    }
    let joiners = joiners(locale, style, list_type);
    let mut parts = Vec::with_capacity(items.len() * 2 - 1);
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            parts.push(part("literal", joiner_for(index, items.len(), &joiners)));
        }
        parts.push(part("element", item));
    }
    parts
}

fn part(kind: &str, value: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String(kind.to_string())),
        ("value".to_string(), Value::String(value.to_string())),
    ])
}

fn joiner_for(index: usize, length: usize, joiners: &(String, String, String)) -> &str {
    if index == length - 1 {
        &joiners.2
    } else {
        &joiners.0
    }
}

fn joiners(locale: &str, style: &str, list_type: &str) -> (String, String, String) {
    if list_type == "unit" {
        return (", ".to_string(), ", ".to_string(), ", ".to_string());
    }
    let spanish = locale.starts_with("es");
    let disjunction = list_type == "disjunction";
    let word = if disjunction {
        if spanish {
            " o "
        } else {
            " or "
        }
    } else if spanish {
        " y "
    } else {
        " and "
    };
    let final_word = if style == "narrow" { " " } else { word };
    let comma = if style == "narrow" { " " } else { ", " };
    let final_joiner = if spanish || style != "long" {
        format!("{}{}", comma, final_word.trim())
    } else {
        format!(",{}", final_word)
    };
    (comma.to_string(), word.to_string(), final_joiner)
}

fn format_list(items: &[String], locale: &str, style: &str, list_type: &str) -> String {
    format_parts(items, locale, style, list_type)
        .into_iter()
        .filter_map(|part| match part {
            Value::Object(object) => object
                .properties
                .iter()
                .find(|(key, _)| key == "value")
                .and_then(|(_, value)| match value {
                    Value::String(value) => Some(value.clone()),
                    _ => None,
                }),
            _ => None,
        })
        .collect()
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
