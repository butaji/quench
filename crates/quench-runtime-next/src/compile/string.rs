use oxc_ast::ast::{StringLiteral, TemplateElementValue};

use crate::bytecode::Constant;

const OCTAL_ESCAPE_DIGITS_FOR_ZERO_TO_THREE: usize = 3;
const OCTAL_ESCAPE_DIGITS_FOR_FOUR_TO_SEVEN: usize = 2;

pub(super) fn constant(value: &StringLiteral<'_>) -> Constant {
    from_raw(
        value.raw.as_ref().map(|raw| raw.as_str()),
        value.value.as_str(),
    )
}

pub(super) fn template_constant(value: &TemplateElementValue<'_>) -> Constant {
    from_raw(
        Some(value.raw.as_str()),
        value.cooked.as_ref().map_or("", |text| text.as_str()),
    )
}

fn from_raw(raw: Option<&str>, fallback: &str) -> Constant {
    let Some(raw) = raw else {
        return Constant::String(fallback.into());
    };
    let units = decode_units(raw);
    match String::from_utf16(&units) {
        Ok(value) => Constant::String(value),
        Err(_) => Constant::StringUnits(units),
    }
}

fn decode_units(raw: &str) -> Vec<u16> {
    let mut chars = raw.chars();
    let quoted = matches!(chars.clone().next(), Some('\'' | '"'));
    if quoted {
        chars.next();
    }
    let mut units = Vec::new();
    while let Some(ch) = chars.next() {
        if quoted && matches!(ch, '\'' | '"') && chars.as_str().is_empty() {
            break;
        }
        if ch != '\\' {
            push_char(&mut units, ch);
            continue;
        }
        let Some(escape) = chars.next() else {
            break;
        };
        match escape {
            'u' => {
                let value = if chars.clone().next() == Some('{') {
                    chars.next();
                    let mut value = 0_u32;
                    for digit in chars.by_ref() {
                        if digit == '}' {
                            break;
                        }
                        value = value * 16 + hex(digit).unwrap_or(0) as u32;
                    }
                    value
                } else {
                    read_hex(&mut chars, 4)
                };
                if value <= crate::unicode::UTF16_MAX_CODE_UNIT {
                    units.push(value as u16);
                } else if let Some(ch) = char::from_u32(value) {
                    push_char(&mut units, ch);
                }
            }
            'x' => units.push(read_hex(&mut chars, 2) as u16),
            '\n' => {}
            '\r' => {
                if chars.clone().next() == Some('\n') {
                    chars.next();
                }
            }
            '\u{2028}' | '\u{2029}' => {}
            'b' => units.push(8),
            'f' => units.push(12),
            'n' => units.push(10),
            'r' => units.push(13),
            't' => units.push(9),
            'v' => units.push(11),
            '0'..='7' => units.push(read_octal_escape(&mut chars, escape)),
            other => push_char(&mut units, other),
        }
    }
    units
}

fn read_octal_escape(chars: &mut std::str::Chars<'_>, first: char) -> u16 {
    let maximum_digits = if first <= '3' {
        OCTAL_ESCAPE_DIGITS_FOR_ZERO_TO_THREE
    } else {
        OCTAL_ESCAPE_DIGITS_FOR_FOUR_TO_SEVEN
    };
    let mut value = first as u16 - b'0' as u16;
    let mut digits = 1;
    while digits < maximum_digits
        && chars
            .clone()
            .next()
            .is_some_and(|next| ('0'..='7').contains(&next))
    {
        let Some(next) = chars.next() else {
            break;
        };
        value = value * 8 + (next as u16 - b'0' as u16);
        digits += 1;
    }
    value
}

fn push_char(units: &mut Vec<u16>, value: char) {
    let mut buffer = [0; 2];
    units.extend(value.encode_utf16(&mut buffer).iter().copied());
}

fn read_hex(chars: &mut std::str::Chars<'_>, count: usize) -> u32 {
    (0..count).fold(0, |value, _| {
        value * 16 + chars.next().and_then(hex).unwrap_or(0) as u32
    })
}

fn hex(value: char) -> Option<u8> {
    match value {
        '0'..='9' => Some(value as u8 - b'0'),
        'a'..='f' => Some(value as u8 - b'a' + 10),
        'A'..='F' => Some(value as u8 - b'A' + 10),
        _ => None,
    }
}
