use std::collections::HashMap;

/// Parse the default Node querystring form into ordered keys and values.
pub(crate) fn parse_default_entries(input: &str) -> Vec<(String, Vec<String>)> {
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    let mut indices: HashMap<String, usize> = HashMap::new();
    for pair in input.split('&').filter(|pair| !pair.is_empty()).take(1000) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode_component(key);
        let value = decode_component(value);
        if let Some(index) = indices.get(&key).copied() {
            entries[index].1.push(value);
        } else {
            indices.insert(key.clone(), entries.len());
            entries.push((key, vec![value]));
        }
    }
    entries
}

fn decode_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => { decoded.push(b' '); index += 1; }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    decoded.push((high << 4) | low); index += 3;
                } else { decoded.push(bytes[index]); index += 1; }
            }
            byte => { decoded.push(byte); index += 1; }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte { b'0'..=b'9' => Some(byte - b'0'), b'a'..=b'f' => Some(byte - b'a' + 10), b'A'..=b'F' => Some(byte - b'A' + 10), _ => None }
}
