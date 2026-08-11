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

pub(crate) fn unescape(value: Option<&Value>) -> Value {
    let text = value.map_or_else(|| value_to_string(&Value::Undefined), value_to_string);
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '%' {
            result.push(character);
            continue;
        }
        let digits: String = chars.by_ref().take(2).collect();
        if let Some(parsed) = u8::from_str_radix(&digits, 16).ok().map(char::from) {
            result.push(parsed);
        } else {
            result.push('%');
            result.push_str(&digits);
        }
    }
    Value::String(result)
}
