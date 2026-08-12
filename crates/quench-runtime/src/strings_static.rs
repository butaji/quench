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
