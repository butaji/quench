//! `dgram` (lib/dgram.js) over `udp_wrap`'s UDP handles: a Socket is an EventEmitter whose
//! private state names its handle and its bind and connect phases. Sends and connects made
//! before the socket is bound wait in its queue until it is listening.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

/// A socket's bind phase.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BindState {
    Unbound,
    Binding,
    Bound,
}

/// A socket's connect phase.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectState {
    Disconnected,
    Connecting,
    Connected,
}

/// The phases a state field records, by their position.
trait Phase: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    const FIELD: &'static str;
}

impl Phase for BindState {
    const ALL: &'static [Self] = &[Self::Unbound, Self::Binding, Self::Bound];
    const FIELD: &'static str = "bindState";
}

impl Phase for ConnectState {
    const ALL: &'static [Self] = &[Self::Disconnected, Self::Connecting, Self::Connected];
    const FIELD: &'static str = "connectState";
}

/// The socket types and the handle methods each selects.
const TYPES: [(&str, Family); 2] = [("udp4", Family::V4), ("udp6", Family::V6)];

#[derive(Clone, Copy)]
enum Family {
    V4,
    V6,
}

impl Family {
    /// `(bind, connect, send)` of the handle prototype.
    fn methods(self) -> [&'static str; 3] {
        match self {
            Self::V4 => ["bind", "connect", "send"],
            Self::V6 => ["bind6", "connect6", "send6"],
        }
    }
    fn number(self) -> f64 {
        f64::from(match self {
            Self::V4 => super::net_config::IPV4_FAMILY,
            Self::V6 => super::net_config::IPV6_FAMILY,
        })
    }
    /// The address `lookup` resolves when none is given.
    fn loopback(self) -> &'static str {
        match self {
            Self::V4 => "127.0.0.1",
            Self::V6 => "::1",
        }
    }
    /// The address `bind` listens on when none is given.
    fn any(self) -> &'static str {
        match self {
            Self::V4 => "0.0.0.0",
            Self::V6 => "::",
        }
    }
}

/// `udp_wrap` flags for the reuse and IPv6-only options.
const FLAG_OPTIONS: [(&str, &str); 3] = [
    ("reuseAddr", "UV_UDP_REUSEADDR"),
    ("ipv6Only", "UV_UDP_IPV6ONLY"),
    ("reusePort", "UV_UDP_REUSEPORT"),
];
const MAX_PORT: f64 = u16::MAX as f64;
/// The `net.BlockList`s a socket filters its peers through.
#[derive(Clone, Copy)]
enum BlockListOption {
    /// Destinations `connect` and `send` refuse with ERR_IP_BLOCKED.
    Send,
    /// Sources whose datagrams are dropped.
    Receive,
}

impl BlockListOption {
    const ALL: [Self; 2] = [Self::Receive, Self::Send];
    fn name(self) -> &'static str {
        match self {
            Self::Send => "sendBlockList",
            Self::Receive => "receiveBlockList",
        }
    }
}

/// Options `validateUint32` checks when they are truthy.
const BUFFER_SIZE_OPTIONS: [&str; 2] = ["recvBufferSize", "sendBufferSize"];

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)?;
    Ok(())
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined() || value.is_null())
}

fn number(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

fn call_method(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, name)?;
    c.call_rooted(function, object, args)
}

fn emit(c: &mut Context<'_>, target: RootId, event: &str, args: &[RootId]) -> Result<(), RootedError> {
    let event = c.string_rooted(event);
    let mut arguments = vec![event];
    arguments.extend_from_slice(args);
    call_method(c, target, "emit", &arguments)?;
    Ok(())
}

fn closure(c: &mut Context<'_>, operation: &str, captures: &[RootId]) -> Outcome {
    let data = c.array_rooted(captures)?;
    c.host_function_with_data(crate::host::shared_vm::operation(operation), data)
}

fn capture(c: &mut Context<'_>, index: usize) -> Outcome {
    let data = c.host_function_data()?;
    get(c, data, &index.to_string())
}

fn next_tick(c: &mut Context<'_>, callback: RootId, args: &[RootId]) -> Result<(), RootedError> {
    super::stream_shared_vm::next_tick(c, callback, args)
}

fn coded(c: &mut Context<'_>, code: &'static str, message: &str) -> RootedError {
    match c.error_rooted(message) {
        Ok(error) => match super::node_errors_shared_vm::decorate_error(c, error, code) {
            Ok(()) => c.throw(error),
            Err(failure) => failure,
        },
        Err(failure) => failure,
    }
}

fn coded_type(c: &mut Context<'_>, code: &'static str, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => match super::node_errors_shared_vm::decorate_type_error(c, error, code) {
            Ok(()) => c.throw(error),
            Err(failure) => failure,
        },
        Err(failure) => failure,
    }
}

/// Node's `ErrnoException(err, syscall)`.
fn errno_exception(c: &mut Context<'_>, status: RootId, syscall: &str) -> Outcome {
    let syscall = c.string_rooted(syscall);
    let undefined = c.undefined();
    super::util_module_shared_vm::errno_exception(c, undefined, &[status, syscall])
}

/// Node's `ExceptionWithHostPort(err, syscall, address, port)`.
fn host_port_exception(c: &mut Context<'_>, status: RootId, syscall: &str, address: RootId, port: RootId) -> Outcome {
    let syscall = c.string_rooted(syscall);
    let undefined = c.undefined();
    super::util_module_shared_vm::exception_with_host_port(c, undefined, &[status, syscall, address, port])
}

/// Throw `ErrnoException(status, syscall)` for a failed handle call.
fn check(c: &mut Context<'_>, status: RootId, syscall: &str) -> Result<(), RootedError> {
    if c.truthy_rooted(status)? {
        let error = errno_exception(c, status, syscall)?;
        return Err(c.throw(error));
    }
    Ok(())
}

/// The module's fixed members.
struct Intrinsics {
    state_slot: RootId,
    owner: RootId,
    binding: RootId,
    socket: RootId,
}

fn intrinsics(c: &mut Context<'_>) -> Result<Intrinsics, RootedError> {
    let holder = crate::host::shared_vm::commonjs::cached_native_binding(c, "\0dgram:intrinsics", build)?;
    Ok(Intrinsics {
        state_slot: get(c, holder, "stateSlot")?,
        owner: get(c, holder, "owner")?,
        binding: get(c, holder, "binding")?,
        socket: get(c, holder, "Socket")?,
    })
}

const PROTOTYPE_METHODS: [(&str, &str); 26] = [
    ("bind", "dgramBind"),
    ("connect", "dgramConnect"),
    ("disconnect", "dgramDisconnect"),
    ("send", "dgramSend"),
    ("sendto", "dgramSendTo"),
    ("close", "dgramClose"),
    ("address", "dgramAddress"),
    ("remoteAddress", "dgramRemoteAddress"),
    ("setBroadcast", "dgramSetBroadcast"),
    ("setTTL", "dgramSetTtl"),
    ("setMulticastTTL", "dgramSetMulticastTtl"),
    ("setMulticastLoopback", "dgramSetMulticastLoopback"),
    ("setMulticastInterface", "dgramSetMulticastInterface"),
    ("addMembership", "dgramAddMembership"),
    ("dropMembership", "dgramDropMembership"),
    ("addSourceSpecificMembership", "dgramAddSourceMembership"),
    ("dropSourceSpecificMembership", "dgramDropSourceMembership"),
    ("ref", "dgramRef"),
    ("unref", "dgramUnref"),
    ("setRecvBufferSize", "dgramSetRecvBufferSize"),
    ("setSendBufferSize", "dgramSetSendBufferSize"),
    ("getRecvBufferSize", "dgramGetRecvBufferSize"),
    ("getSendBufferSize", "dgramGetSendBufferSize"),
    ("getSendQueueSize", "dgramGetSendQueueSize"),
    ("getSendQueueCount", "dgramGetSendQueueCount"),
    ("_healthCheck", "dgramHealthCheck"),
];

fn build(c: &mut Context<'_>) -> Outcome {
    let holder = c.null_object_rooted()?;
    let slot = c.private_symbol_rooted(Some("state symbol"));
    put(c, holder, "stateSlot", slot)?;
    let symbols = super::util_shared_vm::symbols_binding(c)?;
    let owner = get(c, symbols, "owner_symbol")?;
    put(c, holder, "owner", owner)?;
    let binding = super::udp_binding::binding(c)?;
    put(c, holder, "binding", binding)?;
    let emitter_api = c
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventEmitter configuration missing"))?;
    let emitter = get(c, emitter_api, "emitterCtor")?;
    let socket = c.host_constructor_named_with_data(crate::host::shared_vm::operation("dgramSocketConstruct"), "Socket", emitter)?;
    let prototype = get(c, socket, "prototype")?;
    let emitter_prototype = get(c, emitter, "prototype")?;
    c.set_prototype_rooted(prototype, emitter_prototype)?;
    c.set_prototype_rooted(socket, emitter)?;
    for (name, operation) in PROTOTYPE_METHODS {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    let dispose = c.host_function_named(crate::host::shared_vm::operation("dgramAsyncDispose"), "[Symbol.asyncDispose]")?;
    let key = super::fs_shared_vm::classes::well_known_symbol(c, "asyncDispose", "nodejs.asyncDispose")?;
    c.define_data_property_rooted(prototype, key, dispose, true, false, true)?;
    put(c, holder, "Socket", socket)?;
    Ok(holder)
}

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let module = c.object_rooted()?;
    let create = c.host_function_named(crate::host::shared_vm::operation("dgramCreateSocket"), "createSocket")?;
    put(c, module, "createSocket", create)?;
    let socket = intrinsics(c)?.socket;
    put(c, module, "Socket", socket)?;
    Ok(module)
}

fn state(c: &mut Context<'_>, socket: RootId) -> Outcome {
    let slot = intrinsics(c)?.state_slot;
    let state = c.get_property_rooted(socket, slot)?;
    if is_undefined(c, state) {
        let error = c.type_error_rooted("Illegal invocation")?;
        return Err(c.throw(error));
    }
    Ok(state)
}

fn phase<P: Phase>(c: &mut Context<'_>, state: RootId) -> Result<P, RootedError> {
    let value = get(c, state, P::FIELD)?;
    let index = number(c, value).unwrap_or(0.0) as usize;
    Ok(P::ALL.get(index).copied().unwrap_or(P::ALL[0]))
}

fn set_phase<P: Phase>(c: &mut Context<'_>, state: RootId, phase: P) -> Result<(), RootedError> {
    let index = P::ALL.iter().position(|candidate| *candidate == phase).expect("listed phase");
    let value = c.number(index as f64);
    put(c, state, P::FIELD, value)
}

fn handle(c: &mut Context<'_>, state: RootId) -> Outcome {
    get(c, state, "handle")
}

/// `healthCheck`: the socket's handle, or ERR_SOCKET_DGRAM_NOT_RUNNING once closed.
fn running_handle(c: &mut Context<'_>, socket: RootId) -> Outcome {
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    if is_nullish(c, handle) {
        return Err(coded(c, "ERR_SOCKET_DGRAM_NOT_RUNNING", "Not running"));
    }
    Ok(handle)
}

pub(crate) fn health_check(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    running_handle(c, this)?;
    Ok(c.undefined())
}

/// A UDP handle of `type`, its methods and lookup bound to the family.
fn new_handle(c: &mut Context<'_>, kind: RootId, lookup: RootId) -> Outcome {
    let Some(family) = c
        .string_text(kind)?
        .and_then(|text| TYPES.iter().find(|(name, _)| *name == text).map(|(_, family)| *family))
    else {
        return Err(coded_type(c, "ERR_SOCKET_BAD_TYPE", "Bad socket type specified. Valid types are: udp4, udp6"));
    };
    let lookup = if is_undefined(c, lookup) {
        let dns = crate::host::shared_vm::commonjs::dns_module(c)?;
        get(c, dns, "lookup")?
    } else {
        if !c.is_callable_rooted(lookup)? {
            return Err(super::util_shared_vm::argument_type_error(c, lookup, "lookup", "function")?);
        }
        lookup
    };
    let binding = intrinsics(c)?.binding;
    let udp = get(c, binding, "UDP")?;
    let handle = c.construct_rooted(udp, udp, &[])?;
    let family_number = c.number(family.number());
    let loopback = c.string_rooted(family.loopback());
    let resolve = closure(c, "dgramHandleLookup", &[lookup, family_number, loopback])?;
    put(c, handle, "lookup", resolve)?;
    for (name, method) in ["bind", "connect", "send"].into_iter().zip(family.methods()) {
        let function = get(c, handle, method)?;
        put(c, handle, name, function)?;
    }
    Ok(handle)
}

/// `handle.lookup(address, callback)`: `lookup(address || loopback, family, callback)`.
pub(crate) fn handle_lookup(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let lookup = capture(c, 0)?;
    let family = capture(c, 1)?;
    let loopback = capture(c, 2)?;
    let (address, callback) = (arg(c, args, 0), arg(c, args, 1));
    let address = if c.truthy_rooted(address)? { address } else { loopback };
    let undefined = c.undefined();
    c.call_rooted(lookup, undefined, &[address, family, callback])
}

/// `new dgram.Socket(type, listener)`.
pub(crate) fn socket_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if c.new_target_rooted().is_none() {
        let socket = intrinsics(c)?.socket;
        return c.construct_rooted(socket, socket, args);
    }
    let emitter = c.host_function_data()?;
    c.call_rooted(emitter, this, &[])?;
    let (mut kind, listener) = (arg(c, args, 0), arg(c, args, 1));
    let state = c.object_rooted()?;
    let mut lookup = c.undefined();
    let mut options = c.undefined();
    if c.is_object_rooted(kind)? && !c.is_callable_rooted(kind)? {
        options = kind;
        kind = get(c, options, "type")?;
        lookup = get(c, options, "lookup")?;
        for name in ["recvBufferSize", "sendBufferSize", "reuseAddr", "reusePort", "ipv6Only"] {
            let value = get(c, options, name)?;
            if BUFFER_SIZE_OPTIONS.contains(&name) && c.truthy_rooted(value)? {
                let maximum = c.number(f64::from(u32::MAX));
                super::util_shared_vm::validate_integer(c, value, &format!("options.{name}"), 0.0, maximum)?;
            }
            put(c, state, name, value)?;
        }
        for option in BlockListOption::ALL {
            let list = get(c, options, option.name())?;
            if c.truthy_rooted(list)? {
                let net = crate::host::shared_vm::commonjs::net_module(c)?;
                let class = get(c, net, "BlockList")?;
                let valid = call_method(c, class, "isBlockList", &[list])?;
                if !c.truthy_rooted(valid)? {
                    let name = format!("options.{}", option.name());
                    return Err(super::util_shared_vm::argument_type_error_with_expected(c, list, &name, "an net.BlockList")?);
                }
                put(c, state, option.name(), list)?;
            }
        }
    }
    let handle = new_handle(c, kind, lookup)?;
    let intrinsics = intrinsics(c)?;
    c.set_property_rooted(handle, intrinsics.owner, this, handle)?;
    put(c, this, "type", kind)?;
    if c.is_callable_rooted(listener)? {
        let event = c.string_rooted("message");
        call_method(c, this, "on", &[event, listener])?;
    }
    put(c, state, "handle", handle)?;
    let no = c.boolean(false);
    put(c, state, "receiving", no)?;
    set_phase(c, state, BindState::Unbound)?;
    set_phase(c, state, ConnectState::Disconnected)?;
    c.define_data_property_rooted(this, intrinsics.state_slot, state, false, false, false)?;
    if c.is_object_rooted(options)? {
        let signal = get(c, options, "signal")?;
        if !is_undefined(c, signal) {
            super::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
            let close = closure(c, "dgramCloseOnAbort", &[this])?;
            let aborted = get(c, signal, "aborted")?;
            if c.truthy_rooted(aborted)? {
                next_tick(c, close, &[])?;
            } else {
                let event = c.string_rooted("abort");
                let once = c.object_rooted()?;
                let yes = c.boolean(true);
                put(c, once, "once", yes)?;
                call_method(c, signal, "addEventListener", &[event, close, once])?;
            }
        }
    }
    Ok(this)
}

pub(crate) fn close_on_abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    if !is_nullish(c, handle) {
        call_method(c, socket, "close", &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn create_socket(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let socket = intrinsics(c)?.socket;
    let (kind, listener) = (arg(c, args, 0), arg(c, args, 1));
    c.construct_rooted(socket, socket, &[kind, listener])
}

fn buffer_size(c: &mut Context<'_>, socket: RootId, size: RootId, receive: bool) -> Outcome {
    let valid = number(c, size).is_some_and(|value| value >= 0.0 && value.fract() == 0.0 && value <= f64::from(u32::MAX));
    if !valid {
        return Err(coded_type(c, "ERR_SOCKET_BAD_BUFFER_SIZE", "Buffer size must be a positive integer"));
    }
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    let context = c.object_rooted()?;
    let receive = c.boolean(receive);
    let result = call_method(c, handle, "bufferSize", &[size, receive, context])?;
    if is_undefined(c, result) {
        return Err(super::util_system_error_shared_vm::coded_system_error(
            c,
            context,
            "ERR_SOCKET_BUFFER_SIZE",
            "Could not get or set buffer size",
        )?);
    }
    Ok(result)
}

fn start_listening(c: &mut Context<'_>, socket: RootId) -> Result<(), RootedError> {
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    let on_message = c.host_function_named(crate::host::shared_vm::operation("dgramOnMessage"), "onMessage")?;
    put(c, handle, "onmessage", on_message)?;
    let on_error = c.host_function_named(crate::host::shared_vm::operation("dgramOnError"), "onError")?;
    put(c, handle, "onerror", on_error)?;
    call_method(c, handle, "recvStart", &[])?;
    let yes = c.boolean(true);
    put(c, state, "receiving", yes)?;
    set_phase(c, state, BindState::Bound)?;
    for (name, receive) in [("recvBufferSize", true), ("sendBufferSize", false)] {
        let size = get(c, state, name)?;
        if c.truthy_rooted(size)? {
            buffer_size(c, socket, size, receive)?;
        }
    }
    emit(c, socket, "listening", &[])
}

fn owner(c: &mut Context<'_>, handle: RootId) -> Outcome {
    let symbol = intrinsics(c)?.owner;
    c.get_property_rooted(handle, symbol)
}

/// `onmessage(nread, handle, buf, rinfo)`.
pub(crate) fn on_message(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (nread, handle, buffer, rinfo) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let socket = owner(c, handle)?;
    if number(c, nread).is_some_and(|nread| nread < 0.0) {
        let error = errno_exception(c, nread, "recvmsg")?;
        emit(c, socket, "error", &[error])?;
        return Ok(c.undefined());
    }
    let state = state(c, socket)?;
    let address = get(c, rinfo, "address")?;
    let family = get(c, rinfo, "family")?;
    let family = c.to_string(family)?.to_lowercase();
    if blocked(c, state, BlockListOption::Receive, address, Some(family))? {
        return Ok(c.undefined());
    }
    let size = get(c, buffer, "length")?;
    put(c, rinfo, "size", size)?;
    emit(c, socket, "message", &[buffer, rinfo])?;
    Ok(c.undefined())
}

pub(crate) fn on_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (handle, error) = (arg(c, args, 1), arg(c, args, 2));
    let socket = owner(c, handle)?;
    emit(c, socket, "error", &[error])?;
    Ok(c.undefined())
}

/// Queue `entry` until the socket is listening (`enqueue`).
fn enqueue(c: &mut Context<'_>, socket: RootId, entry: RootId) -> Result<(), RootedError> {
    let state = state(c, socket)?;
    let queue = get(c, state, "queue")?;
    let queue = if is_undefined(c, queue) {
        let queue = c.array_rooted(&[])?;
        put(c, state, "queue", queue)?;
        let emitter_api = c
            .host_mut()
            .shared_state()
            .borrow()
            .event_targets_api
            .ok_or_else(|| RootedError::host("EventEmitter configuration missing"))?;
        let emitter = get(c, emitter_api, "emitterCtor")?;
        let monitor = get(c, emitter, "errorMonitor")?;
        let success = closure(c, "dgramListenSuccess", &[socket, monitor])?;
        let failure = closure(c, "dgramListenError", &[socket, success])?;
        put(c, state, "queueFailure", failure)?;
        if c.is_symbol_rooted(monitor)? {
            call_method(c, socket, "once", &[monitor, failure])?;
        }
        let event = c.string_rooted("listening");
        call_method(c, socket, "once", &[event, success])?;
        queue
    } else {
        queue
    };
    call_method(c, queue, "push", &[entry])?;
    Ok(())
}

pub(crate) fn listen_success(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let monitor = capture(c, 1)?;
    let state = state(c, socket)?;
    let failure = get(c, state, "queueFailure")?;
    if c.is_symbol_rooted(monitor)? {
        call_method(c, socket, "removeListener", &[monitor, failure])?;
    }
    let queue = get(c, state, "queue")?;
    let undefined = c.undefined();
    put(c, state, "queue", undefined)?;
    if c.is_object_rooted(queue)? {
        for entry in c.array_like_values_rooted(queue)? {
            c.call_rooted(entry, undefined, &[])?;
        }
    }
    Ok(undefined)
}

pub(crate) fn listen_error(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let success = capture(c, 1)?;
    let event = c.string_rooted("listening");
    call_method(c, socket, "removeListener", &[event, success])?;
    let state = state(c, socket)?;
    let undefined = c.undefined();
    put(c, state, "queue", undefined)?;
    Ok(undefined)
}

/// `bind(port, address, callback)` / `bind(options, callback)`.
pub(crate) fn bind(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let handle = running_handle(c, this)?;
    let state = state(c, this)?;
    if phase::<BindState>(c, state)? != BindState::Unbound {
        return Err(coded(c, "ERR_SOCKET_ALREADY_BOUND", "Socket is already bound"));
    }
    set_phase(c, state, BindState::Binding)?;
    if let Some(&callback) = args.last() {
        if c.is_callable_rooted(callback)? {
            let pair = c.array_rooted(&[])?;
            let remove = closure(c, "dgramBindRemove", &[this, pair])?;
            let listening = closure(c, "dgramBindListening", &[this, pair, callback])?;
            put(c, pair, "0", remove)?;
            put(c, pair, "1", listening)?;
            let error_event = c.string_rooted("error");
            call_method(c, this, "on", &[error_event, remove])?;
            let listening_event = c.string_rooted("listening");
            call_method(c, this, "on", &[listening_event, listening])?;
        }
    }
    let first = arg(c, args, 0);
    let (port, address) = if c.is_object_rooted(first)? && !c.is_callable_rooted(first)? {
        let address = get(c, first, "address")?;
        (get(c, first, "port")?, address)
    } else {
        let address = arg(c, args, 1);
        let address = if c.is_callable_rooted(address)? { c.undefined() } else { address };
        (first, address)
    };
    let address = if c.truthy_rooted(address)? {
        address
    } else {
        let kind = get(c, this, "type")?;
        let family = if c.string_text(kind)?.as_deref() == Some("udp6") { TYPES[1].1 } else { TYPES[0].1 };
        c.string_rooted(family.any())
    };
    let bound = closure(c, "dgramAfterBindLookup", &[this, port])?;
    call_method(c, handle, "lookup", &[address, bound])?;
    Ok(this)
}

pub(crate) fn bind_remove(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let pair = capture(c, 1)?;
    let remove = get(c, pair, "0")?;
    let listening = get(c, pair, "1")?;
    let error_event = c.string_rooted("error");
    call_method(c, socket, "removeListener", &[error_event, remove])?;
    let listening_event = c.string_rooted("listening");
    call_method(c, socket, "removeListener", &[listening_event, listening])?;
    Ok(c.undefined())
}

pub(crate) fn bind_listening(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let pair = capture(c, 1)?;
    let callback = capture(c, 2)?;
    let remove = get(c, pair, "0")?;
    c.call_rooted(remove, socket, &[])?;
    c.call_rooted(callback, socket, &[])
}

/// The bind's address resolved: bind the handle and start listening.
pub(crate) fn after_bind_lookup(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let port = capture(c, 1)?;
    let (error, ip) = (arg(c, args, 0), arg(c, args, 1));
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    if is_nullish(c, handle) {
        return Ok(c.undefined());
    }
    if c.truthy_rooted(error)? {
        set_phase(c, state, BindState::Unbound)?;
        emit(c, socket, "error", &[error])?;
        return Ok(c.undefined());
    }
    let binding = intrinsics(c)?.binding;
    let constants = get(c, binding, "constants")?;
    let mut flags = 0u32;
    for (option, flag) in FLAG_OPTIONS {
        let enabled = get(c, state, option)?;
        if c.truthy_rooted(enabled)? {
            let value = get(c, constants, flag)?;
            flags |= number(c, value).unwrap_or(0.0) as u32;
        }
    }
    let flags = c.number(f64::from(flags));
    let bound_port = if c.truthy_rooted(port)? { port } else { c.number(0.0) };
    let status = call_method(c, handle, "bind", &[ip, bound_port, flags])?;
    if c.truthy_rooted(status)? {
        let error = host_port_exception(c, status, "bind", ip, port)?;
        set_phase(c, state, BindState::Unbound)?;
        emit(c, socket, "error", &[error])?;
        return Ok(c.undefined());
    }
    start_listening(c, socket)?;
    Ok(c.undefined())
}

/// Node's `validatePort(port, 'Port', allowZero)`.
fn validate_port(c: &mut Context<'_>, port: RootId, allow_zero: bool) -> Result<RootId, RootedError> {
    let candidate = match number(c, port) {
        Some(value) => Some(value),
        None => match c.string_text(port)? {
            Some(text) if !text.trim().is_empty() => Some(c.to_number(port)?),
            _ => None,
        },
    };
    match candidate {
        Some(value) if value.fract() == 0.0 && (0.0..=MAX_PORT).contains(&value) && (allow_zero || value != 0.0) => {
            Ok(c.number(value))
        }
        _ => {
            let received = super::util_shared_vm::argument_type_description(c, port)?;
            let relation = if allow_zero { ">=" } else { ">" };
            let limit = u32::from(u16::MAX) + 1;
            let error = c.range_error_rooted(&format!("Port should be {relation} 0 and < {limit}. Received {received}."))?;
            super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_SOCKET_BAD_PORT")?;
            Err(c.throw(error))
        }
    }
}

/// Bind an unbound socket to an ephemeral port before it sends or connects.
fn bind_implicitly(c: &mut Context<'_>, socket: RootId, state: RootId) -> Result<(), RootedError> {
    if phase::<BindState>(c, state)? == BindState::Unbound {
        let options = c.object_rooted()?;
        let zero = c.number(0.0);
        put(c, options, "port", zero)?;
        let yes = c.boolean(true);
        put(c, options, "exclusive", yes)?;
        let null = c.null();
        bind(c, socket, &[options, null])?;
    }
    Ok(())
}

pub(crate) fn connect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let port = arg(c, args, 0);
    let port = validate_port(c, port, false)?;
    let (mut address, mut callback) = (arg(c, args, 1), arg(c, args, 2));
    if c.is_callable_rooted(address)? {
        callback = address;
        address = c.string_rooted("");
    } else if is_undefined(c, address) {
        address = c.string_rooted("");
    }
    if c.string_text(address)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, address, "address", "string")?);
    }
    let state = state(c, this)?;
    if phase::<ConnectState>(c, state)? != ConnectState::Disconnected {
        return Err(coded(c, "ERR_SOCKET_DGRAM_IS_CONNECTED", "Already connected"));
    }
    set_phase(c, state, ConnectState::Connecting)?;
    bind_implicitly(c, this, state)?;
    let pending = closure(c, "dgramConnectNow", &[this, port, address, callback])?;
    if phase::<BindState>(c, state)? != BindState::Bound {
        enqueue(c, this, pending)?;
    } else {
        let undefined = c.undefined();
        c.call_rooted(pending, undefined, &[])?;
    }
    Ok(c.undefined())
}

/// `_connect(port, address, callback)` once bound.
pub(crate) fn connect_now(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let port = capture(c, 1)?;
    let address = capture(c, 2)?;
    let callback = capture(c, 3)?;
    if c.is_callable_rooted(callback)? {
        let event = c.string_rooted("connect");
        call_method(c, socket, "once", &[event, callback])?;
    }
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    let resolved = closure(c, "dgramAfterConnectLookup", &[socket, port, address, callback])?;
    call_method(c, handle, "lookup", &[address, resolved])?;
    Ok(c.undefined())
}

pub(crate) fn after_connect_lookup(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let port = capture(c, 1)?;
    let address = capture(c, 2)?;
    let callback = capture(c, 3)?;
    let (mut error, ip) = (arg(c, args, 0), arg(c, args, 1));
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    if is_nullish(c, handle) {
        return Ok(c.undefined());
    }
    if !c.truthy_rooted(error)? && blocked(c, state, BlockListOption::Send, ip, None)? {
        error = ip_blocked(c, ip)?;
    }
    if !c.truthy_rooted(error)? {
        let status = call_method(c, handle, "connect", &[ip, port])?;
        if c.truthy_rooted(status)? {
            error = host_port_exception(c, status, "connect", address, port)?;
        }
    }
    if c.truthy_rooted(error)? {
        set_phase(c, state, ConnectState::Disconnected)?;
        let report = closure(c, "dgramConnectFailed", &[socket, callback, error])?;
        next_tick(c, report, &[])?;
        return Ok(c.undefined());
    }
    set_phase(c, state, ConnectState::Connected)?;
    let event = c.string_rooted("connect");
    let announce = closure(c, "dgramEmitEvent", &[socket, event])?;
    next_tick(c, announce, &[])?;
    Ok(c.undefined())
}

pub(crate) fn connect_failed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let callback = capture(c, 1)?;
    let error = capture(c, 2)?;
    if c.is_callable_rooted(callback)? {
        let event = c.string_rooted("connect");
        call_method(c, socket, "removeListener", &[event, callback])?;
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[error])?;
    } else {
        emit(c, socket, "error", &[error])?;
    }
    Ok(c.undefined())
}

pub(crate) fn emit_event(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let event = capture(c, 1)?;
    let event = c.to_string(event)?;
    emit(c, socket, &event, &[])?;
    Ok(c.undefined())
}

pub(crate) fn disconnect(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    if phase::<ConnectState>(c, state)? != ConnectState::Connected {
        return Err(coded(c, "ERR_SOCKET_DGRAM_NOT_CONNECTED", "Not connected"));
    }
    let handle = handle(c, state)?;
    let status = call_method(c, handle, "disconnect", &[])?;
    check(c, status, "connect")?;
    set_phase(c, state, ConnectState::Disconnected)?;
    Ok(c.undefined())
}

const BUFFER_TYPES: &str = "of type string or an instance of Buffer, TypedArray, or DataView";

fn is_view(c: &Context<'_>, value: RootId) -> bool {
    c.with_view_bytes_rooted(value, |_| ()).is_some()
}

fn buffer_class(c: &mut Context<'_>) -> Outcome {
    let global = c.global_root()?;
    get(c, global, "Buffer")
}

/// A Buffer over the bytes of `view`.
fn view_buffer(c: &mut Context<'_>, view: RootId) -> Outcome {
    let buffer = buffer_class(c)?;
    let store = get(c, view, "buffer")?;
    let offset = get(c, view, "byteOffset")?;
    let length = get(c, view, "byteLength")?;
    call_method(c, buffer, "from", &[store, offset, length])
}

/// `sliceBuffer(buffer, offset, length)`.
fn slice_buffer(c: &mut Context<'_>, buffer: RootId, offset: RootId, length: RootId) -> Outcome {
    let buffer = if c.string_text(buffer)?.is_some() {
        let class = buffer_class(c)?;
        call_method(c, class, "from", &[buffer])?
    } else if is_view(c, buffer) {
        buffer
    } else {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, buffer, "buffer", BUFFER_TYPES)?);
    };
    let offset = f64::from(super::native_number::uint32(c.to_number(offset)?));
    let length = f64::from(super::native_number::uint32(c.to_number(length)?));
    let size = get(c, buffer, "byteLength")?;
    let size = number(c, size).unwrap_or(0.0);
    for (name, end) in [("offset", offset), ("length", offset + length)] {
        if end > size {
            let error = c.range_error_rooted(&format!("\"{name}\" is outside of buffer bounds"))?;
            super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_BUFFER_OUT_OF_BOUNDS")?;
            return Err(c.throw(error));
        }
    }
    let class = buffer_class(c)?;
    let store = get(c, buffer, "buffer")?;
    let base = get(c, buffer, "byteOffset")?;
    let base = number(c, base).unwrap_or(0.0);
    let start = c.number(base + offset);
    let length = c.number(length);
    call_method(c, class, "from", &[store, start, length])
}

/// `buffer` as a list of Buffers, or None when an entry is not bytes.
fn buffer_list(c: &mut Context<'_>, buffer: RootId) -> Result<Option<Vec<RootId>>, RootedError> {
    if !c.is_array_rooted(buffer)? {
        if c.string_text(buffer)?.is_some() {
            let class = buffer_class(c)?;
            return Ok(Some(vec![call_method(c, class, "from", &[buffer])?]));
        }
        if !is_view(c, buffer) {
            return Err(super::util_shared_vm::argument_type_error_with_expected(c, buffer, "buffer", BUFFER_TYPES)?);
        }
        return Ok(Some(vec![buffer]));
    }
    let mut list = Vec::new();
    for entry in c.array_like_values_rooted(buffer)? {
        if c.string_text(entry)?.is_some() {
            let class = buffer_class(c)?;
            list.push(call_method(c, class, "from", &[entry])?);
        } else if is_view(c, entry) {
            list.push(view_buffer(c, entry)?);
        } else {
            return Ok(None);
        }
    }
    Ok(Some(list))
}

/// `send(msg[, offset, length], port[, address][, callback])`.
pub(crate) fn send(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    let connected = phase::<ConnectState>(c, state)? == ConnectState::Connected;
    let mut buffer = arg(c, args, 0);
    let offset = arg(c, args, 1);
    let length = arg(c, args, 2);
    let mut port = arg(c, args, 3);
    let mut address = arg(c, args, 4);
    let mut callback = arg(c, args, 5);
    if !connected {
        if c.truthy_rooted(address)? || (c.truthy_rooted(port)? && !c.is_callable_rooted(port)?) {
            buffer = slice_buffer(c, buffer, offset, length)?;
        } else {
            callback = port;
            port = offset;
            address = length;
        }
    } else {
        if number(c, length).is_some() {
            buffer = slice_buffer(c, buffer, offset, length)?;
            if c.is_callable_rooted(port)? {
                callback = port;
                port = c.null();
            } else {
                callback = address;
            }
        } else {
            callback = offset;
        }
        if c.truthy_rooted(port)? || c.truthy_rooted(address)? {
            return Err(coded(c, "ERR_SOCKET_DGRAM_IS_CONNECTED", "Already connected"));
        }
    }
    let Some(mut list) = buffer_list(c, buffer)? else {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, buffer, "buffer list arguments", BUFFER_TYPES)?);
    };
    if !connected {
        port = validate_port(c, port, false)?;
    }
    if !c.is_callable_rooted(callback)? {
        callback = c.undefined();
    }
    if c.is_callable_rooted(address)? {
        callback = address;
        address = c.undefined();
    } else if !is_nullish(c, address) && c.string_text(address)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, address, "address", "string")?);
    }
    running_handle(c, this)?;
    bind_implicitly(c, this, state)?;
    if list.is_empty() {
        let class = buffer_class(c)?;
        let zero = c.number(0.0);
        list.push(call_method(c, class, "alloc", &[zero])?);
    }
    let list = c.array_rooted(&list)?;
    if phase::<BindState>(c, state)? != BindState::Bound {
        let pending = closure(c, "dgramQueuedSend", &[this, list, port, address, callback])?;
        enqueue(c, this, pending)?;
        return Ok(c.undefined());
    }
    let resolved = closure(c, "dgramAfterSendLookup", &[this, list, address, port, callback])?;
    if connected {
        let null = c.null();
        let undefined = c.undefined();
        c.call_rooted(resolved, undefined, &[null, null])?;
    } else {
        let handle = handle(c, state)?;
        call_method(c, handle, "lookup", &[address, resolved])?;
    }
    Ok(c.undefined())
}

/// Whether the socket's `option` list blocks `ip`, in `family` (`ipv4`/`ipv6`) or the
/// family `isIP` reads from it.
fn blocked(c: &mut Context<'_>, state: RootId, option: BlockListOption, ip: RootId, family: Option<String>) -> Result<bool, RootedError> {
    let list = get(c, state, option.name())?;
    if !c.truthy_rooted(list)? {
        return Ok(false);
    }
    let family = match family {
        Some(family) => family,
        None => {
            let net = crate::host::shared_vm::commonjs::net_module(c)?;
            let version = call_method(c, net, "isIP", &[ip])?;
            format!("ipv{}", c.to_string(version)?)
        }
    };
    let family = c.string_rooted(&family);
    let verdict = call_method(c, list, "check", &[ip, family])?;
    c.truthy_rooted(verdict)
}

fn ip_blocked(c: &mut Context<'_>, ip: RootId) -> Outcome {
    let ip = c.to_string(ip)?;
    super::net_block_list::ip_blocked(c, &ip)
}

/// `sendto(buffer, offset, length, port, address, callback)`: `send` with every position
/// required.
pub(crate) fn send_to(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    for (index, name) in [(1, "offset"), (2, "length"), (3, "port")] {
        let value = arg(c, args, index);
        if number(c, value).is_none() {
            return Err(super::util_shared_vm::argument_type_error(c, value, name, "number")?);
        }
    }
    let address = arg(c, args, 4);
    if c.string_text(address)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, address, "address", "string")?);
    }
    send(c, this, args)
}

/// `[Symbol.asyncDispose]()`: close the socket unless it is closed already.
pub(crate) fn async_dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    let handle = handle(c, state)?;
    if is_nullish(c, handle) {
        return super::util_module_shared_vm::disposed_promise(c);
    }
    super::util_module_shared_vm::close_promise(c, this)
}

pub(crate) fn queued_send(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let list = capture(c, 1)?;
    let port = capture(c, 2)?;
    let address = capture(c, 3)?;
    let callback = capture(c, 4)?;
    call_method(c, socket, "send", &[list, port, address, callback])
}

/// `doSend`: the destination resolved, hand the datagram to the handle.
pub(crate) fn after_send_lookup(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let list = capture(c, 1)?;
    let address = capture(c, 2)?;
    let port = capture(c, 3)?;
    let callback = capture(c, 4)?;
    let (error, ip) = (arg(c, args, 0), arg(c, args, 1));
    let has_callback = c.is_callable_rooted(callback)?;
    if c.truthy_rooted(error)? {
        if has_callback {
            next_tick(c, callback, &[error])?;
        } else {
            let report = closure(c, "dgramEmitError", &[socket, error])?;
            next_tick(c, report, &[])?;
        }
        return Ok(c.undefined());
    }
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    if is_nullish(c, handle) {
        return Ok(c.undefined());
    }
    if c.truthy_rooted(ip)? && blocked(c, state, BlockListOption::Send, ip, None)? {
        if has_callback {
            let error = ip_blocked(c, ip)?;
            next_tick(c, callback, &[error])?;
        }
        return Ok(c.undefined());
    }
    let binding = intrinsics(c)?.binding;
    let send_wrap = get(c, binding, "SendWrap")?;
    let request = c.construct_rooted(send_wrap, send_wrap, &[])?;
    put(c, request, "list", list)?;
    put(c, request, "address", address)?;
    put(c, request, "port", port)?;
    if has_callback {
        put(c, request, "callback", callback)?;
        let complete = c.host_function_named(crate::host::shared_vm::operation("dgramAfterSend"), "afterSend")?;
        put(c, request, "oncomplete", complete)?;
    }
    let count = get(c, list, "length")?;
    let wants = c.boolean(has_callback);
    let status = if c.truthy_rooted(port)? {
        call_method(c, handle, "send", &[request, list, count, port, ip, wants])?
    } else {
        call_method(c, handle, "send", &[request, list, count, wants])?
    };
    let status_number = number(c, status).unwrap_or(0.0);
    if status_number >= 1.0 {
        if has_callback {
            let null = c.null();
            let sent = c.number(status_number - 1.0);
            next_tick(c, callback, &[null, sent])?;
        }
    } else if status_number != 0.0 && has_callback {
        let error = host_port_exception(c, status, "send", address, port)?;
        next_tick(c, callback, &[error])?;
    }
    Ok(c.undefined())
}

pub(crate) fn emit_error(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    let error = capture(c, 1)?;
    emit(c, socket, "error", &[error])?;
    Ok(c.undefined())
}

/// `afterSend(err, sent)` with the request as receiver.
pub(crate) fn after_send(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (status, sent) = (arg(c, args, 0), arg(c, args, 1));
    let error = if c.truthy_rooted(status)? {
        let address = get(c, this, "address")?;
        let port = get(c, this, "port")?;
        host_port_exception(c, status, "send", address, port)?
    } else {
        c.null()
    };
    let callback = get(c, this, "callback")?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error, sent])
}

pub(crate) fn close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    let callback = arg(c, args, 0);
    if c.is_callable_rooted(callback)? {
        let event = c.string_rooted("close");
        call_method(c, this, "on", &[event, callback])?;
    }
    let queue = get(c, state, "queue")?;
    if !is_undefined(c, queue) {
        let pending = closure(c, "dgramQueuedClose", &[this])?;
        call_method(c, queue, "push", &[pending])?;
        return Ok(this);
    }
    let handle = running_handle(c, this)?;
    if truthy(c, state, "receiving")? {
        call_method(c, handle, "recvStop", &[])?;
        let no = c.boolean(false);
        put(c, state, "receiving", no)?;
    }
    call_method(c, handle, "close", &[])?;
    let null = c.null();
    put(c, state, "handle", null)?;
    let event = c.string_rooted("close");
    let announce = closure(c, "dgramEmitEvent", &[this, event])?;
    next_tick(c, announce, &[])?;
    Ok(this)
}

fn truthy(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    c.truthy_rooted(value)
}

pub(crate) fn queued_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = capture(c, 0)?;
    call_method(c, socket, "close", &[])
}

fn name(c: &mut Context<'_>, socket: RootId, method: &str) -> Outcome {
    let handle = running_handle(c, socket)?;
    let out = c.object_rooted()?;
    let status = call_method(c, handle, method, &[out])?;
    check(c, status, method)?;
    Ok(out)
}

pub(crate) fn address(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    name(c, this, "getsockname")
}

pub(crate) fn remote_address(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    running_handle(c, this)?;
    let state = state(c, this)?;
    if phase::<ConnectState>(c, state)? != ConnectState::Connected {
        return Err(coded(c, "ERR_SOCKET_DGRAM_NOT_CONNECTED", "Not connected"));
    }
    name(c, this, "getpeername")
}

/// A boolean option: `handle[method](arg ? 1 : 0)`, returning `result`.
fn flag(c: &mut Context<'_>, socket: RootId, method: &str, value: RootId) -> Result<(), RootedError> {
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    let enabled = c.truthy_rooted(value)?;
    let enabled = c.number(if enabled { 1.0 } else { 0.0 });
    let status = call_method(c, handle, method, &[enabled])?;
    check(c, status, method)
}

pub(crate) fn set_broadcast(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    flag(c, this, "setBroadcast", value)?;
    Ok(c.undefined())
}

pub(crate) fn set_multicast_loopback(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    flag(c, this, "setMulticastLoopback", value)?;
    Ok(value)
}

fn ttl(c: &mut Context<'_>, socket: RootId, method: &str, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    if number(c, value).is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, value, "ttl", "number")?);
    }
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    let status = call_method(c, handle, method, &[value])?;
    check(c, status, method)?;
    Ok(value)
}

pub(crate) fn set_ttl(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    ttl(c, this, "setTTL", args)
}

pub(crate) fn set_multicast_ttl(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    ttl(c, this, "setMulticastTTL", args)
}

pub(crate) fn set_multicast_interface(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let handle = running_handle(c, this)?;
    let address = arg(c, args, 0);
    if c.string_text(address)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, address, "interfaceAddress", "string")?);
    }
    let status = call_method(c, handle, "setMulticastInterface", &[address])?;
    check(c, status, "setMulticastInterface")?;
    Ok(c.undefined())
}

fn membership(c: &mut Context<'_>, socket: RootId, method: &str, args: &[RootId]) -> Outcome {
    let handle = running_handle(c, socket)?;
    let (group, interface) = (arg(c, args, 0), arg(c, args, 1));
    if !c.truthy_rooted(group)? {
        return Err(coded_type(c, "ERR_MISSING_ARGS", "The \"multicastAddress\" argument must be specified"));
    }
    let status = call_method(c, handle, method, &[group, interface])?;
    check(c, status, method)?;
    Ok(c.undefined())
}

pub(crate) fn add_membership(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    membership(c, this, "addMembership", args)
}

pub(crate) fn drop_membership(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    membership(c, this, "dropMembership", args)
}

fn source_membership(c: &mut Context<'_>, socket: RootId, method: &str, args: &[RootId]) -> Outcome {
    let handle = running_handle(c, socket)?;
    let (source, group, interface) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    for (value, name) in [(source, "sourceAddress"), (group, "groupAddress")] {
        if c.string_text(value)?.is_none() {
            return Err(super::util_shared_vm::argument_type_error(c, value, name, "string")?);
        }
    }
    let status = call_method(c, handle, method, &[source, group, interface])?;
    check(c, status, method)?;
    Ok(c.undefined())
}

pub(crate) fn add_source_membership(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    source_membership(c, this, "addSourceSpecificMembership", args)
}

pub(crate) fn drop_source_membership(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    source_membership(c, this, "dropSourceSpecificMembership", args)
}

fn reference(c: &mut Context<'_>, socket: RootId, method: &str) -> Outcome {
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    if !is_nullish(c, handle) {
        call_method(c, handle, method, &[])?;
    }
    Ok(socket)
}

pub(crate) fn reference_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, this, "ref")
}

pub(crate) fn unreference_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, this, "unref")
}

pub(crate) fn set_recv_buffer_size(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let size = arg(c, args, 0);
    buffer_size(c, this, size, true)?;
    Ok(c.undefined())
}

pub(crate) fn set_send_buffer_size(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let size = arg(c, args, 0);
    buffer_size(c, this, size, false)?;
    Ok(c.undefined())
}

pub(crate) fn get_recv_buffer_size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let zero = c.number(0.0);
    buffer_size(c, this, zero, true)
}

pub(crate) fn get_send_buffer_size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let zero = c.number(0.0);
    buffer_size(c, this, zero, false)
}

fn send_queue(c: &mut Context<'_>, socket: RootId, method: &str) -> Outcome {
    let state = state(c, socket)?;
    let handle = handle(c, state)?;
    call_method(c, handle, method, &[])
}

pub(crate) fn get_send_queue_size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    send_queue(c, this, "getSendQueueSize")
}

pub(crate) fn get_send_queue_count(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    send_queue(c, this, "getSendQueueCount")
}
