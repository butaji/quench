//! Node's `http` (lib/http.js and its `_http_*` modules): `OutgoingMessage`,
//! `IncomingMessage`, `ServerResponse`, `Server`, `ClientRequest` and `Agent` over any
//! `net`/`tls` socket, with the HTTP/1 framing read by the Rust parser (`parser`).

pub(crate) mod agent;
pub(crate) mod client;
pub(crate) mod incoming;
pub(crate) mod outgoing;
pub(crate) mod parser;
mod parsing;
pub(crate) mod server;
mod state;

pub(crate) use state::State;
pub(crate) use crate::modules::host_members::{accessors, captured, constructor, current, function, members, methods};

use crate::modules::stream_shared_vm::{call_method, get, Context, Outcome};
use quench_runtime::{RootId, RootedError};

/// The private symbols the classes key their state with, by Node's names.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    OutHeaders,
    NeedDrain,
    Corked,
    HighWaterMark,
    UniqueHeaders,
    Errored,
    RejectNonStandardBodyWrites,
    BytesWritten,
    Socket,
    Headers,
    HeadersDistinct,
    HeadersCount,
    Trailers,
    TrailersDistinct,
    TrailersCount,
    /// A parser object's host parser id.
    ParserId,
    /// A server connection's bookkeeping (`state` in `connectionListenerInternal`).
    ConnectionState,
    RequestOptions,
    Error,
    IncomingMessageClass,
    ServerResponseClass,
    /// A server's connection-checking interval timer.
    ConnectionsCheckingInterval,
    /// A server's tracked connections, for timeouts and `closeIdleConnections`.
    Connections,
    /// A server response's `close` listener on its socket (`onServerResponseClose`).
    CloseListener,
    /// A socket's agent bookkeeping listeners (`installListeners`).
    AgentListeners,
    /// An agent's `keylog` forwarder for its sockets, once something listens (`kOnKeylog`).
    OnKeylog,
}

impl Slot {
    const ALL: [Slot; 26] = [
        Self::OutHeaders,
        Self::NeedDrain,
        Self::Corked,
        Self::HighWaterMark,
        Self::UniqueHeaders,
        Self::Errored,
        Self::RejectNonStandardBodyWrites,
        Self::BytesWritten,
        Self::Socket,
        Self::Headers,
        Self::HeadersDistinct,
        Self::HeadersCount,
        Self::Trailers,
        Self::TrailersDistinct,
        Self::TrailersCount,
        Self::ParserId,
        Self::ConnectionState,
        Self::RequestOptions,
        Self::Error,
        Self::IncomingMessageClass,
        Self::ServerResponseClass,
        Self::ConnectionsCheckingInterval,
        Self::Connections,
        Self::CloseListener,
        Self::AgentListeners,
        Self::OnKeylog,
    ];

    fn description(self) -> &'static str {
        match self {
            Self::OutHeaders => "kOutHeaders",
            Self::NeedDrain => "kNeedDrain",
            Self::Corked => "corked",
            Self::HighWaterMark => "kHighWaterMark",
            Self::UniqueHeaders => "kUniqueHeaders",
            Self::Errored => "errored",
            Self::RejectNonStandardBodyWrites => "kRejectNonStandardBodyWrites",
            Self::BytesWritten => "kBytesWritten",
            Self::Socket => "kSocket",
            Self::Headers => "kHeaders",
            Self::HeadersDistinct => "kHeadersDistinct",
            Self::TrailersDistinct => "kTrailersDistinct",
            Self::HeadersCount => "kHeadersCount",
            Self::Trailers => "kTrailers",
            Self::TrailersCount => "kTrailersCount",
            Self::ParserId => "kParserId",
            Self::ConnectionState => "kConnectionState",
            Self::RequestOptions => "kRequestOptions",
            Self::Error => "kError",
            Self::IncomingMessageClass => "IncomingMessage",
            Self::ServerResponseClass => "ServerResponse",
            Self::ConnectionsCheckingInterval => "http.server.connectionsCheckingInterval",
            Self::Connections => "http.server.connections",
            Self::CloseListener => "onServerResponseClose",
            Self::AgentListeners => "kAgentListeners",
            Self::OnKeylog => "onkeylog",
        }
    }
}

/// The classes, objects and shared listeners the members construct, inspect or delegate to.
#[derive(Clone, Copy)]
pub(crate) enum Class {
    OutgoingMessage,
    IncomingMessage,
    ServerResponse,
    ClientRequest,
    Agent,
    Server,
    StatusCodes,
    GlobalAgent,
    Net,
    ClientSocketClose,
    ClientSocketError,
    ClientSocketEnd,
    ClientSocketData,
    ClientSocketDrain,
    ClientResponseTimeout,
    ClientRequestTimeout,
    ClientResponseEnd,
    ClientRequestFinish,
    FreeSocketError,
}

impl Class {
    const COUNT: usize = Class::FreeSocketError as usize + 1;
}

/// `Number.MAX_SAFE_INTEGER`, the bound of Node's integer option validation.
pub(crate) use quench_runtime::units::MAX_SAFE_INTEGER;

#[derive(Clone, Copy)]
pub(crate) struct Intrinsics {
    slots: [RootId; Slot::ALL.len()],
    classes: [RootId; Class::COUNT],
}

crate::host::startup_snapshot::host_snap_struct!(Intrinsics { slots, classes });

const _: () = {
    let mut index = 0;
    while index < Slot::ALL.len() {
        assert!(Slot::ALL[index] as usize == index);
        index += 1;
    }
};

fn intrinsics(c: &mut Context<'_>) -> Result<Intrinsics, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .http
        .intrinsics
        .ok_or_else(|| RootedError::host("http module is not initialized"))
}

pub(crate) fn slot(c: &mut Context<'_>, slot: Slot) -> Outcome {
    Ok(intrinsics(c)?.slots[slot as usize])
}

pub(crate) fn class(c: &mut Context<'_>, class: Class) -> Outcome {
    Ok(intrinsics(c)?.classes[class as usize])
}

pub(crate) fn get_slot(c: &mut Context<'_>, object: RootId, which: Slot) -> Outcome {
    let key = slot(c, which)?;
    c.get_property_rooted(object, key)
}

pub(crate) fn put_slot(c: &mut Context<'_>, object: RootId, which: Slot, value: RootId) -> Result<(), RootedError> {
    let key = slot(c, which)?;
    c.set_property_rooted(object, key, value, object).map(drop)
}

/// Replace a class root, as `http.globalAgent = agent` replaces the default agent.
pub(crate) fn set_class(c: &mut Context<'_>, which: Class, value: RootId) -> Result<(), RootedError> {
    let retained = c.retain(value)?;
    let shared = c.host_mut().shared_state();
    let previous = {
        let mut state = shared.borrow_mut();
        let intrinsics = state.http.intrinsics.as_mut().ok_or_else(|| RootedError::host("http module is not initialized"))?;
        std::mem::replace(&mut intrinsics.classes[which as usize], retained)
    };
    c.release_root(previous);
    Ok(())
}

/// A Node error of `class` with `code` and `message`, not thrown.
pub(crate) fn coded(c: &mut Context<'_>, class: ErrorClass, code: &'static str, message: &str) -> Outcome {
    let error = match class {
        ErrorClass::Error => c.error_rooted(message)?,
        ErrorClass::TypeError => c.type_error_rooted(message)?,
        ErrorClass::RangeError => c.range_error_rooted(message)?,
    };
    match class {
        ErrorClass::Error => crate::modules::node_errors_shared_vm::decorate_error(c, error, code)?,
        ErrorClass::TypeError => crate::modules::node_errors_shared_vm::decorate_type_error(c, error, code)?,
        ErrorClass::RangeError => crate::modules::node_errors_shared_vm::decorate_range_error(c, error, code)?,
    }
    Ok(error)
}

pub(crate) fn throw_coded(c: &mut Context<'_>, class: ErrorClass, code: &'static str, message: &str) -> RootedError {
    match coded(c, class, code, message) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ErrorClass {
    Error,
    TypeError,
    RangeError,
}

/// `util.format(template, ...values)`, as Node's error messages format their arguments.
pub(crate) fn format(c: &mut Context<'_>, template: &str, values: &[RootId]) -> Result<String, RootedError> {
    let formatters = crate::modules::util_shared_vm::internal_formatters(c)?;
    let template = c.string_rooted(template);
    let mut arguments = vec![template];
    arguments.extend_from_slice(values);
    let message = call_method(c, formatters, "format", &arguments)?;
    c.to_string(message)
}

/// Node's `ConnResetException(message)`: an `Error` with code `ECONNRESET`.
pub(crate) fn connection_reset(c: &mut Context<'_>, message: &str) -> Outcome {
    let error = c.error_rooted(message)?;
    let code = c.string_rooted("ECONNRESET");
    c.assign_named_property_rooted(error, "code", code)?;
    Ok(error)
}

/// `util.inspect(value)`, as `ERR_INVALID_ARG_VALUE` shows what it received.
pub(crate) fn inspect(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let inspect = crate::modules::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let shown = c.call_rooted(inspect, undefined, &[value])?;
    c.to_string(shown)
}

/// `Buffer.byteLength(chunk, encoding)` of a string or byte view.
pub(crate) fn byte_length(c: &mut Context<'_>, chunk: RootId, encoding: RootId) -> Result<f64, RootedError> {
    if let Some(length) = c.with_buffer_bytes_rooted(chunk, <[u8]>::len) {
        return Ok(length as f64);
    }
    let global = c.global_root()?;
    let buffer = get(c, global, "Buffer")?;
    let length = call_method(c, buffer, "byteLength", &[chunk, encoding])?;
    c.to_number(length)
}

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    crate::host::shared_vm::commonjs::http_module(c)
}

/// Builds the `http` builtin the module cache keeps (`module`).
pub(crate) fn build_module(c: &mut Context<'_>) -> Outcome {
    let undefined = c.undefined();
    let mut slots = [undefined; Slot::ALL.len()];
    for which in Slot::ALL {
        let symbol = c.symbol_rooted(Some(which.description()));
        slots[which as usize] = c.retain(symbol)?;
    }
    let mut classes = [undefined; Class::COUNT];
    for class in &mut classes {
        *class = c.retain(undefined)?;
    }
    c.host_mut().shared_state().borrow_mut().http.intrinsics = Some(Intrinsics { slots, classes });
    let net = crate::host::shared_vm::commonjs::net_module(c)?;
    set_class(c, Class::Net, net)?;
    let codes = crate::modules::http_status_shared_vm::codes(c)?;
    set_class(c, Class::StatusCodes, codes)?;
    let outgoing = outgoing::build(c)?;
    set_class(c, Class::OutgoingMessage, outgoing)?;
    let incoming = incoming::build(c)?;
    set_class(c, Class::IncomingMessage, incoming)?;
    let response = server::build_response(c, outgoing)?;
    set_class(c, Class::ServerResponse, response)?;
    let request = client::build(c, outgoing)?;
    set_class(c, Class::ClientRequest, request)?;
    let agent = agent::build(c)?;
    set_class(c, Class::Agent, agent)?;
    let http_server = server::build_server(c)?;
    set_class(c, Class::Server, http_server)?;
    let global_agent = agent::global(c, agent)?;
    set_class(c, Class::GlobalAgent, global_agent)?;

    let module = c.object_rooted()?;
    let methods_list = crate::modules::http_protocol::HTTP_METHODS
        .iter()
        .map(|method| c.string_rooted(method))
        .collect::<Vec<_>>();
    let methods_array = c.array_rooted(&methods_list)?;
    let listener = function(c, Module::ConnectionListener, "connectionListener", &[])?;
    let exports = [
        ("_connectionListener", listener),
        ("METHODS", methods_array),
        ("STATUS_CODES", codes),
        ("Agent", agent),
        ("ClientRequest", request),
        ("IncomingMessage", incoming),
        ("OutgoingMessage", outgoing),
        ("Server", http_server),
        ("ServerResponse", response),
    ];
    for (name, value) in exports {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(module, key, value, true, true, true)?;
    }
    for (name, member) in [
        ("createServer", Module::CreateServer),
        ("validateHeaderName", Module::ValidateHeaderName),
        ("validateHeaderValue", Module::ValidateHeaderValue),
        ("get", Module::Get),
        ("request", Module::Request),
        ("setMaxIdleHTTPParsers", Module::SetMaxIdleParsers),
    ] {
        let value = function(c, member, name, &[])?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(module, key, value, true, true, true)?;
    }
    for (name, getter, setter) in [
        ("maxHeaderSize", Module::MaxHeaderSize, None),
        ("globalAgent", Module::GlobalAgent, Some(Module::SetGlobalAgent)),
    ] {
        let get = function(c, getter, "get", &[])?;
        let set = match setter {
            Some(setter) => function(c, setter, "set", &[])?,
            None => c.undefined(),
        };
        let key = c.string_rooted(name);
        let fields = quench_runtime::DescriptorFields {
            get: Some(get),
            set: Some(set),
            enumerable: Some(true),
            configurable: Some(true),
            ..Default::default()
        };
        c.define_property_fields_rooted(module, key, fields)?;
    }
    Ok(module)
}

members!(Module, "httpModule", [
    ConnectionListener,
    CreateServer,
    ValidateHeaderName,
    ValidateHeaderValue,
    Get,
    Request,
    SetMaxIdleParsers,
    MaxHeaderSize,
    GlobalAgent,
    SetGlobalAgent,
    ReadStart,
    ReadStop,
    PreClose,
    CheckInvalidHeaderChar,
    CheckIsHttpToken,
    FreeParser,
    IsLenient,
    PrepareError,
]);

pub(crate) fn module_member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Module>(c)? {
        Module::ConnectionListener => server::connection_listener(c, this, args),
        Module::CreateServer => server::create_server(c, this, args),
        Module::ValidateHeaderName => {
            let (name, label) = (at(0), at(1));
            outgoing::validate_header_name(c, name, label)?;
            Ok(c.undefined())
        }
        Module::ValidateHeaderValue => {
            let (name, value) = (at(0), at(1));
            outgoing::validate_header_value(c, name, value)?;
            Ok(c.undefined())
        }
        Module::Get => client::get_request(c, args),
        Module::Request => client::request(c, args),
        Module::SetMaxIdleParsers => {
            let max = at(0);
            let maximum = c.number(f64::from(i32::MAX));
            crate::modules::util_shared_vm::validate_integer(c, max, "max", 1.0, maximum)?;
            Ok(c.undefined())
        }
        Module::MaxHeaderSize => {
            let size = max_header_size(c);
            Ok(c.number(size))
        }
        Module::GlobalAgent => class(c, Class::GlobalAgent),
        Module::SetGlobalAgent => {
            let value = at(0);
            set_class(c, Class::GlobalAgent, value)?;
            Ok(c.undefined())
        }
        Module::ReadStart => {
            incoming::read_start(c, at(0))?;
            Ok(c.undefined())
        }
        Module::ReadStop => {
            let socket = at(0);
            if !crate::modules::stream_shared_vm::is_nullish(c, socket) {
                call_method(c, socket, "pause", &[])?;
            }
            Ok(c.undefined())
        }
        Module::PreClose => {
            server::pre_close(c, at(0))?;
            Ok(c.undefined())
        }
        Module::CheckInvalidHeaderChar => {
            let invalid = outgoing::has_invalid_header_char(c, at(0))?;
            Ok(c.boolean(invalid))
        }
        Module::CheckIsHttpToken => {
            let token = outgoing::is_token(c, at(0))?;
            Ok(c.boolean(token))
        }
        Module::FreeParser => {
            let (parser, request, socket) = (at(0), at(1), at(2));
            let request = if crate::modules::stream_shared_vm::is_nullish(c, request) { None } else { Some(request) };
            parsing::free(c, parser, request, socket)?;
            Ok(c.undefined())
        }
        Module::IsLenient => {
            let lenient = c.host_mut().shared_state().borrow().cli_options.enabled(parsing::INSECURE_PARSER_FLAG);
            Ok(c.boolean(lenient))
        }
        Module::PrepareError => {
            let (error, raw) = (at(0), at(2));
            c.assign_named_property_rooted(error, "rawPacket", raw)?;
            Ok(c.undefined())
        }
    }
}

/// `http._connectionListener`, as `https.Server` serves its secure connections with it.
pub(crate) fn connection_listener_function(c: &mut Context<'_>) -> Outcome {
    let http = module(c)?;
    get(c, http, "_connectionListener")
}

/// `_http_common`'s functions and `kIncomingMessage`.
pub(crate) fn common_members(c: &mut Context<'_>, target: RootId) -> Result<(), RootedError> {
    module(c)?;
    for (name, member) in [
        ("_checkInvalidHeaderChar", Module::CheckInvalidHeaderChar),
        ("_checkIsHttpToken", Module::CheckIsHttpToken),
        ("freeParser", Module::FreeParser),
        ("isLenient", Module::IsLenient),
        ("prepareError", Module::PrepareError),
    ] {
        let value = function(c, member, name, &[])?;
        c.assign_named_property_rooted(target, name, value)?;
    }
    let incoming = slot(c, Slot::IncomingMessageClass)?;
    c.assign_named_property_rooted(target, "kIncomingMessage", incoming)
}

/// The `_http_*` internal modules: each the part of `http` its Node library file defines.
pub(crate) fn internal_module(c: &mut Context<'_>, name: &str) -> Outcome {
    let http = module(c)?;
    let module = c.object_rooted()?;
    let exports: &[&str] = match name {
        "_http_agent" => &["Agent", "globalAgent"],
        "_http_client" => &["ClientRequest"],
        "_http_incoming" => &["IncomingMessage"],
        "_http_outgoing" => &["OutgoingMessage", "validateHeaderName", "validateHeaderValue"],
        _ => &["STATUS_CODES", "Server", "ServerResponse", "_connectionListener"],
    };
    for &export in exports {
        let value = get(c, http, export)?;
        c.assign_named_property_rooted(module, export, value)?;
    }
    match name {
        "_http_incoming" => {
            for (export, member) in [("readStart", Module::ReadStart), ("readStop", Module::ReadStop)] {
                let value = function(c, member, export, &[])?;
                c.assign_named_property_rooted(module, export, value)?;
            }
        }
        "_http_outgoing" => {
            let mark = slot(c, Slot::HighWaterMark)?;
            c.assign_named_property_rooted(module, "kHighWaterMark", mark)?;
            let unique = slot(c, Slot::UniqueHeaders)?;
            c.assign_named_property_rooted(module, "kUniqueHeaders", unique)?;
        }
        "_http_server" => {
            let interval = slot(c, Slot::ConnectionsCheckingInterval)?;
            c.assign_named_property_rooted(module, "kConnectionsCheckingInterval", interval)?;
            let response = slot(c, Slot::ServerResponseClass)?;
            c.assign_named_property_rooted(module, "kServerResponse", response)?;
            let close = function(c, Module::PreClose, "httpServerPreClose", &[])?;
            c.assign_named_property_rooted(module, "httpServerPreClose", close)?;
        }
        _ => {}
    }
    Ok(module)
}

/// The default `maxHeaderSize`: `--max-http-header-size`, else 16 KiB.
pub(crate) fn max_header_size(c: &mut Context<'_>) -> f64 {
    const DEFAULT_MAX_HEADER_SIZE: f64 = 16.0 * 1024.0;
    c.host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .string_value("--max-http-header-size")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(DEFAULT_MAX_HEADER_SIZE)
}
