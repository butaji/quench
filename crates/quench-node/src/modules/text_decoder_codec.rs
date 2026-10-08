//! Runtime-neutral decoding operations shared by the legacy and shared Node
//! TextDecoder adapters.

pub(crate) fn decode_bytes(encoding: &str, bytes: &[u8], fatal: bool) -> Result<String, ()> {
    if encoding == "windows-1252" {
        // WHATWG-canonical windows-1252 decoder (encoding_rs).
        Ok(encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned())
    } else if fatal {
        String::from_utf8(bytes.to_vec()).map_err(|_| ())
    } else {
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

pub(crate) fn canonical_encoding(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" | "unicode-1-1-utf-8" => Some("utf-8"),
        "windows-1252" | "latin1" | "iso-8859-1" | "us-ascii" | "ascii" => Some("windows-1252"),
        _ => None,
    }
}
