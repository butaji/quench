//! `net` module — real non-blocking TCP server and socket driven by
//! the event-loop pump.
//!
//! `std::net` sockets run non-blocking; every pump tick polls the
//! host's server + socket sets and dispatches `listening`,
//! `connection`, `connect`, `data`, `end`, and `close` events on the
//! corresponding JS objects through the standard emitter store. Writes
//! are buffered until the socket drains.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::rc::Rc;
use std::str::FromStr;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::emitter::{emitter_id, Listener};

mod methods;
mod pump;

pub use methods::{
    complete_lookup, connect, connect_existing, connect_path, create_server, server_address,
    server_close, server_close_idle, server_listen, server_ref, server_unref, socket_abort,
    bound_socket_address, bound_socket_close, bound_socket_construct, bound_socket_fd, pipe_bind,
    pipe_construct, socket_address, socket_construct, socket_destroy, socket_end,
    socket_pause, socket_ref,
    socket_reset_and_destroy,
    socket_onread,
    register_fd_stream,
    socket_resume, socket_set_encoding, socket_set_keep_alive, socket_set_no_delay,
    socket_set_type_of_service, socket_get_type_of_service, socket_handle_close,
    socket_set_timeout, socket_timeout_fire, socket_unref, socket_write,
    server_get_connections,
    tcp_bind, tcp_construct,
    server_listen2,
};
pub use pump::{finalize, poll};

const LOCAL_HOST: &str = "127.0.0.1";
/// Hidden property that stores the host-side net id on a JS object.
const NET_ID_PROP: &str = "\0quench:net:id";
pub(crate) const PIPE_FD_PROP: &str = "\0quench:net:pipe-fd";
pub(crate) const PIPE_MARKER_PROP: &str = "\0quench:net:pipe";
pub(crate) const BOUND_ID_PROP: &str = "\0quench:net:bound-id";
pub(crate) const BOUND_HANDLE_PROP: &str = "\0quench:net:bound-handle";
pub(crate) const BOUND_LOCAL_ADDRESS_PROP: &str = "\0quench:net:bound-local-address";
pub(crate) const BOUND_LOCAL_PORT_PROP: &str = "\0quench:net:bound-local-port";
pub(crate) const LOOKUP_ADDRESSES_PROP: &str = "\0quench:net:lookup-addresses";
pub(crate) const ONREAD_BUFFER_PROP: &str = "\0quench:net:onread:buffer";
pub(crate) const ONREAD_CALLBACK_PROP: &str = "\0quench:net:onread:callback";
pub(crate) const ONREAD_PAUSED_PROP: &str = "\0quench:net:onread:paused";
pub(crate) const ONREAD_EOF_PROP: &str = "\0quench:net:onread:eof";
/// Host-only persistence for stream options across reconnect transport swaps.
pub(crate) const SOCKET_ENCODING_PROP: &str = "\0quench:net:encoding";
pub(crate) const TCP_WRAP_BINDING_PROP: &str = "\0quench:net:tcp-wrap-binding";
pub(crate) const NO_DELAY_PROP: &str = "\0quench:net:no-delay";
pub(crate) const TOS_PROP: &str = "\0quench:net:tos";
pub(crate) const HANDLE_CLOSED_PROP: &str = "\0quench:net:handle-closed";
pub(crate) const HANDLE_NO_DELAY_PROP: &str = "\0quench:net:handle-no-delay";
const ASYNC_ITER_TARGET_PROP: &str = "\0quench:net:async-iter-target";
const READ_CHUNK: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SocketState {
    Open,
    Closing,
    Closed,
}

pub struct NetServer {
    pub id: u64,
    pub owner_worker: Option<u64>,
    /// Logical process scope that created the server in the shared host VM.
    pub process_scope: u64,
    /// Cluster construction-order slot for `listen(0)` sharing.
    pub ephemeral_slot: Option<usize>,
    pub listener: Option<TcpListener>,
    pub path: Option<String>,
    pub bind_addr: Option<SocketAddr>,
    pub js: Value,
    pub listening: bool,
    pub refed: bool,
    pub announced: bool,
    pub closed: bool,
    pub close_emitted: bool,
    pub allow_half_open: bool,
    pub pause_on_connect: bool,
}

pub struct NetSocket {
    pub id: u64,
    /// Logical process scope that created or accepted this socket.
    pub process_scope: u64,
    pub stream: Option<TcpStream>,
    pub js: Value,
    pub state: SocketState,
    pub refed: bool,
    pub server_id: Option<u64>,
    pub write_buf: Vec<u8>,
    pub write_offset: usize,
    pub read_buf: Vec<u8>,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_eof: bool,
    pub close_emitted: bool,
    /// Keep EOF/finish observable for one turn before close finalization.
    pub close_deferred: bool,
    /// Delay the local FIN until the next pump turn after `end()` so queued
    /// bytes are observable by the peer before shutdown is observed.
    pub write_shutdown_pending: bool,
    pub finish_emitted: bool,
    pub connect_announced: bool,
    pub peer: Option<SocketAddr>,
    pub local: Option<SocketAddr>,
    pub encoding: Option<String>,
    /// Incomplete bytes retained between reads for streaming text decoding.
    pub decode_buf: Vec<u8>,
}

pub struct NetBoundSocket {
    pub listener: Option<TcpListener>,
    pub path: Option<String>,
    pub address: Option<SocketAddr>,
    pub fd: i64,
    pub adopted: bool,
}

pub struct NetState {
    next: u64,
    pub servers: HashMap<u64, Rc<RefCell<NetServer>>>,
    pub sockets: HashMap<u64, Rc<RefCell<NetSocket>>>,
    pub bound_sockets: HashMap<u64, Rc<RefCell<NetBoundSocket>>>,
    pub pending_errors: Vec<(Value, Value)>,
    pub lookup_result: Option<Value>,
    pub lookup_in_call: bool,
    pub auto_select_family: bool,
    pub auto_select_family_attempt_timeout: u64,
    pub pending_lookups: Vec<PendingLookup>,
    pub pending_events: Vec<(Value, String, Vec<Value>)>,
    pub paths: HashMap<String, u16>,
    pub pending_writes: Vec<(Value, Vec<u8>)>,
    pub pending_connect_writes: HashMap<u64, Vec<u8>>,
    pub pending_request_writes: Vec<(Value, Vec<u8>, Value)>,
    /// Canonical socket timeout timers, independent of VM alias properties.
    pub timeout_timers: HashMap<u64, Value>,
    pub pipe_fds: HashMap<i64, String>,
    /// Host-owned stream endpoints handed to `new net.Socket({ fd })`.
    pub fd_streams: HashMap<i64, Vec<TcpStream>>,
    pub next_pipe_fd: i64,
    pub async_streams: HashMap<u64, NetAsyncStream>,
    pub socket_prototype: Option<Value>,
}

/// Buffered values and pending consumers for one server/socket iterator.
pub struct NetAsyncStream {
    pub queue: Vec<Value>,
    pub waiters: Vec<Rc<quench_runtime::value::PromiseData>>,
    pub ended: bool,
}

pub struct PendingLookup {
    pub socket: Value,
    pub options: Value,
    pub args: Vec<Value>,
}

impl Default for NetState {
    fn default() -> Self {
        Self::new()
    }
}

impl NetState {
    pub fn new() -> Self {
        Self {
            next: 1,
            servers: HashMap::new(),
            sockets: HashMap::new(),
            bound_sockets: HashMap::new(),
            pending_errors: Vec::new(),
            lookup_result: None,
            lookup_in_call: false,
            auto_select_family: false,
            auto_select_family_attempt_timeout: 2500,
            pending_lookups: Vec::new(),
            pending_events: Vec::new(),
            paths: HashMap::new(),
            pending_writes: Vec::new(),
            pending_connect_writes: HashMap::new(),
            pending_request_writes: Vec::new(),
            timeout_timers: HashMap::new(),
            pipe_fds: HashMap::new(),
            fd_streams: HashMap::new(),
            next_pipe_fd: 100,
            async_streams: HashMap::new(),
            socket_prototype: None,
        }
    }
}

/// Does any live net object keep the event loop alive?
pub fn has_work(state: &Rc<RefCell<HostState>>) -> bool {
    let host = state.borrow();
    host.net.servers.values().any(|s| {
        let server = s.borrow();
        server.listening && server.refed && !server.closed
    }) || !host.net.pending_errors.is_empty()
        || !host.net.pending_events.is_empty()
        || host.net.sockets.values().any(|s| {
            let socket = s.borrow();
            let paused = matches!(
                execute::get_property(&socket.js, ONREAD_PAUSED_PROP),
                Value::Boolean(true)
            );
            let custom_handle = matches!(
                execute::get_property(&socket.js, "_handle"),
                Value::Object(_) | Value::ObjectAlias(_)
            );
            socket.refed
                && socket.state != SocketState::Closed
                && (!socket.read_eof
                    || !socket.close_deferred
                    || socket.state == SocketState::Closing)
                && (socket.stream.is_some()
                    || socket.server_id.is_some()
                    || (custom_handle && (socket.read_eof || socket.state == SocketState::Closing)))
                && (!paused || pending_write_len(&socket) > 0)
        })
}

// ---- object builders ----

/// A fresh emitter-backed object with the given extra methods.
fn new_net_object(
    state: &Rc<RefCell<HostState>>,
    methods: Vec<(&'static str, Value)>,
) -> Result<(Value, u64), VmError> {
    let mut object = crate::modules::events::new_emitter_object(state)?;
    let id = allocate_id(state);
    let props: Vec<(String, Value)> = methods
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
    object = install_methods(object, props)?;
    object = install_methods(
        object,
        vec![(NET_ID_PROP.to_string(), Value::Number(id as f64))],
    )?;
    // `internal/async_hooks.symbols` exposes these private keys to Node's
    // HTTP Agent tests; the socket's async identity is the same resource id
    // used by the host lifecycle callbacks.
    object = install_methods(
        object,
        vec![
            (
                "Symbol(async_id_symbol)\0quench".into(),
                Value::Number(id as f64),
            ),
            (
                "Symbol(trigger_async_id_symbol)\0quench".into(),
                Value::Number(crate::modules::async_hooks::current_resource_id(state) as f64),
            ),
        ],
    )?;
    state.borrow_mut().net.async_streams.insert(
        id,
        NetAsyncStream {
            queue: Vec::new(),
            waiters: Vec::new(),
            ended: false,
        },
    );
    Ok((object, id))
}

fn install_methods(mut object: Value, props: Vec<(String, Value)>) -> Result<Value, VmError> {
    for (key, value) in props {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        object = execute::define_property(object, &key, descriptor)?;
    }
    Ok(object)
}

pub(crate) fn install_socket_counters(object: Value) -> Result<Value, VmError> {
    install_methods(
        object,
        vec![
            ("bytesRead".to_string(), Value::Number(0.0)),
            ("bytesWritten".to_string(), Value::Number(0.0)),
            ("bufferSize".to_string(), Value::Number(0.0)),
            ("writableLength".to_string(), Value::Number(0.0)),
            ("writableHighWaterMark".to_string(), Value::Number(16_384.0)),
            ("pending".to_string(), Value::Boolean(true)),
            ("connecting".to_string(), Value::Boolean(false)),
            ("_connecting".to_string(), Value::Boolean(false)),
            (
                "readyState".to_string(),
                Value::String("closed".to_string()),
            ),
        ],
    )
}

pub(crate) fn set_socket_state(socket: &Value, pending: bool, connecting: bool, ready_state: &str) {
    let mut properties = vec![
        ("connecting", Value::Boolean(connecting)),
        ("_connecting", Value::Boolean(connecting)),
        ("readyState", Value::String(ready_state.to_string())),
    ];
    if ready_state == "closed" {
        properties.push(("destroyed", Value::Boolean(true)));
    }
    // `pending=false` may need the runtime replacement path for a VM alias.
    // Apply it last so the replacement snapshots the complete connection
    // state instead of leaving `connecting` on the stale pre-replacement
    // object.
    properties.push(("pending", Value::Boolean(pending)));
    for (key, value) in properties {
        // Host-owned socket records are shared with requests, responses, and
        // Agent pools. Mutate their existing object identity; replacing the
        // value through ordinary copy-on-write semantics would make
        // `res.socket === req.socket` and pooled-socket reuse diverge.
        set_socket_property(socket, key, value);
    }
}

/// Mutate a host socket through either a direct object or a VM alias. Aliases
/// need the runtime replacement path; direct objects can use the allocation-
/// free host mutation path.
pub(crate) fn set_socket_property(socket: &Value, key: &str, value: Value) {
    if matches!(socket, Value::ObjectAlias(_))
        && matches!(key, "pending" | "readable")
        && matches!(value, Value::Boolean(false))
    {
        let updated = execute::set_property(socket.clone(), key, value);
        execute::replace_value(socket, &updated);
    } else {
        execute::set_property_in_place(socket, key, value);
    }
}

pub(crate) fn replace_socket_property(socket: &Value, key: &str, value: Value) {
    let updated = execute::set_property(socket.clone(), key, value);
    execute::replace_value(socket, &updated);
}

pub(crate) fn update_socket_counters(socket: &NetSocket) {
    set_socket_bytes_read(&socket.js, socket.bytes_read);
    execute::set_property_in_place(
        &socket.js,
        "bytesWritten",
        Value::Number(socket.bytes_written as f64),
    );
}

pub(crate) fn set_socket_bytes_read(socket: &Value, bytes: u64) {
    let value = Value::Number(bytes as f64);
    execute::set_property_in_place(socket, "bytesRead", value.clone());
    let handle = execute::get_property(socket, "_handle");
    if matches!(handle, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(&handle, "bytesRead", value);
    }
}

fn allocate_id(state: &Rc<RefCell<HostState>>) -> u64 {
    let id = state.borrow().net.next;
    state.borrow_mut().net.next += 1;
    id
}

/// Socket address info stamped on a socket object.
fn net_info_props(peer: SocketAddr, local: Option<SocketAddr>) -> Vec<(String, Value)> {
    let mut props = vec![
        (
            "remoteAddress".to_string(),
            Value::String(peer.ip().to_string()),
        ),
        ("remotePort".to_string(), Value::Number(peer.port() as f64)),
        ("remoteFamily".to_string(), Value::String(family(peer))),
    ];
    if let Some(local) = local {
        props.push((
            "localAddress".to_string(),
            Value::String(local.ip().to_string()),
        ));
        props.push(("localPort".to_string(), Value::Number(local.port() as f64)));
        props.push(("localFamily".to_string(), Value::String(family(local))));
    }
    props
}

fn server_props() -> Vec<(&'static str, Value)> {
    vec![
        // Node's net.Server keeps the worker socket lists on every server,
        // including a plain server with no cluster workers.  Keep the
        // observable collection present so consumers can inspect its length
        // without manufacturing a second server representation.
        ("_workers", host_api::array(Vec::new())),
        ("listen", cap(crate::registry::SPEC_NET_SERVER_LISTEN)),
        ("_listen2", cap(crate::registry::SPEC_NET_SERVER_LISTEN2)),
        ("close", cap(crate::registry::SPEC_NET_SERVER_CLOSE)),
        (
            "closeIdleConnections",
            cap(crate::registry::SPEC_NET_SERVER_CLOSE_IDLE),
        ),
        ("address", cap(crate::registry::SPEC_NET_SERVER_ADDRESS)),
        (
            "getConnections",
            cap(crate::registry::SPEC_NET_SERVER_GET_CONNECTIONS),
        ),
        ("unref", cap(crate::registry::SPEC_NET_SERVER_UNREF)),
        ("ref", cap(crate::registry::SPEC_NET_SERVER_REF)),
        ("Symbol.asyncIterator", cap(crate::registry::SPEC_NET_ASYNC_ITERATOR)),
    ]
}

pub(crate) fn set_server_listening(server: &Value, listening: bool) -> Result<(), VmError> {
    let descriptor = host_api::object(vec![
        ("value".to_string(), Value::Boolean(listening)),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]);
    let _ = execute::define_property(server.clone(), "listening", descriptor)?;
    Ok(())
}

pub(crate) fn set_server_connection_key(
    server: &Value,
    port: u16,
    host: Option<&str>,
) -> Result<(), VmError> {
    let address = host.unwrap_or("0.0.0.0");
    let key = format!("4:{address}:{port}");
    let descriptor = host_api::object(vec![
        ("value".to_string(), Value::String(key)),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]);
    let _ = execute::define_property(server.clone(), "_connectionKey", descriptor)?;
    Ok(())
}

fn socket_props() -> Vec<(&'static str, Value)> {
    vec![
        (
            "_readableState",
            host_api::object(vec![
                ("pipeCount".into(), Value::Number(0.0)),
                ("awaitDrainWriters".into(), Value::Null),
                ("pipes".into(), host_api::array(Vec::new())),
            ]),
        ),
        // Node exposes a nulled-out native handle after a socket closes.
        ("_handle", Value::Null),
        // HTTP Agent exposes parser=null while a keep-alive socket is idle.
        ("parser", Value::Null),
        ("readable", Value::Boolean(true)),
        ("writable", Value::Boolean(true)),
        ("writableCorked", Value::Number(0.0)),
        (
            "_writableState",
            host_api::object(vec![("highWaterMark".into(), Value::Number(16_384.0))]),
        ),
        ("allowHalfOpen", Value::Boolean(false)),
        ("destroyed", Value::Boolean(false)),
        ("connecting", Value::Boolean(false)),
        ("_connecting", Value::Boolean(false)),
        ("readyState", Value::String("open".into())),
        ("connect", cap(crate::registry::SPEC_NET_CONNECT)),
        ("write", cap(crate::registry::SPEC_NET_SOCKET_WRITE)),
        ("end", cap(crate::registry::SPEC_NET_SOCKET_END)),
        ("destroy", cap(crate::registry::SPEC_NET_SOCKET_DESTROY)),
        (
            "resetAndDestroy",
            cap(crate::registry::SPEC_NET_SOCKET_RESET_AND_DESTROY),
        ),
        ("unref", cap(crate::registry::SPEC_NET_SOCKET_UNREF)),
        ("ref", cap(crate::registry::SPEC_NET_SOCKET_REF)),
        ("address", cap(crate::registry::SPEC_NET_SOCKET_ADDRESS)),
        (
            "setNoDelay",
            cap(crate::registry::SPEC_NET_SOCKET_SET_NO_DELAY),
        ),
        (
            "setTypeOfService",
            cap(crate::registry::SPEC_NET_SOCKET_SET_TOS),
        ),
        (
            "getTypeOfService",
            cap(crate::registry::SPEC_NET_SOCKET_GET_TOS),
        ),
        (
            "setKeepAlive",
            cap(crate::registry::SPEC_NET_SOCKET_SET_KEEP_ALIVE),
        ),
        (
            "setTimeout",
            cap(crate::registry::SPEC_NET_SOCKET_SET_TIMEOUT),
        ),
        (
            "setEncoding",
            cap(crate::registry::SPEC_NET_SOCKET_SET_ENCODING),
        ),
        ("pause", cap(crate::registry::SPEC_NET_SOCKET_PAUSE)),
        ("resume", cap(crate::registry::SPEC_NET_SOCKET_RESUME)),
        // The host write queue already provides cork semantics; expose the
        // pair as stable no-op calls so callers can bracket vectorized writes.
        ("cork", Value::Builtin(quench_runtime::ops::Builtin::Object)),
        (
            "uncork",
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        ("Symbol.asyncIterator", cap(crate::registry::SPEC_NET_ASYNC_ITERATOR)),
    ]
}

fn cap(spec: crate::registry::NodeSpec) -> Value {
    crate::host::capability(spec)
}

fn register_server(
    state: &Rc<RefCell<HostState>>,
    js: &Value,
    listener: Option<TcpListener>,
) -> Result<u64, VmError> {
    register_server_path(state, js, listener, None)
}

fn register_server_path(
    state: &Rc<RefCell<HostState>>,
    js: &Value,
    listener: Option<TcpListener>,
    path: Option<String>,
) -> Result<u64, VmError> {
    let id = net_id(js).ok_or_else(|| execute::type_error("not a net object"))?;
    // A bound listener means the server is listening (createServer
    // registers without one; listen() registers with one).
    let is_listening = listener.is_some();
    let bind_addr = listener.as_ref().and_then(|l| l.local_addr().ok());
    let (owner_worker, refed) = {
        let host = state.borrow();
        let refed = host
            .net
            .servers
            .get(&id)
            .map(|server| server.borrow().refed)
            .unwrap_or(true);
        let worker = host.cluster.worker_context.or_else(|| {
            match execute::get_property(
                &quench_runtime::vm::current_global_object(),
                "__quench_cluster_worker_id",
            ) {
                Value::Number(id) if id.is_finite() && id >= 0.0 => Some(id as u64),
                _ => None,
            }
        });
        (worker, refed)
    };
    let process_scope = state.borrow().cluster.process_scope();
    state.borrow_mut().net.servers.insert(
        id,
        Rc::new(RefCell::new(NetServer {
            id,
            owner_worker,
            process_scope,
            ephemeral_slot: None,
            listener,
            path,
            bind_addr,
            js: js.clone(),
            listening: is_listening,
            refed,
            announced: false,
            closed: false,
            close_emitted: false,
            allow_half_open: matches!(
                execute::get_property(js, "allowHalfOpen"),
                Value::Boolean(true)
            ),
            pause_on_connect: matches!(
                execute::get_property(js, "pauseOnConnect"),
                Value::Boolean(true)
            ),
        })),
    );
    set_server_listening(js, is_listening)?;
    Ok(id)
}

pub(crate) fn net_id(receiver: &Value) -> Option<u64> {
    // Event delivery may expose a canonical object through an alias value;
    // resolve the hidden identity slot through ordinary property semantics so
    // socket methods preserve the same host resource in either representation.
    match execute::get_property(receiver, NET_ID_PROP) {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as u64),
        _ => None,
    }
}

/// Move a transferred net handle into the logical process that received it.
pub(crate) fn transfer_handle_scope(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    scope: u64,
) {
    let Some(id) = net_id(receiver) else {
        return;
    };
    let host = state.borrow();
    if let Some(server) = host.net.servers.get(&id) {
        server.borrow_mut().process_scope = scope;
    }
    if let Some(socket) = host.net.sockets.get(&id) {
        socket.borrow_mut().process_scope = scope;
    }
}

/// Return the Rust-backed async iterator for a server or socket.
pub fn async_iterator(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
    if async_target(&receiver).is_some() {
        return Ok(receiver);
    }
    let id = net_id(&receiver).ok_or_else(|| execute::type_error("not a net object"))?;
    let known = {
        let host = state.borrow();
        host.net.servers.contains_key(&id) || host.net.sockets.contains_key(&id)
    };
    if !known {
        return Err(execute::type_error("not a live net object"));
    }
    let iterator = host_api::object(vec![
        (
            "next".to_string(),
            cap(crate::registry::SPEC_NET_ASYNC_ITERATOR_NEXT),
        ),
        (
            "return".to_string(),
            cap(crate::registry::SPEC_NET_ASYNC_ITERATOR_RETURN),
        ),
        (
            "Symbol.asyncIterator".to_string(),
            cap(crate::registry::SPEC_NET_ASYNC_ITERATOR),
        ),
    ]);
    install_methods(
        iterator,
        vec![(
            ASYNC_ITER_TARGET_PROP.to_string(),
            Value::Number(id as f64),
        )],
    )
}

/// Resolve one queued stream value, or suspend until the host produces one.
pub fn async_iterator_next(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let id = async_target(receiver).ok_or_else(|| execute::type_error("not a net iterator"))?;
    let promise = quench_runtime::value::PromiseData::allocate(
        quench_runtime::value::PromiseState::Pending,
    );
    let outcome = {
        let mut host = state.borrow_mut();
        let stream = host
            .net
            .async_streams
            .get_mut(&id)
            .ok_or_else(|| execute::type_error("net iterator is closed"))?;
        if !stream.queue.is_empty() {
            let value = stream.queue.remove(0);
            Some(iterator_result(value, false))
        } else if stream.ended {
            Some(iterator_result(Value::Undefined, true))
        } else {
            stream.waiters.push(Rc::clone(&promise));
            None
        }
    };
    if let Some(value) = outcome {
        quench_runtime::resolve_promise(&promise, value);
    }
    Ok(Value::Promise(promise))
}

/// Finish an iterator without changing the underlying server/socket.
pub fn async_iterator_return(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let promise = quench_runtime::value::PromiseData::allocate(
        quench_runtime::value::PromiseState::Pending,
    );
    quench_runtime::resolve_promise(&promise, iterator_result(Value::Undefined, true));
    Ok(Value::Promise(promise))
}

fn async_target(value: &Value) -> Option<u64> {
    match execute::get_property(value, ASYNC_ITER_TARGET_PROP) {
        Value::Number(id) if id.is_finite() && id >= 0.0 => Some(id as u64),
        _ => None,
    }
}

fn iterator_result(value: Value, done: bool) -> Value {
    host_api::object(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(done)),
    ])
}

/// Queue a value for a server/socket, retaining it when iteration starts later.
pub(crate) fn queue_async_value(state: &Rc<RefCell<HostState>>, id: u64, value: Value) {
    let waiter = {
        let mut host = state.borrow_mut();
        let stream = host.net.async_streams.entry(id).or_insert(NetAsyncStream {
            queue: Vec::new(),
            waiters: Vec::new(),
            ended: false,
        });
        if stream.ended {
            None
        } else if let Some(waiter) = stream.waiters.pop() {
            Some(waiter)
        } else {
            stream.queue.push(value.clone());
            None
        }
    };
    if let Some(waiter) = waiter {
        quench_runtime::resolve_promise(&waiter, iterator_result(value, false));
    }
}

/// Mark a stream ended and wake every pending `next()` call.
pub(crate) fn end_async_stream(state: &Rc<RefCell<HostState>>, id: u64) {
    let waiters = {
        let mut host = state.borrow_mut();
        let stream = host.net.async_streams.entry(id).or_insert(NetAsyncStream {
            queue: Vec::new(),
            waiters: Vec::new(),
            ended: false,
        });
        stream.ended = true;
        std::mem::take(&mut stream.waiters)
    };
    for waiter in waiters {
        quench_runtime::resolve_promise(&waiter, iterator_result(Value::Undefined, true));
    }
}

// ---- shared value helpers ----

fn address_value(addr: SocketAddr) -> Value {
    host_api::object(vec![
        ("address".to_string(), Value::String(addr.ip().to_string())),
        ("family".to_string(), Value::String(family(addr))),
        ("port".to_string(), Value::Number(addr.port() as f64)),
    ])
}

fn family(addr: SocketAddr) -> String {
    if addr.is_ipv4() {
        "IPv4".to_string()
    } else {
        "IPv6".to_string()
    }
}

pub(crate) fn peer_write_error() -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("write EPIPE".into())],
    );
    let error = execute::set_property(error, "code", Value::String("EPIPE".into()));
    execute::set_property(error, "syscall", Value::String("write".into()))
}

pub(crate) fn handle_write_error(code: &str, message: &str) -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    let error = execute::set_property(error, "code", Value::String(code.into()));
    execute::set_property(error, "syscall", Value::String("write".into()))
}

fn resolve(host: &str, port: u16) -> SocketAddr {
    let host = if host == "localhost" {
        LOCAL_HOST
    } else {
        host
    };
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return SocketAddr::new(ip, port);
    }
    use std::net::ToSocketAddrs;
    (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .unwrap_or(SocketAddr::new(LOCAL_HOST.parse().expect("loopback"), port))
}

pub(crate) fn resolve_connect(host: &str, port: u16) -> Option<SocketAddr> {
    let host = if host == "localhost" {
        LOCAL_HOST
    } else {
        host
    };
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Some(SocketAddr::new(ip, port));
    }
    use std::net::ToSocketAddrs;
    (host, port).to_socket_addrs().ok()?.next()
}

/// Best-effort flush of buffered writes back to the peer. Returns
/// whether the buffer drained fully.
fn try_flush(guard: &mut std::cell::RefMut<'_, NetSocket>) -> bool {
    if guard.write_offset >= guard.write_buf.len() {
        return true;
    }
    let mut stream = guard.stream.take();
    let written = stream
        .as_mut()
        .map(|stream| {
            stream
                .write(&guard.write_buf[guard.write_offset..])
                .unwrap_or(0)
        })
        .unwrap_or(0);
    guard.stream = stream;
    if written > 0 {
        guard.write_offset = guard.write_offset.saturating_add(written);
        if guard.write_offset == guard.write_buf.len() {
            guard.write_buf.clear();
            guard.write_offset = 0;
        } else if guard.write_offset >= 64 * 1024 && guard.write_offset * 2 >= guard.write_buf.len()
        {
            let offset = guard.write_offset;
            guard.write_buf.drain(..offset);
            guard.write_offset = 0;
        }
    }
    guard.write_offset >= guard.write_buf.len()
}

#[inline]
pub(crate) fn pending_write_len(socket: &NetSocket) -> usize {
    socket.write_buf.len().saturating_sub(socket.write_offset)
}

/// Dispatch one emitter event on a JS object from the host side,
/// mirroring `events`' emit (once-listeners removed before their call).
pub(crate) fn emit(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    event: &str,
    args: Vec<Value>,
) -> Result<(), VmError> {
    let global = quench_runtime::vm::current_global_object();
    let previous_resource = execute::get_property(&global, "__nodeCurrentAsyncResource");
    let resource = [
        crate::modules::http_client::CLIENT_ASYNC_RESOURCE_PROP,
        crate::modules::http_client::RES_ASYNC_RESOURCE_PROP,
        crate::modules::http::REQ_ASYNC_RESOURCE_PROP,
    ]
    .into_iter()
    .map(|key| execute::get_property(receiver, key))
    .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    if let Some(resource) = resource.as_ref() {
        crate::modules::async_hooks::resource_before(state, Some(resource), &[])?;
    }
    if let Some(resource) = resource.as_ref() {
        execute::set_property_in_place(&global, "__nodeCurrentAsyncResource", resource.clone());
    }
    if event == "close"
        && matches!(
            execute::get_property(receiver, crate::modules::http::INCOMING_CLOSE_PENDING_PROP),
            Value::Boolean(true)
        )
    {
        execute::set_property_in_place(receiver, "closed", Value::Boolean(true));
        execute::set_property_in_place(
            receiver,
            crate::modules::http::INCOMING_CLOSE_PENDING_PROP,
            Value::Boolean(false),
        );
    }
    let id = emitter_id(receiver).or_else(|| state.borrow().emitters.identity(receiver));
    let process_scope = state.borrow().cluster.process_scope();
    let listeners: Vec<Listener> = id
        .and_then(|id| state.borrow().emitters.get(id))
        .map(|emitter| {
            let listeners = emitter.borrow().listeners_for_scope(event, process_scope);
            listeners
        })
        .unwrap_or_default();
    let result = if listeners.is_empty() {
        if event == "error" {
            Err(unhandled_error(args.first()))
        } else {
            Ok(())
        }
    } else {
        let mut result = Ok(());
        for listener in &listeners {
            if listener.once {
                if let Some(id) = id {
                    if let Some(emitter) = state.borrow().emitters.get(id) {
                        emitter
                            .borrow_mut()
                            .remove_for_scope(event, &listener.callback, process_scope);
                    }
                }
            }
            if let Err(error) = execute::call(&listener.callback, receiver, &args) {
                result = Err(error);
                break;
            }
        }
        result
    };
    if resource.is_some() {
        crate::modules::async_hooks::resource_after(state, None, &[])?;
    }
    execute::set_property_in_place(&global, "__nodeCurrentAsyncResource", previous_resource);
    result
}

fn unhandled_error(arg: Option<&Value>) -> VmError {
    match arg {
        Some(value) if !matches!(value, Value::Undefined) => VmError::Thrown(value.clone()),
        _ => VmError::Thrown(host_api::object(vec![
            ("name".to_string(), Value::String("Error".to_string())),
            (
                "message".to_string(),
                Value::String("Unhandled error.".to_string()),
            ),
        ])),
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Object(_) | Value::ObjectAlias(_) => {
            let method = execute::get_property(value, "toString");
            if quench_runtime::is_callable(&method) {
                if let Ok(Value::String(result)) = execute::call(&method, value, &[]) {
                    return result;
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

// ---- pump-parallel value shaping ----

/// The `'data'` payload: a string when an encoding is set, else a Buffer.
fn data_value(guard: &mut NetSocket, bytes: &[u8]) -> Value {
    match guard.encoding.as_deref() {
        Some("utf8") | Some("utf-8") => {
            guard.decode_buf.extend_from_slice(bytes);
            let complete = match std::str::from_utf8(&guard.decode_buf) {
                Ok(_) => guard.decode_buf.len(),
                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                Err(_) => guard.decode_buf.len(),
            };
            let trailing = guard.decode_buf.split_off(complete);
            let decoded = String::from_utf8_lossy(&guard.decode_buf).into_owned();
            guard.decode_buf = trailing;
            Value::String(decoded)
        }
        Some("hex") => Value::String(hex::encode(bytes)),
        Some("latin1") | Some("ascii") => Value::String(bytes.iter().map(|&b| b as char).collect()),
        _ => crate::modules::buffer_proto::make_buffer(bytes),
    }
}

// ---- module surface ----

pub fn is_ip(args: &[Value]) -> i32 {
    let s = args.first().map(value_to_string).unwrap_or_default();
    if std::net::Ipv4Addr::from_str(&s).is_ok() {
        return 4;
    }
    if parse_ipv6(&s).is_some() {
        return 6;
    }
    0
}

pub fn is_ipv4(args: &[Value]) -> bool {
    let s = args.first().map(value_to_string).unwrap_or_default();
    s.parse::<std::net::Ipv4Addr>().is_ok()
}

pub fn is_ipv6(args: &[Value]) -> bool {
    let s = args.first().map(value_to_string).unwrap_or_default();
    parse_ipv6(&s).is_some()
}

fn parse_ipv6(value: &str) -> Option<std::net::Ipv6Addr> {
    let (address, zone) = value
        .split_once('%')
        .map_or((value, None), |(address, zone)| (address, Some(zone)));
    if zone.is_some_and(|zone| {
        zone.is_empty()
            || !zone
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
    }) {
        return None;
    }
    address.parse().ok()
}

pub fn build() -> Value {
    build_with_state(None)
}

pub fn build_with_state(state: Option<&Rc<RefCell<HostState>>>) -> Value {
    let socket_prototype = host_api::object(Vec::new());
    let global = quench_runtime::vm::current_global_object();
    let require = execute::get_property(&global, "require");
    if quench_runtime::is_callable(&require) {
        if let Ok(stream) = execute::call(&require, &Value::Undefined, &[Value::String("stream".into())]) {
            let duplex = execute::get_property(&stream, "Duplex");
            let prototype = execute::get_property(&duplex, "prototype");
            let pipe = execute::get_property(&prototype, "pipe");
            if quench_runtime::is_callable(&pipe) {
                execute::set_property_in_place(&socket_prototype, "pipe", pipe);
            }
        }
    }
    if let Some(state) = state {
        state.borrow_mut().net.socket_prototype = Some(socket_prototype.clone());
    }
    execute::set_property_in_place(
        &global,
        "\0quench:net:socket-prototype",
        socket_prototype.clone(),
    );
    let socket_ctor = execute::set_property(
        crate::host::capability(crate::registry::SPEC_NET_SOCKET),
        "prototype",
        socket_prototype,
    );
    let server_ctor = execute::set_property(
        crate::host::capability(crate::registry::SPEC_NET_SERVER),
        "prototype",
        host_api::object(Vec::new()),
    );
    let bound_socket_ctor = execute::set_property(
        crate::host::capability(crate::registry::SPEC_NET_BOUND_SOCKET),
        "prototype",
        host_api::object(vec![("isPipe".into(), Value::Boolean(false))]),
    );
    crate::host::namespace_object(vec![
        (
            "connect",
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        ),
        (
            "createConnection",
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        ),
        (
            "createServer",
            crate::host::capability(crate::registry::SPEC_NET_SERVER),
        ),
        ("Socket", socket_ctor.clone()),
        ("Stream", socket_ctor),
        ("Server", server_ctor),
        ("BoundSocket", bound_socket_ctor),
        (
            "BlockList",
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST),
        ),
        (
            "isIP",
            crate::host::capability(crate::registry::SPEC_NET_ISIP),
        ),
        (
            "isIPv4",
            crate::host::capability(crate::registry::SPEC_NET_ISIPV4),
        ),
        (
            "isIPv6",
            crate::host::capability(crate::registry::SPEC_NET_ISIPV6),
        ),
        (
            "getDefaultAutoSelectFamilyAttemptTimeout",
            crate::host::capability(crate::registry::SPEC_NET_GET_ASF_TIMEOUT),
        ),
        (
            "setDefaultAutoSelectFamilyAttemptTimeout",
            crate::host::capability(crate::registry::SPEC_NET_SET_ASF_TIMEOUT),
        ),
        (
            "getDefaultAutoSelectFamily",
            crate::host::capability(crate::registry::SPEC_NET_GET_ASF),
        ),
        (
            "setDefaultAutoSelectFamily",
            crate::host::capability(crate::registry::SPEC_NET_SET_ASF),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}

pub fn block_list_construct(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::object(vec![
        (
            "\0quench:blocklist:addresses".into(),
            host_api::array(Vec::new()),
        ),
        (
            "addSubnet".into(),
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_SUBNET),
        ),
        (
            "addAddress".into(),
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_ADDRESS),
        ),
        (
            "check".into(),
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_CHECK),
        ),
    ]))
}

pub fn block_list_add_address(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let list = execute::get_property(receiver, "\0quench:blocklist:addresses");
    let mut values = match list {
        Value::Array(array) => (0..array.logical_len())
            .map(|i| array.index_value(i))
            .collect(),
        _ => Vec::new(),
    };
    values.push(Value::String(
        args.first().map(value_to_string).unwrap_or_default(),
    ));
    execute::set_property_in_place(
        receiver,
        "\0quench:blocklist:addresses",
        host_api::array(values),
    );
    Ok(Value::Undefined)
}

pub fn block_list_check(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let address = args.first().map(value_to_string).unwrap_or_default();
    let blocked = receiver
        .and_then(
            |value| match execute::get_property(value, "\0quench:blocklist:addresses") {
                Value::Array(array) => Some(
                    (0..array.logical_len())
                        .any(|i| value_to_string(&array.index_value(i)) == address),
                ),
                _ => None,
            },
        )
        .unwrap_or(false);
    Ok(Value::Boolean(blocked))
}

pub fn block_list_add_subnet(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(prefix) = args.get(1) else {
        return Err(execute::type_error("prefix must be a number"));
    };
    if !matches!(prefix, Value::Number(value) if value.is_finite() && *value >= 0.0) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
        ])));
    }
    Ok(Value::Undefined)
}
