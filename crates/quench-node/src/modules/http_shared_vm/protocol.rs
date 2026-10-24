pub(crate) struct RequestMessage {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) version: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) raw_headers: Vec<(String, String)>,
    pub(crate) head_size: usize,
    pub(crate) raw_trailers: Vec<(String, String)>,
    pub(crate) body: Result<Vec<u8>, RequestFramingError>,
    pub(crate) parse_error: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestFramingError {
    InvalidTransferCodingOrder,
    MissingFinalChunked,
    InvalidChunkedBody,
}

#[derive(Clone)]
pub(crate) struct ResponseMessage {
    pub(crate) status: u16,
    pub(crate) message: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) raw_headers: Vec<(String, String)>,
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
            headers: Vec::new(),
            raw_headers: Vec::new(),
            head_size: 0,
            raw_trailers: Vec::new(),
            body: Ok(Vec::new()),
            parse_error: Some("HPE_PAUSED_H2_UPGRADE"),
        });
    }
    let head = parse_head(bytes)?;
    let parse_error = head.request_error;
    let coding = request_transfer_coding(&head.headers);
    let (body, raw_trailers) = match (parse_error, coding) {
        (Some(_), _) => (Ok(Vec::new()), Vec::new()),
        (_, Ok(())) => match framed_body(bytes, head.body_start, &head.headers) {
            Ok(Some((body, trailers))) => (Ok(body), trailers),
            Ok(None) => return None,
            Err(error) => (Err(error), Vec::new()),
        },
        (_, Err(error)) => (Err(error), Vec::new()),
    };
    let method = head.first.first().cloned().unwrap_or_default();
    let target = head.first.get(1).cloned().unwrap_or_default();
    let version = head
        .first
        .get(2)
        .and_then(|version| version.strip_prefix("HTTP/"))
        .unwrap_or_default()
        .to_owned();
    Some(RequestMessage {
        method,
        target,
        version,
        headers: head.headers,
        raw_headers: head.raw_headers,
        head_size: head.body_start,
        raw_trailers,
        body,
        parse_error,
    })
}

pub(crate) fn request_wire_size(bytes: &[u8]) -> usize {
    let Some(head) = parse_head(bytes) else {
        return bytes.len();
    };
    if let Some(length) = content_length(&head.headers).ok().flatten() {
        return head.body_start.saturating_add(length).min(bytes.len());
    }
    if has_chunked_encoding(&head.headers) {
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

pub(crate) struct ResponseProgress {
    pub(crate) information: Vec<ResponseMessage>,
    pub(crate) head: Option<ResponseMessage>,
    pub(crate) body_chunks: Vec<Vec<u8>>,
    pub(crate) raw_trailers: Vec<(String, String)>,
    pub(crate) complete: bool,
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
                    if (100..200).contains(&status) && status != 101 {
                        progress.information.push(ResponseMessage {
                            status,
                            message: head.first.get(2..).unwrap_or_default().join(" "),
                            headers: head.headers,
                            raw_headers: head.raw_headers,
                        });
                        continue;
                    }
                    progress.head = Some(ResponseMessage {
                        status,
                        message: head.first.get(2..).unwrap_or_default().join(" "),
                        headers: head.headers.clone(),
                        raw_headers: head.raw_headers,
                    });
                    self.state = if self.head_request || response_has_no_body(status) {
                        ResponseState::Complete
                    } else if has_chunked_encoding(&head.headers) {
                        ResponseState::ChunkSize
                    } else if let Some(length) = content_length(&head.headers)? {
                        if length == 0 {
                            ResponseState::Complete
                        } else {
                            ResponseState::FixedLength(length)
                        }
                    } else {
                        ResponseState::UntilEof
                    };
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
    headers: Vec<(String, String)>,
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
    let mut headers = Vec::<(String, String)>::new();
    for (name, value) in &raw_headers {
        let name = name.to_ascii_lowercase();
        if let Some((_, current)) = headers.iter_mut().find(|(key, _)| *key == name) {
            // Node drops repeated values for fields that HTTP defines as
            // single-valued (notably Host), while preserving every original
            // pair in rawHeaders. Other fields use their field-specific join.
            if crate::modules::http_protocol::NON_REPEATABLE_HEADERS.contains(&name.as_str()) {
                continue;
            }
            current.push_str(if name == "cookie" { "; " } else { ", " });
            current.push_str(value);
        } else {
            headers.push((name, value.clone()));
        }
    }
    Some(Head {
        first,
        headers,
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
    let Some((_, value)) = headers.iter().find(|(name, _)| name == "transfer-encoding") else {
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
    headers.iter().any(|(name, value)| {
        name == "transfer-encoding"
            && value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("chunked"))
    })
}

fn content_length(headers: &[(String, String)]) -> Result<Option<usize>, String> {
    let Some((_, value)) = headers.iter().find(|(name, _)| name == "content-length") else {
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
