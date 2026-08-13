fn alphabet_value(alphabet: &[u8; 64], byte: u8) -> Option<u32> {
    alphabet
        .iter()
        .position(|&entry| entry == byte)
        .map(|index| index as u32)
}

fn is_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0C | b'\r')
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && is_ascii_whitespace(bytes[index]) {
        index += 1;
    }
    index
}

#[derive(Clone, Copy, PartialEq)]
enum ChunkKind {
    Full,
    Padded,
    Partial,
    PartialOne,
    BadPadding,
}

fn take_group(
    alphabet: &[u8; 64],
    bytes: &[u8],
    start: usize,
) -> Result<(Vec<u8>, ChunkKind, usize), VmError> {
    let mut group = Vec::new();
    let mut index = start;
    while group.len() < 4 {
        index = skip_whitespace(bytes, index);
        if index >= bytes.len() || bytes[index] == b'=' {
            break;
        }
        let byte = bytes[index];
        if alphabet_value(alphabet, byte).is_none() {
            return Err(syntax_error());
        }
        group.push(byte);
        index += 1;
    }
    index = skip_whitespace(bytes, index);
    if bytes.get(index) != Some(&b'=') {
        let kind = match group.len() {
            4 => ChunkKind::Full,
            1 => ChunkKind::PartialOne,
            _ => ChunkKind::Partial,
        };
        return Ok((group, kind, index));
    }
    if group.len() < 2 {
        return Err(syntax_error());
    }
    let mut padding = 0;
    while bytes.get(index) == Some(&b'=') {
        padding += 1;
        index += 1;
    }
    if padding > 4 - group.len() {
        return Err(syntax_error());
    }
    index = skip_whitespace(bytes, index);
    let valid = padding == 4 - group.len() && index == bytes.len();
    let kind = if valid {
        ChunkKind::Padded
    } else {
        ChunkKind::BadPadding
    };
    Ok((group, kind, index))
}

fn decode_group(alphabet: &[u8; 64], group: &[u8], check_bits: bool) -> Option<Vec<u8>> {
    let mut values = [0u32; 4];
    for (index, byte) in group.iter().enumerate() {
        values[index] = alphabet_value(alphabet, *byte)?;
    }
    match group.len() {
        4 => Some(vec![
            (values[0] << 2 | values[1] >> 4) as u8,
            (values[1] << 4 | values[2] >> 2) as u8,
            (values[2] << 6 | values[3]) as u8,
        ]),
        3 if !check_bits || values[2] & 0b11 == 0 => Some(vec![
            (values[0] << 2 | values[1] >> 4) as u8,
            (values[1] << 4 | values[2] >> 2) as u8,
        ]),
        2 if !check_bits || values[1] & 0b1111 == 0 => {
            Some(vec![(values[0] << 2 | values[1] >> 4) as u8])
        }
        _ => None,
    }
}

fn decode_base64(input: &str, options: Base64Options, limit: usize) -> Decoded {
    let alphabet = if options.url_safe {
        BASE64URL_ALPHABET
    } else {
        BASE64_ALPHABET
    };
    let mut decoded = Decoded {
        bytes: Vec::new(),
        read: 0,
        failed: false,
    };
    while decoded.read < input.len() {
        if decoded.bytes.len() >= limit {
            break;
        }
        match take_group(alphabet, input.as_bytes(), decoded.read) {
            Ok((group, kind, next)) => {
                if !decode_chunk(options, &mut decoded, alphabet, &group, kind, next, limit) {
                    break;
                }
            }
            Err(_) => {
                decoded.failed = true;
                break;
            }
        }
    }
    decoded
}

fn decode_chunk(
    options: Base64Options,
    decoded: &mut Decoded,
    alphabet: &[u8; 64],
    group: &[u8],
    kind: ChunkKind,
    next: usize,
    limit: usize,
) -> bool {
    if matches!(
        kind,
        ChunkKind::Partial | ChunkKind::PartialOne | ChunkKind::BadPadding
    ) && options.last_chunk == LastChunk::StopBeforePartial
    {
        return false;
    }
    if matches!(kind, ChunkKind::PartialOne | ChunkKind::BadPadding) {
        decoded.failed = true;
        return false;
    }
    if kind == ChunkKind::Partial && options.last_chunk == LastChunk::Strict {
        decoded.failed = true;
        return false;
    }
    let strict = options.last_chunk == LastChunk::Strict && kind == ChunkKind::Padded;
    let Some(bytes) = decode_group(alphabet, group, strict) else {
        decoded.failed = true;
        return false;
    };
    if decoded.bytes.len() + bytes.len() > limit {
        return false;
    }
    decoded.bytes.extend_from_slice(&bytes);
    decoded.read = next;
    true
}

fn decode_hex(input: &str, limit: usize) -> Decoded {
    let bytes = input.as_bytes();
    let mut decoded = Decoded {
        bytes: Vec::new(),
        read: 0,
        failed: false,
    };
    if bytes.len() % 2 != 0 {
        decoded.failed = true;
        return decoded;
    }
    while decoded.read < bytes.len() {
        if decoded.bytes.len() >= limit {
            break;
        }
        let pair = (bytes[decoded.read] as char, bytes[decoded.read + 1] as char);
        let (Some(high), Some(low)) = (pair.0.to_digit(16), pair.1.to_digit(16)) else {
            decoded.failed = true;
            break;
        };
        decoded.bytes.push((high << 4 | low) as u8);
        decoded.read += 2;
    }
    decoded
}
