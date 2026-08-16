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
    pub trailing_zero_display: String,
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

impl RawOptions {
    fn from_value(options: Option<&Value>) -> Result<Self, VmError> {
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
            trailing_zero_display: "auto".to_string(),
        };
        if let Some(options) = options.filter(|value| crate::value::is_object(value)) {
            for key in OPTION_KEYS {
                let value = crate::execute::get_property_result(options, key)?;
                if !matches!(value, Value::Undefined) {
                    let text = if matches!(*key, "roundingMode" | "trailingZeroDisplay") {
                        crate::conversion::to_string(&value)?
                    } else if *key == "roundingIncrement" {
                        crate::conversion::to_number(&value)?.to_string()
                    } else {
                        to_string_value(&value)
                    };
                    if matches!(*key, "minimumFractionDigits" | "maximumFractionDigits") {
                        let digits = text.parse::<f64>().unwrap_or(f64::NAN);
                        if !digits.is_finite()
                            || digits.fract() != 0.0
                            || !(0.0..=100.0).contains(&digits)
                        {
                            return Err(crate::value::error::throw_range_error(
                                "fraction digits out of range",
                            ));
                        }
                    }
                    if *key == "numberingSystem" {
                        let _ = super::locale::calendar_option(&text)?;
                    }
                    if *key == "trailingZeroDisplay"
                        && !matches!(text.as_str(), "auto" | "stripIfInteger")
                    {
                        return Err(crate::value::error::throw_range_error(
                            "invalid trailingZeroDisplay",
                        ));
                    }
                    apply_option(&mut raw, key, &text);
                }
            }
        }
        Ok(raw)
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
            raw.use_grouping = grouping_enabled(value);
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
    validate_options(arguments.get(1))?;
    let options = NumberOptions::from_options(locale, arguments.get(1))?;
    Ok(options.build_object())
}

pub(crate) fn validate_options(options: Option<&Value>) -> Result<(), VmError> {
    let Some(value @ Value::Object(_)) = options else {
        return Ok(());
    };
    let property = |key: &str| crate::vm::get_property_result(value, key);
    let style = property("style")?;
    let style = crate::conversion::to_string(&style)?;
    if !matches!(
        style.as_str(),
        "undefined" | "decimal" | "currency" | "percent" | "unit"
    ) {
        return Err(runtime_error("RangeError: style"));
    }
    let matcher = property("localeMatcher")?;
    if crate::conversion::to_string(&matcher)? == "null" {
        return Err(runtime_error("TypeError: localeMatcher"));
    }
    let currency = property("currency")?;
    let currency = crate::conversion::to_string(&currency)?;
    if style == "currency" && currency == "undefined" {
        return Err(runtime_error("TypeError: currency"));
    }
    if currency != "undefined"
        && (currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()))
    {
        return Err(runtime_error("RangeError: currency"));
    }
    let significant = property("maximumSignificantDigits")?;
    if crate::conversion::to_string(&significant)? == "-Infinity" {
        return Err(runtime_error("RangeError: maximumSignificantDigits"));
    }
    Ok(())
}

impl NumberOptions {
    fn from_options(locale: String, options: Option<&Value>) -> Result<Self, VmError> {
        let raw = RawOptions::from_value(options)?;
        validate_unit_display(&raw.unit_display)?;
        validate_rounding_mode(&raw.rounding_mode)?;
        validate_rounding_increment(&raw)?;
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
            properties.push(("unit".to_string(), Value::String(unit.clone())));
            properties.push((
                "unitDisplay".to_string(),
                Value::String(self.unit_display.clone()),
            ));
        }
        make_object(properties)
    }
}

fn validate_rounding_mode(value: &str) -> Result<(), VmError> {
    let valid = matches!(
        value,
        "ceil"
            | "floor"
            | "expand"
            | "trunc"
            | "halfCeil"
            | "halfFloor"
            | "halfExpand"
            | "halfTrunc"
            | "halfEven"
    );
    valid
        .then_some(())
        .ok_or_else(|| crate::value::error::throw_range_error("invalid roundingMode"))
}

fn validate_rounding_increment(raw: &RawOptions) -> Result<(), VmError> {
    if raw.rounding_increment != 1.0
        && (!raw.rounding_increment.is_finite()
            || raw.rounding_increment.fract() != 0.0
            || !matches!(
                raw.rounding_increment as u32,
                1 | 2 | 5 | 10 | 20 | 25 | 50 | 100 | 200 | 250 | 500 | 1000 | 2000 | 2500 | 5000
            ))
    {
        return Err(crate::value::error::throw_range_error(
            "invalid roundingIncrement",
        ));
    }
    if raw.rounding_increment == 1.0 {
        return Ok(());
    }
    if raw.minimum_fraction_digits >= 0.0
        && raw.maximum_fraction_digits >= 0.0
        && raw.minimum_fraction_digits != raw.maximum_fraction_digits
    {
        return Err(crate::value::error::throw_range_error(
            "roundingIncrement requires equal fraction digits",
        ));
    }
    if raw.rounding_priority != "auto"
        || raw.minimum_significant_digits >= 0.0
        || raw.maximum_significant_digits >= 0.0
    {
        return Err(crate::value::error::throw_type_error(
            "roundingIncrement requires fraction-digit rounding",
        ));
    }
    Ok(())
}

fn number_options(
    locale: String,
    raw: RawOptions,
    minimum_fraction_digits: f64,
    maximum_fraction_digits: f64,
) -> NumberOptions {
    NumberOptions {
        locale,
        style: raw.style,
        currency: raw.currency,
        currency_display: raw.currency_display,
        currency_sign: raw.currency_sign,
        unit: raw.unit,
        unit_display: raw.unit_display,
        minimum_integer_digits: raw.minimum_integer_digits.max(1.0) as u32,
        minimum_fraction_digits: minimum_fraction_digits as u32,
        maximum_fraction_digits: maximum_fraction_digits as u32,
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
        trailing_zero_display: raw.trailing_zero_display,
    }
}

fn slot_base(number: &NumberOptions) -> Vec<(String, Value)> {
    let mut properties = slot_primary(number);
    properties.extend([
        (
            "notation".to_string(),
            Value::String(number.notation.clone()),
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
        (
            "trailingZeroDisplay".to_string(),
            Value::String(number.trailing_zero_display.clone()),
        ),
    ]);
    if number.notation == "compact" {
        properties.push((
            "compactDisplay".to_string(),
            Value::String(number.compact_display.clone()),
        ));
    }
    properties
}

fn slot_primary(number: &NumberOptions) -> Vec<(String, Value)> {
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
    let Some(unit) = unit else { return false };
    if super::supported_values::UNITS.contains(&unit) {
        return true;
    }
    let Some((left, right)) = unit.split_once("-per-") else {
        return false;
    };
    super::supported_values::UNITS.contains(&left)
        && super::supported_values::UNITS.contains(&right)
}

fn validate_unit_display(value: &str) -> Result<(), VmError> {
    matches!(value, "short" | "narrow" | "long")
        .then_some(())
        .ok_or_else(|| crate::value::error::throw_range_error("invalid unitDisplay"))
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

fn strip_positive_sign(text: &str) -> String {
    text.strip_prefix('+').unwrap_or(text).to_string()
}
