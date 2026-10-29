pub(crate) struct RequestMessage {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) version: String,
    pub(crate) raw_headers: Vec<(String, String)>,
    pub(crate) body: Result<Vec<u8>, RequestFramingError>,
    pub(crate) upgrade: bool,
    pub(crate) upgrade_head: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestFramingError {
    InvalidTransferCodingOrder,
    MissingFinalChunked,
}

pub(crate) struct ResponseMessage {
    pub(crate) status: u16,
    pub(crate) message: String,
    pub(crate) raw_headers: Vec<(String, String)>,
}

pub(crate) fn request(bytes: &[u8]) -> Option<RequestMessage> {
    let head = parse_head(bytes)?;
    let coding = request_transfer_coding(&head.raw_headers);
    let body = match coding {
        Ok(()) => Ok(framed_body(bytes, head.body_start, &head.raw_headers)?),
        Err(error) => Err(error),
    };
    let upgrade = has_upgrade(&head.raw_headers);
    let mut first = head.first.into_iter();
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);
    Some(RequestMessage {
        method,
        target,
        version: version.strip_prefix("HTTP/")?.to_owned(),
        raw_headers: head.raw_headers,
        body,
        upgrade,
        upgrade_head: if upgrade {
            bytes.get(head.body_start..)?.to_vec()
        } else {
            Vec::new()
        },
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
    pub(crate) head: Option<ResponseMessage>,
    pub(crate) body_chunks: Vec<Vec<u8>>,
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
            head: None,
            body_chunks: Vec::new(),
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
                    self.pending.drain(..head_size);
                    let status = head
                        .first
                        .get(1)
                        .and_then(|status| status.parse::<u16>().ok())
                        .ok_or_else(|| "HTTP response has an invalid status code".to_owned())?;
                    if (100..200).contains(&status) && status != 101 {
                        continue;
                    }
                    let chunked = has_chunked_encoding(&head.raw_headers);
                    let length = content_length(&head.raw_headers);
                    progress.head = Some(ResponseMessage {
                        status,
                        message: head.first.get(2..).unwrap_or_default().join(" "),
                        raw_headers: head.raw_headers,
                    });
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
}

fn parse_head(bytes: &[u8]) -> Option<Head> {
    let body_start = head_size(bytes)?;
    let boundary = body_start - 4;
    let text = String::from_utf8_lossy(&bytes[..boundary]);
    let mut lines = text.split("\r\n");
    let first = lines
        .next()?
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let raw_headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect::<Vec<_>>();
    Some(Head {
        first,
        raw_headers,
        body_start,
    })
}

fn framed_body(bytes: &[u8], body_start: usize, headers: &[(String, String)]) -> Option<Vec<u8>> {
    let body = bytes.get(body_start..)?;
    if has_chunked_encoding(headers) {
        return decode_chunked(body);
    }
    if let Some(length) = content_length(headers).ok().flatten() {
        return (body.len() >= length).then(|| body[..length].to_vec());
    }
    Some(Vec::new())
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

fn decode_chunked(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut cursor = 0;
    let mut body = Vec::new();
    loop {
        let line_end = bytes
            .get(cursor..)?
            .windows(2)
            .position(|window| window == b"\r\n")?;
        let line = std::str::from_utf8(bytes.get(cursor..cursor + line_end)?).ok()?;
        let length = usize::from_str_radix(line.split(';').next()?.trim(), 16).ok()?;
        cursor += line_end + 2;
        if length == 0 {
            if bytes.get(cursor..)?.starts_with(b"\r\n") {
                return Some(body);
            }
            let _trailers_end = bytes
                .get(cursor..)?
                .windows(4)
                .position(|window| window == b"\r\n\r\n")?;
            return Some(body);
        }
        let end = cursor.checked_add(length)?;
        body.extend_from_slice(bytes.get(cursor..end)?);
        if bytes.get(end..end + 2)? != b"\r\n" {
            return None;
        }
        cursor = end + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::{ResponseParser, request};

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
