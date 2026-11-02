//! `Http2Server` and `Http2SecureServer` (lib/internal/http2/core.js): a `net.Server` or
//! `tls.Server` whose connections become `ServerHttp2Session`s, falling back to HTTP/1 for
//! TLS clients that negotiate it when `allowHTTP1` is set.

use super::{class, closure, get_slot, put_slot, Class, Slot};
use crate::modules::host_members::{captured, constructor, current, members, methods};
use crate::modules::stream_shared_vm::{
    call_method, emit, get, is_undefined, on, once, put, put_number, remove_listener, truthy_member, Context, Outcome,
};
use quench_runtime::{RootId, RootedError};

/// Node's default `unknownProtocolTimeout`, in milliseconds.
const DEFAULT_UNKNOWN_PROTOCOL_TIMEOUT: f64 = 10_000.0;
/// The HTTP/1 server timeouts an `allowHTTP1` secure server sets, in milliseconds.
const HTTP1_HEADERS_TIMEOUT: f64 = 60_000.0;
const HTTP1_REQUEST_TIMEOUT: f64 = 300_000.0;
const HTTP1_CONNECTIONS_CHECKING_INTERVAL: f64 = 30_000.0;
/// The ALPN identifiers the secure server offers.
const ALPN_H2: &str = "h2";
const ALPN_HTTP1: &str = "http/1.1";
/// What a server answers a client that negotiated no HTTP/2.
const UNKNOWN_PROTOCOL_RESPONSE: &str = "HTTP/1.0 403 Forbidden\r\nContent-Type: text/plain\r\n\r\nMissing ALPN Protocol, expected `h2` to be available.\nIf this is a HTTP request: The server was not configured with the `allowHTTP1` option or a listener for the `unknownProtocol` event.\n";
/// The status `captureRejections` answers a failed request with.
const STATUS_INTERNAL_SERVER_ERROR: f64 = 500.0;

members!(Member, "http2Server", [
    Construct,
    SecureConstruct,
    SetTimeout,
    UpdateSettings,
    SecureClose,
    CloseIdleConnections,
    AsyncDispose,
    /// The `close` callback settling `[Symbol.asyncDispose]()`'s promise.
    Disposed,
    CaptureRejection,
    /// `connectionListener(socket)`.
    Connection,
    /// `setupCompat(ev)`, then `onServerStream` bound to the compatibility classes.
    SetupCompat,
    ServerStream,
    /// The session listeners forwarding to the server.
    SessionStream,
    SessionError,
    SessionTimeout,
    /// `onErrorSecureServerSession(err, socket)`.
    TlsClientError,
    /// The unknown-protocol socket timeout and its clearing on `close`.
    UnknownProtocolTimeout,
    ClearUnknownProtocolTimeout,
    /// The default `shouldUpgradeCallback` of an `allowHTTP1` server.
    ShouldUpgrade,
]);

pub(super) fn build(c: &mut Context<'_>) -> Result<(), RootedError> {
    let net = crate::host::shared_vm::commonjs::net_module(c)?;
    let net_server = get(c, net, "Server")?;
    let server = constructor(c, Member::Construct, "Http2Server")?;
    crate::modules::http_shared_vm::inherit(c, server, net_server)?;
    let prototype = get(c, server, "prototype")?;
    methods(c, prototype, &[("setTimeout", Member::SetTimeout), ("updateSettings", Member::UpdateSettings)])?;
    let dispose = crate::modules::stream_shared_vm::utils::registered_symbol(c, "nodejs.asyncDispose")?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let async_dispose = get(c, symbol, "asyncDispose")?;
    let dispose = if is_undefined(c, async_dispose) { dispose } else { async_dispose };
    let disposer = closure(c, Member::AsyncDispose, &[])?;
    c.define_data_property_rooted(prototype, dispose, disposer, true, false, true)?;
    let capture = crate::modules::stream_shared_vm::utils::registered_symbol(c, "nodejs.rejection")?;
    let rejection = closure(c, Member::CaptureRejection, &[])?;
    c.define_data_property_rooted(prototype, capture, rejection, true, false, true)?;
    super::set_class(c, Class::Http2Server, server)?;

    let tls = crate::host::shared_vm::commonjs::tls_module(c)?;
    let tls_server = get(c, tls, "Server")?;
    let secure = constructor(c, Member::SecureConstruct, "Http2SecureServer")?;
    crate::modules::http_shared_vm::inherit(c, secure, tls_server)?;
    let prototype = get(c, secure, "prototype")?;
    methods(c, prototype, &[
        ("setTimeout", Member::SetTimeout),
        ("updateSettings", Member::UpdateSettings),
        ("close", Member::SecureClose),
        ("closeIdleConnections", Member::CloseIdleConnections),
    ])?;
    super::set_class(c, Class::Http2SecureServer, secure)?;
    let setup = closure(c, Member::SetupCompat, &[])?;
    super::set_class(c, Class::SetupCompat, setup)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::Construct => {
            let options = initialize_options(c, at(0))?;
            let net = crate::host::shared_vm::commonjs::net_module(c)?;
            let net_server = get(c, net, "Server")?;
            let connection = closure(c, Member::Connection, &[])?;
            c.call_rooted(net_server, this, &[options, connection])?;
            initialize(c, this, options, at(1))?;
            Ok(undefined)
        }
        Member::SecureConstruct => {
            let options = tls_options(c, at(0), None)?;
            let tls = crate::host::shared_vm::commonjs::tls_module(c)?;
            let tls_server = get(c, tls, "Server")?;
            let connection = closure(c, Member::Connection, &[])?;
            c.call_rooted(tls_server, this, &[options, connection])?;
            initialize(c, this, options, at(1))?;
            if allow_http1(c, this)? {
                let http_options = c.object_rooted()?;
                let incoming = get(c, options, "Http1IncomingMessage")?;
                put(c, http_options, "IncomingMessage", incoming)?;
                let response = get(c, options, "Http1ServerResponse")?;
                put(c, http_options, "ServerResponse", response)?;
                crate::modules::http_shared_vm::server::store_options(c, this, http_options)?;
                put_number(c, this, "headersTimeout", HTTP1_HEADERS_TIMEOUT)?;
                put_number(c, this, "requestTimeout", HTTP1_REQUEST_TIMEOUT)?;
                put_number(c, this, "connectionsCheckingInterval", HTTP1_CONNECTIONS_CHECKING_INTERVAL)?;
                let should_upgrade = closure(c, Member::ShouldUpgrade, &[])?;
                put(c, this, "shouldUpgradeCallback", should_upgrade)?;
            }
            let client_error = closure(c, Member::TlsClientError, &[])?;
            on(c, this, "tlsClientError", client_error)?;
            Ok(undefined)
        }
        Member::SetTimeout => {
            let (msecs, callback) = (at(0), at(1));
            put(c, this, "timeout", msecs)?;
            if !is_undefined(c, callback) {
                super::validate_function(c, callback, "callback")?;
                on(c, this, "timeout", callback)?;
            }
            Ok(this)
        }
        Member::UpdateSettings => {
            let settings = at(0);
            super::assert_is_object(c, settings, "settings", "Object")?;
            super::settings::validate(c, settings)?;
            let options = get_slot(c, this, Slot::Options)?;
            let merged = c.object_rooted()?;
            let current = get(c, options, "settings")?;
            for source in [current, settings] {
                if c.is_object_rooted(source)? {
                    for key in c.enumerable_own_keys_rooted(source, true, true)? {
                        let value = c.get_property_rooted(source, key)?;
                        c.set_property_rooted(merged, key, value, merged)?;
                    }
                }
            }
            put(c, options, "settings", merged)?;
            Ok(undefined)
        }
        Member::SecureClose => {
            if allow_http1(c, this)? {
                crate::modules::http_shared_vm::server::pre_close(c, this)?;
            }
            let tls = crate::host::shared_vm::commonjs::tls_module(c)?;
            let tls_server = get(c, tls, "Server")?;
            let prototype = get(c, tls_server, "prototype")?;
            let close = get(c, prototype, "close")?;
            c.call_rooted(close, this, args)
        }
        Member::CloseIdleConnections => {
            if allow_http1(c, this)? {
                let http = crate::modules::http_shared_vm::module(c)?;
                let server = get(c, http, "Server")?;
                let prototype = get(c, server, "prototype")?;
                let close = get(c, prototype, "closeIdleConnections")?;
                c.call_rooted(close, this, args)?;
            }
            Ok(undefined)
        }
        Member::AsyncDispose => {
            let promise = c.pending_promise_rooted()?;
            let done = closure(c, Member::Disposed, &[promise])?;
            let net = crate::host::shared_vm::commonjs::net_module(c)?;
            let net_server = get(c, net, "Server")?;
            let prototype = get(c, net_server, "prototype")?;
            let close = get(c, prototype, "close")?;
            c.call_rooted(close, this, &[done])?;
            Ok(promise)
        }
        Member::Disposed => {
            let [promise] = captured(c)?;
            let error = at(0);
            if c.truthy_rooted(error)? {
                c.reject_promise_rooted(promise, error)?;
            } else {
                c.resolve_promise_rooted(promise, undefined)?;
            }
            Ok(undefined)
        }
        Member::CaptureRejection => {
            capture_rejection(c, this, args)?;
            Ok(undefined)
        }
        Member::Connection => {
            connection(c, this, at(0))?;
            Ok(undefined)
        }
        Member::SetupCompat => {
            if c.string_text(at(0))?.as_deref() == Some("request") {
                let setup = class(c, Class::SetupCompat)?;
                remove_listener(c, this, "newListener", setup)?;
                let options = get_slot(c, this, Slot::Options)?;
                let request = get(c, options, "Http2ServerRequest")?;
                let response = get(c, options, "Http2ServerResponse")?;
                let listener = closure(c, Member::ServerStream, &[this, request, response])?;
                on(c, this, "stream", listener)?;
            }
            Ok(undefined)
        }
        Member::ServerStream => {
            let [server, request, response] = captured(c)?;
            super::compat::on_server_stream(c, server, request, response, at(0), at(1), at(3))?;
            Ok(undefined)
        }
        Member::SessionStream => {
            let server = get_slot(c, this, Slot::Server)?;
            if !is_undefined(c, server) {
                let event = c.string_rooted("stream");
                let mut values = vec![event];
                values.extend_from_slice(args);
                call_method(c, server, "emit", &values)?;
            }
            Ok(undefined)
        }
        Member::SessionError => {
            let server = get_slot(c, this, Slot::Server)?;
            if !is_undefined(c, server) {
                emit(c, server, "sessionError", &[at(0), this])?;
            }
            Ok(undefined)
        }
        Member::SessionTimeout => {
            if truthy_member(c, this, "destroyed")? || truthy_member(c, this, "closed")? {
                return Ok(undefined);
            }
            let server = get_slot(c, this, Slot::Server)?;
            if !emit(c, server, "timeout", &[this])? {
                call_method(c, this, "destroy", &[])?;
            }
            Ok(undefined)
        }
        Member::TlsClientError => {
            let (error, socket) = (at(0), at(1));
            if !emit(c, this, "clientError", &[error, socket])? {
                call_method(c, socket, "destroy", &[error])?;
            }
            Ok(undefined)
        }
        Member::UnknownProtocolTimeout => {
            let [socket] = captured(c)?;
            if !truthy_member(c, socket, "destroyed")? {
                call_method(c, socket, "destroy", &[])?;
            }
            Ok(undefined)
        }
        Member::ClearUnknownProtocolTimeout => {
            let [timer] = captured(c)?;
            crate::modules::timers_shared_vm::clear_timer(c, undefined, &[timer])?;
            Ok(undefined)
        }
        Member::ShouldUpgrade => {
            let count = crate::modules::stream_shared_vm::listener_count(c, this, "upgrade")?;
            Ok(c.boolean(count > 0.0))
        }
    }
}

fn allow_http1(c: &mut Context<'_>, server: RootId) -> Result<bool, RootedError> {
    let options = get_slot(c, server, Slot::Options)?;
    if !c.is_object_rooted(options)? {
        return Ok(false);
    }
    let allow = get(c, options, "allowHTTP1")?;
    Ok(c.rooted_value(allow).and_then(|value| value.as_bool()) == Some(true))
}

/// The construction both server classes share after their base class.
fn initialize(c: &mut Context<'_>, server: RootId, options: RootId, listener: RootId) -> Result<(), RootedError> {
    put_slot(c, server, Slot::Options, options)?;
    put_number(c, server, "timeout", 0.0)?;
    let setup = class(c, Class::SetupCompat)?;
    on(c, server, "newListener", setup)?;
    if c.is_callable_rooted(listener)? {
        on(c, server, "request", listener)?;
    }
    Ok(())
}

/// `initializeOptions(options)`.
pub(super) fn initialize_options(c: &mut Context<'_>, given: RootId) -> Outcome {
    super::assert_is_object(c, given, "options", "Object")?;
    let options = c.object_rooted()?;
    if c.is_object_rooted(given)? {
        for key in c.enumerable_own_keys_rooted(given, true, true)? {
            let value = c.get_property_rooted(given, key)?;
            c.set_property_rooted(options, key, value, options)?;
        }
    }
    let settings = get(c, options, "settings")?;
    super::assert_is_object(c, settings, "options.settings", "Object")?;
    let copy = c.object_rooted()?;
    if c.is_object_rooted(settings)? {
        for key in c.enumerable_own_keys_rooted(settings, true, true)? {
            let value = c.get_property_rooted(settings, key)?;
            c.set_property_rooted(copy, key, value, copy)?;
        }
    }
    put(c, options, "settings", copy)?;
    let remote = get(c, options, "remoteCustomSettings")?;
    if !is_undefined(c, remote) && !c.is_array_rooted(remote)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, remote, "options.remoteCustomSettings", "Array")?);
    }
    for name in ["maxSessionInvalidFrames", "maxSessionRejectedStreams"] {
        let value = get(c, options, name)?;
        if !is_undefined(c, value) {
            super::validate_integer(c, value, name, 0.0, super::MAX_INT)?;
        }
    }
    let timeout = get(c, options, "unknownProtocolTimeout")?;
    if is_undefined(c, timeout) {
        put_number(c, options, "unknownProtocolTimeout", DEFAULT_UNKNOWN_PROTOCOL_TIMEOUT)?;
    } else {
        super::validate_integer(c, timeout, "unknownProtocolTimeout", 0.0, super::MAX_INT)?;
    }
    let http = crate::modules::http_shared_vm::module(c)?;
    for (name, source, export) in [
        ("Http1IncomingMessage", Some(http), "IncomingMessage"),
        ("Http1ServerResponse", Some(http), "ServerResponse"),
        ("Http2ServerRequest", None, "Http2ServerRequest"),
        ("Http2ServerResponse", None, "Http2ServerResponse"),
    ] {
        let value = get(c, options, name)?;
        if c.truthy_rooted(value)? {
            continue;
        }
        let default = match source {
            Some(module) => get(c, module, export)?,
            None => class(c, if export == "Http2ServerRequest" { Class::Http2ServerRequest } else { Class::Http2ServerResponse })?,
        };
        put(c, options, name, default)?;
    }
    Ok(options)
}

/// `initializeTLSOptions(options, servername)`.
pub(super) fn tls_options(c: &mut Context<'_>, given: RootId, servername: Option<&str>) -> Outcome {
    let options = initialize_options(c, given)?;
    if !truthy_member(c, options, "ALPNCallback")? {
        let mut protocols = vec![c.string_rooted(ALPN_H2)];
        let allow = get(c, options, "allowHTTP1")?;
        if c.rooted_value(allow).and_then(|value| value.as_bool()) == Some(true) {
            protocols.push(c.string_rooted(ALPN_HTTP1));
        }
        let protocols = c.array_rooted(&protocols)?;
        put(c, options, "ALPNProtocols", protocols)?;
    }
    if let Some(servername) = servername {
        if !truthy_member(c, options, "servername")? {
            let servername = c.string_rooted(servername);
            put(c, options, "servername", servername)?;
        }
    }
    Ok(options)
}

/// `connectionListener(socket)`: `this` is the server.
fn connection(c: &mut Context<'_>, server: RootId, socket: RootId) -> Result<(), RootedError> {
    let options = get_slot(c, server, Slot::Options)?;
    let options = if c.is_object_rooted(options)? { options } else { c.object_rooted()? };
    let alpn = get(c, socket, "alpnProtocol")?;
    let http1 = c.rooted_value(alpn).and_then(|value| value.as_bool()) == Some(false)
        || c.string_text(alpn)?.as_deref() == Some(ALPN_HTTP1);
    if http1 {
        if allow_http1(c, server)? {
            crate::modules::http_shared_vm::server::connection_listener(c, server, &[socket])?;
            return Ok(());
        }
        if !emit(c, server, "unknownProtocol", &[socket])? {
            let fire = closure(c, Member::UnknownProtocolTimeout, &[socket])?;
            let delay = get(c, options, "unknownProtocolTimeout")?;
            let undefined = c.undefined();
            let timer = crate::modules::timers_shared_vm::set_timeout(c, undefined, &[fire, delay])?;
            call_method(c, timer, "unref", &[])?;
            let clear = closure(c, Member::ClearUnknownProtocolTimeout, &[timer])?;
            once(c, socket, "close", clear)?;
            let response = c.string_rooted(UNKNOWN_PROTOCOL_RESPONSE);
            call_method(c, socket, "end", &[response])?;
        }
        return Ok(());
    }
    let class = class(c, Class::ServerHttp2Session)?;
    let session = c.construct_rooted(class, class, &[options, socket, server])?;
    let stream = closure(c, Member::SessionStream, &[])?;
    on(c, session, "stream", stream)?;
    let error = closure(c, Member::SessionError, &[])?;
    on(c, session, "error", error)?;
    let timeout = get(c, server, "timeout")?;
    if c.truthy_rooted(timeout)? {
        let listener = closure(c, Member::SessionTimeout, &[])?;
        call_method(c, session, "setTimeout", &[timeout, listener])?;
    }
    put_slot(c, socket, Slot::Server, server)?;
    emit(c, server, "session", &[session]).map(drop)
}

/// `Http2Server.prototype[EventEmitter.captureRejectionSymbol](err, event, ...args)`.
fn capture_rejection(c: &mut Context<'_>, server: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    let (error, event) = (at(0), at(1));
    match c.string_text(event)?.as_deref() {
        Some("stream") => {
            let stream = at(2);
            let sent = get(c, stream, "sentHeaders")?;
            if c.truthy_rooted(sent)? {
                call_method(c, stream, "destroy", &[error])?;
            } else {
                let headers = c.object_rooted()?;
                put_number(c, headers, super::headers::STATUS, STATUS_INTERNAL_SERVER_ERROR)?;
                call_method(c, stream, "respond", &[headers])?;
                call_method(c, stream, "end", &[])?;
            }
        }
        Some("request") => {
            let response = at(3);
            if !truthy_member(c, response, "headersSent")? && !truthy_member(c, response, "finished")? {
                let names = call_method(c, response, "getHeaderNames", &[])?;
                for name in c.array_like_values_rooted(names)? {
                    call_method(c, response, "removeHeader", &[name])?;
                }
                put_number(c, response, "statusCode", STATUS_INTERNAL_SERVER_ERROR)?;
                let codes = crate::modules::http_status_shared_vm::codes(c)?;
                let text = get(c, codes, "500")?;
                call_method(c, response, "end", &[text])?;
            } else {
                call_method(c, response, "destroy", &[])?;
            }
        }
        _ => {
            let net = crate::host::shared_vm::commonjs::net_module(c)?;
            let net_server = get(c, net, "Server")?;
            let prototype = get(c, net_server, "prototype")?;
            let capture = crate::modules::stream_shared_vm::utils::registered_symbol(c, "nodejs.rejection")?;
            let base = c.get_property_rooted(prototype, capture)?;
            if c.is_callable_rooted(base)? {
                c.call_rooted(base, server, args)?;
            }
        }
    }
    Ok(())
}
