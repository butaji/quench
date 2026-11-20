//! Pure HTTP/1 protocol facts: methods, tokens, header bytes, status reasons and dates.

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

/// The fields an incoming message keeps only the first of (`IncomingMessage._addHeaderLine`).
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
        byte && !b"\0\r\n"
            .iter()
            .any(|value| unit == u16::from(*value))
    } else {
        unit == u16::from(b'\t')
            || (u16::from(b' ')..=u16::from(b'~')).contains(&unit)
            || (OBS_TEXT_START..=u16::from(u8::MAX)).contains(&unit)
    }
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

thread_local! {
    /// The IMF-fixdate of the last second a response head named, as Node's `utcDate`
    /// caches it: responses within one second share the formatted text.
    static HTTP_DATE: std::cell::RefCell<(u64, String)> =
        const { std::cell::RefCell::new((u64::MAX, String::new())) };
}

/// The current time as an IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`), Node's `utcDate()`.
pub(crate) fn utc_date() -> String {
    let now = std::time::SystemTime::now();
    let second = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    HTTP_DATE.with(|cached| {
        let mut cached = cached.borrow_mut();
        if cached.0 != second {
            let date: chrono::DateTime<chrono::Utc> = now.into();
            *cached = (second, date.format("%a, %d %b %Y %H:%M:%S GMT").to_string());
        }
        cached.1.clone()
    })
}

