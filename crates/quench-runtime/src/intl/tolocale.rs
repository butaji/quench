//! `toLocaleString` family and number/string formatting helpers.

use super::{resolve_locales, runtime_error, to_string_value};
use crate::{execute::VmError, ops::Builtin, value::Value};

mod date_kind;
pub(crate) use date_kind::DateLocaleKind;

#[path = "tolocale_number.rs"]
mod number;

pub(crate) mod value {
    use crate::value::Value;
    pub(crate) fn to_string(value: Option<&Value>) -> String {
        if let Some(Value::BindingCell(value)) = value {
            return to_string(Some(&value.borrow()));
        }
        to_string_value(value)
    }
    fn to_string_value(value: Option<&Value>) -> String {
        match value {
            None | Some(Value::Undefined) => "undefined".to_string(),
            Some(value @ (Value::Null | Value::Object(_))) => object_string(value),
            Some(Value::Boolean(value)) => value.to_string(),
            Some(Value::Number(value)) => value.to_string(),
            Some(Value::String(value)) => symbol_string(value),
            Some(Value::Array(values)) => array_to_string(values),
            Some(Value::ArrayBuffer(_)) => "[object ArrayBuffer]".to_string(),
            Some(Value::DataView(_)) => "[object DataView]".to_string(),
            Some(Value::Float32Array(_)) => "[object Float32Array]".to_string(),
            Some(Value::Float64Array(_)) => "[object Float64Array]".to_string(),
            Some(Value::Int16Array(_)) => "[object Int16Array]".to_string(),
            Some(Value::Int8Array(_)) => "[object Int8Array]".to_string(),
            Some(Value::Int32Array(_)) => "[object Int32Array]".to_string(),
            Some(Value::Uint16Array(_)) => "[object Uint16Array]".to_string(),
            Some(Value::Uint32Array(_)) => "[object Uint32Array]".to_string(),
            Some(Value::Uint8Array(_)) => "[object Uint8Array]".to_string(),
            Some(Value::Uint8ClampedArray(_)) => "[object Uint8ClampedArray]".to_string(),
            Some(Value::BigInt64Array(_)) => "[object BigInt64Array]".to_string(),
            Some(Value::BigUint64Array(_)) => "[object BigUint64Array]".to_string(),
            Some(
                Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Builtin(_)
                | Value::Proxy(_)
                | Value::Promise(_)
                | Value::Map(_)
                | Value::Set(_),
            ) => "function".to_string(),
            Some(Value::BigInt(_)) => "[object BigInt]".to_string(),
            Some(
                Value::HostCapability(_)
                | Value::Iterator(_)
                | Value::Generator(_)
                | Value::ObjectAlias(_),
            ) => "[object Object]".to_string(),
            Some(Value::BindingCell(value)) => to_string(Some(&value.borrow())),
        }
    }

    fn object_string(value: &Value) -> String {
        match value {
            Value::Null => "null".to_string(),
            _ => "[object Object]".to_string(),
        }
    }

    fn array_to_string(values: &[Value]) -> String {
        values
            .iter()
            .map(|value| match value {
                Value::Null | Value::Undefined => String::new(),
                _ => to_string(Some(value)),
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn symbol_string(value: &str) -> String {
        let Some((symbol, _identity)) = value.split_once('\0') else {
            return value.to_string();
        };
        if let Some(description) = symbol.strip_prefix("Symbol.for.") {
            return format!("Symbol({description})");
        }
        if let Some(description) = symbol.strip_prefix("Symbol.") {
            return format!("Symbol({description})");
        }
        value.to_string()
    }
    pub(crate) fn to_number(value: Option<&Value>) -> f64 {
        match value {
            Some(Value::BindingCell(value)) => to_number(Some(&value.borrow())),
            None | Some(Value::Undefined) => f64::NAN,
            Some(Value::Null) => 0.0,
            Some(Value::Boolean(value)) => f64::from(*value),
            Some(Value::Number(value)) => *value,
            Some(Value::String(value)) => parse_number(value),
            Some(Value::Object(properties)) => boxed_number(properties),
            Some(
                Value::Array(_)
                | Value::ArrayBuffer(_)
                | Value::DataView(_)
                | Value::Float32Array(_)
                | Value::Float64Array(_)
                | Value::Int16Array(_)
                | Value::Int8Array(_)
                | Value::Int32Array(_)
                | Value::Uint16Array(_)
                | Value::Uint32Array(_)
                | Value::Uint8Array(_)
                | Value::Uint8ClampedArray(_)
                | Value::BigInt64Array(_)
                | Value::BigUint64Array(_)
                | Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Builtin(_)
                | Value::Proxy(_)
                | Value::Promise(_)
                | Value::Map(_)
                | Value::Set(_)
                | Value::Generator(_)
                | Value::BigInt(_)
                | Value::ObjectAlias(_),
            )
            | Some(Value::HostCapability(_))
            | Some(Value::Iterator(_)) => f64::NAN,
        }
    }
    fn boxed_number(properties: &[(String, Value)]) -> f64 {
        properties
            .iter()
            .find_map(|(key, value)| (key == "_value").then_some(value))
            .map_or(f64::NAN, |value| to_number(Some(value)))
    }
    pub(crate) fn to_number_result(value: Option<&Value>) -> Result<f64, crate::execute::VmError> {
        crate::conversion::to_number(value.unwrap_or(&Value::Undefined))
    }
    pub(crate) fn parse_number(value: &str) -> f64 {
        let value = value.trim();
        if value.is_empty() {
            return 0.0;
        }
        if matches!(value, "INFINITY" | "infinity" | "+infinity" | "-infinity") {
            return f64::NAN;
        }
        for (prefix, radix) in [("0b", 2), ("0o", 8), ("0x", 16)] {
            if let Some(digits) = value.strip_prefix(prefix) {
                return i64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
            }
        }
        value.parse().unwrap_or(f64::NAN)
    }
    pub(crate) fn parse_int(arguments: &[Value]) -> f64 {
        let text = to_string(arguments.first()).trim().to_string();
        let radix = arguments.get(1).map_or(0, |v| to_number(Some(v)) as i32);
        let radix = if radix == 0 { 10 } else { radix };
        if !(2..=36).contains(&radix) {
            return f64::NAN;
        }
        let (sign, digits) = match text.strip_prefix('-') {
            Some(value) => (-1.0, value),
            None => (1.0, text.strip_prefix('+').unwrap_or(&text)),
        };
        i64::from_str_radix(digits, radix as u32).map_or(f64::NAN, |v| sign * v as f64)
    }
    pub(crate) fn parse_float(value: Option<&Value>) -> f64 {
        to_string(value).trim().parse().unwrap_or(f64::NAN)
    }
    pub fn is_truthy(value: &Value) -> bool {
        match value {
            Value::BindingCell(value) => is_truthy(&value.borrow()),
            Value::Boolean(value) => *value,
            Value::Number(value) => *value != 0.0 && !value.is_nan(),
            Value::String(value) => !value.is_empty(),
            Value::Null | Value::Undefined => false,
            Value::Array(_)
            | Value::ArrayBuffer(_)
            | Value::DataView(_)
            | Value::Float32Array(_)
            | Value::Float64Array(_)
            | Value::Int16Array(_)
            | Value::Int8Array(_)
            | Value::Int32Array(_)
            | Value::Uint16Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Object(_)
            | Value::Builtin(_)
            | Value::Function(_)
            | Value::BoundFunction(_)
            | Value::Proxy(_)
            | Value::Promise(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Generator(_)
            | Value::BigInt(_) => true,
            Value::HostCapability(_) | Value::Iterator(_) | Value::ObjectAlias(_) => true,
        }
    }

    pub(crate) fn type_of(value: &Value) -> &'static str {
        match value {
            Value::BindingCell(value) => type_of(&value.borrow()),
            Value::Undefined => "undefined",
            Value::Null
            | Value::Array(_)
            | Value::ArrayBuffer(_)
            | Value::DataView(_)
            | Value::Float32Array(_)
            | Value::Float64Array(_)
            | Value::Int16Array(_)
            | Value::Int8Array(_)
            | Value::Int32Array(_)
            | Value::Uint16Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Object(_) => "object",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(value)
                if value.starts_with("Symbol.") || value.starts_with("Symbol.for.") =>
            {
                "symbol"
            }
            Value::String(_) => "string",
            Value::Builtin(_) | Value::Function(_) | Value::BoundFunction(_) => "function",
            Value::BigInt(_) => "bigint",
            Value::Proxy(_)
            | Value::Promise(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Iterator(_)
            | Value::Generator(_) => "object",
            Value::ObjectAlias(_) => "object",
            Value::HostCapability(_) => "object",
        }
    }

    pub(crate) fn is_finite(value: Option<&Value>) -> bool {
        matches!(value, Some(Value::Number(number)) if number.is_finite())
    }
    pub(crate) fn to_int32(value: f64) -> i32 {
        if !value.is_finite() || value == 0.0 {
            return 0;
        }
        let wrapped = value.trunc().rem_euclid(4_294_967_296.0);
        (if wrapped >= 2_147_483_648.0 {
            wrapped - 4_294_967_296.0
        } else {
            wrapped
        }) as i32
    }

    pub(crate) fn strict_equal(left: &Value, right: &Value) -> bool {
        crate::equality::strict_equal(left, right)
    }
}

/// `Symbol` global and well-known symbol helpers extracted from `execute.rs`
/// so the dispatch logic lives next to its sibling `tolocale::dispatch`.
pub(crate) mod symbol {
    use super::{to_string_value, Value, VmError};
    use crate::ops::Builtin;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SYMBOL_COUNTER: AtomicU64 = AtomicU64::new(0);
    pub(crate) fn dispatch(
        builtin: Builtin,
        arguments: &[Value],
        _receiver: Option<&Value>,
    ) -> Option<Result<Value, VmError>> {
        if builtin == Builtin::Symbol {
            return Some(Ok(make_symbol(arguments)));
        }
        if let Some(name) = name(builtin) {
            return Some(Ok(Value::String(name.to_string())));
        }
        Some(match builtin {
            Builtin::SymbolFor => Ok(symbol_for(arguments)),
            Builtin::SymbolKeyFor => Ok(symbol_key_for(arguments)),
            _ => return None,
        })
    }
    pub(crate) fn name(builtin: Builtin) -> Option<&'static str> {
        Some(match builtin {
            Builtin::SymbolIterator => "Symbol.iterator",
            Builtin::SymbolToStringTag => "Symbol.toStringTag",
            Builtin::SymbolToPrimitive => "Symbol.toPrimitive",
            Builtin::SymbolHasInstance => "Symbol.hasInstance",
            Builtin::SymbolIsConcatSpreadable => "Symbol.isConcatSpreadable",
            Builtin::SymbolSpecies => "Symbol.species",
            Builtin::SymbolMatch => "Symbol.match",
            Builtin::SymbolReplace => "Symbol.replace",
            Builtin::SymbolSearch => "Symbol.search",
            Builtin::SymbolSplit => "Symbol.split",
            _ => return None,
        })
    }
    fn make_symbol(arguments: &[Value]) -> Value {
        let description = match arguments.first() {
            None | Some(Value::Undefined) => String::new(),
            Some(value) => to_string_value(value),
        };
        let counter = SYMBOL_COUNTER.fetch_add(1, Ordering::Relaxed);
        Value::String(format!("Symbol.{description}\0{counter}"))
    }

    fn symbol_for(arguments: &[Value]) -> Value {
        Value::String(format!(
            "Symbol.for.{}\0",
            to_string_value(arguments.first().unwrap_or(&Value::Undefined))
        ))
    }

    fn symbol_key_for(arguments: &[Value]) -> Value {
        let Some(Value::String(value)) = arguments.first() else {
            return Value::Undefined;
        };
        let Some(value) = value.strip_prefix("Symbol.for.") else {
            return Value::Undefined;
        };
        Value::String(value.strip_suffix('\0').unwrap_or(value).to_string())
    }
}

/// `Array.prototype.toLocaleString`.
pub(crate) fn array_to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::String(String::new()));
    };
    let locales = resolve_locales(arguments)?;
    let options = arguments.get(1);
    let mut parts = Vec::new();
    for value in values.iter() {
        parts.push(element_to_locale_string(value, &locales, options));
    }
    Ok(Value::String(parts.join(",")))
}

pub(crate) fn dispatch(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::NumberToLocaleString => number_to_locale_string(receiver, arguments),
        Builtin::StringToLocaleLowerCase => string_to_locale_case(receiver, false),
        Builtin::StringToLocaleUpperCase => string_to_locale_case(receiver, true),
        Builtin::DateToLocaleString => date_to_locale_string(DateLocaleKind::String, arguments),
        Builtin::DateToLocaleDateString => date_to_locale_string(DateLocaleKind::Date, arguments),
        Builtin::DateToLocaleTimeString => date_to_locale_string(DateLocaleKind::Time, arguments),
        _ => return None,
    };
    Some(result)
}

fn element_to_locale_string(value: &Value, locales: &[String], options: Option<&Value>) -> String {
    match value {
        Value::Number(number) => format_number(*number, locales, options),
        Value::Null | Value::Undefined => String::new(),
        _ => to_string_value(value),
    }
}

pub(crate) fn number_to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let number = match receiver {
        Some(Value::Number(number)) => *number,
        _ => return Err(runtime_error("TypeError: Number.prototype.toLocaleString")),
    };
    let locales = resolve_locales(arguments)?;
    Ok(Value::String(format_number(
        number,
        &locales,
        arguments.get(1),
    )))
}
fn format_number(number: f64, locales: &[String], options: Option<&Value>) -> String {
    let locale = locales.first().cloned().unwrap_or_else(|| "en".to_string());
    let resolved = number_resolved(locale, options);
    number::format_resolved(number, &resolved)
}
fn number_resolved(locale: String, options: Option<&Value>) -> Vec<(String, Value)> {
    let mut properties = vec![
        ("locale".to_string(), Value::String(locale)),
        ("style".to_string(), Value::String("decimal".to_string())),
        ("useGrouping".to_string(), Value::Boolean(true)),
        ("minimumIntegerDigits".to_string(), Value::Number(1.0)),
        ("minimumFractionDigits".to_string(), Value::Number(0.0)),
        ("maximumFractionDigits".to_string(), Value::Number(3.0)),
    ];
    if let Some(Value::Object(option_map)) = options {
        for (key, value) in option_map.iter() {
            let value = to_string_value(value);
            match key.as_str() {
                "minimumFractionDigits" => {
                    if let Ok(number) = value.parse() {
                        properties
                            .push(("minimumFractionDigits".to_string(), Value::Number(number)));
                    }
                }
                "maximumFractionDigits" => {
                    if let Ok(number) = value.parse() {
                        properties
                            .push(("maximumFractionDigits".to_string(), Value::Number(number)));
                    }
                }
                _ => {}
            }
        }
    }
    properties
}

pub(crate) fn string_to_locale_case(
    receiver: Option<&Value>,
    upper: bool,
) -> Result<Value, VmError> {
    let Some(Value::String(value)) = receiver else {
        return Err(runtime_error("TypeError: String.prototype.toLocale*Case"));
    };
    let result = if upper {
        value.to_uppercase()
    } else {
        value.to_lowercase()
    };
    Ok(Value::String(result))
}

pub(crate) fn date_to_locale_string(
    kind: DateLocaleKind,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let _ = arguments;
    Ok(Value::String(kind.default().to_string()))
}
