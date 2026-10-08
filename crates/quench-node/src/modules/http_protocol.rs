//! Pure HTTP/1 protocol facts and wire-format helpers shared by both Node
//! host implementations.

pub(crate) const HTTP_METHODS: &[&str] = &[
    "ACL",
    "BIND",
    "CHECKOUT",
    "CONNECT",
    "COPY",
    "DELETE",
    "GET",
    "HEAD",
    "LINK",
    "LOCK",
    "M-SEARCH",
    "MERGE",
    "MKACTIVITY",
    "MKCALENDAR",
    "MKCOL",
    "MOVE",
    "NOTIFY",
    "OPTIONS",
    "PATCH",
    "POST",
    "PROPFIND",
    "PROPPATCH",
    "PURGE",
    "PUT",
    "QUERY",
    "REBIND",
    "REPORT",
    "SEARCH",
    "SOURCE",
    "SUBSCRIBE",
    "TRACE",
    "UNBIND",
    "UNLINK",
    "UNLOCK",
    "UNSUBSCRIBE",
];

pub(crate) const NON_REPEATABLE_HEADERS: &[&str] = &[
    "content-type",
    "user-agent",
    "referer",
    "host",
    "authorization",
    "proxy-authorization",
    "if-modified-since",
    "if-unmodified-since",
    "from",
    "location",
    "max-forwards",
    "retry-after",
    "etag",
    "last-modified",
    "server",
    "age",
    "expires",
];

pub(crate) enum HeaderIssue {
    InvalidName(String),
    MissingValue(String),
    InvalidContent(String),
}

impl HeaderIssue {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::InvalidName(_) => "ERR_INVALID_HTTP_TOKEN",
            Self::MissingValue(_) => "ERR_HTTP_INVALID_HEADER_VALUE",
            Self::InvalidContent(_) => "ERR_INVALID_CHAR",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidName(name) => {
                format!("Header name must be a valid HTTP token [\"{name}\"]")
            }
            Self::MissingValue(name) => {
                format!("Invalid value \"undefined\" for header \"{name}\"")
            }
            Self::InvalidContent(name) => {
                format!("Invalid character in header content [\"{name}\"]")
            }
        }
    }
}

pub(crate) fn is_http_token_char(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '.'
                | '^'
                | '_'
                | '`'
                | '|'
                | '~'
        )
}

pub(crate) fn valid_header_value(value: &str) -> bool {
    !value.chars().any(|character| {
        (character != '\t' && (character as u32) < 0x20) || character as u32 > 0xFF
    })
}

pub(crate) fn default_status_message(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        418 => "I'm a Teapot",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "unknown",
    }
}

pub(crate) fn compose(
    status: u16,
    text: &str,
    headers: &[(String, String)],
    body: &[u8],
    _trailers: &[(String, String)],
    keep_alive: bool,
    http10: bool,
    send_date: bool,
) -> Vec<u8> {
    let text = if text.is_empty() { "OK" } else { text };
    let mut out = format!("HTTP/1.1 {status} {text}\r\n").into_bytes();
    let chunked = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    });
    if !http10
        && !chunked
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    if send_date
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("date"))
    {
        out.extend_from_slice(b"Date: Thu, 01 Jan 1970 00:00:00 GMT\r\n");
    }
    let connection = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("connection"))
        .map(|(_, value)| value.as_str())
        .unwrap_or(if keep_alive { "keep-alive" } else { "close" });
    let transfer_encoding = headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
        .collect::<Vec<_>>();
    for (key, value) in headers {
        if key.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("Connection: {connection}\r\n").as_bytes());
    for (key, value) in transfer_encoding {
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if chunked {
        out.extend_from_slice(&chunk_frame(body));
    } else {
        out.extend_from_slice(body);
    }
    out
}

pub(crate) fn chunk_frame(body: &[u8]) -> Vec<u8> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut frame = format!("{:x}\r\n", body.len()).into_bytes();
    frame.extend_from_slice(body);
    frame.extend_from_slice(b"\r\n");
    frame
}

pub(crate) fn chunk_terminator(trailers: &[(String, String)]) -> Vec<u8> {
    let mut out = b"0\r\n".to_vec();
    for (name, value) in trailers {
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out
}

pub(crate) fn valid_status_number(number: f64) -> Option<u16> {
    (number.is_finite() && number.fract() == 0.0 && (100.0..=999.0).contains(&number))
        .then_some(number as u16)
}

pub(crate) fn request_head_with_chunking(
    host: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body_len: usize,
    omit_host: bool,
    force_chunked: bool,
) -> String {
    let mut head = format!("{method} {path} HTTP/1.1\r\n");
    if !omit_host {
        head.push_str(&format!("Host: {host}\r\n"));
    }
    for (key, value) in headers {
        head.push_str(&format!("{key}: {value}\r\n"));
    }
    // Node's implicit zero-length framing is method-dependent: bodyless
    // methods omit `content-length`, while methods that conventionally carry
    // an entity advertise an empty body as `content-length: 0`.
    let has_content_length = headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("content-length"));
    let expect_continue = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("expect") && value.eq_ignore_ascii_case("100-continue")
    });
    if !has_content_length && expect_continue {
        if !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
        {
            head.push_str("Transfer-Encoding: chunked\r\n");
        }
    } else if force_chunked && !has_content_length {
        head.push_str("Transfer-Encoding: chunked\r\n");
    } else if !has_content_length && (body_len > 0 || default_empty_body(method)) {
        head.push_str(&format!("Content-Length: {body_len}\r\n"));
    }
    if !headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("connection"))
    {
        head.push_str("Connection: keep-alive\r\n");
    }
    head.push_str("\r\n");
    head
}

fn default_empty_body(method: &str) -> bool {
    matches!(method.to_ascii_uppercase().as_str(), "POST" | "PUT")
}
