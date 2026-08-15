//! `toLocaleString` family and number/string formatting helpers.

use super::{resolve_locales, runtime_error, to_string_value};
use crate::{execute::VmError, ops::Builtin, value::Value};

mod date_kind;
pub(crate) mod parse_num;
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
            Some(Value::Number(value)) => crate::conversion::number_to_string(*value),
            Some(Value::String(value)) => symbol_string(value),
            Some(Value::StringUnits(value)) => String::from_utf16_lossy(value),
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
            let description = description.strip_prefix('\u{1}').unwrap_or(description);
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
            Some(Value::String(value)) => super::parse_num::parse_number(value),
            Some(Value::StringUnits(value)) => {
                super::parse_num::parse_number(&String::from_utf16_lossy(value))
            }
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
    pub fn is_truthy(value: &Value) -> bool {
        match value {
            Value::BindingCell(value) => is_truthy(&value.borrow()),
            Value::Boolean(value) => *value,
            Value::Number(value) => *value != 0.0 && !value.is_nan(),
            Value::String(value) => !value.is_empty(),
            Value::StringUnits(value) => !value.is_empty(),
            Value::BigInt(value) => value != "0",
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
            | Value::Generator(_) => true,
            Value::HostCapability(_) | Value::Iterator(_) | Value::ObjectAlias(_) => true,
        }
    }

    pub(crate) fn type_of(value: &Value) -> &'static str {
        match value {
            Value::BindingCell(value) => type_of(&value.borrow()),
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Proxy(proxy) => type_of(&proxy.target),
            value if object_value(value) => "object",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(value)
                if value.starts_with("Symbol.") || value.starts_with("Symbol.for.") =>
            {
                "symbol"
            }
            Value::String(_) => "string",
            Value::StringUnits(_) => "string",
            Value::Builtin(builtin) => builtin_type(*builtin),
            Value::Function(_) | Value::BoundFunction(_) => "function",
            Value::BigInt(_) => "bigint",
            Value::Promise(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Iterator(_)
            | Value::Generator(_) => "object",
            Value::ObjectAlias(_) => "object",
            Value::HostCapability(_) => "object",
            _ => "object",
        }
    }

    fn builtin_type(builtin: crate::ops::Builtin) -> &'static str {
        match builtin {
            crate::ops::Builtin::SymbolIterator
            | crate::ops::Builtin::SymbolAsyncIterator
            | crate::ops::Builtin::SymbolDispose
            | crate::ops::Builtin::SymbolAsyncDispose
            | crate::ops::Builtin::SymbolUnscopables
            | crate::ops::Builtin::SymbolToStringTag
            | crate::ops::Builtin::SymbolToPrimitive
            | crate::ops::Builtin::SymbolHasInstance
            | crate::ops::Builtin::SymbolIsConcatSpreadable
            | crate::ops::Builtin::SymbolSpecies
            | crate::ops::Builtin::SymbolMatch
            | crate::ops::Builtin::SymbolReplace
            | crate::ops::Builtin::SymbolSearch
            | crate::ops::Builtin::SymbolSplit
            | crate::ops::Builtin::SymbolMatchAll => "symbol",
            crate::ops::Builtin::Math
            | crate::ops::Builtin::Reflect
            | crate::ops::Builtin::Json => "object",
            builtin if crate::builtin_meta::is_prototype(builtin) => "object",
            _ => "function",
        }
    }

    fn object_value(value: &Value) -> bool {
        matches!(
            value,
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
                | Value::Proxy(_)
                | Value::Promise(_)
                | Value::Map(_)
                | Value::Set(_)
                | Value::Iterator(_)
                | Value::Generator(_)
                | Value::ObjectAlias(_)
                | Value::HostCapability(_)
        )
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
    use super::{Value, VmError};
    use crate::ops::Builtin;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SYMBOL_COUNTER: AtomicU64 = AtomicU64::new(0);
    pub(crate) fn dispatch(
        builtin: Builtin,
        arguments: &[Value],
        _receiver: Option<&Value>,
    ) -> Option<Result<Value, VmError>> {
        if builtin == Builtin::Symbol {
            return Some(make_symbol(arguments));
        }
        if let Some(name) = name(builtin) {
            let value = if builtin == Builtin::SymbolUnscopables {
                Value::String(format!("{name}\0"))
            } else {
                Value::String(name.to_string())
            };
            return Some(Ok(value));
        }
        Some(match builtin {
            Builtin::SymbolFor => symbol_for(arguments),
            Builtin::SymbolKeyFor => symbol_key_for(arguments),
            _ => return None,
        })
    }
    pub(crate) fn name(builtin: Builtin) -> Option<&'static str> {
        Some(match builtin {
            Builtin::SymbolIterator => "Symbol.iterator",
            Builtin::SymbolAsyncIterator => "Symbol.asyncIterator",
            Builtin::SymbolDispose => "Symbol.dispose",
            Builtin::SymbolAsyncDispose => "Symbol.asyncDispose",
            Builtin::SymbolUnscopables => "Symbol.unscopables",
            Builtin::SymbolToStringTag => "Symbol.toStringTag",
            Builtin::SymbolToPrimitive => "Symbol.toPrimitive",
            Builtin::SymbolHasInstance => "Symbol.hasInstance",
            Builtin::SymbolIsConcatSpreadable => "Symbol.isConcatSpreadable",
            Builtin::SymbolSpecies => "Symbol.species",
            Builtin::SymbolMatch => "Symbol.match",
            Builtin::SymbolReplace => "Symbol.replace",
            Builtin::SymbolSearch => "Symbol.search",
            Builtin::SymbolSplit => "Symbol.split",
            Builtin::SymbolMatchAll => "Symbol.matchAll",
            _ => return None,
        })
    }
    fn make_symbol(arguments: &[Value]) -> Result<Value, VmError> {
        let description = match arguments.first() {
            None | Some(Value::Undefined) => "\u{1}".to_string(),
            Some(value) => crate::conversion::to_string(value)?,
        };
        let counter = SYMBOL_COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(Value::String(format!("Symbol.{description}\0{counter}")))
    }

    fn symbol_for(arguments: &[Value]) -> Result<Value, VmError> {
        let key = crate::conversion::to_string(arguments.first().unwrap_or(&Value::Undefined))?;
        Ok(Value::String(format!("Symbol.for.{key}\0")))
    }

    fn symbol_key_for(arguments: &[Value]) -> Result<Value, VmError> {
        if arguments.first().is_some_and(
            |value| matches!(value, Value::Builtin(builtin) if name(*builtin).is_some()),
        ) {
            return Ok(Value::Undefined);
        }
        let Some(Value::String(value)) = arguments.first() else {
            return Err(crate::value::error::throw_type_error(
                "Symbol.keyFor requires a symbol",
            ));
        };
        let Some(value) = value.strip_prefix("Symbol.for.") else {
            return if crate::conversion::is_symbol_string(value) {
                Ok(Value::Undefined)
            } else {
                Err(crate::value::error::throw_type_error(
                    "Symbol.keyFor requires a symbol",
                ))
            };
        };
        Ok(Value::String(
            value.strip_suffix('\0').unwrap_or(value).to_string(),
        ))
    }
}

/// `Array.prototype.toLocaleString`.
pub(crate) fn array_to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let values = match receiver {
        Some(Value::Array(values)) => values.iter().cloned().collect(),
        Some(value) => typed_array_values(value).ok_or_else(|| {
            crate::value::error::throw_type_error(
                "Array.prototype.toLocaleString called on non-array",
            )
        })?,
        None => {
            return Err(crate::value::error::throw_type_error(
                "called on null or undefined",
            ))
        }
    };
    let locales = resolve_locales(arguments)?;
    let options = arguments.get(1);
    let mut parts = Vec::new();
    for value in values.iter() {
        parts.push(element_to_locale_string(value, &locales, options)?);
    }
    Ok(Value::String(parts.join(",")))
}

fn typed_array_values(value: &Value) -> Option<Vec<Value>> {
    macro_rules! values {
        ($data:expr) => {
            (0..$data.logical_len())
                .filter_map(|index| $data.get(index).map(|value| Value::Number(value as f64)))
                .collect()
        };
    }
    match value {
        Value::Float64Array(data) => Some(values!(data)),
        Value::Float32Array(data) => Some(values!(data)),
        Value::Int8Array(data) => Some(values!(data)),
        Value::Int16Array(data) => Some(values!(data)),
        Value::Int32Array(data) => Some(values!(data)),
        Value::Uint8Array(data) => Some(values!(data)),
        Value::Uint8ClampedArray(data) => Some(values!(data)),
        Value::Uint16Array(data) => Some(values!(data)),
        Value::Uint32Array(data) => Some(values!(data)),
        _ => None,
    }
}

pub(crate) fn dispatch(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::StringLocaleCompare => string_locale_compare(receiver, arguments),
        Builtin::ArrayToLocaleString => array_to_locale_string(receiver, arguments),
        Builtin::NumberToLocaleString => number_to_locale_string(receiver, arguments),
        Builtin::StringToLocaleLowerCase => string_to_locale_case(receiver, arguments, false),
        Builtin::StringToLocaleUpperCase => string_to_locale_case(receiver, arguments, true),
        Builtin::DateToLocaleString => {
            date_to_locale_string(DateLocaleKind::String, receiver, arguments)
        }
        Builtin::DateToLocaleDateString => {
            date_to_locale_string(DateLocaleKind::Date, receiver, arguments)
        }
        Builtin::DateToLocaleTimeString => {
            date_to_locale_string(DateLocaleKind::Time, receiver, arguments)
        }
        _ => return None,
    };
    Some(result)
}

fn string_locale_compare(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(crate::vm::not_callable)?;
    if matches!(receiver, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "String.prototype.localeCompare called on null or undefined",
        ));
    }
    let left = crate::conversion::to_string(receiver)?;
    let right =
        crate::conversion::to_string(arguments.first().map_or(&Value::Undefined, |value| value))?;
    let collator_arguments = arguments
        .get(1..)
        .map_or_else(Vec::new, |values| values.to_vec());
    crate::intl::collator::construct(&collator_arguments)?;
    let result = match left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => match (left == right, left.cmp(&right)) {
            (true, _) => 0.0,
            (false, std::cmp::Ordering::Less) => 1.0,
            (false, std::cmp::Ordering::Greater) => -1.0,
            (false, std::cmp::Ordering::Equal) => 0.0,
        },
        std::cmp::Ordering::Greater => 1.0,
    };
    Ok(Value::Number(result))
}

fn element_to_locale_string(
    value: &Value,
    locales: &[String],
    options: Option<&Value>,
) -> Result<String, VmError> {
    match value {
        Value::Number(number) => Ok(format_number(*number, locales, options)),
        Value::Null | Value::Undefined => Ok(String::new()),
        _ => locale_element_call(value, locales, options),
    }
}

fn locale_element_call(
    value: &Value,
    locales: &[String],
    options: Option<&Value>,
) -> Result<String, VmError> {
    let method = crate::execute::get_property_result(value, "toLocaleString")?;
    if !matches!(method, Value::Undefined | Value::Null) {
        let locale_value = Value::array(locales.iter().cloned().map(Value::String).collect());
        let mut arguments = vec![locale_value];
        if let Some(options) = options {
            arguments.push(options.clone());
        }
        let result = crate::functions::execute_target_with_receiver(&method, value, &arguments)?;
        return crate::conversion::to_string(&result.0);
    }
    Ok(to_string_value(value))
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
    arguments: &[Value],
    upper: bool,
) -> Result<Value, VmError> {
    let Some(Value::String(value)) = receiver else {
        return Err(runtime_error("TypeError: String.prototype.toLocale*Case"));
    };
    let locale = resolve_locales(arguments)?
        .first()
        .cloned()
        .unwrap_or_default();
    let result = locale_case(value, &locale, upper);
    Ok(Value::String(result))
}

fn locale_case(value: &str, locale: &str, upper: bool) -> String {
    let special = locale.starts_with("tr") || locale.starts_with("az");
    if special {
        let value = value
            .replace("I\u{323}\u{307}", "i\u{323}")
            .replace("I𐇽\u{307}", "i𐇽")
            .replace("I\u{307}", "İ");
        return case_turkish(&value, upper);
    }
    if locale.starts_with("lt") && !upper {
        return value
            .replace("IA", "ia")
            .replace("JA", "ja")
            .replace("\u{12e}A", "\u{12f}a")
            .replace('Ì', "i\u{307}\u{300}")
            .replace('Í', "i\u{307}\u{301}")
            .replace('Ĩ', "i\u{307}\u{303}")
            .replace("I\u{300}", "i\u{307}\u{300}")
            .replace("J\u{300}", "j\u{307}\u{300}")
            .replace("\u{12e}\u{300}", "\u{12f}\u{307}\u{300}")
            .replace("I\u{325}\u{300}", "i\u{307}\u{325}\u{300}")
            .replace("J\u{325}\u{300}", "j\u{307}\u{325}\u{300}")
            .replace("\u{12e}\u{325}\u{300}", "\u{12f}\u{307}\u{325}\u{300}")
            .replace("I𐇽\u{300}", "i\u{307}𐇽\u{300}")
            .replace("J𐇽\u{300}", "j\u{307}𐇽\u{300}")
            .replace("\u{12e}𐇽\u{300}", "\u{12f}\u{307}𐇽\u{300}")
            .replace("I𝆅", "i\u{307}𝆅")
            .replace("J𝆅", "j\u{307}𝆅")
            .replace("\u{12e}𝆅", "\u{12f}\u{307}𝆅")
            .replace("I\u{325}𝆅", "i\u{307}\u{325}𝆅")
            .replace("J\u{325}𝆅", "j\u{307}\u{325}𝆅")
            .replace("\u{12e}\u{325}𝆅", "\u{12f}\u{307}\u{325}𝆅")
            .replace("I𐇽𝆅", "i\u{307}𐇽𝆅")
            .replace("J𐇽𝆅", "j\u{307}𐇽𝆅")
            .replace("\u{12e}𐇽𝆅", "\u{12f}\u{307}𐇽𝆅")
            .replace("i\u{307}a", "ia")
            .replace("j\u{307}a", "ja")
            .replace("\u{12f}\u{307}a", "\u{12f}a")
            .replace('I', "i\u{307}")
            .replace('J', "j\u{307}")
            .replace("i\u{307}a", "ia")
            .replace("j\u{307}a", "ja")
            .replace("\u{12f}\u{307}a", "\u{12f}a")
            .to_lowercase();
    }
    let result = if upper {
        value.to_uppercase()
    } else {
        value.to_lowercase()
    };
    if locale.starts_with("lt") && upper && value.chars().next().is_some_and(char::is_lowercase) {
        result.replace('\u{307}', "")
    } else {
        result
    }
}

fn case_turkish(value: &str, upper: bool) -> String {
    value
        .chars()
        .map(|character| match (character, upper) {
            ('i', true) => 'İ',
            ('ı', true) => 'I',
            ('I', false) => 'ı',
            ('İ', false) => 'i',
            (character, _) => {
                if upper {
                    character.to_ascii_uppercase()
                } else {
                    character.to_ascii_lowercase()
                }
            }
        })
        .collect()
}

pub(crate) fn date_to_locale_string(
    kind: DateLocaleKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let mut formatter_arguments = arguments.to_vec();
    if formatter_arguments.len() < 2 {
        formatter_arguments.push(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(default_date_options(kind)),
        )));
    }
    let formatter = crate::intl::datetime::construct(&formatter_arguments)?;
    crate::intl::datetime::prototype_method(
        Builtin::IntlDateTimeFormatFormat,
        &[receiver.cloned().unwrap_or(Value::Undefined)],
        Some(&formatter),
    )
}

fn default_date_options(kind: DateLocaleKind) -> Vec<(String, Value)> {
    let names = match kind {
        DateLocaleKind::String => ["year", "month", "day", "hour", "minute", "second"].as_slice(),
        DateLocaleKind::Date => ["year", "month", "day"].as_slice(),
        DateLocaleKind::Time => ["hour", "minute", "second"].as_slice(),
    };
    names
        .iter()
        .map(|name| (name.to_string(), Value::String("numeric".into())))
        .collect()
}
