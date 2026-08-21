//! `Intl.NumberFormat` and `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::number_format::*;

mod number_methods;
mod number_render;

pub(crate) use number_methods::{localize_digits, prototype_method, supports_digit_system};
pub(crate) use number_render::*;

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_bool,
    slot_number, slot_string, to_string_value, SLOT,
};

pub(crate) struct NumberOptions {
    pub locale: String,
    pub numbering_system: String,
    pub style: String,
    pub currency: Option<String>,
    pub currency_display: String,
    pub currency_sign: String,
    pub unit: Option<String>,
    pub unit_display: String,
    pub grouping: String,
    pub minimum_integer_digits: u32,
    pub minimum_fraction_digits: u32,
    pub maximum_fraction_digits: u32,
    pub use_grouping: bool,
    pub grouping_min2: bool,
    pub notation: String,
    pub compact_display: String,
    pub rounding_mode: String,
    pub rounding_increment: u32,
    pub sign_display: String,
    pub minimum_significant_digits: Option<u32>,
    pub maximum_significant_digits: Option<u32>,
    pub rounding_priority: String,
    pub trailing_zero_display: String,
}

pub(crate) struct RawOptions {
    style: String,
    numbering_system: Option<String>,
    currency: Option<String>,
    currency_display: String,
    currency_sign: String,
    unit: Option<String>,
    unit_display: String,
    grouping: String,
    grouping_explicit: bool,
    minimum_fraction_digits: f64,
    minimum_integer_digits: f64,
    maximum_fraction_digits: f64,
    use_grouping: bool,
    grouping_min2: bool,
    notation: String,
    compact_display: String,
    rounding_mode: String,
    rounding_increment: f64,
    sign_display: String,
    minimum_significant_digits: f64,
    maximum_significant_digits: f64,
    rounding_priority: String,
    trailing_zero_display: String,
}

const OPTION_KEYS: &[&str] = &[
    "localeMatcher",
    "numberingSystem",
    "style",
    "currency",
    "currencyDisplay",
    "currencySign",
    "unit",
    "unitDisplay",
    "notation",
    "minimumIntegerDigits",
    "minimumFractionDigits",
    "maximumFractionDigits",
    "minimumSignificantDigits",
    "maximumSignificantDigits",
    "roundingIncrement",
    "roundingMode",
    "roundingPriority",
    "trailingZeroDisplay",
    "compactDisplay",
    "useGrouping",
    "signDisplay",
];

fn option_text(key: &str, value: &Value) -> Result<String, VmError> {
    if matches!(
        key,
        "roundingMode" | "roundingPriority" | "trailingZeroDisplay"
    ) {
        return crate::conversion::to_string(value);
    }
    if key == "roundingIncrement" {
        return Ok(crate::conversion::to_number(value)?.to_string());
    }
    if matches!(
        key,
        "localeMatcher" | "style" | "currency" | "currencyDisplay" | "unitDisplay"
    ) {
        return crate::conversion::to_string(value);
    }
    Ok(to_string_value(value))
}

fn validate_option(key: &str, text: &str) -> Result<(), VmError> {
    if matches!(key, "minimumFractionDigits" | "maximumFractionDigits") {
        let digits = text.parse::<f64>().unwrap_or(f64::NAN);
        if !digits.is_finite() || digits.fract() != 0.0 || !(0.0..=100.0).contains(&digits) {
            return Err(crate::value::error::throw_range_error(
                "fraction digits out of range",
            ));
        }
    }
    if key == "style" && !matches!(text, "decimal" | "currency" | "percent" | "unit") {
        return Err(runtime_error("RangeError: style"));
    }
    if key == "localeMatcher" && !matches!(text, "lookup" | "best fit") {
        return Err(runtime_error("RangeError: localeMatcher"));
    }
    if key == "currency"
        && (text.len() != 3
            || !text
                .chars()
                .all(|character| character.is_ascii_alphabetic()))
    {
        return Err(runtime_error("RangeError: currency"));
    }
    if key == "trailingZeroDisplay" && !matches!(text, "auto" | "stripIfInteger") {
        return Err(crate::value::error::throw_range_error(
            "invalid trailingZeroDisplay",
        ));
    }
    if key == "roundingPriority" && !matches!(text, "auto" | "morePrecision" | "lessPrecision") {
        return Err(crate::value::error::throw_range_error(
            "invalid roundingPriority",
        ));
    }
    if key == "numberingSystem" && !valid_numbering_system_syntax(text) {
        return Err(crate::value::error::throw_range_error(
            "invalid numbering system",
        ));
    }
    Ok(())
}

impl RawOptions {
    fn from_value(options: Option<&Value>) -> Result<Self, VmError> {
        let mut raw = RawOptions {
            style: "decimal".to_string(),
            numbering_system: None,
            currency: None,
            currency_display: "symbol".to_string(),
            currency_sign: "standard".to_string(),
            unit: None,
            unit_display: "short".to_string(),
            grouping: "auto".to_string(),
            grouping_explicit: false,
            minimum_fraction_digits: -1.0,
            minimum_integer_digits: 1.0,
            maximum_fraction_digits: -1.0,
            use_grouping: true,
            grouping_min2: false,
            notation: "standard".to_string(),
            compact_display: "short".to_string(),
            rounding_mode: "halfExpand".to_string(),
            rounding_increment: 1.0,
            sign_display: "auto".to_string(),
            minimum_significant_digits: -1.0,
            maximum_significant_digits: -1.0,
            rounding_priority: "auto".to_string(),
            trailing_zero_display: "auto".to_string(),
        };
        if let Some(options) = options.filter(|value| crate::value::is_object(value)) {
            for key in OPTION_KEYS {
                let value = crate::execute::get_property_result(options, key)?;
                if !matches!(value, Value::Undefined) {
                    if *key == "useGrouping" {
                        let (grouping, enabled, min2) = normalize_grouping(&value)?;
                        raw.grouping = grouping;
                        raw.grouping_explicit = true;
                        raw.use_grouping = enabled;
                        raw.grouping_min2 = min2;
                        continue;
                    }
                    let text = option_text(key, &value)?;
                    validate_option(key, &text)?;
                    apply_option(&mut raw, key, &text);
                }
            }
        }
        if raw.style == "currency" && raw.currency.is_none() {
            return Err(runtime_error("TypeError: currency"));
        }
        Ok(raw)
    }
}

fn valid_numbering_system_syntax(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            (3..=8).contains(&part.len())
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
}

fn apply_option(raw: &mut RawOptions, key: &str, value: &str) {
    match key {
        "style" => raw.style = value.to_string(),
        "numberingSystem" if super::supported_values::NUMBERING_SYSTEMS.contains(&value) => {
            raw.numbering_system = Some(value.to_string())
        }
        "currency" => raw.currency = Some(value.to_ascii_uppercase()),
        "currencyDisplay" => raw.currency_display = value.to_string(),
        "currencySign" => raw.currency_sign = value.to_string(),
        "unit" => raw.unit = Some(value.to_string()),
        "unitDisplay" => raw.unit_display = value.to_string(),
        "minimumFractionDigits" => raw.minimum_fraction_digits = value.parse().unwrap_or(0.0),
        "minimumIntegerDigits" => raw.minimum_integer_digits = value.parse().unwrap_or(1.0),
        "maximumFractionDigits" => raw.maximum_fraction_digits = value.parse().unwrap_or(3.0),
        "useGrouping" => raw.grouping = value.to_string(),
        "notation" => raw.notation = value.to_string(),
        "compactDisplay" => raw.compact_display = value.to_string(),
        "roundingMode" => raw.rounding_mode = value.to_string(),
        "roundingIncrement" => raw.rounding_increment = value.parse().unwrap_or(1.0),
        "signDisplay" => raw.sign_display = value.to_string(),
        "minimumSignificantDigits" => {
            raw.minimum_significant_digits = value.parse().unwrap_or(-1.0)
        }
        "maximumSignificantDigits" => {
            raw.maximum_significant_digits = value.parse().unwrap_or(-1.0)
        }
        "roundingPriority" => raw.rounding_priority = value.to_string(),
        "trailingZeroDisplay" => raw.trailing_zero_display = value.to_string(),
        _ => {}
    }
}

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    if matches!(arguments.get(1), Some(Value::Null)) {
        return Err(crate::value::error::throw_type_error(
            "options must not be null",
        ));
    }
    let options = NumberOptions::from_options(locale, arguments.get(1))?;
    Ok(options.build_object())
}

impl NumberOptions {
    fn from_options(locale: String, options: Option<&Value>) -> Result<Self, VmError> {
        let raw = RawOptions::from_value(options)?;
        validate_unit_display(&raw.unit_display)?;
        validate_significant_digits(&raw)?;
        validate_currency_display(&raw.currency_display)?;
        validate_rounding_mode(&raw.rounding_mode)?;
        validate_rounding_increment(&raw)?;
        validate_trailing_zero_display(&raw.trailing_zero_display)?;
        let minimum_fraction_digits = fraction_digits(
            raw.style.as_str(),
            raw.currency.as_deref(),
            raw.notation.as_str(),
            raw.minimum_fraction_digits,
        );
        let maximum_fraction_digits = maximum_fraction(
            &raw.style,
            &raw.currency,
            &raw.notation,
            raw.maximum_fraction_digits,
            minimum_fraction_digits,
        );
        let minimum_fraction_digits = minimum_fraction_digits.min(maximum_fraction_digits);
        if raw.style == "unit" {
            if raw.unit.is_none() {
                return Err(crate::value::error::throw_type_error("unit is required"));
            }
            if !valid_unit(raw.unit.as_deref()) {
                return Err(crate::value::error::throw_range_error("invalid unit"));
            }
        } else if raw.unit.is_some() && !valid_unit(raw.unit.as_deref()) {
            return Err(crate::value::error::throw_range_error("invalid unit"));
        }
        Ok(number_options(
            locale,
            raw,
            minimum_fraction_digits as f64,
            maximum_fraction_digits as f64,
        ))
    }

    fn build_object(&self) -> Value {
        let properties = vec![
            (
                "format".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlNumberFormatFormat),
            ),
            (
                "formatToParts".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlNumberFormatFormatToParts),
            ),
            (
                "formatRange".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlNumberFormatFormatRange),
            ),
            (
                "formatRangeToParts".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlNumberFormatFormatRangeToParts),
            ),
            (
                "resolvedOptions".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlNumberFormatResolvedOptions),
            ),
            (
                "\0prototype".to_string(),
                crate::vm::realm_intrinsic(crate::ops::Builtin::IntlNumberFormatPrototype),
            ),
            (SLOT.to_string(), self.slot()),
        ];
        make_object(properties)
    }

    fn slot(&self) -> Value {
        let mut properties = slot_primary(self);
        if let Some(currency) = &self.currency {
            properties.push(("currency".to_string(), Value::String(currency.clone())));
            properties.push((
                "currencyDisplay".to_string(),
                Value::String(self.currency_display.clone()),
            ));
            properties.push((
                "currencySign".to_string(),
                Value::String(self.currency_sign.clone()),
            ));
        }
        if let Some(unit) = &self.unit {
            properties.push(("unit".to_string(), Value::String(unit.clone())));
            properties.push((
                "unitDisplay".to_string(),
                Value::String(self.unit_display.clone()),
            ));
        }
        properties.extend([
            (
                "minimumIntegerDigits".to_string(),
                Value::Number(self.minimum_integer_digits as f64),
            ),
            (
                "minimumFractionDigits".to_string(),
                Value::Number(self.minimum_fraction_digits as f64),
            ),
            (
                "maximumFractionDigits".to_string(),
                Value::Number(self.maximum_fraction_digits as f64),
            ),
        ]);
        if let Some(value) = self.minimum_significant_digits {
            properties.push((
                "minimumSignificantDigits".to_string(),
                Value::Number(value as f64),
            ));
        }
        if let Some(value) = self.maximum_significant_digits {
            properties.push((
                "maximumSignificantDigits".to_string(),
                Value::Number(value as f64),
            ));
        }
        properties.push((
            "useGrouping".to_string(),
            Value::String(self.grouping.clone()),
        ));
        properties.push((
            "groupingMin2".to_string(),
            Value::Boolean(self.grouping_min2),
        ));
        properties.extend(slot_tail(self));
        make_object(properties)
    }
}

include!("number_options.rs");

include!("number_tail.rs");
