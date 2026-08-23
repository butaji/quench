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
    connect, create_server, server_address, server_close, server_listen, socket_address,
    socket_destroy, socket_end, socket_pause, socket_resume, socket_set_encoding,
    socket_set_keep_alive, socket_set_no_delay, socket_write,
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
    pub listener: Option<TcpListener>,
    pub bind_addr: Option<SocketAddr>,
    pub js: Value,
    pub listening: bool,
    pub announced: bool,
    pub closed: bool,
    pub close_emitted: bool,
}

pub struct NetSocket {
    pub id: u64,
    pub stream: Option<TcpStream>,
    pub js: Value,
    pub state: SocketState,
    pub server_id: Option<u64>,
    pub write_buf: Vec<u8>,
    pub read_eof: bool,
    pub close_emitted: bool,
    pub connect_announced: bool,
    pub encoding: Option<String>,
}

pub struct NetState {
    next: u64,
    pub servers: HashMap<u64, Rc<RefCell<NetServer>>>,
    pub sockets: HashMap<u64, Rc<RefCell<NetSocket>>>,
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
        }
    }
}

/// Does any live net object keep the event loop alive?
pub fn has_work(state: &Rc<RefCell<HostState>>) -> bool {
    let host = state.borrow();
    host.net
        .servers
        .values()
        .any(|s| s.borrow().listening && !s.borrow().closed)
        || host
            .net
            .sockets
            .values()
            .any(|s| s.borrow().state != SocketState::Closed)
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
    }
    props
}

fn server_props() -> Vec<(&'static str, Value)> {
    vec![
        ("listen", cap(crate::registry::SPEC_NET_SERVER_LISTEN)),
        ("close", cap(crate::registry::SPEC_NET_SERVER_CLOSE)),
        ("address", cap(crate::registry::SPEC_NET_SERVER_ADDRESS)),
    ]
}

fn socket_props() -> Vec<(&'static str, Value)> {
    vec![
        ("write", cap(crate::registry::SPEC_NET_SOCKET_WRITE)),
        ("end", cap(crate::registry::SPEC_NET_SOCKET_END)),
        ("destroy", cap(crate::registry::SPEC_NET_SOCKET_DESTROY)),
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
    let id = net_id(js).ok_or_else(|| execute::type_error("not a net object"))?;
    // A bound listener means the server is listening (createServer
    // registers without one; listen() registers with one).
    let is_listening = listener.is_some();
    let bind_addr = listener.as_ref().and_then(|l| l.local_addr().ok());
    state.borrow_mut().net.servers.insert(
        id,
        Rc::new(RefCell::new(NetServer {
            id,
            listener,
            bind_addr,
            js: js.clone(),
            listening: is_listening,
            announced: false,
            closed: false,
            close_emitted: false,
        })),
    );
    Ok(id)
}

pub(crate) fn net_id(receiver: &Value) -> Option<u64> {
    match quench_runtime::vm::get_property(receiver, NET_ID_PROP) {
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
    let listeners: Vec<Listener> = emitter_id(receiver)
        .and_then(|id| state.borrow().emitters.get(id))
        .map(|emitter| emitter.borrow().listeners_of(event).to_vec())
        .unwrap_or_default();
    if listeners.is_empty() {
        if event == "error" {
            return Err(unhandled_error(args.first()));
        }
        return Ok(());
    }
    for listener in &listeners {
        if listener.once {
            if let Some(id) = emitter_id(receiver) {
                if let Some(emitter) = state.borrow().emitters.get(id) {
                    emitter.borrow_mut().remove(event, &listener.callback);
                }
            }
        }
        execute::call(&listener.callback, receiver, &args)?;
    }
    Ok(())
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
    if std::net::Ipv6Addr::from_str(&s).is_ok() {
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
    s.parse::<std::net::Ipv6Addr>().is_ok()
}

pub fn build() -> Value {
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
