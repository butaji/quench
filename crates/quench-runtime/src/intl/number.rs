//! `Intl.NumberFormat` and `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::number_format::*;

mod number_methods;
mod number_render;

pub(crate) use number_methods::prototype_method;
pub(crate) use number_render::*;

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_bool,
    slot_number, slot_string, to_string_value, SLOT,
};

pub(crate) struct NumberOptions {
    pub locale: String,
    pub style: String,
    pub currency: Option<String>,
    pub currency_display: String,
    pub currency_sign: String,
    pub unit: Option<String>,
    pub unit_display: String,
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
}

pub(crate) struct RawOptions {
    style: String,
    currency: Option<String>,
    currency_display: String,
    currency_sign: String,
    unit: Option<String>,
    unit_display: String,
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
}

impl RawOptions {
    fn from_value(options: Option<&Value>) -> Self {
        let mut raw = RawOptions {
            style: "decimal".to_string(),
            currency: None,
            currency_display: "symbol".to_string(),
            currency_sign: "standard".to_string(),
            unit: None,
            unit_display: "short".to_string(),
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
        };
        if let Some(Value::Object(properties)) = options {
            for (key, value) in properties.iter() {
                let value = to_string_value(value);
                apply_option(&mut raw, key, &value);
            }
        }
        raw
    }
}

fn apply_option(raw: &mut RawOptions, key: &str, value: &str) {
    match key {
        "style" => raw.style = value.to_string(),
        "currency" => raw.currency = Some(value.to_ascii_uppercase()),
        "currencyDisplay" => raw.currency_display = value.to_string(),
        "currencySign" => raw.currency_sign = value.to_string(),
        "unit" => raw.unit = Some(value.to_string()),
        "unitDisplay" => raw.unit_display = value.to_string(),
        "minimumFractionDigits" => raw.minimum_fraction_digits = value.parse().unwrap_or(0.0),
        "minimumIntegerDigits" => raw.minimum_integer_digits = value.parse().unwrap_or(1.0),
        "maximumFractionDigits" => raw.maximum_fraction_digits = value.parse().unwrap_or(3.0),
        "useGrouping" => {
            raw.use_grouping = grouping_enabled(&value);
            raw.grouping_min2 = value == "min2";
        }
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
        _ => {}
    }
}

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let options = NumberOptions::from_options(locale, arguments.get(1))?;
    Ok(options.build_object())
}

impl NumberOptions {
    fn from_options(locale: String, options: Option<&Value>) -> Result<Self, VmError> {
        let raw = RawOptions::from_value(options);
        let minimum_fraction_digits = fraction_digits(
            raw.style.as_str(),
            raw.currency.as_deref(),
            raw.minimum_fraction_digits,
        );
        let maximum_fraction_digits = maximum_fraction(
            &raw.style,
            &raw.currency,
            raw.maximum_fraction_digits,
            minimum_fraction_digits,
        );
        let minimum_fraction_digits = minimum_fraction_digits.min(maximum_fraction_digits);
        if raw.style == "unit" && !valid_unit(raw.unit.as_deref()) {
            return Err(crate::value::error::throw_range_error("invalid unit"));
        }
        Ok(NumberOptions {
            locale,
            style: raw.style,
            currency: raw.currency,
            currency_display: raw.currency_display,
            currency_sign: raw.currency_sign,
            unit: raw.unit,
            unit_display: raw.unit_display,
            minimum_integer_digits: raw.minimum_integer_digits.max(1.0) as u32,
            minimum_fraction_digits,
            maximum_fraction_digits,
            use_grouping: raw.use_grouping,
            grouping_min2: raw.grouping_min2,
            notation: raw.notation,
            compact_display: raw.compact_display,
            rounding_mode: raw.rounding_mode,
            rounding_increment: raw.rounding_increment.max(1.0) as u32,
            sign_display: raw.sign_display,
            minimum_significant_digits: significant_digits(raw.minimum_significant_digits),
            maximum_significant_digits: significant_digits(raw.maximum_significant_digits)
                .or_else(|| significant_digits(raw.minimum_significant_digits).map(|_| 21)),
            rounding_priority: raw.rounding_priority,
        })
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
            (SLOT.to_string(), self.slot()),
        ];
        make_object(properties)
    }

    fn slot(&self) -> Value {
        let mut properties = slot_base(self);
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
            properties.push(("unit".to_string(), Value::String(self.unit.clone())));
            properties.push((
                "unitDisplay".to_string(),
                Value::String(self.unit_display.clone()),
            ));
        }
        make_object(properties)
    }
}

fn slot_base(number: &NumberFormat) -> Vec<(String, Value)> {
    let mut properties = slot_primary(number);
    properties.extend([
        (
            "notation".to_string(),
            Value::String(number.notation.clone()),
        ),
        (
            "compactDisplay".to_string(),
            Value::String(number.compact_display.clone()),
        ),
        (
            "signDisplay".to_string(),
            Value::String(number.sign_display.clone()),
        ),
        (
            "roundingMode".to_string(),
            Value::String(number.rounding_mode.clone()),
        ),
        (
            "roundingPriority".to_string(),
            Value::String(number.rounding_priority.clone()),
        ),
        (
            "roundingIncrement".to_string(),
            Value::Number(number.rounding_increment as f64),
        ),
    ]);
    properties
}

fn slot_primary(number: &NumberFormat) -> Vec<(String, Value)> {
    vec![
        ("locale".to_string(), Value::String(number.locale.clone())),
        ("style".to_string(), Value::String(number.style.clone())),
        (
            "useGrouping".to_string(),
            Value::Boolean(number.use_grouping),
        ),
        (
            "groupingMin2".to_string(),
            Value::Boolean(number.grouping_min2),
        ),
        (
            "minimumIntegerDigits".to_string(),
            Value::Number(number.minimum_integer_digits as f64),
        ),
        (
            "minimumFractionDigits".to_string(),
            Value::Number(number.minimum_fraction_digits as f64),
        ),
        (
            "maximumFractionDigits".to_string(),
            Value::Number(number.maximum_fraction_digits as f64),
        ),
    ]
}

fn valid_unit(unit: Option<&str>) -> bool {
    matches!(
        unit,
        Some("percent" | "meter" | "kilometer" | "kilometer-per-hour")
    )
}

fn grouping_enabled(value: &str) -> bool {
    matches!(value, "true" | "always" | "auto" | "min2")
}

fn fraction_digits(style: &str, currency: Option<&str>, requested: f64) -> u32 {
    if requested >= 0.0 {
        return requested as u32;
    }
    match style {
        "percent" => 0,
        "currency" if currency == Some("JPY") => 0,
        "currency" => 2,
        _ => requested as u32,
    }
}

fn significant_digits(value: f64) -> Option<u32> {
    (value >= 1.0).then_some(value as u32)
}

fn maximum_fraction(style: &str, currency: &Option<String>, requested: f64, minimum: u32) -> u32 {
    let default = match style {
        "currency" if currency.as_deref() == Some("JPY") => 0,
        "currency" => 2,
        _ => 3,
    };
    if requested >= 0.0 {
        requested as u32
    } else {
        default.max(minimum)
    }
}

fn range_value(value: Option<&Value>) -> Result<f64, VmError> {
    match value {
        None | Some(Value::Undefined) => Err(crate::value::error::throw_type_error(
            "Number range argument is undefined",
        )),
        Some(Value::BigInt(value)) => value
            .parse::<f64>()
            .map_err(|_| crate::value::error::throw_range_error("Number range is out of range")),
        Some(value) => crate::conversion::to_number(value),
    }
}

fn strip_currency_prefix(text: &str, currency: Option<&str>) -> String {
    let symbols = ["$", "€", "¥", "£", "₹", "₽", "₩"];
    let (sign, mut result) = if let Some(rest) = text.strip_prefix('+') {
        ("", rest.to_string())
    } else if let Some(rest) = text.strip_prefix('-') {
        ("-", rest.to_string())
    } else {
        ("", text.to_string())
    };
    for symbol in symbols {
        if result.starts_with(symbol) {
            result = result[symbol.len()..].to_string();
            break;
        }
    }
    let _ = currency;
    format!("{sign}{result}")
}

fn strip_currency_suffix(text: &str) -> String {
    text.rsplit_once('\u{a0}')
        .map_or_else(|| text.to_string(), |(number, _)| number.to_string())
}

fn is_decimal_integer(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn decimal_integer_greater(first: &str, second: &str) -> bool {
    let first = first.trim_start_matches('0');
    let second = second.trim_start_matches('0');
    first.len() > second.len() || (first.len() == second.len() && first > second)
}

fn strip_positive_sign(text: &str) -> String {
    text.strip_prefix('+').unwrap_or(text).to_string()
}
