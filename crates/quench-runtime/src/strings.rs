use crate::value::Value;

/// The ECMAScript length of `s`: its count of UTF-16 code units.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The UTF-16 code unit at `index`, if any.
pub(crate) fn utf16_code_unit(s: &str, index: usize) -> Option<u16> {
    s.encode_utf16().nth(index)
}

/// The code point starting at UTF-16 code-unit `index`, folding a surrogate
/// pair into a single code point and otherwise yielding the lone code unit.
pub(crate) fn code_point_at_utf16(s: &str, index: usize) -> Option<u32> {
    let units: Vec<u16> = s.encode_utf16().collect();
    units.get(index).map(|_| code_point(&units, index))
}

/// The character (code point) at UTF-16 code-unit `index`, as a string.
pub(crate) fn char_at_utf16(s: &str, index: usize) -> Option<String> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let unit = *units.get(index)?;
    let code = code_point(&units, index);
    if is_surrogate(code) {
        Some(String::from_utf16_lossy(&[unit]))
    } else {
        Some(char::from_u32(code).unwrap().to_string())
    }
}

/// Converts a byte offset into `s` to a UTF-16 code-unit offset.
fn byte_to_utf16(s: &str, byte: usize) -> usize {
    s.get(..byte)
        .map_or(0, |prefix| prefix.encode_utf16().count())
}

/// The code point beginning at `index` within `units`, folding a valid
/// surrogate pair and otherwise yielding the lone code unit.
fn code_point(units: &[u16], index: usize) -> u32 {
    let unit = units[index];
    if index + 1 < units.len() && is_high_surrogate(unit) && is_low_surrogate(units[index + 1]) {
        0x1_0000 + (((unit - 0xD800) as u32) << 10) + (units[index + 1] - 0xDC00) as u32
    } else {
        unit as u32
    }
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..0xDC00).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..0xE000).contains(&unit)
}

fn is_surrogate(code: u32) -> bool {
    (0xD800..0xE000).contains(&code)
}

pub(crate) fn property_method(key: &str) -> Option<crate::ops::Builtin> {
    match key {
        "includes" => Some(crate::ops::Builtin::StringIncludes),
        "isWellFormed" => Some(crate::ops::Builtin::StringIsWellFormed),
        "toWellFormed" => Some(crate::ops::Builtin::StringToWellFormed),
        "startsWith" => Some(crate::ops::Builtin::StringStartsWith),
        "endsWith" => Some(crate::ops::Builtin::StringEndsWith),
        "repeat" => Some(crate::ops::Builtin::StringRepeat),
        "trim" => Some(crate::ops::Builtin::StringTrim),
        "toLowerCase" => Some(crate::ops::Builtin::StringToLowerCase),
        "toUpperCase" => Some(crate::ops::Builtin::StringToUpperCase),
        "charAt" => Some(crate::ops::Builtin::StringCharAt),
        "charCodeAt" => Some(crate::ops::Builtin::StringCharCodeAt),
        "indexOf" => Some(crate::ops::Builtin::StringIndexOf),
        "lastIndexOf" => Some(crate::ops::Builtin::StringLastIndexOf),
        "slice" => Some(crate::ops::Builtin::StringSlice),
        "substring" => Some(crate::ops::Builtin::StringSubstring),
        "concat" => Some(crate::ops::Builtin::StringConcat),
        "split" => Some(crate::ops::Builtin::StringSplit),
        "padStart" => Some(crate::ops::Builtin::StringPadStart),
        "padEnd" => Some(crate::ops::Builtin::StringPadEnd),
        "trimStart" => Some(crate::ops::Builtin::StringTrimStart),
        "trimEnd" => Some(crate::ops::Builtin::StringTrimEnd),
        "codePointAt" => Some(crate::ops::Builtin::StringCodePointAt),
        "toString" => Some(crate::ops::Builtin::StringToString),
        "valueOf" => Some(crate::ops::Builtin::BoxedValueOf),
        "replace" => Some(crate::ops::Builtin::StringReplace),
        "replaceAll" => Some(crate::ops::Builtin::StringReplaceAll),
        "search" => Some(crate::ops::Builtin::StringSearch),
        _ => None,
    }
}

pub(crate) fn execute_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    let result = match builtin {
        crate::ops::Builtin::StringFromCharCode => Ok(from_char_code(arguments)),
        crate::ops::Builtin::StringIncludes => Ok(includes(receiver, arguments)),
        crate::ops::Builtin::StringIsWellFormed => is_well_formed(receiver),
        crate::ops::Builtin::StringToWellFormed => to_well_formed(receiver),
        crate::ops::Builtin::StringStartsWith => Ok(starts_with(receiver, arguments)),
        crate::ops::Builtin::StringEndsWith => Ok(ends_with(receiver, arguments)),
        crate::ops::Builtin::StringRepeat => Ok(repeat(receiver, arguments)),
        crate::ops::Builtin::StringTrim => Ok(trim(receiver)),
        crate::ops::Builtin::StringToLowerCase => Ok(to_lower_case(receiver)),
        crate::ops::Builtin::StringToUpperCase => Ok(to_upper_case(receiver)),
        crate::ops::Builtin::StringCharAt => Ok(char_at(receiver, arguments)),
        crate::ops::Builtin::StringCharCodeAt => Ok(char_code_at(receiver, arguments)),
        crate::ops::Builtin::StringIndexOf => Ok(index_of(receiver, arguments)),
        crate::ops::Builtin::StringLastIndexOf => Ok(last_index_of(receiver, arguments)),
        crate::ops::Builtin::StringSlice => Ok(slice(receiver, arguments)),
        crate::ops::Builtin::StringSubstring => Ok(substring(receiver, arguments)),
        crate::ops::Builtin::StringConcat => Ok(concat(receiver, arguments)),
        crate::ops::Builtin::StringSplit => Ok(split(receiver, arguments)),
        crate::ops::Builtin::StringPadStart => Ok(pad_start(receiver, arguments)),
        crate::ops::Builtin::StringPadEnd => Ok(pad_end(receiver, arguments)),
        crate::ops::Builtin::StringTrimStart => Ok(trim_start(receiver)),
        crate::ops::Builtin::StringTrimEnd => Ok(trim_end(receiver)),
        crate::ops::Builtin::StringCodePointAt => Ok(code_point_at(receiver, arguments)),
        crate::ops::Builtin::StringToString => Ok(to_string_value(receiver)),
        crate::ops::Builtin::StringReplace => replace(receiver, arguments, false),
        crate::ops::Builtin::StringReplaceAll => replace(receiver, arguments, true),
        crate::ops::Builtin::StringSearch => Ok(search(receiver, arguments)),
        _ => return None,
    };
    Some(result)
}

fn from_char_code(arguments: &[Value]) -> Value {
    let units = arguments
        .iter()
        .map(|value| {
            let number = crate::intl::tolocale::value::to_number(Some(value));
            crate::construct::to_uint16(number)
        })
        .collect::<Vec<_>>();
    Value::String(String::from_utf16_lossy(&units))
}

pub(crate) fn includes(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Boolean(false);
    };
    Value::Boolean(value.contains(&argument(arguments)))
}

fn is_well_formed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = string_receiver(receiver)?;
    Ok(Value::Boolean(
        value
            .encode_utf16()
            .all(|unit| !(0xD800..0xE000).contains(&unit)),
    ))
}

fn to_well_formed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(string_receiver(receiver)?.to_string()))
}

fn string_receiver(receiver: Option<&Value>) -> Result<String, crate::execute::VmError> {
    let Some(value) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert undefined to object",
        ));
    };
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert nullish value to object",
        ));
    }
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert Symbol to string",
        ));
    }
    crate::conversion::to_string(value)
}

pub(crate) fn starts_with(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Boolean(false);
    };
    Value::Boolean(value.starts_with(&argument(arguments)))
}

pub(crate) fn ends_with(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Boolean(false);
    };
    Value::Boolean(value.ends_with(&argument(arguments)))
}

pub(crate) fn repeat(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let count = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    Value::String(value.repeat(count))
}

pub(crate) fn trim(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.trim().to_string())
}

pub(crate) fn to_lower_case(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.to_lowercase())
}

pub(crate) fn to_upper_case(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.to_uppercase())
}

pub(crate) fn char_at(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let index = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    Value::String(char_at_utf16(value, index).unwrap_or_default())
}

pub(crate) fn char_code_at(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Number(f64::NAN);
    };
    let index = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    utf16_code_unit(value, index).map_or(Value::Number(f64::NAN), |unit| Value::Number(unit as f64))
}

pub(crate) fn index_of(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Number(-1.0);
    };
    let search = argument(arguments);
    Value::Number(
        value
            .find(&search)
            .map_or(-1.0, |byte| byte_to_utf16(value, byte) as f64),
    )
}

pub(crate) fn last_index_of(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Number(-1.0);
    };
    let search = argument(arguments);
    Value::Number(
        value
            .rfind(&search)
            .map_or(-1.0, |byte| byte_to_utf16(value, byte) as f64),
    )
}

pub(crate) fn slice(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let units: Vec<u16> = value.encode_utf16().collect();
    let length = units.len() as isize;
    let start = string_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| string_index(Some(value), length));
    let range = start.min(end) as usize..end.max(start) as usize;
    Value::String(String::from_utf16_lossy(&units[range]))
}

pub(crate) fn substring(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let units: Vec<u16> = value.encode_utf16().collect();
    let length = units.len() as isize;
    let start = substring_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| substring_index(Some(value), length));
    let range = start.min(end) as usize..end.max(start) as usize;
    Value::String(String::from_utf16_lossy(&units[range]))
}

fn string_index(value: Option<&Value>, length: isize) -> isize {
    let number = value.and_then(number).unwrap_or(0.0).trunc() as isize;
    if number < 0 {
        (length + number).max(0)
    } else {
        number.min(length)
    }
}

fn substring_index(value: Option<&Value>, length: isize) -> isize {
    value
        .and_then(number)
        .unwrap_or(0.0)
        .max(0.0)
        .trunc()
        .min(length as f64) as isize
}

pub(crate) fn concat(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let mut result = value.clone();
    for argument in arguments {
        result.push_str(&to_string(argument));
    }
    Value::String(result)
}

pub(crate) fn split(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::array(Vec::new());
    };
    let separator = arguments.first().map_or_else(String::new, to_string);
    let values = if separator.is_empty() {
        value
            .encode_utf16()
            .map(|unit| Value::String(String::from_utf16_lossy(&[unit])))
            .collect()
    } else {
        value
            .split(&separator)
            .map(|part| Value::String(part.to_string()))
            .collect()
    };
    Value::array(values)
}

pub(crate) fn pad_start(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    pad(receiver, arguments, true)
}

pub(crate) fn pad_end(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    pad(receiver, arguments, false)
}

fn pad(receiver: Option<&Value>, arguments: &[Value], start: bool) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let target = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    let fill = arguments.get(1).map_or_else(|| " ".to_string(), to_string);
    let count = target.saturating_sub(utf16_len(value));
    let padding_units: Vec<u16> = fill.encode_utf16().cycle().take(count).collect();
    let padding = String::from_utf16_lossy(&padding_units);
    if start {
        Value::String(format!("{padding}{value}"))
    } else {
        Value::String(format!("{value}{padding}"))
    }
}

pub(crate) fn trim_start(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.trim_start().to_string())
}

pub(crate) fn trim_end(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.trim_end().to_string())
}

pub(crate) fn code_point_at(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Undefined;
    };
    let index = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    code_point_at_utf16(value, index).map_or(Value::Undefined, |code| Value::Number(code as f64))
}

pub(crate) fn to_string_value(receiver: Option<&Value>) -> Value {
    Value::String(
        receiver
            .map(|value| crate::intl::tolocale::value::to_string(Some(value)))
            .unwrap_or_default(),
    )
}

pub(crate) fn replace(
    receiver: Option<&Value>,
    arguments: &[Value],
    all: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::String(value)) = receiver else {
        return Ok(Value::String(String::new()));
    };
    let pattern = arguments.first().map_or_else(String::new, to_string);
    let Some(replacement) = arguments.get(1) else {
        let result = if all {
            value.replace(&pattern, "")
        } else {
            value.replacen(&pattern, "", 1)
        };
        return Ok(Value::String(result));
    };
    let result = if crate::conversion::is_callable(replacement) {
        apply_callable_replacement(value, pattern, replacement, all)?
    } else {
        let template = to_string(replacement);
        if all {
            value.replace(&pattern, &template)
        } else {
            value.replacen(&pattern, &template, 1)
        }
    };
    Ok(Value::String(result))
}

fn apply_callable_replacement(
    value: &str,
    pattern: String,
    replacement: &Value,
    all: bool,
) -> Result<String, crate::execute::VmError> {
    let mut result = String::new();
    let mut rest = value;
    while let Some(index) = rest.find(&pattern) {
        let matched = rest[..index + pattern.len()].to_string();
        let suffix_start = index + pattern.len();
        let offset = value.len() - rest.len() + index;
        let callback_args = [
            Value::String(matched.clone()),
            Value::Number(offset as f64),
            Value::String(value.to_string()),
        ];
        let replaced =
            crate::functions::execute_target(replacement, &Value::Undefined, &callback_args)?;
        result.push_str(&matched[..index]);
        result.push_str(&to_string(&replaced));
        rest = &rest[suffix_start..];
        if !all {
            result.push_str(rest);
            return Ok(result);
        }
    }
    result.push_str(rest);
    Ok(result)
}

pub(crate) fn search(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Number(-1.0);
    };
    let pattern = arguments.first().map_or_else(String::new, to_string);
    Value::Number(value.find(&pattern).map_or(-1.0, |index| index as f64))
}

fn argument(arguments: &[Value]) -> String {
    arguments.first().map_or_else(String::new, to_string)
}

fn to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}
