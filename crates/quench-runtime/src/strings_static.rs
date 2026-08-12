fn from_code_point(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let mut result = String::new();
    for value in arguments {
        let number = crate::intl::tolocale::value::to_number_result(Some(value))?;
        if !number.is_finite() || number.fract() != 0.0 || !(0.0..=0x10ffff as f64).contains(&number) {
            return Err(crate::value::error::throw_range_error("Invalid code point"));
        }
        let character = char::from_u32(number as u32)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid code point"))?;
        result.push(character);
    }
    Ok(Value::String(result))
}

fn raw(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let template = arguments.first().ok_or_else(|| {
        crate::value::error::throw_type_error("String.raw requires a template")
    })?;
    let raw = crate::execute::get_property_result(template, "raw")?;
    let length = crate::conversion::to_number(&crate::execute::get_property_result(&raw, "length")?)?;
    let length = if !length.is_finite() || length <= 0.0 { 0 } else { length.floor() as usize };
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
