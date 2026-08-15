use crate::value::Value;
include!("strings_static.rs");

/// Builds the canonical string value for raw UTF-16 code units: a plain
/// `String` when the units are valid UTF-16, otherwise `StringUnits`.
pub(crate) fn from_units(units: Vec<u16>) -> Value {
    match String::from_utf16(&units) {
        Ok(value) => Value::String(value),
        Err(_) => Value::StringUnits(std::rc::Rc::new(units)),
    }
}

/// The raw UTF-16 code units of a string value.
pub(crate) fn units_of(value: &Value) -> Option<Vec<u16>> {
    match value {
        Value::String(value) => Some(value.encode_utf16().collect()),
        Value::StringUnits(units) => Some((**units).clone()),
        _ => None,
    }
}

/// The lossy UTF-8 view of a string value (lone surrogates become U+FFFD).
pub(crate) fn lossy(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::StringUnits(units) => Some(String::from_utf16_lossy(units)),
        _ => None,
    }
}

pub(crate) fn source_text(value: &Value) -> Option<String> {
    let units = units_of(value)?;
    let mut source = String::new();
    for unit in units {
        if (0xD800..=0xDFFF).contains(&unit) {
            source.push_str(&format!("\\u{unit:04X}"));
        } else if let Some(character) = char::from_u32(u32::from(unit)) {
            source.push(character);
        }
    }
    Some(source)
}

pub(crate) fn decode_surrogate_escapes(value: &str) -> Value {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut units = Vec::new();
    let mut changed = false;
    while index < bytes.len() {
        let unit = bytes
            .get(index..index + 6)
            .filter(|slice| slice[0] == b'\\' && slice[1] == b'u')
            .and_then(|slice| std::str::from_utf8(&slice[2..]).ok())
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
            .filter(|unit| (0xD800..=0xDFFF).contains(unit));
        if let Some(unit) = unit {
            units.push(unit);
            index += 6;
            changed = true;
            continue;
        }
        let character = value[index..].chars().next().unwrap_or('\0');
        let mut encoded = [0; 2];
        let length = character.encode_utf16(&mut encoded).len();
        units.extend_from_slice(&encoded[..length]);
        index += character.len_utf8();
    }
    if changed {
        from_units(units)
    } else {
        Value::String(value.to_string())
    }
}

/// Whether two string values hold identical UTF-16 code units.
pub(crate) fn units_equal(left: &Value, right: &Value) -> bool {
    match (units_of(left), units_of(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Whether `units` form a well-formed UTF-16 sequence (no lone surrogates).
pub(crate) fn units_well_formed(units: &[u16]) -> bool {
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if is_high_surrogate(unit) {
            if !units
                .get(index + 1)
                .is_some_and(|next| is_low_surrogate(*next))
            {
                return false;
            }
            index += 2;
        } else if is_low_surrogate(unit) {
            return false;
        } else {
            index += 1;
        }
    }
    true
}

/// The string value of the code point at UTF-16 code-unit `index`, preserving
/// a lone surrogate as a one-unit `StringUnits`.
pub(crate) fn char_at_units(units: &[u16], index: usize) -> Option<Value> {
    let unit = *units.get(index)?;
    let code = code_point(units, index);
    if code > 0xFFFF {
        Some(from_units(units[index..index + 2].to_vec()))
    } else if is_surrogate(code) {
        Some(from_units(vec![unit]))
    } else {
        char::from_u32(code).map(|character| Value::String(character.to_string()))
    }
}

/// The ECMAScript length of `s`: its count of UTF-16 code units.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The UTF-16 code unit at `index`, if any.
pub(crate) fn utf16_code_unit(s: &str, index: usize) -> Option<u16> {
    s.encode_utf16().nth(index)
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
pub(crate) fn byte_to_utf16(s: &str, byte: usize) -> usize {
    s.get(..byte)
        .map_or(0, |prefix| prefix.encode_utf16().count())
}

/// Converts a UTF-16 code-unit offset into its corresponding UTF-8 byte offset.
pub(crate) fn utf16_byte_index(s: &str, index: usize) -> usize {
    let mut units = 0;
    for (byte, character) in s.char_indices() {
        if units >= index {
            return byte;
        }
        units += character.len_utf16();
        if units >= index {
            return byte + character.len_utf8();
        }
    }
    s.len()
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
        "valueOf" => Some(crate::ops::Builtin::StringValueOf),
        "replace" => Some(crate::ops::Builtin::StringReplace),
        "replaceAll" => Some(crate::ops::Builtin::StringReplaceAll),
        "search" => Some(crate::ops::Builtin::StringSearch),
        "match" => Some(crate::ops::Builtin::StringMatch),
        _ => None,
    }
}

pub(crate) fn execute_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    let result = match builtin {
        crate::ops::Builtin::StringFromCharCode => from_char_code(arguments),
        crate::ops::Builtin::StringFromCodePoint => from_code_point(arguments),
        crate::ops::Builtin::StringRaw => raw(arguments),
        crate::ops::Builtin::StringIncludes => includes(receiver, arguments),
        crate::ops::Builtin::StringIsWellFormed => is_well_formed(receiver),
        crate::ops::Builtin::StringToWellFormed => to_well_formed(receiver),
        crate::ops::Builtin::StringStartsWith => starts_with(receiver, arguments),
        crate::ops::Builtin::StringEndsWith => ends_with(receiver, arguments),
        crate::ops::Builtin::StringRepeat => repeat(receiver, arguments),
        crate::ops::Builtin::StringTrim => trim(receiver),
        crate::ops::Builtin::StringToLowerCase => Ok(to_lower_case(receiver)),
        crate::ops::Builtin::StringToUpperCase => Ok(to_upper_case(receiver)),
        crate::ops::Builtin::StringCharAt => char_at(receiver, arguments),
        crate::ops::Builtin::StringCharCodeAt => char_code_at(receiver, arguments),
        crate::ops::Builtin::StringIndexOf => index_of(receiver, arguments),
        crate::ops::Builtin::StringLastIndexOf => last_index_of(receiver, arguments),
        crate::ops::Builtin::StringSlice => slice(receiver, arguments),
        crate::ops::Builtin::StringSubstring => substring(receiver, arguments),
        crate::ops::Builtin::StringConcat => concat(receiver, arguments),
        crate::ops::Builtin::StringSplit => split(receiver, arguments),
        crate::ops::Builtin::StringPadStart => Ok(pad_start(receiver, arguments)),
        crate::ops::Builtin::StringPadEnd => Ok(pad_end(receiver, arguments)),
        crate::ops::Builtin::StringTrimStart => trim_start(receiver),
        crate::ops::Builtin::StringTrimEnd => trim_end(receiver),
        crate::ops::Builtin::StringCodePointAt => code_point_at(receiver, arguments),
        crate::ops::Builtin::StringToString => Ok(to_string_value(receiver)),
        crate::ops::Builtin::StringReplace => replace(receiver, arguments, false),
        crate::ops::Builtin::StringReplaceAll => replace(receiver, arguments, true),
        crate::ops::Builtin::StringSearch => Ok(search(receiver, arguments)),
        crate::ops::Builtin::StringMatch => string_match(receiver, arguments),
        _ => return None,
    };
    Some(result)
}

fn from_char_code(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let mut units = Vec::with_capacity(arguments.len());
    for value in arguments {
        let number = match value {
            Value::Number(number) => *number,
            _ => crate::intl::tolocale::value::to_number_result(Some(value))?,
        };
        units.push(crate::construct::to_uint16(number));
    }
    Ok(from_units(units))
}

fn is_well_formed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    if let Some(Value::StringUnits(units)) = receiver {
        return Ok(Value::Boolean(units_well_formed(units)));
    }
    let Some(Value::String(value)) = receiver else {
        return Ok(Value::String(String::new()));
    };
    let units: Vec<u16> = value.encode_utf16().collect();
    Ok(Value::Boolean(units_well_formed(&units)))
}

fn to_well_formed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    if let Some(Value::StringUnits(units)) = receiver {
        return Ok(Value::String(String::from_utf16_lossy(units)));
    }
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
    if let Value::BigInt(value) = value {
        return Ok(value.clone());
    }
    crate::conversion::to_string(value)
}

pub(crate) fn repeat(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = string_receiver(receiver)?;
    let count = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    if count.is_infinite() || count < 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid count value",
        ));
    }
    Ok(Value::String(value.repeat(count.trunc() as usize)))
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

pub(crate) fn char_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    string_receiver(receiver)?;
    let Some(units) = receiver.and_then(units_of) else {
        return Ok(Value::String(String::new()));
    };
    let index = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    Ok(char_at_units(&units, index).unwrap_or_else(|| Value::String(String::new())))
}

pub(crate) fn char_code_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    string_receiver(receiver)?;
    let Some(units) = receiver.and_then(units_of) else {
        return Ok(Value::Number(f64::NAN));
    };
    let index = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    Ok(units
        .get(index)
        .map_or(Value::Number(f64::NAN), |unit| Value::Number(*unit as f64)))
}

pub(crate) fn slice(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    string_receiver(receiver)?;
    let Some(units) = receiver.and_then(units_of) else {
        return Ok(Value::String(String::new()));
    };
    let length = units.len() as isize;
    let start = string_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| string_index(Some(value), length));
    let range = start.min(end) as usize..end.max(start) as usize;
    Ok(from_units(units[range].to_vec()))
}

pub(crate) fn substring(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    string_receiver(receiver)?;
    let Some(units) = receiver.and_then(units_of) else {
        return Ok(Value::String(String::new()));
    };
    let length = units.len() as isize;
    let start = substring_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| substring_index(Some(value), length));
    let range = start.min(end) as usize..end.max(start) as usize;
    Ok(from_units(units[range].to_vec()))
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

pub(crate) fn concat(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(mut units) = receiver.and_then(units_of) else {
        return Ok(Value::String(String::new()));
    };
    for argument in arguments {
        units.extend(crate::conversion::to_string(argument)?.encode_utf16());
    }
    Ok(from_units(units))
}

pub(crate) fn pad_start(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    pad(receiver, arguments, true)
}

pub(crate) fn pad_end(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    pad(receiver, arguments, false)
}

fn pad(receiver: Option<&Value>, arguments: &[Value], start: bool) -> Value {
    let Some(value) = receiver.and_then(lossy) else {
        return Value::String(String::new());
    };
    let target = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    let fill = arguments.get(1).map_or_else(|| " ".to_string(), to_string);
    let count = target.saturating_sub(utf16_len(&value));
    let padding_units: Vec<u16> = fill.encode_utf16().cycle().take(count).collect();
    let padding = String::from_utf16_lossy(&padding_units);
    if start {
        Value::String(format!("{padding}{value}"))
    } else {
        Value::String(format!("{value}{padding}"))
    }
}

pub(crate) fn code_point_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = match receiver {
        Some(Value::StringUnits(units)) => (**units).clone(),
        _ => string_receiver(receiver)?.encode_utf16().collect(),
    };
    let position = match arguments.first() {
        None => 0.0,
        Some(value) => crate::conversion::to_number(value)?,
    };
    if position < 0.0 || position.is_nan() || position >= units.len() as f64 {
        return Ok(Value::Undefined);
    }
    let code = code_point(&units, position as usize);
    Ok(Value::Number(code as f64))
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
    let value = string_receiver(receiver)?;
    let pattern = arguments
        .first()
        .map(crate::conversion::to_string)
        .transpose()?
        .unwrap_or_default();
    let Some(replacement) = arguments.get(1) else {
        let result = if all {
            value.replace(&pattern, "")
        } else {
            value.replacen(&pattern, "", 1)
        };
        return Ok(Value::String(result));
    };
    let result = if crate::conversion::is_callable(replacement) {
        apply_callable_replacement(&value, pattern, replacement, all)?
    } else {
        let template = crate::conversion::to_string(replacement)?;
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

include!("strings_tail.rs");

include!("strings_search.rs");
