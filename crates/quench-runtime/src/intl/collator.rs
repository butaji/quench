//! `Intl.Collator`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_bool, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let mut locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut options = parse_options(arguments.get(1), locale.starts_with("th"))?;
    if locale_has_true_kn(&locale) && numeric_option_absent(arguments.get(1)) {
        options.numeric = true;
    }
    for key in ["co", "ka", "kb", "kc", "kh", "kk", "kr", "ks", "vt"] {
        locale = remove_unsupported_extension(&locale, key);
    }
    if options.case_first != "false" {
        locale = remove_conflicting_extension(&locale, "kf", &options.case_first);
    }
    if options.numeric {
        locale = remove_conflicting_extension(&locale, "kn", "true");
    }
    Ok(collator_object(locale, options))
}

fn locale_has_true_kn(locale: &str) -> bool {
    let parts: Vec<&str> = locale.split('-').collect();
    parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("kn"))
        .is_some_and(|index| {
            parts
                .get(index + 1)
                .is_none_or(|value| value.len() == 2 || value.eq_ignore_ascii_case("true"))
        })
}

fn numeric_option_absent(value: Option<&Value>) -> bool {
    let Some(Value::Object(properties)) = value else {
        return true;
    };
    properties
        .iter()
        .find(|(name, _)| name == "numeric")
        .is_none_or(|(_, value)| matches!(value, Value::Undefined))
}

fn collator_object(locale: String, options: CollatorOptions) -> Value {
    make_object(vec![
        (
            "compare".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlCollatorCompare),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlCollatorResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("usage".to_string(), Value::String(options.usage)),
                ("numeric".to_string(), Value::Boolean(options.numeric)),
                ("caseFirst".to_string(), Value::String(options.case_first)),
                (
                    "sensitivity".to_string(),
                    Value::String(options.sensitivity),
                ),
                (
                    "ignorePunctuation".to_string(),
                    Value::Boolean(options.ignore_punctuation),
                ),
            ]),
        ),
    ])
}

struct CollatorOptions {
    usage: String,
    sensitivity: String,
    ignore_punctuation: bool,
    numeric: bool,
    case_first: String,
}

fn parse_options(value: Option<&Value>, default_ignore: bool) -> Result<CollatorOptions, VmError> {
    let mut options = CollatorOptions {
        usage: "sort".to_string(),
        sensitivity: "variant".to_string(),
        ignore_punctuation: default_ignore,
        numeric: false,
        case_first: "false".to_string(),
    };
    let Some(Value::Object(properties)) = value else {
        return Ok(options);
    };
    let properties = Value::Object(properties.clone());
    apply_properties(&mut options, &properties)?;
    Ok(options)
}

fn apply_properties(options: &mut CollatorOptions, properties: &Value) -> Result<(), VmError> {
    options.usage = option_string(properties, "usage", &options.usage)?;
    validate_option(&options.usage, &["sort", "search"], "usage")?;
    let locale_matcher = option_value(properties, "localeMatcher")?;
    if matches!(locale_matcher, Value::Null) {
        return Err(crate::value::error::throw_type_error(
            "Invalid localeMatcher",
        ));
    }
    let _ = option_value(properties, "collation")?;
    options.numeric = option_boolean(properties, "numeric", options.numeric)?;
    options.case_first = option_string(properties, "caseFirst", &options.case_first)?;
    validate_option(
        &options.case_first,
        &["upper", "lower", "false"],
        "caseFirst",
    )?;
    options.sensitivity = option_string(properties, "sensitivity", &options.sensitivity)?;
    validate_option(
        &options.sensitivity,
        &["base", "accent", "case", "variant"],
        "sensitivity",
    )?;
    options.ignore_punctuation =
        option_boolean(properties, "ignorePunctuation", options.ignore_punctuation)?;
    Ok(())
}

fn option_value(properties: &Value, name: &str) -> Result<Value, VmError> {
    crate::execute::get_property_result(properties, name)
}

fn option_string(properties: &Value, name: &str, default: &str) -> Result<String, VmError> {
    let value = option_value(properties, name)?;
    if matches!(value, Value::Undefined) {
        return Ok(default.to_string());
    }
    crate::conversion::to_string(&value)
}

fn option_boolean(properties: &Value, name: &str, default: bool) -> Result<bool, VmError> {
    let value = option_value(properties, name)?;
    Ok(if matches!(value, Value::Undefined) {
        default
    } else {
        crate::execute::is_truthy(&value)
    })
}

fn validate_option(value: &str, allowed: &[&str], name: &str) -> Result<(), VmError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(runtime_error(&format!("RangeError: invalid {name}")))
    }
}

fn remove_conflicting_extension(locale: &str, key: &str, value: &str) -> String {
    let parts: Vec<&str> = locale.split('-').collect();
    let Some(index) = parts.iter().position(|part| part.eq_ignore_ascii_case(key)) else {
        return locale.to_string();
    };
    let extension_value = parts.get(index + 1).copied().unwrap_or("true");
    if extension_value.eq_ignore_ascii_case(value) {
        if value == "true" && parts.get(index + 1).is_some() {
            return parts[..=index]
                .iter()
                .chain(parts.get(index + 2..).unwrap_or_default().iter())
                .copied()
                .collect::<Vec<_>>()
                .join("-");
        }
        return locale.to_string();
    }
    let result = parts[..index]
        .iter()
        .chain(parts.get(index + 2..).unwrap_or_default().iter())
        .copied()
        .collect::<Vec<_>>()
        .join("-")
        .trim_end_matches("-u")
        .to_string();
    result
}

fn remove_unsupported_extension(locale: &str, key: &str) -> String {
    let parts: Vec<&str> = locale.split('-').collect();
    let Some(index) = parts.iter().position(|part| part.eq_ignore_ascii_case(key)) else {
        return locale.to_string();
    };
    let end = parts
        .iter()
        .enumerate()
        .skip(index + 1)
        .find_map(|(position, part)| (part.len() == 2).then_some(position))
        .unwrap_or(parts.len());
    parts[..index]
        .iter()
        .chain(parts.get(end..).unwrap_or_default().iter())
        .copied()
        .collect::<Vec<_>>()
        .join("-")
        .trim_end_matches("-u")
        .to_string()
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    match builtin {
        crate::ops::Builtin::IntlCollatorCompare => {
            let left = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            let right = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            Ok(Value::Number(compare(&left, &right, &locale)))
        }
        crate::ops::Builtin::IntlCollatorResolvedOptions => Ok(resolved_options(&slots, locale)),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn resolved_options(slots: &[(String, Value)], locale: String) -> Value {
    make_object(vec![
        ("locale".to_string(), Value::String(locale)),
        (
            "usage".to_string(),
            Value::String(slot_string(slots, "usage").unwrap_or_else(|| "sort".to_string())),
        ),
        (
            "sensitivity".to_string(),
            Value::String(
                slot_string(slots, "sensitivity").unwrap_or_else(|| "variant".to_string()),
            ),
        ),
        (
            "ignorePunctuation".to_string(),
            Value::Boolean(slot_bool(slots, "ignorePunctuation").unwrap_or(false)),
        ),
        (
            "collation".to_string(),
            Value::String("default".to_string()),
        ),
        (
            "numeric".to_string(),
            Value::Boolean(slot_bool(slots, "numeric").unwrap_or(false)),
        ),
        (
            "caseFirst".to_string(),
            Value::String(slot_string(slots, "caseFirst").unwrap_or_else(|| "false".to_string())),
        ),
    ])
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

pub(crate) fn compare(left: &str, right: &str, locale: &str) -> f64 {
    let _ = locale;
    match left.cmp(right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlCollator => Some(construct(arguments)),
        crate::ops::Builtin::IntlCollatorCompare
        | crate::ops::Builtin::IntlCollatorResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
