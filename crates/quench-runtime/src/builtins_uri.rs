use crate::execute::VmError;

const URI_RESERVED: &str = ";/?:@&=+$,#";

pub(crate) fn encode_uri(value: Option<&Value>, preserve_reserved: bool) -> Result<Value, VmError> {
    let source = uri_source(value)?;
    Ok(Value::String(encode(&source, preserve_reserved)))
}

pub(crate) fn decode_uri(value: Option<&Value>, preserve_reserved: bool) -> Result<Value, VmError> {
    let source = uri_source(value)?;
    Ok(Value::String(decode(&source, preserve_reserved)?))
}

fn uri_source(value: Option<&Value>) -> Result<String, VmError> {
    match value {
        Some(value) => crate::conversion::to_string(value),
        None => crate::conversion::to_string(&Value::Undefined),
    }
}

fn encode(source: &str, preserve_reserved: bool) -> String {
    let mut encoded = String::new();
    for character in source.chars() {
        if uri_unescaped(character, preserve_reserved) {
            encoded.push(character);
        } else {
            encode_character(&mut encoded, character);
        }
    }
    encoded
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

fn decode(source: &str, preserve_reserved: bool) -> Result<String, VmError> {
    let mut decoded = String::new();
    let mut index = 0;
    while index < source.len() {
        if source.as_bytes()[index] == b'%' {
            decode_run(source, &mut index, preserve_reserved, &mut decoded)?;
        } else {
            copy_character(source, &mut index, &mut decoded);
        }
    }
    Ok(decoded)
}

fn copy_character(source: &str, index: &mut usize, decoded: &mut String) {
    if let Some(character) = source[*index..].chars().next() {
        decoded.push(character);
        *index += character.len_utf8();
    }
}

fn decode_run(
    source: &str,
    index: &mut usize,
    preserve_reserved: bool,
    decoded: &mut String,
) -> Result<(), VmError> {
    let (bytes, escapes) = percent_run(source, index)?;
    std::str::from_utf8(&bytes).map_err(|_| uri_error())?;
    append_decoded(&bytes, &escapes, preserve_reserved, decoded);
    Ok(())
}

fn percent_run<'a>(source: &'a str, index: &mut usize) -> Result<(Vec<u8>, Vec<&'a str>), VmError> {
    let mut bytes = Vec::new();
    let mut escapes = Vec::new();
    while source.as_bytes().get(*index) == Some(&b'%') {
        let escape = source.get(*index..*index + 3).ok_or_else(uri_error)?;
        let byte = u8::from_str_radix(&escape[1..], 16).map_err(|_| uri_error())?;
        bytes.push(byte);
        escapes.push(escape);
        *index += 3;
    }
    Ok((bytes, escapes))
}

fn append_decoded(bytes: &[u8], escapes: &[&str], preserve_reserved: bool, decoded: &mut String) {
    let mut index = 0;
    while index < bytes.len() {
        if preserve_reserved && URI_RESERVED.contains(bytes[index] as char) {
            decoded.push_str(escapes[index]);
            index += 1;
        } else {
            append_character(bytes, &mut index, decoded);
        }
    }
}

fn append_character(bytes: &[u8], index: &mut usize, decoded: &mut String) {
    if let Ok(text) = std::str::from_utf8(&bytes[*index..]) {
        if let Some(character) = text.chars().next() {
            decoded.push(character);
            *index += character.len_utf8();
        }
    }
}

fn uri_error() -> VmError {
    crate::value::error::throw_uri_error("URI malformed")
}
