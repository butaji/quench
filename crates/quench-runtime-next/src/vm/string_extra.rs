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

pub(super) fn decode_uri(
    value: &super::wtf16::JsString,
    component: bool,
) -> Result<super::wtf16::JsString, &'static str> {
    let units = value.units();
    let mut output = Vec::with_capacity(units.len());
    let reserved = b";/?:@&=+$,#";
    let hex = |unit: u16| match unit {
        unit if (b'0' as u16..=b'9' as u16).contains(&unit) => Some((unit - b'0' as u16) as u8),
        unit if (b'a' as u16..=b'f' as u16).contains(&unit) => Some((unit - b'a' as u16 + 10) as u8),
        unit if (b'A' as u16..=b'F' as u16).contains(&unit) => Some((unit - b'A' as u16 + 10) as u8),
        _ => None,
    };
    let mut index = 0;
    while index < units.len() {
        if units[index] != b'%' as u16 {
            output.push(units[index]);
            index += 1;
            continue;
        }
        if index + 2 >= units.len() {
            return Err("malformed URI escape");
        }
        let first = (hex(units[index + 1]).ok_or("malformed URI escape")? << 4)
            | hex(units[index + 2]).ok_or("malformed URI escape")?;
        let (width, mut code_point, minimum) = match first {
            0x00..=0x7f => (1, u32::from(first), 0),
            0xc2..=0xdf => (2, u32::from(first & 0x1f), 0x80),
            0xe0..=0xef => (3, u32::from(first & 0x0f), 0x800),
            0xf0..=0xf4 => (4, u32::from(first & 0x07), 0x10000),
            _ => return Err("malformed URI sequence"),
        };
        if width == 1 && !component && reserved.contains(&(first as u8)) {
            output.extend_from_slice(&units[index..index + 3]);
            index += 3;
            continue;
        }
        for continuation in 1..width {
            let escape = index + continuation * 3;
            if units.get(escape) != Some(&(b'%' as u16)) || escape + 2 >= units.len() {
                return Err("malformed URI sequence");
            }
            let byte = (hex(units[escape + 1]).ok_or("malformed URI escape")? << 4)
                | hex(units[escape + 2]).ok_or("malformed URI escape")?;
            if byte & 0xc0 != 0x80 {
                return Err("malformed URI sequence");
            }
            if continuation == 1
                && ((first == 0xe0 && byte < 0xa0)
                    || (first == 0xf0 && byte < 0x90)
                    || (first == 0xf4 && byte > 0x8f))
            {
                return Err("malformed URI sequence");
            }
            code_point = (code_point << 6) | u32::from(byte & 0x3f);
        }
        if code_point < minimum || code_point > 0x10ffff {
            return Err("malformed URI sequence");
        }
        if code_point <= 0xffff {
            output.push(code_point as u16);
        } else {
            let scalar = code_point - 0x10000;
            output.push(0xd800 | (scalar >> 10) as u16);
            output.push(0xdc00 | (scalar & 0x3ff) as u16);
        }
        index += width * 3;
    }
    Ok(super::wtf16::JsString::from_units(&output))
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
