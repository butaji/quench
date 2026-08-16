//! `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_number, slot_string, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let (notation, compact_display, plural_type) = plural_options(arguments.get(1))?;
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
                ("notation".to_string(), Value::String(notation)),
                (
                    "compactDisplay".to_string(),
                    Value::String(compact_display.unwrap_or_else(|| "short".to_string())),
                ),
                ("minimumIntegerDigits".to_string(), Value::Number(1.0)),
                ("minimumFractionDigits".to_string(), Value::Number(0.0)),
                ("maximumFractionDigits".to_string(), Value::Number(3.0)),
            ]),
        ),
    ]))
}

fn plural_options(option: Option<&Value>) -> Result<(String, Option<String>, String), VmError> {
    let Some(option) = option.filter(|value| !matches!(value, Value::Undefined | Value::Null))
    else {
        return Ok(("standard".to_string(), None, "cardinal".to_string()));
    };
    let keys = [
        "localeMatcher",
        "type",
        "notation",
        "compactDisplay",
        "minimumIntegerDigits",
        "minimumFractionDigits",
        "maximumFractionDigits",
        "minimumSignificantDigits",
        "maximumSignificantDigits",
        "roundingIncrement",
        "roundingMode",
        "roundingPriority",
        "trailingZeroDisplay",
    ];
    let values = keys
        .iter()
        .map(|key| read_option(option, key))
        .collect::<Result<Vec<_>, _>>()?;
    let notation = match &values[2] {
        Value::Undefined => "standard".to_string(),
        value => option_text(value)?,
    };
    if !matches!(
        notation.as_str(),
        "standard" | "compact" | "scientific" | "engineering"
    ) {
        return Err(runtime_error("RangeError: invalid notation"));
    }
    let compact = option_text(&values[3])?;
    let compact_display = matches!(compact.as_str(), "short" | "long").then_some(compact);
    let plural_type = match option_text(&values[1])?.as_str() {
        "" => "cardinal".to_string(),
        value => value.to_string(),
    };
    Ok((notation, compact_display, plural_type))
}

fn read_option(option: &Value, key: &str) -> Result<Value, VmError> {
    crate::execute::get_property_result(option, key)
}

fn option_text(value: &Value) -> Result<String, VmError> {
    match value {
        Value::Undefined => Ok(String::new()),
        value => crate::conversion::to_string(value),
    }
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
            let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
            Ok(Value::String(select(number, &plural_type, &locale)))
        }
        crate::ops::Builtin::IntlPluralRulesResolvedOptions => plural_resolved_options(receiver),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn plural_resolved_options(receiver: Option<&Value>) -> Result<Value, VmError> {
    let slots = super::intl_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    let plural_type = slot_string(&slots, "type").unwrap_or_else(|| "cardinal".to_string());
    let notation = slot_string(&slots, "notation").unwrap_or_else(|| "standard".to_string());
    let compact_display =
        slot_string(&slots, "compactDisplay").unwrap_or_else(|| "short".to_string());
    let mut properties = vec![
        ("locale".to_string(), Value::String(locale)),
        ("type".to_string(), Value::String(plural_type)),
        ("notation".to_string(), Value::String(notation.clone())),
        (
            "minimumIntegerDigits".to_string(),
            number_slot_value(&slots, "minimumIntegerDigits", 1.0),
        ),
        (
            "minimumFractionDigits".to_string(),
            number_slot_value(&slots, "minimumFractionDigits", 0.0),
        ),
        (
            "maximumFractionDigits".to_string(),
            number_slot_value(&slots, "maximumFractionDigits", 3.0),
        ),
    ];
    if notation == "compact" {
        properties.push(("compactDisplay".to_string(), Value::String(compact_display)));
    }
    Ok(make_object(properties))
}

fn number_slot_value(slots: &[(String, Value)], key: &str, default: f64) -> Value {
    Value::Number(slot_number(slots, key).unwrap_or(default))
}

fn select(number: f64, plural_type: &str, locale: &str) -> String {
    if plural_type == "ordinal" {
        return ordinal_select(number);
    }
    if locale.starts_with("fr") {
        return if number == 0.0 || number == 1.0 {
            "one"
        } else {
            "other"
        }
        .to_string();
    }
    if locale.starts_with("ru") || locale.starts_with("uk") {
        return russian_cardinal(number);
    }
    if locale.starts_with("ar") {
        return arabic_cardinal(number);
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

fn russian_cardinal(number: f64) -> String {
    if !number.is_finite() || number.fract() != 0.0 {
        return "other".to_string();
    }
    let integer = number.abs() as i64;
    let last = integer % 10;
    let last_two = integer % 100;
    if last == 1 && last_two != 11 {
        "one"
    } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
        "few"
    } else if last == 0 || (5..=9).contains(&last) || (11..=14).contains(&last_two) {
        "many"
    } else {
        "other"
    }
    .to_string()
}

fn arabic_cardinal(number: f64) -> String {
    if number == 0.0 {
        "zero"
    } else if number == 1.0 {
        "one"
    } else if number == 2.0 {
        "two"
    } else if number.fract() == 0.0 && (3.0..=10.0).contains(&number) {
        "few"
    } else if number.fract() == 0.0 && (11.0..=99.0).contains(&number) {
        "many"
    } else {
        "other"
    }
    .to_string()
}

fn ordinal_select(number: f64) -> String {
    if !number.is_finite() || number.fract() != 0.0 {
        return "other".to_string();
    }
    let integer = number.abs() as i64;
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
