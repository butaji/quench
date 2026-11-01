//! An incremental HTTP/1 message parser with llhttp's strict-mode rules: the request and
//! response heads, Content-Length and chunked framing, keep-alive and upgrade detection,
//! and llhttp's error codes and reasons. Bytes go in with `push`; `next` yields one event
//! at a time, so the caller can decide how a message's body is read once its head is known.

use crate::modules::http_protocol::{is_http_token_char, HTTP_METHODS};

/// Which side of an exchange the parser reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Request,
    Response,
}

/// A parsed message head, as llhttp's `on_headers_complete` reports it.
pub(crate) struct Head {
    pub(crate) major: u8,
    pub(crate) minor: u8,
    /// The request line's method and target (requests only).
    pub(crate) method: String,
    pub(crate) url: String,
    /// The status line's code and reason phrase (responses only).
    pub(crate) status: u16,
    pub(crate) reason: String,
    /// The header lines as `[name, value]` pairs, in wire order.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) upgrade: bool,
    pub(crate) keep_alive: bool,
}

pub(crate) enum Event {
    Head(Head),
    Body(Vec<u8>),
    /// The message ended, with its trailer lines.
    Complete(Vec<(String, String)>),
}

/// How the caller reads the body of the head just parsed (llhttp's
/// `on_headers_complete` result).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeadOutcome {
    Body,
    SkipBody,
    Upgrade,
}

/// A parse failure: llhttp's error name and reason, and the bytes of the current input
/// consumed before it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub(crate) code: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) bytes_parsed: usize,
}

/// llhttp's failures, as (error name, reason).
mod failure {
    pub(super) type Failure = (&'static str, &'static str);
    pub(super) const INVALID_METHOD: Failure = ("HPE_INVALID_METHOD", "Invalid method encountered");
    pub(super) const INVALID_CONSTANT: Failure = ("HPE_INVALID_CONSTANT", "Expected HTTP/");
    pub(super) const INVALID_VERSION: Failure = ("HPE_INVALID_VERSION", "Invalid HTTP version");
    pub(super) const INVALID_STATUS: Failure = ("HPE_INVALID_STATUS", "Invalid status code");
    pub(super) const INVALID_URL: Failure = ("HPE_INVALID_URL", "Invalid char in url path");
    pub(super) const INVALID_HEADER_TOKEN: Failure = ("HPE_INVALID_HEADER_TOKEN", "Invalid header token");
    pub(super) const INVALID_HEADER_VALUE: Failure = ("HPE_INVALID_HEADER_TOKEN", "Invalid header value char");
    pub(super) const LF_EXPECTED: Failure = ("HPE_LF_EXPECTED", "Missing expected LF after header value");
    pub(super) const HEADER_OVERFLOW: Failure = ("HPE_HEADER_OVERFLOW", "Header overflow");
    pub(super) const INVALID_CONTENT_LENGTH: Failure =
        ("HPE_INVALID_CONTENT_LENGTH", "Invalid character in Content-Length");
    pub(super) const DUPLICATE_CONTENT_LENGTH: Failure = ("HPE_UNEXPECTED_CONTENT_LENGTH", "Duplicate Content-Length");
    pub(super) const TRANSFER_ENCODING_WITH_LENGTH: Failure =
        ("HPE_INVALID_TRANSFER_ENCODING", "Transfer-Encoding can't be present with Content-Length");
    pub(super) const INVALID_TRANSFER_ENCODING: Failure =
        ("HPE_INVALID_TRANSFER_ENCODING", "Request has invalid `Transfer-Encoding`");
    pub(super) const INVALID_CHUNK_SIZE: Failure = ("HPE_INVALID_CHUNK_SIZE", "Invalid character in chunk size");
    pub(super) const CHUNK_DATA_END: Failure = ("HPE_STRICT", "Expected LF after chunk data");
    pub(super) const INVALID_EOF: Failure = ("HPE_INVALID_EOF_STATE", "Invalid EOF state");
}
use failure::Failure;

const CRLF: &[u8] = b"\r\n";
const HEAD_END: &[u8] = b"\r\n\r\n";
const HTTP_PREFIX: &str = "HTTP/";
/// A status code's digit count.
const STATUS_DIGITS: usize = 3;
/// The radix of a chunk-size line.
const CHUNK_SIZE_RADIX: u32 = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Head,
    /// A head is out; its body mode waits for `resolve_head`.
    AwaitingOutcome(Framing),
    Fixed(usize),
    /// A body's last byte is out; the message's completion is next.
    BodyEnd,
    ChunkSize,
    ChunkData(usize),
    ChunkDataEnd,
    Trailers,
    UntilEof,
    Upgraded,
    Failed,
}

/// How a message's body is delimited, from its head.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Framing {
    None,
    Fixed(usize),
    Chunked,
    UntilEof,
}

pub(crate) struct Parser {
    kind: Kind,
    state: State,
    buffer: Vec<u8>,
    /// The unconsumed bytes of earlier inputs at the start of the current one.
    carried: usize,
    /// The largest head, in bytes.
    max_header_size: usize,
    /// The method of the request a response answers: a `HEAD` response has no body.
    head_request: bool,
    /// When the current message's first byte arrived (server timeouts).
    pub(crate) started: Option<std::time::Instant>,
}

impl Parser {
    pub(crate) fn new(kind: Kind, max_header_size: usize) -> Self {
        Self {
            kind,
            state: State::Head,
            buffer: Vec::new(),
            carried: 0,
            max_header_size,
            head_request: false,
            started: None,
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) fn set_head_request(&mut self, head_request: bool) {
        self.head_request = head_request;
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.carried = self.buffer.len();
        self.buffer.extend_from_slice(bytes);
    }

    /// Whether a message has started and not yet ended.
    pub(crate) fn in_message(&self) -> bool {
        !matches!(self.state, State::Head | State::Upgraded | State::Failed) || !self.buffer.is_empty()
    }

    /// Whether the current message's head is still being read.
    pub(crate) fn in_head(&self) -> bool {
        self.state == State::Head && !self.buffer.is_empty()
    }

    /// The bytes after an upgraded message's head.
    pub(crate) fn take_remaining(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }

    fn fail(&mut self, (code, reason): Failure, position: usize) -> ParseError {
        self.state = State::Failed;
        ParseError { code, reason, bytes_parsed: position.saturating_sub(self.carried) }
    }

    fn consume(&mut self, count: usize) {
        self.buffer.drain(..count);
        self.carried = self.carried.saturating_sub(count);
    }

    /// Settle how the body of the head `next` just yielded is read.
    pub(crate) fn resolve_head(&mut self, outcome: HeadOutcome) {
        let State::AwaitingOutcome(framing) = self.state else {
            return;
        };
        self.state = match (outcome, framing) {
            (HeadOutcome::Upgrade, _) => State::Upgraded,
            (HeadOutcome::SkipBody, _) | (_, Framing::None) => State::Head,
            (_, Framing::Fixed(0)) => State::Head,
            (_, Framing::Fixed(length)) => State::Fixed(length),
            (_, Framing::Chunked) => State::ChunkSize,
            (_, Framing::UntilEof) => State::UntilEof,
        };
    }

    /// Whether the parser waits for a message head: after `resolve_head`, the message just
    /// headed has no body to read and its end is due.
    pub(crate) fn at_message_start(&self) -> bool {
        self.state == State::Head
    }

    pub(crate) fn next(&mut self) -> Result<Option<Event>, ParseError> {
        match self.state {
            State::Head => self.head(),
            State::AwaitingOutcome(_) | State::Upgraded | State::Failed => Ok(None),
            State::Fixed(remaining) => {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                let take = remaining.min(self.buffer.len());
                let body = self.buffer[..take].to_vec();
                self.consume(take);
                self.state = if take == remaining { State::BodyEnd } else { State::Fixed(remaining - take) };
                Ok(Some(Event::Body(body)))
            }
            State::BodyEnd => {
                self.state = State::Head;
                Ok(Some(Event::Complete(Vec::new())))
            }
            State::UntilEof => {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                Ok(Some(Event::Body(std::mem::take(&mut self.buffer))))
            }
            State::ChunkSize => self.chunk_size(),
            State::ChunkData(remaining) => {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                let take = remaining.min(self.buffer.len());
                let body = self.buffer[..take].to_vec();
                self.consume(take);
                self.state = if take == remaining { State::ChunkDataEnd } else { State::ChunkData(remaining - take) };
                Ok(Some(Event::Body(body)))
            }
            State::ChunkDataEnd => {
                if self.buffer.len() < CRLF.len() {
                    return Ok(None);
                }
                if !self.buffer.starts_with(CRLF) {
                    return Err(self.fail(failure::CHUNK_DATA_END, 0));
                }
                self.consume(CRLF.len());
                self.state = State::ChunkSize;
                self.next()
            }
            State::Trailers => self.trailers(),
        }
    }

    /// The end of input: a body read until EOF completes; any other unfinished message fails.
    pub(crate) fn finish(&mut self) -> Result<Option<Event>, ParseError> {
        match self.state {
            State::UntilEof => {
                self.state = State::Head;
                Ok(Some(Event::Complete(Vec::new())))
            }
            State::Head if self.buffer.is_empty() => Ok(None),
            State::Upgraded | State::Failed => Ok(None),
            _ => {
                let position = self.buffer.len();
                Err(self.fail(failure::INVALID_EOF, position))
            }
        }
    }

    fn head(&mut self) -> Result<Option<Event>, ParseError> {
        // A request may be preceded by empty lines (RFC 9112 2.2).
        if self.kind == Kind::Request {
            while self.buffer.starts_with(CRLF) {
                self.consume(CRLF.len());
            }
        }
        if self.buffer.is_empty() {
            return Ok(None);
        }
        if let Some(failure) = self.early_failure() {
            return Err(self.fail(failure, 1));
        }
        let Some(end) = find(&self.buffer, HEAD_END) else {
            if self.buffer.len() > self.max_header_size {
                let position = self.buffer.len();
                return Err(self.fail(failure::HEADER_OVERFLOW, position));
            }
            return Ok(None);
        };
        let head_length = end + HEAD_END.len();
        if head_length > self.max_header_size {
            return Err(self.fail(failure::HEADER_OVERFLOW, head_length));
        }
        let parsed = parse_head(self.kind, &self.buffer[..end]);
        let (head, framing) = match parsed.and_then(|head| self.framing(head)) {
            Ok(value) => value,
            Err((failure, position)) => return Err(self.fail(failure, position)),
        };
        self.consume(head_length);
        self.state = State::AwaitingOutcome(framing);
        Ok(Some(Event::Head(head)))
    }

    /// A failure the first bytes of a head already prove (llhttp fails as it reads them).
    fn early_failure(&self) -> Option<Failure> {
        let first_space = self.buffer.iter().position(|&byte| byte == b' ');
        let token_end = first_space.unwrap_or(self.buffer.len());
        let token = &self.buffer[..token_end];
        match self.kind {
            Kind::Request => {
                let known = HTTP_METHODS.iter().any(|method| {
                    let method = method.as_bytes();
                    if first_space.is_some() { method == token } else { method.starts_with(token) }
                });
                (!known).then_some(failure::INVALID_METHOD)
            }
            Kind::Response => {
                let prefix = HTTP_PREFIX.as_bytes();
                let compared = token.len().min(prefix.len());
                (token[..compared] != prefix[..compared]).then_some(failure::INVALID_CONSTANT)
            }
        }
    }

    /// The head's body framing and connection persistence, per llhttp.
    fn framing(&self, mut head: Head) -> Result<(Head, Framing), (Failure, usize)> {
        let mut content_length = None;
        let mut transfer_encoding: Option<String> = None;
        let mut connection_close = false;
        let mut connection_keep_alive = false;
        let mut connection_upgrade = false;
        let mut upgrade_header = false;
        for (name, value) in &head.headers {
            if name.eq_ignore_ascii_case("content-length") {
                if content_length.is_some() {
                    return Err((failure::DUPLICATE_CONTENT_LENGTH, 0));
                }
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err((failure::INVALID_CONTENT_LENGTH, 0));
                }
                content_length = Some(value.parse::<usize>().map_err(|_| (failure::INVALID_CONTENT_LENGTH, 0))?);
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                match &mut transfer_encoding {
                    Some(existing) => {
                        existing.push(',');
                        existing.push_str(value);
                    }
                    None => transfer_encoding = Some(value.clone()),
                }
            } else if name.eq_ignore_ascii_case("connection") {
                for token in value.split(',').map(str::trim) {
                    connection_close |= token.eq_ignore_ascii_case("close");
                    connection_keep_alive |= token.eq_ignore_ascii_case("keep-alive");
                    connection_upgrade |= token.eq_ignore_ascii_case("upgrade");
                }
            } else if name.eq_ignore_ascii_case("upgrade") {
                upgrade_header = true;
            }
        }
        let chunked = transfer_encoding.as_deref().is_some_and(|value| {
            value.rsplit(',').next().is_some_and(|last| last.trim().eq_ignore_ascii_case("chunked"))
        });
        if transfer_encoding.is_some() && content_length.is_some() {
            return Err((failure::TRANSFER_ENCODING_WITH_LENGTH, 0));
        }
        if self.kind == Kind::Request && transfer_encoding.is_some() && !chunked {
            return Err((failure::INVALID_TRANSFER_ENCODING, 0));
        }
        head.upgrade = match self.kind {
            Kind::Request => (connection_upgrade && upgrade_header) || head.method == "CONNECT",
            Kind::Response => head.status == SWITCHING_PROTOCOLS,
        };
        let no_body_response = self.kind == Kind::Response
            && (self.head_request || is_informational(head.status) || NO_BODY_STATUSES.contains(&head.status));
        let framing = if no_body_response {
            Framing::None
        } else if chunked {
            Framing::Chunked
        } else if let Some(length) = content_length {
            Framing::Fixed(length)
        } else if self.kind == Kind::Response {
            Framing::UntilEof
        } else {
            Framing::None
        };
        let persistent = if head.major > 0 && head.minor > 0 { !connection_close } else { connection_keep_alive };
        head.keep_alive = persistent && framing != Framing::UntilEof;
        Ok((head, framing))
    }

    fn chunk_size(&mut self) -> Result<Option<Event>, ParseError> {
        let Some(end) = find(&self.buffer, CRLF) else {
            if self.buffer.iter().any(|&byte| !is_chunk_size_line_byte(byte)) {
                return Err(self.fail(failure::INVALID_CHUNK_SIZE, 0));
            }
            return Ok(None);
        };
        let line = &self.buffer[..end];
        let digits = line.split(|&byte| byte == b';').next().unwrap_or_default();
        let size = std::str::from_utf8(digits)
            .ok()
            .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .and_then(|digits| usize::from_str_radix(digits, CHUNK_SIZE_RADIX).ok());
        let Some(size) = size else {
            return Err(self.fail(failure::INVALID_CHUNK_SIZE, end));
        };
        self.consume(end + CRLF.len());
        self.state = if size == 0 { State::Trailers } else { State::ChunkData(size) };
        self.next()
    }

    fn trailers(&mut self) -> Result<Option<Event>, ParseError> {
        if self.buffer.starts_with(CRLF) {
            self.consume(CRLF.len());
            self.state = State::Head;
            return Ok(Some(Event::Complete(Vec::new())));
        }
        let Some(end) = find(&self.buffer, HEAD_END) else {
            return Ok(None);
        };
        let trailers = match header_lines(&self.buffer[..end]) {
            Ok(trailers) => trailers,
            Err(failure) => return Err(self.fail(failure, end)),
        };
        self.consume(end + HEAD_END.len());
        self.state = State::Head;
        Ok(Some(Event::Complete(trailers)))
    }
}

/// The status that switches protocols: the response is an upgrade.
const SWITCHING_PROTOCOLS: u16 = 101;
/// The final statuses whose responses have no body.
const NO_BODY_STATUSES: [u16; 2] = [204, 304];
const INFORMATIONAL: std::ops::Range<u16> = 100..200;

pub(crate) fn is_informational(status: u16) -> bool {
    INFORMATIONAL.contains(&status)
}

fn is_chunk_size_line_byte(byte: u8) -> bool {
    byte != b'\n'
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// A head byte string as Node reads it: each byte one Latin-1 character.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

fn parse_head(kind: Kind, bytes: &[u8]) -> Result<Head, (Failure, usize)> {
    let line_end = find(bytes, CRLF).unwrap_or(bytes.len());
    let line = &bytes[..line_end];
    let mut head = Head {
        major: 0,
        minor: 0,
        method: String::new(),
        url: String::new(),
        status: 0,
        reason: String::new(),
        headers: Vec::new(),
        upgrade: false,
        keep_alive: false,
    };
    let mut parts = line.splitn(3, |&byte| byte == b' ');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().ok_or((failure::INVALID_CONSTANT, line_end))?;
    let third = parts.next();
    match kind {
        Kind::Request => {
            head.method = latin1(first);
            if second.is_empty() || second.iter().any(|&byte| byte <= b' ' || byte == 0x7f) {
                return Err((failure::INVALID_URL, first.len() + 1));
            }
            head.url = latin1(second);
            let version = third.ok_or((failure::INVALID_CONSTANT, line_end))?;
            (head.major, head.minor) = version_of(version)?;
        }
        Kind::Response => {
            (head.major, head.minor) = version_of(first)?;
            if second.len() != STATUS_DIGITS || !second.iter().all(u8::is_ascii_digit) {
                return Err((failure::INVALID_STATUS, first.len() + 1));
            }
            head.status = latin1(second).parse().map_err(|_| (failure::INVALID_STATUS, first.len() + 1))?;
            head.reason = latin1(third.unwrap_or_default());
        }
    }
    let rest = bytes.get(line_end + CRLF.len()..).unwrap_or_default();
    head.headers = header_lines(rest).map_err(|failure| (failure, line_end))?;
    Ok(head)
}

/// `HTTP/<digit>.<digit>`.
fn version_of(bytes: &[u8]) -> Result<(u8, u8), (Failure, usize)> {
    let Some(digits) = bytes.strip_prefix(HTTP_PREFIX.as_bytes()) else {
        return Err((failure::INVALID_CONSTANT, 0));
    };
    match digits {
        [major, b'.', minor] if major.is_ascii_digit() && minor.is_ascii_digit() => Ok((major - b'0', minor - b'0')),
        _ => Err((failure::INVALID_VERSION, 0)),
    }
}

/// Header lines `name: value`, each value without its surrounding whitespace.
fn header_lines(bytes: &[u8]) -> Result<Vec<(String, String)>, Failure> {
    let mut headers = Vec::new();
    if bytes.is_empty() {
        return Ok(headers);
    }
    for line in split_lines(bytes) {
        if line.contains(&b'\r') || line.contains(&b'\n') {
            return Err(failure::LF_EXPECTED);
        }
        let Some(colon) = line.iter().position(|&byte| byte == b':') else {
            return Err(failure::INVALID_HEADER_TOKEN);
        };
        let name = &line[..colon];
        if name.is_empty() || !name.iter().all(|&byte| is_http_token_char(char::from(byte))) {
            return Err(failure::INVALID_HEADER_TOKEN);
        }
        let value = trim_whitespace(&line[colon + 1..]);
        if value.iter().any(|&byte| byte != b'\t' && (byte < b' ' || byte == 0x7f)) {
            return Err(failure::INVALID_HEADER_VALUE);
        }
        headers.push((latin1(name), latin1(value)));
    }
    Ok(headers)
}

fn split_lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = Some(bytes);
    std::iter::from_fn(move || {
        let current = rest?;
        match find(current, CRLF) {
            Some(end) => {
                rest = Some(&current[end + CRLF.len()..]);
                Some(&current[..end])
            }
            None => {
                rest = None;
                Some(current)
            }
        }
    })
}

fn trim_whitespace(bytes: &[u8]) -> &[u8] {
    let is_space = |byte: &u8| *byte == b' ' || *byte == b'\t';
    let start = bytes.iter().position(|byte| !is_space(byte)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|byte| !is_space(byte)).map_or(start, |end| end + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 16 * 1024;

    fn events(parser: &mut Parser, outcome: HeadOutcome) -> Result<Vec<Event>, ParseError> {
        let mut events = Vec::new();
        while let Some(event) = parser.next()? {
            if matches!(event, Event::Head(_)) {
                parser.resolve_head(outcome);
            }
            events.push(event);
            if parser.at_message_start() && matches!(events.last(), Some(Event::Head(_))) {
                events.push(Event::Complete(Vec::new()));
            }
        }
        Ok(events)
    }

    #[test]
    fn request_with_fixed_body_arrives_in_pieces() {
        let mut parser = Parser::new(Kind::Request, LIMIT);
        parser.push(b"POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhe");
        let first = events(&mut parser, HeadOutcome::Body).unwrap();
        assert!(matches!(&first[0], Event::Head(head) if head.method == "POST" && head.url == "/a" && head.keep_alive));
        assert!(matches!(&first[1], Event::Body(body) if body == b"he"));
        parser.push(b"llo");
        let second = events(&mut parser, HeadOutcome::Body).unwrap();
        assert!(matches!(&second[0], Event::Body(body) if body == b"llo"));
        assert!(parser.at_message_start());
    }

    #[test]
    fn chunked_response_with_trailers() {
        let mut parser = Parser::new(Kind::Response, LIMIT);
        parser.push(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nX-T: 1\r\n\r\n");
        let events = events(&mut parser, HeadOutcome::Body).unwrap();
        assert!(matches!(&events[1], Event::Body(body) if body == b"abc"));
        assert!(matches!(&events[2], Event::Complete(trailers) if trailers[0].0 == "X-T"));
    }

    #[test]
    fn unknown_method_fails_early() {
        let mut parser = Parser::new(Kind::Request, LIMIT);
        parser.push(b"GARBAGE\r\n\r\n");
        let error = parser.next().err().unwrap();
        assert_eq!((error.code, error.bytes_parsed), ("HPE_INVALID_METHOD", 1));
    }

    #[test]
    fn length_with_transfer_encoding_fails() {
        let mut parser = Parser::new(Kind::Request, LIMIT);
        parser.push(b"GET / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert_eq!(parser.next().err().unwrap().code, "HPE_INVALID_TRANSFER_ENCODING");
    }

    #[test]
    fn response_without_length_reads_until_eof() {
        let mut parser = Parser::new(Kind::Response, LIMIT);
        parser.push(b"HTTP/1.0 200 OK\r\n\r\nbody");
        let events = events(&mut parser, HeadOutcome::Body).unwrap();
        assert!(matches!(&events[0], Event::Head(head) if !head.keep_alive));
        assert!(matches!(parser.finish().unwrap(), Some(Event::Complete(_))));
    }

    #[test]
    fn upgrade_keeps_the_bytes_after_the_head() {
        let mut parser = Parser::new(Kind::Request, LIMIT);
        parser.push(b"GET / HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: ws\r\n\r\nrest");
        let Ok(Some(Event::Head(head))) = parser.next() else { panic!("head") };
        assert!(head.upgrade);
        parser.resolve_head(HeadOutcome::Upgrade);
        assert_eq!(parser.take_remaining(), b"rest");
    }

    #[test]
    fn oversized_head_overflows() {
        let mut parser = Parser::new(Kind::Request, 32);
        parser.push(b"GET / HTTP/1.1\r\nX: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(parser.next().err().unwrap().code, "HPE_HEADER_OVERFLOW");
    }
}
