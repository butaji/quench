//! `Http2Session`, `ServerHttp2Session` and `ClientHttp2Session` (lib/internal/http2/core.js):
//! an EventEmitter bound to one socket, whose bytes feed the session's `Connection` and whose
//! events `drive` delivers as Node's session and stream callbacks do.

use super::connection::{Category, Connection, Event, Refusal, Role};
use super::faults::{Fault, NghttpCode};
use super::frame::{self, flags, ErrorCode};
use super::hpack::Field;
use super::stream::{self, stream_flags};
use super::{
    buffer, class, closure, emit_next_tick, get_slot, handle_of, has_flag, headers, put_slot, put_slot_bool,
    put_slot_number, set_flag, set_immediate, slot_number, slot_truthy, with_state, Class, SessionHandle, Slot,
};
use crate::modules::host_members::{accessors, captured, constructor, current, members};
use crate::modules::stream_shared_vm::{
    attempt, call_method, emit, get, is_nullish, is_undefined, next_tick, on, once, put, truthy_member, Context, Outcome,
};
use quench_runtime::{RootId, RootedError};

/// Node's `NGHTTP2_SESSION_SERVER` and `NGHTTP2_SESSION_CLIENT`, a session's `type`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionType {
    Server = 0,
    Client = 1,
}

impl SessionType {
    fn role(self) -> Role {
        match self {
            Self::Server => Role::Server,
            Self::Client => Role::Client,
        }
    }
}

/// A session's `kState.flags`.
pub(super) mod session_flags {
    pub(in super::super) const READY: u32 = 0x1;
    pub(in super::super) const CLOSED: u32 = 0x2;
    pub(in super::super) const DESTROYED: u32 = 0x4;
}

/// A request head waiting for its session to connect.
pub(crate) struct PendingRequest {
    pub(super) fields: Vec<Field>,
    pub(super) end_stream: bool,
    pub(super) want_trailers: bool,
}

/// The ports `connect` defaults to by scheme.
const HTTP_PORT: &str = "80";
const HTTPS_PORT: &str = "443";
const HTTP_PROTOCOL: &str = "http:";
const HTTPS_PROTOCOL: &str = "https:";
/// The ALPN identifier of HTTP/2 over cleartext.
const H2C: &str = "h2c";
/// The informational statuses, which a response's `headers` event carries.
const INFORMATIONAL: std::ops::Range<f64> = 100.0..200.0;
/// The largest stream id `setNextStreamID` accepts.
const MAX_STREAMS: f64 = u32::MAX as f64;
/// ALTSVC values and origins share the frame's payload limit (`kMaxALTSVC`).
const MAX_ALTSVC: usize = (1 << 14) - 2;

members!(Member, "http2Session", [
    Construct,
    ServerConstruct,
    ClientConstruct,
    Encrypted,
    AlpnProtocol,
    OriginSet,
    Connecting,
    Closed,
    Destroyed,
    Socket,
    Type,
    GoawayCode,
    GoawayLastStreamId,
    PendingSettingsAck,
    State,
    LocalSettings,
    RemoteSettings,
    Server,
    SetNextStreamId,
    SetLocalWindowSize,
    Ping,
    Settings,
    Goaway,
    Destroy,
    Close,
    Ref,
    Unref,
    SetTimeout,
    OnTimeout,
    CaptureRejection,
    Altsvc,
    Origin,
    Request,
    /// `socketOnError` and `socketOnClose`, on the bound socket.
    SocketError,
    SocketClose,
    /// The socket's data, read into the connection.
    SocketData,
    /// `setupHandle`, once the socket connects.
    Setup,
    /// `submitSettings` and `submitGoaway`, deferred until the session connects.
    SubmitSettings,
    SubmitGoaway,
    /// `requestOnConnect`, and the pending requests' shared `connect` listener.
    RequestOnConnect,
    RunPendingRequests,
    /// `emitClose(session, error)`.
    EmitClose,
    /// `finishSessionClose`'s `socket.end` callback and its deferred `socket.destroy`.
    SocketEnded,
    DestroySocket,
    /// A ping callback answered with `ERR_HTTP2_PING_CANCEL`.
    PingCancelled,
    /// The scheduled write of the session's queued frames.
    FlushWrites,
    /// The tick delivering the session's next event.
    ContinueDelivery,
    /// `session[kMaybeDestroy]()`, deferred from a stream's destruction.
    MaybeDestroy,
    /// The timer of `setTimeout`, calling the target's `_onTimeout`.
    TimerFired,
    /// A request's abort signal listener.
    RequestAborted,
]);

/// Install the session classes.
pub(super) fn build(c: &mut Context<'_>) -> Result<(), RootedError> {
    let emitter = crate::modules::stream_shared_vm::intrinsics(c)?.class(crate::modules::stream_shared_vm::Class::Emitter);
    let session = constructor(c, Member::Construct, "Http2Session")?;
    crate::modules::http_shared_vm::inherit(c, session, emitter)?;
    let prototype = get(c, session, "prototype")?;
    accessors(c, prototype, &[
        ("encrypted", Member::Encrypted, None),
        ("alpnProtocol", Member::AlpnProtocol, None),
        ("originSet", Member::OriginSet, None),
        ("connecting", Member::Connecting, None),
        ("closed", Member::Closed, None),
        ("destroyed", Member::Destroyed, None),
        ("socket", Member::Socket, None),
        ("type", Member::Type, None),
        ("goawayCode", Member::GoawayCode, None),
        ("goawayLastStreamID", Member::GoawayLastStreamId, None),
        ("pendingSettingsAck", Member::PendingSettingsAck, None),
        ("state", Member::State, None),
        ("localSettings", Member::LocalSettings, None),
        ("remoteSettings", Member::RemoteSettings, None),
    ])?;
    super::class_methods(c, prototype, &[
        ("setNextStreamID", Member::SetNextStreamId),
        ("setLocalWindowSize", Member::SetLocalWindowSize),
        ("ping", Member::Ping),
        ("settings", Member::Settings),
        ("goaway", Member::Goaway),
        ("destroy", Member::Destroy),
        ("close", Member::Close),
        ("_onTimeout", Member::OnTimeout),
        ("ref", Member::Ref),
        ("unref", Member::Unref),
    ])?;
    crate::modules::host_members::methods(c, prototype, &[("setTimeout", Member::SetTimeout)])?;
    let capture = crate::modules::stream_shared_vm::utils::registered_symbol(c, "nodejs.rejection")?;
    let rejection = closure(c, Member::CaptureRejection, &[])?;
    c.define_data_property_rooted(prototype, capture, rejection, true, false, true)?;
    super::set_class(c, Class::Http2Session, session)?;

    let server = constructor(c, Member::ServerConstruct, "ServerHttp2Session")?;
    crate::modules::http_shared_vm::inherit(c, server, session)?;
    let prototype = get(c, server, "prototype")?;
    accessors(c, prototype, &[("server", Member::Server, None)])?;
    super::class_methods(c, prototype, &[("altsvc", Member::Altsvc), ("origin", Member::Origin)])?;
    super::set_class(c, Class::ServerHttp2Session, server)?;

    let client = constructor(c, Member::ClientConstruct, "ClientHttp2Session")?;
    crate::modules::http_shared_vm::inherit(c, client, session)?;
    let prototype = get(c, client, "prototype")?;
    super::class_methods(c, prototype, &[("request", Member::Request)])?;
    super::set_class(c, Class::ClientHttp2Session, client)?;

    let socket_error = closure(c, Member::SocketError, &[])?;
    super::set_class(c, Class::SocketError, socket_error)?;
    let socket_close = closure(c, Member::SocketClose, &[])?;
    super::set_class(c, Class::SocketClose, socket_close)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::Construct => {
            let session_type = if c.to_number(at(0))? == SessionType::Server as u8 as f64 { SessionType::Server } else { SessionType::Client };
            initialize(c, this, session_type, at(1), at(2))?;
            Ok(undefined)
        }
        Member::ServerConstruct => {
            initialize(c, this, SessionType::Server, at(0), at(1))?;
            put_slot(c, this, Slot::Server, at(2))?;
            Ok(undefined)
        }
        Member::ClientConstruct => {
            initialize(c, this, SessionType::Client, at(0), at(1))?;
            let null = c.null();
            put_slot(c, this, Slot::PendingRequestCalls, null)?;
            Ok(undefined)
        }
        Member::Encrypted => get_slot(c, this, Slot::Encrypted),
        Member::AlpnProtocol => get_slot(c, this, Slot::AlpnProtocol),
        Member::OriginSet => origin_set(c, this),
        Member::Connecting => {
            let ready = has_flag(c, this, session_flags::READY)?;
            Ok(c.boolean(!ready))
        }
        Member::Closed => {
            let closed = closed(c, this)?;
            Ok(c.boolean(closed))
        }
        Member::Destroyed => {
            let destroyed = destroyed(c, this)?;
            Ok(c.boolean(destroyed))
        }
        Member::Socket => get_slot(c, this, Slot::Socket),
        Member::Type => get_slot(c, this, Slot::Type),
        Member::GoawayCode => {
            let code = get_slot(c, this, Slot::GoawayCode)?;
            if c.truthy_rooted(code)? { Ok(code) } else { Ok(c.number(f64::from(ErrorCode::NoError as u32))) }
        }
        Member::GoawayLastStreamId => {
            let last = get_slot(c, this, Slot::GoawayLastStreamId)?;
            if c.truthy_rooted(last)? { Ok(last) } else { Ok(c.number(0.0)) }
        }
        Member::PendingSettingsAck => {
            let pending = slot_number(c, this, Slot::PendingAck)? > 0.0;
            Ok(c.boolean(pending))
        }
        Member::State => session_state(c, this),
        Member::LocalSettings => settings_of(c, this, Slot::LocalSettings),
        Member::RemoteSettings => settings_of(c, this, Slot::RemoteSettings),
        Member::Server => get_slot(c, this, Slot::Server),
        Member::SetNextStreamId => {
            ensure_live(c, this)?;
            let id = super::validate_number(c, at(0), "id")?;
            if id <= 0.0 || id > MAX_STREAMS {
                let range = format!("> 0 and <= {}", super::number_text(MAX_STREAMS));
                return Err(out_of_range(c, "id", &range, at(0))?);
            }
            with_connection(c, this, |connection| connection.set_next_stream_id(id as u32))?;
            Ok(undefined)
        }
        Member::SetLocalWindowSize => {
            ensure_live(c, this)?;
            let size = super::validate_integer(c, at(0), "windowSize", 0.0, f64::from(i32::MAX))?;
            with_connection(c, this, |connection| connection.set_local_window_size(size as i64))?;
            drive_object(c, this)?;
            Ok(undefined)
        }
        Member::Ping => ping(c, this, at(0), at(1)),
        Member::Settings => {
            settings(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::Goaway => {
            goaway(c, this, args)?;
            Ok(undefined)
        }
        Member::Destroy => {
            destroy(c, this, args)?;
            Ok(undefined)
        }
        Member::Close => {
            close(c, this, at(0))?;
            Ok(undefined)
        }
        Member::Ref | Member::Unref => {
            let socket = get_slot(c, this, Slot::Socket)?;
            if c.truthy_rooted(socket)? {
                let name = if matches!(current::<Member>(c)?, Member::Ref) { "ref" } else { "unref" };
                call_method(c, socket, name, &[])?;
            }
            Ok(undefined)
        }
        Member::SetTimeout => set_timeout(c, this, at(0), at(1)),
        Member::OnTimeout => {
            if !destroyed(c, this)? {
                emit(c, this, "timeout", &[])?;
            }
            Ok(undefined)
        }
        Member::CaptureRejection => {
            let (error, event) = (at(0), at(1));
            if c.string_text(event)?.as_deref() == Some("stream") {
                call_method(c, at(2), "destroy", &[error])?;
            } else {
                call_method(c, this, "destroy", &[error])?;
            }
            Ok(undefined)
        }
        Member::Altsvc => {
            altsvc(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Member::Origin => {
            origin(c, this, args)?;
            Ok(undefined)
        }
        Member::Request => request(c, this, at(0), at(1)),
        Member::SocketError => {
            socket_error(c, this, at(0))?;
            Ok(undefined)
        }
        Member::SocketClose => {
            socket_close(c, this)?;
            Ok(undefined)
        }
        Member::SocketData => {
            let [session] = captured(c)?;
            receive(c, session, at(0))?;
            Ok(undefined)
        }
        Member::Setup => {
            let [session, socket, session_type, options] = captured(c)?;
            let session_type = if c.to_number(session_type)? == SessionType::Server as u8 as f64 {
                SessionType::Server
            } else {
                SessionType::Client
            };
            if let Err(exception) = attempt(setup_handle(c, session, socket, session_type, options))? {
                call_method(c, socket, "destroy", &[exception])?;
            }
            Ok(undefined)
        }
        Member::SubmitSettings => {
            let [session, settings, callback] = captured(c)?;
            submit_settings(c, session, settings, callback)?;
            Ok(undefined)
        }
        Member::SubmitGoaway => {
            let [session, code, last, data] = captured(c)?;
            submit_goaway(c, session, code, last, data)?;
            Ok(undefined)
        }
        Member::RequestOnConnect => {
            let [stream] = captured(c)?;
            request_on_connect(c, stream)?;
            Ok(undefined)
        }
        Member::RunPendingRequests => {
            let [session] = captured(c)?;
            let pending = get_slot(c, session, Slot::PendingRequestCalls)?;
            if c.truthy_rooted(pending)? {
                for call in c.array_like_values_rooted(pending)? {
                    c.call_rooted(call, undefined, &[])?;
                }
            }
            let null = c.null();
            put_slot(c, session, Slot::PendingRequestCalls, null)?;
            Ok(undefined)
        }
        Member::EmitClose => {
            let [session, error] = captured(c)?;
            if c.truthy_rooted(error)? {
                emit(c, session, "error", &[error])?;
            }
            emit(c, session, "close", &[])?;
            Ok(undefined)
        }
        Member::SocketEnded => {
            let [session, socket, error] = captured(c)?;
            if !closed(c, session)? {
                let destroy = closure(c, Member::DestroySocket, &[socket, error])?;
                set_immediate(c, destroy, &[])?;
            }
            Ok(undefined)
        }
        Member::DestroySocket => {
            let [socket, error] = captured(c)?;
            call_method(c, socket, "destroy", &[error])?;
            Ok(undefined)
        }
        Member::PingCancelled => {
            let [callback] = captured(c)?;
            let error = Fault::PingCancel.error(c, &[])?;
            c.call_rooted(callback, undefined, &[error])?;
            Ok(undefined)
        }
        Member::ContinueDelivery => {
            let [session] = captured(c)?;
            if let Some(key) = handle_of(c, session)? {
                with_state(c, |state| {
                    if let Some(handle) = state.sessions.get_mut(&key) {
                        handle.continuation_scheduled = false;
                    }
                });
                drive(c, key)?;
            }
            Ok(undefined)
        }
        Member::FlushWrites => {
            let [session] = captured(c)?;
            if let Some(key) = handle_of(c, session)? {
                flush_writes(c, key)?;
            }
            Ok(undefined)
        }
        Member::MaybeDestroy => {
            let [session] = captured(c)?;
            maybe_destroy(c, session, undefined)?;
            Ok(undefined)
        }
        Member::TimerFired => {
            let [target] = captured(c)?;
            call_method(c, target, "_onTimeout", &[])?;
            Ok(undefined)
        }
        Member::RequestAborted => {
            let [stream, signal] = captured(c)?;
            let reason = get(c, signal, "reason")?;
            let error = abort_error(c, reason)?;
            call_method(c, stream, "destroy", &[error])?;
            Ok(undefined)
        }
    }
}

fn out_of_range(c: &mut Context<'_>, name: &str, range: &str, value: RootId) -> Result<RootedError, RootedError> {
    let received = crate::modules::util_shared_vm::range_error_received(c, value)?;
    let message = format!("The value of \"{name}\" is out of range. It must be {range}. Received {received}");
    Ok(crate::modules::http_shared_vm::throw_coded(
        c,
        crate::modules::http_shared_vm::ErrorClass::RangeError,
        "ERR_OUT_OF_RANGE",
        &message,
    ))
}

/// Node's `AbortError` with `cause`.
fn abort_error(c: &mut Context<'_>, cause: RootId) -> Outcome {
    let error = c.error_rooted("The operation was aborted")?;
    let name = c.string_rooted("AbortError");
    put(c, error, "name", name)?;
    let code = c.string_rooted("ABORT_ERR");
    put(c, error, "code", code)?;
    let key = c.string_rooted("cause");
    c.define_data_property_rooted(error, key, cause, true, false, true)?;
    Ok(error)
}

pub(super) fn closed(c: &mut Context<'_>, session: RootId) -> Result<bool, RootedError> {
    has_flag(c, session, session_flags::CLOSED)
}

pub(super) fn destroyed(c: &mut Context<'_>, session: RootId) -> Result<bool, RootedError> {
    has_flag(c, session, session_flags::DESTROYED)
}

fn connecting(c: &mut Context<'_>, session: RootId) -> Result<bool, RootedError> {
    Ok(!has_flag(c, session, session_flags::READY)?)
}

fn ensure_live(c: &mut Context<'_>, session: RootId) -> Result<(), RootedError> {
    if destroyed(c, session)? {
        return Err(super::throw(c, Fault::InvalidSession, &[]));
    }
    Ok(())
}

pub(super) fn session_type(c: &mut Context<'_>, session: RootId) -> Result<SessionType, RootedError> {
    let value = slot_number(c, session, Slot::Type)?;
    Ok(if value == SessionType::Server as u8 as f64 { SessionType::Server } else { SessionType::Client })
}

/// Run `f` over the session's connection, when it has one.
pub(super) fn with_connection<T>(c: &mut Context<'_>, session: RootId, f: impl FnOnce(&mut Connection) -> T) -> Result<Option<T>, RootedError> {
    let Some(key) = handle_of(c, session)? else { return Ok(None) };
    Ok(with_state(c, |state| state.sessions.get_mut(&key).and_then(|handle| handle.connection.as_mut()).map(f)))
}

/// The objects of the session's streams with ids and of its pending streams.
pub(super) fn stream_objects(c: &mut Context<'_>, session: RootId) -> Result<(Vec<RootId>, Vec<RootId>), RootedError> {
    let Some(key) = handle_of(c, session)? else { return Ok((Vec::new(), Vec::new())) };
    Ok(with_state(c, |state| {
        let Some(handle) = state.sessions.get(&key) else { return (Vec::new(), Vec::new()) };
        let mut ids: Vec<(&u32, &u64)> = handle.streams.iter().collect();
        ids.sort();
        let open = ids.into_iter().filter_map(|(_, key)| state.streams.get(key)).map(|stream| stream.object).collect();
        let pending = handle.pending.iter().filter_map(|key| state.streams.get(key)).map(|stream| stream.object).collect();
        (open, pending)
    }))
}

/// `new Http2Session(type, options, socket)`.
fn initialize(c: &mut Context<'_>, this: RootId, session_type: SessionType, options: RootId, socket: RootId) -> Result<(), RootedError> {
    let emitter = crate::modules::stream_shared_vm::intrinsics(c)?.class(crate::modules::stream_shared_vm::Class::Emitter);
    c.call_rooted(emitter, this, &[])?;
    let bound = get_slot(c, socket, Slot::BoundSession)?;
    if !is_undefined(c, bound) {
        return Err(super::throw(c, Fault::SocketBound, &[]));
    }
    put_slot(c, socket, Slot::BoundSession, this)?;
    let socket_error = class(c, Class::SocketError)?;
    on(c, socket, "error", socket_error)?;
    let socket_close = class(c, Class::SocketClose)?;
    on(c, socket, "close", socket_close)?;
    put_slot_number(c, this, Slot::Flags, 0.0)?;
    put_slot_number(c, this, Slot::DestroyCode, f64::from(ErrorCode::NoError as u32))?;
    let null = c.null();
    put_slot(c, this, Slot::GoawayCode, null)?;
    put_slot(c, this, Slot::GoawayLastStreamId, null)?;
    put_slot_number(c, this, Slot::PendingAck, 0.0)?;
    let undefined = c.undefined();
    put_slot(c, this, Slot::Encrypted, undefined)?;
    put_slot(c, this, Slot::AlpnProtocol, undefined)?;
    put_slot_number(c, this, Slot::Type, f64::from(session_type as u8))?;
    put_slot(c, this, Slot::Socket, socket)?;
    put_slot(c, this, Slot::Timeout, null)?;
    let object = c.retain(this)?;
    let retained_socket = c.retain(socket)?;
    let key = with_state(c, |state| {
        let key = state.allocate();
        state.sessions.insert(key, SessionHandle {
            object,
            socket: retained_socket,
            connection: None,
            streams: Default::default(),
            pending: Vec::new(),
            settings_callbacks: Default::default(),
            ping_callbacks: Default::default(),
            driving: false,
            write_scheduled: false,
            deferred: Default::default(),
            continuation_scheduled: false,
        });
        key
    });
    put_slot_number(c, this, Slot::Handle, key as f64)?;
    let set_no_delay = get(c, socket, "setNoDelay")?;
    if c.is_callable_rooted(set_no_delay)? {
        c.call_rooted(set_no_delay, socket, &[])?;
    }
    let disable = get(c, socket, "disableRenegotiation")?;
    if c.is_callable_rooted(disable)? {
        c.call_rooted(disable, socket, &[])?;
    }
    let type_value = c.number(f64::from(session_type as u8));
    let setup = closure(c, Member::Setup, &[this, socket, type_value, options])?;
    if truthy_member(c, socket, "connecting")? || truthy_member(c, socket, "secureConnecting")? {
        let event = if truthy_member(c, socket, "encrypted")? { "secureConnect" } else { "connect" };
        once(c, socket, event, setup)?;
    } else {
        c.call_rooted(setup, undefined, &[])?;
    }
    Ok(())
}

/// `setupHandle(socket, type, options)`.
fn setup_handle(c: &mut Context<'_>, session: RootId, socket: RootId, session_type: SessionType, options: RootId) -> Result<(), RootedError> {
    if destroyed(c, session)? {
        return emit_next_tick(c, session, "connect", &[session, socket]);
    }
    set_flag(c, session, session_flags::READY, true)?;
    let connection = Connection::new(session_type.role());
    let Some(key) = handle_of(c, session)? else { return Ok(()) };
    with_state(c, |state| {
        if let Some(handle) = state.sessions.get_mut(&key) {
            handle.connection = Some(connection);
        }
    });
    let data = closure(c, Member::SocketData, &[session])?;
    on(c, socket, "data", data)?;
    if truthy_member(c, socket, "encrypted")? {
        let alpn = get(c, socket, "alpnProtocol")?;
        put_slot(c, session, Slot::AlpnProtocol, alpn)?;
        put_slot_bool(c, session, Slot::Encrypted, true)?;
    } else {
        let alpn = c.string_rooted(H2C);
        put_slot(c, session, Slot::AlpnProtocol, alpn)?;
        put_slot_bool(c, session, Slot::Encrypted, false)?;
    }
    let given = get(c, options, "settings")?;
    let settings = if c.is_object_rooted(given)? { given } else { c.object_rooted()? };
    call_method(c, session, "settings", &[settings])?;
    if session_type == SessionType::Server {
        let origins = get(c, options, "origins")?;
        if c.is_array_rooted(origins)? {
            let origins = c.array_like_values_rooted(origins)?;
            call_method(c, session, "origin", &origins)?;
        }
    }
    emit_next_tick(c, session, "connect", &[session, socket])
}

fn receive(c: &mut Context<'_>, session: RootId, chunk: RootId) -> Result<(), RootedError> {
    if destroyed(c, session)? {
        return Ok(());
    }
    let Some(bytes) = c.view_bytes_rooted(chunk) else { return Ok(()) };
    update_timer(c, session)?;
    if with_connection(c, session, |connection| connection.receive(&bytes))?.is_none() {
        return Ok(());
    }
    // What the received frames call for is sent at once, as nghttp2 answers what it read.
    match handle_of(c, session)? {
        Some(key) => {
            drive(c, key)?;
            flush_writes(c, key)
        }
        None => Ok(()),
    }
}

pub(super) fn drive_object(c: &mut Context<'_>, session: RootId) -> Result<(), RootedError> {
    match handle_of(c, session)? {
        Some(key) => drive(c, key),
        None => Ok(()),
    }
}

/// Deliver the session's connection events, then schedule what it queued to be sent. A
/// delivery that submits more continues this loop rather than nesting its own.
pub(super) fn drive(c: &mut Context<'_>, key: u64) -> Result<(), RootedError> {
    let started = with_state(c, |state| match state.sessions.get_mut(&key) {
        Some(handle) if !handle.driving => {
            handle.driving = true;
            true
        }
        _ => false,
    });
    if !started {
        return Ok(());
    }
    let result = deliver(c, key);
    with_state(c, |state| {
        if let Some(handle) = state.sessions.get_mut(&key) {
            handle.driving = false;
        }
    });
    result?;
    schedule_writes(c, key)
}

/// Deliver one event, then the rest on the next tick: Node calls into JavaScript once per
/// event, and the ticks one call queues run before the next event is delivered.
fn deliver(c: &mut Context<'_>, key: u64) -> Result<(), RootedError> {
    let step = with_state(c, |state| {
        let handle = state.sessions.get_mut(&key)?;
        let arrived = handle.connection.as_mut()?.take_events();
        handle.deferred.extend(arrived);
        if handle.continuation_scheduled {
            return None;
        }
        let event = handle.deferred.pop_front()?;
        Some((event, handle.object))
    });
    let Some((event, session)) = step else { return Ok(()) };
    dispatch(c, session, event)?;
    let continuation = with_state(c, |state| {
        let handle = state.sessions.get_mut(&key)?;
        let more = !handle.deferred.is_empty() || handle.connection.as_ref().is_some_and(Connection::has_events);
        (more && !std::mem::replace(&mut handle.continuation_scheduled, true)).then_some(handle.object)
    });
    if let Some(session) = continuation {
        let resume = closure(c, Member::ContinueDelivery, &[session])?;
        next_tick(c, resume, &[])?;
    }
    Ok(())
}

/// Send the connection's queued frames on the next immediate, coalescing what one turn
/// submits into one socket write, as Node's `MaybeScheduleWrite` does.
fn schedule_writes(c: &mut Context<'_>, key: u64) -> Result<(), RootedError> {
    let session = with_state(c, |state| {
        let handle = state.sessions.get_mut(&key)?;
        let wants = handle.connection.as_ref()?.wants_write();
        (wants && !std::mem::replace(&mut handle.write_scheduled, true)).then_some(handle.object)
    });
    let Some(session) = session else { return Ok(()) };
    let flush = closure(c, Member::FlushWrites, &[session])?;
    set_immediate(c, flush, &[])
}

/// Frame and send what the connection queued, then deliver what framing completed. A closed
/// session whose frames are all sent finishes closing (`ongracefulclosecomplete`).
fn flush_writes(c: &mut Context<'_>, key: u64) -> Result<(), RootedError> {
    let step = with_state(c, |state| {
        let handle = state.sessions.get_mut(&key)?;
        handle.write_scheduled = false;
        let output = handle.connection.as_mut()?.take_output();
        Some((output, handle.socket, handle.object))
    });
    let Some((output, socket, session)) = step else { return Ok(()) };
    write_socket(c, socket, output)?;
    drive(c, key)?;
    if closed(c, session)? {
        let undefined = c.undefined();
        maybe_destroy(c, session, undefined)?;
    }
    Ok(())
}

fn write_socket(c: &mut Context<'_>, socket: RootId, output: Vec<u8>) -> Result<(), RootedError> {
    if output.is_empty() || truthy_member(c, socket, "destroyed")? || !truthy_member(c, socket, "writable")? {
        return Ok(());
    }
    let chunk = buffer(c, output)?;
    call_method(c, socket, "write", &[chunk]).map(drop)
}

/// The stream object for protocol stream `id`, while it is live.
pub(super) fn stream_object(c: &mut Context<'_>, session: RootId, id: u32) -> Result<Option<RootId>, RootedError> {
    let Some(key) = handle_of(c, session)? else { return Ok(None) };
    Ok(with_state(c, |state| {
        let stream = state.sessions.get(&key)?.streams.get(&id)?;
        state.streams.get(stream).map(|stream| stream.object)
    }))
}

fn dispatch(c: &mut Context<'_>, session: RootId, event: Event) -> Result<(), RootedError> {
    if destroyed(c, session)? && !matches!(event, Event::StreamClosed { .. } | Event::WriteDone { .. }) {
        return Ok(());
    }
    match event {
        Event::Headers { stream, category, flags, fields } => on_headers(c, session, stream, category, flags, &fields),
        Event::Data { stream, bytes, end } => {
            let Some(object) = stream_object(c, session, stream)? else { return Ok(()) };
            if stream::destroyed(c, object)? {
                return Ok(());
            }
            stream::update_timer(c, object)?;
            if !bytes.is_empty() {
                let chunk = buffer(c, bytes)?;
                call_method(c, object, "push", &[chunk])?;
            }
            if end {
                let null = c.null();
                call_method(c, object, "push", &[null])?;
            }
            Ok(())
        }
        Event::StreamClosed { stream, code } => match stream_object(c, session, stream)? {
            Some(object) => stream::on_close(c, object, code),
            None => Ok(()),
        },
        Event::WriteDone { stream } => match stream_object(c, session, stream)? {
            Some(object) => stream::write_done(c, object),
            None => Ok(()),
        },
        Event::WantTrailers { stream } => match stream_object(c, session, stream)? {
            Some(object) => stream::on_trailers(c, object),
            None => Ok(()),
        },
        Event::RemoteSettings => {
            update_timer(c, session)?;
            let undefined = c.undefined();
            put_slot(c, session, Slot::RemoteSettings, undefined)?;
            let settings = get(c, session, "remoteSettings")?;
            emit(c, session, "remoteSettings", &[settings]).map(drop)
        }
        Event::SettingsAck { duration } => {
            let callback = take_callback(c, session, |handle| handle.settings_callbacks.pop_front())?;
            settings_answered(c, session, callback, Some(duration))
        }
        Event::Ping { payload } => {
            update_timer(c, session)?;
            let payload = buffer(c, payload.to_vec())?;
            emit(c, session, "ping", &[payload]).map(drop)
        }
        Event::PingAck { payload, duration } => {
            let callback = take_callback(c, session, |handle| handle.ping_callbacks.pop_front())?;
            let Some(callback) = callback else { return Ok(()) };
            let null = c.null();
            let duration = c.number(duration);
            let payload = buffer(c, payload.to_vec())?;
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[null, duration, payload]).map(drop)
        }
        Event::Goaway { code, last_stream, data } => on_goaway(c, session, code, last_stream, data),
        Event::AltSvc { stream, origin, value } => {
            let value = super::latin1_string(c, &value);
            let origin = super::latin1_string(c, &origin);
            let stream = c.number(f64::from(stream));
            emit(c, session, "altsvc", &[value, origin, stream]).map(drop)
        }
        Event::Origin { origins } => {
            let origins: Vec<RootId> = origins.iter().map(|origin| super::latin1_string(c, origin)).collect();
            let origins = c.array_rooted(&origins)?;
            emit(c, session, "origin", &[origins]).map(drop)
        }
        Event::ConnectionError { code } => {
            let reason = if code == ErrorCode::ProtocolError as u32 && connecting_preface(c, session)? {
                NghttpCode::BadClientMagic
            } else {
                NghttpCode::Protocol
            };
            let error = reason.error(c)?;
            call_method(c, session, "destroy", &[error]).map(drop)
        }
    }
}

/// Whether a server session failed before the client preface completed.
fn connecting_preface(c: &mut Context<'_>, session: RootId) -> Result<bool, RootedError> {
    Ok(session_type(c, session)? == SessionType::Server
        && with_connection(c, session, |connection| connection.session_state().last_proc_stream_id == 0.0)?.unwrap_or(false))
}

/// The retained callback `take` removes from the session's handle, released to a local root.
fn take_callback(c: &mut Context<'_>, session: RootId, take: impl FnOnce(&mut SessionHandle) -> Option<RootId>) -> Result<Option<RootId>, RootedError> {
    let Some(key) = handle_of(c, session)? else { return Ok(None) };
    let retained = with_state(c, |state| state.sessions.get_mut(&key).and_then(take));
    Ok(retained.map(|retained| release_to_local(c, retained)))
}

/// A local root for a retained one, releasing the retained root.
pub(super) fn release_to_local(c: &mut Context<'_>, retained: RootId) -> RootId {
    let local = c.scope_root(retained).unwrap_or(retained);
    c.release_root(retained);
    local
}

/// `settingsCallback(cb, ack, duration)`.
fn settings_answered(c: &mut Context<'_>, session: RootId, callback: Option<RootId>, duration: Option<f64>) -> Result<(), RootedError> {
    let pending = slot_number(c, session, Slot::PendingAck)?;
    put_slot_number(c, session, Slot::PendingAck, pending - 1.0)?;
    let undefined = c.undefined();
    put_slot(c, session, Slot::LocalSettings, undefined)?;
    let callback = callback.filter(|callback| c.is_callable_rooted(*callback).unwrap_or(false));
    match duration {
        Some(duration) => {
            let settings = get(c, session, "localSettings")?;
            if let Some(callback) = callback {
                let null = c.null();
                let duration = c.number(duration);
                c.call_rooted(callback, undefined, &[null, settings, duration])?;
            }
            emit(c, session, "localSettings", &[settings]).map(drop)
        }
        None => {
            if let Some(callback) = callback {
                let error = Fault::SettingsCancel.error(c, &[])?;
                c.call_rooted(callback, undefined, &[error])?;
            }
            Ok(())
        }
    }
}

/// `onSessionHeaders(handle, id, cat, flags, headers, sensitiveHeaders)`.
fn on_headers(c: &mut Context<'_>, session: RootId, id: u32, category: Category, flags: u8, fields: &[Field]) -> Result<(), RootedError> {
    update_timer(c, session)?;
    let end_of_stream = flags & flags::END_STREAM != 0;
    let (object, raw) = headers::received(c, fields)?;
    let flags_value = c.number(f64::from(flags));
    let null = c.null();
    let stream = match stream_object(c, session, id)? {
        Some(stream) => {
            let status = get(c, object, headers::STATUS)?;
            let status = c.rooted_value(status).and_then(|value| value.as_number());
            let event = match category {
                Category::Response if !end_of_stream && status.is_some_and(|status| INFORMATIONAL.contains(&status)) => {
                    "headers"
                }
                Category::Response => "response",
                Category::PushResponse => "push",
                _ if status.is_some_and(|status| status >= INFORMATIONAL.end) => "response",
                _ if end_of_stream => "trailers",
                _ => "headers",
            };
            emit_next_tick(c, stream, event, &[object, flags_value, raw])?;
            stream
        }
        None => {
            if closed(c, session)? {
                with_connection(c, session, |connection| connection.rst_stream(id, ErrorCode::RefusedStream as u32))?;
                return Ok(());
            }
            let undefined = c.undefined();
            let id_value = c.number(f64::from(id));
            let empty = c.object_rooted()?;
            let stream = match session_type(c, session)? {
                SessionType::Server => {
                    let class = class(c, Class::ServerHttp2Stream)?;
                    let stream = c.construct_rooted(class, class, &[session, undefined, id_value, empty, object])?;
                    if end_of_stream {
                        call_method(c, stream, "push", &[null])?;
                    }
                    let method = get(c, object, headers::METHOD)?;
                    if c.string_text(method)?.as_deref() == Some(headers::METHOD_HEAD) {
                        call_method(c, stream, "end", &[])?;
                        set_flag(c, stream, stream_flags::HEAD_REQUEST, true)?;
                    }
                    stream
                }
                SessionType::Client => {
                    let class = class(c, Class::ClientHttp2Stream)?;
                    let stream = c.construct_rooted(class, class, &[session, undefined, id_value, empty])?;
                    if end_of_stream {
                        call_method(c, stream, "push", &[null])?;
                    }
                    call_method(c, stream, "end", &[])?;
                    stream
                }
            };
            if end_of_stream {
                put_slot_bool(c, stream, Slot::EndAfterHeaders, true)?;
            }
            emit_next_tick(c, session, "stream", &[stream, object, flags_value, raw])?;
            stream
        }
    };
    if end_of_stream {
        call_method(c, stream, "push", &[null])?;
    }
    Ok(())
}

/// `onGoawayData(code, lastStreamID, buf)`.
fn on_goaway(c: &mut Context<'_>, session: RootId, code: u32, last_stream: u32, data: Vec<u8>) -> Result<(), RootedError> {
    put_slot_number(c, session, Slot::GoawayCode, f64::from(code))?;
    put_slot_number(c, session, Slot::GoawayLastStreamId, f64::from(last_stream))?;
    let code_value = c.number(f64::from(code));
    let last = c.number(f64::from(last_stream));
    let data = buffer(c, data)?;
    emit(c, session, "goaway", &[code_value, last, data])?;
    if code == ErrorCode::NoError as u32 {
        call_method(c, session, "close", &[]).map(drop)
    } else {
        let error = Fault::SessionError.error(c, &[&code.to_string()])?;
        let no_error = c.number(f64::from(ErrorCode::NoError as u32));
        call_method(c, session, "destroy", &[error, no_error]).map(drop)
    }
}

/// `session[kUpdateTimer]()`.
pub(super) fn update_timer(c: &mut Context<'_>, target: RootId) -> Result<(), RootedError> {
    let timer = get_slot(c, target, Slot::Timeout)?;
    if c.truthy_rooted(timer)? {
        call_method(c, timer, "refresh", &[])?;
    }
    Ok(())
}

/// `setStreamTimeout(msecs, callback)`, shared by sessions and streams.
pub(super) fn set_timeout(c: &mut Context<'_>, this: RootId, msecs: RootId, callback: RootId) -> Outcome {
    if truthy_member(c, this, "destroyed")? {
        return Ok(this);
    }
    put(c, this, "timeout", msecs)?;
    let duration = super::validate_number(c, msecs, "msecs")?;
    let timer = get_slot(c, this, Slot::Timeout)?;
    if c.truthy_rooted(timer)? {
        let undefined = c.undefined();
        crate::modules::timers_shared_vm::clear_timer(c, undefined, &[timer])?;
    }
    let has_callback = !is_undefined(c, callback);
    if duration == 0.0 {
        let null = c.null();
        put_slot(c, this, Slot::Timeout, null)?;
        if has_callback {
            super::validate_function(c, callback, "callback")?;
            crate::modules::stream_shared_vm::remove_listener(c, this, "timeout", callback)?;
        }
    } else {
        let fired = closure(c, Member::TimerFired, &[this])?;
        let undefined = c.undefined();
        let delay = c.number(duration);
        let timer = crate::modules::timers_shared_vm::set_timeout(c, undefined, &[fired, delay])?;
        call_method(c, timer, "unref", &[])?;
        put_slot(c, this, Slot::Timeout, timer)?;
        let session = get_slot(c, this, Slot::Session)?;
        if c.is_object_rooted(session)? {
            update_timer(c, session)?;
        }
        if has_callback {
            super::validate_function(c, callback, "callback")?;
            once(c, this, "timeout", callback)?;
        }
    }
    Ok(this)
}

fn origin_set(c: &mut Context<'_>, session: RootId) -> Outcome {
    let encrypted = slot_truthy(c, session, Slot::Encrypted)?;
    if !encrypted || destroyed(c, session)? {
        return Ok(c.undefined());
    }
    let socket = get_slot(c, session, Slot::Socket)?;
    let mut origins = Vec::new();
    let servername = get(c, socket, "servername")?;
    if c.is_object_rooted(socket)? && !is_nullish(c, servername) {
        let mut text = format!("https://{}", c.to_string(servername)?);
        let port = get(c, socket, "remotePort")?;
        if !is_nullish(c, port) {
            text.push(':');
            text.push_str(&c.to_string(port)?);
        }
        let text = c.string_rooted(&text);
        let url = super::url(c, text)?;
        origins.push(get(c, url, "origin")?);
    }
    c.array_rooted(&origins)
}

fn session_state(c: &mut Context<'_>, session: RootId) -> Outcome {
    let object = c.object_rooted()?;
    if connecting(c, session)? || destroyed(c, session)? {
        return Ok(object);
    }
    let Some(state) = with_connection(c, session, |connection| connection.session_state())? else { return Ok(object) };
    let fields = [
        ("effectiveLocalWindowSize", state.effective_local_window_size),
        ("effectiveRecvDataLength", state.effective_recv_data_length),
        ("nextStreamID", state.next_stream_id),
        ("localWindowSize", state.local_window_size),
        ("lastProcStreamID", state.last_proc_stream_id),
        ("remoteWindowSize", state.remote_window_size),
        ("outboundQueueSize", state.outbound_queue_size),
        ("deflateDynamicTableSize", state.deflate_dynamic_table_size),
        ("inflateDynamicTableSize", state.inflate_dynamic_table_size),
    ];
    for (name, value) in fields {
        crate::modules::stream_shared_vm::put_number(c, object, name, value)?;
    }
    Ok(object)
}

/// The `localSettings` or `remoteSettings` getter: cached until the settings change.
fn settings_of(c: &mut Context<'_>, session: RootId, cache: Slot) -> Outcome {
    let cached = get_slot(c, session, cache)?;
    if !is_undefined(c, cached) {
        return Ok(cached);
    }
    if destroyed(c, session)? || connecting(c, session)? {
        return c.object_rooted();
    }
    let remote = matches!(cache, Slot::RemoteSettings);
    let Some(settings) = with_connection(c, session, |connection| if remote { connection.remote } else { connection.local })? else {
        return c.object_rooted();
    };
    let object = super::settings::object(c, &settings)?;
    put_slot(c, session, cache, object)?;
    Ok(object)
}

/// `session.ping([payload, ]callback)`.
fn ping(c: &mut Context<'_>, session: RootId, payload: RootId, callback: RootId) -> Outcome {
    ensure_live(c, session)?;
    let (payload, callback) = if c.is_callable_rooted(payload)? { (c.undefined(), payload) } else { (payload, callback) };
    let mut bytes = [0u8; frame::PING_LENGTH];
    if c.truthy_rooted(payload)? {
        let Some(given) = c.view_bytes_rooted(payload) else {
            return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(
                c,
                payload,
                "payload",
                &super::types_expectation("Buffer,TypedArray,DataView"),
            )?);
        };
        bytes = given.try_into().map_err(|_| super::throw(c, Fault::PingLength, &[]))?;
    }
    super::validate_function(c, callback, "callback")?;
    if connecting(c, session)? || closed(c, session)? {
        let cancelled = closure(c, Member::PingCancelled, &[callback])?;
        next_tick(c, cancelled, &[])?;
        return Ok(c.undefined());
    }
    let sent = with_connection(c, session, |connection| connection.ping(bytes))?;
    let accepted = matches!(sent, Some(Ok(())));
    if accepted {
        let retained = c.retain(callback)?;
        let key = handle_of(c, session)?;
        with_state(c, |state| {
            if let Some(handle) = key.and_then(|key| state.sessions.get_mut(&key)) {
                handle.ping_callbacks.push_back(retained);
            }
        });
        drive_object(c, session)?;
    }
    Ok(c.boolean(accepted))
}

/// `session.settings(settings, callback)`.
fn settings(c: &mut Context<'_>, session: RootId, settings: RootId, callback: RootId) -> Result<(), RootedError> {
    ensure_live(c, session)?;
    super::assert_is_object(c, settings, "settings", "Object")?;
    super::settings::validate(c, settings)?;
    if c.truthy_rooted(callback)? {
        super::validate_function(c, callback, "callback")?;
    }
    let pending = slot_number(c, session, Slot::PendingAck)?;
    put_slot_number(c, session, Slot::PendingAck, pending + 1.0)?;
    let copy = super::spread(c, settings)?;
    let submit = closure(c, Member::SubmitSettings, &[session, copy, callback])?;
    if connecting(c, session)? {
        once(c, session, "connect", submit)
    } else {
        let undefined = c.undefined();
        c.call_rooted(submit, undefined, &[]).map(drop)
    }
}

/// `submitSettings(settings, callback)`.
fn submit_settings(c: &mut Context<'_>, session: RootId, settings: RootId, callback: RootId) -> Result<(), RootedError> {
    if destroyed(c, session)? {
        return Ok(());
    }
    update_timer(c, session)?;
    let entries = super::settings::entries(c, settings)?;
    let submitted = with_connection(c, session, |connection| connection.submit_settings(entries))?;
    if matches!(submitted, Some(Err(Refusal::TooManyPending))) {
        let error = Fault::MaxPendingSettingsAck.error(c, &[])?;
        return call_method(c, session, "destroy", &[error]).map(drop);
    }
    let retained = c.retain(callback)?;
    let key = handle_of(c, session)?;
    with_state(c, |state| {
        if let Some(handle) = key.and_then(|key| state.sessions.get_mut(&key)) {
            handle.settings_callbacks.push_back(retained);
        }
    });
    drive_object(c, session)
}

/// `session.goaway(code, lastStreamID, opaqueData)`.
fn goaway(c: &mut Context<'_>, session: RootId, args: &[RootId]) -> Result<(), RootedError> {
    ensure_live(c, session)?;
    let undefined = c.undefined();
    let given: Vec<Option<RootId>> =
        (0..3).map(|index| args.get(index).copied().filter(|value| !is_undefined(c, *value))).collect();
    let code = match given[0] {
        Some(code) => code,
        None => c.number(f64::from(ErrorCode::NoError as u32)),
    };
    let last = match given[1] {
        Some(last) => last,
        None => c.number(0.0),
    };
    let data = given[2].unwrap_or(undefined);
    if !is_undefined(c, data) && c.view_bytes_rooted(data).is_none() {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(
            c,
            data,
            "opaqueData",
            &super::types_expectation("Buffer,TypedArray,DataView"),
        )?);
    }
    super::validate_number(c, code, "code")?;
    super::validate_number(c, last, "lastStreamID")?;
    let submit = closure(c, Member::SubmitGoaway, &[session, code, last, data])?;
    if connecting(c, session)? {
        once(c, session, "connect", submit)
    } else {
        c.call_rooted(submit, undefined, &[]).map(drop)
    }
}

fn submit_goaway(c: &mut Context<'_>, session: RootId, code: RootId, last: RootId, data: RootId) -> Result<(), RootedError> {
    if destroyed(c, session)? {
        return Ok(());
    }
    update_timer(c, session)?;
    let code = c.to_number(code)? as u32;
    let last = c.to_number(last)?.max(0.0) as u32;
    let data = c.view_bytes_rooted(data).unwrap_or_default();
    with_connection(c, session, |connection| connection.goaway(code, last, &data))?;
    drive_object(c, session)
}

/// `session.destroy(error = NGHTTP2_NO_ERROR, code)`.
fn destroy(c: &mut Context<'_>, session: RootId, args: &[RootId]) -> Result<(), RootedError> {
    if destroyed(c, session)? {
        return Ok(());
    }
    let undefined = c.undefined();
    let mut error = args.first().copied().unwrap_or(undefined);
    let mut code = args.get(1).copied().unwrap_or(undefined);
    if is_undefined(c, error) {
        error = c.number(f64::from(ErrorCode::NoError as u32));
    }
    if let Some(number) = c.rooted_value(error).and_then(|value| value.as_number()) {
        code = error;
        error = if number != f64::from(ErrorCode::NoError as u32) {
            Fault::SessionError.error(c, &[&super::number_text(number)])?
        } else {
            undefined
        };
    }
    if is_undefined(c, code) && !is_nullish(c, error) {
        code = c.number(f64::from(ErrorCode::InternalError as u32));
    }
    close_session(c, session, code, error)
}

/// `closeSession(session, code, error)`.
pub(super) fn close_session(c: &mut Context<'_>, session: RootId, code: RootId, error: RootId) -> Result<(), RootedError> {
    set_flag(c, session, session_flags::DESTROYED, true)?;
    put_slot(c, session, Slot::DestroyCode, code)?;
    let zero = c.number(0.0);
    call_method(c, session, "setTimeout", &[zero])?;
    let timeout = c.string_rooted("timeout");
    call_method(c, session, "removeAllListeners", &[timeout])?;
    // What was submitted goes out before the streams' resets, which the terminating
    // GOAWAY then supersedes.
    if let Some(key) = handle_of(c, session)? {
        let pending = with_state(c, |state| {
            let handle = state.sessions.get_mut(&key)?;
            Some((handle.connection.as_mut()?.take_output(), handle.socket))
        });
        if let Some((output, socket)) = pending {
            write_socket(c, socket, output)?;
        }
    }
    let (open, pending) = stream_objects(c, session)?;
    if !open.is_empty() || !pending.is_empty() {
        let cancel = stream_cancel(c, error)?;
        for stream in pending {
            call_method(c, stream, "destroy", &[cancel])?;
        }
        for stream in open {
            call_method(c, stream, "destroy", &[error])?;
        }
    }
    let socket = get_slot(c, session, Slot::Socket)?;
    let Some(key) = handle_of(c, session)? else { return finish_close(c, session, error) };
    let wire_code = c.rooted_value(code).and_then(|value| value.as_number()).unwrap_or(0.0) as u32;
    let socket_destroyed = c.is_object_rooted(socket)? && truthy_member(c, socket, "destroyed")?;
    let teardown = with_state(c, |state| {
        let handle = state.sessions.get_mut(&key)?;
        let mut connection = handle.connection.take()?;
        if !socket_destroyed {
            connection.terminate(wire_code);
        }
        Some((
            connection.take_output(),
            std::mem::take(&mut handle.ping_callbacks),
            std::mem::take(&mut handle.settings_callbacks),
        ))
    });
    if let Some((output, pings, settings)) = teardown {
        write_socket(c, socket, output)?;
        for callback in pings {
            let callback = release_to_local(c, callback);
            let cancelled = closure(c, Member::PingCancelled, &[callback])?;
            set_immediate(c, cancelled, &[])?;
        }
        for callback in settings {
            let callback = release_to_local(c, callback);
            settings_answered(c, session, Some(callback), None)?;
        }
    }
    finish_close(c, session, error)
}

/// `new ERR_HTTP2_STREAM_CANCEL(error)`.
fn stream_cancel(c: &mut Context<'_>, error: RootId) -> Outcome {
    if !c.truthy_rooted(error)? {
        return Fault::StreamCancel.error(c, &[""]);
    }
    let message = get(c, error, "message")?;
    let suffix = match c.string_text(message)? {
        Some(message) => format!(" (caused by: {message})"),
        None => String::new(),
    };
    let cancel = Fault::StreamCancel.error(c, &[&suffix])?;
    put(c, cancel, "cause", error)?;
    Ok(cancel)
}

/// `finishSessionClose(session, error)`, after `cleanupSession`.
fn finish_close(c: &mut Context<'_>, session: RootId, error: RootId) -> Result<(), RootedError> {
    let socket = get_slot(c, session, Slot::Socket)?;
    cleanup(c, session, socket)?;
    let emit_close = closure(c, Member::EmitClose, &[session, error])?;
    if c.is_object_rooted(socket)? && !truthy_member(c, socket, "destroyed")? {
        on(c, socket, "close", emit_close)?;
        if closed(c, session)? {
            call_method(c, socket, "resume", &[])?;
        }
        let ended = closure(c, Member::SocketEnded, &[session, socket, error])?;
        call_method(c, socket, "end", &[ended])?;
        Ok(())
    } else {
        next_tick(c, emit_close, &[])
    }
}

/// `cleanupSession(session)`: unbind the socket and release the session's host side.
fn cleanup(c: &mut Context<'_>, session: RootId, socket: RootId) -> Result<(), RootedError> {
    let undefined = c.undefined();
    put_slot(c, session, Slot::Socket, undefined)?;
    let key = handle_of(c, session)?;
    put_slot(c, session, Slot::Handle, undefined)?;
    if c.is_object_rooted(socket)? {
        put_slot(c, socket, Slot::BoundSession, undefined)?;
        put_slot(c, socket, Slot::Server, undefined)?;
    }
    let Some(key) = key else { return Ok(()) };
    let released = with_state(c, |state| state.sessions.remove(&key));
    if let Some(handle) = released {
        let SessionHandle { object, socket, ping_callbacks, settings_callbacks, .. } = handle;
        for root in [object, socket].into_iter().chain(ping_callbacks).chain(settings_callbacks) {
            c.release_root(root);
        }
    }
    Ok(())
}

/// `session.close(callback)`.
fn close(c: &mut Context<'_>, session: RootId, callback: RootId) -> Result<(), RootedError> {
    if closed(c, session)? || destroyed(c, session)? {
        return Ok(());
    }
    set_flag(c, session, session_flags::CLOSED, true)?;
    if c.is_callable_rooted(callback)? {
        once(c, session, "close", callback)?;
    }
    call_method(c, session, "goaway", &[])?;
    let undefined = c.undefined();
    maybe_destroy(c, session, undefined)
}

/// `() => session[kMaybeDestroy]()`, as a stream's destruction defers it.
pub(super) fn maybe_destroy_closure(c: &mut Context<'_>, session: RootId) -> Outcome {
    closure(c, Member::MaybeDestroy, &[session])
}

/// `session[kMaybeDestroy](error)`.
pub(super) fn maybe_destroy(c: &mut Context<'_>, session: RootId, error: RootId) -> Result<(), RootedError> {
    if is_nullish(c, error) {
        let (open, pending) = stream_objects(c, session)?;
        let pending_data = with_connection(c, session, |connection| connection.has_pending_data())?.unwrap_or(false);
        if !closed(c, session)? || !open.is_empty() || !pending.is_empty() || pending_data {
            return Ok(());
        }
    }
    let args: Vec<RootId> = if is_nullish(c, error) { Vec::new() } else { vec![error] };
    call_method(c, session, "destroy", &args).map(drop)
}

/// `socketOnError(error)`, on the bound socket.
fn socket_error(c: &mut Context<'_>, socket: RootId, error: RootId) -> Result<(), RootedError> {
    let session = get_slot(c, socket, Slot::BoundSession)?;
    if is_undefined(c, session) {
        return Ok(());
    }
    let code = get(c, error, "code")?;
    let reset = c.string_text(code)?.as_deref() == Some("ECONNRESET");
    let goaway = get_slot(c, session, Slot::GoawayCode)?;
    if reset && !crate::modules::stream_shared_vm::is_null(c, goaway) {
        return call_method(c, session, "destroy", &[]).map(drop);
    }
    call_method(c, session, "destroy", &[error]).map(drop)
}

/// `socketOnClose()`, on the bound socket.
fn socket_close(c: &mut Context<'_>, socket: RootId) -> Result<(), RootedError> {
    let session = get_slot(c, socket, Slot::BoundSession)?;
    if is_undefined(c, session) {
        return Ok(());
    }
    let error = if connecting(c, session)? { Fault::SocketClosed.error(c, &[])? } else { c.null() };
    let (open, pending) = stream_objects(c, session)?;
    let cancel = c.number(f64::from(ErrorCode::Cancel as u32));
    for stream in open.into_iter().chain(pending) {
        call_method(c, stream, "close", &[cancel])?;
    }
    call_method(c, session, "close", &[])?;
    let code = c.number(f64::from(ErrorCode::NoError as u32));
    close_session(c, session, code, error)
}

/// `serverSession.altsvc(alt, originOrStream)`.
fn altsvc(c: &mut Context<'_>, session: RootId, alt: RootId, origin_or_stream: RootId) -> Result<(), RootedError> {
    ensure_live(c, session)?;
    let mut stream = 0u32;
    let mut origin = String::new();
    if let Some(text) = c.string_text(origin_or_stream)? {
        let text = c.string_rooted(&text);
        let url = attempt(super::url(c, text))?;
        origin = match url {
            Ok(url) => {
                let value = get(c, url, "origin")?;
                c.to_string(value)?
            }
            Err(_) => "null".to_owned(),
        };
        if origin == "null" {
            return Err(super::throw(c, Fault::AltsvcInvalidOrigin, &[]));
        }
    } else if let Some(number) = c.rooted_value(origin_or_stream).and_then(|value| value.as_number()) {
        if number.fract() != 0.0 || number <= 0.0 || number > MAX_STREAMS {
            return Err(out_of_range(c, "originOrStream", "> 0 && < 4294967296", origin_or_stream)?);
        }
        stream = number as u32;
    } else if !is_undefined(c, origin_or_stream) {
        let value = if c.is_object_rooted(origin_or_stream)? { get(c, origin_or_stream, "origin")? } else { c.undefined() };
        let Some(text) = c.string_text(value)? else {
            return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(
                c,
                origin_or_stream,
                "originOrStream",
                &super::types_expectation("string,number,URL,Object"),
            )?);
        };
        if text == "null" || text.is_empty() {
            return Err(super::throw(c, Fault::AltsvcInvalidOrigin, &[]));
        }
        origin = text;
    }
    let Some(value) = c.string_text(alt)? else {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, alt, "alt", "string")?);
    };
    let quoted = value.chars().all(|ch| matches!(ch, '\t' | ' '..='[' | ']'..='~' | '\u{80}'..='\u{ff}'));
    if !quoted {
        let message = "The argument 'alt' is invalid";
        return Err(crate::modules::http_shared_vm::throw_coded(c, crate::modules::http_shared_vm::ErrorClass::TypeError, "ERR_INVALID_CHAR", message));
    }
    if value.len() + origin.len() > MAX_ALTSVC {
        return Err(super::throw(c, Fault::AltsvcLength, &[]));
    }
    let value = super::latin1(c, alt)?;
    with_connection(c, session, |connection| connection.altsvc(stream, origin.as_bytes(), &value))?;
    drive_object(c, session)
}

/// `serverSession.origin(...origins)`.
fn origin(c: &mut Context<'_>, session: RootId, origins: &[RootId]) -> Result<(), RootedError> {
    ensure_live(c, session)?;
    if origins.is_empty() {
        return Ok(());
    }
    let mut serialized = Vec::new();
    let mut length = 0;
    for &origin in origins {
        let value = if c.string_text(origin)?.is_some() {
            let url = attempt(super::url(c, origin))?;
            match url {
                Ok(url) => get(c, url, "origin")?,
                Err(_) => c.string_rooted("null"),
            }
        } else if c.is_object_rooted(origin)? {
            get(c, origin, "origin")?
        } else {
            origin
        };
        let Some(text) = c.string_text(value)? else {
            return Err(crate::modules::util_shared_vm::argument_type_error(c, value, "origin", "string")?);
        };
        if text == "null" {
            return Err(super::throw(c, Fault::InvalidOrigin, &[]));
        }
        length += text.len();
        serialized.push(text.into_bytes());
    }
    if length > MAX_ALTSVC {
        return Err(super::throw(c, Fault::OriginLength, &[]));
    }
    with_connection(c, session, |connection| connection.origin(&serialized))?;
    drive_object(c, session)
}

/// `clientSession.request(headers, options)`.
fn request(c: &mut Context<'_>, session: RootId, headers: RootId, options: RootId) -> Outcome {
    ensure_live(c, session)?;
    if closed(c, session)? {
        return Err(super::throw(c, Fault::GoawaySession, &[]));
    }
    update_timer(c, session)?;
    let headers = if is_undefined(c, headers) {
        c.object_rooted()?
    } else if c.is_object_rooted(headers)? && !c.is_callable_rooted(headers)? {
        headers
    } else {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(
            c,
            headers,
            "headers",
            &super::types_expectation("Object,Array"),
        )?);
    };
    let head = headers::request(c, headers, session)?;
    super::assert_is_object(c, options, "options", "Object")?;
    let options = super::spread(c, options)?;
    validate_priority(c, options)?;
    let end_stream = get(c, options, "endStream")?;
    let end_stream = if is_undefined(c, end_stream) {
        let method = c.string_text(head.method)?;
        matches!(method.as_deref(), Some("DELETE" | headers::METHOD_GET | headers::METHOD_HEAD))
    } else {
        super::validate_boolean(c, end_stream, "options.endStream")?
    };
    let want_trailers = truthy_member(c, options, "waitForTrailers")?;
    let class = class(c, Class::ClientHttp2Stream)?;
    let undefined = c.undefined();
    let empty = c.object_rooted()?;
    let stream = c.construct_rooted(class, class, &[session, undefined, undefined, empty])?;
    if let Some(object) = head.object {
        put_slot(c, stream, Slot::SentHeaders, object)?;
    }
    if let Some(raw) = head.raw {
        put_slot(c, stream, Slot::RawHeaders, raw)?;
    }
    let origin = format!("{}://{}", c.to_string(head.scheme)?, c.to_string(head.authority)?);
    let origin = c.string_rooted(&origin);
    put_slot(c, stream, Slot::Origin, origin)?;
    if end_stream {
        call_method(c, stream, "end", &[])?;
    }
    if want_trailers {
        set_flag(c, stream, stream_flags::HAS_TRAILERS, true)?;
    }
    let signal = get(c, options, "signal")?;
    if c.truthy_rooted(signal)? {
        crate::modules::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
        let aborter = closure(c, Member::RequestAborted, &[stream, signal])?;
        if truthy_member(c, signal, "aborted")? {
            c.call_rooted(aborter, undefined, &[])?;
        } else {
            let disposable = crate::modules::stream_shared_vm::utils::add_abort_listener(c, signal, aborter)?;
            let dispose = crate::modules::stream_shared_vm::utils::registered_symbol(c, "nodejs.dispose")?;
            let dispose = c.get_property_rooted(disposable, dispose)?;
            if c.is_callable_rooted(dispose)? {
                let bound = call_method(c, dispose, "bind", &[disposable])?;
                once(c, stream, "close", bound)?;
            }
        }
    }
    stream::set_pending_request(c, stream, PendingRequest { fields: head.fields, end_stream, want_trailers })?;
    let on_connect = closure(c, Member::RequestOnConnect, &[stream])?;
    if connecting(c, session)? {
        let pending = get_slot(c, session, Slot::PendingRequestCalls)?;
        if c.truthy_rooted(pending)? {
            call_method(c, pending, "push", &[on_connect])?;
        } else {
            let pending = c.array_rooted(&[on_connect])?;
            put_slot(c, session, Slot::PendingRequestCalls, pending)?;
            let run = closure(c, Member::RunPendingRequests, &[session])?;
            once(c, session, "connect", run)?;
        }
    } else {
        c.call_rooted(on_connect, undefined, &[])?;
    }
    Ok(stream)
}

/// `setAndValidatePriorityOptions(options)`.
fn validate_priority(c: &mut Context<'_>, options: RootId) -> Result<(), RootedError> {
    let parent = get(c, options, "parent")?;
    if is_undefined(c, parent) {
        crate::modules::stream_shared_vm::put_number(c, options, "parent", 0.0)?;
    } else {
        let number = super::validate_number(c, parent, "options.parent")?;
        if number < 0.0 {
            return Err(out_of_range(c, "options.parent", ">= 0", parent)?);
        }
    }
    for name in ["exclusive", "silent"] {
        let value = get(c, options, name)?;
        if is_undefined(c, value) {
            crate::modules::stream_shared_vm::put_bool(c, options, name, false)?;
        } else {
            super::validate_boolean(c, value, &format!("options.{name}"))?;
        }
    }
    Ok(())
}

/// `requestOnConnect(headersList, options)`.
fn request_on_connect(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let session = get_slot(c, stream, Slot::Session)?;
    if !c.is_object_rooted(session)? || destroyed(c, session)? {
        return Ok(());
    }
    if closed(c, session)? {
        let error = Fault::GoawaySession.error(c, &[])?;
        return call_method(c, stream, "destroy", &[error]).map(drop);
    }
    let Some(request) = stream::take_pending_request(c, stream)? else { return Ok(()) };
    let submitted = with_connection(c, session, |connection| {
        connection.submit_request(&request.fields, request.end_stream, request.want_trailers)
    })?;
    match submitted {
        Some(Ok(id)) => {
            stream::init(c, stream, id)?;
            drive_object(c, session)
        }
        Some(Err(_)) => {
            let error = Fault::OutOfStreams.error(c, &[])?;
            call_method(c, stream, "destroy", &[error]).map(drop)
        }
        None => Ok(()),
    }
}

/// `http2.connect(authority, options, listener)`.
pub(super) fn connect(c: &mut Context<'_>, authority: RootId, options: RootId, listener: RootId) -> Outcome {
    let (options, listener) = if c.is_callable_rooted(options)? { (c.undefined(), options) } else { (options, listener) };
    super::assert_is_object(c, options, "options", "Object")?;
    let options = super::spread(c, options)?;
    // The origin set holds the session's own origin only, so its bound is just validated.
    let max_origin_set_size = get(c, options, "maxOriginSetSize")?;
    if !is_nullish(c, max_origin_set_size)
        && super::validate_number(c, max_origin_set_size, "options.maxOriginSetSize")? < 0.0
    {
        return Err(out_of_range(c, "options.maxOriginSetSize", ">= 0", max_origin_set_size)?);
    }
    let remote_custom = get(c, options, "remoteCustomSettings")?;
    if !is_undefined(c, remote_custom) && !c.is_array_rooted(remote_custom)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, remote_custom, "options.remoteCustomSettings", "Array")?);
    }
    let authority = if c.string_text(authority)?.is_some() { super::url(c, authority)? } else { authority };
    super::assert_is_object(c, authority, "authority", "string,Object,URL")?;
    let own_protocol = get(c, authority, "protocol")?;
    let option_protocol = get(c, options, "protocol")?;
    let protocol = if c.truthy_rooted(own_protocol)? {
        c.to_string(own_protocol)?
    } else if c.truthy_rooted(option_protocol)? {
        c.to_string(option_protocol)?
    } else {
        HTTPS_PROTOCOL.to_owned()
    };
    let port_value = get(c, authority, "port")?;
    let port_text = if is_undefined(c, port_value) { String::new() } else { c.to_string(port_value)? };
    let port = if !port_text.is_empty() {
        port_text
    } else if c.to_string(own_protocol)? == HTTP_PROTOCOL {
        HTTP_PORT.to_owned()
    } else {
        HTTPS_PORT.to_owned()
    };
    let hostname = get(c, authority, "hostname")?;
    let host_value = get(c, authority, "host")?;
    let host = if c.truthy_rooted(hostname)? {
        let host = c.to_string(hostname)?;
        match host.strip_prefix('[') {
            Some(inner) => inner[..inner.len().saturating_sub(1)].to_owned(),
            None => host,
        }
    } else if c.truthy_rooted(host_value)? {
        c.to_string(host_value)?
    } else {
        "localhost".to_owned()
    };
    let create_connection = get(c, options, "createConnection")?;
    let socket = if c.is_callable_rooted(create_connection)? {
        let undefined = c.undefined();
        c.call_rooted(create_connection, undefined, &[authority, options])?
    } else {
        match protocol.as_str() {
            HTTP_PROTOCOL => {
                let net = crate::host::shared_vm::commonjs::net_module(c)?;
                let connect_options = c.object_rooted()?;
                let port_root = c.string_rooted(&port);
                put(c, connect_options, "port", port_root)?;
                let host_root = c.string_rooted(&host);
                put(c, connect_options, "host", host_root)?;
                super::assign(c, connect_options, options)?;
                call_method(c, net, "connect", &[connect_options])?
            }
            HTTPS_PROTOCOL => {
                let tls = crate::host::shared_vm::commonjs::tls_module(c)?;
                let net = crate::host::shared_vm::commonjs::net_module(c)?;
                let host_root = c.string_rooted(&host);
                let is_ip = call_method(c, net, "isIP", &[host_root])?;
                let servername = if c.truthy_rooted(is_ip)? { None } else { Some(host.as_str()) };
                let tls_options = super::server::tls_options(c, options, servername)?;
                let port_root = c.string_rooted(&port);
                call_method(c, tls, "connect", &[port_root, host_root, tls_options])?
            }
            _ => return Err(super::throw(c, Fault::UnsupportedProtocol, &[&protocol])),
        }
    };
    let class = class(c, Class::ClientHttp2Session)?;
    let session = c.construct_rooted(class, class, &[options, socket])?;
    let servername = get(c, options, "servername")?;
    let name = if c.truthy_rooted(servername)? { c.to_string(servername)? } else { host };
    let authority_text = c.string_rooted(&format!("{name}:{port}"));
    put_slot(c, session, Slot::Authority, authority_text)?;
    let protocol = c.string_rooted(&protocol);
    put_slot(c, session, Slot::Protocol, protocol)?;
    if c.is_callable_rooted(listener)? {
        once(c, session, "connect", listener)?;
    }
    Ok(session)
}
