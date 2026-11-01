pub(crate) struct RequestMessage {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) version: String,
    pub(crate) raw_headers: Vec<(String, String)>,
    pub(crate) head_size: usize,
    pub(crate) raw_trailers: Vec<(String, String)>,
    pub(crate) body: Result<Vec<u8>, RequestFramingError>,
    pub(crate) upgrade: bool,
    pub(crate) upgrade_head: Vec<u8>,
    pub(crate) parse_error: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestFramingError {
    InvalidTransferCodingOrder,
    MissingFinalChunked,
    InvalidChunkedBody,
}

#[derive(Clone, Debug)]
pub(crate) struct ResponseMessage {
    pub(crate) status: u16,
    pub(crate) message: String,
    pub(crate) version: String,
    pub(crate) raw_headers: Vec<(String, String)>,
}

/// Whether a response leaves its connection reusable: HTTP/1.1 persists unless
/// the response says `Connection: close`; HTTP/1.0 persists only on an explicit
/// `Connection: keep-alive`.
pub(crate) fn response_keep_alive(version: &str, headers: &[(String, String)]) -> bool {
    let connection = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim);
    let mut keep_alive = !version.starts_with("1.0");
    for token in connection {
        if token.eq_ignore_ascii_case("close") {
            return false;
        }
        if token.eq_ignore_ascii_case("keep-alive") {
            keep_alive = true;
        }
    }
    keep_alive
}

pub(crate) fn request(bytes: &[u8]) -> Option<RequestMessage> {
    const HTTP2_CLIENT_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
    // Node's HTTP/1 parser pauses when it recognizes the HTTP/2 connection
    // preface so an HTTP server can report HPE_PAUSED_H2_UPGRADE instead of
    // dispatching "PRI * HTTP/2.0" as an ordinary request.
    if bytes.len() < HTTP2_CLIENT_PREFACE.len() && HTTP2_CLIENT_PREFACE.starts_with(bytes) {
        return None;
    }
    if bytes.starts_with(HTTP2_CLIENT_PREFACE) {
        return Some(RequestMessage {
            method: "PRI".to_owned(),
            target: "*".to_owned(),
            version: "2.0".to_owned(),
            raw_headers: Vec::new(),
            head_size: 0,
            raw_trailers: Vec::new(),
            body: Ok(Vec::new()),
            upgrade: false,
            upgrade_head: Vec::new(),
            parse_error: Some("HPE_PAUSED_H2_UPGRADE"),
        });
    }
    let head = parse_head(bytes)?;
    let parse_error = head.request_error;
    let coding = request_transfer_coding(&head.raw_headers);
    let (body, raw_trailers) = match (parse_error, coding) {
        (Some(_), _) => (Ok(Vec::new()), Vec::new()),
        (_, Ok(())) => match framed_body(bytes, head.body_start, &head.raw_headers) {
            Ok(Some((body, trailers))) => (Ok(body), trailers),
            Ok(None) => return None,
            Err(error) => (Err(error), Vec::new()),
        },
        (_, Err(error)) => (Err(error), Vec::new()),
    };
    let upgrade = parse_error.is_none() && has_upgrade(&head.raw_headers);
    // A head that failed validation still reports its parts, so the server can name
    // the parse error (`parse_error`) for whatever request line it saw.
    let mut first = head.first.into_iter();
    let method = first.next().unwrap_or_default();
    let target = first.next().unwrap_or_default();
    let version = first
        .next()
        .and_then(|version| version.strip_prefix("HTTP/").map(str::to_owned))
        .unwrap_or_default();
    Some(RequestMessage {
        method,
        target,
        version,
        raw_headers: head.raw_headers,
        head_size: head.body_start,
        raw_trailers,
        body,
        upgrade,
        upgrade_head: if upgrade {
            bytes.get(head.body_start..)?.to_vec()
        } else {
            Vec::new()
        },
        parse_error,
    })
}

fn has_upgrade(headers: &[(String, String)]) -> bool {
    let upgrade = header(headers, "upgrade").is_some_and(|value| !value.trim().is_empty());
    let connection = header(headers, "connection")
        .is_some_and(|value| value.split(',').any(|token| token.trim().eq_ignore_ascii_case("upgrade")));
    upgrade && connection
}

/// The value of header `name` (lowercase) among a head's raw headers, as Node's
/// `IncomingMessage.headers` combines repeats: in order, joined by `", "`, or by `"; "` for
/// `cookie`.
pub(crate) fn header<'h>(headers: &'h [(String, String)], name: &str) -> Option<std::borrow::Cow<'h, str>> {
    let mut values = headers.iter().filter(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str());
    let first = values.next()?;
    let Some(second) = values.next() else {
        return Some(std::borrow::Cow::Borrowed(first));
    };
    let separator = if name == "cookie" { "; " } else { ", " };
    let mut combined = [first, second].join(separator);
    for value in values {
        combined.push_str(separator);
        combined.push_str(value);
    }
    Some(std::borrow::Cow::Owned(combined))
}

pub(crate) fn request_wire_size(bytes: &[u8]) -> usize {
    let Some(head) = parse_head(bytes) else {
        return bytes.len();
    };
    if let Some(length) = content_length(&head.raw_headers).ok().flatten() {
        return head.body_start.saturating_add(length).min(bytes.len());
    }
    if has_chunked_encoding(&head.raw_headers) {
        let mut cursor = head.body_start;
        loop {
            let Some(line_end) = bytes.get(cursor..).and_then(line_end) else {
                return bytes.len();
            };
            let line = &bytes[cursor..cursor + line_end];
            let size = line
                .split(|byte| *byte == b';')
                .next()
                .and_then(|size| std::str::from_utf8(size).ok())
                .and_then(|size| usize::from_str_radix(size.trim(), 16).ok());
            let Some(size) = size else {
                return bytes.len();
            };
            cursor = cursor.saturating_add(line_end + 2);
            if size == 0 {
                if bytes.get(cursor..).is_some_and(|tail| tail.starts_with(b"\r\n")) {
                    return cursor.saturating_add(2).min(bytes.len());
                }
                let Some(trailers_end) = bytes.get(cursor..).and_then(head_size) else {
                    return bytes.len();
                };
                return cursor.saturating_add(trailers_end).min(bytes.len());
            }
            cursor = cursor.saturating_add(size + 2);
            if cursor > bytes.len() {
                return bytes.len();
            }
        }
    }
    head.body_start
}

#[derive(Default)]
pub(crate) struct ResponseParser {
    pending: Vec<u8>,
    state: ResponseState,
    head_request: bool,
}

#[derive(Clone, Copy, Default)]
enum ResponseState {
    #[default]
    Head,
    FixedLength(usize),
    ChunkSize,
    ChunkData(usize),
    ChunkTerminator,
    Trailers,
    UntilEof,
    Complete,
}

#[derive(Debug)]
pub(crate) struct ResponseProgress {
    pub(crate) information: Vec<ResponseMessage>,
    pub(crate) head: Option<ResponseMessage>,
    pub(crate) body_chunks: Vec<Vec<u8>>,
    pub(crate) raw_trailers: Vec<(String, String)>,
    pub(crate) complete: bool,
    pub(crate) upgrade_head: Option<Vec<u8>>,
}

impl ResponseParser {
    pub(crate) fn for_method(method: &str) -> Self {
        Self {
            head_request: method == "HEAD",
            ..Self::default()
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        matches!(self.state, ResponseState::Complete)
    }

    pub(crate) fn pending_head_len(&self) -> usize {
        matches!(self.state, ResponseState::Head)
            .then_some(self.pending.len())
            .unwrap_or(0)
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<ResponseProgress, String> {
        self.pending.extend_from_slice(bytes);
        self.advance(false)
    }

    pub(crate) fn finish(&mut self) -> Result<ResponseProgress, String> {
        self.advance(true)
    }

    fn advance(&mut self, eof: bool) -> Result<ResponseProgress, String> {
        let mut progress = ResponseProgress {
            information: Vec::new(),
            head: None,
            body_chunks: Vec::new(),
            raw_trailers: Vec::new(),
            complete: false,
            upgrade_head: None,
        };
        loop {
            match self.state {
                ResponseState::Head => {
                    let Some(head_size) = head_size(&self.pending) else {
                        if eof {
                            return Err("HTTP response ended before its head completed".into());
                        }
                        break;
                    };
                    let head = parse_head(&self.pending[..head_size])
                        .ok_or_else(|| "HTTP response head is malformed".to_owned())?;
                    if let Some(code) = validate_response_head(&head) {
                        return Err(code.to_owned());
                    }
                    self.pending.drain(..head_size);
                    let status = head
                        .first
                        .get(1)
                        .and_then(|status| status.parse::<u16>().ok())
                        .ok_or_else(|| "HTTP response has an invalid status code".to_owned())?;
                    let message = ResponseMessage {
                        status,
                        message: head.first.get(2..).unwrap_or_default().join(" "),
                        version: head
                            .first
                            .first()
                            .and_then(|line| line.strip_prefix("HTTP/"))
                            .unwrap_or("1.1")
                            .to_owned(),
                        raw_headers: head.raw_headers,
                    };
                    if (100..200).contains(&status) && status != 101 {
                        progress.information.push(message);
                        continue;
                    }
                    let chunked = has_chunked_encoding(&message.raw_headers);
                    let length = content_length(&message.raw_headers);
                    progress.head = Some(message);
                    if status == 101 {
                        progress.upgrade_head = Some(std::mem::take(&mut self.pending));
                        self.state = ResponseState::Complete;
                    } else {
                    self.state = if self.head_request || response_has_no_body(status) {
                        ResponseState::Complete
                    } else if chunked {
                        ResponseState::ChunkSize
                    } else if let Some(length) = length? {
                        if length == 0 {
                            ResponseState::Complete
                        } else {
                            ResponseState::FixedLength(length)
                        }
                    } else {
                        ResponseState::UntilEof
                    };
                    }
                }
                ResponseState::FixedLength(remaining) => {
                    let count = remaining.min(self.pending.len());
                    if count == 0 {
                        if eof {
                            return Err(
                                "HTTP response ended before Content-Length bytes arrived".into()
                            );
                        }
                        break;
                    }
                    progress
                        .body_chunks
                        .push(self.pending.drain(..count).collect());
                    self.state = if count == remaining {
                        ResponseState::Complete
                    } else {
                        ResponseState::FixedLength(remaining - count)
                    };
                }
                ResponseState::ChunkSize => {
                    let Some(line_end) = line_end(&self.pending) else {
                        if eof {
                            return Err("HTTP response ended before chunk size completed".into());
                        }
                        break;
                    };
                    let line = std::str::from_utf8(&self.pending[..line_end])
                        .map_err(|_| "HTTP response has an invalid chunk size".to_owned())?;
                    let size =
                        usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
                            .map_err(|_| "HTTP response has an invalid chunk size".to_owned())?;
                    self.pending.drain(..line_end + 2);
                    self.state = if size == 0 {
                        ResponseState::Trailers
                    } else {
                        ResponseState::ChunkData(size)
                    };
                }
                ResponseState::ChunkData(remaining) => {
                    let count = remaining.min(self.pending.len());
                    if count == 0 {
                        if eof {
                            return Err("HTTP response ended before chunk data completed".into());
                        }
                        break;
                    }
                    progress
                        .body_chunks
                        .push(self.pending.drain(..count).collect());
                    self.state = if count == remaining {
                        ResponseState::ChunkTerminator
                    } else {
                        ResponseState::ChunkData(remaining - count)
                    };
                }
                ResponseState::ChunkTerminator => {
                    if self.pending.len() < 2 {
                        if eof {
                            return Err("HTTP response ended before chunk terminator".into());
                        }
                        break;
                    }
                    if !self.pending.starts_with(b"\r\n") {
                        return Err("HTTP response has an invalid chunk terminator".into());
                    }
                    self.pending.drain(..2);
                    self.state = ResponseState::ChunkSize;
                }
                ResponseState::Trailers => {
                    if self.pending.starts_with(b"\r\n") {
                        self.pending.drain(..2);
                        self.state = ResponseState::Complete;
                    } else if let Some(end) = self
                        .pending
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                    {
                        progress.raw_trailers = parse_trailer_lines(&self.pending[..end + 2])?;
                        self.pending.drain(..end + 4);
                        self.state = ResponseState::Complete;
                    } else {
                        if eof {
                            return Err("HTTP response ended before trailers completed".into());
                        }
                        break;
                    }
                }
                ResponseState::UntilEof => {
                    if !self.pending.is_empty() {
                        progress.body_chunks.push(std::mem::take(&mut self.pending));
                    }
                    if eof {
                        self.state = ResponseState::Complete;
                    }
                    break;
                }
                ResponseState::Complete => break,
            }
        }
        progress.complete = matches!(self.state, ResponseState::Complete);
        Ok(progress)
    }
}

fn parse_trailer_lines(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut trailers = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let remaining = &bytes[start..];
        let Some(end) = remaining.windows(2).position(|window| window == b"\r\n") else {
            return Err("HTTP response has a malformed trailer line".into());
        };
        if end == 0 {
            break;
        }
        let line = &remaining[..end];
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            return Err("HTTP response has a malformed trailer line".into());
        };
        let name = line[..colon]
            .iter()
            .copied()
            .map(char::from)
            .collect::<String>();
        let value = line[colon + 1..]
            .iter()
            .copied()
            .map(char::from)
            .collect::<String>();
        trailers.push((
            name,
            value
                .trim_start_matches(|character| character == ' ' || character == '\t')
                .to_owned(),
        ));
        start += end + 2;
    }
    Ok(trailers)
}

pub(crate) fn head_size(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|boundary| boundary + 4)
}

fn line_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|window| window == b"\r\n")
}

struct Head {
    first: Vec<String>,
    raw_headers: Vec<(String, String)>,
    body_start: usize,
    request_error: Option<&'static str>,
}

fn parse_head(bytes: &[u8]) -> Option<Head> {
    let body_start = head_size(bytes)?;
    let boundary = body_start - 4;
    let text = String::from_utf8_lossy(&bytes[..boundary]);
    let mut lines = text.split("\r\n");
    let first_line = lines.next()?;
    let first = first_line.split_whitespace().map(str::to_owned).collect();
    let lines = lines.collect::<Vec<_>>();
    let request_error = validate_request_head(first_line, &lines);
    let raw_headers = lines
        .iter()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_owned(), value.trim().to_owned()))
        })
        .collect::<Vec<_>>();
    Some(Head {
        first,
        raw_headers,
        body_start,
        request_error,
    })
}

fn validate_response_head(head: &Head) -> Option<&'static str> {
    let first_line = head.first.join(" ");
    if !first_line.starts_with("HTTP/")
        || head.first.get(1).is_none_or(|status| {
            status.len() != 3 || !status.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Some("HPE_INVALID_CONSTANT");
    }
    for (name, value) in &head.raw_headers {
        if name.as_bytes().contains(&b'\r') || value.as_bytes().contains(&b'\r') {
            return Some("HPE_LF_EXPECTED");
        }
        if name.is_empty()
            || !name
                .chars()
                .all(crate::modules::http_protocol::is_http_token_char)
        {
            return Some("HPE_INVALID_HEADER_TOKEN");
        }
    }
    let has_content_length = head
        .raw_headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let has_transfer_encoding = head
        .raw_headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"));
    if has_content_length && has_transfer_encoding {
        return Some("HPE_INVALID_TRANSFER_ENCODING");
    }
    None
}

#[cfg(test)]
mod response_parser_tests {
    use super::ResponseParser;

    #[test]
    fn bare_carriage_return_in_response_header_reports_lf_expected() {
        let mut parser = ResponseParser::default();
        let error = parser
            .push(b"HTTP/1.1 200 OK\r\nFoo: Bar\rContent-Length: 1\r\n\r\n")
            .unwrap_err();
        assert_eq!(error, "HPE_LF_EXPECTED");
    }

    #[test]
    fn conflicting_response_content_length_and_transfer_encoding_is_rejected() {
        let mut parser = ResponseParser::default();
        let error = parser
            .push(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n")
            .unwrap_err();
        assert_eq!(error, "HPE_INVALID_TRANSFER_ENCODING");
    }
}

#[cfg(test)]
mod request_parser_tests {
    use super::request;

    const HTTP2_CLIENT_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

    #[test]
    fn waits_for_the_complete_http2_preface_before_pausing_http1_parsing() {
        assert!(request(&HTTP2_CLIENT_PREFACE[..18]).is_none());
    }

    #[test]
    fn recognizes_http2_preface_as_paused_upgrade() {
        // A following SETTINGS frame is commonly present in the same read.
        let mut bytes = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
        bytes.extend_from_slice(b"\0\0\0\x04\0\0\0\0\0");
        let message = request(&bytes).expect("complete HTTP/2 preface");
        assert_eq!(message.parse_error, Some("HPE_PAUSED_H2_UPGRADE"));
        assert_eq!(message.method, "PRI");
    }
}

fn validate_request_head(first_line: &str, lines: &[&str]) -> Option<&'static str> {
    let mut request_line = first_line.split_ascii_whitespace();
    let method = request_line.next()?;
    let target = request_line.next()?;
    let version = request_line.next()?;
    if request_line.next().is_some()
        || method.is_empty()
        || !method
            .chars()
            .all(crate::modules::http_protocol::is_http_token_char)
        || target.is_empty()
        || !version.starts_with("HTTP/")
        || !version[5..].contains('.')
    {
        return Some("HPE_INVALID_CONSTANT");
    }

    for line in lines {
        if line.as_bytes().contains(&b'\r') || line.as_bytes().contains(&b'\n') {
            return Some("HPE_LF_EXPECTED");
        }
        let Some((name, _)) = line.split_once(':') else {
            return Some("HPE_INVALID_HEADER_TOKEN");
        };
        if name.is_empty()
            || !name
                .chars()
                .all(crate::modules::http_protocol::is_http_token_char)
        {
            return Some("HPE_INVALID_HEADER_TOKEN");
        }
    }

    let mut content_lengths = 0;
    let mut has_transfer_encoding = false;
    for line in lines {
        if let Some((name, _)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_lengths += 1;
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                has_transfer_encoding = true;
            }
        }
    }
    if content_lengths > 1 {
        return Some("HPE_UNEXPECTED_CONTENT_LENGTH");
    }
    if content_lengths > 0 && has_transfer_encoding {
        return Some("HPE_INVALID_TRANSFER_ENCODING");
    }
    None
}

fn framed_body(
    bytes: &[u8],
    body_start: usize,
    headers: &[(String, String)],
) -> Result<Option<(Vec<u8>, Vec<(String, String)>)>, RequestFramingError> {
    let Some(body) = bytes.get(body_start..) else {
        return Ok(None);
    };
    if has_chunked_encoding(headers) {
        return match decode_chunked(body) {
            ChunkedBody::Complete(body, trailers) => Ok(Some((body, trailers))),
            ChunkedBody::Incomplete => Ok(None),
            ChunkedBody::Invalid => Err(RequestFramingError::InvalidChunkedBody),
        };
    }
    if let Some(length) = content_length(headers).ok().flatten() {
        return Ok((body.len() >= length).then(|| (body[..length].to_vec(), Vec::new())));
    }
    Ok(Some((Vec::new(), Vec::new())))
}

fn response_has_no_body(status: u16) -> bool {
    (100..200).contains(&status) || matches!(status, 204 | 304)
}

fn request_transfer_coding(headers: &[(String, String)]) -> Result<(), RequestFramingError> {
    let Some(value) = header(headers, "transfer-encoding") else {
        return Ok(());
    };
    let mut tokens = value.split(',').map(str::trim).peekable();
    let mut chunked = false;
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("chunked") {
            if chunked || tokens.peek().is_some() {
                return Err(RequestFramingError::InvalidTransferCodingOrder);
            }
            chunked = true;
        }
    }
    if chunked {
        Ok(())
    } else {
        Err(RequestFramingError::MissingFinalChunked)
    }
}

fn has_chunked_encoding(headers: &[(String, String)]) -> bool {
    header(headers, "transfer-encoding")
        .is_some_and(|value| value.split(',').any(|token| token.trim().eq_ignore_ascii_case("chunked")))
}

fn content_length(headers: &[(String, String)]) -> Result<Option<usize>, String> {
    let Some(value) = header(headers, "content-length") else {
        return Ok(None);
    };
    value
        .parse::<usize>()
        .map(Some)
        .map_err(|_| "HTTP response has an invalid Content-Length".to_owned())
}

enum ChunkedBody {
    Complete(Vec<u8>, Vec<(String, String)>),
    Incomplete,
    Invalid,
}

fn decode_chunked(bytes: &[u8]) -> ChunkedBody {
    let mut cursor = 0;
    let mut body = Vec::new();
    loop {
        let Some(remaining) = bytes.get(cursor..) else {
            return ChunkedBody::Incomplete;
        };
        let Some(line_end) = remaining.windows(2).position(|window| window == b"\r\n") else {
            return if remaining.ends_with(b"\r") {
                ChunkedBody::Incomplete
            } else if remaining.contains(&b'\r') || remaining.contains(&b'\n') {
                ChunkedBody::Invalid
            } else {
                ChunkedBody::Incomplete
            };
        };
        let Some(line_bytes) = remaining.get(..line_end) else {
            return ChunkedBody::Invalid;
        };
        if line_bytes.contains(&b'\r') || line_bytes.contains(&b'\n') {
            return ChunkedBody::Invalid;
        }
        let Ok(line) = std::str::from_utf8(line_bytes) else {
            return ChunkedBody::Invalid;
        };
        let Some(size) = line.split(';').next() else {
            return ChunkedBody::Invalid;
        };
        if size.is_empty() || !size.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return ChunkedBody::Invalid;
        }
        let Ok(length) = usize::from_str_radix(size, 16) else {
            return ChunkedBody::Invalid;
        };
        cursor += line_end + 2;
        if length == 0 {
            let Some(remaining) = bytes.get(cursor..) else {
                return ChunkedBody::Incomplete;
            };
            if remaining.starts_with(b"\r\n") {
                return ChunkedBody::Complete(body, Vec::new());
            }
            let Some(trailers_end) = remaining
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            else {
                return ChunkedBody::Incomplete;
            };
            let trailer_bytes = &remaining[..trailers_end + 2];
            let Ok(trailer_text) = std::str::from_utf8(trailer_bytes) else {
                return ChunkedBody::Invalid;
            };
            let mut trailers = Vec::new();
            for line in trailer_text.split("\r\n").filter(|line| !line.is_empty()) {
                let Some((name, value)) = line.split_once(':') else {
                    return ChunkedBody::Invalid;
                };
                trailers.push((
                    name.to_owned(),
                    value
                        .trim_start_matches(|character| character == ' ' || character == '\t')
                        .to_owned(),
                ));
            }
            return ChunkedBody::Complete(body, trailers);
        }
        let Some(end) = cursor.checked_add(length) else {
            return ChunkedBody::Invalid;
        };
        let Some(chunk) = bytes.get(cursor..end) else {
            return ChunkedBody::Incomplete;
        };
        body.extend_from_slice(chunk);
        let Some(terminator) = bytes.get(end..) else {
            return ChunkedBody::Incomplete;
        };
        if !terminator.starts_with(b"\r\n") {
            return if terminator.len() < 2 {
                ChunkedBody::Incomplete
            } else {
                ChunkedBody::Invalid
            };
        }
        cursor = end + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::{request, response_keep_alive, ResponseParser};

    #[test]
    fn http_1_1_persists_unless_the_response_closes() {
        assert!(response_keep_alive("1.1", &[]));
        assert!(response_keep_alive(
            "1.1",
            &[("Connection".to_owned(), "keep-alive".to_owned())]
        ));
        assert!(!response_keep_alive(
            "1.1",
            &[("connection".to_owned(), "close".to_owned())]
        ));
        // A comma-separated list keeps the last decisive token.
        assert!(!response_keep_alive(
            "1.1",
            &[("Connection".to_owned(), "keep-alive, close".to_owned())]
        ));
    }

    #[test]
    fn http_1_0_persists_only_on_an_explicit_keep_alive() {
        assert!(!response_keep_alive("1.0", &[]));
        assert!(response_keep_alive(
            "1.0",
            &[("Connection".to_owned(), "keep-alive".to_owned())]
        ));
        assert!(!response_keep_alive(
            "1.0",
            &[("Connection".to_owned(), "close".to_owned())]
        ));
    }

    #[test]
    fn upgrade_request_preserves_bytes_after_the_head() {
        let bytes = b"GET / HTTP/1.1\r\nConnection: keep-alive, Upgrade\r\nUpgrade: websocket\r\n\r\n\x81\x02hi";
        let request = request(bytes).unwrap();
        assert!(request.upgrade);
        assert_eq!(request.method, "GET");
        assert_eq!(request.upgrade_head, b"\x81\x02hi");
    }

    #[test]
    fn ordinary_request_is_not_classified_as_upgrade() {
        let request = request(b"GET / HTTP/1.1\r\nConnection: keep-alive\r\n\r\n").unwrap();
        assert!(!request.upgrade);
        assert!(request.upgrade_head.is_empty());
    }

    #[test]
    fn switching_protocols_preserves_raw_bytes_after_the_head() {
        let mut parser = ResponseParser::default();
        let progress = parser
            .push(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n\x81\x02hi")
            .unwrap();
        assert_eq!(progress.head.unwrap().status, 101);
        assert!(progress.complete);
        assert!(progress.body_chunks.is_empty());
        assert_eq!(progress.upgrade_head.unwrap(), b"\x81\x02hi");
    }
}
