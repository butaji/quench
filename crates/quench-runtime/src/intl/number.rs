//! `Intl.NumberFormat` and `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::number_format::*;

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
    pub trailing_zero_display: String,
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
    trailing_zero_display: String,
    sign_display: String,
    minimum_significant_digits: f64,
    maximum_significant_digits: f64,
    rounding_priority: String,
}

const OPTION_READ_ORDER: [&str; 21] = [
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
            trailing_zero_display: "auto".to_string(),
            sign_display: "auto".to_string(),
            minimum_significant_digits: -1.0,
            maximum_significant_digits: -1.0,
            rounding_priority: "auto".to_string(),
        };
        if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
            for key in OPTION_READ_ORDER {
                let value = crate::execute::get_property_result(options, key)?;
                if matches!(value, Value::Undefined) {
                    continue;
                }
                match key {
                    "style" => raw.style = crate::conversion::to_string(&value)?,
                    "currency" => {
                        raw.currency = Some(crate::conversion::to_string(&value)?.to_ascii_uppercase())
                    }
                    "currencyDisplay" => raw.currency_display = crate::conversion::to_string(&value)?,
                    "currencySign" => raw.currency_sign = crate::conversion::to_string(&value)?,
                    "unit" => raw.unit = Some(crate::conversion::to_string(&value)?),
                    "unitDisplay" => raw.unit_display = crate::conversion::to_string(&value)?,
                    "minimumFractionDigits" => {
                        raw.minimum_fraction_digits = crate::conversion::to_number(&value)?
                    }
                    "minimumIntegerDigits" => {
                        raw.minimum_integer_digits = crate::conversion::to_number(&value)?
                    }
                    "maximumFractionDigits" => {
                        raw.maximum_fraction_digits = crate::conversion::to_number(&value)?
                    }
                    "useGrouping" => {
                        let value = crate::conversion::to_string(&value)?;
                        raw.use_grouping = grouping_enabled(&value);
                        raw.grouping_min2 = value == "min2";
                    }
                    "notation" => raw.notation = crate::conversion::to_string(&value)?,
                    "compactDisplay" => raw.compact_display = crate::conversion::to_string(&value)?,
                    "roundingMode" => raw.rounding_mode = crate::conversion::to_string(&value)?,
                    "roundingIncrement" => {
                        raw.rounding_increment = crate::conversion::to_number(&value)?
                    }
                    "roundingPriority" => raw.rounding_priority = crate::conversion::to_string(&value)?,
                    "trailingZeroDisplay" => {
                        raw.trailing_zero_display = crate::conversion::to_string(&value)?
                    }
                    "signDisplay" => raw.sign_display = crate::conversion::to_string(&value)?,
                    "minimumSignificantDigits" => {
                        raw.minimum_significant_digits = crate::conversion::to_number(&value)?
                    }
                    "maximumSignificantDigits" => {
                        raw.maximum_significant_digits = crate::conversion::to_number(&value)?
                    }
                    _ => {}
                }
            }
        }
        Ok(raw)
    }
}

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales
        .first()
        .map(|locale| strip_unicode_extensions(locale))
        .unwrap_or_else(default_locale);
    let options = NumberOptions::from_options(locale, arguments.get(1))?;
    Ok(options.build_object())
}

pub(crate) fn format_with_options(
    number: f64,
    locales: &[String],
    options: Option<&Value>,
) -> Result<String, VmError> {
    let locale = locales
        .first()
        .map(|locale| strip_unicode_extensions(locale))
        .unwrap_or_else(default_locale);
    let options = NumberOptions::from_options(locale, options)?;
    Ok(options.format_number(number))
}

fn strip_unicode_extensions(locale: &str) -> String {
    locale
        .split_once("-u-")
        .map_or_else(|| locale.to_string(), |(base, _)| base.to_string())
}

impl NumberOptions {
    fn from_options(locale: String, options: Option<&Value>) -> Result<Self, VmError> {
        if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
            if matches!(options, Value::Null) {
                return Err(crate::value::error::throw_type_error(
                    "Cannot convert null to object",
                ));
            }
        }
        if let Some(Value::Object(properties)) = options {
            if let Some((_, value)) = properties.iter().find(|(key, _)| key == "numberingSystem") {
                if !matches!(value, Value::Undefined) {
                    let numbering = crate::conversion::to_string(value)?;
                    if numbering.len() < 3
                        || numbering.len() > 8
                        || !numbering.chars().all(|character| character.is_ascii_alphanumeric())
                    {
                        return Err(crate::value::error::throw_range_error(
                            "invalid numberingSystem",
                        ));
                    }
                }
            }
            if let Some((_, value)) = properties.iter().find(|(key, _)| key == "localeMatcher") {
                if !matches!(value, Value::String(value) if value == "lookup" || value == "best fit") {
                    return Err(crate::value::error::throw_range_error(
                        "invalid localeMatcher",
                    ));
                }
            }
            if let Some((_, value)) = properties.iter().find(|(key, _)| key == "style") {
                let style = to_string_value(value);
                if !matches!(style.as_str(), "decimal" | "percent" | "currency" | "unit") {
                    return Err(crate::value::error::throw_range_error("invalid style"));
                }
            }
            if let Some((_, value)) = properties
                .iter()
                .find(|(key, _)| key == "maximumSignificantDigits")
            {
                let digits = crate::conversion::to_number(value)?;
                if !digits.is_finite() || digits.fract() != 0.0 || !(1.0..=21.0).contains(&digits) {
                    return Err(crate::value::error::throw_range_error(
                        "invalid significant digits",
                    ));
                }
            }
        }
        let raw = RawOptions::from_value(options)?;
        validate_rounding_options(&raw)?;
        if !matches!(raw.unit_display.as_str(), "short" | "narrow" | "long") {
            return Err(crate::value::error::throw_range_error("invalid unitDisplay"));
        }
        if let Some(currency) = raw.currency.as_deref() {
            if currency.len() != 3 || !currency.chars().all(|character| character.is_ascii_alphabetic()) {
                return Err(crate::value::error::throw_range_error("invalid currency"));
            }
        }
        if raw.style == "currency" && raw.currency.is_none() {
            return Err(crate::value::error::throw_type_error("currency is required"));
        }
        let nonstandard_currency = raw.style == "currency" && raw.notation != "standard";
        let minimum_fraction_digits = if nonstandard_currency
            && raw.minimum_fraction_digits < 0.0
        {
            0
        } else {
            fraction_digits(
                raw.style.as_str(),
                raw.currency.as_deref(),
                raw.minimum_fraction_digits,
            )
        };
        let maximum_fraction_digits = if nonstandard_currency && raw.maximum_fraction_digits < 0.0 {
            if raw.notation == "compact" { 0 } else { 3 }
        } else {
            maximum_fraction(
                &raw.style,
                &raw.currency,
                raw.maximum_fraction_digits,
                minimum_fraction_digits,
            )
        };
        let minimum_fraction_digits = minimum_fraction_digits.min(maximum_fraction_digits);
        if raw.unit.is_some() && !valid_unit(raw.unit.as_deref()) {
            return Err(crate::value::error::throw_range_error("invalid unit"));
        }
        if raw.style == "unit" && raw.unit.is_none() {
            return Err(crate::value::error::throw_type_error("unit is required"));
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
            trailing_zero_display: raw.trailing_zero_display,
            sign_display: raw.sign_display,
            minimum_significant_digits: significant_digits(raw.minimum_significant_digits)
                .or_else(|| significant_digits(raw.maximum_significant_digits).map(|_| 1)),
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
            (
                "\0prototype".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlNumberFormatPrototype),
            ),
        ];
        make_object(properties)
    }

    fn slot(&self) -> Value {
        let mut properties = vec![
            ("locale".to_string(), Value::String(self.locale.clone())),
            ("style".to_string(), Value::String(self.style.clone())),
            ("useGrouping".to_string(), Value::Boolean(self.use_grouping)),
            (
                "groupingMin2".to_string(),
                Value::Boolean(self.grouping_min2),
            ),
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
            ("notation".to_string(), Value::String(self.notation.clone())),
            (
                "signDisplay".to_string(),
                Value::String(self.sign_display.clone()),
            ),
            (
                "roundingMode".to_string(),
                Value::String(self.rounding_mode.clone()),
            ),
            (
                "roundingPriority".to_string(),
                Value::String(self.rounding_priority.clone()),
            ),
            (
                "roundingIncrement".to_string(),
                Value::Number(self.rounding_increment as f64),
            ),
            (
                "trailingZeroDisplay".to_string(),
                Value::String(self.trailing_zero_display.clone()),
            ),
        ];
        if self.notation == "compact" {
            properties.insert(8, ("compactDisplay".to_string(), Value::String(self.compact_display.clone())));
        }
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

fn valid_unit(unit: Option<&str>) -> bool {
    let Some(unit) = unit else {
        return false;
    };
    unit.split_once("-per-").map_or_else(
        || super::UNITS.contains(&unit),
        |(numerator, denominator)| {
            super::UNITS.contains(&numerator) && super::UNITS.contains(&denominator)
        },
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

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let options = NumberOptions::from_slots(&slots)?;
    match builtin {
        crate::ops::Builtin::IntlNumberFormatFormat => {
            if let Some(Value::String(value)) = arguments.first() {
                if options.maximum_fraction_digits >= 20 && is_decimal_literal(value) {
                    return Ok(Value::String(format_decimal_literal(
                        value,
                        &options.locale,
                    )));
                }
            }
            let number = to_number_result(arguments.first())?;
            Ok(Value::String(options.format_number(number)))
        }
        crate::ops::Builtin::IntlNumberFormatFormatToParts => {
            let number = to_number_result(arguments.first())?;
            Ok(make_array(options.parts(number)))
        }
        crate::ops::Builtin::IntlNumberFormatFormatRange => {
            Ok(Value::String(options.format_range(arguments)?))
        }
        crate::ops::Builtin::IntlNumberFormatFormatRangeToParts => {
            Ok(make_array(options.range_parts(arguments)?))
        }
        crate::ops::Builtin::IntlNumberFormatResolvedOptions => Ok(options.resolved()),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn validate_rounding_options(raw: &RawOptions) -> Result<(), VmError> {
    const MODES: [&str; 9] = [
        "ceil",
        "floor",
        "expand",
        "trunc",
        "halfCeil",
        "halfFloor",
        "halfExpand",
        "halfTrunc",
        "halfEven",
    ];
    const INCREMENTS: [f64; 15] = [
        1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0,
        2000.0, 2500.0, 5000.0,
    ];
    if !MODES.contains(&raw.rounding_mode.as_str()) {
        return Err(crate::value::error::throw_range_error("invalid roundingMode"));
    }
    if !raw.rounding_increment.is_finite() || !INCREMENTS.contains(&raw.rounding_increment) {
        return Err(crate::value::error::throw_range_error(
            "invalid roundingIncrement",
        ));
    }
    if raw.rounding_increment != 1.0
        && (raw.rounding_priority != "auto"
            || raw.minimum_significant_digits >= 0.0
            || raw.maximum_significant_digits >= 0.0)
    {
        return Err(crate::value::error::throw_type_error(
            "roundingIncrement conflicts with rounding precision",
        ));
    }
    if raw.rounding_increment != 1.0
        && raw.minimum_fraction_digits >= 0.0
        && raw.maximum_fraction_digits >= 0.0
        && raw.minimum_fraction_digits != raw.maximum_fraction_digits
    {
        return Err(crate::value::error::throw_range_error(
            "roundingIncrement requires equal fraction digits",
        ));
    }
    if raw.trailing_zero_display != "auto" && raw.trailing_zero_display != "stripIfInteger" {
        return Err(crate::value::error::throw_range_error(
            "invalid trailingZeroDisplay",
        ));
    }
    Ok(())
}

impl NumberOptions {
    fn from_slots(slots: &[(String, Value)]) -> Result<Self, VmError> {
        Ok(NumberOptions {
            locale: slot_string(slots, "locale").unwrap_or_else(default_locale),
            style: slot_string(slots, "style").unwrap_or_else(|| "decimal".to_string()),
            currency: slot_string(slots, "currency"),
            currency_display: slot_string(slots, "currencyDisplay")
                .unwrap_or_else(|| "symbol".to_string()),
            currency_sign: slot_string(slots, "currencySign")
                .unwrap_or_else(|| "standard".to_string()),
            unit: slot_string(slots, "unit"),
            unit_display: slot_string(slots, "unitDisplay").unwrap_or_else(|| "short".to_string()),
            minimum_integer_digits: slot_number(slots, "minimumIntegerDigits").unwrap_or(1.0)
                as u32,
            minimum_fraction_digits: slot_number(slots, "minimumFractionDigits").unwrap_or(0.0)
                as u32,
            maximum_fraction_digits: slot_number(slots, "maximumFractionDigits").unwrap_or(3.0)
                as u32,
            use_grouping: slot_bool(slots, "useGrouping").unwrap_or(true),
            grouping_min2: slot_bool(slots, "groupingMin2").unwrap_or(false),
            notation: slot_string(slots, "notation").unwrap_or_else(|| "standard".to_string()),
            compact_display: slot_string(slots, "compactDisplay")
                .unwrap_or_else(|| "short".to_string()),
            rounding_mode: slot_string(slots, "roundingMode")
                .unwrap_or_else(|| "halfExpand".to_string()),
            rounding_priority: slot_string(slots, "roundingPriority")
                .unwrap_or_else(|| "auto".to_string()),
            rounding_increment: slot_number(slots, "roundingIncrement").unwrap_or(1.0) as u32,
            trailing_zero_display: slot_string(slots, "trailingZeroDisplay")
                .unwrap_or_else(|| "auto".to_string()),
            sign_display: slot_string(slots, "signDisplay").unwrap_or_else(|| "auto".to_string()),
            minimum_significant_digits: slot_number(slots, "minimumSignificantDigits")
                .map(|v| v as u32),
            maximum_significant_digits: slot_number(slots, "maximumSignificantDigits")
                .map(|v| v as u32),
        })
    }

    fn format_number(&self, number: f64) -> String {
        let scaled = match self.style.as_str() {
            "percent" => number * 100.0,
            _ => number,
        };
        let scientific = match self.notation.as_str() {
            "scientific" => Some(scientific_parts(scaled, false)),
            "engineering" => Some(scientific_parts(scaled, true)),
            _ => None,
        };
        let magnitude = if self.notation == "compact" {
            compact_scale(scaled, &self.locale, &self.compact_display)
        } else {
            0
        };
        let value = if let Some((coefficient, _)) = scientific {
            coefficient
        } else if magnitude == 0 {
            scaled
        } else {
            scaled / 10f64.powi(magnitude)
        };
        let compact_unscaled_de = self.notation == "compact"
            && self.locale.starts_with("de")
            && magnitude == 0
            && scaled.abs() >= 1_000.0;
        let fraction_digits = if self.notation == "compact" && !compact_unscaled_de {
            compact_fraction_digits(value)
        } else {
            self.maximum_fraction_digits
        };
        let fraction_text = format_number_rounded(value, fraction_digits, self.rounding_increment);
        let (mut text, significant_selected) =
            if let Some(maximum) = self.maximum_significant_digits {
                let significant_text = format_significant(
                    value,
                    self.minimum_significant_digits.unwrap_or(1),
                    maximum,
                    &self.rounding_mode,
                );
                let selected = match self.rounding_priority.as_str() {
                    "morePrecision"
                        if decimal_places(&fraction_text) > decimal_places(&significant_text) =>
                    {
                        (fraction_text, false)
                    }
                    "lessPrecision"
                        if decimal_places(&fraction_text) < decimal_places(&significant_text) =>
                    {
                        (fraction_text, false)
                    }
                    _ => (significant_text, true),
                };
                (selected.0, selected.1)
            } else {
                (fraction_text, false)
            };
        if scientific.is_none()
            && self.use_grouping
            && (!self.grouping_min2 || scaled.abs() >= 10_000.0)
            && (self.notation != "compact" || (compact_unscaled_de && scaled.abs() >= 10_000.0))
        {
            text = group_integer_locale(&text, &self.locale);
        } else if self.notation == "compact" && self.locale.starts_with("de") {
            text = text.replace('.', ",");
        }
        if let Some((_, exponent)) = scientific.filter(|(value, _)| value.is_finite()) {
            if self.locale.starts_with("de") {
                text = text.replace('.', ",");
            }
            let exponent = format!("E{exponent}");
            text.push_str(&exponent);
        }
        text = apply_minimum_integer(&text, self.minimum_integer_digits);
        if self.minimum_fraction_digits > 0 && !significant_selected {
            text = pad_locale_fraction(
                &text,
                self.minimum_fraction_digits,
                &self.locale,
            );
        }
        let negative = text.starts_with('-');
        if number.is_nan() && self.locale.starts_with("zh") {
            text = "非數值".to_string();
        }
        let zero = number == 0.0;
        let rounded_zero = text
            .trim_start_matches('-')
            .chars()
            .all(|character| matches!(character, '0' | '.' | ','));
        let hide_negative = self.sign_display == "never"
            || (self.sign_display == "auto"
                && zero
                && self.style == "currency"
                && self.currency_sign != "accounting")
            || (self.sign_display == "exceptZero" && rounded_zero)
            || (self.sign_display == "negative" && rounded_zero);
        if hide_negative && negative {
            text.remove(0);
        } else if !negative
            && (!number.is_nan() || self.sign_display == "always")
            && (self.sign_display == "always"
                || (self.sign_display == "exceptZero" && !rounded_zero))
        {
            text.insert(0, '+');
        }
        match self.style.as_str() {
            "percent" => text.push('%'),
            "currency" => {
                text = format_currency(
                    &text,
                    self.currency.as_deref(),
                    &self.currency_display,
                    &self.locale,
                    &self.currency_sign,
                )
            }
            "unit" => {
                text = format_localized_unit(
                    &text,
                    self.unit.as_deref(),
                    &self.unit_display,
                    &self.locale,
                )
            }
            _ => {}
        }
        if magnitude > 0 {
            text.push_str(compact_suffix(
                magnitude,
                &self.locale,
                &self.compact_display,
            ));
        }
        text
    }

    fn parts(&self, number: f64) -> Vec<Value> {
        let formatted = self.format_number(number);
        if self.style == "currency" {
            return currency_parts(
                &formatted,
                self.currency.as_deref(),
                &self.currency_display,
                &self.locale,
            );
        }
        if self.style == "unit" {
            if self.locale.starts_with("ja")
                && self.unit == Some("kilometer-per-hour".to_string())
                && self.unit_display == "long"
            {
                return japanese_speed_parts(&formatted);
            }
            return unit_parts(
                &formatted,
                self.unit.as_deref(),
                &self.unit_display,
                &self.locale,
            );
        }
        if self.style == "percent" {
            let mut parts = numeric_parts(
                formatted.strip_suffix('%').unwrap_or(&formatted),
                &self.locale,
            );
            parts.push(crate::intl::number_format::percent_part());
            return parts;
        }
        if self.style == "decimal" && self.unit.is_none() {
            if number.is_infinite()
                || number.is_nan()
                || (formatted.starts_with(['-', '+']) && !formatted.contains('.'))
            {
                return numeric_parts(&formatted, &self.locale);
            }
        }
        numeric_parts(&self.format_number(number), &self.locale)
    }

    fn range_values(&self, arguments: &[Value]) -> Result<(f64, f64), VmError> {
        let start = range_value(arguments.first())?;
        let end = range_value(arguments.get(1))?;
        if start.is_nan() || end.is_nan() {
            return Err(crate::value::error::throw_range_error(
                "Invalid number range",
            ));
        }
        if start > end {
            return Err(crate::value::error::throw_range_error(
                "Number range start is greater than end",
            ));
        }
        Ok((start, end))
    }

    fn format_range(&self, arguments: &[Value]) -> Result<String, VmError> {
        if let Some(result) = self.format_string_range(arguments) {
            return result;
        }
        let (start, end) = self.range_values(arguments)?;
        let first = self.format_number(start);
        let second = self.format_number(end);
        Ok(if first == second {
            if start == end {
                first
            } else {
                format!("~{first}")
            }
        } else if self.locale.starts_with("pt") && self.style == "currency" {
            let first = strip_currency_suffix(&first);
            let second = strip_positive_sign(&second);
            let separator = " - ";
            format!("{first}{separator}{second}")
        } else if self.style == "currency" {
            if first.contains('.') || first.contains(',') {
                let second = strip_currency_prefix(&second, self.currency.as_deref());
                format!("{first}–{second}")
            } else {
                format!("{first} – {second}")
            }
        } else {
            format!("{first} – {second}")
        })
    }

    fn format_string_range(&self, arguments: &[Value]) -> Option<Result<String, VmError>> {
        let (Some(Value::String(start)), Some(Value::String(end))) =
            (arguments.first(), arguments.get(1))
        else {
            return None;
        };
        if !is_decimal_integer(start) || !is_decimal_integer(end) {
            return None;
        }
        if decimal_integer_greater(start, end) {
            return Some(Err(crate::value::error::throw_range_error(
                "Number range start is greater than end",
            )));
        }
        let first = if self.locale.starts_with("pt") {
            group_integer_locale(start, "pt")
        } else {
            group_integer_locale(start, &self.locale)
        };
        let second = if self.locale.starts_with("pt") {
            group_integer_locale(end, "pt")
        } else {
            group_integer_locale(end, &self.locale)
        };
        let separator = if self.locale.starts_with("pt") {
            " - "
        } else {
            "–"
        };
        Some(Ok(format!("{first}{separator}{second}")))
    }

    fn range_parts(&self, arguments: &[Value]) -> Result<Vec<Value>, VmError> {
        let (start, end) = self.range_values(arguments)?;
        let mut parts = self.parts(start);
        if start != end {
            let separator = if self.locale.starts_with("pt") && self.style == "currency" {
                " - "
            } else if self.style == "currency" {
                "–"
            } else {
                " – "
            };
            parts.push(make_object(vec![
                ("type".to_string(), Value::String("literal".to_string())),
                ("value".to_string(), Value::String(separator.to_string())),
            ]));
            parts.extend(self.parts(end));
        }
        Ok(parts)
    }

    fn resolved(&self) -> Value {
        let mut properties = vec![
            ("locale".to_string(), Value::String(self.locale.clone())),
            (
                "numberingSystem".to_string(),
                Value::String("latn".to_string()),
            ),
            ("style".to_string(), Value::String(self.style.clone())),
            ("useGrouping".to_string(), Value::Boolean(self.use_grouping)),
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
            ("notation".to_string(), Value::String(self.notation.clone())),
            (
                "signDisplay".to_string(),
                Value::String(self.sign_display.clone()),
            ),
            (
                "roundingMode".to_string(),
                Value::String(self.rounding_mode.clone()),
            ),
            (
                "roundingIncrement".to_string(),
                Value::Number(self.rounding_increment as f64),
            ),
            (
                "trailingZeroDisplay".to_string(),
                Value::String(self.trailing_zero_display.clone()),
            ),
        ];
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
        if self.notation == "compact" {
            properties.insert(8, ("compactDisplay".to_string(), Value::String(self.compact_display.clone())));
        }
        make_object(properties)
    }
}

fn japanese_speed_parts(formatted: &str) -> Vec<Value> {
    let Some(number) = formatted
        .strip_prefix("時速 ")
        .and_then(|value| value.strip_suffix(" キロメートル"))
    else {
        return numeric_parts(formatted, "ja-JP");
    };
    let mut parts = vec![make_object(vec![
        ("type".to_string(), Value::String("unit".to_string())),
        ("value".to_string(), Value::String("時速".to_string())),
        (
            "unit".to_string(),
            Value::String("kilometer-per-hour".to_string()),
        ),
    ])];
    parts.push(make_object(vec![
        ("type".to_string(), Value::String("literal".to_string())),
        ("value".to_string(), Value::String(" ".to_string())),
    ]));
    parts.extend(numeric_parts(number, "ja-JP"));
    parts.push(make_object(vec![
        ("type".to_string(), Value::String("literal".to_string())),
        ("value".to_string(), Value::String(" ".to_string())),
    ]));
    parts.push(make_object(vec![
        ("type".to_string(), Value::String("unit".to_string())),
        (
            "value".to_string(),
            Value::String("キロメートル".to_string()),
        ),
        (
            "unit".to_string(),
            Value::String("kilometer-per-hour".to_string()),
        ),
    ]));
    parts
}

fn pad_locale_fraction(text: &str, minimum: u32, locale: &str) -> String {
    if !locale.starts_with("de") && !locale.starts_with("pt") {
        return pad_fraction(text, minimum);
    }
    let (sign, rest) = text
        .strip_prefix(['-', '+'])
        .map_or(("", text), |rest| (&text[..1], rest));
    let fraction_digits = rest.split_once(',').map_or(0, |(_, fraction)| fraction.len());
    if fraction_digits >= minimum as usize {
        return text.to_string();
    }
    let mut result = format!("{sign}{rest}");
    if fraction_digits == 0 {
        result.push(',');
    }
    result.extend(std::iter::repeat('0').take(minimum as usize - fraction_digits));
    result
}

fn format_localized_unit(text: &str, unit: Option<&str>, display: &str, locale: &str) -> String {
    if unit != Some("kilometer-per-hour") {
        return format_unit(text, unit, display);
    }
    let (prefix, suffix) = match (locale, display) {
        (locale, "long") if locale.starts_with("ja") => ("時速 ", " キロメートル"),
        (locale, "long") if locale.starts_with("ko") => ("시속 ", "킬로미터"),
        (locale, "long") if locale.starts_with("zh-TW") => ("每小時 ", " 公里"),
        (locale, "narrow") if locale.starts_with("zh-TW") => ("", "公里/小時"),
        (locale, _) if locale.starts_with("zh-TW") => ("", " 公里/小時"),
        (locale, "long") if locale.starts_with("de") => ("", " Kilometer pro Stunde"),
        (locale, "long") if locale.starts_with("en") => ("", " kilometers per hour"),
        (locale, _) if locale.starts_with("ko") => ("", "km/h"),
        _ => ("", " km/h"),
    };
    let text = if locale.starts_with("de") {
        text.replace('.', ",")
    } else {
        text.to_string()
    };
    if display == "narrow" && !locale.starts_with("de") {
        format!("{prefix}{text}{}", suffix.trim_start())
    } else {
        format!("{prefix}{text}{suffix}")
    }
}

fn is_decimal_literal(value: &str) -> bool {
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    !integer.is_empty()
        && integer.chars().all(|c| c.is_ascii_digit())
        && fraction.chars().all(|c| c.is_ascii_digit())
}

fn decimal_places(value: &str) -> usize {
    value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len())
}

fn format_decimal_literal(value: &str, locale: &str) -> String {
    let (sign, body) = value
        .strip_prefix('-')
        .map_or(("", value), |rest| ("-", rest));
    let (integer, fraction) = body.split_once('.').unwrap_or((body, ""));
    let integer = group_integer_locale(integer, locale);
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        let decimal = if locale.starts_with("de") || locale.starts_with("pt") {
            ","
        } else {
            "."
        };
        format!("{sign}{integer}{decimal}{fraction}")
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn to_number_result(value: Option<&Value>) -> Result<f64, VmError> {
    crate::conversion::to_number(value.unwrap_or(&Value::Undefined))
}

pub(crate) fn to_number(value: Option<&Value>) -> f64 {
    to_number_result(value).unwrap_or(f64::NAN)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlNumberFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlNumberFormatFormat
        | crate::ops::Builtin::IntlNumberFormatFormatToParts
        | crate::ops::Builtin::IntlNumberFormatFormatRange
        | crate::ops::Builtin::IntlNumberFormatFormatRangeToParts
        | crate::ops::Builtin::IntlNumberFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
