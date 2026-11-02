//! `Http2Stream`, `ServerHttp2Stream` and `ClientHttp2Stream` (lib/internal/http2/core.js):
//! a Duplex over one protocol stream of its session's connection. Writes queue DATA in
//! the connection and complete once framed; received DATA is pushed as it arrives.

use super::faults::Fault;
use super::frame::ErrorCode;
use super::headers::{self, Pseudo};
use super::session::{self, PendingRequest, SessionType};
use super::{
    class, closure, get_slot, handle_of, has_flag, put_slot, put_slot_bool, put_slot_number, set_flag, set_immediate,
    slot_number, slot_truthy, with_state, Class, Slot, StreamHandle,
};
use crate::modules::host_members::{accessors, captured, constructor, current, members};
use crate::modules::stream_shared_vm::{
    call_method, emit, get, is_nullish, is_undefined, next_tick, on, once, put, put_bool, put_number, truthy_member, Context,
    Outcome,
};
use quench_runtime::{RootId, RootedError};

/// A stream's `kState.flags`.
pub(super) mod stream_flags {
    pub(in super::super) const READY: u32 = 0x1;
    pub(in super::super) const CLOSED: u32 = 0x2;
    pub(in super::super) const HEADERS_SENT: u32 = 0x4;
    pub(in super::super) const HEAD_REQUEST: u32 = 0x8;
    pub(in super::super) const ABORTED: u32 = 0x10;
    pub(in super::super) const HAS_TRAILERS: u32 = 0x20;
}

/// Whether closing a stream submits RST_STREAM (`kNoRstStream`, `kSubmitRstStream`,
/// `kForceRstStream`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RstPolicy {
    None,
    Submit,
    Force,
}

/// The statuses whose responses carry no payload.
const STATUS_NO_CONTENT: f64 = 204.0;
const STATUS_RESET_CONTENT: f64 = 205.0;
const STATUS_NOT_MODIFIED: f64 = 304.0;
const NO_PAYLOAD_STATUSES: [f64; 3] = [STATUS_NO_CONTENT, STATUS_RESET_CONTENT, STATUS_NOT_MODIFIED];
const STATUS_CONTINUE: f64 = 100.0;
const STATUS_SWITCHING_PROTOCOLS: f64 = 101.0;
const INFORMATIONAL: std::ops::Range<f64> = 100.0..200.0;
/// nghttp2's default stream weight, which `state` reports.
const DEFAULT_WEIGHT: f64 = 16.0;
/// `priority()`'s deprecation (RFC 9113 deprecated priority signals).
const PRIORITY_DEPRECATION: &str =
    "http2Stream.priority is longer supported after priority signalling was deprecated in RFC 9113";
const PRIORITY_DEPRECATION_CODE: &str = "DEP0194";
/// The chunk a file response reads its descriptor in.
const FILE_CHUNK: usize = 64 * 1024;

members!(Member, "http2Stream", [
    Construct,
    ServerConstruct,
    ClientConstruct,
    BufferSize,
    EndAfterHeaders,
    SentHeaders,
    SentTrailers,
    SentInfoHeaders,
    Pending,
    Id,
    Session,
    HeadersSent,
    Aborted,
    HeadRequest,
    RstCode,
    State,
    Closed,
    PushAllowed,
    Write,
    Writev,
    Final,
    Read,
    SendTrailers,
    Close,
    Destroy,
    Priority,
    SetTimeout,
    OnTimeout,
    PushStream,
    Respond,
    RespondWithFd,
    RespondWithFile,
    AdditionalHeaders,
    /// A write or `_final` deferred until the stream has its id.
    WriteWhenReady,
    FinalWhenReady,
    /// `kMaybeDestroy`, as an `end` or `finish` listener.
    MaybeDestroy,
    /// `finishSendTrailers(stream, headersList)`.
    FinishSendTrailers,
    /// `finishCloseStream(code)` on `finish`, and its RST_STREAM on `ready`.
    FinishClose,
    SubmitRst,
    /// `handleHeaderContinue(headers)`.
    HeadersContinue,
    /// `callStreamClose(stream)`.
    CallStreamClose,
    /// `respondWithFile`'s `fs.open` and both responses' `fs.fstat` callbacks.
    FileOpened,
    FileStat,
]);

pub(super) fn build(c: &mut Context<'_>) -> Result<(), RootedError> {
    let duplex = crate::modules::stream_shared_vm::intrinsics(c)?.class(crate::modules::stream_shared_vm::Class::Duplex);
    let stream = constructor(c, Member::Construct, "Http2Stream")?;
    crate::modules::http_shared_vm::inherit(c, stream, duplex)?;
    let prototype = get(c, stream, "prototype")?;
    accessors(c, prototype, &[
        ("bufferSize", Member::BufferSize, None),
        ("endAfterHeaders", Member::EndAfterHeaders, None),
        ("sentHeaders", Member::SentHeaders, None),
        ("sentTrailers", Member::SentTrailers, None),
        ("sentInfoHeaders", Member::SentInfoHeaders, None),
        ("pending", Member::Pending, None),
        ("id", Member::Id, None),
        ("session", Member::Session, None),
        ("headersSent", Member::HeadersSent, None),
        ("aborted", Member::Aborted, None),
        ("headRequest", Member::HeadRequest, None),
        ("rstCode", Member::RstCode, None),
        ("state", Member::State, None),
        ("closed", Member::Closed, None),
    ])?;
    super::class_methods(c, prototype, &[
        ("_onTimeout", Member::OnTimeout),
        ("_write", Member::Write),
        ("_writev", Member::Writev),
        ("_final", Member::Final),
        ("_read", Member::Read),
        ("sendTrailers", Member::SendTrailers),
        ("close", Member::Close),
        ("_destroy", Member::Destroy),
    ])?;
    crate::modules::host_members::methods(c, prototype, &[("setTimeout", Member::SetTimeout)])?;
    let priority = crate::modules::host_members::function(c, Member::Priority, "priority", &[])?;
    let util = crate::host::shared_vm::commonjs::util_module(c)?;
    let message = c.string_rooted(PRIORITY_DEPRECATION);
    let code = c.string_rooted(PRIORITY_DEPRECATION_CODE);
    let priority = call_method(c, util, "deprecate", &[priority, message, code])?;
    put(c, prototype, "priority", priority)?;
    super::set_class(c, Class::Http2Stream, stream)?;

    let server = constructor(c, Member::ServerConstruct, "ServerHttp2Stream")?;
    crate::modules::http_shared_vm::inherit(c, server, stream)?;
    let prototype = get(c, server, "prototype")?;
    accessors(c, prototype, &[("pushAllowed", Member::PushAllowed, None)])?;
    super::class_methods(c, prototype, &[
        ("pushStream", Member::PushStream),
        ("respond", Member::Respond),
        ("respondWithFD", Member::RespondWithFd),
        ("respondWithFile", Member::RespondWithFile),
        ("additionalHeaders", Member::AdditionalHeaders),
    ])?;
    super::set_class(c, Class::ServerHttp2Stream, server)?;

    let client = constructor(c, Member::ClientConstruct, "ClientHttp2Stream")?;
    crate::modules::http_shared_vm::inherit(c, client, stream)?;
    super::set_class(c, Class::ClientHttp2Stream, client)?;

    let maybe_destroy = closure(c, Member::MaybeDestroy, &[])?;
    super::set_class(c, Class::StreamMaybeDestroy, maybe_destroy)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::Construct => {
            initialize(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::ServerConstruct => {
            let (session, id, options, headers) = (at(0), at(2), at(3), at(4));
            initialize(c, this, session, options)?;
            let id = c.to_number(id)? as u32;
            init(c, this, id)?;
            let scheme = get(c, headers, headers::SCHEME)?;
            put_slot(c, this, Slot::Protocol, scheme)?;
            let authority = headers::authority(c, headers)?;
            put_slot(c, this, Slot::Authority, authority)?;
            Ok(undefined)
        }
        Member::ClientConstruct => {
            let (session, id, options) = (at(0), at(2), at(3));
            initialize(c, this, session, options)?;
            set_flag(c, this, stream_flags::HEADERS_SENT, true)?;
            if !is_undefined(c, id) {
                let id = c.to_number(id)? as u32;
                init(c, this, id)?;
            }
            let listener = closure(c, Member::HeadersContinue, &[])?;
            on(c, this, "headers", listener)?;
            Ok(undefined)
        }
        Member::BufferSize => {
            let length = crate::modules::stream_shared_vm::number(c, this, "writableLength")?;
            let queued = handle_of(c, this)?
                .map(|key| with_state(c, |state| state.streams.get(&key).map_or(0, |stream| stream.writes.len())))
                .unwrap_or(0);
            Ok(c.number(length + queued as f64))
        }
        Member::EndAfterHeaders => {
            let value = slot_truthy(c, this, Slot::EndAfterHeaders)?;
            Ok(c.boolean(value))
        }
        Member::SentHeaders => sent_headers(c, this),
        Member::SentTrailers => get_slot(c, this, Slot::SentTrailers),
        Member::SentInfoHeaders => get_slot(c, this, Slot::InfoHeaders),
        Member::Pending => {
            let id = get_slot(c, this, Slot::Id)?;
            Ok(c.boolean(is_undefined(c, id)))
        }
        Member::Id => get_slot(c, this, Slot::Id),
        Member::Session => get_slot(c, this, Slot::Session),
        Member::HeadersSent => flag_value(c, this, stream_flags::HEADERS_SENT),
        Member::Aborted => flag_value(c, this, stream_flags::ABORTED),
        Member::HeadRequest => flag_value(c, this, stream_flags::HEAD_REQUEST),
        Member::Closed => flag_value(c, this, stream_flags::CLOSED),
        Member::RstCode => get_slot(c, this, Slot::RstCode),
        Member::State => stream_state(c, this),
        Member::PushAllowed => {
            let allowed = push_allowed(c, this)?;
            Ok(c.boolean(allowed))
        }
        Member::Write => {
            write(c, this, &[(at(0), at(1))], at(2))?;
            Ok(undefined)
        }
        Member::Writev => {
            let mut chunks = Vec::new();
            for entry in c.array_like_values_rooted(at(0))? {
                let chunk = get(c, entry, "chunk")?;
                let encoding = get(c, entry, "encoding")?;
                chunks.push((chunk, encoding));
            }
            write(c, this, &chunks, at(1))?;
            Ok(undefined)
        }
        Member::Final => {
            finalize(c, this, at(0))?;
            Ok(undefined)
        }
        Member::Read => {
            if destroyed(c, this)? {
                let null = c.null();
                call_method(c, this, "push", &[null])?;
                return Ok(undefined);
            }
            if !slot_truthy(c, this, Slot::DidRead)? {
                let state = get(c, this, "_readableState")?;
                put_bool(c, state, "readingMore", false)?;
                put_slot_bool(c, this, Slot::DidRead, true)?;
            }
            Ok(undefined)
        }
        Member::SendTrailers => {
            send_trailers(c, this, at(0))?;
            Ok(undefined)
        }
        Member::Close => {
            let (code, callback) = (at(0), at(1));
            let code = if is_undefined(c, code) { c.number(f64::from(ErrorCode::NoError as u32)) } else { code };
            super::validate_integer(c, code, "code", 0.0, super::MAX_INT)?;
            if !is_undefined(c, callback) {
                super::validate_function(c, callback, "callback")?;
            }
            if closed(c, this)? {
                return Ok(undefined);
            }
            if !is_undefined(c, callback) {
                once(c, this, "close", callback)?;
            }
            let code = c.to_number(code)? as u32;
            close_stream(c, this, code, RstPolicy::Submit)?;
            Ok(undefined)
        }
        Member::Destroy => {
            destroy_stream(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::Priority => {
            if destroyed(c, this)? {
                return Err(super::throw(c, Fault::InvalidStream, &[]));
            }
            Ok(undefined)
        }
        Member::SetTimeout => session::set_timeout(c, this, at(0), at(1)),
        Member::OnTimeout => {
            if !destroyed(c, this)? {
                emit(c, this, "timeout", &[])?;
            }
            Ok(undefined)
        }
        Member::PushStream => {
            push_stream(c, this, at(0), at(1), at(2))?;
            Ok(undefined)
        }
        Member::Respond => {
            respond(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::RespondWithFd => {
            respond_with_fd(c, this, at(0), at(1), at(2))?;
            Ok(undefined)
        }
        Member::RespondWithFile => {
            respond_with_file(c, this, at(0), at(1), at(2))?;
            Ok(undefined)
        }
        Member::AdditionalHeaders => {
            additional_headers(c, this, at(0))?;
            Ok(undefined)
        }
        Member::WriteWhenReady => {
            let [stream, chunks, callback] = captured(c)?;
            let mut pairs = Vec::new();
            for entry in c.array_like_values_rooted(chunks)? {
                let chunk = get(c, entry, "chunk")?;
                let encoding = get(c, entry, "encoding")?;
                pairs.push((chunk, encoding));
            }
            write(c, stream, &pairs, callback)?;
            Ok(undefined)
        }
        Member::FinalWhenReady => {
            let [stream, callback] = captured(c)?;
            finalize(c, stream, callback)?;
            Ok(undefined)
        }
        Member::MaybeDestroy => {
            maybe_destroy(c, this, ErrorCode::NoError as u32)?;
            Ok(undefined)
        }
        Member::FinishSendTrailers => {
            let [stream, headers] = captured(c)?;
            finish_send_trailers(c, stream, headers)?;
            Ok(undefined)
        }
        Member::FinishClose => {
            let [stream, code] = captured(c)?;
            let code = c.to_number(code)? as u32;
            finish_close(c, stream, code)?;
            Ok(undefined)
        }
        Member::SubmitRst => {
            let [stream, code] = captured(c)?;
            let code = c.to_number(code)? as u32;
            submit_rst(c, stream, code)?;
            Ok(undefined)
        }
        Member::HeadersContinue => {
            let status = get(c, at(0), headers::STATUS)?;
            if c.rooted_value(status).and_then(|value| value.as_number()) == Some(STATUS_CONTINUE) {
                emit(c, this, "continue", &[])?;
            }
            Ok(undefined)
        }
        Member::CallStreamClose => {
            let [stream] = captured(c)?;
            call_method(c, stream, "close", &[])?;
            Ok(undefined)
        }
        Member::FileOpened => {
            let [stream, options, headers, fd_owned] = captured(c)?;
            let (error, fd) = (at(0), at(1));
            file_opened(c, stream, options, headers, fd_owned, error, fd)?;
            Ok(undefined)
        }
        Member::FileStat => {
            let [stream, options, fd, headers] = captured(c)?;
            file_stat(c, stream, options, fd, headers, at(0), at(1))?;
            Ok(undefined)
        }
    }
}

fn flag_value(c: &mut Context<'_>, stream: RootId, flag: u32) -> Outcome {
    let on = has_flag(c, stream, flag)?;
    Ok(c.boolean(on))
}

pub(super) fn destroyed(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    truthy_member(c, stream, "destroyed")
}

fn closed(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    has_flag(c, stream, stream_flags::CLOSED)
}

fn pending(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    let id = get_slot(c, stream, Slot::Id)?;
    Ok(is_undefined(c, id))
}

fn session_of(c: &mut Context<'_>, stream: RootId) -> Result<Option<RootId>, RootedError> {
    let session = get_slot(c, stream, Slot::Session)?;
    Ok(c.is_object_rooted(session)?.then_some(session))
}

fn is_server(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    match session_of(c, stream)? {
        Some(session) => Ok(session::session_type(c, session)? == SessionType::Server),
        None => Ok(false),
    }
}

/// The stream's protocol id and its session's handle, while both are live.
fn live(c: &mut Context<'_>, stream: RootId) -> Result<Option<(u64, u32)>, RootedError> {
    let Some(key) = handle_of(c, stream)? else { return Ok(None) };
    Ok(with_state(c, |state| {
        let stream = state.streams.get(&key)?;
        Some((stream.session, stream.id?))
    }))
}

/// Run `f` over the connection carrying the stream, with the stream's id.
fn with_connection<T>(c: &mut Context<'_>, stream: RootId, f: impl FnOnce(&mut super::connection::Connection, u32) -> T) -> Result<Option<T>, RootedError> {
    let Some((session, id)) = live(c, stream)? else { return Ok(None) };
    Ok(with_state(c, |state| state.sessions.get_mut(&session)?.connection.as_mut().map(|connection| f(connection, id))))
}

fn drive(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    match live(c, stream)? {
        Some((session, _)) => session::drive(c, session),
        None => Ok(()),
    }
}

/// `stream[kUpdateTimer]()`.
pub(super) fn update_timer(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    if destroyed(c, stream)? {
        return Ok(());
    }
    session::update_timer(c, stream)?;
    if let Some(session) = session_of(c, stream)? {
        session::update_timer(c, session)?;
    }
    Ok(())
}

/// `new Http2Stream(session, options)`.
fn initialize(c: &mut Context<'_>, this: RootId, session: RootId, options: RootId) -> Result<(), RootedError> {
    let options = if c.is_object_rooted(options)? { options } else { c.object_rooted()? };
    put_bool(c, options, "allowHalfOpen", true)?;
    put_bool(c, options, "decodeStrings", false)?;
    put_bool(c, options, "autoDestroy", false)?;
    let duplex = crate::modules::stream_shared_vm::intrinsics(c)?.class(crate::modules::stream_shared_vm::Class::Duplex);
    c.call_rooted(duplex, this, &[options])?;
    call_method(c, this, "cork", &[])?;
    put_slot(c, this, Slot::Session, session)?;
    let session_key = handle_of(c, session)?;
    if let Some(session_key) = session_key {
        let object = c.retain(this)?;
        let key = with_state(c, |state| {
            let key = state.allocate();
            state.streams.insert(key, StreamHandle { object, session: session_key, id: None, writes: Default::default(), request: None });
            if let Some(handle) = state.sessions.get_mut(&session_key) {
                handle.pending.push(key);
            }
            key
        });
        put_slot_number(c, this, Slot::Handle, key as f64)?;
    }
    let readable_state = get(c, this, "_readableState")?;
    put_bool(c, readable_state, "readingMore", true)?;
    let null = c.null();
    put_slot(c, this, Slot::Timeout, null)?;
    put_slot_number(c, this, Slot::Flags, 0.0)?;
    put_slot_number(c, this, Slot::RstCode, f64::from(ErrorCode::NoError as u32))?;
    put_slot_bool(c, this, Slot::DidRead, false)?;
    put_slot_bool(c, this, Slot::TrailersReady, false)?;
    put_slot_bool(c, this, Slot::EndAfterHeaders, false)?;
    put_slot_bool(c, this, Slot::ShutdownWritableCalled, false)?;
    put_slot(c, this, Slot::Request, null)?;
    Ok(())
}

/// `stream[kInit](id, handle)`.
pub(super) fn init(c: &mut Context<'_>, stream: RootId, id: u32) -> Result<(), RootedError> {
    set_flag(c, stream, stream_flags::READY, true)?;
    if let Some(key) = handle_of(c, stream)? {
        with_state(c, |state| {
            let Some(handle) = state.streams.get_mut(&key) else { return };
            handle.id = Some(id);
            let session = handle.session;
            if let Some(session) = state.sessions.get_mut(&session) {
                session.pending.retain(|pending| *pending != key);
                session.streams.insert(id, key);
            }
        });
    }
    put_slot_number(c, stream, Slot::Id, f64::from(id))?;
    call_method(c, stream, "uncork", &[])?;
    emit(c, stream, "ready", &[]).map(drop)
}

pub(super) fn set_pending_request(c: &mut Context<'_>, stream: RootId, request: PendingRequest) -> Result<(), RootedError> {
    let Some(key) = handle_of(c, stream)? else { return Ok(()) };
    with_state(c, |state| {
        if let Some(handle) = state.streams.get_mut(&key) {
            handle.request = Some(request);
        }
    });
    Ok(())
}

pub(super) fn take_pending_request(c: &mut Context<'_>, stream: RootId) -> Result<Option<PendingRequest>, RootedError> {
    let Some(key) = handle_of(c, stream)? else { return Ok(None) };
    Ok(with_state(c, |state| state.streams.get_mut(&key).and_then(|handle| handle.request.take())))
}

fn sent_headers(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let sent = get_slot(c, stream, Slot::SentHeaders)?;
    let raw = get_slot(c, stream, Slot::RawHeaders)?;
    if c.truthy_rooted(sent)? || !c.truthy_rooted(raw)? {
        return Ok(sent);
    }
    let object = c.null_object_rooted()?;
    let values = c.array_like_values_rooted(raw)?;
    for pair in values.chunks(headers::PAIR) {
        let (key, value) = (pair[0], pair.get(1).copied().unwrap_or_else(|| c.undefined()));
        let existing = c.get_property_rooted(object, key)?;
        if is_undefined(c, existing) {
            c.set_property_rooted(object, key, value, object)?;
        } else if c.is_array_rooted(existing)? {
            call_method(c, existing, "push", &[value])?;
        } else {
            let both = c.array_rooted(&[existing, value])?;
            c.set_property_rooted(object, key, both, object)?;
        }
    }
    let key = class(c, Class::SensitiveHeaders)?;
    let sensitive = c.get_property_rooted(raw, key)?;
    if !is_undefined(c, sensitive) {
        c.set_property_rooted(object, key, sensitive, object)?;
    }
    put_slot(c, stream, Slot::SentHeaders, object)?;
    Ok(object)
}

fn stream_state(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let object = c.object_rooted()?;
    if destroyed(c, stream)? || pending(c, stream)? {
        return Ok(object);
    }
    let Some(state) = with_connection(c, stream, |connection, id| connection.stream_state(id))? else { return Ok(object) };
    put_number(c, object, "state", f64::from(state.phase as u8))?;
    put_number(c, object, "weight", DEFAULT_WEIGHT)?;
    put_number(c, object, "sumDependencyWeight", 0.0)?;
    put_number(c, object, "localClose", f64::from(u8::from(state.local_close)))?;
    put_number(c, object, "remoteClose", f64::from(u8::from(state.remote_close)))?;
    put_number(c, object, "localWindowSize", state.local_window_size)?;
    Ok(object)
}

/// The octets of a written chunk: a byte view, or a string in its encoding.
fn chunk_bytes(c: &mut Context<'_>, chunk: RootId, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    if let Some(bytes) = c.view_bytes_rooted(chunk) {
        return Ok(bytes);
    }
    let encoding = if is_undefined(c, encoding) || c.string_text(encoding)?.as_deref() == Some("buffer") { None } else { Some(encoding) };
    crate::modules::buffer_shared_vm::encode_bytes(c, chunk, encoding)
}

/// `kWriteGeneric(writev, data, encoding, cb)`: queue the chunks, completing `callback`
/// once all are framed.
fn write(c: &mut Context<'_>, stream: RootId, chunks: &[(RootId, RootId)], callback: RootId) -> Result<(), RootedError> {
    if pending(c, stream)? {
        let mut entries = Vec::with_capacity(chunks.len());
        for &(chunk, encoding) in chunks {
            let entry = c.object_rooted()?;
            put(c, entry, "chunk", chunk)?;
            put(c, entry, "encoding", encoding)?;
            entries.push(entry);
        }
        let entries = c.array_rooted(&entries)?;
        let retry = closure(c, Member::WriteWhenReady, &[stream, entries, callback])?;
        return once(c, stream, "ready", retry);
    }
    if destroyed(c, stream)? {
        return Ok(());
    }
    update_timer(c, stream)?;
    if !has_flag(c, stream, stream_flags::HEADERS_SENT)? {
        call_method(c, stream, "respond", &[])?;
    }
    let mut payloads = Vec::with_capacity(chunks.len());
    for &(chunk, encoding) in chunks {
        payloads.push(chunk_bytes(c, chunk, encoding)?);
    }
    let Some(key) = handle_of(c, stream)? else { return Ok(()) };
    let retained = c.retain(callback)?;
    let count = payloads.len();
    with_state(c, |state| {
        if let Some(handle) = state.streams.get_mut(&key) {
            handle.writes.push_back((retained, count));
        }
    });
    if count == 0 {
        return write_done_count(c, stream, 0);
    }
    for payload in payloads {
        with_connection(c, stream, |connection, id| connection.write(id, payload))?;
    }
    drive(c, stream)
}

/// A chunk of the stream was framed: complete the write it belongs to once all its chunks are.
pub(super) fn write_done(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    write_done_count(c, stream, 1)
}

fn write_done_count(c: &mut Context<'_>, stream: RootId, framed: usize) -> Result<(), RootedError> {
    let Some(key) = handle_of(c, stream)? else { return Ok(()) };
    let completed = with_state(c, |state| {
        let handle = state.streams.get_mut(&key)?;
        let front = handle.writes.front_mut()?;
        front.1 = front.1.saturating_sub(framed);
        if front.1 == 0 { handle.writes.pop_front().map(|(callback, _)| callback) } else { None }
    });
    let Some(callback) = completed else { return Ok(()) };
    let callback = session::release_to_local(c, callback);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[]).map(drop)
}

/// `_final(cb)`.
fn finalize(c: &mut Context<'_>, stream: RootId, callback: RootId) -> Result<(), RootedError> {
    if pending(c, stream)? {
        let retry = closure(c, Member::FinalWhenReady, &[stream, callback])?;
        return once(c, stream, "ready", retry);
    }
    shutdown_writable(c, stream, callback)
}

/// `shutdownWritable(callback)`.
fn shutdown_writable(c: &mut Context<'_>, stream: RootId, callback: RootId) -> Result<(), RootedError> {
    let undefined = c.undefined();
    if live(c, stream)?.is_none() || slot_truthy(c, stream, Slot::ShutdownWritableCalled)? {
        return c.call_rooted(callback, undefined, &[]).map(drop);
    }
    put_slot_bool(c, stream, Slot::ShutdownWritableCalled, true)?;
    with_connection(c, stream, |connection, id| connection.shutdown(id))?;
    let maybe_destroy = class(c, Class::StreamMaybeDestroy)?;
    on(c, stream, "finish", maybe_destroy)?;
    c.call_rooted(callback, undefined, &[])?;
    drive(c, stream)
}

/// `onStreamTrailers()`.
pub(super) fn on_trailers(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    put_slot_bool(c, stream, Slot::TrailersReady, true)?;
    if destroyed(c, stream)? || closed(c, stream)? {
        return Ok(());
    }
    if !emit(c, stream, "wantTrailers", &[])? {
        let empty = c.object_rooted()?;
        call_method(c, stream, "sendTrailers", &[empty])?;
    }
    Ok(())
}

/// `stream.sendTrailers(headers)`.
fn send_trailers(c: &mut Context<'_>, stream: RootId, given: RootId) -> Result<(), RootedError> {
    if destroyed(c, stream)? || closed(c, stream)? {
        return Err(super::throw(c, Fault::InvalidStream, &[]));
    }
    let sent = get_slot(c, stream, Slot::SentTrailers)?;
    if c.truthy_rooted(sent)? {
        return Err(super::throw(c, Fault::TrailersAlreadySent, &[]));
    }
    if !slot_truthy(c, stream, Slot::TrailersReady)? {
        return Err(super::throw(c, Fault::TrailersNotReady, &[]));
    }
    super::assert_is_object(c, given, "headers", "Object")?;
    let headers = headers::null_copy(c, given)?;
    update_timer(c, stream)?;
    headers::fields(c, headers, Pseudo::Trailer)?;
    put_slot(c, stream, Slot::SentTrailers, headers)?;
    let finish = closure(c, Member::FinishSendTrailers, &[stream, headers])?;
    set_immediate(c, finish, &[])
}

/// `finishSendTrailers(stream, headersList)`.
fn finish_send_trailers(c: &mut Context<'_>, stream: RootId, headers: RootId) -> Result<(), RootedError> {
    if destroyed(c, stream)? {
        return Ok(());
    }
    set_flag(c, stream, stream_flags::HAS_TRAILERS, false)?;
    let fields = headers::fields(c, headers, Pseudo::Trailer)?;
    with_connection(c, stream, |connection, id| connection.submit_trailers(id, &fields))?;
    drive(c, stream)?;
    maybe_destroy(c, stream, ErrorCode::NoError as u32)
}

/// `closeStream(stream, code, rstStreamStatus)`.
fn close_stream(c: &mut Context<'_>, stream: RootId, code: u32, policy: RstPolicy) -> Result<(), RootedError> {
    set_flag(c, stream, stream_flags::CLOSED, true)?;
    put_slot_number(c, stream, Slot::RstCode, f64::from(code))?;
    let zero = c.number(0.0);
    call_method(c, stream, "setTimeout", &[zero])?;
    let timeout = c.string_rooted("timeout");
    call_method(c, stream, "removeAllListeners", &[timeout])?;
    let writable_state = get(c, stream, "_writableState")?;
    let ending = truthy_member(c, writable_state, "ending")?;
    if !ending {
        if !has_flag(c, stream, stream_flags::ABORTED)? {
            set_flag(c, stream, stream_flags::ABORTED, true)?;
            emit(c, stream, "aborted", &[])?;
        }
        call_method(c, stream, "end", &[])?;
    }
    if policy != RstPolicy::None {
        let finished = truthy_member(c, stream, "writableFinished")?;
        if !ending || finished || code != ErrorCode::NoError as u32 || policy == RstPolicy::Force {
            finish_close(c, stream, code)?;
        } else {
            let code = c.number(f64::from(code));
            let finish = closure(c, Member::FinishClose, &[stream, code])?;
            once(c, stream, "finish", finish)?;
        }
    }
    Ok(())
}

/// `finishCloseStream(code)`.
fn finish_close(c: &mut Context<'_>, stream: RootId, code: u32) -> Result<(), RootedError> {
    if pending(c, stream)? {
        let null = c.null();
        call_method(c, stream, "push", &[null])?;
        let code = c.number(f64::from(code));
        let submit = closure(c, Member::SubmitRst, &[stream, code])?;
        return once(c, stream, "ready", submit);
    }
    submit_rst(c, stream, code)
}

fn submit_rst(c: &mut Context<'_>, stream: RootId, code: u32) -> Result<(), RootedError> {
    let Some((session, _)) = live(c, stream)? else { return Ok(()) };
    with_connection(c, stream, |connection, id| connection.rst_stream(id, code))?;
    session::drive(c, session)
}

/// `onStreamClose(code)`.
pub(super) fn on_close(c: &mut Context<'_>, stream: RootId, code: u32) -> Result<(), RootedError> {
    if destroyed(c, stream)? {
        return Ok(());
    }
    if !closed(c, stream)? {
        close_stream(c, stream, code, RstPolicy::None)?;
    }
    if !truthy_member(c, stream, "readable")? || code != ErrorCode::NoError as u32 {
        return call_method(c, stream, "destroy", &[]).map(drop);
    }
    let maybe_destroy = class(c, Class::StreamMaybeDestroy)?;
    on(c, stream, "end", maybe_destroy)?;
    let null = c.null();
    call_method(c, stream, "push", &[null])?;
    let flowing = get(c, stream, "readableFlowing")?;
    if is_server(c, stream)? && !slot_truthy(c, stream, Slot::DidRead)? && crate::modules::stream_shared_vm::is_null(c, flowing) {
        call_method(c, stream, "resume", &[])?;
    } else {
        let zero = c.number(0.0);
        call_method(c, stream, "read", &[zero])?;
    }
    Ok(())
}

/// `_destroy(err, callback)`.
fn destroy_stream(c: &mut Context<'_>, stream: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    let session = get_slot(c, stream, Slot::Session)?;
    let has_session = c.is_object_rooted(session)?;
    let session_code = if has_session {
        let goaway = get_slot(c, session, Slot::GoawayCode)?;
        if c.truthy_rooted(goaway)? { goaway } else { get_slot(c, session, Slot::DestroyCode)? }
    } else {
        c.number(0.0)
    };
    let session_code = c.rooted_value(session_code).and_then(|value| value.as_number()).unwrap_or(0.0) as u32;
    let mut code = if closed(c, stream)? { slot_number(c, stream, Slot::RstCode)? as u32 } else { session_code };
    let mut error = error;
    if !is_nullish(c, error) {
        code = if session_code != 0 {
            session_code
        } else if is_abort_error(c, error)? {
            ErrorCode::Cancel as u32
        } else {
            ErrorCode::InternalError as u32
        };
    }
    let has_handle = live(c, stream)?.is_some();
    if !closed(c, stream)? {
        close_stream(c, stream, code, if has_handle { RstPolicy::Force } else { RstPolicy::None })?;
    }
    let null = c.null();
    call_method(c, stream, "push", &[null])?;
    release_handle(c, stream)?;
    if is_nullish(c, error) && code != ErrorCode::NoError as u32 && code != ErrorCode::Cancel as u32 {
        let name = super::frame::ErrorCode::name_of(code).map_or_else(|| code.to_string(), str::to_owned);
        error = Fault::StreamError.error(c, &[&name])?;
    }
    let undefined = c.undefined();
    put_slot(c, stream, Slot::Session, undefined)?;
    put_slot(c, stream, Slot::Handle, undefined)?;
    if has_session {
        let later = session::maybe_destroy_closure(c, session)?;
        set_immediate(c, later, &[])?;
    }
    let error = if is_nullish(c, error) { undefined } else { error };
    c.call_rooted(callback, undefined, &[error]).map(drop)
}

fn is_abort_error(c: &mut Context<'_>, error: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(error)? {
        return Ok(false);
    }
    let name = get(c, error, "name")?;
    Ok(c.string_text(name)?.as_deref() == Some("AbortError"))
}

/// `handle.destroy()`: forget the stream's host side.
fn release_handle(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let Some(key) = handle_of(c, stream)? else { return Ok(()) };
    let released = with_state(c, |state| {
        let handle = state.streams.remove(&key)?;
        if let Some(session) = state.sessions.get_mut(&handle.session) {
            session.pending.retain(|pending| *pending != key);
            if let Some(id) = handle.id {
                session.streams.remove(&id);
            }
        }
        Some(handle)
    });
    if let Some(handle) = released {
        c.release_root(handle.object);
        for (callback, _) in handle.writes {
            c.release_root(callback);
        }
    }
    Ok(())
}

/// `stream[kMaybeDestroy](code)`.
fn maybe_destroy(c: &mut Context<'_>, stream: RootId, code: u32) -> Result<(), RootedError> {
    if code != ErrorCode::NoError as u32 {
        return call_method(c, stream, "destroy", &[]).map(drop);
    }
    if !truthy_member(c, stream, "writableFinished")? {
        return Ok(());
    }
    if !truthy_member(c, stream, "readable")? && closed(c, stream)? {
        return call_method(c, stream, "destroy", &[]).map(drop);
    }
    let flowing = get(c, stream, "readableFlowing")?;
    if has_flag(c, stream, stream_flags::HEADERS_SENT)?
        && is_server(c, stream)?
        && !has_flag(c, stream, stream_flags::HAS_TRAILERS)?
        && !slot_truthy(c, stream, Slot::DidRead)?
        && crate::modules::stream_shared_vm::is_null(c, flowing)
    {
        let close = closure(c, Member::CallStreamClose, &[stream])?;
        set_immediate(c, close, &[])?;
    }
    Ok(())
}

fn push_allowed(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    if destroyed(c, stream)? || closed(c, stream)? {
        return Ok(false);
    }
    let Some(session) = session_of(c, stream)? else { return Ok(false) };
    if session::closed(c, session)? || session::destroyed(c, session)? {
        return Ok(false);
    }
    let settings = get(c, session, "remoteSettings")?;
    truthy_member(c, settings, "enablePush")
}

/// `stream.respond(headers, options)`.
fn respond(c: &mut Context<'_>, stream: RootId, given: RootId, options: RootId) -> Result<(), RootedError> {
    if destroyed(c, stream)? || closed(c, stream)? {
        return Err(super::throw(c, Fault::InvalidStream, &[]));
    }
    if has_flag(c, stream, stream_flags::HEADERS_SENT)? {
        return Err(super::throw(c, Fault::HeadersSent, &[]));
    }
    super::assert_is_object(c, options, "options", "Object")?;
    let options = super::spread(c, options)?;
    update_timer(c, stream)?;
    let mut end_stream = truthy_member(c, options, "endStream")?;
    let want_trailers = truthy_member(c, options, "waitForTrailers")?;
    if want_trailers {
        set_flag(c, stream, stream_flags::HAS_TRAILERS, true)?;
    }
    let head = headers::response(c, given, options)?;
    let slot = if c.is_array_rooted(head.headers)? { Slot::RawHeaders } else { Slot::SentHeaders };
    put_slot(c, stream, slot, head.headers)?;
    set_flag(c, stream, stream_flags::HEADERS_SENT, true)?;
    if end_stream || NO_PAYLOAD_STATUSES.contains(&head.status) || has_flag(c, stream, stream_flags::HEAD_REQUEST)? {
        end_stream = true;
        call_method(c, stream, "end", &[])?;
    }
    with_connection(c, stream, |connection, id| {
        let submitted = connection.submit_headers(id, &head.fields, end_stream);
        if want_trailers && !end_stream {
            connection.want_trailers(id);
        }
        submitted
    })?;
    drive(c, stream)
}


/// `stream.additionalHeaders(headers)`.
fn additional_headers(c: &mut Context<'_>, stream: RootId, given: RootId) -> Result<(), RootedError> {
    if destroyed(c, stream)? || closed(c, stream)? {
        return Err(super::throw(c, Fault::InvalidStream, &[]));
    }
    if has_flag(c, stream, stream_flags::HEADERS_SENT)? {
        return Err(super::throw(c, Fault::HeadersAfterRespond, &[]));
    }
    super::assert_is_object(c, given, "headers", "Object")?;
    let headers = headers::null_copy(c, given)?;
    let status = get(c, headers, headers::STATUS)?;
    if !is_nullish(c, status) {
        let code = f64::from(c.to_number(status)? as i32);
        let code_value = c.number(code);
        put(c, headers, headers::STATUS, code_value)?;
        if code == STATUS_SWITCHING_PROTOCOLS {
            return Err(super::throw(c, Fault::Status101, &[]));
        }
        if !INFORMATIONAL.contains(&code) {
            let shown = c.to_string(code_value)?;
            return Err(super::throw(c, Fault::InvalidInfoStatus, &[&shown]));
        }
    }
    update_timer(c, stream)?;
    let fields = headers::fields(c, headers, Pseudo::Response)?;
    let info = get_slot(c, stream, Slot::InfoHeaders)?;
    if c.truthy_rooted(info)? {
        call_method(c, info, "push", &[headers])?;
    } else {
        let info = c.array_rooted(&[headers])?;
        put_slot(c, stream, Slot::InfoHeaders, info)?;
    }
    with_connection(c, stream, |connection, id| connection.submit_headers(id, &fields, false))?;
    drive(c, stream)
}

/// `stream.pushStream(headers, options, callback)`.
fn push_stream(c: &mut Context<'_>, stream: RootId, given: RootId, options: RootId, callback: RootId) -> Result<(), RootedError> {
    if !push_allowed(c, stream)? {
        return Err(super::throw(c, Fault::PushDisabled, &[]));
    }
    let id = slot_number(c, stream, Slot::Id)?;
    if super::connection::server_initiated(id as u32) {
        return Err(super::throw(c, Fault::NestedPush, &[]));
    }
    let Some(session) = session_of(c, stream)? else { return Ok(()) };
    update_timer(c, stream)?;
    let (options, callback) = if c.is_callable_rooted(options)? { (c.undefined(), options) } else { (options, callback) };
    super::validate_function(c, callback, "callback")?;
    super::assert_is_object(c, options, "options", "Object")?;
    let options = super::spread(c, options)?;
    let end_stream = truthy_member(c, options, "endStream")?;
    put_bool(c, options, "endStream", end_stream)?;
    super::assert_is_object(c, given, "headers", "Object")?;
    let headers = headers::null_copy(c, given)?;
    let defaults = [
        (headers::METHOD, None),
        (headers::AUTHORITY, Some(Slot::Authority)),
        (headers::SCHEME, Some(Slot::Protocol)),
        (headers::PATH, None),
    ];
    for (name, slot) in defaults {
        let value = if name == headers::AUTHORITY { headers::authority(c, headers)? } else { get(c, headers, name)? };
        if !is_undefined(c, value) {
            continue;
        }
        let default = match (name, slot) {
            (_, Some(slot)) => get_slot(c, stream, slot)?,
            (headers::METHOD, None) => c.string_rooted(headers::METHOD_GET),
            _ => c.string_rooted("/"),
        };
        put(c, headers, name, default)?;
    }
    let method = get(c, headers, headers::METHOD)?;
    let head_request = c.string_text(method)?.as_deref() == Some(headers::METHOD_HEAD);
    let end_stream = end_stream || head_request;
    if head_request {
        put_bool(c, options, "endStream", true)?;
    }
    let fields = headers::fields(c, headers, Pseudo::Request)?;
    let promised = with_connection(c, stream, |connection, id| connection.submit_push_promise(id, &fields))?;
    let promised = match promised {
        Some(Ok(promised)) => promised,
        Some(Err(refusal)) => {
            let fault = match refusal {
                super::connection::Refusal::OutOfStreams => Fault::OutOfStreams,
                _ => Fault::InvalidStream,
            };
            let error = fault.error(c, &[])?;
            return next_tick(c, callback, &[error]);
        }
        None => return Ok(()),
    };
    let class = class(c, Class::ServerHttp2Stream)?;
    let undefined = c.undefined();
    let id_value = c.number(f64::from(promised));
    let pushed = c.construct_rooted(class, class, &[session, undefined, id_value, options, headers])?;
    put_slot(c, pushed, Slot::SentHeaders, headers)?;
    let null = c.null();
    call_method(c, pushed, "push", &[null])?;
    if end_stream {
        call_method(c, pushed, "end", &[])?;
    }
    if head_request {
        set_flag(c, pushed, stream_flags::HEAD_REQUEST, true)?;
    }
    let zero = c.number(0.0);
    next_tick(c, callback, &[null, pushed, headers, zero])?;
    drive(c, stream)
}

/// The checks `respondWithFD` and `respondWithFile` share; the prepared headers.
fn file_response_head(c: &mut Context<'_>, stream: RootId, given: RootId, options: RootId) -> Result<(RootId, RootId), RootedError> {
    if destroyed(c, stream)? || closed(c, stream)? {
        return Err(super::throw(c, Fault::InvalidStream, &[]));
    }
    if has_flag(c, stream, stream_flags::HEADERS_SENT)? {
        return Err(super::throw(c, Fault::HeadersSent, &[]));
    }
    super::assert_is_object(c, options, "options", "Object")?;
    let options = super::spread(c, options)?;
    for (name, label) in [("offset", "options.offset"), ("length", "options.length")] {
        let value = get(c, options, name)?;
        if !is_undefined(c, value) && c.rooted_value(value).and_then(|value| value.as_number()).is_none() {
            return Err(crate::modules::util_shared_vm::argument_value_error(c, value, label, "is invalid")?);
        }
    }
    let stat_check = get(c, options, "statCheck")?;
    if !is_undefined(c, stat_check) && !c.is_callable_rooted(stat_check)? {
        return Err(crate::modules::util_shared_vm::argument_value_error(c, stat_check, "options.statCheck", "is invalid")?);
    }
    if truthy_member(c, options, "waitForTrailers")? {
        set_flag(c, stream, stream_flags::HAS_TRAILERS, true)?;
    }
    update_timer(c, stream)?;
    let (headers, status) = headers::response_object(c, given, options)?;
    if NO_PAYLOAD_STATUSES.contains(&status) || has_flag(c, stream, stream_flags::HEAD_REQUEST)? {
        return Err(super::throw(c, Fault::PayloadForbidden, &[&super::number_text(status)]));
    }
    Ok((headers, options))
}

fn fs_module(c: &mut Context<'_>) -> Outcome {
    crate::host::shared_vm::commonjs::fs_module(c)
}

/// `stream.respondWithFD(fd, headers, options)`.
fn respond_with_fd(c: &mut Context<'_>, stream: RootId, fd: RootId, given: RootId, options: RootId) -> Result<(), RootedError> {
    let (headers, options) = file_response_head(c, stream, given, options)?;
    let fd = if c.is_object_rooted(fd)? { get(c, fd, "fd")? } else { fd };
    if c.rooted_value(fd).and_then(|value| value.as_number()).is_none() {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(
            c,
            fd,
            "fd",
            &super::types_expectation("number,FileHandle"),
        )?);
    }
    put_slot_bool(c, stream, Slot::OwnsFd, false)?;
    let stat_check = get(c, options, "statCheck")?;
    if !is_undefined(c, stat_check) {
        let fs = fs_module(c)?;
        let after = closure(c, Member::FileStat, &[stream, options, fd, headers])?;
        call_method(c, fs, "fstat", &[fd, after])?;
        return Ok(());
    }
    let offset = get(c, options, "offset")?;
    let length = get(c, options, "length")?;
    send_fd(c, stream, fd, headers, offset, length)
}

/// `stream.respondWithFile(path, headers, options)`.
fn respond_with_file(c: &mut Context<'_>, stream: RootId, path: RootId, given: RootId, options: RootId) -> Result<(), RootedError> {
    let (headers, options) = file_response_head(c, stream, given, options)?;
    put_slot_bool(c, stream, Slot::OwnsFd, true)?;
    let fs = fs_module(c)?;
    let yes = c.boolean(true);
    let opened = closure(c, Member::FileOpened, &[stream, options, headers, yes])?;
    let mode = c.string_rooted("r");
    call_method(c, fs, "open", &[path, mode, opened]).map(drop)
}

/// A failure of a file response: `options.onError(error)`, else the stream's destruction.
fn file_failed(c: &mut Context<'_>, stream: RootId, options: RootId, error: RootId) -> Result<(), RootedError> {
    let on_error = get(c, options, "onError")?;
    if c.is_callable_rooted(on_error)? {
        let undefined = c.undefined();
        return c.call_rooted(on_error, undefined, &[error]).map(drop);
    }
    call_method(c, stream, "destroy", &[error]).map(drop)
}

fn close_fd(c: &mut Context<'_>, fd: RootId) -> Result<(), RootedError> {
    let fs = fs_module(c)?;
    // Like Node's `tryClose`, a failure to close is not the response's failure.
    let _closed = crate::modules::stream_shared_vm::attempt(call_method(c, fs, "closeSync", &[fd]))?;
    Ok(())
}

/// `afterOpen(session, options, headers, streamOptions, err, fd)`.
fn file_opened(c: &mut Context<'_>, stream: RootId, options: RootId, headers: RootId, _owned: RootId, error: RootId, fd: RootId) -> Result<(), RootedError> {
    if c.truthy_rooted(error)? {
        return file_failed(c, stream, options, error);
    }
    if destroyed(c, stream)? || closed(c, stream)? {
        return close_fd(c, fd);
    }
    let fs = fs_module(c)?;
    let after = closure(c, Member::FileStat, &[stream, options, fd, headers])?;
    call_method(c, fs, "fstat", &[fd, after]).map(drop)
}

/// `doSendFD` and `doSendFileFD`: check the descriptor's stat, then respond with it.
fn file_stat(c: &mut Context<'_>, stream: RootId, options: RootId, fd: RootId, headers: RootId, error: RootId, stat: RootId) -> Result<(), RootedError> {
    let owns = slot_truthy(c, stream, Slot::OwnsFd)?;
    if c.truthy_rooted(error)? {
        if owns {
            close_fd(c, fd)?;
            return file_failed(c, stream, options, error);
        }
        return call_method(c, stream, "destroy", &[error]).map(drop);
    }
    let offset = get(c, options, "offset")?;
    let length = get(c, options, "length")?;
    let is_file = call_method(c, stat, "isFile", &[])?;
    let is_file = c.truthy_rooted(is_file)?;
    if owns && !is_file {
        let directory = call_method(c, stat, "isDirectory", &[])?;
        let directory = c.truthy_rooted(directory)?;
        if !is_undefined(c, offset) || !is_undefined(c, length) || directory {
            let fault = if directory { Fault::SendFile } else { Fault::SendFileNoSeek };
            let failure = fault.error(c, &[])?;
            close_fd(c, fd)?;
            return file_failed(c, stream, options, failure);
        }
    }
    if destroyed(c, stream)? || closed(c, stream)? {
        if owns {
            close_fd(c, fd)?;
        }
        let failure = Fault::InvalidStream.error(c, &[])?;
        return call_method(c, stream, "destroy", &[failure]).map(drop);
    }
    let stat_options = c.object_rooted()?;
    let offset_value = if is_undefined(c, offset) { c.number(0.0) } else { offset };
    put(c, stat_options, "offset", offset_value)?;
    let length_value = if is_undefined(c, length) { c.number(-1.0) } else { length };
    put(c, stat_options, "length", length_value)?;
    let stat_check = get(c, options, "statCheck")?;
    let mut cancelled = false;
    if c.is_callable_rooted(stat_check)? {
        let args: Vec<RootId> = if owns { vec![stat, headers] } else { vec![stat, headers, stat_options] };
        let result = c.call_rooted(stat_check, stream, &args)?;
        cancelled = c.rooted_value(result).and_then(|value| value.as_bool()) == Some(false);
    }
    if cancelled || has_flag(c, stream, stream_flags::HEADERS_SENT)? {
        if owns {
            close_fd(c, fd)?;
        }
        return Ok(());
    }
    let start = get(c, stat_options, "offset")?;
    let mut span = get(c, stat_options, "length")?;
    if is_file {
        let size = crate::modules::stream_shared_vm::number(c, stat, "size")?;
        let from = c.to_number(start)?;
        let requested = c.to_number(span)?;
        let available = size - from;
        let length = if requested < 0.0 { available } else { available.min(requested) };
        span = c.number(length);
        if owns {
            put(c, headers, "content-length", span)?;
        }
    }
    let start = if owns { offset_value } else { start };
    send_fd(c, stream, fd, headers, start, span)
}

/// `processRespondWithFD(self, fd, headers, offset, length, streamOptions)`: respond, then
/// send the descriptor's bytes from `offset` for `length` (all when negative).
fn send_fd(c: &mut Context<'_>, stream: RootId, fd: RootId, headers: RootId, offset: RootId, length: RootId) -> Result<(), RootedError> {
    set_flag(c, stream, stream_flags::HEADERS_SENT, true)?;
    let fields = match crate::modules::stream_shared_vm::attempt(headers::fields(c, headers, Pseudo::Response))? {
        Ok(fields) => fields,
        Err(error) => {
            if slot_truthy(c, stream, Slot::OwnsFd)? {
                close_fd(c, fd)?;
            }
            return call_method(c, stream, "destroy", &[error]).map(drop);
        }
    };
    put_slot(c, stream, Slot::SentHeaders, headers)?;
    let null = c.null();
    put(c, stream, "_final", null)?;
    call_method(c, stream, "end", &[])?;
    let want_trailers = has_flag(c, stream, stream_flags::HAS_TRAILERS)?;
    with_connection(c, stream, |connection, id| {
        let submitted = connection.submit_headers(id, &fields, false);
        if want_trailers {
            connection.want_trailers(id);
        }
        submitted
    })?;
    let start = if is_undefined(c, offset) { 0.0 } else { c.to_number(offset)?.max(0.0) };
    let limit = if is_undefined(c, length) { -1.0 } else { c.to_number(length)? };
    let fs = fs_module(c)?;
    let mut position = start;
    let mut remaining = if limit < 0.0 { f64::INFINITY } else { limit };
    while remaining > 0.0 {
        let size = (FILE_CHUNK as f64).min(remaining) as usize;
        let chunk = crate::modules::buffer_shared_vm::from_vec(c, vec![0; size])?;
        let zero = c.number(0.0);
        let size_value = c.number(size as f64);
        let position_value = c.number(position);
        let read = call_method(c, fs, "readSync", &[fd, chunk, zero, size_value, position_value])?;
        let read = c.to_number(read)? as usize;
        if read == 0 {
            break;
        }
        let mut bytes = c.view_bytes_rooted(chunk).unwrap_or_default();
        bytes.truncate(read);
        with_connection(c, stream, |connection, id| connection.write(id, bytes))?;
        position += read as f64;
        remaining -= read as f64;
    }
    if slot_truthy(c, stream, Slot::OwnsFd)? {
        close_fd(c, fd)?;
    }
    with_connection(c, stream, |connection, id| connection.shutdown(id))?;
    drive(c, stream)
}
