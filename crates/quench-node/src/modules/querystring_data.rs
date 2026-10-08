//! Runtime-neutral query-string scanning and default decoding.
//!
//! Shared NodeHost adapters lower ordered entries into runtime objects. The
//! scan and built-in decoder stay independent of guest value representation.

use std::collections::HashMap;

pub(crate) const SEP_DEFAULT: &[u16] = &[38]; // '&'
pub(crate) const EQ_DEFAULT: &[u16] = &[61]; // '='
pub(crate) const PLUS_DECODED: &[u16] = &[32]; // ' '
pub(crate) const PLUS_ENCODED: &[u16] = &[37, 50, 48]; // '%20'
pub(crate) const MAX_KEYS_DEFAULT: i64 = 1000;
const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

pub(crate) fn is_hex(unit: u16) -> bool {
    unhex(unit) >= 0
}

/// Node's query-string component encoder, including the querystring-safe
/// punctuation left unescaped by `encodeURIComponent`.
pub(crate) fn encode_component(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.bytes() {
        if is_querystring_safe(byte) {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
            output.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
        }
    }
    output
}

fn is_querystring_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte)
}

/// Parse query data into insertion-ordered unique keys and their values.
///
/// `decode_component` is called only for components containing a complete
/// `%XX` sequence. Custom decoders and their fallback policy are supplied by
/// the caller; ordinary Node query parsing uses `parse_default_entries`.
pub(crate) fn parse_entries(
    units: &[u16],
    separator: &[u16],
    equality: &[u16],
    pairs: i64,
    custom_decoder: bool,
    mut decode_component: impl FnMut(&[u16]) -> String,
) -> Vec<(String, Vec<String>)> {
    if units.is_empty() {
        return Vec::new();
    }
    let mut scan = Scan::new(units, separator, equality, pairs, custom_decoder);
    let mut entries = Vec::new();
    let mut indices = HashMap::new();
    if !scan.run(&mut entries, &mut indices, &mut decode_component) {
        finish_scan(
            &mut scan,
            units,
            &mut entries,
            &mut indices,
            &mut decode_component,
        );
    }
    entries
}

/// Node's default `querystring.parse` decoder as ordered data.
pub(crate) fn parse_default_entries(input: &str) -> Vec<(String, Vec<String>)> {
    let units = input.encode_utf16().collect::<Vec<_>>();
    parse_entries(
        &units,
        SEP_DEFAULT,
        EQ_DEFAULT,
        MAX_KEYS_DEFAULT,
        false,
        |component| decode_default_component(component, false),
    )
}

/// Node's default strict URI-component decode with querystring's byte-level
/// malformed-input fallback.
pub(crate) fn decode_default_component(units: &[u16], decode_spaces: bool) -> String {
    decode_uri_component(units).unwrap_or_else(|| {
        String::from_utf8_lossy(&unescape_buffer_units(units, decode_spaces)).into_owned()
    })
}

/// Byte-oriented Node `unescapeBuffer`; malformed percent sequences pass
/// through literally and non-ASCII UTF-16 units are narrowed to bytes.
pub(crate) fn unescape_buffer_units(units: &[u16], decode_spaces: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len());
    let mut index = 0;
    let max_length = units.len().saturating_sub(2);
    while index < units.len() {
        let mut current = units[index];
        if current == 43 && decode_spaces {
            out.push(32);
            index += 1;
            continue;
        }
        if current == 37 && index < max_length {
            index += 1;
            current = units[index];
            let high = unhex(current);
            if high < 0 {
                out.push(37);
                continue;
            }
            index += 1;
            let low = unhex(units[index]);
            if low < 0 {
                out.push(37);
                index -= 1;
            } else {
                current = (high * 16 + low) as u16;
            }
        }
        out.push(current as u8);
        index += 1;
    }
    out
}

fn decode_uri_component(units: &[u16]) -> Option<String> {
    let mut decoded = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units[index] != 37 {
            decoded.push(units[index]);
            index += 1;
            continue;
        }

        let mut bytes = Vec::new();
        while units.get(index) == Some(&37) {
            let high = hex_value(*units.get(index + 1)?)?;
            let low = hex_value(*units.get(index + 2)?)?;
            bytes.push((high << 4) | low);
            index += 3;
        }
        let text = std::str::from_utf8(&bytes).ok()?;
        decoded.extend(text.encode_utf16());
    }
    Some(String::from_utf16_lossy(&decoded))
}

fn hex_value(unit: u16) -> Option<u8> {
    match unit {
        0x30..=0x39 => Some((unit - 0x30) as u8),
        0x41..=0x46 => Some((unit - 0x41 + 10) as u8),
        0x61..=0x66 => Some((unit - 0x61 + 10) as u8),
        _ => None,
    }
}

fn unhex(unit: u16) -> i32 {
    match unit {
        0x30..=0x39 => i32::from(unit - 0x30),
        0x41..=0x46 => i32::from(unit - 0x41 + 10),
        0x61..=0x66 => i32::from(unit - 0x61 + 10),
        _ => -1,
    }
}

struct Scan<'a> {
    units: &'a [u16],
    separator: &'a [u16],
    equality: &'a [u16],
    last_pos: usize,
    separator_index: usize,
    equality_index: usize,
    key: Vec<u16>,
    value: Vec<u16>,
    key_encoded: bool,
    value_encoded: bool,
    encode_check: u8,
    pairs: i64,
    custom_decoder: bool,
}

impl<'a> Scan<'a> {
    fn new(
        units: &'a [u16],
        separator: &'a [u16],
        equality: &'a [u16],
        pairs: i64,
        custom_decoder: bool,
    ) -> Self {
        Self {
            units,
            separator,
            equality,
            last_pos: 0,
            separator_index: 0,
            equality_index: 0,
            key: Vec::new(),
            value: Vec::new(),
            key_encoded: custom_decoder,
            value_encoded: custom_decoder,
            encode_check: 0,
            pairs,
            custom_decoder,
        }
    }

    fn run(
        &mut self,
        entries: &mut Vec<(String, Vec<String>)>,
        indices: &mut HashMap<String, usize>,
        decode_component: &mut impl FnMut(&[u16]) -> String,
    ) -> bool {
        for (index, &code) in self.units.iter().enumerate() {
            if self.separator.get(self.separator_index) == Some(&code) {
                if self.on_separator(index, entries, indices, decode_component) {
                    return true;
                }
            } else {
                self.on_other(index, code);
            }
        }
        false
    }

    fn on_separator(
        &mut self,
        index: usize,
        entries: &mut Vec<(String, Vec<String>)>,
        indices: &mut HashMap<String, usize>,
        decode_component: &mut impl FnMut(&[u16]) -> String,
    ) -> bool {
        self.separator_index += 1;
        if self.separator_index != self.separator.len() {
            return false;
        }
        let end = index + 1 - self.separator_index;
        if self.equality_index < self.equality.len() {
            if self.last_pos < end {
                self.key.extend_from_slice(&self.units[self.last_pos..end]);
            } else if self.key.is_empty() {
                self.pairs -= 1;
                if self.pairs == 0 {
                    return true;
                }
                self.reset_pair(index + 1);
                return false;
            }
        } else if self.last_pos < end {
            self.value
                .extend_from_slice(&self.units[self.last_pos..end]);
        }
        self.flush(entries, indices, decode_component);
        self.pairs -= 1;
        if self.pairs == 0 {
            return true;
        }
        self.reset_pair(index + 1);
        false
    }

    fn reset_pair(&mut self, next: usize) {
        self.last_pos = next;
        self.separator_index = 0;
        self.equality_index = 0;
    }

    fn flush(
        &mut self,
        entries: &mut Vec<(String, Vec<String>)>,
        indices: &mut HashMap<String, usize>,
        decode_component: &mut impl FnMut(&[u16]) -> String,
    ) {
        add_key_value(
            entries,
            indices,
            &self.key,
            &self.value,
            self.key_encoded,
            self.value_encoded,
            decode_component,
        );
        self.key_encoded = self.custom_decoder;
        self.value_encoded = self.custom_decoder;
        self.key.clear();
        self.value.clear();
        self.encode_check = 0;
    }

    fn on_other(&mut self, index: usize, code: u16) {
        self.separator_index = 0;
        if self.equality_index < self.equality.len() && self.on_equality_char(index, code) {
            return;
        }
        if code == 43 {
            if self.last_pos < index {
                self.value
                    .extend_from_slice(&self.units[self.last_pos..index]);
            }
            if self.custom_decoder {
                self.value.extend_from_slice(PLUS_ENCODED);
            } else {
                self.value.extend_from_slice(PLUS_DECODED);
            }
            self.last_pos = index + 1;
        } else if !self.value_encoded {
            self.track_value_encoding(code);
        }
    }

    fn on_equality_char(&mut self, index: usize, code: u16) -> bool {
        if self.equality.get(self.equality_index) == Some(&code) {
            self.equality_index += 1;
            if self.equality_index == self.equality.len() {
                let end = index + 1 - self.equality_index;
                if self.last_pos < end {
                    self.key.extend_from_slice(&self.units[self.last_pos..end]);
                }
                self.encode_check = 0;
                self.last_pos = index + 1;
            }
            return true;
        }
        self.equality_index = 0;
        if !self.key_encoded && self.track_key_encoding(code) {
            return true;
        }
        if code == 43 {
            if self.last_pos < index {
                self.key
                    .extend_from_slice(&self.units[self.last_pos..index]);
            }
            if self.custom_decoder {
                self.key.extend_from_slice(PLUS_ENCODED);
            } else {
                self.key.extend_from_slice(PLUS_DECODED);
            }
            self.last_pos = index + 1;
        }
        true
    }

    fn track_key_encoding(&mut self, code: u16) -> bool {
        if code == 37 {
            self.encode_check = 1;
            return true;
        }
        if self.encode_check == 0 {
            return false;
        }
        if is_hex(code) {
            self.encode_check += 1;
            if self.encode_check == 3 {
                self.key_encoded = true;
            }
            return true;
        }
        self.encode_check = 0;
        false
    }

    fn track_value_encoding(&mut self, code: u16) {
        if code == 37 {
            self.encode_check = 1;
        } else if self.encode_check > 0 {
            if is_hex(code) {
                self.encode_check += 1;
                if self.encode_check == 3 {
                    self.value_encoded = true;
                }
            } else {
                self.encode_check = 0;
            }
        }
    }
}

fn finish_scan(
    scan: &mut Scan<'_>,
    units: &[u16],
    entries: &mut Vec<(String, Vec<String>)>,
    indices: &mut HashMap<String, usize>,
    decode_component: &mut impl FnMut(&[u16]) -> String,
) {
    if scan.last_pos < units.len() {
        if scan.equality_index < scan.equality.len() {
            scan.key.extend_from_slice(&units[scan.last_pos..]);
        } else if scan.separator_index < scan.separator.len() {
            scan.value.extend_from_slice(&units[scan.last_pos..]);
        }
    } else if scan.equality_index == 0 && scan.key.is_empty() {
        return;
    }
    scan.flush(entries, indices, decode_component);
}

fn add_key_value(
    entries: &mut Vec<(String, Vec<String>)>,
    indices: &mut HashMap<String, usize>,
    key: &[u16],
    value: &[u16],
    key_encoded: bool,
    value_encoded: bool,
    decode_component: &mut impl FnMut(&[u16]) -> String,
) {
    let key = if !key.is_empty() && key_encoded {
        decode_component(key)
    } else {
        String::from_utf16_lossy(key)
    };
    let value = if !value.is_empty() && value_encoded {
        decode_component(value)
    } else {
        String::from_utf16_lossy(value)
    };
    if let Some(index) = indices.get(&key).copied() {
        entries[index].1.push(value);
    } else {
        indices.insert(key.clone(), entries.len());
        entries.push((key, vec![value]));
    }
}
