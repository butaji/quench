//! `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_instance, make_object, resolve_locales, runtime_error, slot_string, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut notation = "standard".to_string();
    let mut compact_display: Option<String> = None;
    let mut minimum_integer_digits = 1.0;
    let mut minimum_fraction_digits = 0.0;
    let mut maximum_fraction_digits = 3.0;
    let mut minimum_significant_digits = None;
    let mut maximum_significant_digits = None;
    let mut plural_type = "cardinal".to_string();
    let mut parsed = ParsedOptions {
        plural_type: plural_type.clone(),
        notation: notation.clone(),
        compact_display: compact_display.clone(),
        minimum_integer_digits,
        minimum_fraction_digits,
        maximum_fraction_digits,
        minimum_significant_digits,
        maximum_significant_digits,
    };
    if let Some(options) = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
    {
        if matches!(options, Value::Null) {
            return Err(crate::value::error::throw_type_error(
                "Cannot convert null to object",
            ));
        }
        let source = options.clone();
        for key in [
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
        ] {
            let value = crate::execute::get_property_result(&source, key)?;
            if matches!(value, Value::Undefined) {
                continue;
            }
            apply_option(key, &value, &mut parsed)?;
        }
    }
    plural_type = parsed.plural_type;
    notation = parsed.notation;
    compact_display = parsed.compact_display;
    minimum_integer_digits = parsed.minimum_integer_digits;
    minimum_fraction_digits = parsed.minimum_fraction_digits;
    maximum_fraction_digits = parsed.maximum_fraction_digits;
    minimum_significant_digits = parsed.minimum_significant_digits;
    maximum_significant_digits = parsed.maximum_significant_digits;
    if !matches!(plural_type.as_str(), "cardinal" | "ordinal") {
        return Err(runtime_error("RangeError: invalid type"));
    }
    let mut slot_properties = vec![
        ("locale".to_string(), Value::String(locale.clone())),
        ("type".to_string(), Value::String(plural_type)),
        ("notation".to_string(), Value::String(notation)),
        (
            "minimumIntegerDigits".to_string(),
            Value::Number(minimum_integer_digits),
        ),
        (
            "minimumFractionDigits".to_string(),
            Value::Number(minimum_fraction_digits),
        ),
        (
            "maximumFractionDigits".to_string(),
            Value::Number(maximum_fraction_digits),
        ),
    ];
    if let Some(value) = minimum_significant_digits {
        slot_properties.push(("minimumSignificantDigits".to_string(), Value::Number(value)));
    }
    if let Some(value) = maximum_significant_digits {
        slot_properties.push(("maximumSignificantDigits".to_string(), Value::Number(value)));
    }
    slot_properties.push((
        "compactDisplay".to_string(),
        Value::String(compact_display.unwrap_or_else(|| "short".to_string())),
    ));
    Ok(make_instance(
        crate::ops::Builtin::IntlPluralRules,
        vec![
            (
                "select".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlPluralRulesSelect),
            ),
            (
                "selectRange".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlPluralRulesSelectRange),
            ),
            (
                "resolvedOptions".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlPluralRulesResolvedOptions),
            ),
            (SLOT.to_string(), make_object(slot_properties)),
        ],
    ))
}

struct ParsedOptions {
    plural_type: String,
    notation: String,
    compact_display: Option<String>,
    minimum_integer_digits: f64,
    minimum_fraction_digits: f64,
    maximum_fraction_digits: f64,
    minimum_significant_digits: Option<f64>,
    maximum_significant_digits: Option<f64>,
}

fn apply_option(key: &str, value: &Value, options: &mut ParsedOptions) -> Result<(), VmError> {
    let text = option_string(value)?;
    match key {
        "type" => options.plural_type = text,
        "notation" => {
            if !matches!(
                text.as_str(),
                "standard" | "compact" | "scientific" | "engineering"
            ) {
                return Err(runtime_error("RangeError: invalid notation"));
            }
            options.notation = text;
        }
        "compactDisplay" => {
            if !matches!(text.as_str(), "short" | "long") {
                return Err(runtime_error("RangeError: invalid compactDisplay"));
            }
            options.compact_display = Some(text);
        }
        "minimumIntegerDigits" => options.minimum_integer_digits = text.parse().unwrap_or(1.0),
        "minimumFractionDigits" => options.minimum_fraction_digits = text.parse().unwrap_or(0.0),
        "maximumFractionDigits" => options.maximum_fraction_digits = text.parse().unwrap_or(3.0),
        "minimumSignificantDigits" => options.minimum_significant_digits = text.parse().ok(),
        "maximumSignificantDigits" => options.maximum_significant_digits = text.parse().ok(),
        _ => {}
    }
    Ok(())
}

fn option_string(value: &Value) -> Result<String, VmError> {
    if crate::value::is_object(value) {
        if let Ok(Value::String(value)) = crate::execute::get_property_result(value, "_value") {
            return Ok(value);
        }
    }
    if let Value::Object(properties) = value {
        if let Some(Value::String(value)) = properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "_value").then_some(value))
        {
            return Ok(value.clone());
        }
    }
    crate::conversion::to_string(value)
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
            let notation =
                slot_string(&slots, "notation").unwrap_or_else(|| "standard".to_string());
            Ok(Value::String(select(
                number,
                &plural_type,
                &locale,
                &notation,
            )))
        }
        crate::ops::Builtin::IntlPluralRulesSelectRange => {
            if arguments.len() < 2
                || matches!(arguments.first(), Some(Value::Undefined))
                || matches!(arguments.get(1), Some(Value::Undefined))
            {
                return Err(crate::value::error::throw_type_error(
                    "selectRange requires two arguments",
                ));
            }
            let start = super::tolocale::value::to_number_result(arguments.first())?;
            let end = super::tolocale::value::to_number_result(arguments.get(1))?;
            if !start.is_finite() || !end.is_finite() {
                return Err(runtime_error("RangeError: value must be finite"));
            }
            let slots = super::intl_slots(receiver)?;
            let plural_type = slot_string(&slots, "type").unwrap_or_else(|| "cardinal".to_string());
            let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
            let notation =
                slot_string(&slots, "notation").unwrap_or_else(|| "standard".to_string());
            Ok(Value::String(select(end, &plural_type, &locale, &notation)))
        }
        crate::ops::Builtin::IntlPluralRulesResolvedOptions => {
            let slots = super::intl_slots(receiver)?;
            let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
            let plural_type = slot_string(&slots, "type").unwrap_or_else(|| "cardinal".to_string());
            let notation =
                slot_string(&slots, "notation").unwrap_or_else(|| "standard".to_string());
            let compact_display =
                slot_string(&slots, "compactDisplay").unwrap_or_else(|| "short".to_string());
            let minimum_integer_digits = slot_number(&slots, "minimumIntegerDigits").unwrap_or(1.0);
            let minimum_fraction_digits =
                slot_number(&slots, "minimumFractionDigits").unwrap_or(0.0);
            let maximum_fraction_digits =
                slot_number(&slots, "maximumFractionDigits").unwrap_or(3.0);
            let minimum_significant_digits = slot_number(&slots, "minimumSignificantDigits");
            let maximum_significant_digits = slot_number(&slots, "maximumSignificantDigits");
            let mut properties = vec![
                ("locale".to_string(), Value::String(locale.clone())),
                ("type".to_string(), Value::String(plural_type.clone())),
                ("notation".to_string(), Value::String(notation.clone())),
                (
                    "minimumIntegerDigits".to_string(),
                    Value::Number(minimum_integer_digits),
                ),
            ];
            if let Some(value) = minimum_significant_digits {
                properties.push(("minimumSignificantDigits".to_string(), Value::Number(value)));
                if let Some(value) = maximum_significant_digits {
                    properties.push(("maximumSignificantDigits".to_string(), Value::Number(value)));
                }
            } else {
                properties.push((
                    "minimumFractionDigits".to_string(),
                    Value::Number(minimum_fraction_digits),
                ));
                properties.push((
                    "maximumFractionDigits".to_string(),
                    Value::Number(maximum_fraction_digits),
                ));
            }
            properties.push((
                "pluralCategories".to_string(),
                Value::array(plural_categories(&locale, &plural_type)),
            ));
            if notation == "compact" {
                properties.push(("compactDisplay".to_string(), Value::String(compact_display)));
            }
            Ok(make_object(properties))
        }
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn plural_categories(locale: &str, plural_type: &str) -> Vec<Value> {
    let names: &[&str] = if plural_type == "ordinal" {
        &["one", "two", "few", "other"]
    } else if locale.starts_with("ar") {
        &["zero", "one", "two", "few", "many", "other"]
    } else if locale.starts_with("ru") || locale.starts_with("uk") {
        &["one", "few", "many", "other"]
    } else if locale.starts_with("fr") {
        &["one", "many", "other"]
    } else if locale.starts_with("gv") {
        &["one", "two", "few", "many", "other"]
    } else if locale.starts_with("sl") {
        &["one", "two", "few", "other"]
    } else if locale.starts_with("ko") {
        &["other"]
    } else {
        &["one", "other"]
    };
    names
        .iter()
        .map(|name| Value::String((*name).to_string()))
        .collect()
}

fn slot_number(slots: &[(String, Value)], key: &str) -> Option<f64> {
    slots
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .and_then(|value| {
            if let Value::Number(number) = value {
                Some(*number)
            } else {
                None
            }
        })
}

fn select(number: f64, plural_type: &str, locale: &str, notation: &str) -> String {
    if plural_type == "ordinal" {
        return ordinal_select(number);
    }
    if locale.starts_with("fr") {
        if notation == "compact" && number.abs() >= 1_000_000.0 {
            return "many".to_string();
        }
        if notation == "standard" && number.abs() == 1_000_000.0 {
            return "many".to_string();
        }
        return if number.abs() < 2.0 { "one" } else { "other" }.to_string();
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
        | crate::ops::Builtin::IntlPluralRulesSelectRange
        | crate::ops::Builtin::IntlPluralRulesResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
