//! `dgram` module — real loopback UDP sockets over `std::net::UdpSocket`.
//!
//! Sockets live in a host registry keyed by an opaque id attached to each
//! JS socket object. `poll` runs from the event-loop pump and fans received
//! datagrams out to each socket's `'message'` listeners.

use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;

const ID: &str = "\0quench:dgram:id";

struct Sock {
    socket: UdpSocket,
    js: Value,
}

thread_local! {
    static SOCKS: RefCell<HashMap<u64, Sock>> = RefCell::new(HashMap::new());
    static NEXT: RefCell<u64> = RefCell::new(1);
}

fn install(mut object: Value, name: &str, value: Value) -> Result<Value, VmError> {
    object = execute::set_property(object, name, value);
    Ok(object)
}

fn sock_id(value: &Value) -> Option<u64> {
    execute::get_property_result(value, ID)
        .ok()
        .and_then(|v| if let Value::Number(n) = v { Some(n as u64) } else { None })
}

fn addr_object(addr: SocketAddr) -> Value {
    quench_runtime::host_api::object(vec![
        ("address".to_string(), Value::String(addr.ip().to_string())),
        ("family".to_string(), Value::String("IPv4".into())),
        ("port".to_string(), Value::Number(addr.port() as f64)),
    ])
}

fn rinfo_object(addr: SocketAddr, size: usize) -> Value {
    quench_runtime::host_api::object(vec![
        ("address".to_string(), Value::String(addr.ip().to_string())),
        ("family".to_string(), Value::String("IPv4".into())),
        ("port".to_string(), Value::Number(addr.port() as f64)),
        ("size".to_string(), Value::Number(size as f64)),
    ])
}

/// `dgram.createSocket(type)` — build a socket object. `type` is accepted but
/// only IPv4-ish loopback behaviour is meaningful today.
pub fn create_socket(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let mut object = crate::modules::events::new_emitter_object(state)?;
    for (name, cap_id) in [
        ("bind", 0x2301),
        ("send", 0x2302),
        ("close", 0x2303),
        ("address", 0x2304),
    ] {
        object = install(object, name, crate::host::capability(crate::registry::NodeSpec::new("dgram:method", cap_id)))?;
    }
    let id = NEXT.with(|next| {
        let value = *next.borrow();
        *next.borrow_mut() += 1;
        value
    });
    install(object, ID, Value::Number(id as f64))
}

/// `socket.bind(port[, address][, cb])` — bind and emit `'listening'`.
pub fn bind(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let port = args
        .first()
        .and_then(|v| execute::to_js_string(v).ok())
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let host = args
        .get(1)
        .and_then(|v| execute::to_js_string(v).ok())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let socket = UdpSocket::bind((host.as_str(), port))
        .map_err(|e| execute::type_error(&e.to_string()))?;
    socket.set_nonblocking(true).ok();
    if let Some(id) = sock_id(&receiver) {
        SOCKS.with(|registry| {
            registry
                .borrow_mut()
                .insert(id, Sock { socket, js: receiver.clone() });
        });
        crate::modules::net::emit(state, &receiver, "listening", Vec::new())?;
    }
    Ok(receiver)
}

/// `socket.send(buffer, port[, address][, cb])` — send one datagram.
pub fn send(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let Some(id) = sock_id(&receiver) else {
        return Ok(receiver);
    };
    let data = match args.first() {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Uint8Array(a)) => {
            let bytes = a.buffer.bytes.borrow();
            bytes[a.byte_offset..a.byte_offset + a.length].to_vec()
        }
        _ => Vec::new(),
    };
    let port = args
        .get(1)
        .and_then(|v| execute::to_js_string(v).ok())
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let host = args
        .get(2)
        .and_then(|v| execute::to_js_string(v).ok())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    SOCKS.with(|registry| {
        if let Some(sock) = registry.borrow().get(&id) {
            let _ = sock.socket.send_to(&data, (host.as_str(), port));
        }
    });
    if let Some(callback) = args.last() {
        if quench_runtime::is_callable(callback) {
            execute::call(callback, &Value::Undefined, &[Value::Number(data.len() as f64)]).ok();
        }
    }
    Ok(receiver)
}

/// `socket.close([cb])` — close the socket and emit `'close'`.
pub fn close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    if let Some(id) = sock_id(&receiver) {
        SOCKS.with(|registry| {
            registry.borrow_mut().remove(&id);
        });
        crate::modules::net::emit(state, &receiver, "close", Vec::new())?;
    }
    Ok(receiver)
}

/// `socket.address()` — bound local address, or `null` when unbounded.
pub fn address(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Null);
    };
    let Some(id) = sock_id(receiver) else {
        return Ok(Value::Null);
    };
    let value = SOCKS.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .and_then(|sock| sock.socket.local_addr().ok())
            .map(addr_object)
            .unwrap_or(Value::Null)
    });
    Ok(value)
}

/// Event-loop pump: deliver any pending datagrams to `'message'` listeners.
pub fn poll(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let sockets: Vec<Value> = SOCKS.with(|registry| {
        registry
            .borrow()
            .values()
            .map(|sock| sock.js.clone())
            .collect()
    });
    for js in sockets {
        let Some(id) = sock_id(&js) else { continue };
        let received: Option<(Vec<u8>, SocketAddr)> = SOCKS.with(|registry| {
            let binding = registry.borrow();
            let sock = binding.get(&id)?;
            let mut buffer = [0u8; 65536];
            sock.socket
                .recv_from(&mut buffer)
                .ok()
                .map(|(n, addr)| (buffer[..n].to_vec(), addr))
        });
        if let Some((bytes, addr)) = received {
            let data = crate::modules::buffer_proto::make_buffer(&bytes);
            let info = rinfo_object(addr, bytes.len());
            crate::modules::net::emit(state, &js, "message", vec![data, info])?;
        }
    }
    Ok(())
}

