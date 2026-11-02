//! The `HTTPParser` a connection reads with: a JavaScript object carrying Node's parser
//! fields (`socket`, `incoming`, `outgoing`, `maxHeaderPairs`, `joinDuplicateHeaders`) whose
//! framing state is the host parser its id names. `execute` feeds it a chunk and runs the
//! parser callbacks Node's `_http_common.js` defines.

use super::parser::{Event, Head, HeadOutcome, Kind, ParseError, Parser};
use super::{class, get_slot, put_slot, Class, Slot};
use crate::modules::stream_shared_vm::{call_method, get, is_nullish, put, put_bool, put_number, Context, Outcome};
use quench_runtime::{RootId, RootedError};

/// Node's default `parser.maxHeaderPairs`.
const DEFAULT_MAX_HEADER_PAIRS: f64 = 2000.0;
/// The CLI switch that makes every parser lenient unless its owner chooses.
pub(crate) const INSECURE_PARSER_FLAG: &str = "--insecure-http-parser";

/// How a chunk's execution ended.
pub(crate) enum Executed {
    Consumed,
    /// A parse error, as Node's parser reports it.
    Failed(RootId),
    /// The message upgraded: the bytes after its head belong to the new protocol.
    Upgraded(Vec<u8>),
}

/// `parsers.alloc()` then `parser.initialize(kind, …, maxHeaderSize, lenient)`: `owner` (the
/// request or the server) chooses the size limit and `insecureHTTPParser`, whose absence
/// defers to `--insecure-http-parser`.
pub(crate) fn create(c: &mut Context<'_>, kind: Kind, socket: RootId, owner: RootId) -> Outcome {
    let max_header_size = get(c, owner, "maxHeaderSize")?;
    let max_header_size = if is_nullish(c, max_header_size) { 0.0 } else { c.to_number(max_header_size)? };
    let limit = if max_header_size > 0.0 { max_header_size } else { super::max_header_size(c) };
    let insecure = get(c, owner, "insecureHTTPParser")?;
    let lenient = match c.rooted_value(insecure).and_then(|value| value.as_bool()) {
        Some(lenient) => lenient,
        None => c.host_mut().shared_state().borrow().cli_options.enabled(INSECURE_PARSER_FLAG),
    };
    let id = c.host_mut().shared_state().borrow_mut().http.add_parser(Parser::new(kind, limit as usize, lenient));
    let parser = c.object_rooted()?;
    let id = c.number(id as f64);
    put_slot(c, parser, Slot::ParserId, id)?;
    put(c, parser, "socket", socket)?;
    let null = c.null();
    put(c, parser, "incoming", null)?;
    put(c, parser, "outgoing", null)?;
    put_number(c, parser, "maxHeaderPairs", DEFAULT_MAX_HEADER_PAIRS)?;
    let undefined = c.undefined();
    put(c, parser, "joinDuplicateHeaders", undefined)?;
    Ok(parser)
}

fn parser_id(c: &mut Context<'_>, parser: RootId) -> Result<Option<u64>, RootedError> {
    if is_nullish(c, parser) {
        return Ok(None);
    }
    let id = get_slot(c, parser, Slot::ParserId)?;
    Ok(c.rooted_value(id).and_then(|value| value.as_number()).map(|id| id as u64))
}

fn with_parser<T>(c: &mut Context<'_>, id: u64, operation: impl FnOnce(&mut Parser) -> T) -> Option<T> {
    c.host_mut().shared_state().borrow_mut().http.parsers.get_mut(&id).map(operation)
}

/// Whether the parser is inside a message: a connection that is not idle.
pub(crate) fn in_message(c: &mut Context<'_>, parser: RootId) -> Result<bool, RootedError> {
    let Some(id) = parser_id(c, parser)? else {
        return Ok(false);
    };
    Ok(with_parser(c, id, |parser| parser.in_message()).unwrap_or(false))
}

/// Whether the current message has outlived its server's head or request timeout.
pub(crate) fn expired(c: &mut Context<'_>, parser: RootId, headers_timeout: f64, request_timeout: f64) -> Result<bool, RootedError> {
    let Some(id) = parser_id(c, parser)? else {
        return Ok(false);
    };
    Ok(with_parser(c, id, |parser| {
        let Some(started) = parser.started else {
            return false;
        };
        let elapsed = started.elapsed().as_secs_f64() * MILLISECONDS_PER_SECOND;
        (headers_timeout > 0.0 && parser.in_head() && elapsed > headers_timeout)
            || (request_timeout > 0.0 && elapsed > request_timeout)
    })
    .unwrap_or(false))
}

const MILLISECONDS_PER_SECOND: f64 = 1000.0;

/// `parser.execute(data)` with Node's callbacks.
pub(crate) fn execute(c: &mut Context<'_>, parser: RootId, data: RootId) -> Result<Executed, RootedError> {
    let Some(id) = parser_id(c, parser)? else {
        return Ok(Executed::Consumed);
    };
    let bytes = c.view_bytes_rooted(data).unwrap_or_default();
    with_parser(c, id, |parser| {
        if !parser.in_message() && !bytes.is_empty() {
            parser.started = Some(std::time::Instant::now());
        }
        parser.push(&bytes);
    });
    loop {
        let Some(event) = with_parser(c, id, Parser::next) else {
            return Ok(Executed::Consumed);
        };
        match event {
            Err(failure) => return Ok(Executed::Failed(parse_error(c, &failure)?)),
            Ok(None) => return Ok(Executed::Consumed),
            Ok(Some(Event::Head(head))) => {
                let kind = with_parser(c, id, |parser| parser.kind()).unwrap_or(Kind::Request);
                let outcome = headers_complete(c, parser, kind, head)?;
                with_parser(c, id, |parser| parser.resolve_head(outcome));
                if outcome == HeadOutcome::Upgrade {
                    let rest = with_parser(c, id, Parser::take_remaining).unwrap_or_default();
                    return Ok(Executed::Upgraded(rest));
                }
                if with_parser(c, id, |parser| parser.at_message_start()).unwrap_or(false) {
                    message_complete(c, id, parser, Vec::new())?;
                }
            }
            Ok(Some(Event::Body(body))) => {
                let incoming = get(c, parser, "incoming")?;
                if !is_nullish(c, incoming) && !crate::modules::stream_shared_vm::truthy_member(c, incoming, "_dumped")? {
                    let chunk = crate::modules::buffer_shared_vm::from_vec(c, body)?;
                    let accepted = call_method(c, incoming, "push", &[chunk])?;
                    if !c.truthy_rooted(accepted)? {
                        let socket = get(c, parser, "socket")?;
                        if !is_nullish(c, socket) {
                            call_method(c, socket, "pause", &[])?;
                        }
                    }
                }
            }
            Ok(Some(Event::Complete(trailers))) => message_complete(c, id, parser, trailers)?,
        }
    }
}

/// `parser.finish()`: the end of input, with the error an unfinished message makes.
pub(crate) fn finish(c: &mut Context<'_>, parser: RootId) -> Result<Option<RootId>, RootedError> {
    let Some(id) = parser_id(c, parser)? else {
        return Ok(None);
    };
    match with_parser(c, id, Parser::finish) {
        Some(Ok(Some(Event::Complete(trailers)))) => {
            message_complete(c, id, parser, trailers)?;
            Ok(None)
        }
        Some(Err(failure)) => parse_error(c, &failure).map(Some),
        _ => Ok(None),
    }
}

/// The error Node's parser binding makes of a failure.
fn parse_error(c: &mut Context<'_>, failure: &ParseError) -> Outcome {
    let error = c.error_rooted(&format!("Parse Error: {}", failure.reason))?;
    put_number(c, error, "bytesParsed", failure.bytes_parsed as f64)?;
    let code = c.string_rooted(failure.code);
    put(c, error, "code", code)?;
    let reason = c.string_rooted(failure.reason);
    put(c, error, "reason", reason)?;
    Ok(error)
}

/// `freeParser(parser, req, socket)`: the host parser goes; the object forgets its peers.
pub(crate) fn free(c: &mut Context<'_>, parser: RootId, request: Option<RootId>, socket: RootId) -> Result<(), RootedError> {
    if let Some(id) = parser_id(c, parser)? {
        c.host_mut().shared_state().borrow_mut().http.parsers.remove(&id);
        let undefined = c.undefined();
        put_slot(c, parser, Slot::ParserId, undefined)?;
        let null = c.null();
        for name in ["incoming", "outgoing", "socket"] {
            put(c, parser, name, null)?;
        }
    }
    let null = c.null();
    if let Some(request) = request {
        put(c, request, "parser", null)?;
    }
    if !is_nullish(c, socket) {
        put(c, socket, "parser", null)?;
    }
    Ok(())
}

/// Tell a response parser its request was `HEAD`: the response has no body.
pub(crate) fn set_head_request(c: &mut Context<'_>, parser: RootId, head_request: bool) -> Result<(), RootedError> {
    if let Some(id) = parser_id(c, parser)? {
        with_parser(c, id, |parser| parser.set_head_request(head_request));
    }
    Ok(())
}

/// `parserOnHeadersComplete`: the message object, then the side's `onIncoming`.
fn headers_complete(c: &mut Context<'_>, parser: RootId, kind: Kind, head: Head) -> Result<HeadOutcome, RootedError> {
    let socket = get(c, parser, "socket")?;
    let server = if is_nullish(c, socket) { c.undefined() } else { get(c, socket, "server")? };
    let custom = if is_nullish(c, server) { c.undefined() } else { get_slot(c, server, Slot::IncomingMessageClass)? };
    let constructor = if c.truthy_rooted(custom)? { custom } else { class(c, Class::IncomingMessage)? };
    let incoming = c.construct_rooted(constructor, constructor, &[socket])?;
    put(c, parser, "incoming", incoming)?;
    put_number(c, incoming, "httpVersionMajor", f64::from(head.major))?;
    put_number(c, incoming, "httpVersionMinor", f64::from(head.minor))?;
    let version = c.string_rooted(&format!("{}.{}", head.major, head.minor));
    put(c, incoming, "httpVersion", version)?;
    let server_join = if is_nullish(c, server) { c.undefined() } else { get(c, server, "joinDuplicateHeaders")? };
    let join = if c.truthy_rooted(server_join)? { server_join } else { get(c, parser, "joinDuplicateHeaders")? };
    put(c, incoming, "joinDuplicateHeaders", join)?;
    let url = c.string_rooted(&head.url);
    put(c, incoming, "url", url)?;
    put_bool(c, incoming, "upgrade", head.upgrade)?;
    let mut lines = Vec::with_capacity(head.headers.len() * 2);
    for (name, value) in &head.headers {
        lines.push(c.string_rooted(name));
        lines.push(c.string_rooted(value));
    }
    let maximum = get(c, parser, "maxHeaderPairs")?;
    let maximum = c.rooted_value(maximum).and_then(|value| value.as_number()).unwrap_or(0.0);
    let count = if maximum > 0.0 { (lines.len() as f64).min(maximum) } else { lines.len() as f64 };
    let lines = c.array_rooted(&lines)?;
    let count = c.number(count);
    call_method(c, incoming, "_addHeaderLines", &[lines, count])?;
    match kind {
        Kind::Request => {
            let method = c.string_rooted(&head.method);
            put(c, incoming, "method", method)?;
            super::server::on_incoming(c, socket, incoming, head.keep_alive)
        }
        Kind::Response => {
            put_number(c, incoming, "statusCode", f64::from(head.status))?;
            let reason = c.string_rooted(&head.reason);
            put(c, incoming, "statusMessage", reason)?;
            super::client::on_incoming(c, socket, incoming, head.keep_alive)
        }
    }
}

/// `parserOnMessageComplete`: the message completes with its trailers and ends.
fn message_complete(c: &mut Context<'_>, id: u64, parser: RootId, trailers: Vec<(String, String)>) -> Result<(), RootedError> {
    with_parser(c, id, |parser| parser.started = None);
    let incoming = get(c, parser, "incoming")?;
    if !is_nullish(c, incoming) {
        put_bool(c, incoming, "complete", true)?;
        if !trailers.is_empty() {
            let mut lines = Vec::with_capacity(trailers.len() * 2);
            for (name, value) in &trailers {
                lines.push(c.string_rooted(name));
                lines.push(c.string_rooted(value));
            }
            let count = c.number(lines.len() as f64);
            let lines = c.array_rooted(&lines)?;
            call_method(c, incoming, "_addHeaderLines", &[lines, count])?;
        }
        let null = c.null();
        call_method(c, incoming, "push", &[null])?;
    }
    let socket = get(c, parser, "socket")?;
    super::incoming::read_start(c, socket)
}
