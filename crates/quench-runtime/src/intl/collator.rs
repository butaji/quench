//! `Intl.Collator`.

use crate::{execute::VmError, value::Value};

use icu_collator::{
    options::{AlternateHandling, CaseLevel, CollatorOptions as IcuOptions, Strength},
    preferences::{CollationCaseFirst, CollationNumericOrdering, CollationType},
    provider::CollationTailoringV1,
    Collator, CollatorPreferences,
};
use icu_provider::{
    marker::DataMarkerExt, DataIdentifierBorrowed, DataMarkerAttributes, DataProvider, DataRequest,
};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_bool, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let mut locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut options = parse_options(arguments.get(1), locale.starts_with("th"))?;
    let extension = locale_collation(&locale);
    let option = options.collation.take();
    let collation = select_collation(&locale, option.as_deref(), extension.as_deref());
    locale = resolve_collation(
        &locale,
        option.as_deref(),
        extension.as_deref(),
        collation.as_deref(),
    )?;
    options.collation = collation;
    if options.case_first == "false" && case_first_option_absent(arguments.get(1)) {
        options.case_first =
            super::locale::case_first_extension(&locale).unwrap_or_else(|| "false".to_string());
    }
    if locale_has_true_kn(&locale) && numeric_option_absent(arguments.get(1)) {
        options.numeric = true;
    }
    for key in ["ka", "kb", "kc", "kh", "kk", "kr", "ks", "vt"] {
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

fn case_first_option_absent(value: Option<&Value>) -> bool {
    let Some(Value::Object(properties)) = value else {
        return true;
    };
    properties
        .iter()
        .find(|(name, _)| name == "caseFirst")
        .is_none_or(|(_, value)| matches!(value, Value::Undefined))
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
            "\0prototype".to_string(),
            crate::vm::realm_intrinsic(crate::ops::Builtin::IntlCollatorPrototype),
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
                (
                    "collation".to_string(),
                    options.collation.map_or(Value::Undefined, Value::String),
                ),
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
    collation: Option<String>,
}

fn parse_options(value: Option<&Value>, default_ignore: bool) -> Result<CollatorOptions, VmError> {
    let mut options = CollatorOptions {
        usage: "sort".to_string(),
        sensitivity: "variant".to_string(),
        ignore_punctuation: default_ignore,
        numeric: false,
        case_first: "false".to_string(),
        collation: None,
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
    let locale_matcher = option_string(properties, "localeMatcher", "best fit")?;
    validate_option(&locale_matcher, &["lookup", "best fit"], "localeMatcher")?;
    let collation = option_value(properties, "collation")?;
    if !matches!(collation, Value::Undefined) {
        options.collation = Some(crate::conversion::to_string(&collation)?);
    }
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

fn locale_collation(locale: &str) -> Option<String> {
    let parts: Vec<&str> = locale.split('-').collect();
    let index = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("co"))?;
    parts.get(index + 1).map(|value| (*value).to_string())
}

fn select_collation(locale: &str, option: Option<&str>, extension: Option<&str>) -> Option<String> {
    option
        .filter(|value| provider_has_collation(locale, value))
        .or_else(|| extension.filter(|value| provider_has_collation(locale, value)))
        .map(str::to_string)
}

fn resolve_collation(
    locale: &str,
    option: Option<&str>,
    extension: Option<&str>,
    collation: Option<&str>,
) -> Result<String, VmError> {
    let Some(collation) = collation else {
        return Ok(remove_unsupported_extension(locale, "co"));
    };
    if option.is_some_and(|value| Some(value) != extension && provider_has_collation(locale, value))
    {
        return Ok(remove_unsupported_extension(locale, "co"));
    }
    Ok(set_extension(locale, "co", collation))
}

fn provider_has_collation(locale: &str, collation: &str) -> bool {
    let locale = remove_unsupported_extension(locale, "co");
    let Ok(locale) = icu_locale_core::Locale::try_from_str(&locale) else {
        return false;
    };
    let Ok(value) = collation.parse::<icu_locale_core::extensions::unicode::Value>() else {
        return false;
    };
    let Ok(collation_type) = CollationType::try_from(&value) else {
        return false;
    };
    let mut preferences = CollatorPreferences::default();
    preferences.locale_preferences = (&locale).into();
    preferences.collation_type = Some(collation_type);
    let Ok(attributes) = DataMarkerAttributes::try_from_str(collation) else {
        return false;
    };
    let data_locale = CollationTailoringV1::make_locale(preferences.locale_preferences);
    let request = DataRequest {
        id: DataIdentifierBorrowed::for_marker_attributes_and_locale(attributes, &data_locale),
        metadata: Default::default(),
    };
    <icu_collator::provider::Baked as DataProvider<CollationTailoringV1>>::load(
        &icu_collator::provider::Baked,
        request,
    )
    .is_ok()
}

fn set_extension(locale: &str, key: &str, value: &str) -> String {
    let mut parts: Vec<&str> = locale.split('-').collect();
    let Some(index) = parts.iter().position(|part| part.eq_ignore_ascii_case("u")) else {
        return format!("{locale}-u-{key}-{value}");
    };
    let end = parts
        .iter()
        .enumerate()
        .skip(index + 1)
        .find_map(|(position, part)| (part.len() == 1).then_some(position))
        .unwrap_or(parts.len());
    let mut extension = parts[index + 1..end].to_vec();
    while extension.first().is_some_and(|part| part.len() != 2) {
        extension.remove(0);
    }
    if let Some(key_index) = extension
        .iter()
        .position(|part| part.eq_ignore_ascii_case(key))
    {
        let value_end = extension
            .iter()
            .enumerate()
            .skip(key_index + 1)
            .find_map(|(position, part)| (part.len() == 2).then_some(position))
            .unwrap_or(extension.len());
        extension.splice(key_index..value_end, [key, value]);
    } else {
        extension.extend([key, value]);
    }
    parts.splice(index + 1..end, extension);
    parts.join("-")
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
        crate::ops::Builtin::IntlCollatorCompareGetter => {
            let receiver =
                receiver.ok_or_else(|| runtime_error("TypeError: not an Intl object"))?;
            if !matches!(receiver, Value::Object(properties) if properties.iter().any(|(name, _)| name == SLOT))
            {
                return Err(runtime_error("TypeError: not an Intl object"));
            }
            Ok(crate::vm::bind_receiver_property(
                Value::Builtin(crate::ops::Builtin::IntlCollatorCompare),
                receiver,
            ))
        }
        crate::ops::Builtin::IntlCollatorCompare => {
            let left = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            let right = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            let ignore_punctuation = slot_bool(&slots, "ignorePunctuation").unwrap_or(false);
            let sensitivity =
                slot_string(&slots, "sensitivity").unwrap_or_else(|| "variant".to_string());
            let usage = slot_string(&slots, "usage").unwrap_or_else(|| "sort".to_string());
            let numeric = slot_bool(&slots, "numeric").unwrap_or(false);
            let case_first =
                slot_string(&slots, "caseFirst").unwrap_or_else(|| "false".to_string());
            Ok(Value::Number(compare_with_options(
                &left,
                &right,
                &locale,
                ignore_punctuation,
                &sensitivity,
                &usage,
                numeric,
                &case_first,
            )))
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
            Value::String(slot_string(slots, "collation").unwrap_or_else(|| "default".to_string())),
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

pub(crate) fn compare(
    left: &str,
    right: &str,
    locale: &str,
    ignore_punctuation: bool,
    sensitivity: &str,
) -> f64 {
    compare_with_options(
        left,
        right,
        locale,
        ignore_punctuation,
        sensitivity,
        "sort",
        false,
        "false",
    )
}

fn compare_with_options(
    left: &str,
    right: &str,
    locale: &str,
    ignore_punctuation: bool,
    sensitivity: &str,
    usage: &str,
    numeric: bool,
    case_first: &str,
) -> f64 {
    if let Some(ordering) = icu_ordering(
        left,
        right,
        locale,
        ignore_punctuation,
        sensitivity,
        usage,
        numeric,
        case_first,
    ) {
        return ordering;
    }
    lexical_compare(left, right, ignore_punctuation, sensitivity)
}

fn lexical_compare(left: &str, right: &str, ignore_punctuation: bool, sensitivity: &str) -> f64 {
    let left = sensitivity_text(left, ignore_punctuation, sensitivity);
    let right = sensitivity_text(right, ignore_punctuation, sensitivity);
    match left.cmp(&right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

fn icu_ordering(
    left: &str,
    right: &str,
    locale: &str,
    ignore_punctuation: bool,
    sensitivity: &str,
    usage: &str,
    numeric: bool,
    case_first: &str,
) -> Option<f64> {
    let collation = locale_collation(locale).unwrap_or_else(|| "standard".to_string());
    if !provider_has_collation(locale, &collation) {
        return None;
    }
    let locale = icu_locale_core::Locale::try_from_str(locale).ok()?;
    let preferences = icu_preferences(&locale, usage, numeric, case_first);
    let options = icu_options(ignore_punctuation, sensitivity);
    let collator = Collator::try_new(preferences, options).ok()?;
    Some(match collator.compare(left, right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    })
}

fn icu_preferences(
    locale: &icu_locale_core::Locale,
    usage: &str,
    numeric: bool,
    case_first: &str,
) -> CollatorPreferences {
    let mut preferences = CollatorPreferences::default();
    preferences.locale_preferences = (&*locale).into();
    preferences.numeric_ordering = Some(if numeric {
        CollationNumericOrdering::True
    } else {
        CollationNumericOrdering::False
    });
    preferences.case_first = Some(match case_first {
        "upper" => CollationCaseFirst::Upper,
        "lower" => CollationCaseFirst::Lower,
        _ => CollationCaseFirst::False,
    });
    if usage == "search" {
        preferences.collation_type = Some(CollationType::Search);
    }
    preferences
}

fn icu_options(ignore_punctuation: bool, sensitivity: &str) -> IcuOptions {
    let mut options = IcuOptions::default();
    options.strength = Some(match sensitivity {
        "base" | "case" => Strength::Primary,
        "accent" => Strength::Secondary,
        _ => Strength::Tertiary,
    });
    options.case_level = (sensitivity == "case").then_some(CaseLevel::On);
    options.alternate_handling = ignore_punctuation.then_some(AlternateHandling::Shifted);
    options
}

fn sensitivity_text(value: &str, ignore_punctuation: bool, sensitivity: &str) -> String {
    let value = comparable_text(value, ignore_punctuation);
    let value = unicode_normalization::UnicodeNormalization::nfd(value.chars())
        .filter(|character| sensitivity != "base" && sensitivity != "case" || !is_mark(*character))
        .collect::<String>();
    if sensitivity == "base" || sensitivity == "accent" {
        value.to_lowercase()
    } else {
        value
    }
}

fn is_mark(character: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&character)
}

fn comparable_text(value: &str, ignore_punctuation: bool) -> String {
    if !ignore_punctuation {
        return value.to_string();
    }
    value
        .chars()
        .filter(|character| !character.is_ascii_punctuation() && !character.is_whitespace())
        .collect()
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlCollator => Some(construct(arguments)),
        crate::ops::Builtin::IntlCollatorCompare
        | crate::ops::Builtin::IntlCollatorCompareGetter
        | crate::ops::Builtin::IntlCollatorResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
