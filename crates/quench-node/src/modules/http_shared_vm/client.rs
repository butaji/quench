//! `ClientRequest` (lib/_http_client.js): an `OutgoingMessage` that renders a request head,
//! takes a socket from its agent (or a connection of its own), and reads the response with
//! a response parser, handing a reusable socket back to the agent.

use super::outgoing;
use super::parser::{is_informational, HeadOutcome, Kind};
use super::parsing::{self, Executed};
use super::{captured, class, current, function, get_slot, put_slot, throw_coded, Class, ErrorClass, Slot};
use crate::modules::stream_shared_vm::{
    arg, call_method, emit, get, is_nullish, is_string, is_undefined, listener_count, next_tick, number, on, once,
    put, put_bool, remove_listener, truthy_member, Context, Outcome,
};
use quench_runtime::{RootId, RootedError};

super::members!(Member, "httpClient", [
    Construct,
    Finish,
    ImplicitHeader,
    Abort,
    Destroy,
    OnSocket,
    DeferToConnect,
    SetTimeout,
    SetNoDelay,
    SetSocketKeepAlive,
    ClearTimeout,
    /// `emitAbortNT(req)`.
    EmitAbort,
    /// `onSocketNT(req, socket, err)`.
    OnSocketTick,
    /// `_destroy(req, err)` once a socket destroyed for a destroyed request finishes.
    SocketDestroyed,
    /// The socket listeners a request installs: `socketCloseListener`,
    /// `socketErrorListener`, `socketOnEnd`, `socketOnData` and `ondrain`.
    SocketClose,
    SocketError,
    SocketEnd,
    SocketData,
    SocketDrain,
    /// `responseOnTimeout` and `emitRequestTimeout`, on the socket.
    ResponseTimeout,
    RequestTimeout,
    /// `responseOnEnd`, on the response; `requestOnFinish`, on the request.
    ResponseEnd,
    RequestFinish,
    /// `emitFreeNT(req)`.
    EmitFree,
    /// `listenSocketTimeout`'s `socket` listener.
    ListenSocketTimeout,
    /// `setSocketTimeout(sock, msecs)`, once a socket is assigned or connects.
    SocketSetTimeout,
    /// A deferred socket call (`_deferToConnect`).
    DeferredCall,
    /// The `createConnection` callback, called at most once.
    ConnectionCreated,
]);

/// The methods whose requests carry no body by default.
const BODYLESS_METHODS: [&str; 6] = ["GET", "HEAD", "DELETE", "OPTIONS", "TRACE", "CONNECT"];
/// The port a request defaults to without an agent's.
const DEFAULT_PORT: f64 = 80.0;
/// The largest timer duration (`TIMEOUT_MAX`).
const TIMEOUT_MAX: f64 = 2_147_483_647.0;
/// The bytes a request path may hold (`INVALID_PATH_REGEX` rejects the rest).
const PATH_BYTES: std::ops::RangeInclusive<u16> = 0x21..=0xff;

pub(crate) fn build(c: &mut Context<'_>, outgoing: RootId) -> Outcome {
    let class = super::constructor(c, Member::Construct, "ClientRequest")?;
    super::inherit(c, class, outgoing)?;
    let prototype = get(c, class, "prototype")?;
    super::methods(c, prototype, &[
        ("_finish", Member::Finish),
        ("_implicitHeader", Member::ImplicitHeader),
        ("abort", Member::Abort),
        ("destroy", Member::Destroy),
        ("onSocket", Member::OnSocket),
        ("_deferToConnect", Member::DeferToConnect),
        ("setTimeout", Member::SetTimeout),
        ("setNoDelay", Member::SetNoDelay),
        ("setSocketKeepAlive", Member::SetSocketKeepAlive),
        ("clearTimeout", Member::ClearTimeout),
    ])?;
    for (which, member, name) in [
        (Class::ClientSocketClose, Member::SocketClose, "socketCloseListener"),
        (Class::ClientSocketError, Member::SocketError, "socketErrorListener"),
        (Class::ClientSocketEnd, Member::SocketEnd, "socketOnEnd"),
        (Class::ClientSocketData, Member::SocketData, "socketOnData"),
        (Class::ClientSocketDrain, Member::SocketDrain, "ondrain"),
        (Class::ClientResponseTimeout, Member::ResponseTimeout, "responseOnTimeout"),
        (Class::ClientRequestTimeout, Member::RequestTimeout, "emitRequestTimeout"),
        (Class::ClientResponseEnd, Member::ResponseEnd, "responseOnEnd"),
        (Class::ClientRequestFinish, Member::RequestFinish, "requestOnFinish"),
    ] {
        let listener = function(c, member, name, &[])?;
        super::set_class(c, which, listener)?;
    }
    Ok(class)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::Construct => {
            construct(c, this, args)?;
            Ok(c.undefined())
        }
        Member::Finish => {
            emit(c, this, "prefinish", &[])?;
            Ok(c.undefined())
        }
        Member::ImplicitHeader => {
            implicit_header(c, this)?;
            Ok(c.undefined())
        }
        Member::Abort => {
            if !truthy_member(c, this, "aborted")? {
                put_bool(c, this, "aborted", true)?;
                let abort = function(c, Member::EmitAbort, "emitAbortNT", &[this])?;
                next_tick(c, abort, &[])?;
                call_method(c, this, "destroy", &[])?;
            }
            Ok(c.undefined())
        }
        Member::Destroy => {
            if truthy_member(c, this, "destroyed")? {
                return Ok(this);
            }
            let error = at(0);
            put_bool(c, this, "destroyed", true)?;
            let response = get(c, this, "res")?;
            if c.truthy_rooted(response)? {
                call_method(c, response, "_dump", &[])?;
            }
            put_slot(c, this, Slot::Error, error)?;
            let socket = outgoing::socket(c, this)?;
            if !is_nullish(c, socket) {
                call_method(c, socket, "destroy", &[error])?;
            }
            Ok(this)
        }
        Member::OnSocket => {
            let (socket, error) = (at(0), at(1));
            let tick = function(c, Member::OnSocketTick, "onSocketNT", &[this, socket, error])?;
            next_tick(c, tick, &[])?;
            Ok(c.undefined())
        }
        Member::DeferToConnect => {
            let (method, values) = (at(0), at(1));
            defer_to_connect(c, this, method, values)?;
            Ok(c.undefined())
        }
        Member::SetTimeout => set_timeout(c, this, at(0), at(1)),
        Member::SetNoDelay => {
            let value = at(0);
            let method = c.string_rooted("setNoDelay");
            let values = c.array_rooted(&[value])?;
            defer_to_connect(c, this, method, values)?;
            Ok(c.undefined())
        }
        Member::SetSocketKeepAlive => {
            let (enable, delay) = (at(0), at(1));
            let method = c.string_rooted("setKeepAlive");
            let values = c.array_rooted(&[enable, delay])?;
            defer_to_connect(c, this, method, values)?;
            Ok(c.undefined())
        }
        Member::ClearTimeout => {
            let zero = c.number(0.0);
            set_timeout(c, this, zero, at(0))?;
            Ok(c.undefined())
        }
        Member::EmitAbort => {
            let [request] = captured(c)?;
            emit(c, request, "abort", &[])?;
            Ok(c.undefined())
        }
        Member::OnSocketTick => {
            let [request, socket, error] = captured(c)?;
            on_socket_tick(c, request, socket, error)?;
            Ok(c.undefined())
        }
        Member::SocketDestroyed => {
            let [request, error] = captured(c)?;
            let mut failure = at(0);
            if crate::modules::stream_shared_vm::errors::has_code(c, failure, "ERR_STREAM_PREMATURE_CLOSE")? {
                failure = c.null();
            }
            let error = if c.truthy_rooted(failure)? { failure } else { error };
            destroyed(c, request, error)?;
            Ok(c.undefined())
        }
        Member::SocketClose => {
            socket_close(c, this)?;
            Ok(c.undefined())
        }
        Member::SocketError => {
            socket_error(c, this, at(0))?;
            Ok(c.undefined())
        }
        Member::SocketEnd => {
            socket_end(c, this)?;
            Ok(c.undefined())
        }
        Member::SocketData => {
            socket_data(c, this, at(0))?;
            Ok(c.undefined())
        }
        Member::SocketDrain => {
            let message = get(c, this, "_httpMessage")?;
            if c.truthy_rooted(message)? && !truthy_member(c, message, "finished")? {
                let need = get_slot(c, message, Slot::NeedDrain)?;
                if c.truthy_rooted(need)? {
                    let no = c.boolean(false);
                    put_slot(c, message, Slot::NeedDrain, no)?;
                    emit(c, message, "drain", &[])?;
                }
            }
            Ok(c.undefined())
        }
        Member::ResponseTimeout => {
            let request = get(c, this, "_httpMessage")?;
            if c.truthy_rooted(request)? {
                let response = get(c, request, "res")?;
                if c.truthy_rooted(response)? {
                    emit(c, response, "timeout", &[])?;
                }
            }
            Ok(c.undefined())
        }
        Member::RequestTimeout => {
            let request = get(c, this, "_httpMessage")?;
            if c.truthy_rooted(request)? {
                emit(c, request, "timeout", &[])?;
            }
            Ok(c.undefined())
        }
        Member::ResponseEnd => {
            response_end(c, this)?;
            Ok(c.undefined())
        }
        Member::RequestFinish => {
            if truthy_member(c, this, "shouldKeepAlive")? && truthy_member(c, this, "_ended")? {
                response_keep_alive(c, this)?;
            }
            Ok(c.undefined())
        }
        Member::EmitFree => {
            let [request] = captured(c)?;
            put_bool(c, request, "_closed", true)?;
            emit(c, request, "close", &[])?;
            let socket = outgoing::socket(c, request)?;
            if c.truthy_rooted(socket)? {
                emit(c, socket, "free", &[])?;
            }
            Ok(c.undefined())
        }
        Member::ListenSocketTimeout => {
            let socket = at(0);
            let listener = class(c, Class::ClientRequestTimeout)?;
            once(c, socket, "timeout", listener)?;
            Ok(c.undefined())
        }
        Member::SocketSetTimeout => {
            let [msecs, known] = captured(c)?;
            let socket = if is_undefined(c, known) { at(0) } else { known };
            set_socket_timeout(c, socket, msecs)?;
            Ok(c.undefined())
        }
        Member::DeferredCall => {
            let [socket, method, values] = captured(c)?;
            call_socket_method(c, socket, method, values)?;
            Ok(c.undefined())
        }
        Member::ConnectionCreated => {
            let [request, called] = captured(c)?;
            if truthy_member(c, called, "0")? {
                return Ok(c.undefined());
            }
            put_bool(c, called, "0", true)?;
            let (error, socket) = (at(0), at(1));
            if c.truthy_rooted(error)? {
                let nop = crate::modules::stream_shared_vm::intrinsics(c)?.nop;
                let emit_error = function(c, super::outgoing::Member::EmitError, "", &[request, error, nop])?;
                next_tick(c, emit_error, &[])?;
            } else {
                call_method(c, request, "onSocket", &[socket])?;
            }
            Ok(c.undefined())
        }
    }
}

/// `validateHost(host, name)`.
fn validate_host(c: &mut Context<'_>, host: RootId, name: &str) -> Outcome {
    if !is_nullish(c, host) && !is_string(c, host) {
        return Err(match crate::modules::util_shared_vm::argument_type_error_with_expected(
            c,
            host,
            &format!("options.{name}"),
            "of type string or one of type undefined or null",
        ) {
            Ok(error) | Err(error) => error,
        });
    }
    Ok(host)
}

/// `getTimerDuration(msecs, name)`.
fn timer_duration(c: &mut Context<'_>, msecs: RootId, name: &str) -> Result<f64, RootedError> {
    let Some(value) = c.rooted_value(msecs).and_then(|value| value.as_number()) else {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, msecs, name, "number") {
            Ok(error) | Err(error) => error,
        });
    };
    if value < 0.0 || !value.is_finite() {
        let received = crate::modules::util_shared_vm::range_error_received(c, msecs)?;
        return Err(throw_coded(
            c,
            ErrorClass::RangeError,
            "ERR_OUT_OF_RANGE",
            &format!("The value of \"{name}\" is out of range. It must be a non-negative finite number. Received {received}"),
        ));
    }
    Ok(value.min(TIMEOUT_MAX))
}

/// The request options from `(input, options, cb)`: a URL string or object, options, or
/// both merged.
fn request_options(c: &mut Context<'_>, args: &[RootId]) -> Result<(RootId, RootId), RootedError> {
    let (mut input, mut options, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if is_string(c, input) {
        let global = c.global_root()?;
        let url_class = get(c, global, "URL")?;
        let url = c.construct_rooted(url_class, url_class, &[input])?;
        let undefined = c.undefined();
        input = crate::modules::url_shared_vm::url_to_http_options(c, undefined, &[url])?;
    } else if crate::modules::url_shared_vm::is_url_like(c, input)? {
        let undefined = c.undefined();
        input = crate::modules::url_shared_vm::url_to_http_options(c, undefined, &[input])?;
    } else {
        callback = options;
        options = input;
        input = c.null();
    }
    if c.is_callable_rooted(options)? {
        callback = options;
        options = if is_nullish(c, input) { c.object_rooted()? } else { input };
    } else {
        let target = if is_nullish(c, input) { c.object_rooted()? } else { input };
        if !is_nullish(c, options) {
            for key in c.enumerable_own_keys_rooted(options, true, true)? {
                let value = c.get_property_rooted(options, key)?;
                c.set_property_rooted(target, key, value, target)?;
            }
        }
        options = target;
    }
    Ok((options, callback))
}

/// `new ClientRequest(input, options, cb)`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let undefined = c.undefined();
    outgoing::initialize(c, this, undefined)?;
    let (options, callback) = request_options(c, args)?;
    let mut agent = get(c, options, "agent")?;
    let default_agent = get(c, options, "_defaultAgent")?;
    let default_agent = if c.truthy_rooted(default_agent)? { default_agent } else { class(c, Class::GlobalAgent)? };
    let create_connection = get(c, options, "createConnection")?;
    if c.rooted_value(agent).and_then(|value| value.as_bool()) == Some(false) {
        let constructor = get(c, default_agent, "constructor")?;
        agent = c.construct_rooted(constructor, constructor, &[])?;
    } else if is_nullish(c, agent) {
        if !c.is_callable_rooted(create_connection)? {
            agent = default_agent;
        }
    } else {
        let add_request = get(c, agent, "addRequest")?;
        if !c.is_callable_rooted(add_request)? {
            return Err(match crate::modules::util_shared_vm::argument_type_error_with_expected(
                c,
                agent,
                "options.agent",
                "of type Agent-like Object, undefined, or false",
            ) {
                Ok(error) | Err(error) => error,
            });
        }
    }
    put(c, this, "agent", agent)?;
    let has_agent = c.truthy_rooted(agent)?;
    let protocol = get(c, options, "protocol")?;
    let default_protocol = get(c, default_agent, "protocol")?;
    let protocol = if c.truthy_rooted(protocol)? { protocol } else { default_protocol };
    let agent_protocol = if has_agent { get(c, agent, "protocol")? } else { c.undefined() };
    let expected = if c.truthy_rooted(agent_protocol)? { agent_protocol } else { default_protocol };
    let path = get(c, options, "path")?;
    if c.truthy_rooted(path)? {
        let units = c.to_string_code_units(path)?;
        if units.iter().any(|unit| !PATH_BYTES.contains(unit)) {
            return Err(throw_coded(c, ErrorClass::TypeError, "ERR_UNESCAPED_CHARACTERS", "Request path contains unescaped characters"));
        }
    }
    if !c.strict_equal_rooted(protocol, expected)? {
        let message = super::format(c, "Protocol \"%s\" not supported. Expected \"%s\"", &[protocol, expected])?;
        return Err(throw_coded(c, ErrorClass::TypeError, "ERR_INVALID_PROTOCOL", &message));
    }
    let default_port = get(c, options, "defaultPort")?;
    let default_port = if c.truthy_rooted(default_port)? {
        default_port
    } else if has_agent {
        get(c, agent, "defaultPort")?
    } else {
        c.undefined()
    };
    let connect_options = c.null_object_rooted()?;
    for key in c.enumerable_own_keys_rooted(options, true, true)? {
        let value = c.get_property_rooted(options, key)?;
        c.set_property_rooted(connect_options, key, value, connect_options)?;
    }
    let port = get(c, options, "port")?;
    let port = if c.truthy_rooted(port)? {
        port
    } else if c.truthy_rooted(default_port)? {
        default_port
    } else {
        c.number(DEFAULT_PORT)
    };
    put(c, connect_options, "port", port)?;
    let hostname = get(c, options, "hostname")?;
    let hostname = validate_host(c, hostname, "hostname")?;
    let host_option = get(c, options, "host")?;
    let host = if c.truthy_rooted(hostname)? {
        hostname
    } else {
        let host = validate_host(c, host_option, "host")?;
        if c.truthy_rooted(host)? { host } else { c.string_rooted("localhost") }
    };
    put(c, connect_options, "host", host)?;
    let set_host = get(c, options, "setHost")?;
    let set_host = is_undefined(c, set_host) || c.truthy_rooted(set_host)?;
    let socket_path = get(c, options, "socketPath")?;
    put(c, this, "socketPath", socket_path)?;
    let timeout = get(c, options, "timeout")?;
    if !is_undefined(c, timeout) {
        let duration = timer_duration(c, timeout, "timeout")?;
        let duration = c.number(duration);
        put(c, this, "timeout", duration)?;
    }
    let signal = get(c, options, "signal")?;
    if c.truthy_rooted(signal)? {
        let stream_module = crate::modules::stream_shared_vm::module(c)?;
        call_method(c, stream_module, "addAbortSignal", &[signal, this])?;
        let key = c.string_rooted("signal");
        c.delete_property_rooted(connect_options, key)?;
    }
    let method = get(c, options, "method")?;
    let method_text = if is_nullish(c, method) {
        None
    } else if is_string(c, method) {
        c.string_text(method)?.filter(|method| !method.is_empty())
    } else {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, method, "options.method", "string") {
            Ok(error) | Err(error) => error,
        });
    };
    let method_text = match method_text {
        Some(text) => {
            if !text.chars().all(crate::modules::http_protocol::is_http_token_char) {
                let label = c.string_rooted("Method");
                let message = super::format(c, "%s must be a valid HTTP token [\"%s\"]", &[label, method])?;
                return Err(throw_coded(c, ErrorClass::TypeError, "ERR_INVALID_HTTP_TOKEN", &message));
            }
            text.to_uppercase()
        }
        None => "GET".to_owned(),
    };
    let method_value = c.string_rooted(&method_text);
    put(c, this, "method", method_value)?;
    let max_header_size = get(c, options, "maxHeaderSize")?;
    if !is_undefined(c, max_header_size) {
        let maximum = c.number(f64::from(u32::MAX));
        crate::modules::util_shared_vm::validate_integer(c, max_header_size, "maxHeaderSize", 0.0, maximum)?;
    }
    put(c, this, "maxHeaderSize", max_header_size)?;
    let insecure = get(c, options, "insecureHTTPParser")?;
    if !is_undefined(c, insecure) && c.rooted_value(insecure).and_then(|value| value.as_bool()).is_none() {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, insecure, "options.insecureHTTPParser", "boolean") {
            Ok(error) | Err(error) => error,
        });
    }
    put(c, this, "insecureHTTPParser", insecure)?;
    let join = get(c, options, "joinDuplicateHeaders")?;
    if !is_undefined(c, join) && c.rooted_value(join).and_then(|value| value.as_bool()).is_none() {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, join, "options.joinDuplicateHeaders", "boolean") {
            Ok(error) | Err(error) => error,
        });
    }
    put(c, this, "joinDuplicateHeaders", join)?;
    let path = if c.truthy_rooted(path)? { path } else { c.string_rooted("/") };
    put(c, this, "path", path)?;
    if c.truthy_rooted(callback)? {
        once(c, this, "response", callback)?;
    }
    put_bool(c, this, "useChunkedEncodingByDefault", !BODYLESS_METHODS.contains(&method_text.as_str()))?;
    put_bool(c, this, "_ended", false)?;
    let null = c.null();
    put(c, this, "res", null)?;
    put_bool(c, this, "aborted", false)?;
    put(c, this, "timeoutCb", null)?;
    put_bool(c, this, "upgradeOrConnect", false)?;
    put(c, this, "parser", null)?;
    put(c, this, "maxHeadersCount", null)?;
    put_bool(c, this, "reusedSocket", false)?;
    put(c, this, "host", host)?;
    put(c, this, "protocol", protocol)?;
    if has_agent {
        let keep_alive = truthy_member(c, agent, "keepAlive")?;
        let max_sockets = number(c, agent, "maxSockets")?;
        let reusable = keep_alive || max_sockets.is_finite();
        put_bool(c, this, "_last", !reusable)?;
        put_bool(c, this, "shouldKeepAlive", reusable)?;
    }
    let headers = get(c, options, "headers")?;
    if c.is_array_rooted(headers)? {
        let first_line = format!("{method_text} {} HTTP/1.1\r\n", c.to_string(path)?);
        outgoing::store_header(c, this, first_line, headers)?;
    } else {
        if c.truthy_rooted(headers)? {
            for key in c.enumerable_own_keys_rooted(headers, false, true)? {
                let value = c.get_property_rooted(headers, key)?;
                outgoing::set_header(c, this, key, value)?;
            }
        }
        let existing_host = header(c, this, "host")?;
        if c.truthy_rooted(host)? && !c.truthy_rooted(existing_host)? && set_host {
            let mut host_header = c.to_string(host)?;
            if let Some(colon) = host_header.find(':') {
                if host_header[colon + 1..].contains(':') && !host_header.starts_with('[') {
                    host_header = format!("[{host_header}]");
                }
            }
            let port_number = c.to_number(port)?;
            let default_number = if c.truthy_rooted(default_port)? { c.to_number(default_port)? } else { f64::NAN };
            if c.truthy_rooted(port)? && port_number != default_number {
                host_header.push(':');
                host_header.push_str(&c.to_string(port)?);
            }
            let name = c.string_rooted("Host");
            let value = c.string_rooted(&host_header);
            outgoing::set_header(c, this, name, value)?;
        }
        let auth = get(c, options, "auth")?;
        let authorization = header(c, this, "authorization")?;
        if c.truthy_rooted(auth)? && !c.truthy_rooted(authorization)? {
            let global = c.global_root()?;
            let buffer = get(c, global, "Buffer")?;
            let bytes = call_method(c, buffer, "from", &[auth])?;
            let base64 = c.string_rooted("base64");
            let encoded = call_method(c, bytes, "toString", &[base64])?;
            let encoded = c.to_string(encoded)?;
            let name = c.string_rooted("Authorization");
            let value = c.string_rooted(&format!("Basic {encoded}"));
            outgoing::set_header(c, this, name, value)?;
        }
        let expect = header(c, this, "expect")?;
        if c.truthy_rooted(expect)? {
            if truthy_member(c, this, "_header")? {
                return Err(throw_coded(
                    c,
                    ErrorClass::Error,
                    "ERR_HTTP_HEADERS_SENT",
                    "Cannot render headers after they are sent to the client",
                ));
            }
            implicit_header(c, this)?;
        }
    }
    if has_agent {
        call_method(c, agent, "addRequest", &[this, connect_options])?;
        return Ok(());
    }
    put_bool(c, this, "_last", true)?;
    put_bool(c, this, "shouldKeepAlive", false)?;
    let path = get(c, connect_options, "path")?;
    if c.truthy_rooted(path)? || c.truthy_rooted(socket_path)? {
        let key = c.string_rooted("path");
        if c.truthy_rooted(socket_path)? {
            c.set_property_rooted(connect_options, key, socket_path, connect_options)?;
        } else {
            c.delete_property_rooted(connect_options, key)?;
        }
    }
    if c.is_callable_rooted(create_connection)? {
        let called = c.array_rooted(&[])?;
        let created = function(c, Member::ConnectionCreated, "", &[this, called])?;
        let undefined = c.undefined();
        match crate::modules::stream_shared_vm::attempt(c.call_rooted(create_connection, undefined, &[connect_options, created]))? {
            Ok(socket) => {
                if c.truthy_rooted(socket)? {
                    let null = c.null();
                    c.call_rooted(created, undefined, &[null, socket])?;
                }
            }
            Err(error) => {
                c.call_rooted(created, undefined, &[error])?;
            }
        }
    } else {
        let net = class(c, Class::Net)?;
        let socket = call_method(c, net, "createConnection", &[connect_options])?;
        call_method(c, this, "onSocket", &[socket])?;
    }
    Ok(())
}

fn header(c: &mut Context<'_>, this: RootId, name: &str) -> Outcome {
    let name = c.string_rooted(name);
    call_method(c, this, "getHeader", &[name])
}

pub(crate) fn implicit_header(c: &mut Context<'_>, this: RootId) -> Result<(), RootedError> {
    if truthy_member(c, this, "_header")? {
        return Err(throw_coded(
            c,
            ErrorClass::Error,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot render headers after they are sent to the client",
        ));
    }
    let method = get(c, this, "method")?;
    let path = get(c, this, "path")?;
    let first_line = format!("{} {} HTTP/1.1\r\n", c.to_string(method)?, c.to_string(path)?);
    let headers = get_slot(c, this, Slot::OutHeaders)?;
    outgoing::store_header(c, this, first_line, headers)
}

fn destroyed(c: &mut Context<'_>, request: RootId, error: RootId) -> Result<(), RootedError> {
    let error = if !truthy_member(c, request, "aborted")? && !c.truthy_rooted(error)? {
        super::connection_reset(c, "socket hang up")?
    } else {
        error
    };
    if c.truthy_rooted(error)? {
        emit(c, request, "error", &[error])?;
    }
    put_bool(c, request, "_closed", true)?;
    emit(c, request, "close", &[]).map(drop)
}

/// `onSocketNT(req, socket, err)`.
fn on_socket_tick(c: &mut Context<'_>, request: RootId, socket: RootId, error: RootId) -> Result<(), RootedError> {
    if truthy_member(c, request, "destroyed")? || c.truthy_rooted(error)? {
        put_bool(c, request, "destroyed", true)?;
        if c.truthy_rooted(socket)? {
            let agent = get(c, request, "agent")?;
            if !c.truthy_rooted(error)? && c.truthy_rooted(agent)? && !truthy_member(c, socket, "destroyed")? {
                emit(c, socket, "free", &[])?;
            } else {
                let cause = if c.truthy_rooted(error)? { error } else { get_slot(c, request, Slot::Error)? };
                let destroyed_socket = call_method(c, socket, "destroy", &[cause])?;
                let done = function(c, Member::SocketDestroyed, "", &[request, error])?;
                let undefined = c.undefined();
                crate::modules::stream_shared_vm::eos::eos(c, destroyed_socket, undefined, done)?;
                return Ok(());
            }
        }
        let error = if c.truthy_rooted(error)? { error } else { get_slot(c, request, Slot::Error)? };
        return destroyed(c, request, error);
    }
    tick_on_socket(c, request, socket)?;
    outgoing::flush(c, request)
}

/// `tickOnSocket(req, socket)`: the response parser and the socket listeners.
fn tick_on_socket(c: &mut Context<'_>, request: RootId, socket: RootId) -> Result<(), RootedError> {
    let max_header_size = get(c, request, "maxHeaderSize")?;
    let max_header_size = if is_nullish(c, max_header_size) { 0.0 } else { c.to_number(max_header_size)? };
    let parser = parsing::create(c, Kind::Response, socket, max_header_size)?;
    let method = get(c, request, "method")?;
    let head_request = c.string_text(method)?.as_deref() == Some("HEAD");
    parsing::set_head_request(c, parser, head_request)?;
    put(c, request, "socket", socket)?;
    put(c, parser, "outgoing", request)?;
    put(c, request, "parser", parser)?;
    put(c, socket, "parser", parser)?;
    put(c, socket, "_httpMessage", request)?;
    let max_headers = get(c, request, "maxHeadersCount")?;
    if let Some(count) = c.rooted_value(max_headers).and_then(|value| value.as_number()) {
        crate::modules::stream_shared_vm::put_number(c, parser, "maxHeaderPairs", count * 2.0)?;
    }
    let join = get(c, request, "joinDuplicateHeaders")?;
    put(c, parser, "joinDuplicateHeaders", join)?;
    for (event, listener) in [
        ("error", Class::ClientSocketError),
        ("data", Class::ClientSocketData),
        ("end", Class::ClientSocketEnd),
        ("close", Class::ClientSocketClose),
        ("drain", Class::ClientSocketDrain),
    ] {
        let listener = class(c, listener)?;
        on(c, socket, event, listener)?;
    }
    let timeout = get(c, request, "timeout")?;
    let agent = get(c, request, "agent")?;
    let agent_timeout = if c.truthy_rooted(agent)? {
        let options = get(c, agent, "options")?;
        if c.truthy_rooted(options)? { get(c, options, "timeout")? } else { c.undefined() }
    } else {
        c.undefined()
    };
    if !is_undefined(c, timeout) || c.truthy_rooted(agent_timeout)? {
        listen_socket_timeout(c, request)?;
    }
    emit(c, request, "socket", &[socket]).map(drop)
}

/// `listenSocketTimeout(req)`: the request's `timeout` follows its socket's.
fn listen_socket_timeout(c: &mut Context<'_>, request: RootId) -> Result<(), RootedError> {
    if truthy_member(c, request, "timeoutCb")? {
        return Ok(());
    }
    let listener = class(c, Class::ClientRequestTimeout)?;
    put(c, request, "timeoutCb", listener)?;
    let socket = outgoing::socket(c, request)?;
    if c.truthy_rooted(socket)? {
        once(c, socket, "timeout", listener)
    } else {
        let on_socket = function(c, Member::ListenSocketTimeout, "", &[])?;
        on(c, request, "socket", on_socket)
    }
}

fn set_socket_timeout(c: &mut Context<'_>, socket: RootId, msecs: RootId) -> Result<(), RootedError> {
    if truthy_member(c, socket, "connecting")? {
        let later = function(c, Member::SocketSetTimeout, "", &[msecs, socket])?;
        once(c, socket, "connect", later)
    } else {
        call_method(c, socket, "setTimeout", &[msecs]).map(drop)
    }
}

fn set_timeout(c: &mut Context<'_>, this: RootId, msecs: RootId, callback: RootId) -> Outcome {
    if truthy_member(c, this, "_ended")? {
        return Ok(this);
    }
    listen_socket_timeout(c, this)?;
    let duration = timer_duration(c, msecs, "msecs")?;
    let msecs = c.number(duration);
    if c.truthy_rooted(callback)? {
        once(c, this, "timeout", callback)?;
    }
    let socket = outgoing::socket(c, this)?;
    if c.truthy_rooted(socket)? {
        set_socket_timeout(c, socket, msecs)?;
    } else {
        let undefined = c.undefined();
        let later = function(c, Member::SocketSetTimeout, "", &[msecs, undefined])?;
        once(c, this, "socket", later)?;
    }
    Ok(this)
}

fn call_socket_method(c: &mut Context<'_>, socket: RootId, method: RootId, values: RootId) -> Result<(), RootedError> {
    if !c.truthy_rooted(method)? {
        return Ok(());
    }
    let function = c.get_property_rooted(socket, method)?;
    let values = if is_nullish(c, values) { Vec::new() } else { c.array_like_values_rooted(values)? };
    c.call_rooted(function, socket, &values).map(drop)
}

/// `_deferToConnect(method, arguments_)`: a socket call once the socket is writable.
fn defer_to_connect(c: &mut Context<'_>, this: RootId, method: RootId, values: RootId) -> Result<(), RootedError> {
    let socket = outgoing::socket(c, this)?;
    if !c.truthy_rooted(socket)? {
        let undefined = c.undefined();
        let later = function(c, Member::DeferredCall, "", &[undefined, method, values])?;
        return once(c, this, "socket", later);
    }
    if truthy_member(c, socket, "writable")? {
        call_socket_method(c, socket, method, values)
    } else {
        let later = function(c, Member::DeferredCall, "", &[socket, method, values])?;
        once(c, socket, "connect", later)
    }
}

fn hang_up(c: &mut Context<'_>, request: RootId, socket: RootId) -> Result<(), RootedError> {
    let carrier = outgoing::socket(c, request)?;
    let carrier = if c.truthy_rooted(carrier)? { carrier } else { socket };
    if !truthy_member(c, carrier, "_hadError")? {
        put_bool(c, carrier, "_hadError", true)?;
        let error = super::connection_reset(c, "socket hang up")?;
        emit(c, request, "error", &[error])?;
    }
    Ok(())
}

fn socket_close(c: &mut Context<'_>, socket: RootId) -> Result<(), RootedError> {
    let request = get(c, socket, "_httpMessage")?;
    if is_nullish(c, request) {
        return Ok(());
    }
    let parser = get(c, socket, "parser")?;
    let response = get(c, request, "res")?;
    put_bool(c, request, "destroyed", true)?;
    if c.truthy_rooted(response)? {
        if !truthy_member(c, response, "complete")? {
            let error = super::connection_reset(c, "aborted")?;
            call_method(c, response, "destroy", &[error])?;
        }
        put_bool(c, request, "_closed", true)?;
        emit(c, request, "close", &[])?;
        if !truthy_member(c, response, "aborted")? && truthy_member(c, response, "readable")? {
            let null = c.null();
            call_method(c, response, "push", &[null])?;
        }
    } else {
        hang_up(c, request, socket)?;
        put_bool(c, request, "_closed", true)?;
        emit(c, request, "close", &[])?;
    }
    let output = get(c, request, "outputData")?;
    if c.truthy_rooted(output)? {
        crate::modules::stream_shared_vm::put_number(c, output, "length", 0.0)?;
    }
    if !is_nullish(c, parser) {
        parsing::finish(c, parser)?;
        parsing::free(c, parser, Some(request), socket)?;
    }
    Ok(())
}

fn detach_parser_listeners(c: &mut Context<'_>, socket: RootId, events: &[(&str, Class)]) -> Result<(), RootedError> {
    for &(event, listener) in events {
        let listener = class(c, listener)?;
        remove_listener(c, socket, event, listener)?;
    }
    Ok(())
}

fn socket_error(c: &mut Context<'_>, socket: RootId, error: RootId) -> Result<(), RootedError> {
    let request = get(c, socket, "_httpMessage")?;
    if c.truthy_rooted(request)? {
        let carrier = outgoing::socket(c, request)?;
        let carrier = if c.truthy_rooted(carrier)? { carrier } else { socket };
        put_bool(c, carrier, "_hadError", true)?;
        emit(c, request, "error", &[error])?;
    }
    let parser = get(c, socket, "parser")?;
    if !is_nullish(c, parser) {
        parsing::finish(c, parser)?;
        let request = if c.truthy_rooted(request)? { Some(request) } else { None };
        parsing::free(c, parser, request, socket)?;
    }
    detach_parser_listeners(c, socket, &[("data", Class::ClientSocketData), ("end", Class::ClientSocketEnd)])?;
    call_method(c, socket, "destroy", &[]).map(drop)
}

fn socket_end(c: &mut Context<'_>, socket: RootId) -> Result<(), RootedError> {
    let request = get(c, socket, "_httpMessage")?;
    let parser = get(c, socket, "parser")?;
    if c.truthy_rooted(request)? {
        let response = get(c, request, "res")?;
        if !c.truthy_rooted(response)? {
            hang_up(c, request, socket)?;
        }
    }
    if !is_nullish(c, parser) {
        parsing::finish(c, parser)?;
        let request = if c.truthy_rooted(request)? { Some(request) } else { None };
        parsing::free(c, parser, request, socket)?;
    }
    call_method(c, socket, "destroy", &[]).map(drop)
}

fn socket_data(c: &mut Context<'_>, socket: RootId, data: RootId) -> Result<(), RootedError> {
    let request = get(c, socket, "_httpMessage")?;
    let parser = get(c, socket, "parser")?;
    if is_nullish(c, parser) {
        return Ok(());
    }
    match parsing::execute(c, parser, data)? {
        Executed::Failed(error) => {
            put(c, error, "rawPacket", data)?;
            parsing::free(c, parser, Some(request), socket)?;
            detach_parser_listeners(c, socket, &[("data", Class::ClientSocketData), ("end", Class::ClientSocketEnd)])?;
            call_method(c, socket, "destroy", &[])?;
            put_bool(c, socket, "_hadError", true)?;
            emit(c, request, "error", &[error])?;
        }
        Executed::Upgraded(rest) => {
            let response = get(c, parser, "incoming")?;
            put(c, request, "res", response)?;
            detach_parser_listeners(c, socket, &[
                ("data", Class::ClientSocketData),
                ("end", Class::ClientSocketEnd),
                ("drain", Class::ClientSocketDrain),
                ("timeout", Class::ClientRequestTimeout),
                ("timeout", Class::ClientResponseTimeout),
            ])?;
            parsing::finish(c, parser)?;
            parsing::free(c, parser, Some(request), socket)?;
            let head = crate::modules::buffer_shared_vm::from_vec(c, rest)?;
            let method = get(c, request, "method")?;
            let event = if c.string_text(method)?.as_deref() == Some("CONNECT") { "connect" } else { "upgrade" };
            if listener_count(c, request, event)? > 0.0 {
                put_bool(c, request, "upgradeOrConnect", true)?;
                emit(c, socket, "agentRemove", &[])?;
                detach_parser_listeners(c, socket, &[("close", Class::ClientSocketClose), ("error", Class::ClientSocketError)])?;
                let null = c.null();
                put(c, socket, "_httpMessage", null)?;
                put(c, socket, "readableFlowing", null)?;
                emit(c, request, event, &[response, socket, head])?;
                put_bool(c, request, "destroyed", true)?;
                put_bool(c, request, "_closed", true)?;
                emit(c, request, "close", &[])?;
            } else {
                call_method(c, socket, "destroy", &[])?;
            }
        }
        Executed::Consumed => {
            let response = get(c, parser, "incoming")?;
            if c.truthy_rooted(response)? && truthy_member(c, response, "complete")? {
                let status = number(c, response, "statusCode")?;
                if !is_informational(status as u16) {
                    detach_parser_listeners(c, socket, &[
                        ("data", Class::ClientSocketData),
                        ("end", Class::ClientSocketEnd),
                        ("drain", Class::ClientSocketDrain),
                    ])?;
                    parsing::free(c, parser, Some(request), socket)?;
                }
            }
        }
    }
    Ok(())
}

/// `parserOnIncomingClient(res, shouldKeepAlive)`.
pub(crate) fn on_incoming(c: &mut Context<'_>, socket: RootId, response: RootId, keep_alive: bool) -> Result<HeadOutcome, RootedError> {
    let request = get(c, socket, "_httpMessage")?;
    if truthy_member(c, request, "res")? {
        call_method(c, socket, "destroy", &[])?;
        return Ok(HeadOutcome::Body);
    }
    put(c, request, "res", response)?;
    if truthy_member(c, response, "upgrade")? {
        return Ok(HeadOutcome::Upgrade);
    }
    let method = get(c, request, "method")?;
    let method = c.string_text(method)?.unwrap_or_default();
    if method == "CONNECT" {
        put_bool(c, response, "upgrade", true)?;
        return Ok(HeadOutcome::Upgrade);
    }
    const STATUS_CONTINUE: f64 = 100.0;
    const STATUS_NOT_MODIFIED: f64 = 304.0;
    let status = number(c, response, "statusCode")?;
    if is_informational(status as u16) {
        let null = c.null();
        put(c, request, "res", null)?;
        if status == STATUS_CONTINUE {
            emit(c, request, "continue", &[])?;
        }
        let information = c.object_rooted()?;
        for name in ["statusCode", "statusMessage", "httpVersion", "httpVersionMajor", "httpVersionMinor", "headers", "rawHeaders"] {
            let value = get(c, response, name)?;
            put(c, information, name, value)?;
        }
        emit(c, request, "information", &[information])?;
        return Ok(HeadOutcome::SkipBody);
    }
    if truthy_member(c, request, "shouldKeepAlive")? && !keep_alive && !truthy_member(c, request, "upgradeOrConnect")? {
        put_bool(c, request, "shouldKeepAlive", false)?;
    }
    put(c, request, "res", response)?;
    put(c, response, "req", request)?;
    let end = class(c, Class::ClientResponseEnd)?;
    on(c, response, "end", end)?;
    let finish = class(c, Class::ClientRequestFinish)?;
    on(c, request, "finish", finish)?;
    let timeout = class(c, Class::ClientResponseTimeout)?;
    on(c, socket, "timeout", timeout)?;
    if truthy_member(c, request, "aborted")? || !emit(c, request, "response", &[response])? {
        call_method(c, response, "_dump", &[])?;
    }
    if method == "HEAD" {
        return Ok(HeadOutcome::SkipBody);
    }
    if status == STATUS_NOT_MODIFIED {
        put_bool(c, response, "complete", true)?;
        return Ok(HeadOutcome::SkipBody);
    }
    Ok(HeadOutcome::Body)
}

/// `responseKeepAlive(req)`: the socket goes back to the agent once the exchange is over.
fn response_keep_alive(c: &mut Context<'_>, request: RootId) -> Result<(), RootedError> {
    let socket = outgoing::socket(c, request)?;
    let timeout = get(c, request, "timeoutCb")?;
    if c.truthy_rooted(timeout)? {
        let zero = c.number(0.0);
        call_method(c, socket, "setTimeout", &[zero, timeout])?;
        let null = c.null();
        put(c, request, "timeoutCb", null)?;
    }
    detach_parser_listeners(c, socket, &[
        ("close", Class::ClientSocketClose),
        ("error", Class::ClientSocketError),
        ("data", Class::ClientSocketData),
        ("end", Class::ClientSocketEnd),
    ])?;
    let free = function(c, Member::EmitFree, "emitFreeNT", &[request])?;
    next_tick(c, free, &[])?;
    put_bool(c, request, "destroyed", true)?;
    let response = get(c, request, "res")?;
    if c.truthy_rooted(response)? {
        let null = c.null();
        put(c, response, "socket", null)?;
    }
    Ok(())
}

/// `responseOnEnd`: `this` is the response.
fn response_end(c: &mut Context<'_>, response: RootId) -> Result<(), RootedError> {
    let request = get(c, response, "req")?;
    let socket = outgoing::socket(c, request)?;
    if c.truthy_rooted(socket)? {
        let timeout = get(c, request, "timeoutCb")?;
        if c.truthy_rooted(timeout)? {
            let listener = class(c, Class::ClientRequestTimeout)?;
            remove_listener(c, socket, "timeout", listener)?;
        }
        let listener = class(c, Class::ClientResponseTimeout)?;
        remove_listener(c, socket, "timeout", listener)?;
    }
    put_bool(c, request, "_ended", true)?;
    if !truthy_member(c, request, "shouldKeepAlive")? {
        if c.truthy_rooted(socket)? && truthy_member(c, socket, "writable")? {
            let destroy_soon = get(c, socket, "destroySoon")?;
            if c.is_callable_rooted(destroy_soon)? {
                c.call_rooted(destroy_soon, socket, &[])?;
            } else {
                call_method(c, socket, "end", &[])?;
            }
        }
    } else {
        let finished = get(c, request, "writableFinished")?;
        if c.truthy_rooted(finished)? && !truthy_member(c, response, "aborted")? {
            response_keep_alive(c, request)?;
        }
    }
    Ok(())
}

/// `http.request(url[, options][, callback])`.
pub(crate) fn request(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let class = class(c, Class::ClientRequest)?;
    let values: Vec<RootId> = (0..3).map(|index| arg(c, args, index)).collect();
    c.construct_rooted(class, class, &values)
}

/// `http.get(url[, options][, callback])`: a request ended at once.
pub(crate) fn get_request(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let request = request(c, args)?;
    call_method(c, request, "end", &[])?;
    Ok(request)
}
