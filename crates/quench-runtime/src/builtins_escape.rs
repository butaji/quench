pub(crate) fn escape(value: Option<&Value>) -> Value {
    let source = value.map_or_else(|| value_to_string(&Value::Undefined), value_to_string);
    let mut result = String::new();
    for character in source.chars() {
        if character.is_ascii_alphanumeric() || "@*_+-./".contains(character) {
            result.push(character);
        } else {
            let code = character as u32;
            let escaped = if code <= 0xFF {
                format!("%{code:02X}")
            } else {
                format!("%u{code:04X}")
            };
            result.push_str(&escaped);
        }
    }
    Value::String(result)
}

pub(crate) fn unescape(value: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let text = match value {
        Some(value) => crate::conversion::to_string(value)?,
        None => "undefined".to_string(),
    };
    let input = text.encode_utf16().collect::<Vec<_>>();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' as u16 {
            if let Some((unit, consumed)) = unescape_unit(&input[index..]) {
                output.push(unit);
                index += consumed;
                continue;
            }
        }
        output.push(input[index]);
        index += 1;
    }
    Ok(crate::strings::from_units(output))
}

fn unescape_unit(input: &[u16]) -> Option<(u16, usize)> {
    if input.get(1) == Some(&(b'u' as u16)) {
        let digits = input.get(2..6)?.iter().map(|unit| hex_digit(*unit));
        let mut value = 0;
        for digit in digits {
            value = value * 16 + digit?;
        }
        return Some((value, 6));
    }
    let first = hex_digit(*input.get(1)?)?;
    let second = hex_digit(*input.get(2)?)?;
    Some((first * 16 + second, 3))
}

fn hex_digit(unit: u16) -> Option<u16> {
    if (b'0' as u16..=b'9' as u16).contains(&unit) {
        Some(unit - b'0' as u16)
    } else if (b'a' as u16..=b'f' as u16).contains(&unit) {
        Some(unit - b'a' as u16 + 10)
    } else if (b'A' as u16..=b'F' as u16).contains(&unit) {
        Some(unit - b'A' as u16 + 10)
    } else {
        None
    }
}
