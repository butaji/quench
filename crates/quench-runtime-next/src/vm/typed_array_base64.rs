use super::*;

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

#[derive(Clone, Copy, PartialEq)]
enum LastChunk {
    Loose,
    Strict,
    StopBeforePartial,
}

#[derive(Clone, Copy)]
struct DecodeOptions {
    alphabet: &'static [u8; 64],
    last_chunk: LastChunk,
}

struct Decoded {
    bytes: Vec<u8>,
    read: usize,
    failed: bool,
}

impl<H: Host> Vm<H> {
    pub(super) fn typed_array_base64_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::Uint8ArrayFromBase64 | Native::Uint8ArrayFromHex => {
                let input =
                    self.base64_input(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let decoded = if native == Native::Uint8ArrayFromBase64 {
                    let options = self.base64_decode_options(p, args.get(1).copied())?;
                    decode_base64(&input, options, usize::MAX)
                } else {
                    decode_hex(&input, usize::MAX)
                };
                if decoded.failed {
                    return self.syntax_error_result(p, "invalid base64/hex input");
                }
                let target = self.construct_value(
                    p,
                    self.native_value(Native::Uint8Array),
                    &[Value::number(decoded.bytes.len() as f64)],
                )?;
                self.validate_typed_array_result(p, target, decoded.bytes.len(), true)?;
                for (index, byte) in decoded.bytes.into_iter().enumerate() {
                    self.typed_array_set(p, target, index, Value::number(byte as f64))?;
                }
                Ok(target)
            }
            Native::Uint8ArraySetFromBase64 | Native::Uint8ArraySetFromHex => {
                let (buffer, length) = self.uint8_base64_receiver(p, this)?;
                let input =
                    self.base64_input(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if matches!(
                    self.heap.get(buffer),
                    Some(Cell::ArrayBuffer {
                        immutable: true,
                        ..
                    })
                ) {
                    return Err(self.type_error(p, "Uint8Array buffer is immutable".into()));
                }
                let decoded = if native == Native::Uint8ArraySetFromBase64 {
                    let options = self.base64_decode_options(p, args.get(1).copied())?;
                    self.ensure_array_buffer_attached(p, buffer)?;
                    decode_base64(&input, options, length)
                } else {
                    self.ensure_array_buffer_attached(p, buffer)?;
                    decode_hex(&input, length)
                };
                for (index, byte) in decoded.bytes.iter().copied().enumerate() {
                    self.typed_array_set(p, this, index, Value::number(byte as f64))?;
                }
                if decoded.failed {
                    return self.syntax_error_result(p, "invalid base64/hex input");
                }
                let result = self.object();
                self.set_named(p, result, "read", Value::number(decoded.read as f64))?;
                self.set_named(
                    p,
                    result,
                    "written",
                    Value::number(decoded.bytes.len() as f64),
                )?;
                Ok(result)
            }
            Native::Uint8ArrayToBase64 | Native::Uint8ArrayToHex => {
                let (buffer, length) = self.uint8_base64_receiver(p, this)?;
                if native == Native::Uint8ArrayToHex {
                    self.ensure_array_buffer_attached(p, buffer)?;
                    let bytes = self.uint8_base64_bytes(this, length);
                    return Ok(self.heap.alloc(Cell::String(
                        bytes
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>()
                            .into(),
                    )));
                }
                let options = args.first().copied().unwrap_or(Value::UNDEFINED);
                let (alphabet, omit_padding) = self.base64_encode_options(p, options)?;
                self.ensure_array_buffer_attached(p, buffer)?;
                let bytes = self.uint8_base64_bytes(this, length);
                let mut encoded = encode_base64(&bytes, alphabet);
                if omit_padding {
                    encoded.truncate(encoded.trim_end_matches('=').len());
                }
                Ok(self.heap.alloc(Cell::String(encoded.into())))
            }
            _ => unreachable!("base64 native dispatch is exhaustive"),
        }
    }

    fn uint8_base64_bytes(&mut self, view: Value, length: usize) -> Vec<u8> {
        (0..length)
            .filter_map(|index| {
                self.typed_array_get(view, index)?
                    .as_number()
                    .map(|n| n as u8)
            })
            .collect()
    }

    fn uint8_base64_receiver(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<(Value, usize), JsError> {
        let Some(Cell::TypedArray {
            kind: TypedArrayKind::Uint8,
            buffer,
            ..
        }) = self.heap.get(receiver)
        else {
            return Err(self.type_error(
                p,
                "Uint8Array base64/hex method requires a Uint8Array receiver".into(),
            ));
        };
        let buffer = *buffer;
        let length = self.typed_array_length(receiver).unwrap_or_default();
        Ok((buffer, length))
    }

    fn ensure_array_buffer_attached(
        &mut self,
        p: &ResidualProgram,
        buffer: Value,
    ) -> Result<(), JsError> {
        if self.array_buffer_detached(buffer) {
            Err(self.type_error(p, "Uint8Array buffer is detached".into()))
        } else {
            Ok(())
        }
    }

    fn base64_input(&mut self, p: &ResidualProgram, value: Value) -> Result<String, JsError> {
        match self.heap.get(value) {
            Some(Cell::String(text)) => Ok(text.to_string()),
            _ => Err(self.type_error(p, "Uint8Array base64/hex input must be a string".into())),
        }
    }

    fn base64_option(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        name: &str,
    ) -> Result<Value, JsError> {
        if options.is_undefined() {
            return Ok(Value::UNDEFINED);
        }
        let object = self.box_object_or_type_error(p, options)?;
        let atom = self.intern_atom(name);
        self.get_property(p, object, atom)
    }

    fn base64_decode_options(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
    ) -> Result<DecodeOptions, JsError> {
        let options = options.unwrap_or(Value::UNDEFINED);
        let alphabet = self.base64_option(p, options, "alphabet")?;
        let alphabet = if alphabet.is_undefined() {
            BASE64
        } else {
            match self.base64_option_string(p, alphabet)?.as_str() {
                "base64" => BASE64,
                "base64url" => BASE64_URL,
                _ => return Err(self.type_error(p, "Invalid Uint8Array alphabet option".into())),
            }
        };
        let handling = self.base64_option(p, options, "lastChunkHandling")?;
        let last_chunk = if handling.is_undefined() {
            LastChunk::Loose
        } else {
            match self.base64_option_string(p, handling)?.as_str() {
                "loose" => LastChunk::Loose,
                "strict" => LastChunk::Strict,
                "stop-before-partial" => LastChunk::StopBeforePartial,
                _ => {
                    return Err(
                        self.type_error(p, "Invalid Uint8Array lastChunkHandling option".into())
                    );
                }
            }
        };
        Ok(DecodeOptions {
            alphabet,
            last_chunk,
        })
    }

    fn base64_encode_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<(&'static [u8; 64], bool), JsError> {
        let alphabet = self.base64_option(p, options, "alphabet")?;
        let alphabet = if alphabet.is_undefined() {
            BASE64
        } else {
            match self.base64_option_string(p, alphabet)?.as_str() {
                "base64" => BASE64,
                "base64url" => BASE64_URL,
                _ => return Err(self.type_error(p, "Invalid Uint8Array alphabet option".into())),
            }
        };
        let omit = self.base64_option(p, options, "omitPadding")?;
        Ok((alphabet, self.truthy(omit)))
    }

    fn base64_option_string(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        match self.heap.get(value) {
            Some(Cell::String(text)) => Ok(text.to_string()),
            _ => Err(self.type_error(p, "Uint8Array option must be a string".into())),
        }
    }
}

fn alphabet_value(alphabet: &[u8; 64], byte: u8) -> Option<u32> {
    alphabet
        .iter()
        .position(|&entry| entry == byte)
        .map(|index| index as u32)
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0c | b'\r')
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && is_whitespace(bytes[index]) {
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
    PartialPadded,
    BadPadding,
}

fn take_group(
    alphabet: &[u8; 64],
    bytes: &[u8],
    start: usize,
) -> Option<(Vec<u8>, ChunkKind, usize)> {
    let mut group = Vec::new();
    let mut index = start;
    while group.len() < 4 {
        index = skip_whitespace(bytes, index);
        if index >= bytes.len() || bytes[index] == b'=' {
            break;
        }
        if alphabet_value(alphabet, bytes[index]).is_none() {
            return None;
        }
        group.push(bytes[index]);
        index += 1;
    }
    index = skip_whitespace(bytes, index);
    if bytes.get(index) != Some(&b'=') {
        let kind = match group.len() {
            4 => ChunkKind::Full,
            1 => ChunkKind::PartialOne,
            _ => ChunkKind::Partial,
        };
        return Some((group, kind, index));
    }
    if group.len() < 2 {
        return None;
    }
    let start_padding = index;
    while bytes.get(index) == Some(&b'=') {
        index += 1;
    }
    let padding = index - start_padding;
    index = skip_whitespace(bytes, index);
    let expected_padding = 4 - group.len();
    let kind = if padding == expected_padding && index == bytes.len() {
        ChunkKind::Padded
    } else if padding < expected_padding && index == bytes.len() {
        ChunkKind::PartialPadded
    } else {
        ChunkKind::BadPadding
    };
    Some((group, kind, index))
}

fn decode_group(alphabet: &[u8; 64], group: &[u8], strict: bool) -> Option<Vec<u8>> {
    let mut value = [0u32; 4];
    for (index, byte) in group.iter().enumerate() {
        value[index] = alphabet_value(alphabet, *byte)?;
    }
    match group.len() {
        4 => Some(vec![
            (value[0] << 2 | value[1] >> 4) as u8,
            (value[1] << 4 | value[2] >> 2) as u8,
            (value[2] << 6 | value[3]) as u8,
        ]),
        3 if !strict || value[2] & 3 == 0 => Some(vec![
            (value[0] << 2 | value[1] >> 4) as u8,
            (value[1] << 4 | value[2] >> 2) as u8,
        ]),
        2 if !strict || value[1] & 15 == 0 => Some(vec![(value[0] << 2 | value[1] >> 4) as u8]),
        _ => None,
    }
}

fn decode_base64(input: &str, options: DecodeOptions, limit: usize) -> Decoded {
    let mut decoded = Decoded {
        bytes: Vec::new(),
        read: 0,
        failed: false,
    };
    while decoded.read < input.len() && decoded.bytes.len() < limit {
        let Some((group, kind, next)) =
            take_group(options.alphabet, input.as_bytes(), decoded.read)
        else {
            decoded.failed = true;
            break;
        };
        if kind == ChunkKind::BadPadding {
            decoded.failed = true;
            break;
        }
        if matches!(
            kind,
            ChunkKind::Partial | ChunkKind::PartialOne | ChunkKind::PartialPadded
        ) && options.last_chunk == LastChunk::StopBeforePartial
        {
            break;
        }
        if matches!(kind, ChunkKind::PartialOne | ChunkKind::PartialPadded)
            || (kind == ChunkKind::Partial && options.last_chunk == LastChunk::Strict)
        {
            decoded.failed = true;
            break;
        }
        let strict = options.last_chunk == LastChunk::Strict && kind == ChunkKind::Padded;
        let Some(bytes) = decode_group(options.alphabet, &group, strict) else {
            decoded.failed = true;
            break;
        };
        if decoded.bytes.len() + bytes.len() > limit {
            break;
        }
        decoded.bytes.extend_from_slice(&bytes);
        decoded.read = next;
    }
    decoded
}

fn decode_hex(input: &str, limit: usize) -> Decoded {
    let bytes = input.as_bytes();
    let mut decoded = Decoded {
        bytes: Vec::new(),
        read: 0,
        failed: false,
    };
    if !bytes.len().is_multiple_of(2) {
        decoded.failed = true;
        return decoded;
    }
    while decoded.read < bytes.len() && decoded.bytes.len() < limit {
        let pair = (bytes[decoded.read] as char, bytes[decoded.read + 1] as char);
        let (Some(high), Some(low)) = (pair.0.to_digit(16), pair.1.to_digit(16)) else {
            decoded.failed = true;
            break;
        };
        decoded.bytes.push((high * 16 + low) as u8);
        decoded.read += 2;
    }
    decoded
}

fn encode_base64(bytes: &[u8], alphabet: &[u8; 64]) -> String {
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(*chunk.get(1).unwrap_or(&0));
        let third = u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(alphabet[(first >> 2) as usize] as char);
        output.push(alphabet[((first << 4 | second >> 4) & 0x3f) as usize] as char);
        match chunk.len() {
            3 => {
                output.push(alphabet[((second << 2 | third >> 6) & 0x3f) as usize] as char);
                output.push(alphabet[(third & 0x3f) as usize] as char);
            }
            2 => {
                output.push(alphabet[((second << 2) & 0x3f) as usize] as char);
                output.push('=');
            }
            _ => output.push_str("=="),
        }
    }
    output
}
