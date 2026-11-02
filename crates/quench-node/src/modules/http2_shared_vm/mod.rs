//! Node's `http2` (lib/http2.js and lib/internal/http2/*) over a Rust HTTP/2 engine:
//! `connection` holds each connection's protocol state (framing, HPACK, flow control,
//! settings, pings, GOAWAY) and the sessions, streams, servers and compatibility classes
//! project it as Node's JavaScript API over any `net` or `tls` socket.

mod compat;
mod connection;
mod constants;
mod faults;
mod frame;
mod headers;
mod hpack;
mod huffman;
mod server;
mod session;
mod settings;
mod stream;

pub(crate) use compat::member as compat_member;
pub(crate) use server::member as server_member;
pub(crate) use session::member as session_member;
pub(crate) use stream::member as stream_member;

use crate::modules::host_members::{current, function, members};
use crate::modules::stream_shared_vm::{call_method, get, put, Context, Outcome};
use connection::Connection;
use faults::Fault;
use quench_runtime::{RootId, RootedError};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;

/// Node's default SETTINGS_MAX_HEADER_LIST_SIZE (`getDefaultSettings().maxHeaderListSize`).
const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 65_535;
/// `2 ** 32 - 1`, the bound of Node's 32-bit setting and code validation.
const MAX_INT: f64 = u32::MAX as f64;

/// The private symbols the classes key their state with, by Node's names.
#[derive(Clone, Copy)]
enum Slot {
    /// The host handle id of a live session or stream (`kHandle`).
    Handle,
    Type,
    /// A session's or stream's `kState.flags`.
    Flags,
    DestroyCode,
    GoawayCode,
    GoawayLastStreamId,
    PendingAck,
    Encrypted,
    AlpnProtocol,
    Socket,
    Server,
    Authority,
    Protocol,
    Timeout,
    LocalSettings,
    RemoteSettings,
    PendingRequestCalls,
    Options,
    /// On a socket: the session bound to it (`kBoundSession`).
    BoundSession,
    Session,
    Id,
    RstCode,
    DidRead,
    TrailersReady,
    EndAfterHeaders,
    ShutdownWritableCalled,
    SentHeaders,
    RawHeaders,
    SentTrailers,
    InfoHeaders,
    Origin,
    Request,
    Response,
    OwnsFd,
    /// The compatibility objects' fields.
    CompatState,
    Stream,
    Headers,
    Trailers,
    RawTrailers,
    Aborted,
    /// The `Proxy` a session's or stream's `socket` is (`kProxySocket`).
    ProxySocket,
    /// A session's `kStrictSingleValueFields`.
    StrictSingleValueFields,
    /// A server's live sessions (`kSessions`).
    Sessions,
}

impl Slot {
    const ALL: [Slot; 43] = [
        Self::Handle,
        Self::Type,
        Self::Flags,
        Self::DestroyCode,
        Self::GoawayCode,
        Self::GoawayLastStreamId,
        Self::PendingAck,
        Self::Encrypted,
        Self::AlpnProtocol,
        Self::Socket,
        Self::Server,
        Self::Authority,
        Self::Protocol,
        Self::Timeout,
        Self::LocalSettings,
        Self::RemoteSettings,
        Self::PendingRequestCalls,
        Self::Options,
        Self::BoundSession,
        Self::Session,
        Self::Id,
        Self::RstCode,
        Self::DidRead,
        Self::TrailersReady,
        Self::EndAfterHeaders,
        Self::ShutdownWritableCalled,
        Self::SentHeaders,
        Self::RawHeaders,
        Self::SentTrailers,
        Self::InfoHeaders,
        Self::Origin,
        Self::Request,
        Self::Response,
        Self::OwnsFd,
        Self::CompatState,
        Self::Stream,
        Self::Headers,
        Self::Trailers,
        Self::RawTrailers,
        Self::Aborted,
        Self::ProxySocket,
        Self::StrictSingleValueFields,
        Self::Sessions,
    ];
}

const _: () = {
    let mut index = 0;
    while index < Slot::ALL.len() {
        assert!(Slot::ALL[index] as usize == index);
        index += 1;
    }
};

/// The classes and shared functions the members construct or install.
#[derive(Clone, Copy)]
enum Class {
    Http2Session,
    ServerHttp2Session,
    ClientHttp2Session,
    Http2Stream,
    ServerHttp2Stream,
    ClientHttp2Stream,
    Http2Server,
    Http2SecureServer,
    Http2ServerRequest,
    Http2ServerResponse,
    /// `kSensitiveHeaders`, exported as `sensitiveHeaders`.
    SensitiveHeaders,
    /// `emit(self, ...args)`, as `process.nextTick` defers an event.
    Emit,
    /// `setupCompat`, a server's `newListener` listener until `request` is listened for.
    SetupCompat,
    /// The session's socket listeners and the stream's `kMaybeDestroy` listener.
    SocketError,
    SocketClose,
    StreamMaybeDestroy,
    /// The `Proxy` handlers of a session's socket and of a compatibility object's socket.
    SessionSocketHandler,
    StreamSocketHandler,
}

impl Class {
    const COUNT: usize = Class::StreamSocketHandler as usize + 1;
}

#[derive(Clone, Copy)]
pub(crate) struct Intrinsics {
    slots: [RootId; Slot::ALL.len()],
    classes: [RootId; Class::COUNT],
}

crate::host::startup_snapshot::host_snap_struct!(Intrinsics { slots, classes });

/// A live session's host side, from construction until it is cleaned up: its protocol
/// connection once the socket is ready, and the streams it multiplexes.
struct SessionHandle {
    object: RootId,
    socket: RootId,
    connection: Option<Connection>,
    /// Streams with an id, and pending streams (client requests before their id).
    streams: FxHashMap<u32, u64>,
    pending: Vec<u64>,
    /// Callbacks of SETTINGS and PING frames awaiting acknowledgement, in order.
    settings_callbacks: VecDeque<RootId>,
    ping_callbacks: VecDeque<RootId>,
    /// Whether `session::drive` is delivering this session's events.
    driving: bool,
    /// Whether an immediate will send the connection's queued frames.
    write_scheduled: bool,
    /// Events taken from the connection and not yet delivered, and whether a tick will
    /// deliver the next of them.
    deferred: VecDeque<connection::Event>,
    continuation_scheduled: bool,
}

/// A live stream's host side, from construction until it is destroyed.
struct StreamHandle {
    object: RootId,
    session: u64,
    id: Option<u32>,
    /// The write callbacks awaiting their chunks' framing, each with its chunk count.
    writes: VecDeque<(RootId, usize)>,
    /// A request head waiting for the session to connect.
    request: Option<session::PendingRequest>,
}

#[derive(Default)]
pub(crate) struct State {
    next_handle: u64,
    sessions: FxHashMap<u64, SessionHandle>,
    streams: FxHashMap<u64, StreamHandle>,
    intrinsics: Option<Intrinsics>,
    /// The compatibility API's one-time warnings, given once per process.
    status_message_warned: bool,
    connection_header_warned: bool,
}

impl State {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn allocate(&mut self) -> u64 {
        self.next_handle += 1;
        self.next_handle
    }

    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        let Self { next_handle, sessions, streams, intrinsics, status_message_warned, connection_header_warned } = self;
        require_quiescent(sessions.is_empty() && streams.is_empty(), "an HTTP/2 session")?;
        next_handle.write(writer)?;
        intrinsics.write(writer)?;
        status_message_warned.write(writer)?;
        connection_header_warned.write(writer)
    }

    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        use crate::host::startup_snapshot::HostSnap;
        self.next_handle = HostSnap::read(reader)?;
        self.sessions.clear();
        self.streams.clear();
        self.intrinsics = HostSnap::read(reader)?;
        self.status_message_warned = HostSnap::read(reader)?;
        self.connection_header_warned = HostSnap::read(reader)?;
        Ok(())
    }
}

/// Run `f` over the module's host state.
fn with_state<T>(c: &mut Context<'_>, f: impl FnOnce(&mut State) -> T) -> T {
    let shared = c.host_mut().shared_state();
    let mut shared = shared.borrow_mut();
    f(&mut shared.http2)
}

fn intrinsics(c: &mut Context<'_>) -> Result<Intrinsics, RootedError> {
    with_state(c, |state| state.intrinsics).ok_or_else(|| RootedError::host("http2 module is not initialized"))
}

fn class(c: &mut Context<'_>, class: Class) -> Outcome {
    Ok(intrinsics(c)?.classes[class as usize])
}

fn get_slot(c: &mut Context<'_>, object: RootId, which: Slot) -> Outcome {
    let key = intrinsics(c)?.slots[which as usize];
    c.get_property_rooted(object, key)
}

fn put_slot(c: &mut Context<'_>, object: RootId, which: Slot, value: RootId) -> Result<(), RootedError> {
    let key = intrinsics(c)?.slots[which as usize];
    c.define_data_property_rooted(object, key, value, true, false, true).map(drop)
}

fn put_slot_number(c: &mut Context<'_>, object: RootId, which: Slot, value: f64) -> Result<(), RootedError> {
    let value = c.number(value);
    put_slot(c, object, which, value)
}

fn put_slot_bool(c: &mut Context<'_>, object: RootId, which: Slot, value: bool) -> Result<(), RootedError> {
    let value = c.boolean(value);
    put_slot(c, object, which, value)
}

fn slot_number(c: &mut Context<'_>, object: RootId, which: Slot) -> Result<f64, RootedError> {
    let value = get_slot(c, object, which)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).unwrap_or(0.0))
}

fn slot_truthy(c: &mut Context<'_>, object: RootId, which: Slot) -> Result<bool, RootedError> {
    let value = get_slot(c, object, which)?;
    c.truthy_rooted(value)
}

/// Whether bit `flag` of the object's `kState.flags` is set.
fn has_flag(c: &mut Context<'_>, object: RootId, flag: u32) -> Result<bool, RootedError> {
    Ok(slot_number(c, object, Slot::Flags)? as u32 & flag != 0)
}

fn set_flag(c: &mut Context<'_>, object: RootId, flag: u32, on: bool) -> Result<(), RootedError> {
    let flags = slot_number(c, object, Slot::Flags)? as u32;
    let flags = if on { flags | flag } else { flags & !flag };
    put_slot_number(c, object, Slot::Flags, f64::from(flags))
}

/// Whether `session` enforces single-value fields: unless its options turned that off.
fn strict_single_values(c: &mut Context<'_>, session: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(session)? {
        return Ok(true);
    }
    let strict = get_slot(c, session, Slot::StrictSingleValueFields)?;
    Ok(c.rooted_value(strict).and_then(|value| value.as_bool()) != Some(false))
}

/// The host handle id an object's `kHandle` slot names, while it is live.
fn handle_of(c: &mut Context<'_>, object: RootId) -> Result<Option<u64>, RootedError> {
    let value = get_slot(c, object, Slot::Handle)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).map(|id| id as u64))
}

/// `process.nextTick(emit, target, event, ...args)`.
fn emit_next_tick(c: &mut Context<'_>, target: RootId, event: &str, args: &[RootId]) -> Result<(), RootedError> {
    let emit = class(c, Class::Emit)?;
    let event = c.string_rooted(event);
    let mut values = vec![target, event];
    values.extend_from_slice(args);
    crate::modules::stream_shared_vm::next_tick(c, emit, &values)
}

/// `setImmediate(callback, ...args)`.
fn set_immediate(c: &mut Context<'_>, callback: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let mut values = vec![callback];
    values.extend_from_slice(args);
    let undefined = c.undefined();
    crate::modules::timers_shared_vm::set_immediate(c, undefined, &values).map(drop)
}

/// Methods as a `class` body defines them: writable, configurable and not enumerable.
fn class_methods<M: crate::modules::host_members::Members>(c: &mut Context<'_>, target: RootId, members: &[(&str, M)]) -> Result<(), RootedError> {
    for &(name, member) in members {
        let value = function(c, member, name, &[])?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(target, key, value, true, false, true)?;
    }
    Ok(())
}

/// `new Proxy(target, handler)`.
fn proxy(c: &mut Context<'_>, target: RootId, handler: RootId) -> Outcome {
    let global = c.global_root()?;
    let class = get(c, global, "Proxy")?;
    c.construct_rooted(class, class, &[target, handler])
}

/// A `Proxy` handler of the given traps.
fn proxy_handler<M: crate::modules::host_members::Members>(c: &mut Context<'_>, traps: &[(&str, M)]) -> Outcome {
    let handler = c.null_object_rooted()?;
    for &(name, member) in traps {
        let trap = function(c, member, name, &[])?;
        put(c, handler, name, trap)?;
    }
    Ok(handler)
}

/// `value` bound to `receiver` when it is a function (`value.bind(receiver)`), else itself.
fn bound(c: &mut Context<'_>, value: RootId, receiver: RootId) -> Outcome {
    if c.is_callable_rooted(value)? { call_method(c, value, "bind", &[receiver]) } else { Ok(value) }
}

/// `[util.inspect.custom](depth)`: `${name} ${format(fields)}`, or the object itself when
/// `depth` is negative.
fn inspect_as(c: &mut Context<'_>, this: RootId, depth: RootId, name: &str, fields: RootId) -> Outcome {
    if c.rooted_value(depth).and_then(|value| value.as_number()).is_some_and(|depth| depth < 0.0) {
        return Ok(this);
    }
    let inspect = crate::modules::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let shown = c.call_rooted(inspect, undefined, &[fields])?;
    let shown = c.to_string(shown)?;
    Ok(c.string_rooted(&format!("{name} {shown}")))
}

/// Install `[util.inspect.custom]` as member `member`.
fn install_inspect<M: crate::modules::host_members::Members>(c: &mut Context<'_>, prototype: RootId, member: M) -> Result<(), RootedError> {
    let key = crate::modules::stream_shared_vm::utils::registered_symbol(c, "nodejs.util.inspect.custom")?;
    let inspect = function(c, member, "[nodejs.util.inspect.custom]", &[])?;
    c.define_data_property_rooted(prototype, key, inspect, true, false, true).map(drop)
}

/// A host closure of member `member` over `captures`.
fn closure<M: crate::modules::host_members::Members>(c: &mut Context<'_>, member: M, captures: &[RootId]) -> Outcome {
    function(c, member, "", captures)
}

/// `ObjectAssign(target, source)`: `source`'s own enumerable properties, when it is an object.
fn assign(c: &mut Context<'_>, target: RootId, source: RootId) -> Result<(), RootedError> {
    if !c.is_object_rooted(source)? {
        return Ok(());
    }
    for key in c.enumerable_own_keys_rooted(source, true, true)? {
        let value = c.get_property_rooted(source, key)?;
        c.set_property_rooted(target, key, value, target)?;
    }
    Ok(())
}

/// `{ ...source }`.
fn spread(c: &mut Context<'_>, source: RootId) -> Outcome {
    let copy = c.object_rooted()?;
    assign(c, copy, source)?;
    Ok(copy)
}

/// Node's `assertIsObject(value, name, types)`: undefined or a non-array object.
fn assert_is_object(c: &mut Context<'_>, value: RootId, name: &str, types: &str) -> Result<(), RootedError> {
    if crate::modules::stream_shared_vm::is_undefined(c, value) {
        return Ok(());
    }
    if !c.is_object_rooted(value)? || c.is_callable_rooted(value)? || c.is_array_rooted(value)? {
        let expected = types_expectation(types);
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, value, name, &expected)?);
    }
    Ok(())
}

/// The type names `ERR_INVALID_ARG_TYPE` lists as `of type` (its `kTypes`).
const PRIMITIVE_TYPES: [&str; 9] = ["string", "function", "number", "object", "Function", "Object", "boolean", "bigint", "symbol"];

/// The phrase `ERR_INVALID_ARG_TYPE` describes a comma-separated list of accepted types
/// with (`invalidArgTypeHelper`): primitive types, then classes, `Object` last among them.
fn types_expectation(types: &str) -> String {
    let mut primitives: Vec<String> = Vec::new();
    let mut classes: Vec<String> = Vec::new();
    for kind in types.split(',') {
        if PRIMITIVE_TYPES.contains(&kind) {
            primitives.push(kind.to_lowercase());
        } else {
            classes.push(kind.to_owned());
        }
    }
    if !classes.is_empty() {
        if let Some(position) = primitives.iter().position(|kind| kind == "object") {
            primitives.remove(position);
            classes.push("Object".to_owned());
        }
    }
    let list = |items: &[String]| match items {
        [one] => one.clone(),
        [first, second] => format!("{first} or {second}"),
        [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
        [] => String::new(),
    };
    let one_of = |items: &[String]| if items.len() > 1 { "one of type" } else { "of type" };
    match (primitives.is_empty(), classes.is_empty()) {
        (false, true) => format!("{} {}", one_of(&primitives), list(&primitives)),
        (true, _) => format!("an instance of {}", list(&classes)),
        (false, false) => format!("{} {} or an instance of {}", one_of(&primitives), list(&primitives), list(&classes)),
    }
}

fn validate_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_callable_rooted(value)? {
        return Ok(());
    }
    Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "function")?)
}

fn validate_boolean(c: &mut Context<'_>, value: RootId, name: &str) -> Result<bool, RootedError> {
    match c.rooted_value(value).and_then(|value| value.as_bool()) {
        Some(value) => Ok(value),
        None => Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "boolean")?),
    }
}

fn validate_number(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    match c.rooted_value(value).and_then(|value| value.as_number()) {
        Some(value) => Ok(value),
        None => Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "number")?),
    }
}

/// `validateInteger(value, name, minimum, maximum)`.
fn validate_integer(c: &mut Context<'_>, value: RootId, name: &str, minimum: f64, maximum: f64) -> Result<f64, RootedError> {
    let maximum = c.number(maximum);
    crate::modules::util_shared_vm::validate_integer(c, value, name, minimum, maximum)
}

/// The octets of a Latin-1 string (Node's header and ALTSVC encoding).
fn latin1(c: &mut Context<'_>, value: RootId) -> Result<Vec<u8>, RootedError> {
    Ok(c.to_string_code_units(value)?.into_iter().map(|unit| unit as u8).collect())
}

fn latin1_string(c: &mut Context<'_>, bytes: &[u8]) -> RootId {
    let units: Vec<u16> = bytes.iter().map(|&byte| u16::from(byte)).collect();
    c.string_units_rooted(&units)
}

fn buffer(c: &mut Context<'_>, bytes: Vec<u8>) -> Outcome {
    crate::modules::buffer_shared_vm::from_vec(c, bytes)
}

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    crate::host::shared_vm::commonjs::http2_module(c)
}

members!(Module, "http2Module", [
    Connect,
    CreateServer,
    CreateSecureServer,
    GetDefaultSettings,
    GetPackedSettings,
    GetUnpackedSettings,
    PerformServerHandshake,
    /// `emit(self, ...args)`.
    Emit,
]);

/// Builds the `http2` builtin the module cache keeps.
pub(crate) fn build_module(c: &mut Context<'_>) -> Outcome {
    let undefined = c.undefined();
    let mut slots = [undefined; Slot::ALL.len()];
    for slot in &mut slots {
        let symbol = c.private_symbol_rooted(None);
        *slot = c.retain(symbol)?;
    }
    let mut classes = [undefined; Class::COUNT];
    for class in &mut classes {
        *class = c.retain(undefined)?;
    }
    with_state(c, |state| state.intrinsics = Some(Intrinsics { slots, classes }));
    let sensitive = c.symbol_rooted(Some("sensitiveHeaders"));
    set_class(c, Class::SensitiveHeaders, sensitive)?;
    let emit = function(c, Module::Emit, "emit", &[])?;
    set_class(c, Class::Emit, emit)?;
    crate::modules::stream_shared_vm::module(c)?;
    session::build(c)?;
    stream::build(c)?;
    let (request, response) = compat::build(c)?;
    server::build(c)?;

    let module = c.object_rooted()?;
    let constants = constants_object(c)?;
    for (name, member) in [
        ("connect", Some(Module::Connect)),
        ("constants", None),
        ("createServer", Some(Module::CreateServer)),
        ("createSecureServer", Some(Module::CreateSecureServer)),
        ("getDefaultSettings", Some(Module::GetDefaultSettings)),
        ("getPackedSettings", Some(Module::GetPackedSettings)),
        ("getUnpackedSettings", Some(Module::GetUnpackedSettings)),
        ("performServerHandshake", Some(Module::PerformServerHandshake)),
    ] {
        let value = match member {
            Some(member) => function(c, member, name, &[])?,
            None => constants,
        };
        put(c, module, name, value)?;
    }
    put(c, module, "sensitiveHeaders", sensitive)?;
    put(c, module, "Http2ServerRequest", request)?;
    put(c, module, "Http2ServerResponse", response)?;
    Ok(module)
}

/// `internalBinding('http2')`: the constants and `nameForErrorCode`.
pub(crate) fn binding(c: &mut Context<'_>) -> Outcome {
    let module = module(c)?;
    let binding = c.object_rooted()?;
    let constants = get(c, module, "constants")?;
    put(c, binding, "constants", constants)?;
    let names = c.object_rooted()?;
    for code in frame::ErrorCode::ALL {
        let name = c.string_rooted(code.name());
        put(c, names, &(code as u32).to_string(), name)?;
    }
    put(c, binding, "nameForErrorCode", names)?;
    Ok(binding)
}

/// Replace a class root.
fn set_class(c: &mut Context<'_>, which: Class, value: RootId) -> Result<(), RootedError> {
    let retained = c.retain(value)?;
    let previous = with_state(c, |state| {
        let intrinsics = state.intrinsics.as_mut()?;
        Some(std::mem::replace(&mut intrinsics.classes[which as usize], retained))
    });
    if let Some(previous) = previous {
        c.release_root(previous);
    }
    Ok(())
}

/// `http2.constants`: a plain object of read-only, enumerable values.
fn constants_object(c: &mut Context<'_>) -> Outcome {
    let object = c.object_rooted()?;
    for (name, constant) in constants::CONSTANTS {
        let value = match constant {
            constants::Constant::Number(number) => c.number(number),
            constants::Constant::Text(text) => c.string_rooted(text),
        };
        let key = c.string_rooted(name);
        c.define_data_property_rooted(object, key, value, false, true, false)?;
    }
    Ok(object)
}

pub(crate) fn module_member(c: &mut Context<'_>, _this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Module>(c)? {
        Module::Connect => session::connect(c, at(0), at(1), at(2)),
        Module::CreateServer => {
            let (mut options, mut handler) = (at(0), at(1));
            if c.is_callable_rooted(options)? {
                handler = options;
                options = c.object_rooted()?;
            }
            let class = class(c, Class::Http2Server)?;
            c.construct_rooted(class, class, &[options, handler])
        }
        Module::CreateSecureServer => {
            let class = class(c, Class::Http2SecureServer)?;
            c.construct_rooted(class, class, &[at(0), at(1)])
        }
        Module::GetDefaultSettings => settings::default_settings(c),
        Module::GetPackedSettings => settings::packed(c, at(0)),
        Module::GetUnpackedSettings => settings::unpacked(c, at(0), at(1)),
        Module::PerformServerHandshake => {
            let options = server::initialize_options(c, at(1))?;
            let class = class(c, Class::ServerHttp2Session)?;
            c.construct_rooted(class, class, &[options, at(0), undefined])
        }
        Module::Emit => {
            let Some((&target, rest)) = args.split_first() else { return Ok(undefined) };
            call_method(c, target, "emit", rest)?;
            Ok(undefined)
        }
    }
}

/// `new URL(text)`.
fn url(c: &mut Context<'_>, text: RootId) -> Outcome {
    let global = c.global_root()?;
    let class = get(c, global, "URL")?;
    c.construct_rooted(class, class, &[text])
}

/// Fault values whose message shows a number as JavaScript prints it.
fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e21 { format!("{}", value as i64) } else { format!("{value}") }
}

fn throw(c: &mut Context<'_>, fault: Fault, values: &[&str]) -> RootedError {
    fault.throw(c, values)
}

/// `process.emitWarning(message[, kind])`.
fn emit_warning(c: &mut Context<'_>, message: &str, kind: Option<&str>) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    let mut values = vec![c.string_rooted(message)];
    if let Some(kind) = kind {
        values.push(c.string_rooted(kind));
    }
    call_method(c, process, "emitWarning", &values).map(drop)
}
