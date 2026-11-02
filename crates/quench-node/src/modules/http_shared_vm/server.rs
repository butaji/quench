//! `ServerResponse` and `Server` (lib/_http_server.js): the connection listener attaches a
//! request parser to each socket, answers each parsed request with a response, queues
//! pipelined responses, and keeps the connection alive, upgrades it or closes it.

use super::outgoing::{self, has_token};
use super::parser::{HeadOutcome, Kind};
use super::parsing::{self, Executed};
use super::{captured, class, current, function, get_slot, put_slot, throw_coded, Class, ErrorClass, Slot};
use crate::modules::http_protocol::STATUS_CODES;
use crate::modules::stream_shared_vm::{
    arg, call_method, emit, get, is_nullish, is_string, is_undefined, listener_count, next_tick, number, on, once,
    put, put_bool, put_number, remove_listener, truthy_member, Context, Outcome,
};
use quench_runtime::{RootId, RootedError};

crate::modules::host_members::members!(Member, "httpServer", [
    ResponseConstruct,
    ResponseFinish,
    AssignSocket,
    DetachSocket,
    WriteContinue,
    WriteProcessing,
    WriteEarlyHints,
    ImplicitHeader,
    WriteHead,
    ServerConstruct,
    Close,
    CloseAllConnections,
    CloseIdleConnections,
    SetTimeout,
    /// `[Symbol.asyncDispose]()`: the promise of `close`.
    AsyncDispose,
    /// `onServerResponseClose`, on the socket.
    OnResponseClose,
    /// `emitCloseNT(res)`.
    EmitClose,
    /// `socketOnData`, `socketOnEnd`, `socketOnClose`, `socketOnDrain`, `socketOnError` and
    /// `socketOnTimeout`, each over its connection's server, socket and state.
    SocketData,
    SocketEnd,
    SocketClose,
    SocketDrain,
    SocketError,
    SocketTimeout,
    /// `updateOutgoingData(socket, state, delta)`.
    PendingData,
    /// `resOnFinish(req, res, socket, state, server)`.
    ResponseFinished,
    /// `clearIncoming(req)`, on the request's `end`.
    ClearIncoming,
    /// `setupConnectionsTracking`, on `listening`.
    TrackConnections,
    /// `checkConnections`, on the tracking interval.
    CheckConnections,
    /// The default `shouldUpgradeCallback`: whether the server listens for `upgrade`.
    ShouldUpgrade,
]);

/// The connection's bookkeeping fields (`connectionListenerInternal`'s `state`).
const STATE_OUTGOING: &str = "outgoing";
const STATE_INCOMING: &str = "incoming";
const STATE_OUTGOING_DATA: &str = "outgoingData";
const STATE_REQUESTS: &str = "requestsCount";
const STATE_KEEP_ALIVE_TIMEOUT_SET: &str = "keepAliveTimeoutSet";
/// The listeners the connection installs, kept for removal on upgrade.
const STATE_LISTENERS: [(&str, &str); 6] = [
    ("data", "onData"),
    ("end", "onEnd"),
    ("close", "onClose"),
    ("drain", "onDrain"),
    ("error", "onError"),
    ("timeout", "onTimeout"),
];

/// Node's `Server` option defaults, in milliseconds.
const DEFAULT_REQUEST_TIMEOUT: f64 = 300_000.0;
const DEFAULT_HEADERS_TIMEOUT: f64 = 60_000.0;
const DEFAULT_KEEP_ALIVE_TIMEOUT: f64 = 5_000.0;
const DEFAULT_KEEP_ALIVE_TIMEOUT_BUFFER: f64 = 1_000.0;
const DEFAULT_CONNECTIONS_CHECKING_INTERVAL: f64 = 30_000.0;
/// The status codes `ServerResponse` assigns itself.
const STATUS_OK: u16 = 200;
const STATUS_CONTINUE: u16 = 100;
const STATUS_PROCESSING: u16 = 102;
const STATUS_EARLY_HINTS: u16 = 103;
const STATUS_BAD_REQUEST: u16 = 400;
const STATUS_REQUEST_TIMEOUT: u16 = 408;
const STATUS_PAYLOAD_TOO_LARGE: u16 = 413;
const STATUS_EXPECTATION_FAILED: u16 = 417;
const STATUS_HEADER_FIELDS_TOO_LARGE: u16 = 431;
const STATUS_SERVICE_UNAVAILABLE: u16 = 503;
const STATUS_NO_CONTENT: u16 = 204;
const STATUS_NOT_MODIFIED: u16 = 304;
const STATUS_RANGE: std::ops::RangeInclusive<f64> = 100.0..=999.0;
const INFORMATIONAL: std::ops::RangeInclusive<f64> = 100.0..=199.0;

fn reason(status: u16) -> &'static str {
    STATUS_CODES.iter().find(|(code, _)| *code == status).map_or("unknown", |(_, reason)| reason)
}

/// A status line with `Connection: close` and no body, as a server answers what it cannot
/// serve.
fn closing_response(status: u16) -> String {
    format!("HTTP/1.1 {status} {}\r\nConnection: close\r\n\r\n", reason(status))
}

pub(crate) fn build_response(c: &mut Context<'_>, outgoing: RootId) -> Outcome {
    let class = super::constructor(c, Member::ResponseConstruct, "ServerResponse")?;
    super::inherit(c, class, outgoing)?;
    let prototype = get(c, class, "prototype")?;
    super::methods(c, prototype, &[("_finish", Member::ResponseFinish)])?;
    put_number(c, prototype, "statusCode", f64::from(STATUS_OK))?;
    let undefined = c.undefined();
    put(c, prototype, "statusMessage", undefined)?;
    super::methods(c, prototype, &[
        ("assignSocket", Member::AssignSocket),
        ("detachSocket", Member::DetachSocket),
        ("writeContinue", Member::WriteContinue),
        ("writeProcessing", Member::WriteProcessing),
        ("writeEarlyHints", Member::WriteEarlyHints),
        ("_implicitHeader", Member::ImplicitHeader),
        ("writeHead", Member::WriteHead),
    ])?;
    let write_head = get(c, prototype, "writeHead")?;
    put(c, prototype, "writeHeader", write_head)?;
    Ok(class)
}

pub(crate) fn build_server(c: &mut Context<'_>) -> Outcome {
    let net = class(c, Class::Net)?;
    let net_server = get(c, net, "Server")?;
    let class = super::constructor(c, Member::ServerConstruct, "Server")?;
    super::inherit(c, class, net_server)?;
    let prototype = get(c, class, "prototype")?;
    install_server_methods(c, prototype)?;
    Ok(class)
}

/// The members an HTTP server class shares with `https.Server`.
pub(crate) fn install_server_methods(c: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    super::methods(c, prototype, &[
        ("close", Member::Close),
        ("closeAllConnections", Member::CloseAllConnections),
        ("closeIdleConnections", Member::CloseIdleConnections),
        ("setTimeout", Member::SetTimeout),
    ])?;
    let dispose = function(c, Member::AsyncDispose, "[Symbol.asyncDispose]", &[])?;
    let key = crate::modules::fs_shared_vm::classes::well_known_symbol(c, "asyncDispose", "nodejs.asyncDispose")?;
    c.define_data_property_rooted(prototype, key, dispose, true, true, true)?;
    Ok(())
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::ResponseConstruct => {
            let (request, options) = (at(0), at(1));
            construct_response(c, this, request, options)?;
            Ok(c.undefined())
        }
        Member::ResponseFinish => {
            emit(c, this, "prefinish", &[])?;
            Ok(c.undefined())
        }
        Member::AssignSocket => {
            assign_socket(c, this, at(0))?;
            Ok(c.undefined())
        }
        Member::DetachSocket => {
            detach_socket(c, this, at(0))?;
            Ok(c.undefined())
        }
        Member::WriteContinue => {
            write_interim(c, this, STATUS_CONTINUE, at(0))?;
            put_bool(c, this, "_sent100", true)?;
            Ok(c.undefined())
        }
        Member::WriteProcessing => {
            write_interim(c, this, STATUS_PROCESSING, at(0))?;
            Ok(c.undefined())
        }
        Member::WriteEarlyHints => {
            write_early_hints(c, this, at(0), at(1))?;
            Ok(c.undefined())
        }
        Member::ImplicitHeader => {
            let status = get(c, this, "statusCode")?;
            call_method(c, this, "writeHead", &[status])?;
            Ok(c.undefined())
        }
        Member::WriteHead => write_head(c, this, args),
        Member::ServerConstruct => construct_server(c, this, args),
        Member::Close => {
            pre_close(c, this)?;
            let net = class(c, Class::Net)?;
            let net_server = get(c, net, "Server")?;
            let prototype = get(c, net_server, "prototype")?;
            let close = get(c, prototype, "close")?;
            c.call_rooted(close, this, args)?;
            Ok(this)
        }
        Member::CloseAllConnections => {
            for socket in connections(c, this)? {
                call_method(c, socket, "destroy", &[])?;
            }
            Ok(c.undefined())
        }
        Member::AsyncDispose => crate::modules::util_module_shared_vm::close_promise(c, this),
        Member::CloseIdleConnections => {
            close_idle_connections(c, this)?;
            Ok(c.undefined())
        }
        Member::SetTimeout => {
            let (msecs, callback) = (at(0), at(1));
            put(c, this, "timeout", msecs)?;
            if c.truthy_rooted(callback)? {
                on(c, this, "timeout", callback)?;
            }
            Ok(this)
        }
        Member::OnResponseClose => {
            let response = get(c, this, "_httpMessage")?;
            if !is_nullish(c, response) {
                emit_close(c, response)?;
            }
            Ok(c.undefined())
        }
        Member::EmitClose => {
            let [response] = captured(c)?;
            emit_close(c, response)?;
            Ok(c.undefined())
        }
        Member::SocketData => {
            let [server, socket, state] = captured(c)?;
            socket_data(c, server, socket, state, at(0))?;
            Ok(c.undefined())
        }
        Member::SocketEnd => {
            let [server, socket, state] = captured(c)?;
            socket_end(c, server, socket, state)?;
            Ok(c.undefined())
        }
        Member::SocketClose => {
            let [server, socket, state] = captured(c)?;
            let parser = get(c, socket, "parser")?;
            parsing::free(c, parser, None, socket)?;
            abort_incoming(c, state)?;
            let tracked = get_slot(c, server, Slot::Connections)?;
            if !is_nullish(c, tracked) {
                call_method(c, tracked, "delete", &[socket])?;
            }
            Ok(c.undefined())
        }
        Member::SocketDrain => {
            let [_, socket, state] = captured(c)?;
            socket_drain(c, socket, state)?;
            Ok(c.undefined())
        }
        Member::SocketError => {
            let [server, socket, state] = captured(c)?;
            socket_error(c, server, socket, state, at(0))?;
            Ok(c.undefined())
        }
        Member::SocketTimeout => {
            let [server, socket, _] = captured(c)?;
            socket_timeout(c, server, socket)?;
            Ok(c.undefined())
        }
        Member::PendingData => {
            let [socket, state] = captured(c)?;
            let delta = at(0);
            let delta = c.to_number(delta)?;
            let data = number(c, state, STATE_OUTGOING_DATA)?;
            put_number(c, state, STATE_OUTGOING_DATA, data + delta)?;
            socket_drain(c, socket, state)?;
            Ok(c.undefined())
        }
        Member::ResponseFinished => {
            let [request, response, socket, state, server] = captured(c)?;
            response_finished(c, request, response, socket, state, server)?;
            Ok(c.undefined())
        }
        Member::ClearIncoming => {
            clear_incoming(c, this)?;
            Ok(c.undefined())
        }
        Member::TrackConnections => {
            track_connections(c, this)?;
            Ok(c.undefined())
        }
        Member::CheckConnections => {
            let [server] = captured(c)?;
            check_connections(c, server)?;
            Ok(c.undefined())
        }
        Member::ShouldUpgrade => {
            let listening = listener_count(c, this, "upgrade")? > 0.0;
            Ok(c.boolean(listening))
        }
    }
}

// ---------------------------------------------------------------------------------------
// ServerResponse
// ---------------------------------------------------------------------------------------

fn construct_response(c: &mut Context<'_>, this: RootId, request: RootId, options: RootId) -> Result<(), RootedError> {
    outgoing::initialize(c, this, options)?;
    let method = get(c, request, "method")?;
    if c.string_text(method)?.as_deref() == Some("HEAD") {
        put_bool(c, this, "_hasBody", false)?;
    }
    put(c, this, "req", request)?;
    put_bool(c, this, "sendDate", true)?;
    put_bool(c, this, "_sent100", false)?;
    put_bool(c, this, "_expect_continue", false)?;
    let major = number(c, request, "httpVersionMajor")?;
    let minor = number(c, request, "httpVersionMinor")?;
    if major < 1.0 || minor < 1.0 {
        let headers = get(c, request, "headers")?;
        let te = get(c, headers, "te")?;
        let chunked = if is_undefined(c, te) { false } else { has_token(&c.to_string(te)?, "chunked") };
        put_bool(c, this, "useChunkedEncodingByDefault", chunked)?;
        put_bool(c, this, "shouldKeepAlive", false)?;
    }
    publish_exchange(c, RESPONSE_CREATED_CHANNEL, &[("request", request), ("response", this)])
}

fn assign_socket(c: &mut Context<'_>, this: RootId, socket: RootId) -> Result<(), RootedError> {
    let owner = get(c, socket, "_httpMessage")?;
    if c.truthy_rooted(owner)? {
        return Err(throw_coded(c, ErrorClass::Error, "ERR_HTTP_SOCKET_ASSIGNED", "Socket already assigned"));
    }
    put(c, socket, "_httpMessage", this)?;
    let listener = function(c, Member::OnResponseClose, "onServerResponseClose", &[])?;
    put_slot(c, this, Slot::CloseListener, listener)?;
    on(c, socket, "close", listener)?;
    put_slot(c, this, Slot::Socket, socket)?;
    emit(c, this, "socket", &[socket])?;
    outgoing::flush(c, this)
}

fn detach_socket(c: &mut Context<'_>, this: RootId, socket: RootId) -> Result<(), RootedError> {
    let listener = get_slot(c, this, Slot::CloseListener)?;
    if !is_nullish(c, listener) {
        remove_listener(c, socket, "close", listener)?;
    }
    let null = c.null();
    put(c, socket, "_httpMessage", null)?;
    put_slot(c, this, Slot::Socket, null)
}

fn emit_close(c: &mut Context<'_>, response: RootId) -> Result<(), RootedError> {
    if !truthy_member(c, response, "_closed")? {
        put_bool(c, response, "destroyed", true)?;
        put_bool(c, response, "_closed", true)?;
        emit(c, response, "close", &[])?;
    }
    Ok(())
}

fn write_raw(c: &mut Context<'_>, this: RootId, head: &str, callback: RootId) -> Result<(), RootedError> {
    let head = c.string_rooted(head);
    let ascii = c.string_rooted("ascii");
    call_method(c, this, "_writeRaw", &[head, ascii, callback]).map(drop)
}

fn write_interim(c: &mut Context<'_>, this: RootId, status: u16, callback: RootId) -> Result<(), RootedError> {
    write_raw(c, this, &format!("HTTP/1.1 {status} {}\r\n\r\n", reason(status)), callback)
}

fn write_early_hints(c: &mut Context<'_>, this: RootId, hints: RootId, callback: RootId) -> Result<(), RootedError> {
    if !c.is_object_rooted(hints)? || c.is_array_rooted(hints)? {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, hints, "hints", "Object") {
            Ok(error) | Err(error) => error,
        });
    }
    let link = get(c, hints, "link")?;
    if is_nullish(c, link) {
        return Ok(());
    }
    let link = if c.is_array_rooted(link)? {
        let separator = c.string_rooted(", ");
        let joined = call_method(c, link, "join", &[separator])?;
        c.to_string(joined)?
    } else {
        c.to_string(link)?
    };
    if link.is_empty() {
        return Ok(());
    }
    let mut head = format!("HTTP/1.1 {STATUS_EARLY_HINTS} {}\r\nLink: {link}\r\n", reason(STATUS_EARLY_HINTS));
    for key in c.enumerable_own_keys_rooted(hints, false, true)? {
        let name = c.to_string(key)?;
        if name != "link" {
            let value = c.get_property_rooted(hints, key)?;
            let value = c.to_string(value)?;
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    write_raw(c, this, &head, callback)
}

fn write_head(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (original, reason_or_headers, mut headers) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if truthy_member(c, this, "_header")? {
        return Err(throw_coded(
            c,
            ErrorClass::Error,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot write headers after they are sent to the client",
        ));
    }
    let zero = c.number(0.0);
    let status = c.bitwise_or_rooted(original, zero)?;
    let status = c.to_number(status)?;
    if !STATUS_RANGE.contains(&status) {
        let message = super::format(c, "Invalid status code: %s", &[original])?;
        return Err(throw_coded(c, ErrorClass::RangeError, "ERR_HTTP_INVALID_STATUS_CODE", &message));
    }
    if is_string(c, reason_or_headers) {
        put(c, this, "statusMessage", reason_or_headers)?;
    } else {
        let current = get(c, this, "statusMessage")?;
        if !c.truthy_rooted(current)? {
            let codes = class(c, Class::StatusCodes)?;
            let key = c.number(status);
            let default = c.get_property_rooted(codes, key)?;
            let default = if c.truthy_rooted(default)? { default } else { c.string_rooted("unknown") };
            put(c, this, "statusMessage", default)?;
        }
        if is_nullish(c, headers) {
            headers = reason_or_headers;
        }
    }
    put_number(c, this, "statusCode", status)?;
    let own = get_slot(c, this, Slot::OutHeaders)?;
    let rendered = if c.truthy_rooted(own)? {
        if c.is_array_rooted(headers)? {
            let items = c.array_like_values_rooted(headers)?;
            if items.len() % 2 != 0 {
                return Err(match crate::modules::util_shared_vm::argument_value_error(c, headers, "headers", "is invalid") {
                    Ok(error) | Err(error) => error,
                });
            }
            for pair in items.chunks(2) {
                call_method(c, this, "removeHeader", &[pair[0]])?;
            }
            for pair in items.chunks(2) {
                if c.truthy_rooted(pair[0])? {
                    call_method(c, this, "appendHeader", &[pair[0], pair[1]])?;
                }
            }
        } else if c.truthy_rooted(headers)? {
            for key in c.enumerable_own_keys_rooted(headers, false, true)? {
                if c.truthy_rooted(key)? {
                    let value = c.get_property_rooted(headers, key)?;
                    call_method(c, this, "setHeader", &[key, value])?;
                }
            }
        }
        get_slot(c, this, Slot::OutHeaders)?
    } else {
        headers
    };
    let message = get(c, this, "statusMessage")?;
    if outgoing::has_invalid_header_char(c, message)? {
        return Err(throw_coded(c, ErrorClass::TypeError, "ERR_INVALID_CHAR", "Invalid character in statusMessage"));
    }
    let message = c.to_string(message)?;
    let status_line = format!("HTTP/1.1 {status} {message}\r\n");
    if status == f64::from(STATUS_NO_CONTENT) || status == f64::from(STATUS_NOT_MODIFIED) || INFORMATIONAL.contains(&status) {
        put_bool(c, this, "_hasBody", false)?;
    }
    if truthy_member(c, this, "_expect_continue")? && !truthy_member(c, this, "_sent100")? {
        put_bool(c, this, "shouldKeepAlive", false)?;
    }
    outgoing::store_header(c, this, status_line, rendered)?;
    Ok(this)
}

// ---------------------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------------------

fn validate_milliseconds(c: &mut Context<'_>, options: RootId, name: &str, default: f64) -> Outcome {
    let value = get(c, options, name)?;
    if is_undefined(c, value) {
        return Ok(c.number(default));
    }
    let maximum = c.number(super::MAX_SAFE_INTEGER);
    crate::modules::util_shared_vm::validate_integer(c, value, name, 0.0, maximum)?;
    Ok(value)
}


fn validate_boolean(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if is_undefined(c, value) || c.rooted_value(value).is_some_and(|value| value.as_bool().is_some()) {
        return Ok(());
    }
    Err(match crate::modules::util_shared_vm::argument_type_error(c, value, name, "boolean") {
        Ok(error) | Err(error) => error,
    })
}

/// `storeHTTPOptions.call(server, options)`.
pub(crate) fn store_options(c: &mut Context<'_>, server: RootId, options: RootId) -> Result<(), RootedError> {
    let incoming = get(c, options, "IncomingMessage")?;
    let incoming = if c.truthy_rooted(incoming)? { incoming } else { class(c, Class::IncomingMessage)? };
    put_slot(c, server, Slot::IncomingMessageClass, incoming)?;
    let response = get(c, options, "ServerResponse")?;
    let response = if c.truthy_rooted(response)? { response } else { class(c, Class::ServerResponse)? };
    put_slot(c, server, Slot::ServerResponseClass, response)?;
    let max_header_size = get(c, options, "maxHeaderSize")?;
    if !is_undefined(c, max_header_size) {
        let maximum = c.number(super::MAX_SAFE_INTEGER);
        crate::modules::util_shared_vm::validate_integer(c, max_header_size, "maxHeaderSize", 0.0, maximum)?;
    }
    put(c, server, "maxHeaderSize", max_header_size)?;
    let insecure = get(c, options, "insecureHTTPParser")?;
    validate_boolean(c, insecure, "options.insecureHTTPParser")?;
    put(c, server, "insecureHTTPParser", insecure)?;
    let request_timeout = validate_milliseconds(c, options, "requestTimeout", DEFAULT_REQUEST_TIMEOUT)?;
    put(c, server, "requestTimeout", request_timeout)?;
    let request_timeout = c.to_number(request_timeout)?;
    let headers_timeout = validate_milliseconds(c, options, "headersTimeout", DEFAULT_HEADERS_TIMEOUT.min(request_timeout))?;
    put(c, server, "headersTimeout", headers_timeout)?;
    let headers = c.to_number(headers_timeout)?;
    if request_timeout > 0.0 && headers > 0.0 && headers > request_timeout {
        let received = crate::modules::util_shared_vm::range_error_received(c, headers_timeout)?;
        return Err(throw_coded(
            c,
            ErrorClass::RangeError,
            "ERR_OUT_OF_RANGE",
            &format!("The value of \"headersTimeout\" is out of range. It must be <= requestTimeout. Received {received}"),
        ));
    }
    for (name, default) in [
        ("keepAliveTimeout", DEFAULT_KEEP_ALIVE_TIMEOUT),
        ("keepAliveTimeoutBuffer", DEFAULT_KEEP_ALIVE_TIMEOUT_BUFFER),
        ("connectionsCheckingInterval", DEFAULT_CONNECTIONS_CHECKING_INTERVAL),
    ] {
        let value = validate_milliseconds(c, options, name, default)?;
        put(c, server, name, value)?;
    }
    let require_host = get(c, options, "requireHostHeader")?;
    validate_boolean(c, require_host, "options.requireHostHeader")?;
    let require_host = if is_undefined(c, require_host) { c.boolean(true) } else { require_host };
    put(c, server, "requireHostHeader", require_host)?;
    let join = get(c, options, "joinDuplicateHeaders")?;
    validate_boolean(c, join, "options.joinDuplicateHeaders")?;
    put(c, server, "joinDuplicateHeaders", join)?;
    let reject = get(c, options, "rejectNonStandardBodyWrites")?;
    validate_boolean(c, reject, "options.rejectNonStandardBodyWrites")?;
    let reject = if is_undefined(c, reject) { c.boolean(false) } else { reject };
    put(c, server, "rejectNonStandardBodyWrites", reject)?;
    let should_upgrade = get(c, options, "shouldUpgradeCallback")?;
    let should_upgrade = if is_undefined(c, should_upgrade) {
        function(c, Member::ShouldUpgrade, "shouldUpgradeCallback", &[])?
    } else if c.is_callable_rooted(should_upgrade)? {
        should_upgrade
    } else {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, should_upgrade, "options.shouldUpgradeCallback", "function") {
            Ok(error) | Err(error) => error,
        });
    };
    put(c, server, "shouldUpgradeCallback", should_upgrade)
}

/// The HTTP fields a server takes after its socket server is made.
pub(crate) fn initialize_server(c: &mut Context<'_>, server: RootId, listener: RootId) -> Result<(), RootedError> {
    if c.truthy_rooted(listener)? {
        on(c, server, "request", listener)?;
    }
    put_bool(c, server, "httpAllowHalfOpen", false)?;
    let track = function(c, Member::TrackConnections, "setupConnectionsTracking", &[])?;
    on(c, server, "listening", track)?;
    put_number(c, server, "timeout", 0.0)?;
    let null = c.null();
    put(c, server, "maxHeadersCount", null)?;
    put_number(c, server, "maxRequestsPerSocket", 0.0)
}

/// `options` as the server constructors take it: a listener moves to `listener`.
pub(crate) fn server_options(c: &mut Context<'_>, options: RootId, listener: RootId) -> Result<(RootId, RootId), RootedError> {
    if c.is_callable_rooted(options)? {
        return Ok((c.object_rooted()?, options));
    }
    if is_nullish(c, options) {
        return Ok((c.object_rooted()?, listener));
    }
    if !c.is_object_rooted(options)? || c.is_array_rooted(options)? {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, options, "options", "Object") {
            Ok(error) | Err(error) => error,
        });
    }
    Ok((options, listener))
}

fn construct_server(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let server_class = class(c, Class::Server)?;
    if c.new_target_rooted().is_none() && !(c.is_object_rooted(this)? && c.instance_of_rooted(this, server_class)?) {
        return c.construct_rooted(server_class, server_class, args);
    }
    let (options, listener) = (arg(c, args, 0), arg(c, args, 1));
    let (options, listener) = server_options(c, options, listener)?;
    store_options(c, this, options)?;
    let net_options = c.object_rooted()?;
    put_bool(c, net_options, "allowHalfOpen", true)?;
    let no_delay = get(c, options, "noDelay")?;
    let no_delay = if is_nullish(c, no_delay) { c.boolean(true) } else { no_delay };
    put(c, net_options, "noDelay", no_delay)?;
    for name in ["keepAlive", "keepAliveInitialDelay", "highWaterMark"] {
        let value = get(c, options, name)?;
        put(c, net_options, name, value)?;
    }
    let net = class(c, Class::Net)?;
    let net_server = get(c, net, "Server")?;
    c.call_rooted(net_server, this, &[net_options])?;
    let connection = function(c, super::Module::ConnectionListener, "connectionListener", &[])?;
    on(c, this, "connection", connection)?;
    initialize_server(c, this, listener)?;
    Ok(this)
}

pub(crate) fn create_server(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let server = class(c, Class::Server)?;
    let (options, listener) = (arg(c, args, 0), arg(c, args, 1));
    c.construct_rooted(server, server, &[options, listener])
}

/// `httpServerPreClose(server)`.
pub(crate) fn pre_close(c: &mut Context<'_>, server: RootId) -> Result<(), RootedError> {
    call_method(c, server, "closeIdleConnections", &[])?;
    clear_checking_interval(c, server)
}

fn clear_checking_interval(c: &mut Context<'_>, server: RootId) -> Result<(), RootedError> {
    let interval = get_slot(c, server, Slot::ConnectionsCheckingInterval)?;
    if c.truthy_rooted(interval)? {
        let global = c.global_root()?;
        call_method(c, global, "clearInterval", &[interval])?;
    }
    Ok(())
}

fn connections_set(c: &mut Context<'_>, server: RootId) -> Outcome {
    let tracked = get_slot(c, server, Slot::Connections)?;
    if !is_nullish(c, tracked) {
        return Ok(tracked);
    }
    let global = c.global_root()?;
    let set = get(c, global, "Set")?;
    let tracked = c.construct_rooted(set, set, &[])?;
    put_slot(c, server, Slot::Connections, tracked)?;
    Ok(tracked)
}

fn connections(c: &mut Context<'_>, server: RootId) -> Result<Vec<RootId>, RootedError> {
    let tracked = get_slot(c, server, Slot::Connections)?;
    if is_nullish(c, tracked) {
        return Ok(Vec::new());
    }
    let global = c.global_root()?;
    let array = get(c, global, "Array")?;
    let sockets = call_method(c, array, "from", &[tracked])?;
    c.array_like_values_rooted(sockets)
}

fn close_idle_connections(c: &mut Context<'_>, server: RootId) -> Result<(), RootedError> {
    for socket in connections(c, server)? {
        let parser = get(c, socket, "parser")?;
        if parsing::in_message(c, parser)? {
            continue;
        }
        let response = get(c, socket, "_httpMessage")?;
        if c.truthy_rooted(response)? && !truthy_member(c, response, "finished")? {
            continue;
        }
        call_method(c, socket, "destroy", &[])?;
    }
    Ok(())
}

fn track_connections(c: &mut Context<'_>, server: RootId) -> Result<(), RootedError> {
    connections_set(c, server)?;
    clear_checking_interval(c, server)?;
    let check = function(c, Member::CheckConnections, "checkConnections", &[server])?;
    let interval = get(c, server, "connectionsCheckingInterval")?;
    let global = c.global_root()?;
    let timer = call_method(c, global, "setInterval", &[check, interval])?;
    call_method(c, timer, "unref", &[])?;
    put_slot(c, server, Slot::ConnectionsCheckingInterval, timer)
}

fn check_connections(c: &mut Context<'_>, server: RootId) -> Result<(), RootedError> {
    let headers_timeout = number(c, server, "headersTimeout")?;
    let request_timeout = number(c, server, "requestTimeout")?;
    if headers_timeout == 0.0 && request_timeout == 0.0 {
        return Ok(());
    }
    for socket in connections(c, server)? {
        let parser = get(c, socket, "parser")?;
        if parsing::expired(c, parser, headers_timeout, request_timeout)? {
            let error = super::coded(c, ErrorClass::Error, "ERR_HTTP_REQUEST_TIMEOUT", "Request timeout")?;
            let state = get_slot(c, socket, Slot::ConnectionState)?;
            socket_error(c, server, socket, state, error)?;
        }
    }
    Ok(())
}

/// `connectionListenerInternal(server, socket)`: `this` is the server.
pub(crate) fn connection_listener(c: &mut Context<'_>, server: RootId, args: &[RootId]) -> Outcome {
    let socket = arg(c, args, 0);
    put(c, socket, "server", server)?;
    let timeout = get(c, server, "timeout")?;
    let setter = get(c, socket, "setTimeout")?;
    if c.truthy_rooted(timeout)? && c.is_callable_rooted(setter)? {
        c.call_rooted(setter, socket, &[timeout])?;
    }
    let parser = parsing::create(c, Kind::Request, socket, server)?;
    put(c, socket, "parser", parser)?;
    let max_headers = get(c, server, "maxHeadersCount")?;
    if let Some(count) = c.rooted_value(max_headers).and_then(|value| value.as_number()) {
        put_number(c, parser, "maxHeaderPairs", count * 2.0)?;
    }
    let state = c.null_object_rooted()?;
    for name in [STATE_OUTGOING, STATE_INCOMING] {
        let list = c.array_rooted(&[])?;
        put(c, state, name, list)?;
    }
    put_number(c, state, STATE_OUTGOING_DATA, 0.0)?;
    put_number(c, state, STATE_REQUESTS, 0.0)?;
    put_bool(c, state, STATE_KEEP_ALIVE_TIMEOUT_SET, false)?;
    put_slot(c, socket, Slot::ConnectionState, state)?;
    let captures = [server, socket, state];
    for ((event, field), member) in STATE_LISTENERS.into_iter().zip([
        Member::SocketData,
        Member::SocketEnd,
        Member::SocketClose,
        Member::SocketDrain,
        Member::SocketError,
        Member::SocketTimeout,
    ]) {
        let listener = function(c, member, field, &captures)?;
        put(c, state, field, listener)?;
        on(c, socket, event, listener)?;
    }
    let tracked = connections_set(c, server)?;
    call_method(c, tracked, "add", &[socket])?;
    put_bool(c, socket, "_paused", false)?;
    Ok(c.undefined())
}

fn remove_connection_listeners(c: &mut Context<'_>, socket: RootId, state: RootId) -> Result<(), RootedError> {
    for (event, field) in STATE_LISTENERS {
        let listener = get(c, state, field)?;
        remove_listener(c, socket, event, listener)?;
    }
    Ok(())
}

fn socket_data(c: &mut Context<'_>, server: RootId, socket: RootId, state: RootId, data: RootId) -> Result<(), RootedError> {
    let parser = get(c, socket, "parser")?;
    if is_nullish(c, parser) {
        return Ok(());
    }
    let executed = parsing::execute(c, parser, data)?;
    reset_socket_timeout(c, server, socket, state)?;
    match executed {
        Executed::Failed(error) => {
            put(c, error, "rawPacket", data)?;
            socket_error(c, server, socket, state, error)?;
        }
        Executed::Upgraded(rest) => {
            let request = get(c, parser, "incoming")?;
            remove_connection_listeners(c, socket, state)?;
            parsing::free(c, parser, Some(request), socket)?;
            let method = get(c, request, "method")?;
            let event = if c.string_text(method)?.as_deref() == Some("CONNECT") { "connect" } else { "upgrade" };
            if listener_count(c, server, event)? > 0.0 {
                let head = crate::modules::buffer_shared_vm::from_vec(c, rest)?;
                let null = c.null();
                put(c, socket, "readableFlowing", null)?;
                emit(c, server, event, &[request, socket, head])?;
            } else {
                call_method(c, socket, "destroy", &[])?;
            }
        }
        Executed::Consumed => {}
    }
    Ok(())
}

fn socket_end(c: &mut Context<'_>, server: RootId, socket: RootId, state: RootId) -> Result<(), RootedError> {
    let parser = get(c, socket, "parser")?;
    if let Some(error) = parsing::finish(c, parser)? {
        return socket_error(c, server, socket, state, error);
    }
    let outgoing = get(c, state, STATE_OUTGOING)?;
    let pending = c.array_like_values_rooted(outgoing)?;
    let response = get(c, socket, "_httpMessage")?;
    if !truthy_member(c, server, "httpAllowHalfOpen")? {
        abort_incoming(c, state)?;
        if truthy_member(c, socket, "writable")? {
            call_method(c, socket, "end", &[])?;
        }
    } else if let Some(&last) = pending.last() {
        put_bool(c, last, "_last", true)?;
    } else if c.truthy_rooted(response)? {
        put_bool(c, response, "_last", true)?;
    } else if truthy_member(c, socket, "writable")? {
        call_method(c, socket, "end", &[])?;
    }
    Ok(())
}

fn abort_incoming(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let incoming = get(c, state, STATE_INCOMING)?;
    while number(c, incoming, "length")? > 0.0 {
        let request = call_method(c, incoming, "shift", &[])?;
        let error = super::connection_reset(c, "aborted")?;
        call_method(c, request, "destroy", &[error])?;
    }
    Ok(())
}

fn socket_drain(c: &mut Context<'_>, socket: RootId, state: RootId) -> Result<(), RootedError> {
    let need_pause = number(c, state, STATE_OUTGOING_DATA)? > number(c, socket, "writableHighWaterMark")?;
    if truthy_member(c, socket, "_paused")? && !need_pause {
        put_bool(c, socket, "_paused", false)?;
        call_method(c, socket, "resume", &[])?;
    }
    let message = get(c, socket, "_httpMessage")?;
    if c.truthy_rooted(message)? && !truthy_member(c, message, "finished")? {
        let need = get_slot(c, message, Slot::NeedDrain)?;
        if c.truthy_rooted(need)? {
            let no = c.boolean(false);
            put_slot(c, message, Slot::NeedDrain, no)?;
            emit(c, message, "drain", &[])?;
        }
    }
    Ok(())
}

/// `socketOnError(e)`: `clientError`, or the closing response a client error gets.
fn socket_error(c: &mut Context<'_>, server: RootId, socket: RootId, state: RootId, error: RootId) -> Result<(), RootedError> {
    if !is_nullish(c, state) {
        let listener = get(c, state, "onError")?;
        remove_listener(c, socket, "error", listener)?;
    }
    let nop = crate::modules::stream_shared_vm::intrinsics(c)?.nop;
    if listener_count(c, socket, "error")? == 0.0 {
        on(c, socket, "error", nop)?;
    }
    if emit(c, server, "clientError", &[error, socket])? {
        return Ok(());
    }
    let response = get(c, socket, "_httpMessage")?;
    let unsent = !c.truthy_rooted(response)? || !truthy_member(c, response, "_headerSent")?;
    if truthy_member(c, socket, "writable")? && unsent {
        let code = get(c, error, "code")?;
        let status = match c.string_text(code)?.as_deref() {
            Some("HPE_HEADER_OVERFLOW") => STATUS_HEADER_FIELDS_TOO_LARGE,
            Some("HPE_CHUNK_EXTENSIONS_OVERFLOW") => STATUS_PAYLOAD_TOO_LARGE,
            Some("ERR_HTTP_REQUEST_TIMEOUT") => STATUS_REQUEST_TIMEOUT,
            _ => STATUS_BAD_REQUEST,
        };
        let text = c.string_rooted(&closing_response(status));
        call_method(c, socket, "write", &[text])?;
    }
    call_method(c, socket, "destroy", &[error]).map(drop)
}

fn socket_timeout(c: &mut Context<'_>, server: RootId, socket: RootId) -> Result<(), RootedError> {
    let parser = get(c, socket, "parser")?;
    let request = if is_nullish(c, parser) { c.null() } else { get(c, parser, "incoming")? };
    let request_timeout = !is_nullish(c, request) && !truthy_member(c, request, "complete")? && emit(c, request, "timeout", &[socket])?;
    let response = get(c, socket, "_httpMessage")?;
    let response_timeout = !is_nullish(c, response) && emit(c, response, "timeout", &[socket])?;
    let server_timeout = emit(c, server, "timeout", &[socket])?;
    if !request_timeout && !response_timeout && !server_timeout {
        call_method(c, socket, "destroy", &[])?;
    }
    Ok(())
}

fn reset_socket_timeout(c: &mut Context<'_>, server: RootId, socket: RootId, state: RootId) -> Result<(), RootedError> {
    if !truthy_member(c, state, STATE_KEEP_ALIVE_TIMEOUT_SET)? {
        return Ok(());
    }
    let timeout = get(c, server, "timeout")?;
    let timeout = if c.truthy_rooted(timeout)? { timeout } else { c.number(0.0) };
    call_method(c, socket, "setTimeout", &[timeout])?;
    put_bool(c, state, STATE_KEEP_ALIVE_TIMEOUT_SET, false)
}

/// `parserOnIncoming(server, socket, state, req, keepAlive)`.
pub(crate) fn on_incoming(c: &mut Context<'_>, socket: RootId, request: RootId, keep_alive: bool) -> Result<HeadOutcome, RootedError> {
    let server = get(c, socket, "server")?;
    let state = get_slot(c, socket, Slot::ConnectionState)?;
    reset_socket_timeout(c, server, socket, state)?;
    if truthy_member(c, request, "upgrade")? {
        let method = get(c, request, "method")?;
        let upgrade = c.string_text(method)?.as_deref() == Some("CONNECT") || {
            let decided = call_method(c, server, "shouldUpgradeCallback", &[request])?;
            c.truthy_rooted(decided)?
        };
        put_bool(c, request, "upgrade", upgrade)?;
        if upgrade {
            return Ok(HeadOutcome::Upgrade);
        }
    }
    let incoming = get(c, state, STATE_INCOMING)?;
    call_method(c, incoming, "push", &[request])?;
    if !truthy_member(c, socket, "_paused")? {
        let writable = get(c, socket, "_writableState")?;
        let need_drain = !is_nullish(c, writable) && truthy_member(c, writable, "needDrain")?;
        if need_drain || number(c, state, STATE_OUTGOING_DATA)? >= number(c, socket, "writableHighWaterMark")? {
            put_bool(c, socket, "_paused", true)?;
            call_method(c, socket, "pause", &[])?;
        }
    }
    let response_class = get_slot(c, server, Slot::ServerResponseClass)?;
    let response_class = if c.truthy_rooted(response_class)? { response_class } else { class(c, Class::ServerResponse)? };
    let options = c.object_rooted()?;
    let mark = get(c, socket, "writableHighWaterMark")?;
    put(c, options, "highWaterMark", mark)?;
    let reject = get(c, server, "rejectNonStandardBodyWrites")?;
    put(c, options, "rejectNonStandardBodyWrites", reject)?;
    let response = c.construct_rooted(response_class, response_class, &[request, options])?;
    let keep_alive_timeout = get(c, server, "keepAliveTimeout")?;
    put(c, response, "_keepAliveTimeout", keep_alive_timeout)?;
    let max_requests = get(c, server, "maxRequestsPerSocket")?;
    put(c, response, "_maxRequestsPerSocket", max_requests)?;
    let pending = function(c, Member::PendingData, "updateOutgoingData", &[socket, state])?;
    put(c, response, "_onPendingData", pending)?;
    put_bool(c, response, "shouldKeepAlive", keep_alive)?;
    publish_exchange(c, REQUEST_START_CHANNEL, &[("request", request), ("response", response), ("socket", socket), ("server", server)])?;
    let owner = get(c, socket, "_httpMessage")?;
    if c.truthy_rooted(owner)? {
        let outgoing = get(c, state, STATE_OUTGOING)?;
        call_method(c, outgoing, "push", &[response])?;
    } else {
        call_method(c, response, "assignSocket", &[socket])?;
    }
    let finished = function(c, Member::ResponseFinished, "resOnFinish", &[request, response, socket, state, server])?;
    on(c, response, "finish", finished)?;
    let mut handled = false;
    let major = number(c, request, "httpVersionMajor")?;
    let minor = number(c, request, "httpVersionMinor")?;
    if major == 1.0 && minor == 1.0 {
        let headers = get(c, request, "headers")?;
        let host = get(c, headers, "host")?;
        if truthy_member(c, server, "requireHostHeader")? && is_undefined(c, host) {
            let status = c.number(f64::from(STATUS_BAD_REQUEST));
            let connection = c.string_rooted("Connection");
            let close = c.string_rooted("close");
            let head = c.array_rooted(&[connection, close])?;
            call_method(c, response, "writeHead", &[status, head])?;
            call_method(c, response, "end", &[])?;
            return Ok(HeadOutcome::Body);
        }
        let limit = number(c, server, "maxRequestsPerSocket")?;
        if limit > 0.0 {
            let count = number(c, state, STATE_REQUESTS)? + 1.0;
            put_number(c, state, STATE_REQUESTS, count)?;
            put_bool(c, response, "maxRequestsOnConnectionReached", limit <= count)?;
            if limit < count {
                handled = true;
                emit(c, server, "dropRequest", &[request, socket])?;
                let status = c.number(f64::from(STATUS_SERVICE_UNAVAILABLE));
                call_method(c, response, "writeHead", &[status])?;
                call_method(c, response, "end", &[])?;
            }
        }
        let expect = get(c, headers, "expect")?;
        if !handled && !is_undefined(c, expect) {
            handled = true;
            if has_token(&c.to_string(expect)?, "100-continue") {
                put_bool(c, response, "_expect_continue", true)?;
                if listener_count(c, server, "checkContinue")? > 0.0 {
                    emit(c, server, "checkContinue", &[request, response])?;
                } else {
                    call_method(c, response, "writeContinue", &[])?;
                    emit(c, server, "request", &[request, response])?;
                }
            } else if listener_count(c, server, "checkExpectation")? > 0.0 {
                emit(c, server, "checkExpectation", &[request, response])?;
            } else {
                let status = c.number(f64::from(STATUS_EXPECTATION_FAILED));
                call_method(c, response, "writeHead", &[status])?;
                call_method(c, response, "end", &[])?;
            }
        }
    }
    if !handled {
        emit(c, server, "request", &[request, response])?;
    }
    Ok(HeadOutcome::Body)
}

/// `resOnFinish`: the next pipelined response gets the socket, or the connection idles
/// or closes.
/// The server's diagnostics channels (lib/_http_server.js).
const REQUEST_START_CHANNEL: &str = "http.server.request.start";
const RESPONSE_CREATED_CHANNEL: &str = "http.server.response.created";
const RESPONSE_FINISH_CHANNEL: &str = "http.server.response.finish";

/// Publish an exchange's members (`request`, `response`, then `socket` and `server` where
/// the channel carries them) on `channel` when it has subscribers.
fn publish_exchange(c: &mut Context<'_>, channel: &str, members: &[(&str, RootId)]) -> Result<(), RootedError> {
    if !crate::modules::diagnostics_channel_shared_vm::channel_has_subscribers(&c.host_mut().shared_state().borrow(), channel) {
        return Ok(());
    }
    let message = c.object_rooted()?;
    for &(name, value) in members {
        put(c, message, name, value)?;
    }
    crate::modules::diagnostics_channel_shared_vm::publish_named(c, channel, message)
}

fn response_finished(c: &mut Context<'_>, request: RootId, response: RootId, socket: RootId, state: RootId, server: RootId) -> Result<(), RootedError> {
    publish_exchange(c, RESPONSE_FINISH_CHANNEL, &[("request", request), ("response", response), ("socket", socket), ("server", server)])?;
    let incoming = get(c, state, STATE_INCOMING)?;
    call_method(c, incoming, "shift", &[])?;
    let readable = get(c, request, "_readableState")?;
    if !truthy_member(c, request, "_consuming")? && !truthy_member(c, readable, "resumeScheduled")? {
        call_method(c, request, "_dump", &[])?;
    }
    call_method(c, response, "detachSocket", &[socket])?;
    clear_incoming(c, request)?;
    let close = function(c, Member::EmitClose, "emitCloseNT", &[response])?;
    next_tick(c, close, &[])?;
    let outgoing = get(c, state, STATE_OUTGOING)?;
    if truthy_member(c, response, "_last")? {
        let destroy_soon = get(c, socket, "destroySoon")?;
        if c.is_callable_rooted(destroy_soon)? {
            call_method(c, socket, "destroySoon", &[])?;
        } else {
            call_method(c, socket, "end", &[])?;
        }
    } else if number(c, outgoing, "length")? == 0.0 {
        let duration = |value: f64, fallback: f64| if value.is_finite() && value >= 0.0 { value } else { fallback };
        let keep_alive = duration(number(c, server, "keepAliveTimeout")?, 0.0);
        let setter = get(c, socket, "setTimeout")?;
        if keep_alive > 0.0 && c.is_callable_rooted(setter)? {
            let buffer = duration(number(c, server, "keepAliveTimeoutBuffer")?, DEFAULT_KEEP_ALIVE_TIMEOUT_BUFFER);
            let timeout = c.number(keep_alive + buffer);
            call_method(c, socket, "setTimeout", &[timeout])?;
            put_bool(c, state, STATE_KEEP_ALIVE_TIMEOUT_SET, true)?;
        }
    } else {
        let next = call_method(c, outgoing, "shift", &[])?;
        if c.truthy_rooted(next)? {
            call_method(c, next, "assignSocket", &[socket])?;
        }
    }
    Ok(())
}

/// `clearIncoming(req)`: the parser forgets a request once it has ended.
fn clear_incoming(c: &mut Context<'_>, request: RootId) -> Result<(), RootedError> {
    let socket = get(c, request, "socket")?;
    if is_nullish(c, socket) {
        return Ok(());
    }
    let parser = get(c, socket, "parser")?;
    if is_nullish(c, parser) {
        return Ok(());
    }
    let incoming = get(c, parser, "incoming")?;
    if c.strict_equal_rooted(incoming, request)? {
        if truthy_member(c, request, "readableEnded")? {
            let null = c.null();
            put(c, parser, "incoming", null)?;
        } else {
            let clear = function(c, Member::ClearIncoming, "clearIncoming", &[])?;
            once(c, request, "end", clear)?;
        }
    }
    Ok(())
}
