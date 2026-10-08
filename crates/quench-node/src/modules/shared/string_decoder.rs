pub(crate) fn decode_chunk_units(prior_pending: &[u8], input: &[u8], encoding: &str, mode: DecodeMode) -> DecodedChunk {
    let streaming = matches!(mode, DecodeMode::Streaming);
    let mut bytes = Vec::with_capacity(prior_pending.len().saturating_add(input.len()));
    bytes.extend_from_slice(prior_pending);
    bytes.extend_from_slice(input);
    let had_pending = !prior_pending.is_empty();
    let (mut units, mut pending) = match encoding {
        "base64" | "base64url" => {
            let complete = if streaming { bytes.len() / 3 * 3 } else { bytes.len() };
            (crate::modules::buffer_enc::decode_units(&bytes[..complete], encoding), bytes[complete..].to_vec())
        }
        "hex" => {
            let complete = if streaming { bytes.len() / 2 * 2 } else { bytes.len() };
            (crate::modules::buffer_enc::decode_units(&bytes[..complete], encoding), bytes[complete..].to_vec())
        }
        "latin1" => (bytes.iter().map(|byte| u16::from(*byte)).collect(), Vec::new()),
        "ascii" => (bytes.iter().map(|byte| u16::from(byte & 0x7f)).collect(), Vec::new()),
        "utf16le" => {
            let mut complete = bytes.len() / 2 * 2;
            if complete >= 2 {
                let last = u16::from_le_bytes([bytes[complete - 2], bytes[complete - 1]]);
                if (0xd800..=0xdbff).contains(&last) && (had_pending || bytes.len() == complete) { complete -= 2; }
            }
            let decoded = bytes[..complete].chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            (decoded, bytes[complete..].to_vec())
        }
        _ => { let (text, pending) = decode_utf8(&bytes, had_pending, streaming); (text.encode_utf16().collect(), pending) }
    };
    if !streaming {
        match encoding {
            "utf8" if !pending.is_empty() => units.push(0xfffd),
            "utf16le" if pending.len() >= 2 => units.push(u16::from_le_bytes([pending[0], pending[1]])),
            _ => {}
        }
        pending.clear();
    }
    let last_total = match encoding {
        "utf8" => pending.first().map_or(0, |byte| match byte { 0xc0..=0xdf => 2, 0xe0..=0xef => 3, 0xf0..=0xf7 => 4, _ => 0 }),
        "utf16le" if pending.len() >= 2 => {
            let unit = u16::from_le_bytes([pending[0], pending[1]]);
            if (0xd800..=0xdbff).contains(&unit) { 4 } else { 2 }
        }
        "utf16le" if !pending.is_empty() => 2,
        _ => 0,
    };
    DecodedChunk { units, pending, last_total }
}

fn decode_utf8(bytes: &[u8], had_pending: bool, streaming: bool) -> (String, Vec<u8>) {
    if streaming && !had_pending && bytes.len() < 3 && bytes.first().is_some_and(|byte| (0xf5..=0xff).contains(byte)) {
        return (String::new(), bytes.to_vec());
    }
    let mut rest = bytes;
    let mut text = String::new();
    loop {
        match std::str::from_utf8(rest) {
            Ok(value) => { text.push_str(value); return (text, Vec::new()); }
            Err(error) if error.error_len().is_none() => {
                let valid = error.valid_up_to();
                text.push_str(&String::from_utf8_lossy(&rest[..valid]));
                return (text, rest[valid..].to_vec());
            }
            Err(error) => {
                let valid = error.valid_up_to();
                text.push_str(&String::from_utf8_lossy(&rest[..valid]));
                text.push('\u{fffd}');
                let skip = valid + error.error_len().unwrap_or(1);
                rest = &rest[skip.min(rest.len())..];
            }
        }
    }
}
