use crate::execute::VmError;

const URI_RESERVED: &str = ";/?:@&=+$,#";

pub(crate) fn encode_uri(value: Option<&Value>, preserve_reserved: bool) -> Result<Value, VmError> {
    let units = uri_units(value)?;
    Ok(Value::String(encode(&units, preserve_reserved)?))
}

pub(crate) fn decode_uri(value: Option<&Value>, preserve_reserved: bool) -> Result<Value, VmError> {
    let units = uri_units(value)?;
    Ok(crate::strings::from_units(decode(&units, preserve_reserved)?))
}

fn uri_units(value: Option<&Value>) -> Result<Vec<u16>, VmError> {
    let value = match value {
        Some(value) => value,
        None => &Value::Undefined,
    };
    if let Some(units) = crate::strings::units_of(value) {
        return Ok(units);
    }
    Ok(crate::conversion::to_string(value)?.encode_utf16().collect())
}

fn encode(units: &[u16], preserve_reserved: bool) -> Result<String, VmError> {
    let mut encoded = String::new();
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if (0xD800..0xDC00).contains(&unit) {
            let Some(low) = units.get(index + 1) else {
                return Err(uri_error());
            };
            if !(0xDC00..0xE000).contains(low) {
                return Err(uri_error());
            }
            let code = 0x1_0000 + (((unit - 0xD800) as u32) << 10) + (*low - 0xDC00) as u32;
            if let Some(character) = char::from_u32(code) {
                encode_character(&mut encoded, character);
            }
            index += 2;
        } else if (0xDC00..0xE000).contains(&unit) {
            return Err(uri_error());
        } else if let Some(character) = char::from_u32(unit as u32) {
            if uri_unescaped(character, preserve_reserved) {
                encoded.push(character);
            } else {
                encode_character(&mut encoded, character);
            }
            index += 1;
        } else {
            index += 1;
        }
    }
    Ok(encoded)
}

fn uri_unescaped(character: char, preserve_reserved: bool) -> bool {
    character.is_ascii_alphanumeric()
        || "-_.!~*'()".contains(character)
        || (preserve_reserved && URI_RESERVED.contains(character))
}

fn encode_character(encoded: &mut String, character: char) {
    for byte in character.to_string().bytes() {
        encoded.push('%');
        encoded.push_str(&format!("{byte:02X}"));
    }
}

fn decode(units: &[u16], preserve_reserved: bool) -> Result<Vec<u16>, VmError> {
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < units.len() {
        if units[index] == u16::from(b'%') {
            decode_run(units, &mut index, preserve_reserved, &mut decoded)?;
        } else {
            decoded.push(units[index]);
            index += 1;
        }
    }
    Ok(decoded)
}

fn decode_run(
    units: &[u16],
    index: &mut usize,
    preserve_reserved: bool,
    decoded: &mut Vec<u16>,
) -> Result<(), VmError> {
    let (bytes, escapes) = percent_run(units, index)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| uri_error())?;
    append_decoded(&escapes, preserve_reserved, text, decoded);
    Ok(())
}

fn percent_run(units: &[u16], index: &mut usize) -> Result<(Vec<u8>, Vec<String>), VmError> {
    let mut bytes = Vec::new();
    let mut escapes = Vec::new();
    while units.get(*index) == Some(&u16::from(b'%')) {
        let escape = units.get(*index + 1..*index + 3).ok_or_else(uri_error)?;
        if !escape
            .iter()
            .all(|unit| char::from_u32(*unit as u32).is_some_and(|c| c.is_ascii_hexdigit()))
        {
            return Err(uri_error());
        }
        let text = String::from_utf16_lossy(escape);
        let byte = u8::from_str_radix(&text, 16).map_err(|_| uri_error())?;
        bytes.push(byte);
        escapes.push(String::from_utf16_lossy(&units[*index..*index + 3]));
        *index += 3;
    }
    Ok((bytes, escapes))
}

fn append_decoded(escapes: &[String], preserve_reserved: bool, text: &str, decoded: &mut Vec<u16>) {
    let mut index = 0;
    for character in text.chars() {
        if preserve_reserved && URI_RESERVED.contains(character) {
            decoded.extend(escapes[index].encode_utf16());
        } else {
            decoded.extend(character.to_string().encode_utf16());
        }
        index += character.len_utf8();
    }
}

fn uri_error() -> VmError {
    crate::value::error::throw_uri_error("URI malformed")
}
