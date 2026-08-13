//! `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::{default_locale, make_object, resolve_locales, runtime_error, slot_string, SLOT};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let plural_type = match arguments.get(1) {
        Some(Value::Object(properties)) => properties
            .iter()
            .find(|(key, _)| key == "type")
            .map(|(_, value)| super::to_string_value(value))
            .unwrap_or_else(|| "cardinal".to_string()),
        _ => "cardinal".to_string(),
    };
    if !matches!(plural_type.as_str(), "cardinal" | "ordinal") {
        return Err(runtime_error("RangeError: invalid type"));
    }
    Ok(make_object(vec![
        (
            "select".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlPluralRulesSelect),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlPluralRulesResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("type".to_string(), Value::String(plural_type)),
            ]),
        ),
    ]))
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        crate::ops::Builtin::IntlPluralRulesSelect => {
            let number = super::number::to_number(arguments.first());
            let slots = super::intl_slots(receiver)?;
            let plural_type = slot_string(&slots, "type").unwrap_or_else(|| "cardinal".to_string());
            Ok(Value::String(select(number, &plural_type)))
        }
        crate::ops::Builtin::IntlPluralRulesResolvedOptions => {
            let slots = super::intl_slots(receiver)?;
            let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
            let plural_type = slot_string(&slots, "type").unwrap_or_else(|| "cardinal".to_string());
            Ok(make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("type".to_string(), Value::String(plural_type)),
            ]))
        }
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn select(number: f64, plural_type: &str) -> String {
    if plural_type == "ordinal" {
        return ordinal_select(number);
    }
    if number == 1.0 {
        "one"
    } else if number == 2.0 {
        "two"
    } else if number == 0.0 {
        "zero"
    } else {
        "other"
    }
    .to_string()
}

fn ordinal_select(number: f64) -> String {
    let integer = number as i64;
    if integer % 10 == 1 && integer % 100 != 11 {
        "one"
    } else if integer % 10 == 2 && integer % 100 != 12 {
        "two"
    } else if integer % 10 == 3 && integer % 100 != 13 {
        "few"
    } else {
        "other"
    }
    .to_string()
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlPluralRules => Some(construct(arguments)),
        crate::ops::Builtin::IntlPluralRulesSelect
        | crate::ops::Builtin::IntlPluralRulesResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
