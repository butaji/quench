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
        crate::ops::Builtin::StringRepeat => repeat(receiver, arguments),
        crate::ops::Builtin::StringTrim => trim(receiver),
        crate::ops::Builtin::StringToLowerCase => to_lower_case(receiver),
        crate::ops::Builtin::StringToUpperCase => to_upper_case(receiver),
        crate::ops::Builtin::StringNormalize => normalize(receiver, arguments),
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
        crate::ops::Builtin::StringBlink => html_wrapper(receiver, "blink"),
        crate::ops::Builtin::StringBold => html_wrapper(receiver, "b"),
        crate::ops::Builtin::StringFixed => html_wrapper(receiver, "tt"),
        crate::ops::Builtin::StringFontcolor => {
            html_attribute_wrapper(receiver, arguments, "font", "color")
        }
        crate::ops::Builtin::StringFontsize => {
            html_attribute_wrapper(receiver, arguments, "font", "size")
        }
        crate::ops::Builtin::StringItalics => html_wrapper(receiver, "i"),
        crate::ops::Builtin::StringLink => html_attribute_wrapper(receiver, arguments, "a", "href"),
        crate::ops::Builtin::StringStrike => html_wrapper(receiver, "strike"),
        crate::ops::Builtin::StringSmall => html_wrapper(receiver, "small"),
        crate::ops::Builtin::StringSub => html_wrapper(receiver, "sub"),
        crate::ops::Builtin::StringSup => html_wrapper(receiver, "sup"),
        crate::ops::Builtin::StringSlice => Ok(slice(receiver, arguments)),
        crate::ops::Builtin::StringSubstring => Ok(substring(receiver, arguments)),
        crate::ops::Builtin::StringSubstr => substr(receiver, arguments),
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

fn normalize(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = string_receiver(receiver)?;
    let form = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
        .map_or_else(|| Ok("NFC".to_string()), crate::conversion::to_string)?;
    let normalized = match form.as_str() {
        "NFC" => unicode_normalization::UnicodeNormalization::nfc(value.chars()).collect(),
        "NFD" => unicode_normalization::UnicodeNormalization::nfd(value.chars()).collect(),
        "NFKC" => unicode_normalization::UnicodeNormalization::nfkc(value.chars()).collect(),
        "NFKD" => unicode_normalization::UnicodeNormalization::nfkd(value.chars()).collect(),
        _ => {
            return Err(crate::value::error::throw_range_error(
                "Invalid normalization form",
            ))
        }
    };
    Ok(Value::String(normalized))
}
