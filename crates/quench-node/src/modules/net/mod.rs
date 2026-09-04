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
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
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
    bound_socket_address, bound_socket_close, bound_socket_construct, bound_socket_fd,
    complete_lookup, connect, connect_existing, connect_path, create_server, pipe_bind,
    pipe_construct, register_fd_stream, server_address, server_close, server_close_idle,
    server_get_connections, server_listen, server_listen2, server_ref, server_unref, socket_abort,
    socket_address, socket_construct, socket_destroy, socket_end, socket_get_type_of_service,
    socket_handle_close, socket_onread, socket_pause, socket_ref, socket_reset_and_destroy,
    socket_resume, socket_set_encoding, socket_set_keep_alive, socket_set_no_delay,
    socket_set_timeout, socket_set_type_of_service, socket_timeout_fire, socket_unref,
    socket_write, tcp_bind, tcp_construct,
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
    /// Bootstrap PerformanceObserver bridge captured while a VM context is
    /// active; the event-loop pump may run outside that context.
    pub performance_record: Option<Value>,
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
            performance_record: None,
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
        (
            "Symbol.asyncIterator",
            cap(crate::registry::SPEC_NET_ASYNC_ITERATOR),
        ),
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
        (
            "Symbol.asyncIterator",
            cap(crate::registry::SPEC_NET_ASYNC_ITERATOR),
        ),
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
pub(crate) fn transfer_handle_scope(state: &Rc<RefCell<HostState>>, receiver: &Value, scope: u64) {
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
        vec![(ASYNC_ITER_TARGET_PROP.to_string(), Value::Number(id as f64))],
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
    let promise =
        quench_runtime::value::PromiseData::allocate(quench_runtime::value::PromiseState::Pending);
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
    let promise =
        quench_runtime::value::PromiseData::allocate(quench_runtime::value::PromiseState::Pending);
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
    let resource_keys = [
        crate::modules::http_client::CLIENT_ASYNC_RESOURCE_PROP,
        crate::modules::http_client::RES_ASYNC_RESOURCE_PROP,
        crate::modules::http::REQ_ASYNC_RESOURCE_PROP,
    ];
    let resource = resource_keys
        .iter()
        .map(|key| execute::get_property(receiver, key))
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .or_else(|| {
            args.iter().find_map(|value| {
                resource_keys
                    .iter()
                    .map(|key| execute::get_property(value, key))
                    .find(|resource| matches!(resource, Value::Object(_) | Value::ObjectAlias(_)))
            })
        });
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
    let mut args = args;
    if let Some(id) = net_id(receiver) {
        if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
            let source = socket.borrow().js.clone();
            for key in [
                "getProtocol",
                "getCipher",
                "getPeerCertificate",
                "getSession",
            ] {
                let value = execute::get_property(&source, key);
                if quench_runtime::is_callable(&value) {
                    execute::set_property_in_place(receiver, key, value);
                }
            }
        }
    }
    for value in &mut args {
        if let Some(id) = net_id(value) {
            if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
                let alpn = execute::get_property(
                    &socket.borrow().js,
                    crate::modules::tls::TLS_NEGOTIATED_ALPN_PROP,
                );
                if !matches!(alpn, Value::Undefined) {
                    execute::set_property_in_place(value, "alpnProtocol", alpn);
                }
            }
        }
    }
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
                        emitter.borrow_mut().remove_for_scope(
                            event,
                            &listener.callback,
                            process_scope,
                        );
                    }
                }
            }
            if let Err(error) = execute::call(&listener.callback, receiver, &args) {
                match error {
                    VmError::Thrown(reason) => {
                        match crate::modules::events::route_domain_error(
                            state,
                            receiver,
                            Some(&reason),
                        )? {
                            Some(_) => continue,
                            None => {
                                result = Err(VmError::Thrown(reason));
                                break;
                            }
                        }
                    }
                    other => {
                        result = Err(other);
                        break;
                    }
                }
            }
        }
        result
    };
    if resource.is_some() {
        crate::modules::async_hooks::resource_after(state, None, &[])?;
    }
    execute::set_property_in_place(&global, "__nodeCurrentAsyncResource", previous_resource.clone());
    if matches!(previous_resource, Value::Undefined) {
        let descriptor = host_api::object(vec![
            ("value".into(), Value::Undefined),
            ("writable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(false)),
        ]);
        let _ = execute::define_property(global.clone(), "__nodeCurrentAsyncResource", descriptor);
    }
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

/// Node's internal/net predicate accepts host spellings used by connection
/// options, including bracketed IPv6 literals and the localhost alias.
pub fn is_loopback(args: &[Value]) -> bool {
    let value = args.first().map(value_to_string).unwrap_or_default();
    if value == "localhost" {
        return true;
    }
    if let Ok(address) = value.parse::<std::net::Ipv4Addr>() {
        return address.octets()[0] == 127;
    }
    let literal = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&value);
    literal
        .parse::<std::net::Ipv6Addr>()
        .is_ok_and(|address| address.is_loopback())
}

pub fn internal_module() -> Value {
    let global = quench_runtime::vm::current_global_object();
    let normalized = execute::get_property(&global, "__quenchNetNormalizedArgsSymbol");
    let normalized = if matches!(normalized, Value::Undefined) {
        Value::String("Symbol(normalizedArgs)\0quench".into())
    } else {
        normalized
    };
    crate::host::namespace_object_from_pairs(vec![
        (
            "isLoopback".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_NET_IS_LOOPBACK),
        ),
        ("normalizedArgsSymbol".into(), normalized),
    ])
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
    let mut socket_prototype = host_api::object(Vec::new());
    let global = quench_runtime::vm::current_global_object();
    let require = execute::get_property(&global, "require");
    if quench_runtime::is_callable(&require) {
        if let Ok(stream) = execute::call(
            &require,
            &Value::Undefined,
            &[Value::String("stream".into())],
        ) {
            let duplex = execute::get_property(&stream, "Duplex");
            let prototype = execute::get_property(&duplex, "prototype");
            for name in ["pipe", "unpipe"] {
                let method = execute::get_property(&prototype, name);
                if quench_runtime::is_callable(&method) {
                    execute::set_property_in_place(&socket_prototype, name, method);
                }
            }
        }
    }
    socket_prototype = execute::define_property(
        socket_prototype,
        "alpnProtocol",
        host_api::object(vec![(
            "get".into(),
            crate::host::capability(crate::registry::SPEC_TLS_SOCKET_GET_ALPN),
        )]),
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    for (name, method) in [
        (
            "getProtocol",
            crate::host::capability(crate::registry::SPEC_TLS_SOCKET_GET_PROTOCOL),
        ),
        (
            "getCipher",
            crate::host::capability(crate::registry::SPEC_TLS_SOCKET_GET_CIPHER),
        ),
    ] {
        execute::set_property_in_place(&socket_prototype, name, method);
    }
    if let Some(state) = state {
        let mut host = state.borrow_mut();
        host.net.socket_prototype = Some(socket_prototype.clone());
        let record = execute::get_property(&global, "__nodePerformanceRecord");
        if quench_runtime::is_callable(&record) {
            host.net.performance_record = Some(record);
        }
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
    let block_list = crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST);
    let _ = execute::set_property(
        block_list.clone(),
        "isBlockList",
        crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_IS),
    );
    let private_ranges = host_api::array(
        [
            "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8",
            "169.254.0.0/16", "::1/128", "fe80::/10", "fc00::/7",
        ]
        .into_iter()
        .map(|value| Value::String(value.into()))
        .collect(),
    );
    let private_ranges = {
        let object = execute::get_property(&global, "Object");
        let freeze = execute::get_property(&object, "freeze");
        execute::call(&freeze, &Value::Undefined, &[private_ranges.clone()])
            .unwrap_or(private_ranges)
    };
    let _ = execute::set_property(
        block_list.clone(),
        "PRIVATE_RANGES",
        private_ranges,
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
            block_list,
        ),
        (
            "SocketAddress",
            crate::host::capability(crate::registry::SPEC_NET_SOCKET_ADDRESS_CONSTRUCT),
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
    let object = host_api::object(vec![
        (
            "\0quench:blocklist:addresses".into(),
            host_api::array(Vec::new()),
        ),
        ("\0quench:blocklist:ranges".into(), host_api::array(Vec::new())),
        ("\0quench:blocklist:subnets".into(), host_api::array(Vec::new())),
        (
            "addSubnet".into(),
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_SUBNET),
        ),
        (
            "addAddress".into(),
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_ADDRESS),
        ),
        (
            "addRange".into(),
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_RANGE),
        ),
        (
            "check".into(),
            crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_CHECK),
        ),
        ("clear".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_CLEAR)),
        ("addAddresses".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_ADDRESSES)),
        ("addCIDR".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_CIDR)),
        ("addCIDRs".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_ADD_CIDRS)),
        ("removeAddress".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_REMOVE_ADDRESS)),
        ("removeRange".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_REMOVE_RANGE)),
        ("removeSubnet".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_REMOVE_SUBNET)),
        ("removeCIDR".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_REMOVE_CIDR)),
        ("toJSON".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_TO_JSON)),
        ("fromJSON".into(), crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_FROM_JSON)),
        ("\0quench:blocklist:marker".into(), Value::Boolean(true)),
        ("\0original_constructor_name".into(), Value::String("BlockList".into())),
        ("Symbol.toStringTag".into(), Value::String("BlockList".into())),
        (
            "rules".into(),
            host_api::array(Vec::new()),
        ),
        ("size".into(), Value::Number(0.0)),
    ]);
    execute::define_property(
        object,
        "Symbol.for.nodejs.util.inspect.custom\0",
        host_api::object(vec![
            (
                "value".into(),
                crate::host::capability(crate::registry::SPEC_NET_BLOCK_LIST_INSPECT),
            ),
            ("writable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )
}

pub fn block_list_add_address(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let (address, object_family) = blocklist_address_arg(args.first())?;
    let requested = blocklist_family(args.get(1))?.or(object_family);
    let explicit = args.get(1).is_some_and(|value| !matches!(value, Value::Undefined)) || object_family.is_some();
    let ip = parse_blocklist_ip(&address)?;
    let family = blocklist_family_name(requested, ip);
    if requested.is_some_and(|family| family != blocklist_ip_family(ip)) {
        return Err(blocklist_error(
            "TypeError",
            "ERR_INVALID_ARG_VALUE",
            "The address does not match the requested IP family",
        ));
    }
    if blocklist_entries(receiver, "\0quench:blocklist:addresses")
        .iter()
        .any(|entry| execute::get_property(entry, "address") == Value::String(address.clone()))
    {
        return Ok(Value::Undefined);
    }
    let entry = host_api::object(vec![
        ("address".into(), Value::String(address.clone())),
        ("family".into(), Value::String(family.into())),
        ("explicit".into(), Value::Boolean(explicit)),
    ]);
    append_blocklist_entry(receiver, "\0quench:blocklist:addresses", entry);
    append_blocklist_rule(receiver, format!("Address: {} {address}", blocklist_family_label(family)));
    Ok(Value::Undefined)
}

pub fn block_list_add_range(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation"));
    };
    let (start, start_family) = blocklist_address_arg(args.first())?;
    let (end, end_family) = blocklist_address_arg(args.get(1))?;
    let requested = blocklist_family(args.get(2))?.or(start_family).or(end_family);
    let start_ip = parse_blocklist_ip(&start)?;
    let end_ip = parse_blocklist_ip(&end)?;
    let family = blocklist_family_name(requested, start_ip);
    if blocklist_ip_family(start_ip) != blocklist_ip_family(end_ip)
        || requested.is_some_and(|value| value != blocklist_ip_family(start_ip))
        || blocklist_ip_value(start_ip) > blocklist_ip_value(end_ip)
    {
        return Err(blocklist_error(
            "TypeError",
            "ERR_INVALID_ARG_VALUE",
            "The value of \"start\" must be lower than \"end\"",
        ));
    }
    let entry = host_api::object(vec![
        ("start".into(), Value::String(start.clone())),
        ("end".into(), Value::String(end.clone())),
        ("family".into(), Value::String(family.into())),
    ]);
    append_blocklist_entry(receiver, "\0quench:blocklist:ranges", entry);
    append_blocklist_rule(
        receiver,
        format!("Range: {} {start}-{end}", blocklist_family_label(family)),
    );
    Ok(Value::Undefined)
}

pub fn block_list_check(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation"));
    };
    let (address, object_family) = blocklist_address_arg(args.first())?;
    let requested = blocklist_family(args.get(1))?.or(object_family);
    let ip = match parse_blocklist_ip(&address) {
        Ok(ip) => ip,
        Err(_) => return Ok(Value::Boolean(false)),
    };
    let family = requested.unwrap_or_else(|| blocklist_ip_family(ip));
    let blocked = blocklist_entries(receiver, "\0quench:blocklist:addresses")
        .iter()
        .any(|entry| {
            let candidate = execute::get_property(entry, "address");
            let candidate = match candidate {
                Value::String(value) => parse_blocklist_ip(&value).ok(),
                _ => None,
            };
            let candidate_explicit = matches!(execute::get_property(entry, "explicit"), Value::Boolean(true));
            if requested.is_none()
                && candidate_explicit
                && matches!(candidate, Some(IpAddr::V6(value)) if value.to_ipv4_mapped().is_none())
            {
                return false;
            }
            candidate.is_some_and(|candidate| blocklist_match_ip(ip, family, candidate))
        })
        || blocklist_entries(receiver, "\0quench:blocklist:ranges")
            .iter()
            .any(|entry| blocklist_match_range(entry, ip, family))
        || blocklist_entries(receiver, "\0quench:blocklist:subnets")
            .iter()
            .any(|entry| blocklist_match_subnet(entry, ip, family));
    Ok(Value::Boolean(blocked))
}

pub fn block_list_add_subnet(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = _receiver else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation"));
    };
    let (network, object_family) = blocklist_address_arg(args.first())?;
    let Some(Value::Number(prefix)) = args.get(1) else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "prefix must be a number"));
    };
    let requested = blocklist_family(args.get(2))?.or(object_family);
    let ip = parse_blocklist_ip(&network)?;
    let family = requested.unwrap_or_else(|| blocklist_ip_family(ip));
    let max = if family == "ipv4" { 32.0 } else { 128.0 };
    if !prefix.is_finite() || prefix.fract() != 0.0 || *prefix < 0.0 || *prefix > max {
        return Err(blocklist_error("RangeError", "ERR_OUT_OF_RANGE", "prefix is out of range"));
    }
    if requested.is_some_and(|value| value != blocklist_ip_family(ip)) {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid IP family"));
    }
    let entry = host_api::object(vec![
        ("network".into(), Value::String(network.clone())),
        ("prefix".into(), Value::Number(*prefix)),
        ("family".into(), Value::String(family.into())),
    ]);
    append_blocklist_entry(receiver, "\0quench:blocklist:subnets", entry);
    append_blocklist_rule(
        receiver,
        format!("Subnet: {} {network}/{prefix}", blocklist_family_label(family)),
    );
    Ok(Value::Undefined)
}

pub fn socket_address_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or_else(|| host_api::object(Vec::new()));
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "options must be an object"));
    }
    let address = execute::get_property(&options, "address");
    let address = match address {
        Value::String(value) => value,
        _ => String::new(),
    };
    let family = execute::get_property(&options, "family");
    if !address.is_empty() && address.parse::<IpAddr>().is_err() {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ADDRESS", "Invalid socket address"));
    }
    if !matches!(family, Value::Undefined | Value::String(_)) {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "family must be a string"));
    }
    if let Value::String(value) = &family {
        if !value.eq_ignore_ascii_case("ipv4") && !value.eq_ignore_ascii_case("ipv6") {
            return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid IP family"));
        }
    }
    Ok(host_api::object(vec![
        ("address".into(), Value::String(address)),
        ("family".into(), family),
        ("flowlabel".into(), execute::get_property(&options, "flowlabel")),
        ("port".into(), execute::get_property(&options, "port")),
        ("Symbol.toStringTag".into(), Value::String("SocketAddress".into())),
    ]))
}

pub fn block_list_clear(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else { return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation")); };
    for key in ["\0quench:blocklist:addresses", "\0quench:blocklist:ranges", "\0quench:blocklist:subnets", "rules"] {
        let _ = execute::set_property_in_place(receiver, key, host_api::array(Vec::new()));
    }
    let _ = execute::set_property_in_place(receiver, "size", Value::Number(0.0));
    Ok(Value::Undefined)
}

pub fn block_list_add_addresses(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Array(array)) = args.first() else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "addresses must be an array"));
    };
    let family = blocklist_family(args.get(1))?;
    let values: Vec<Value> = (0..array.logical_len()).map(|index| array.index_value(index)).collect();
    for value in &values {
        let (address, object_family) = blocklist_address_arg(Some(value))?;
        let requested = family.or(object_family);
        let ip = parse_blocklist_ip(&address)?;
        if requested.is_some_and(|value| value != blocklist_ip_family(ip)) {
            return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid IP family"));
        }
    }
    for value in values {
        let mut one = vec![value];
        if let Some(family) = family { one.push(Value::String(family.into())); }
        block_list_add_address(_state, receiver, &one)?;
    }
    Ok(Value::Undefined)
}

pub fn block_list_add_cidr(
    _state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(value)) = args.first() else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "CIDR must be a string"));
    };
    let Some((address, prefix)) = value.rsplit_once('/') else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid CIDR"));
    };
    let prefix = prefix.parse::<f64>().map_err(|_| blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid CIDR"))?;
    let values = [Value::String(address.into()), Value::Number(prefix)];
    block_list_add_subnet(_state, receiver, &values)
}

pub fn block_list_add_cidrs(
    _state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Array(array)) = args.first() else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "CIDRs must be an array"));
    };
    let values: Vec<Value> = (0..array.logical_len()).map(|index| array.index_value(index)).collect();
    for value in &values { if !matches!(value, Value::String(_)) { return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "CIDR must be a string")); } }
    for value in &values {
        let Value::String(value) = value else { unreachable!() };
        if !value.contains('/') { return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid CIDR")); }
    }
    for value in values { block_list_add_cidr(_state, receiver, &[value])?; }
    Ok(Value::Undefined)
}

pub fn block_list_remove_address(
    _state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else { return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation")); };
    let (address, object_family) = blocklist_address_arg(args.first())?;
    let requested = blocklist_family(args.get(1))?.or(object_family);
    let ip = parse_blocklist_ip(&address)?;
    let mut values = blocklist_entries(receiver, "\0quench:blocklist:addresses");
    values.retain(|entry| {
        let Value::String(value) = execute::get_property(entry, "address") else { return true; };
        let Ok(candidate) = parse_blocklist_ip(&value) else { return true; };
        !blocklist_match_ip(ip, requested.unwrap_or(blocklist_ip_family(ip)), candidate)
    });
    let _ = execute::set_property_in_place(receiver, "\0quench:blocklist:addresses", host_api::array(values));
    rebuild_blocklist_rules(receiver);
    Ok(Value::Undefined)
}

pub fn block_list_remove_range(
    _state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else { return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation")); };
    let (start, start_family) = blocklist_address_arg(args.first())?;
    let (end, end_family) = blocklist_address_arg(args.get(1))?;
    let requested = blocklist_family(args.get(2))?.or(start_family).or(end_family);
    let (Ok(start), Ok(end)) = (parse_blocklist_ip(&start), parse_blocklist_ip(&end)) else { return Ok(Value::Undefined); };
    let mut values = blocklist_entries(receiver, "\0quench:blocklist:ranges");
    values.retain(|entry| {
        let (Value::String(a), Value::String(b)) = (execute::get_property(entry, "start"), execute::get_property(entry, "end")) else { return true; };
        let Ok(a) = parse_blocklist_ip(&a) else { return true; }; let Ok(b) = parse_blocklist_ip(&b) else { return true; };
        !(a == start && b == end && requested.is_none_or(|family| family == blocklist_ip_family(a)))
    });
    let _ = execute::set_property_in_place(receiver, "\0quench:blocklist:ranges", host_api::array(values));
    rebuild_blocklist_rules(receiver);
    Ok(Value::Undefined)
}

pub fn block_list_remove_subnet(
    _state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else { return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation")); };
    let (network, object_family) = blocklist_address_arg(args.first())?;
    let Some(Value::Number(prefix)) = args.get(1) else { return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "prefix must be a number")); };
    let requested = blocklist_family(args.get(2))?.or(object_family);
    let mut values = blocklist_entries(receiver, "\0quench:blocklist:subnets");
    values.retain(|entry| {
        execute::get_property(entry, "network") != Value::String(network.clone())
            || execute::get_property(entry, "prefix") != Value::Number(*prefix)
            || requested.is_some_and(|family| execute::get_property(entry, "family") != Value::String(family.into()))
    });
    let _ = execute::set_property_in_place(receiver, "\0quench:blocklist:subnets", host_api::array(values));
    rebuild_blocklist_rules(receiver);
    Ok(Value::Undefined)
}

pub fn block_list_remove_cidr(
    _state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(value)) = args.first() else { return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "CIDR must be a string")); };
    let Some((address, prefix)) = value.rsplit_once('/') else { return Ok(Value::Undefined); };
    let prefix = prefix.parse::<f64>().unwrap_or(-1.0);
    block_list_remove_subnet(_state, receiver, &[Value::String(address.into()), Value::Number(prefix)])
}

pub fn block_list_to_json(
    _state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else { return Err(blocklist_error("TypeError", "ERR_INVALID_THIS", "Illegal invocation")); };
    Ok(host_api::array(blocklist_entries(receiver, "rules")))
}

pub fn block_list_from_json(
    state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().ok_or_else(|| blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "rules must be an array"))?;
    let value = match value {
        Value::String(text) => quench_runtime::parse_json(text).map_err(|_| blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "invalid JSON"))?,
        other => other.clone(),
    };
    let Value::Array(array) = value else { return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "rules must be an array")); };
    let mut rules = Vec::new();
    for index in 0..array.logical_len() {
        let entry = array.index_value(index);
        if execute::is_symbol(&entry) {
            return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "rules must contain strings"));
        }
        let Value::String(rule) = entry else { return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "rules must contain strings")); };
        rules.push(rule);
    }
    for rule in rules {
        if let Some(value) = rule.strip_prefix("Address: IPv4 ") { block_list_add_address(state, receiver, &[Value::String(value.into())])?; }
        else if let Some(value) = rule.strip_prefix("Address: IPv6 ") { block_list_add_address(state, receiver, &[Value::String(value.into()), Value::String("ipv6".into())])?; }
        else if let Some(value) = rule.strip_prefix("Subnet: IPv4 ") { block_list_add_cidr(state, receiver, &[Value::String(value.into())])?; }
        else if let Some(value) = rule.strip_prefix("Subnet: IPv6 ") { block_list_add_cidr(state, receiver, &[Value::String(value.into())])?; }
        else if let Some(value) = rule.strip_prefix("Range: IPv4 ") { if let Some((a,b)) = value.split_once('-') { block_list_add_range(state, receiver, &[Value::String(a.into()), Value::String(b.into())])?; } }
        else if let Some(value) = rule.strip_prefix("Range: IPv6 ") { if let Some((a,b)) = value.split_once('-') { block_list_add_range(state, receiver, &[Value::String(a.into()), Value::String(b.into()), Value::String("ipv6".into())])?; } }
    }
    Ok(Value::Undefined)
}

pub fn block_list_is(
    _state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(args.first().is_some_and(|value| matches!(execute::get_property(value, "\0quench:blocklist:marker"), Value::Boolean(true)))))
}

pub fn block_list_inspect(
    _state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    if matches!(args.first(), Some(Value::Number(depth)) if *depth < 0.0) {
        return Ok(Value::String("[BlockList]".into()));
    }
    Ok(Value::String("BlockList { rules: [] }".into()))
}

fn rebuild_blocklist_rules(receiver: &Value) {
    let mut rules = Vec::new();
    for entry in blocklist_entries(receiver, "\0quench:blocklist:addresses") {
        if let (Value::String(address), Value::String(family)) = (execute::get_property(&entry, "address"), execute::get_property(&entry, "family")) {
            rules.push(Value::String(format!("Address: {} {address}", blocklist_family_label(&family))));
        }
    }
    for entry in blocklist_entries(receiver, "\0quench:blocklist:ranges") {
        if let (Value::String(start), Value::String(end), Value::String(family)) = (execute::get_property(&entry, "start"), execute::get_property(&entry, "end"), execute::get_property(&entry, "family")) {
            rules.push(Value::String(format!("Range: {} {start}-{end}", blocklist_family_label(&family))));
        }
    }
    for entry in blocklist_entries(receiver, "\0quench:blocklist:subnets") {
        if let (Value::String(network), Value::Number(prefix), Value::String(family)) = (execute::get_property(&entry, "network"), execute::get_property(&entry, "prefix"), execute::get_property(&entry, "family")) {
            rules.push(Value::String(format!("Subnet: {} {network}/{prefix}", blocklist_family_label(&family))));
        }
    }
    let _ = execute::set_property_in_place(receiver, "rules", host_api::array(rules.clone()));
    let _ = execute::set_property_in_place(receiver, "size", Value::Number(rules.len() as f64));
}

fn blocklist_error(name: &str, code: &str, message: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String(name.into())),
        ("message".into(), Value::String(message.into())),
        ("code".into(), Value::String(code.into())),
    ]))
}

fn blocklist_address_arg(value: Option<&Value>) -> Result<(String, Option<&'static str>), VmError> {
    match value {
        Some(Value::String(value)) => Ok((value.clone(), None)),
        Some(value @ (Value::Object(_) | Value::ObjectAlias(_))) => {
            let address = execute::get_property(value, "address");
            let Value::String(address) = address else {
                return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "address must be a string"));
            };
            let family = match execute::get_property(value, "family") {
                Value::String(family) if family.eq_ignore_ascii_case("ipv4") => Some("ipv4"),
                Value::String(family) if family.eq_ignore_ascii_case("ipv6") => Some("ipv6"),
                _ => None,
            };
            Ok((address, family))
        }
        _ => Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "address must be a string")),
    }
}

fn blocklist_family(value: Option<&Value>) -> Result<Option<&'static str>, VmError> {
    let Some(value) = value.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(None);
    };
    let Value::String(value) = value else {
        return Err(blocklist_error("TypeError", "ERR_INVALID_ARG_TYPE", "type must be a string"));
    };
    if value.eq_ignore_ascii_case("ipv4") {
        Ok(Some("ipv4"))
    } else if value.eq_ignore_ascii_case("ipv6") {
        Ok(Some("ipv6"))
    } else {
        Err(blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid IP family"))
    }
}

fn parse_blocklist_ip(address: &str) -> Result<IpAddr, VmError> {
    address.parse().map_err(|_| {
        blocklist_error("TypeError", "ERR_INVALID_ARG_VALUE", "invalid IP address")
    })
}

fn blocklist_ip_family(ip: IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(_) => "ipv4",
        IpAddr::V6(_) => "ipv6",
    }
}

fn blocklist_family_name(requested: Option<&str>, ip: IpAddr) -> &'static str {
    match requested {
        Some("ipv4") => "ipv4",
        Some("ipv6") => "ipv6",
        _ => blocklist_ip_family(ip),
    }
}

fn blocklist_family_label(family: &str) -> &'static str {
    match family {
        "ipv4" => "IPv4",
        _ => "IPv6",
    }
}

fn blocklist_ip_value(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(value) => u32::from(value) as u128,
        IpAddr::V6(value) => u128::from(value),
    }
}

fn blocklist_entries(receiver: &Value, key: &str) -> Vec<Value> {
    match execute::get_property(receiver, key) {
        Value::Array(array) => (0..array.logical_len()).map(|index| array.index_value(index)).collect(),
        _ => Vec::new(),
    }
}

fn append_blocklist_entry(receiver: &Value, key: &str, entry: Value) {
    let mut values = blocklist_entries(receiver, key);
    values.push(entry);
    let _ = execute::set_property_in_place(receiver, key, host_api::array(values));
}

fn append_blocklist_rule(receiver: &Value, rule: String) {
    append_blocklist_entry(receiver, "rules", Value::String(rule));
    let size = blocklist_entries(receiver, "rules").len();
    let _ = execute::set_property_in_place(receiver, "size", Value::Number(size as f64));
}

fn blocklist_match_ip(query: IpAddr, family: &str, candidate: IpAddr) -> bool {
    if family == "ipv4" {
        return match (query, candidate) {
            (IpAddr::V4(left), IpAddr::V4(right)) => left == right,
            (IpAddr::V4(query), IpAddr::V6(candidate)) => candidate.to_ipv4_mapped() == Some(query),
            _ => false,
        };
    }
    match (query, candidate) {
        (IpAddr::V6(query), IpAddr::V6(candidate)) => query == candidate,
        (IpAddr::V6(query), IpAddr::V4(candidate)) => query.to_ipv4_mapped() == Some(candidate),
        (IpAddr::V4(query), IpAddr::V6(candidate)) => candidate.to_ipv4_mapped() == Some(query),
        _ => false,
    }
}

fn blocklist_match_range(entry: &Value, query: IpAddr, family: &str) -> bool {
    let Value::String(start) = execute::get_property(entry, "start") else { return false; };
    let Value::String(end) = execute::get_property(entry, "end") else { return false; };
    let (Ok(start), Ok(end)) = (parse_blocklist_ip(&start), parse_blocklist_ip(&end)) else { return false; };
    match (query, start, end) {
        (IpAddr::V4(query), IpAddr::V4(start), IpAddr::V4(end)) if family == "ipv4" => {
            u32::from(start) <= u32::from(query) && u32::from(query) <= u32::from(end)
        }
        (IpAddr::V6(query), IpAddr::V4(start), IpAddr::V4(end))
            if query.to_ipv4_mapped().is_some() =>
        {
            let query = u32::from(query.to_ipv4_mapped().expect("mapped address"));
            u32::from(start) <= query && query <= u32::from(end)
        }
        (IpAddr::V4(query), IpAddr::V6(start), IpAddr::V6(end))
            if start.to_ipv4_mapped().is_some() && end.to_ipv4_mapped().is_some() =>
        {
            let start = u32::from(start.to_ipv4_mapped().expect("mapped address"));
            let end = u32::from(end.to_ipv4_mapped().expect("mapped address"));
            start <= u32::from(query) && u32::from(query) <= end
        }
        (IpAddr::V6(query), IpAddr::V6(start), IpAddr::V6(end)) if family == "ipv6" => {
            u128::from(start) <= u128::from(query) && u128::from(query) <= u128::from(end)
        }
        _ => false,
    }
}

fn blocklist_match_subnet(entry: &Value, query: IpAddr, family: &str) -> bool {
    let Value::String(network) = execute::get_property(entry, "network") else { return false; };
    let Value::Number(prefix) = execute::get_property(entry, "prefix") else { return false; };
    let Ok(network) = parse_blocklist_ip(&network) else { return false; };
    let (query, network, bits) = match (query, network) {
        (IpAddr::V4(query), IpAddr::V4(network)) if family == "ipv4" => {
            (u32::from(query) as u128, u32::from(network) as u128, 32)
        }
        (IpAddr::V6(query), IpAddr::V4(network)) if query.to_ipv4_mapped().is_some() => {
            (u32::from(query.to_ipv4_mapped().expect("mapped address")) as u128, u32::from(network) as u128, 32)
        }
        (IpAddr::V4(query), IpAddr::V6(network)) if network.to_ipv4_mapped().is_some() => {
            (u32::from(query) as u128, u32::from(network.to_ipv4_mapped().expect("mapped address")) as u128, 32)
        }
        (IpAddr::V6(query), IpAddr::V6(network)) if family == "ipv6" => {
            (u128::from(query), u128::from(network), 128)
        }
        _ => return false,
    };
    let prefix = if bits == 32 && prefix > 32.0 {
        // An IPv4-mapped IPv6 network (for example ::ffff:10.0.0.0/120)
        // is compared in its embedded IPv4 space.
        prefix - 96.0
    } else {
        prefix
    } as usize;
    if prefix > bits {
        return false;
    }
    prefix == 0 || (query >> (bits - prefix)) == (network >> (bits - prefix))
}
