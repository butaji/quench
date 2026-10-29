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
    // An ASCII value's bytes are its UTF-16 code units.
    if value.is_ascii() {
        return value.bytes().all(|byte| valid_header_unit(u16::from(byte), false));
    }
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
    http10: bool,
    send_date: bool,
    now: std::time::SystemTime,
) -> Vec<u8> {
    use std::io::Write as _;
    let header_bytes: usize = headers.iter().map(|(key, value)| key.len() + value.len() + 4).sum();
    let mut out = Vec::with_capacity(RESPONSE_HEAD_RESERVE + header_bytes + body.len());
    // Writes into a Vec cannot fail.
    let _ = write!(out, "HTTP/1.1 {status} ");
    out.extend_from_slice(text);
    out.extend_from_slice(b"\r\n");
    let chunked = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    });
    if !http10
        && !chunked
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    {
        let _ = write!(out, "Content-Length: {}\r\n", body.len());
    }
    if send_date
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("date"))
    {
        out.extend_from_slice(b"Date: ");
        append_http_date(&mut out, now);
        out.extend_from_slice(b"\r\n");
    }
    let connection = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("connection"))
        .map(|(_, value)| value.as_str())
        .unwrap_or(if keep_alive { "keep-alive" } else { "close" });
    let header_line = |out: &mut Vec<u8>, key: &str, value: &str| {
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    };
    for (key, value) in headers {
        if key.eq_ignore_ascii_case("transfer-encoding") || key.eq_ignore_ascii_case("connection") {
            continue;
        }
        header_line(&mut out, key, value);
    }
    header_line(&mut out, "Connection", connection);
    for (key, value) in headers {
        if key.eq_ignore_ascii_case("transfer-encoding") {
            header_line(&mut out, key, value);
        }
    }
    out.extend_from_slice(b"\r\n");
    if chunked {
        out.extend_from_slice(&chunk_frame(body));
    } else {
        out.extend_from_slice(body);
    }
    out
}

/// Bytes a response head takes beyond its headers: the status line, the framing and date
/// headers and the blank line.
const RESPONSE_HEAD_RESERVE: usize = 128;

thread_local! {
    /// The IMF-fixdate of the last second a response head named, as Node's `utcDate`
    /// caches it: responses within one second share the formatted text.
    static HTTP_DATE: std::cell::RefCell<(u64, String)> =
        const { std::cell::RefCell::new((u64::MAX, String::new())) };
}

/// Appends `now` as an IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`).
fn append_http_date(out: &mut Vec<u8>, now: std::time::SystemTime) {
    let second = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    HTTP_DATE.with(|cached| {
        let mut cached = cached.borrow_mut();
        if cached.0 != second {
            let date: chrono::DateTime<chrono::Utc> = now.into();
            *cached = (second, date.format("%a, %d %b %Y %H:%M:%S GMT").to_string());
        }
        out.extend_from_slice(cached.1.as_bytes());
    });
}

pub(crate) fn chunk_frame(body: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    append_chunk_frame(&mut frame, body);
    frame
}

/// The size line that opens a chunked-coding frame of `length` bytes: hexadecimal digits
/// and CRLF, the CRLF that closes the frame being `CHUNK_FRAME_END`.
pub(crate) struct ChunkFrameHead {
    bytes: [u8; CHUNK_FRAME_HEAD_CAPACITY],
    length: usize,
}

/// A `usize` in hexadecimal, two digits per byte, then CRLF.
const CHUNK_FRAME_HEAD_CAPACITY: usize = 2 * std::mem::size_of::<usize>() + CHUNK_FRAME_END.len();
pub(crate) const CHUNK_FRAME_END: &[u8] = b"\r\n";

impl ChunkFrameHead {
    pub(crate) fn new(length: usize) -> Self {
        use std::io::Write;
        let mut bytes = [0; CHUNK_FRAME_HEAD_CAPACITY];
        let mut cursor = &mut bytes[..];
        write!(cursor, "{length:x}\r\n").expect("a usize's hexadecimal size line fits");
        let length = CHUNK_FRAME_HEAD_CAPACITY - cursor.len();
        Self { bytes, length }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

/// Append one chunked-coding frame; an empty body has none, as `0` ends the message.
pub(crate) fn append_chunk_frame(out: &mut Vec<u8>, body: &[u8]) {
    if body.is_empty() {
        return;
    }
    out.extend_from_slice(ChunkFrameHead::new(body.len()).bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(CHUNK_FRAME_END);
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
    } else if !has_content_length && default_request_chunking(method) {
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

pub(crate) fn default_request_chunking(method: &str) -> bool {
    !matches!(
        method.to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "DELETE" | "OPTIONS" | "TRACE" | "CONNECT"
    )
}
