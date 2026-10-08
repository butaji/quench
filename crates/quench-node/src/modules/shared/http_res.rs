pub(crate) fn is_http_token_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character,
        '!' | '#' | '$' | '%' | '&' | '\'' | '*' | '+' | '-' | '.' | '^' | '_' | '`' | '|' | '~')
}

pub(crate) fn valid_header_value(value: &str) -> bool {
    !value.chars().any(|character| (character != '\t' && (character as u32) < 0x20) || character as u32 > 0xff)
}

pub(crate) fn compose(status: u16, text: &str, headers: &[(String, String)], body: &[u8], _trailers: &[(String, String)], keep_alive: bool, http10: bool, send_date: bool) -> Vec<u8> {
    let text = if text.is_empty() { "OK" } else { text };
    let mut out = format!("HTTP/1.1 {status} {text}\r\n").into_bytes();
    let chunked = headers.iter().any(|(key, value)| key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked"));
    if !http10 && !chunked && !headers.iter().any(|(key, _)| key.eq_ignore_ascii_case("content-length")) {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    if send_date && !headers.iter().any(|(key, _)| key.eq_ignore_ascii_case("date")) {
        out.extend_from_slice(b"Date: Thu, 01 Jan 1970 00:00:00 GMT\r\n");
    }
    let connection = headers.iter().find(|(key, _)| key.eq_ignore_ascii_case("connection")).map(|(_, value)| value.as_str()).unwrap_or(if keep_alive { "keep-alive" } else { "close" });
    let transfer = headers.iter().filter(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding")).collect::<Vec<_>>();
    for (key, value) in headers {
        if !key.eq_ignore_ascii_case("transfer-encoding") { out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes()); }
    }
    out.extend_from_slice(format!("Connection: {connection}\r\n").as_bytes());
    for (key, value) in transfer { out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes()); }
    out.extend_from_slice(b"\r\n");
    if chunked { if !body.is_empty() { out.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes()); out.extend_from_slice(body); out.extend_from_slice(b"\r\n"); } out.extend_from_slice(b"0\r\n\r\n"); }
    else { out.extend_from_slice(body); }
    out
}
