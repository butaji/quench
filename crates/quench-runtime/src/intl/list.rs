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
        (style, list_type) = list_options(options, style, list_type)?;
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

fn list_options(
    options: &Value,
    mut style: String,
    mut list_type: String,
) -> Result<(String, String), VmError> {
    if matches!(options, Value::Null) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null or undefined to object",
        ));
    }
    let matcher = crate::execute::get_property_result(options, "localeMatcher")?;
    validate_matcher(&matcher)?;
    let type_value = crate::execute::get_property_result(options, "type")?;
    if !matches!(type_value, Value::Undefined) {
        list_type = crate::conversion::to_string(&type_value)?;
    }
    let style_value = crate::execute::get_property_result(options, "style")?;
    if !matches!(style_value, Value::Undefined) {
        style = crate::conversion::to_string(&style_value)?;
    }
    Ok((style, list_type))
}

fn validate_matcher(value: &Value) -> Result<(), VmError> {
    if matches!(value, Value::Undefined) {
        return Ok(());
    }
    let matcher = crate::conversion::to_string(value)?;
    if matches!(matcher.as_str(), "lookup" | "best fit") {
        Ok(())
    } else {
        Err(runtime_error("RangeError: invalid localeMatcher"))
    }
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
            ("type".to_string(), Value::String(list_type)),
            ("style".to_string(), Value::String(style)),
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
    if length == 2 {
        return &joiners.1;
    }
    if index == length - 1 {
        &joiners.2
    } else {
        &joiners.0
    }
}

fn joiners(locale: &str, style: &str, list_type: &str) -> (String, String, String) {
    if style == "narrow" {
        return narrow_joiners(list_type);
    }
    let spanish = locale.starts_with("es");
    if list_type == "unit" && !spanish {
        return comma_joiners();
    }
    let disjunction = list_type == "disjunction";
    let word = join_word(style, spanish, disjunction);
    if list_type == "unit" && spanish && style == "short" {
        return (", ".to_string(), word.to_string(), ", ".to_string());
    }
    let final_joiner = final_joiner(style, spanish, disjunction, word);
    (", ".to_string(), word.to_string(), final_joiner)
}

fn narrow_joiners(list_type: &str) -> (String, String, String) {
    let separator = if list_type == "unit" { " " } else { ", " };
    (
        separator.to_string(),
        separator.to_string(),
        separator.to_string(),
    )
}

fn comma_joiners() -> (String, String, String) {
    (", ".to_string(), ", ".to_string(), ", ".to_string())
}

fn join_word(style: &str, spanish: bool, disjunction: bool) -> &'static str {
    match (style, spanish, disjunction) {
        ("short", false, false) => " & ",
        (_, true, true) => " o ",
        (_, false, true) => " or ",
        (_, true, false) => " y ",
        _ => " and ",
    }
}

fn final_joiner(style: &str, spanish: bool, disjunction: bool, word: &str) -> String {
    if spanish {
        word.to_string()
    } else if style == "short" && disjunction {
        ", or ".to_string()
    } else if style == "short" {
        ", & ".to_string()
    } else {
        format!(",{}", word)
    }
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
