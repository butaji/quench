fn from_code_point(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let mut result = String::new();
    for value in arguments {
        let number = crate::intl::tolocale::value::to_number_result(Some(value))?;
        if !number.is_finite()
            || number.fract() != 0.0
            || !(0.0..=0x10ffff as f64).contains(&number)
        {
            return Err(crate::value::error::throw_range_error("Invalid code point"));
        }
        let character = char::from_u32(number as u32)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid code point"))?;
        result.push(character);
    }
    Ok(Value::String(result))
}

fn raw(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let template = arguments
        .first()
        .ok_or_else(|| crate::value::error::throw_type_error("String.raw requires a template"))?;
    let raw = crate::execute::get_property_result(template, "raw")?;
    let length =
        crate::conversion::to_number(&crate::execute::get_property_result(&raw, "length")?)?;
    let length = if !length.is_finite() || length <= 0.0 {
        0
    } else {
        length.floor() as usize
    };
    let mut result = String::new();
    for index in 0..length {
        if index > 0 {
            if let Some(value) = arguments.get(index) {
                result.push_str(&crate::conversion::to_string(value)?);
            }
        }
        let segment = crate::execute::get_property_result(&raw, &index.to_string())?;
        result.push_str(&crate::conversion::to_string(&segment)?);
    }
    Ok(Value::String(result))
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

pub(crate) fn source_value(source: &str) -> Value {
    let mut units = Vec::new();
    let mut chars = source.chars().peekable();
    let mut has_surrogate = false;
    while let Some(character) = chars.next() {
        if character == '\\' && chars.next_if_eq(&'u').is_some() {
            let digits: String = chars.by_ref().take(4).collect();
            if digits.len() == 4 {
                if let Ok(unit) = u16::from_str_radix(&digits, 16) {
                    has_surrogate |= (0xD800..=0xDFFF).contains(&unit);
                    units.push(unit);
                    continue;
                }
            }
            units.extend("\\u".encode_utf16(&mut [0; 2]));
            units.extend(digits.encode_utf16(&mut [0; 4]));
            continue;
        }
        units.extend(character.encode_utf16(&mut [0; 2]));
    }
    if has_surrogate {
        Value::StringUnits(std::rc::Rc::new(units))
    } else {
        Value::String(source.to_string())
    }
}

/// Whether two string values hold identical UTF-16 code units.
pub(crate) fn units_equal(left: &Value, right: &Value) -> bool {
    match (units_of(left), units_of(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
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
