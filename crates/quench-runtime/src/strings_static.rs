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

/// Whether two string values hold identical UTF-16 code units.
pub(crate) fn units_equal(left: &Value, right: &Value) -> bool {
    match (units_of(left), units_of(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}
