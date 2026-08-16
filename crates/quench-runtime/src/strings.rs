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
    if key == "Symbol.iterator" {
        return Some(crate::ops::Builtin::StringIterator);
    }
    match key {
        "anchor" => Some(crate::ops::Builtin::StringAnchor),
        "big" => Some(crate::ops::Builtin::StringBig),
        "bold" => Some(crate::ops::Builtin::StringBold),
        "fixed" => Some(crate::ops::Builtin::StringFixed),
        "fontcolor" => Some(crate::ops::Builtin::StringFontcolor),
        "italics" => Some(crate::ops::Builtin::StringItalics),
        "strike" => Some(crate::ops::Builtin::StringStrike),
        "small" => Some(crate::ops::Builtin::StringSmall),
        "includes" => Some(crate::ops::Builtin::StringIncludes),
        "isWellFormed" => Some(crate::ops::Builtin::StringIsWellFormed),
        "toWellFormed" => Some(crate::ops::Builtin::StringToWellFormed),
        "startsWith" => Some(crate::ops::Builtin::StringStartsWith),
        "endsWith" => Some(crate::ops::Builtin::StringEndsWith),
        "at" => Some(crate::ops::Builtin::StringAt),
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
        "localeCompare" => Some(crate::ops::Builtin::StringLocaleCompare),
        "match" => Some(crate::ops::Builtin::StringMatch),
        "matchAll" => Some(crate::ops::Builtin::StringMatchAll),
        _ => None,
    }
}

pub(crate) fn execute_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    let result = match builtin {
        crate::ops::Builtin::StringIterator => string_iterator(receiver),
        crate::ops::Builtin::StringFromCharCode => from_char_code(arguments),
        crate::ops::Builtin::StringFromCodePoint => from_code_point(arguments),
        crate::ops::Builtin::StringRaw => raw(arguments),
        crate::ops::Builtin::StringIncludes => includes(receiver, arguments),
        crate::ops::Builtin::StringIsWellFormed => is_well_formed(receiver),
        crate::ops::Builtin::StringToWellFormed => to_well_formed(receiver),
        crate::ops::Builtin::StringStartsWith => starts_with(receiver, arguments),
        crate::ops::Builtin::StringEndsWith => ends_with(receiver, arguments),
        crate::ops::Builtin::StringAt => at(receiver, arguments),
        crate::ops::Builtin::StringRepeat => Ok(repeat(receiver, arguments)),
        crate::ops::Builtin::StringTrim => trim(receiver),
        crate::ops::Builtin::StringToLowerCase => to_lower_case(receiver),
        crate::ops::Builtin::StringToUpperCase => to_upper_case(receiver),
        crate::ops::Builtin::StringCharAt => char_at(receiver, arguments),
        crate::ops::Builtin::StringCharCodeAt => char_code_at(receiver, arguments),
        crate::ops::Builtin::StringIndexOf => index_of(receiver, arguments),
        crate::ops::Builtin::StringLastIndexOf => last_index_of(receiver, arguments),
        _ => return execute_builtin_tail(builtin, receiver, arguments),
    };
    Some(result)
}

fn execute_builtin_tail(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    let result = match builtin {
        crate::ops::Builtin::StringAnchor => anchor(receiver, arguments),
        crate::ops::Builtin::StringBig => html_wrapper(receiver, "big"),
        crate::ops::Builtin::StringBold => html_wrapper(receiver, "b"),
        crate::ops::Builtin::StringFixed => html_wrapper(receiver, "tt"),
        crate::ops::Builtin::StringFontcolor => {
            html_attribute_wrapper(receiver, arguments, "font", "color")
        }
        crate::ops::Builtin::StringItalics => html_wrapper(receiver, "i"),
        crate::ops::Builtin::StringStrike => html_wrapper(receiver, "strike"),
        crate::ops::Builtin::StringSmall => html_wrapper(receiver, "small"),
        crate::ops::Builtin::StringSlice => Ok(slice(receiver, arguments)),
        crate::ops::Builtin::StringSubstring => Ok(substring(receiver, arguments)),
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
        crate::ops::Builtin::StringSearch => search(receiver, arguments),
        crate::ops::Builtin::StringLocaleCompare => locale_compare(receiver, arguments),
        crate::ops::Builtin::StringMatch => string_match(receiver, arguments),
        crate::ops::Builtin::StringMatchAll => string_match_all(receiver, arguments),
        _ => return None,
    };
    Some(result)
}

fn html_wrapper(receiver: Option<&Value>, tag: &str) -> Result<Value, crate::execute::VmError> {
    let text = string_receiver(receiver)?;
    Ok(Value::String(format!("<{tag}>{text}</{tag}>")))
}

fn anchor(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    html_attribute_wrapper(receiver, arguments, "a", "name")
}

fn html_attribute_wrapper(
    receiver: Option<&Value>,
    arguments: &[Value],
    tag: &str,
    attribute: &str,
) -> Result<Value, crate::execute::VmError> {
    let text = string_receiver(receiver)?;
    let name = arguments
        .first()
        .map(crate::conversion::to_string)
        .transpose()?;
    let attribute_value = name.map_or_else(String::new, |value| {
        format!(" {attribute}=\"{}\"", value.replace('"', "&quot;"))
    });
    Ok(Value::String(format!(
        "<{tag}{attribute_value}>{text}</{tag}>"
    )))
}

fn at(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let units = receiver
        .and_then(units_of)
        .unwrap_or(string_receiver(receiver)?.encode_utf16().collect());
    let index = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let index = if index.is_nan() {
        0
    } else {
        index.trunc() as isize
    };
    let position = if index < 0 {
        units.len() as isize + index
    } else {
        index
    };
    if position < 0 {
        return Ok(Value::Undefined);
    }
    Ok(units
        .get(position as usize)
        .map(|unit| from_units(vec![*unit]))
        .unwrap_or(Value::Undefined))
}

fn string_iterator(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = receiver.ok_or_else(|| {
        crate::value::error::throw_type_error("Cannot convert undefined to object")
    })?;
    let units = match crate::strings::units_of(value) {
        Some(units) => units,
        None => string_receiver(Some(value))?.encode_utf16().collect(),
    };
    Ok(crate::collections::iterator::make_string(units))
}

fn from_char_code(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let mut units = Vec::with_capacity(arguments.len());
    for value in arguments {
        let number = crate::intl::tolocale::value::to_number_result(Some(value))?;
        units.push(crate::construct::to_uint16(number));
    }
    Ok(from_units(units))
}

fn is_well_formed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    if let Some(Value::StringUnits(units)) = receiver {
        return Ok(Value::Boolean(units_well_formed(units)));
    }
    let value = string_receiver(receiver)?;
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

pub(crate) fn repeat(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let count = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    Value::String(value.repeat(count))
}

pub(crate) fn to_lower_case(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(string_receiver(receiver)?.to_lowercase()))
}

pub(crate) fn to_upper_case(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(string_receiver(receiver)?.to_uppercase()))
}

pub(crate) fn char_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let text = string_receiver(receiver)?;
    let units = text.encode_utf16().collect::<Vec<_>>();
    let index = arguments
        .first()
        .map_or(Ok(0.0), crate::conversion::to_number)?;
    let index = index.trunc();
    if index < 0.0 {
        return Ok(Value::String(String::new()));
    }
    let index = index as usize;
    Ok(char_at_units(&units, index).unwrap_or_else(|| Value::String(String::new())))
}

pub(crate) fn char_code_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let text = string_receiver(receiver)?;
    let units = text.encode_utf16().collect::<Vec<_>>();
    let index = arguments
        .first()
        .map_or(Ok(0.0), crate::conversion::to_number)?;
    let index = index.trunc();
    if index < 0.0 {
        return Ok(Value::Number(f64::NAN));
    }
    let index = index as usize;
    Ok(units
        .get(index)
        .map_or(Value::Number(f64::NAN), |unit| Value::Number(*unit as f64)))
}

pub(crate) fn slice(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(units) = receiver.and_then(units_of) else {
        return Value::String(String::new());
    };
    let length = units.len() as isize;
    let start = string_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| string_index(Some(value), length));
    let range = start.min(end) as usize..end.max(start) as usize;
    from_units(units[range].to_vec())
}

pub(crate) fn substring(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(units) = receiver.and_then(units_of) else {
        return Value::String(String::new());
    };
    let length = units.len() as isize;
    let start = substring_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| substring_index(Some(value), length));
    let range = start.min(end) as usize..end.max(start) as usize;
    from_units(units[range].to_vec())
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
    let mut units = string_receiver(receiver)?
        .encode_utf16()
        .collect::<Vec<_>>();
    for argument in arguments {
        units.extend(crate::conversion::to_string(argument)?.encode_utf16());
    }
    Ok(from_units(units))
}

pub(crate) fn locale_compare(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let left = string_receiver(receiver)?;
    let right = arguments
        .first()
        .map_or(Ok(String::from("undefined")), crate::conversion::to_string)?;
    let left = unicode_normalization::UnicodeNormalization::nfc(left.chars()).collect::<String>();
    let right = unicode_normalization::UnicodeNormalization::nfc(right.chars()).collect::<String>();
    Ok(Value::Number(crate::intl::collator::compare(
        &left,
        &right,
        &crate::intl::default_locale(),
        false,
        "variant",
    )))
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
        Some(value) => {
            let number = crate::conversion::to_number(value)?;
            if number.is_nan() {
                0.0
            } else {
                number.trunc()
            }
        }
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

include!("strings_tail.rs");

include!("strings_search.rs");
