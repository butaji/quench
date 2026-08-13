//! `Intl.NumberFormat` and `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_bool,
    slot_number, slot_string, to_string_value, SLOT,
};

pub(crate) struct NumberOptions {
    pub locale: String,
    pub style: String,
    pub currency: Option<String>,
    pub currency_display: String,
    pub unit: Option<String>,
    pub minimum_integer_digits: u32,
    pub minimum_fraction_digits: u32,
    pub maximum_fraction_digits: u32,
    pub use_grouping: bool,
    pub notation: String,
    pub rounding_mode: String,
}

pub(crate) struct RawOptions {
    style: String,
    currency: Option<String>,
    currency_display: String,
    unit: Option<String>,
    minimum_fraction_digits: f64,
    maximum_fraction_digits: f64,
    use_grouping: bool,
    notation: String,
    rounding_mode: String,
}

impl RawOptions {
    fn from_value(options: Option<&Value>) -> Self {
        let mut raw = RawOptions {
            style: "decimal".to_string(),
            currency: None,
            currency_display: "symbol".to_string(),
            unit: None,
            minimum_fraction_digits: 0.0,
            maximum_fraction_digits: 3.0,
            use_grouping: true,
            notation: "standard".to_string(),
            rounding_mode: "halfExpand".to_string(),
        };
        if let Some(Value::Object(properties)) = options {
            for (key, value) in properties.iter() {
                let value = to_string_value(value);
                match key.as_str() {
                    "style" => raw.style = value,
                    "currency" => raw.currency = Some(value.to_ascii_uppercase()),
                    "currencyDisplay" => raw.currency_display = value,
                    "unit" => raw.unit = Some(value),
                    "minimumFractionDigits" => {
                        raw.minimum_fraction_digits = value.parse().unwrap_or(0.0)
                    }
                    "maximumFractionDigits" => {
                        raw.maximum_fraction_digits = value.parse().unwrap_or(3.0)
                    }
                    "useGrouping" => raw.use_grouping = value == "true",
                    "notation" => raw.notation = value,
                    "roundingMode" => raw.rounding_mode = value,
                    _ => {}
                }
            }
        }
        raw
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
        let minimum_fraction_digits =
            fraction_digits(raw.style.as_str(), raw.minimum_fraction_digits);
        let maximum_fraction_digits = maximum_fraction(
            &raw.style,
            &raw.currency,
            raw.maximum_fraction_digits,
            minimum_fraction_digits,
        );
        Ok(NumberOptions {
            locale,
            style: raw.style,
            currency: raw.currency,
            currency_display: raw.currency_display,
            unit: raw.unit,
            minimum_integer_digits: 1,
            minimum_fraction_digits,
            maximum_fraction_digits,
            use_grouping: raw.use_grouping,
            notation: raw.notation,
            rounding_mode: raw.rounding_mode,
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
                "resolvedOptions".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlNumberFormatResolvedOptions),
            ),
            (SLOT.to_string(), self.slot()),
        ];
        make_object(properties)
    }

    fn slot(&self) -> Value {
        let mut properties = vec![
            ("locale".to_string(), Value::String(self.locale.clone())),
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
                "roundingMode".to_string(),
                Value::String(self.rounding_mode.clone()),
            ),
        ];
        if let Some(currency) = &self.currency {
            properties.push(("currency".to_string(), Value::String(currency.clone())));
            properties.push((
                "currencyDisplay".to_string(),
                Value::String(self.currency_display.clone()),
            ));
        }
        if let Some(unit) = &self.unit {
            properties.push(("unit".to_string(), Value::String(unit.clone())));
        }
        make_object(properties)
    }
}

fn fraction_digits(style: &str, requested: f64) -> u32 {
    match style {
        "percent" => 0,
        _ => requested as u32,
    }
}

fn maximum_fraction(style: &str, currency: &Option<String>, requested: f64, minimum: u32) -> u32 {
    let default = match style {
        "currency" if currency.as_deref() == Some("JPY") => 0,
        "currency" => 2,
        _ => 3,
    };
    if requested > 0.0 {
        requested as u32
    } else {
        default.max(minimum)
    }
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
            let number = to_number_result(arguments.first())?;
            Ok(Value::String(options.format_number(number)))
        }
        crate::ops::Builtin::IntlNumberFormatFormatToParts => {
            let number = to_number_result(arguments.first())?;
            Ok(make_array(options.parts(number)))
        }
        crate::ops::Builtin::IntlNumberFormatResolvedOptions => Ok(options.resolved()),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

impl NumberOptions {
    fn from_slots(slots: &[(String, Value)]) -> Result<Self, VmError> {
        Ok(NumberOptions {
            locale: slot_string(slots, "locale").unwrap_or_else(default_locale),
            style: slot_string(slots, "style").unwrap_or_else(|| "decimal".to_string()),
            currency: slot_string(slots, "currency"),
            currency_display: slot_string(slots, "currencyDisplay")
                .unwrap_or_else(|| "symbol".to_string()),
            unit: slot_string(slots, "unit"),
            minimum_integer_digits: slot_number(slots, "minimumIntegerDigits").unwrap_or(1.0)
                as u32,
            minimum_fraction_digits: slot_number(slots, "minimumFractionDigits").unwrap_or(0.0)
                as u32,
            maximum_fraction_digits: slot_number(slots, "maximumFractionDigits").unwrap_or(3.0)
                as u32,
            use_grouping: slot_bool(slots, "useGrouping").unwrap_or(true),
            notation: slot_string(slots, "notation").unwrap_or_else(|| "standard".to_string()),
            rounding_mode: slot_string(slots, "roundingMode")
                .unwrap_or_else(|| "halfExpand".to_string()),
        })
    }

    fn format_number(&self, number: f64) -> String {
        let scaled = match self.style.as_str() {
            "percent" => number * 100.0,
            _ => number,
        };
        let magnitude = if self.notation == "compact" {
            compact_scale(scaled)
        } else {
            0
        };
        let value = if magnitude == 0 {
            scaled
        } else {
            scaled / 10f64.powi(magnitude)
        };
        let mut text = format_number_rounded(value, self.maximum_fraction_digits);
        if self.use_grouping {
            text = group_integer(&text);
        }
        text = apply_minimum_integer(&text, self.minimum_integer_digits);
        if self.minimum_fraction_digits > 0 {
            text = pad_fraction(&text, self.minimum_fraction_digits);
        }
        match self.style.as_str() {
            "percent" => text.push('%'),
            "currency" => {
                text = format_currency(&text, self.currency.as_deref(), &self.currency_display)
            }
            "unit" => text = format_unit(&text, self.unit.as_deref()),
            _ => {}
        }
        if magnitude > 0 {
            text.push_str(compact_suffix(magnitude));
        }
        text
    }

    fn parts(&self, number: f64) -> Vec<Value> {
        vec![make_object(vec![
            ("type".to_string(), Value::String("integer".to_string())),
            (
                "value".to_string(),
                Value::String(self.format_number(number)),
            ),
        ])]
    }

    fn resolved(&self) -> Value {
        make_object(vec![
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
                "roundingMode".to_string(),
                Value::String(self.rounding_mode.clone()),
            ),
        ])
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

fn format_number_rounded(value: f64, max_fraction: u32) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    let formatter = format!("{:.*}", max_fraction as usize, value);
    let mut formatter = formatter;
    if formatter.contains('.') {
        while formatter.ends_with('0') {
            formatter.pop();
        }
        if formatter.ends_with('.') {
            formatter.pop();
        }
    }
    formatter
}

fn group_integer(text: &str) -> String {
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (integer, fraction) = match rest.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (rest, None),
    };
    let mut grouped = String::new();
    let chars: Vec<char> = integer.chars().collect();
    for (index, character) in chars.iter().enumerate() {
        if index > 0 && (chars.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*character);
    }
    let mut result = format!("{sign}{grouped}");
    if let Some(fraction) = fraction {
        result.push('.');
        result.push_str(fraction);
    }
    result
}

fn apply_minimum_integer(text: &str, minimum: u32) -> String {
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (integer, fraction) = match rest.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (rest, None),
    };
    let integer: String = integer.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = integer.len();
    let mut result = String::new();
    if digits < minimum as usize {
        for _ in 0..(minimum as usize - digits) {
            result.push('0');
        }
    }
    result.push_str(&integer);
    let mut out = format!("{sign}{result}");
    if let Some(fraction) = fraction {
        out.push('.');
        out.push_str(fraction);
    }
    out
}

fn pad_fraction(text: &str, minimum: u32) -> String {
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let fraction_digits = rest
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let mut out = format!("{sign}{rest}");
    if minimum > 0 {
        if !rest.contains('.') {
            out.push('.');
        } else {
            let _ = fraction_digits;
        }
        for _ in fraction_digits..minimum as usize {
            out.push('0');
        }
    }
    out
}

fn compact_scale(value: f64) -> i32 {
    let magnitude = value.abs().log10().floor() as i32;
    if magnitude >= 9 {
        9
    } else if magnitude >= 6 {
        6
    } else if magnitude >= 3 {
        3
    } else {
        0
    }
}

fn compact_suffix(magnitude: i32) -> &'static str {
    match magnitude {
        3 => "K",
        6 => "M",
        9 => "B",
        _ => "",
    }
}

fn format_currency(text: &str, currency: Option<&str>, display: &str) -> String {
    let symbol = match display {
        "code" => currency.unwrap_or("USD"),
        "name" => currency.unwrap_or("USD"),
        _ => match currency {
            Some("USD") => "$",
            Some("EUR") => "€",
            Some("JPY") => "¥",
            Some("GBP") => "£",
            Some("CNY") => "¥",
            Some("INR") => "₹",
            Some("RUB") => "₽",
            Some("KRW") => "₩",
            _ => currency.unwrap_or("USD"),
        },
    };
    format!("{symbol}{text}")
}

fn format_unit(text: &str, unit: Option<&str>) -> String {
    match unit {
        Some("percent") => format!("{text}%"),
        Some("kilometer") => format!("{text} km"),
        _ => text.to_string(),
    }
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
        | crate::ops::Builtin::IntlNumberFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
