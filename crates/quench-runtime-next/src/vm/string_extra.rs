pub(super) fn encode_uri(value: &str, component: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    const HEX_NIBBLE_MASK: u8 = 0x0f;
    let mut output = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let unescaped = byte.is_ascii_alphanumeric()
            || b"-_.!~*'()".contains(byte)
            || (!component && b";/?:@&=+$,#".contains(byte));
        if unescaped {
            output.push(*byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & HEX_NIBBLE_MASK) as usize] as char);
        }
    }
    output
}

pub(super) fn decode_uri(value: &str, component: bool) -> Result<String, &'static str> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let reserved = b";/?:@&=+$,#";
    let hex = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err("malformed URI escape");
        }
        let decoded = (hex(bytes[index + 1]).ok_or("malformed URI escape")? << 4)
            | hex(bytes[index + 2]).ok_or("malformed URI escape")?;
        if !component && reserved.contains(&decoded) {
            output.extend_from_slice(&bytes[index..index + 3]);
        } else {
            output.push(decoded);
        }
        index += 3;
    }
    String::from_utf8(output).map_err(|_| "malformed URI sequence")
}

pub(super) fn parse_integer(text: &str, mut radix: i32) -> f64 {
    let mut input = text.trim_start();
    let sign = if let Some(rest) = input.strip_prefix('-') {
        input = rest;
        -1.0
    } else {
        input = input.strip_prefix('+').unwrap_or(input);
        1.0
    };
    if radix == 0 {
        radix = if input.starts_with("0x") || input.starts_with("0X") {
            16
        } else {
            10
        };
    }
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    if radix == 16 {
        input = input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
            .unwrap_or(input);
    }
    let mut result = 0.0;
    let mut digits = 0;
    for digit in input
        .chars()
        .map_while(|character| character.to_digit(radix as u32))
    {
        result = result * f64::from(radix) + f64::from(digit);
        digits += 1;
    }
    if digits == 0 { f64::NAN } else { sign * result }
}

pub(super) fn number_to_radix(number: f64, radix: u32) -> String {
    if radix == 10 || !number.is_finite() || number.fract() != 0.0 {
        return if number.fract() == 0.0 {
            format!("{number:.0}")
        } else {
            number.to_string()
        };
    }
    if number == 0.0 {
        return "0".into();
    }
    let negative = number < 0.0;
    let mut value = number.abs();
    let alphabet = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = Vec::new();
    while value >= 1.0 {
        let digit = (value % f64::from(radix)) as usize;
        digits.push(alphabet[digit] as char);
        value = (value / f64::from(radix)).floor();
    }
    if negative {
        digits.push('-');
    }
    digits.iter().rev().collect()
}
