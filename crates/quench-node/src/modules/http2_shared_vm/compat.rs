//! The HTTP/1-style compatibility API (lib/internal/http2/compat.js): `Http2ServerRequest`,
//! a Readable fed by its stream's data, and `Http2ServerResponse`, a Stream writing its
//! stream with headers gathered until the head is sent.

use super::faults::Fault;
use super::headers;
use super::{class, closure, get_slot, put_slot, with_state, Class, Slot};
use crate::modules::host_members::{accessors, captured, constructor, current, members};
use crate::modules::stream_shared_vm::{
    call_method, emit, get, is_nullish, is_undefined, next_tick, on, once, put, put_bool, put_number, truthy_member, Context,
    Outcome,
};
use quench_runtime::{RootId, RootedError};

/// The HTTP version a compatibility request reports.
const HTTP_VERSION_MAJOR: f64 = 2.0;
const HTTP_VERSION_MINOR: f64 = 0.0;
const HTTP_VERSION: &str = "2.0";
const STATUS_OK: f64 = 200.0;
const STATUS_CONTINUE: f64 = 100.0;
const STATUS_EARLY_HINTS: f64 = 103.0;
const STATUS_SWITCHING_PROTOCOLS: f64 = 101.0;
const STATUS_EXPECTATION_FAILED: f64 = 417.0;
const STATUS_METHOD_NOT_ALLOWED: f64 = 405.0;
const INFORMATIONAL: std::ops::Range<f64> = 100.0..200.0;
const STATUS_RANGE: std::ops::RangeInclusive<f64> = 100.0..=599.0;
/// The pseudo-headers the compatibility API refuses to set.
const PSEUDO_HEADERS: [&str; 5] = [headers::STATUS, headers::METHOD, headers::PATH, headers::AUTHORITY, headers::SCHEME];
const CONNECTION: &str = "connection";
const TRAILERS: &str = "trailers";
const EXPECT_CONTINUE: &str = "100-continue";
const STATUS_MESSAGE_WARNING: &str = "Status message is not supported by HTTP/2 (RFC7540 8.1.2.4)";
const CONNECTION_HEADER_WARNING: &str =
    "The provided connection header is not valid, the value will be dropped from the header and will never be in use.";
const UNSUPPORTED_WARNING: &str = "UnsupportedWarning";

members!(Member, "http2Compat", [
    RequestConstruct,
    RequestAborted,
    RequestComplete,
    RequestStream,
    RequestHeaders,
    RequestRawHeaders,
    RequestTrailers,
    RequestRawTrailers,
    HttpVersionMajor,
    HttpVersionMinor,
    HttpVersion,
    RequestSocket,
    RequestRead,
    Method,
    SetMethod,
    Authority,
    Scheme,
    Url,
    SetUrl,
    RequestSetTimeout,
    ResponseConstruct,
    Header,
    WritableEnded,
    ResponseSocket,
    ResponseStream,
    HeadersSent,
    SendDate,
    SetSendDate,
    StatusCode,
    SetStatusCode,
    WritableCorked,
    WritableHighWaterMark,
    WritableFinished,
    WritableLength,
    WritableObjectMode,
    WritableNeedDrain,
    StatusMessage,
    SetStatusMessage,
    SetTrailer,
    AddTrailers,
    GetHeader,
    GetHeaderNames,
    GetHeaders,
    HasHeader,
    RemoveHeader,
    SetHeader,
    AppendHeader,
    FlushHeaders,
    WriteHead,
    Cork,
    Uncork,
    Write,
    End,
    Destroy,
    ResponseSetTimeout,
    CreatePushResponse,
    WriteContinue,
    WriteInformation,
    WriteEarlyHints,
    /// The stream listeners of a request and of a response.
    StreamData,
    StreamTrailers,
    StreamEnd,
    StreamError,
    StreamAbortedRequest,
    StreamCloseRequest,
    StreamTimeout,
    RequestPause,
    RequestResume,
    ResumeStream,
    StreamDrain,
    StreamCloseResponse,
    StreamTrailersReady,
    /// `createPushResponse`'s `pushStream` callback.
    PushResponse,
    /// The traps of `req.socket` and `res.socket`.
    SocketHas,
    SocketGet,
    SocketSet,
    SocketPrototype,
]);

pub(super) fn build(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let stream_module = crate::modules::stream_shared_vm::module(c)?;
    let readable = get(c, stream_module, "Readable")?;
    let request = constructor(c, Member::RequestConstruct, "Http2ServerRequest")?;
    crate::modules::http_shared_vm::inherit(c, request, readable)?;
    let prototype = get(c, request, "prototype")?;
    accessors(c, prototype, &[
        ("aborted", Member::RequestAborted, None),
        ("complete", Member::RequestComplete, None),
        ("stream", Member::RequestStream, None),
        ("headers", Member::RequestHeaders, None),
        ("rawHeaders", Member::RequestRawHeaders, None),
        ("trailers", Member::RequestTrailers, None),
        ("rawTrailers", Member::RequestRawTrailers, None),
        ("httpVersionMajor", Member::HttpVersionMajor, None),
        ("httpVersionMinor", Member::HttpVersionMinor, None),
        ("httpVersion", Member::HttpVersion, None),
        ("socket", Member::RequestSocket, None),
        ("connection", Member::RequestSocket, None),
        ("method", Member::Method, Some(Member::SetMethod)),
        ("authority", Member::Authority, None),
        ("scheme", Member::Scheme, None),
        ("url", Member::Url, Some(Member::SetUrl)),
    ])?;
    super::class_methods(c, prototype, &[("_read", Member::RequestRead), ("setTimeout", Member::RequestSetTimeout)])?;
    super::set_class(c, Class::Http2ServerRequest, request)?;

    let base = get(c, stream_module, "Stream")?;
    let response = constructor(c, Member::ResponseConstruct, "Http2ServerResponse")?;
    crate::modules::http_shared_vm::inherit(c, response, base)?;
    let prototype = get(c, response, "prototype")?;
    accessors(c, prototype, &[
        ("_header", Member::Header, None),
        ("writableEnded", Member::WritableEnded, None),
        ("finished", Member::WritableEnded, None),
        ("socket", Member::ResponseSocket, None),
        ("connection", Member::ResponseSocket, None),
        ("stream", Member::ResponseStream, None),
        ("headersSent", Member::HeadersSent, None),
        ("sendDate", Member::SendDate, Some(Member::SetSendDate)),
        ("statusCode", Member::StatusCode, Some(Member::SetStatusCode)),
        ("writableCorked", Member::WritableCorked, None),
        ("writableHighWaterMark", Member::WritableHighWaterMark, None),
        ("writableFinished", Member::WritableFinished, None),
        ("writableLength", Member::WritableLength, None),
        ("writableObjectMode", Member::WritableObjectMode, None),
        ("writableNeedDrain", Member::WritableNeedDrain, None),
        ("statusMessage", Member::StatusMessage, Some(Member::SetStatusMessage)),
    ])?;
    super::class_methods(c, prototype, &[
        ("setTrailer", Member::SetTrailer),
        ("addTrailers", Member::AddTrailers),
        ("getHeader", Member::GetHeader),
        ("getHeaderNames", Member::GetHeaderNames),
        ("getHeaders", Member::GetHeaders),
        ("hasHeader", Member::HasHeader),
        ("removeHeader", Member::RemoveHeader),
        ("setHeader", Member::SetHeader),
        ("appendHeader", Member::AppendHeader),
        ("flushHeaders", Member::FlushHeaders),
        ("writeHead", Member::WriteHead),
        ("cork", Member::Cork),
        ("uncork", Member::Uncork),
        ("write", Member::Write),
        ("end", Member::End),
        ("destroy", Member::Destroy),
        ("setTimeout", Member::ResponseSetTimeout),
        ("createPushResponse", Member::CreatePushResponse),
        ("writeInformation", Member::WriteInformation),
        ("writeContinue", Member::WriteContinue),
        ("writeEarlyHints", Member::WriteEarlyHints),
    ])?;
    super::set_class(c, Class::Http2ServerResponse, response)?;
    let handler = super::proxy_handler(c, &[
        ("has", Member::SocketHas),
        ("get", Member::SocketGet),
        ("set", Member::SocketSet),
        ("getPrototypeOf", Member::SocketPrototype),
    ])?;
    super::set_class(c, Class::StreamSocketHandler, handler)?;
    Ok((request, response))
}

fn state(c: &mut Context<'_>, object: RootId) -> Outcome {
    get_slot(c, object, Slot::CompatState)
}

fn state_flag(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let state = state(c, object)?;
    truthy_member(c, state, name)
}

fn set_state(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let state = state(c, object)?;
    put(c, state, name, value)
}

fn stream(c: &mut Context<'_>, object: RootId) -> Outcome {
    get_slot(c, object, Slot::Stream)
}

/// `req.socket` and `res.socket`: a `Proxy` of the stream, reaching its session's socket
/// for what the stream does not answer itself (compat.js's `proxySocketHandler`).
fn socket_of(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let cached = get_slot(c, stream, Slot::ProxySocket)?;
    if c.is_object_rooted(cached)? {
        return Ok(cached);
    }
    let handler = class(c, Class::StreamSocketHandler)?;
    let proxy = super::proxy(c, stream, handler)?;
    put_slot(c, stream, Slot::ProxySocket, proxy)?;
    Ok(proxy)
}

/// What the proxy reaches for the members the stream does not own: the session's socket
/// while the stream has a session, else the stream.
fn socket_target(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let session = get(c, stream, "session")?;
    if !c.is_object_rooted(session)? {
        return Ok(stream);
    }
    get_slot(c, session, Slot::Socket)
}

/// The socket members the stream answers itself, bound to it.
const STREAM_BOUND: [&str; 5] = ["on", "once", "end", "emit", "destroy"];
/// The socket members read from and written to the stream as they are.
const STREAM_FIELDS: [&str; 2] = ["writable", "destroyed"];
/// The socket members the compatibility socket refuses.
const SOCKET_MANIPULATION: [&str; 4] = ["write", "read", "pause", "resume"];

fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<String, RootedError> {
    match c.string_text(value)? {
        Some(text) => Ok(text),
        None => Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "string")?),
    }
}

/// A process-wide one-time warning (`statusMessageWarned`, `statusConnectionHeaderWarned`).
fn warn_once(c: &mut Context<'_>, message: &str, select: fn(&mut super::State) -> &mut bool) -> Result<(), RootedError> {
    let first = with_state(c, |state| !std::mem::replace(select(state), true));
    if first {
        super::emit_warning(c, message, Some(UNSUPPORTED_WARNING))?;
    }
    Ok(())
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::RequestConstruct => {
            construct_request(c, this, at(0), at(1), at(2), at(3))?;
            Ok(undefined)
        }
        Member::RequestAborted => get_slot(c, this, Slot::Aborted),
        Member::RequestComplete => {
            let stream = stream(c, this)?;
            let complete = super::slot_truthy(c, this, Slot::Aborted)?
                || truthy_member(c, this, "readableEnded")?
                || state_flag(c, this, "closed")?
                || truthy_member(c, stream, "destroyed")?;
            Ok(c.boolean(complete))
        }
        Member::RequestStream | Member::ResponseStream => stream(c, this),
        Member::RequestHeaders => get_slot(c, this, Slot::Headers),
        Member::RequestRawHeaders => get_slot(c, this, Slot::RawHeaders),
        Member::RequestTrailers => get_slot(c, this, Slot::Trailers),
        Member::RequestRawTrailers => get_slot(c, this, Slot::RawTrailers),
        Member::HttpVersionMajor => Ok(c.number(HTTP_VERSION_MAJOR)),
        Member::HttpVersionMinor => Ok(c.number(HTTP_VERSION_MINOR)),
        Member::HttpVersion => Ok(c.string_rooted(HTTP_VERSION)),
        Member::RequestSocket => {
            let stream = stream(c, this)?;
            socket_of(c, stream)
        }
        Member::RequestRead => {
            let stream = stream(c, this)?;
            if !state_flag(c, this, "didRead")? {
                let yes = c.boolean(true);
                set_state(c, this, "didRead", yes)?;
                let listener = closure(c, Member::StreamData, &[])?;
                on(c, stream, "data", listener)?;
            } else {
                let resume = closure(c, Member::ResumeStream, &[stream])?;
                next_tick(c, resume, &[])?;
            }
            Ok(undefined)
        }
        Member::Method => {
            let headers = get_slot(c, this, Slot::Headers)?;
            get(c, headers, headers::METHOD)
        }
        Member::SetMethod => {
            let method = validate_string(c, at(0), "method")?;
            if method.trim().is_empty() {
                return Err(crate::modules::util_shared_vm::argument_value_error(c, at(0), "method", "is invalid")?);
            }
            let headers = get_slot(c, this, Slot::Headers)?;
            put(c, headers, headers::METHOD, at(0))?;
            Ok(undefined)
        }
        Member::Authority => {
            let headers = get_slot(c, this, Slot::Headers)?;
            headers::authority(c, headers)
        }
        Member::Scheme => {
            let headers = get_slot(c, this, Slot::Headers)?;
            get(c, headers, headers::SCHEME)
        }
        Member::Url => {
            let headers = get_slot(c, this, Slot::Headers)?;
            get(c, headers, headers::PATH)
        }
        Member::SetUrl => {
            let headers = get_slot(c, this, Slot::Headers)?;
            put(c, headers, headers::PATH, at(0))?;
            Ok(undefined)
        }
        Member::RequestSetTimeout => {
            if !state_flag(c, this, "closed")? {
                let stream = stream(c, this)?;
                call_method(c, stream, "setTimeout", &[at(0), at(1)])?;
            }
            Ok(this)
        }
        Member::ResponseConstruct => {
            construct_response(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::Header | Member::HeadersSent => {
            let stream = stream(c, this)?;
            get(c, stream, "headersSent")
        }
        Member::WritableEnded => {
            let state = state(c, this)?;
            get(c, state, "ending")
        }
        Member::ResponseSocket => {
            if state_flag(c, this, "closed")? {
                return Ok(undefined);
            }
            let stream = stream(c, this)?;
            socket_of(c, stream)
        }
        Member::SendDate => {
            let state = state(c, this)?;
            get(c, state, "sendDate")
        }
        Member::SetSendDate => {
            let value = c.truthy_rooted(at(0))?;
            let value = c.boolean(value);
            set_state(c, this, "sendDate", value)?;
            Ok(undefined)
        }
        Member::StatusCode => {
            let state = state(c, this)?;
            get(c, state, "statusCode")
        }
        Member::SetStatusCode => {
            set_status_code(c, this, at(0))?;
            Ok(undefined)
        }
        Member::WritableCorked => forward(c, this, "writableCorked"),
        Member::WritableHighWaterMark => forward(c, this, "writableHighWaterMark"),
        Member::WritableFinished => forward(c, this, "writableFinished"),
        Member::WritableLength => forward(c, this, "writableLength"),
        Member::WritableObjectMode => forward(c, this, "writableObjectMode"),
        Member::WritableNeedDrain => forward(c, this, "writableNeedDrain"),
        Member::StatusMessage => {
            warn_once(c, STATUS_MESSAGE_WARNING, |state| &mut state.status_message_warned)?;
            Ok(c.string_rooted(""))
        }
        Member::SetStatusMessage => {
            warn_once(c, STATUS_MESSAGE_WARNING, |state| &mut state.status_message_warned)?;
            Ok(undefined)
        }
        Member::SetTrailer => {
            let name = normalized_name(c, at(0))?;
            assert_valid_header(c, &name, at(1))?;
            let trailers = get_slot(c, this, Slot::Trailers)?;
            put(c, trailers, &name, at(1))?;
            Ok(undefined)
        }
        Member::AddTrailers => {
            let headers = at(0);
            for key in c.enumerable_own_keys_rooted(headers, false, true)? {
                let value = c.get_property_rooted(headers, key)?;
                call_method(c, this, "setTrailer", &[key, value])?;
            }
            Ok(undefined)
        }
        Member::GetHeader => {
            let name = normalized_name(c, at(0))?;
            let headers = get_slot(c, this, Slot::Headers)?;
            get(c, headers, &name)
        }
        Member::GetHeaderNames => {
            let headers = get_slot(c, this, Slot::Headers)?;
            let keys = c.enumerable_own_keys_rooted(headers, false, true)?;
            c.array_rooted(&keys)
        }
        Member::GetHeaders => {
            let headers = get_slot(c, this, Slot::Headers)?;
            headers::null_copy(c, headers)
        }
        Member::HasHeader => {
            let name = normalized_name(c, at(0))?;
            let headers = get_slot(c, this, Slot::Headers)?;
            let has = c.has_own_property_rooted(headers, &name)?;
            Ok(c.boolean(has))
        }
        Member::RemoveHeader => {
            validate_string(c, at(0), "name")?;
            ensure_head_unsent(c, this)?;
            let name = normalized_name(c, at(0))?;
            if name == headers::DATE {
                let no = c.boolean(false);
                set_state(c, this, "sendDate", no)?;
                return Ok(undefined);
            }
            let headers = get_slot(c, this, Slot::Headers)?;
            let key = c.string_rooted(&name);
            c.delete_property_rooted(headers, key)?;
            Ok(undefined)
        }
        Member::SetHeader => {
            validate_string(c, at(0), "name")?;
            ensure_head_unsent(c, this)?;
            set_header(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::AppendHeader => {
            validate_string(c, at(0), "name")?;
            ensure_head_unsent(c, this)?;
            append_header(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::FlushHeaders => {
            let stream = stream(c, this)?;
            if !state_flag(c, this, "closed")? && !truthy_member(c, stream, "headersSent")? {
                let state = state(c, this)?;
                let status = get(c, state, "statusCode")?;
                call_method(c, this, "writeHead", &[status])?;
            }
            Ok(undefined)
        }
        Member::WriteHead => write_head(c, this, at(0), at(1), at(2)),
        Member::Cork | Member::Uncork => {
            let name = if matches!(current::<Member>(c)?, Member::Cork) { "cork" } else { "uncork" };
            let stream = stream(c, this)?;
            call_method(c, stream, name, &[])?;
            Ok(undefined)
        }
        Member::Write => write(c, this, at(0), at(1), at(2)),
        Member::End => end(c, this, at(0), at(1), at(2)),
        Member::Destroy => {
            if state_flag(c, this, "destroyed")? {
                return Ok(undefined);
            }
            let yes = c.boolean(true);
            set_state(c, this, "destroyed", yes)?;
            let stream = stream(c, this)?;
            call_method(c, stream, "destroy", &[at(0)])?;
            Ok(undefined)
        }
        Member::ResponseSetTimeout => {
            if !state_flag(c, this, "closed")? {
                let stream = stream(c, this)?;
                call_method(c, stream, "setTimeout", &[at(0), at(1)])?;
            }
            Ok(undefined)
        }
        Member::CreatePushResponse => {
            let (headers, callback) = (at(0), at(1));
            super::validate_function(c, callback, "callback")?;
            if state_flag(c, this, "closed")? {
                let error = Fault::InvalidStream.error(c, &[])?;
                next_tick(c, callback, &[error])?;
                return Ok(undefined);
            }
            let stream = stream(c, this)?;
            let options = c.object_rooted()?;
            let answer = closure(c, Member::PushResponse, &[callback])?;
            call_method(c, stream, "pushStream", &[headers, options, answer])?;
            Ok(undefined)
        }
        Member::WriteContinue => {
            let status = c.number(STATUS_CONTINUE);
            call_method(c, this, "writeInformation", &[status])
        }
        Member::WriteInformation => write_information(c, this, at(0), at(1)),
        Member::WriteEarlyHints => write_early_hints(c, this, at(0)),
        Member::StreamData => {
            let request = get_slot(c, this, Slot::Request)?;
            if c.is_object_rooted(request)? {
                let pushed = call_method(c, request, "push", &[at(0)])?;
                if !c.truthy_rooted(pushed)? {
                    call_method(c, this, "pause", &[])?;
                }
            }
            Ok(undefined)
        }
        Member::StreamTrailers => {
            let request = get_slot(c, this, Slot::Request)?;
            if c.is_object_rooted(request)? {
                let trailers = get_slot(c, request, Slot::Trailers)?;
                for key in c.enumerable_own_keys_rooted(at(0), false, true)? {
                    let value = c.get_property_rooted(at(0), key)?;
                    c.set_property_rooted(trailers, key, value, trailers)?;
                }
                let raw = get_slot(c, request, Slot::RawTrailers)?;
                let values = c.array_like_values_rooted(at(2))?;
                call_method(c, raw, "push", &values)?;
            }
            Ok(undefined)
        }
        Member::StreamEnd => {
            let request = get_slot(c, this, Slot::Request)?;
            if c.is_object_rooted(request)? {
                let null = c.null();
                call_method(c, request, "push", &[null])?;
            }
            Ok(undefined)
        }
        Member::StreamError => Ok(undefined),
        Member::StreamAbortedRequest => {
            let request = get_slot(c, this, Slot::Request)?;
            if c.is_object_rooted(request)? && !state_flag(c, request, "closed")? {
                let yes = c.boolean(true);
                put_slot(c, request, Slot::Aborted, yes)?;
                emit(c, request, "aborted", &[])?;
            }
            Ok(undefined)
        }
        Member::StreamCloseRequest => {
            stream_close_request(c, this)?;
            Ok(undefined)
        }
        Member::StreamTimeout => {
            let [which] = captured(c)?;
            let slot = if c.truthy_rooted(which)? { Slot::Request } else { Slot::Response };
            let target = get_slot(c, this, slot)?;
            if c.is_object_rooted(target)? {
                emit(c, target, "timeout", &[])?;
            }
            Ok(undefined)
        }
        Member::RequestPause | Member::RequestResume => {
            let name = if matches!(current::<Member>(c)?, Member::RequestPause) { "pause" } else { "resume" };
            let stream = stream(c, this)?;
            call_method(c, stream, name, &[])?;
            Ok(undefined)
        }
        Member::ResumeStream => {
            let [stream] = captured(c)?;
            call_method(c, stream, "resume", &[])?;
            Ok(undefined)
        }
        Member::StreamDrain => {
            let response = get_slot(c, this, Slot::Response)?;
            if c.is_object_rooted(response)? {
                emit(c, response, "drain", &[])?;
            }
            Ok(undefined)
        }
        Member::StreamCloseResponse => {
            stream_close_response(c, this)?;
            Ok(undefined)
        }
        Member::StreamTrailersReady => {
            let response = get_slot(c, this, Slot::Response)?;
            let trailers = get_slot(c, response, Slot::Trailers)?;
            call_method(c, this, "sendTrailers", &[trailers])?;
            Ok(undefined)
        }
        Member::SocketHas => {
            let (stream, property) = (at(0), at(1));
            let target = socket_target(c, stream)?;
            let has = c.has_property_rooted(stream, property)? || c.has_property_rooted(target, property)?;
            Ok(c.boolean(has))
        }
        Member::SocketGet => {
            let (stream, property) = (at(0), at(1));
            let name = c.string_text(property)?;
            match name.as_deref() {
                Some(name) if STREAM_BOUND.contains(&name) => {
                    let value = get(c, stream, name)?;
                    super::bound(c, value, stream)
                }
                Some(name) if STREAM_FIELDS.contains(&name) => get(c, stream, name),
                Some("readable") => {
                    if truthy_member(c, stream, "destroyed")? {
                        return Ok(c.boolean(false));
                    }
                    let request = get_slot(c, stream, Slot::Request)?;
                    let source = if c.is_object_rooted(request)? { request } else { stream };
                    get(c, source, "readable")
                }
                Some("setTimeout") => {
                    let session = get(c, stream, "session")?;
                    let owner = if c.is_object_rooted(session)? { session } else { stream };
                    let value = get(c, owner, "setTimeout")?;
                    super::bound(c, value, owner)
                }
                Some(name) if SOCKET_MANIPULATION.contains(&name) => Err(super::throw(c, Fault::NoSocketManipulation, &[])),
                _ => {
                    let target = socket_target(c, stream)?;
                    let value = c.get_property_rooted(target, property)?;
                    super::bound(c, value, target)
                }
            }
        }
        Member::SocketSet => {
            let (stream, property, value) = (at(0), at(1), at(2));
            let name = c.string_text(property)?;
            match name.as_deref() {
                Some(name) if STREAM_BOUND.contains(&name) || STREAM_FIELDS.contains(&name) || name == "readable" => {
                    put(c, stream, name, value)?;
                }
                Some("setTimeout") => {
                    let session = get(c, stream, "session")?;
                    let owner = if c.is_object_rooted(session)? { session } else { stream };
                    put(c, owner, "setTimeout", value)?;
                }
                Some(name) if SOCKET_MANIPULATION.contains(&name) => return Err(super::throw(c, Fault::NoSocketManipulation, &[])),
                _ => {
                    let target = socket_target(c, stream)?;
                    c.set_property_rooted(target, property, value, target)?;
                }
            }
            Ok(c.boolean(true))
        }
        Member::SocketPrototype => {
            let target = socket_target(c, at(0))?;
            c.prototype_rooted(target)
        }
        Member::PushResponse => {
            let [callback] = captured(c)?;
            if c.truthy_rooted(at(0))? {
                return c.call_rooted(callback, undefined, &[at(0)]);
            }
            let class = class(c, Class::Http2ServerResponse)?;
            let response = c.construct_rooted(class, class, &[at(1)])?;
            let null = c.null();
            c.call_rooted(callback, undefined, &[null, response])
        }
    }
}

fn forward(c: &mut Context<'_>, response: RootId, name: &str) -> Outcome {
    let stream = stream(c, response)?;
    get(c, stream, name)
}

fn construct_request(c: &mut Context<'_>, this: RootId, stream: RootId, headers: RootId, options: RootId, raw: RootId) -> Result<(), RootedError> {
    let readable_options = c.object_rooted()?;
    put_bool(c, readable_options, "autoDestroy", false)?;
    super::assign(c, readable_options, options)?;
    let stream_module = crate::modules::stream_shared_vm::module(c)?;
    let readable = get(c, stream_module, "Readable")?;
    c.call_rooted(readable, this, &[readable_options])?;
    let no = c.boolean(false);
    let state = c.object_rooted()?;
    put(c, state, "closed", no)?;
    put(c, state, "didRead", no)?;
    put_slot(c, this, Slot::CompatState, state)?;
    put_slot(c, this, Slot::Headers, headers)?;
    put_slot(c, this, Slot::RawHeaders, raw)?;
    let trailers = c.object_rooted()?;
    put_slot(c, this, Slot::Trailers, trailers)?;
    let raw_trailers = c.array_rooted(&[])?;
    put_slot(c, this, Slot::RawTrailers, raw_trailers)?;
    put_slot(c, this, Slot::Stream, stream)?;
    put_slot(c, this, Slot::Aborted, no)?;
    put_slot(c, stream, Slot::Request, this)?;
    let yes = c.boolean(true);
    for (event, member, captures) in [
        ("trailers", Member::StreamTrailers, None),
        ("end", Member::StreamEnd, None),
        ("error", Member::StreamError, None),
        ("aborted", Member::StreamAbortedRequest, None),
        ("close", Member::StreamCloseRequest, None),
        ("timeout", Member::StreamTimeout, Some(yes)),
    ] {
        let captures: Vec<RootId> = captures.into_iter().collect();
        let listener = closure(c, member, &captures)?;
        on(c, stream, event, listener)?;
    }
    let pause = closure(c, Member::RequestPause, &[])?;
    on(c, this, "pause", pause)?;
    let resume = closure(c, Member::RequestResume, &[])?;
    on(c, this, "resume", resume)
}

fn construct_response(c: &mut Context<'_>, this: RootId, stream: RootId, options: RootId) -> Result<(), RootedError> {
    let stream_module = crate::modules::stream_shared_vm::module(c)?;
    let base = get(c, stream_module, "Stream")?;
    c.call_rooted(base, this, &[options])?;
    let no = c.boolean(false);
    let yes = c.boolean(true);
    let state = c.object_rooted()?;
    for name in ["closed", "ending", "destroyed", "headRequest"] {
        put(c, state, name, no)?;
    }
    put(c, state, "sendDate", yes)?;
    put_number(c, state, "statusCode", STATUS_OK)?;
    put_slot(c, this, Slot::CompatState, state)?;
    let headers = c.null_object_rooted()?;
    put_slot(c, this, Slot::Headers, headers)?;
    let trailers = c.null_object_rooted()?;
    put_slot(c, this, Slot::Trailers, trailers)?;
    put_slot(c, this, Slot::Stream, stream)?;
    put_slot(c, stream, Slot::Response, this)?;
    put(c, this, "writable", yes)?;
    let request = get_slot(c, stream, Slot::Request)?;
    put(c, this, "req", request)?;
    let no_op = closure(c, Member::StreamError, &[])?;
    for (event, listener) in [
        ("drain", closure(c, Member::StreamDrain, &[])?),
        ("aborted", no_op),
        ("close", closure(c, Member::StreamCloseResponse, &[])?),
        ("wantTrailers", closure(c, Member::StreamTrailersReady, &[])?),
        ("timeout", closure(c, Member::StreamTimeout, &[no])?),
    ] {
        on(c, stream, event, listener)?;
    }
    Ok(())
}

/// `onStreamCloseRequest()`.
fn stream_close_request(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let request = get_slot(c, stream, Slot::Request)?;
    if !c.is_object_rooted(request)? {
        return Ok(());
    }
    let yes = c.boolean(true);
    set_state(c, request, "closed", yes)?;
    let null = c.null();
    call_method(c, request, "push", &[null])?;
    let readable_state = get(c, request, "_readableState")?;
    if !state_flag(c, request, "didRead")? && !truthy_member(c, readable_state, "resumeScheduled")? {
        call_method(c, request, "resume", &[])?;
    }
    let undefined = c.undefined();
    put_slot(c, stream, Slot::Request, undefined)?;
    emit(c, request, "close", &[]).map(drop)
}

/// `onStreamCloseResponse()`.
fn stream_close_response(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let response = get_slot(c, stream, Slot::Response)?;
    if !c.is_object_rooted(response)? {
        return Ok(());
    }
    let head = truthy_member(c, stream, "headRequest")?;
    if head != state_flag(c, response, "headRequest")? {
        return Ok(());
    }
    let yes = c.boolean(true);
    set_state(c, response, "closed", yes)?;
    let undefined = c.undefined();
    put_slot(c, stream, Slot::Response, undefined)?;
    emit(c, response, "finish", &[])?;
    emit(c, response, "close", &[]).map(drop)
}

fn normalized_name(c: &mut Context<'_>, name: RootId) -> Result<String, RootedError> {
    let name = validate_string(c, name, "name")?;
    Ok(name.trim().to_lowercase())
}

fn ensure_head_unsent(c: &mut Context<'_>, response: RootId) -> Result<(), RootedError> {
    let stream = stream(c, response)?;
    if truthy_member(c, stream, "headersSent")? {
        return Err(super::throw(c, Fault::HeadersSent, &[]));
    }
    Ok(())
}

/// `assertValidHeader(name, value)`.
fn assert_valid_header(c: &mut Context<'_>, name: &str, value: RootId) -> Result<(), RootedError> {
    if name.is_empty() || name.contains(' ') {
        let name = c.string_rooted(name);
        let label = c.undefined();
        crate::modules::http_shared_vm::outgoing::validate_header_name(c, name, label)?;
        return Err(RootedError::host("invalid header name passed validation"));
    }
    if PSEUDO_HEADERS.contains(&name) {
        return Err(super::throw(c, Fault::PseudoheaderNotAllowed, &[]));
    }
    if is_nullish(c, value) {
        let shown = c.to_string(value)?;
        return Err(super::throw(c, Fault::InvalidHeaderValue, &[&shown, name]));
    }
    if !connection_header_allowed(c, name, value)? {
        warn_once(c, CONNECTION_HEADER_WARNING, |state| &mut state.connection_header_warned)?;
    }
    Ok(())
}

fn connection_header_allowed(c: &mut Context<'_>, name: &str, value: RootId) -> Result<bool, RootedError> {
    Ok(name != CONNECTION || c.string_text(value)?.as_deref() == Some(TRAILERS))
}

/// The validation `kSetHeader` and `kAppendHeader` share; whether the header is kept.
fn checked_name(c: &mut Context<'_>, response: RootId, name: RootId, value: RootId) -> Result<Option<String>, RootedError> {
    let name = normalized_name(c, name)?;
    assert_valid_header(c, &name, value)?;
    if !connection_header_allowed(c, &name, value)? {
        return Ok(None);
    }
    if name.starts_with(':') {
        if !headers::is_valid_pseudo_header(&name) {
            return Err(super::throw(c, Fault::InvalidPseudoheader, &[&name]));
        }
    } else {
        let root = c.string_rooted(&name);
        if !crate::modules::http_shared_vm::outgoing::is_token(c, root)? {
            let label = c.undefined();
            if let Err(error) = crate::modules::http_shared_vm::outgoing::validate_header_name(c, root, label) {
                let error = error.exception.ok_or(error)?;
                call_method(c, response, "destroy", &[error])?;
            }
        }
    }
    Ok(Some(name))
}

/// `kSetHeader(name, value)`.
fn set_header(c: &mut Context<'_>, response: RootId, name: RootId, value: RootId) -> Result<(), RootedError> {
    let Some(name) = checked_name(c, response, name, value)? else { return Ok(()) };
    let headers = get_slot(c, response, Slot::Headers)?;
    put(c, headers, &name, value)
}

/// `kAppendHeader(name, value)`.
fn append_header(c: &mut Context<'_>, response: RootId, name: RootId, value: RootId) -> Result<(), RootedError> {
    let Some(key) = checked_name(c, response, name, value)? else { return Ok(()) };
    let headers = get_slot(c, response, Slot::Headers)?;
    let existing = get(c, headers, &key)?;
    if !c.truthy_rooted(existing)? {
        return call_method(c, response, "setHeader", &[name, value]).map(drop);
    }
    let list = if c.is_array_rooted(existing)? {
        existing
    } else {
        let list = c.array_rooted(&[existing])?;
        put(c, headers, &key, list)?;
        list
    };
    let values = if c.is_array_rooted(value)? { c.array_like_values_rooted(value)? } else { vec![value] };
    call_method(c, list, "push", &values).map(drop)
}

/// `res.statusCode = code`.
fn set_status_code(c: &mut Context<'_>, response: RootId, code: RootId) -> Result<(), RootedError> {
    let code = f64::from(c.to_number(code)? as i32);
    if INFORMATIONAL.contains(&code) {
        return Err(super::throw(c, Fault::InfoStatusNotAllowed, &[]));
    }
    if !STATUS_RANGE.contains(&code) {
        return Err(super::throw(c, Fault::StatusInvalid, &[&super::number_text(code)]));
    }
    let code = c.number(code);
    set_state(c, response, "statusCode", code)
}

/// `res.writeHead(statusCode, statusMessage, headers)`.
fn write_head(c: &mut Context<'_>, response: RootId, status: RootId, message: RootId, headers: RootId) -> Outcome {
    let stream = stream(c, response)?;
    if state_flag(c, response, "closed")? || truthy_member(c, stream, "destroyed")? || truthy_member(c, stream, "closed")? {
        return Ok(response);
    }
    if truthy_member(c, stream, "headersSent")? {
        return Err(super::throw(c, Fault::HeadersSent, &[]));
    }
    if c.string_text(message)?.is_some() {
        warn_once(c, STATUS_MESSAGE_WARNING, |state| &mut state.status_message_warned)?;
    }
    let headers = if is_undefined(c, headers) && c.is_object_rooted(message)? { message } else { headers };
    if c.is_array_rooted(headers)? {
        let values = c.array_like_values_rooted(headers)?;
        let tuples = match values.first() {
            Some(&first) => c.is_array_rooted(first)?,
            None => false,
        };
        let pairs: Vec<(RootId, RootId)> = if tuples {
            let mut pairs = Vec::new();
            for tuple in values {
                let name = get(c, tuple, "0")?;
                let value = get(c, tuple, "1")?;
                pairs.push((name, value));
            }
            pairs
        } else {
            if values.len() % 2 != 0 {
                return Err(crate::modules::util_shared_vm::argument_value_error(c, headers, "headers", "is invalid")?);
            }
            values.chunks(headers::PAIR).map(|pair| (pair[0], pair[1])).collect()
        };
        for &(name, _) in &pairs {
            call_method(c, response, "removeHeader", &[name])?;
        }
        for (name, value) in pairs {
            append_header(c, response, name, value)?;
        }
    } else if c.is_object_rooted(headers)? {
        for key in c.enumerable_own_keys_rooted(headers, false, true)? {
            let value = c.get_property_rooted(headers, key)?;
            set_header(c, response, key, value)?;
        }
    }
    set_state(c, response, "statusCode", status)?;
    begin_send(c, response)?;
    Ok(response)
}

/// `kBeginSend()`.
fn begin_send(c: &mut Context<'_>, response: RootId) -> Result<(), RootedError> {
    let state = state(c, response)?;
    let headers = get_slot(c, response, Slot::Headers)?;
    let status = get(c, state, "statusCode")?;
    put(c, headers, headers::STATUS, status)?;
    let options = c.object_rooted()?;
    let ending = get(c, state, "ending")?;
    put(c, options, "endStream", ending)?;
    put_bool(c, options, "waitForTrailers", true)?;
    let send_date = get(c, state, "sendDate")?;
    put(c, options, "sendDate", send_date)?;
    let stream = stream(c, response)?;
    call_method(c, stream, "respond", &[headers, options]).map(drop)
}

/// `res.write(chunk, encoding, cb)`.
fn write(c: &mut Context<'_>, response: RootId, chunk: RootId, encoding: RootId, callback: RootId) -> Outcome {
    let (encoding, callback) = if c.is_callable_rooted(encoding)? { (c.string_rooted("utf8"), encoding) } else { (encoding, callback) };
    let fault = if state_flag(c, response, "ending")? {
        Some(Fault::StreamWriteAfterEnd)
    } else if state_flag(c, response, "closed")? {
        Some(Fault::InvalidStream)
    } else if state_flag(c, response, "destroyed")? {
        return Ok(c.boolean(false));
    } else {
        None
    };
    if let Some(fault) = fault {
        let error = fault.error(c, &[])?;
        if c.is_callable_rooted(callback)? {
            next_tick(c, callback, &[error])?;
        }
        call_method(c, response, "destroy", &[error])?;
        return Ok(c.boolean(false));
    }
    let stream = stream(c, response)?;
    if !truthy_member(c, stream, "headersSent")? {
        let state = state(c, response)?;
        let status = get(c, state, "statusCode")?;
        call_method(c, response, "writeHead", &[status])?;
    }
    call_method(c, stream, "write", &[chunk, encoding, callback])
}

/// `res.end(chunk, encoding, cb)`.
fn end(c: &mut Context<'_>, response: RootId, chunk: RootId, encoding: RootId, callback: RootId) -> Outcome {
    let stream = stream(c, response)?;
    let (chunk, encoding, callback) = if c.is_callable_rooted(chunk)? {
        (c.null(), encoding, chunk)
    } else if c.is_callable_rooted(encoding)? {
        (chunk, c.string_rooted("utf8"), encoding)
    } else {
        (chunk, encoding, callback)
    };
    let head = truthy_member(c, stream, "headRequest")?;
    let finished = state_flag(c, response, "closed")? || state_flag(c, response, "ending")?;
    if finished && head == state_flag(c, response, "headRequest")? {
        if c.is_callable_rooted(callback)? {
            next_tick(c, callback, &[])?;
        }
        return Ok(response);
    }
    if !is_nullish(c, chunk) {
        call_method(c, response, "write", &[chunk, encoding])?;
    }
    let head_value = c.boolean(head);
    set_state(c, response, "headRequest", head_value)?;
    let yes = c.boolean(true);
    set_state(c, response, "ending", yes)?;
    if c.is_callable_rooted(callback)? {
        if truthy_member(c, stream, "writableEnded")? {
            once(c, response, "finish", callback)?;
        } else {
            once(c, stream, "finish", callback)?;
        }
    }
    if !truthy_member(c, stream, "headersSent")? {
        let state = state(c, response)?;
        let status = get(c, state, "statusCode")?;
        call_method(c, response, "writeHead", &[status])?;
    }
    if state_flag(c, response, "closed")? || truthy_member(c, stream, "destroyed")? {
        stream_close_response(c, stream)?;
    } else {
        call_method(c, stream, "end", &[])?;
    }
    Ok(response)
}

/// `res.writeEarlyHints(hints)`.
fn write_early_hints(c: &mut Context<'_>, response: RootId, hints: RootId) -> Outcome {
    if !c.is_object_rooted(hints)? || c.is_array_rooted(hints)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, hints, "hints", "Object")?);
    }
    let link = get(c, hints, "link")?;
    let link = link_header_value(c, link)?;
    let headers = c.null_object_rooted()?;
    for key in c.enumerable_own_keys_rooted(hints, false, true)? {
        if c.string_text(key)?.as_deref() == Some("link") {
            continue;
        }
        let value = c.get_property_rooted(hints, key)?;
        let name = normalized_name(c, key)?;
        assert_valid_header(c, &name, value)?;
        let root = c.string_rooted(&name);
        let label = c.undefined();
        crate::modules::http_shared_vm::outgoing::validate_header_name(c, root, label)?;
        put(c, headers, &name, value)?;
    }
    if link.is_empty() {
        return Ok(c.boolean(false));
    }
    let link = c.string_rooted(&link);
    put(c, headers, "Link", link)?;
    let status = c.number(STATUS_EARLY_HINTS);
    call_method(c, response, "writeInformation", &[status, headers])
}

/// `res.writeInformation(statusCode, headers)`: an informational head, unless the final
/// head was sent or the response closed.
fn write_information(c: &mut Context<'_>, response: RootId, status: RootId, given: RootId) -> Outcome {
    let code = c.rooted_value(status).and_then(|value| value.as_number());
    if !code.is_some_and(|code| INFORMATIONAL.contains(&code) && code != STATUS_SWITCHING_PROTOCOLS) {
        let shown = c.to_string(status)?;
        return Err(super::throw(c, Fault::StatusInvalid, &[&shown]));
    }
    let stream = stream(c, response)?;
    if truthy_member(c, stream, "headersSent")? || state_flag(c, response, "closed")? {
        return Ok(c.boolean(false));
    }
    let headers = c.null_object_rooted()?;
    if !is_nullish(c, given) {
        if !c.is_object_rooted(given)? || c.is_array_rooted(given)? {
            return Err(crate::modules::util_shared_vm::argument_type_error(c, given, "headers", "Object")?);
        }
        for key in c.enumerable_own_keys_rooted(given, false, true)? {
            let value = c.get_property_rooted(given, key)?;
            c.set_property_rooted(headers, key, value, headers)?;
        }
    }
    put(c, headers, headers::STATUS, status)?;
    call_method(c, stream, "additionalHeaders", &[headers])?;
    Ok(c.boolean(true))
}

/// `validateLinkHeaderValue(hints.link)`: one value, or a list joined with `, `.
fn link_header_value(c: &mut Context<'_>, link: RootId) -> Result<String, RootedError> {
    if let Some(text) = c.string_text(link)? {
        return Ok(text);
    }
    if c.is_array_rooted(link)? {
        let mut parts = Vec::new();
        for value in c.array_like_values_rooted(link)? {
            match c.string_text(value)? {
                Some(text) => parts.push(text),
                None => break,
            }
        }
        if parts.len() == c.array_like_values_rooted(link)?.len() {
            return Ok(parts.join(", "));
        }
    }
    Err(crate::modules::util_shared_vm::argument_value_error(c, link, "hints.link", "must be an array or string of format \"</styles.css>; rel=preload; as=style\"")?)
}

/// `onServerStream(ServerRequest, ServerResponse, stream, headers, flags, rawHeaders)`.
pub(super) fn on_server_stream(
    c: &mut Context<'_>,
    server: RootId,
    request_class: RootId,
    response_class: RootId,
    stream: RootId,
    headers: RootId,
    raw: RootId,
) -> Result<(), RootedError> {
    let undefined = c.undefined();
    let request = c.construct_rooted(request_class, request_class, &[stream, headers, undefined, raw])?;
    let response = c.construct_rooted(response_class, response_class, &[stream])?;
    let method = get(c, headers, headers::METHOD)?;
    if c.string_text(method)?.as_deref() == Some("CONNECT") {
        if !emit(c, server, "connect", &[request, response])? {
            put_number(c, response, "statusCode", STATUS_METHOD_NOT_ALLOWED)?;
            call_method(c, response, "end", &[])?;
        }
        return Ok(());
    }
    let expect = get(c, headers, "expect")?;
    if !is_undefined(c, expect) {
        if c.string_text(expect)?.as_deref() == Some(EXPECT_CONTINUE) {
            if crate::modules::stream_shared_vm::listener_count(c, server, "checkContinue")? > 0.0 {
                emit(c, server, "checkContinue", &[request, response])?;
            } else {
                call_method(c, response, "writeContinue", &[])?;
                emit(c, server, "request", &[request, response])?;
            }
        } else if crate::modules::stream_shared_vm::listener_count(c, server, "checkExpectation")? > 0.0 {
            emit(c, server, "checkExpectation", &[request, response])?;
        } else {
            put_number(c, response, "statusCode", STATUS_EXPECTATION_FAILED)?;
            call_method(c, response, "end", &[])?;
        }
        return Ok(());
    }
    emit(c, server, "request", &[request, response]).map(drop)
}
