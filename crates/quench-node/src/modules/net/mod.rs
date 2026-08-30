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
    complete_lookup, connect, connect_existing, connect_path, create_server, server_address, server_close,
    server_close_idle, server_listen, server_ref, server_unref, socket_address, socket_construct,
    socket_destroy, socket_end, socket_pause, socket_ref, socket_resume, socket_set_encoding,
    socket_set_keep_alive, socket_set_no_delay, socket_set_timeout, socket_timeout_fire,
    socket_unref, socket_write,
};
pub use pump::{finalize, poll};

const LOCAL_HOST: &str = "127.0.0.1";
/// Hidden property that stores the host-side net id on a JS object.
const NET_ID_PROP: &str = "\0quench:net:id";
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
    pub listener: Option<TcpListener>,
    pub path: Option<String>,
    pub bind_addr: Option<SocketAddr>,
    pub js: Value,
    pub listening: bool,
    pub refed: bool,
    pub announced: bool,
    pub closed: bool,
    pub close_emitted: bool,
}

pub struct NetSocket {
    pub id: u64,
    pub stream: Option<TcpStream>,
    pub js: Value,
    pub state: SocketState,
    pub refed: bool,
    pub server_id: Option<u64>,
    pub write_buf: Vec<u8>,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_eof: bool,
    pub close_emitted: bool,
    pub finish_emitted: bool,
    pub connect_announced: bool,
    pub peer: Option<SocketAddr>,
    pub local: Option<SocketAddr>,
    pub encoding: Option<String>,
}

pub struct NetState {
    next: u64,
    pub servers: HashMap<u64, Rc<RefCell<NetServer>>>,
    pub sockets: HashMap<u64, Rc<RefCell<NetSocket>>>,
    pub pending_errors: Vec<(Value, Value)>,
    pub lookup_result: Option<Value>,
    pub lookup_in_call: bool,
    pub pending_lookups: Vec<PendingLookup>,
    pub pending_events: Vec<(Value, String, Vec<Value>)>,
    pub paths: HashMap<String, u16>,
    pub pending_writes: Vec<(Value, Vec<u8>)>,
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
            pending_errors: Vec::new(),
            lookup_result: None,
            lookup_in_call: false,
            pending_lookups: Vec::new(),
            pending_events: Vec::new(),
            paths: HashMap::new(),
            pending_writes: Vec::new(),
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
        || !host.net.pending_lookups.is_empty()
        || host.net.sockets.values().any(|s| {
            let socket = s.borrow();
            socket.refed
                && socket.state != SocketState::Closed
                && !socket.read_eof
                && (socket.stream.is_some() || socket.server_id.is_some())
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
            (
                "writableHighWaterMark".to_string(),
                Value::Number(16_384.0),
            ),
            ("pending".to_string(), Value::Boolean(true)),
            ("connecting".to_string(), Value::Boolean(false)),
            (
                "readyState".to_string(),
                Value::String("closed".to_string()),
            ),
        ],
    )
}

pub(crate) fn set_socket_state(socket: &Value, pending: bool, connecting: bool, ready_state: &str) {
    let mut properties = vec![
        ("pending", Value::Boolean(pending)),
        ("connecting", Value::Boolean(connecting)),
        ("readyState", Value::String(ready_state.to_string())),
    ];
    if ready_state == "closed" {
        properties.push(("destroyed", Value::Boolean(true)));
    }
    for (key, value) in properties {
        execute::set_property_in_place(socket, key, value.clone());
        let updated = execute::set_property(socket.clone(), key, value);
        execute::replace_value(socket, &updated);
    }
}

pub(crate) fn update_socket_counters(socket: &NetSocket) {
    execute::set_property_in_place(
        &socket.js,
        "bytesRead",
        Value::Number(socket.bytes_read as f64),
    );
    execute::set_property_in_place(
        &socket.js,
        "bytesWritten",
        Value::Number(socket.bytes_written as f64),
    );
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
        ("listen", cap(crate::registry::SPEC_NET_SERVER_LISTEN)),
        ("close", cap(crate::registry::SPEC_NET_SERVER_CLOSE)),
        (
            "closeIdleConnections",
            cap(crate::registry::SPEC_NET_SERVER_CLOSE_IDLE),
        ),
        ("address", cap(crate::registry::SPEC_NET_SERVER_ADDRESS)),
        ("unref", cap(crate::registry::SPEC_NET_SERVER_UNREF)),
        ("ref", cap(crate::registry::SPEC_NET_SERVER_REF)),
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
        // Node exposes a nulled-out native handle after a socket closes.
        ("_handle", Value::Null),
        ("readable", Value::Boolean(true)),
        ("writable", Value::Boolean(true)),
        ("allowHalfOpen", Value::Boolean(false)),
        ("destroyed", Value::Boolean(false)),
        ("connecting", Value::Boolean(false)),
        ("readyState", Value::String("open".into())),
        ("connect", cap(crate::registry::SPEC_NET_CONNECT)),
        ("write", cap(crate::registry::SPEC_NET_SOCKET_WRITE)),
        ("end", cap(crate::registry::SPEC_NET_SOCKET_END)),
        ("destroy", cap(crate::registry::SPEC_NET_SOCKET_DESTROY)),
        ("unref", cap(crate::registry::SPEC_NET_SOCKET_UNREF)),
        ("ref", cap(crate::registry::SPEC_NET_SOCKET_REF)),
        ("address", cap(crate::registry::SPEC_NET_SOCKET_ADDRESS)),
        (
            "setNoDelay",
            cap(crate::registry::SPEC_NET_SOCKET_SET_NO_DELAY),
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
    let owner_worker = state.borrow().cluster.worker_context;
    state.borrow_mut().net.servers.insert(
        id,
        Rc::new(RefCell::new(NetServer {
            id,
            owner_worker,
            listener,
            path,
            bind_addr,
            js: js.clone(),
            listening: is_listening,
            refed: true,
            announced: false,
            closed: false,
            close_emitted: false,
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
    if guard.write_buf.is_empty() {
        return true;
    }
    let pending = guard.write_buf.clone();
    let written = {
        let Some(stream) = guard.stream.as_mut() else {
            return false;
        };
        stream.write(&pending).unwrap_or(0)
    };
    if written > 0 {
        guard.write_buf.drain(..written);
    }
    guard.write_buf.is_empty()
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
    let listeners: Vec<Listener> = id
        .and_then(|id| state.borrow().emitters.get(id))
        .map(|emitter| emitter.borrow().listeners_of(event).to_vec())
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
                        emitter.borrow_mut().remove(event, &listener.callback);
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
fn data_value(guard: &NetSocket, bytes: &[u8]) -> Value {
    match guard.encoding.as_deref() {
        Some("utf8") | Some("utf-8") => Value::String(String::from_utf8_lossy(bytes).into_owned()),
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
    let socket_prototype = host_api::object(Vec::new());
    let global = quench_runtime::vm::current_global_object();
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
