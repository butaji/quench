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

/// RFC 7230 field-vchar/obs-text, or Fetch's relaxed byte-value rules.
pub(crate) fn valid_header_unit(unit: u16, lenient: bool) -> bool {
    const OBS_TEXT_START: u16 = 0x80;
    let byte = unit <= u16::from(u8::MAX);
    if lenient {
        byte && ![b'\0', b'\r', b'\n']
            .iter()
            .any(|value| unit == u16::from(*value))
    } else {
        unit == u16::from(b'\t')
            || (u16::from(b' ')..=u16::from(b'~')).contains(&unit)
            || (OBS_TEXT_START..=u16::from(u8::MAX)).contains(&unit)
    }
}

pub(crate) fn valid_header_value(value: &str) -> bool {
    value
        .encode_utf16()
        .all(|unit| valid_header_unit(unit, false))
}

/// Initial entries of Node's mutable public STATUS_CODES object.
pub(crate) const STATUS_CODES: &[(u16, &str)] = &[
    (100, "Continue"),
    (101, "Switching Protocols"),
    (102, "Processing"),
    (103, "Early Hints"),
    (200, "OK"),
    (201, "Created"),
    (202, "Accepted"),
    (203, "Non-Authoritative Information"),
    (204, "No Content"),
    (205, "Reset Content"),
    (206, "Partial Content"),
    (207, "Multi-Status"),
    (208, "Already Reported"),
    (226, "IM Used"),
    (300, "Multiple Choices"),
    (301, "Moved Permanently"),
    (302, "Found"),
    (303, "See Other"),
    (304, "Not Modified"),
    (305, "Use Proxy"),
    (307, "Temporary Redirect"),
    (308, "Permanent Redirect"),
    (400, "Bad Request"),
    (401, "Unauthorized"),
    (402, "Payment Required"),
    (403, "Forbidden"),
    (404, "Not Found"),
    (405, "Method Not Allowed"),
    (406, "Not Acceptable"),
    (407, "Proxy Authentication Required"),
    (408, "Request Timeout"),
    (409, "Conflict"),
    (410, "Gone"),
    (411, "Length Required"),
    (412, "Precondition Failed"),
    (413, "Payload Too Large"),
    (414, "URI Too Long"),
    (415, "Unsupported Media Type"),
    (416, "Range Not Satisfiable"),
    (417, "Expectation Failed"),
    (418, "I'm a Teapot"),
    (421, "Misdirected Request"),
    (422, "Unprocessable Entity"),
    (423, "Locked"),
    (424, "Failed Dependency"),
    (425, "Too Early"),
    (426, "Upgrade Required"),
    (428, "Precondition Required"),
    (429, "Too Many Requests"),
    (431, "Request Header Fields Too Large"),
    (451, "Unavailable For Legal Reasons"),
    (500, "Internal Server Error"),
    (501, "Not Implemented"),
    (502, "Bad Gateway"),
    (503, "Service Unavailable"),
    (504, "Gateway Timeout"),
    (505, "HTTP Version Not Supported"),
    (506, "Variant Also Negotiates"),
    (507, "Insufficient Storage"),
    (508, "Loop Detected"),
    (509, "Bandwidth Limit Exceeded"),
    (510, "Not Extended"),
    (511, "Network Authentication Required"),
];

pub(crate) fn compose(
    status: u16,
    text: &[u8],
    headers: &[(String, String)],
    body: &[u8],
    _trailers: &[(String, String)],
    keep_alive: bool,
    omit_connection: bool,
    omit_auto_content_length: bool,
    http10: bool,
    send_date: bool,
    now: std::time::SystemTime,
) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} ").into_bytes();
    out.extend_from_slice(text);
    out.extend_from_slice(b"\r\n");
    let chunked = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    });
    // Node serializes caller headers first, then adds Date and framing headers.
    // Keeping this order matters to rawHeaders consumers, especially for
    // writeHead() arrays where duplicate header pairs must remain adjacent.
    for (key, value) in headers
        .iter()
        .filter(|(key, _)| !key.eq_ignore_ascii_case("transfer-encoding"))
    {
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    if send_date
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("date"))
    {
        let now: chrono::DateTime<chrono::Utc> = now.into();
        out.extend_from_slice(
            format!("Date: {}\r\n", now.format("%a, %d %b %Y %H:%M:%S GMT")).as_bytes(),
        );
    }
    let connection = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("connection"))
        .map(|(_, value)| value.as_str())
        .unwrap_or(if keep_alive { "keep-alive" } else { "close" });
    if !omit_connection
        && !headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("connection"))
    {
        out.extend_from_slice(format!("Connection: {connection}\r\n").as_bytes());
    }
    for (key, value) in headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
    {
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    if !omit_auto_content_length
        && !http10
        && !chunked
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
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
    set_default_headers: bool,
    force_chunked: bool,
) -> String {
    let mut head = format!("{method} {path} HTTP/1.1\r\n");
    for (key, value) in headers {
        head.push_str(&format!("{key}: {value}\r\n"));
    }
    // Node writes the caller's headers first, followed by its implicit Host
    // header and then the generated framing/connection headers. Keep this
    // order observable through the peer's rawHeaders array.
    if !omit_host {
        head.push_str(&format!("Host: {host}\r\n"));
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
    if set_default_headers {
        if !has_content_length && expect_continue {
            if !headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
            {
                head.push_str("Transfer-Encoding: chunked\r\n");
            }
        } else if force_chunked && !has_content_length {
            head.push_str("Transfer-Encoding: chunked\r\n");
        } else if !has_content_length && default_request_chunking(method) {
            head.push_str(&format!("Content-Length: {body_len}\r\n"));
        }
        if !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("connection"))
        {
            head.push_str("Connection: keep-alive\r\n");
        }
    }
    head.push_str("\r\n");
    head
}

pub(crate) fn default_request_chunking(method: &str) -> bool {
    !matches!(
        method.to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "DELETE" | "OPTIONS" | "TRACE" | "CONNECT"
    )
}
