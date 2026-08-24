//! `net` server and socket methods: construction, listen/close/address,
//! connect, write/end/destroy, and the socket configuration no-ops.

use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::*;

/// `net.createServer([connectionListener])` — a server object backed by
/// an emitter; the listener, if given, registers for `'connection'`.
pub fn create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let (object, _id) = new_net_object(state, server_props())?;
    register_server(state, &object, None)?;
    add_listener_cb(state, &object, args.first(), "connection")?;
    Ok(object)
}

/// `net.connect(port[, host][, cb])` / `net.connect(options, cb)`.
/// Connects (bounded) on loopback and returns a socket object;
/// `'connect'` fires on the next pump tick.
pub fn connect(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let (port, host) = connect_target(state, args)?;
    let addr = resolve(host.as_deref().unwrap_or(LOCAL_HOST), port);
    let stream = match TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(3000)) {
        Ok(stream) => stream,
        Err(_) => return connect_refused(state, &addr),
    };
    let _ = stream.set_nonblocking(true);
    let (object, id) = new_net_object(state, socket_props())?;
    let local = stream.local_addr().ok();
    let object = install_methods(object, net_info_props(addr, local))?;
    let socket = Rc::new(std::cell::RefCell::new(NetSocket {
        id,
        stream: Some(stream),
        js: object.clone(),
        state: SocketState::Open,
        server_id: None,
        write_buf: Vec::new(),
        read_eof: false,
        close_emitted: false,
        connect_announced: false,
        encoding: None,
    }));
    state.borrow_mut().net.sockets.insert(id, socket);
    add_listener_cb(state, &object, args.last(), "connect")?;
    Ok(object)
}

/// A refused/absent loopback peer surfaces as an `'error'` on a
/// destroyed socket (never a synchronous throw).
fn connect_refused(state: &Rc<RefCell<HostState>>, addr: &SocketAddr) -> Result<Value, VmError> {
    let (object, _id) = new_net_object(state, socket_props())?;
    let error = host_api::object(vec![
        ("name".to_string(), Value::String("Error".to_string())),
        (
            "message".to_string(),
            Value::String(format!("connect ECONNREFUSED {addr}")),
        ),
        (
            "code".to_string(),
            Value::String("ECONNREFUSED".to_string()),
        ),
        ("errno".to_string(), Value::Number(-61.0)),
        ("syscall".to_string(), Value::String("connect".to_string())),
    ]);
    emit(state, &object, "error", vec![error])?;
    Ok(object)
}

fn connect_target(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<(u16, Option<String>), VmError> {
    match args.first() {
        Some(Value::Object(_)) => {
            let options = args.first().cloned().unwrap_or(Value::Undefined);
            let port = execute::to_js_string(&execute::get_property_result(&options, "port")?)?
                .parse::<u16>()
                .map_err(|_| execute::type_error("port must be a number"))?;
            let host = execute::get_property_result(&options, "host")
                .ok()
                .and_then(|v| execute::to_js_string(&v).ok());
            Ok((port, host))
        }
        _ => {
            let _ = state;
            let port = args
                .first()
                .map(execute::to_js_string)
                .transpose()?
                .unwrap_or_default()
                .parse::<u16>()
                .map_err(|_| execute::type_error("port must be a number"))?;
            let host = args.get(1).and_then(|v| execute::to_js_string(v).ok());
            Ok((port, host))
        }
    }
}

/// `server.listen(port[, host][, cb])` (or `listen(options, cb)`).
/// Binds a non-blocking listener; `'listening'` fires next pump tick.
pub fn server_listen(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let (port, host) = listen_target(state, args)?;
    let listener = match bind_listener(port, host.as_deref()) {
        Ok(listener) => listener,
        Err(error) => return Err(server_bind_error(&error)),
    };
    register_server(state, &receiver, Some(listener))?;
    add_listener_cb(state, &receiver, args.last(), "listening")?;
    Ok(receiver.clone())
}

/// Resolve the `(port, host)` listen target, mirroring `connect_target`.
fn listen_target(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<(u16, Option<String>), VmError> {
    if matches!(args.first(), Some(Value::Object(_))) {
        return connect_target(state, args);
    }
    let port = args
        .first()
        .map(execute::to_js_string)
        .transpose()?
        .unwrap_or_default()
        .parse::<u16>()
        .map_err(|_| execute::type_error("port must be a number"))?;
    let host = args.get(1).and_then(|v| execute::to_js_string(v).ok());
    Ok((port, host))
}

fn bind_listener(port: u16, host: Option<&str>) -> std::io::Result<TcpListener> {
    let addr = resolve(host.unwrap_or("0.0.0.0"), port);
    let listener = TcpListener::bind(addr)?;
    let _ = listener.set_nonblocking(true);
    Ok(listener)
}

/// Register a callable `cb` as a one-shot listener for `event`.
fn add_listener_cb(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    cb: Option<&Value>,
    event: &str,
) -> Result<(), VmError> {
    if let Some(cb) = cb {
        if quench_runtime::is_callable(cb) {
            crate::modules::events::method_on(
                state,
                Some(receiver),
                &[Value::String(event.to_string()), cb.clone()],
            )?;
        }
    }
    Ok(())
}

fn server_bind_error(error: &std::io::Error) -> VmError {
    let code = bind_code(error);
    let message = format!("{code}: Another server is running on port");
    let props = vec![
        ("name".to_string(), Value::String("Error".to_string())),
        ("message".to_string(), Value::String(message)),
        ("code".to_string(), Value::String(code.to_string())),
    ];
    VmError::Thrown(host_api::object(props))
}

fn bind_code(error: &std::io::Error) -> &'static str {
    if let Some(raw) = error.raw_os_error() {
        match raw {
            48 => "EADDRINUSE",
            49 => "EADDRNOTAVAIL",
            _ => "EADDRINUSE",
        }
    } else {
        "EADDRINUSE"
    }
}

/// `server.close([cb])` — stop listening; `'close'` fires once no
/// connection remains.
pub fn server_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let Some(id) = net_id(&receiver) else {
        return Ok(receiver);
    };
    if let Some(server) = state.borrow().net.servers.get(&id).cloned() {
        let mut server = server.borrow_mut();
        server.listener.take();
        server.listening = false;
        server.closed = true;
    }
    add_listener_cb(state, &receiver, args.first(), "close")?;
    Ok(receiver)
}

/// `server.address()` — the bound address object, or null.
pub fn server_address(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = receiver.and_then(net_id) else {
        return Ok(Value::Null);
    };
    let bind_addr = state
        .borrow()
        .net
        .servers
        .get(&id)
        .and_then(|s| s.borrow().bind_addr);
    Ok(bind_addr.map_or(Value::Null, address_value))
}

/// `socket.write(data[, encoding][, cb])` — buffers bytes and flushes
/// what the socket will take; returns whether everything flushed.
pub fn socket_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = receiver.and_then(net_id) else {
        return Ok(Value::Boolean(false));
    };
    let Some(sock) = state.borrow().net.sockets.get(&id).cloned() else {
        return Ok(Value::Boolean(false));
    };
    let bytes = match args.first() {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Uint8Array(view)) => {
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        _ => return Ok(Value::Boolean(false)),
    };
    let mut guard = sock.borrow_mut();
    if guard.state == SocketState::Closed {
        return Ok(Value::Boolean(false));
    }
    guard.write_buf.extend_from_slice(&bytes);
    let flushed = try_flush(&mut guard);
    Ok(Value::Boolean(flushed))
}

/// `socket.end([data][, cb])` — write `data` if given, then close the
/// write side; the socket closes fully once the peer also ends.
pub fn socket_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(data) = args.first() {
        if !matches!(data, Value::Undefined) {
            socket_write(state, receiver, std::slice::from_ref(data))?;
        }
    }
    let Some(id) = receiver.and_then(net_id) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    if let Some(sock) = state.borrow().net.sockets.get(&id).cloned() {
        let mut guard = sock.borrow_mut();
        guard.state = SocketState::Closing;
        try_flush(&mut guard);
        if guard.write_buf.is_empty() {
            if let Some(stream) = guard.stream.as_mut() {
                let _ = stream.shutdown(Shutdown::Write);
            }
        }
    }
    let _ = state;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `socket.destroy()` — drop the socket and emit `'close'`.
pub fn socket_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let Some(id) = net_id(&receiver) else {
        return Ok(receiver);
    };
    let mut emit_close = false;
    if let Some(sock) = state.borrow().net.sockets.get(&id).cloned() {
        let mut guard = sock.borrow_mut();
        if guard.state != SocketState::Closed {
            if let Some(stream) = guard.stream.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            guard.state = SocketState::Closed;
            emit_close = true;
        }
    }
    if emit_close {
        emit(state, &receiver, "close", Vec::new())?;
    }
    Ok(receiver)
}

/// `socket.address()` — the local address object.
pub fn socket_address(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = receiver.and_then(net_id) else {
        return Ok(Value::Null);
    };
    let addr = state.borrow().net.sockets.get(&id).and_then(|s| {
        s.borrow()
            .stream
            .as_ref()
            .and_then(|st| st.local_addr().ok())
    });
    Ok(addr.map_or(Value::Null, address_value))
}

/// `socket.setNoDelay([noDelay])` — accepted for loopback; no-op.
pub fn socket_set_no_delay(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `socket.setKeepAlive([enable][, initialDelay])` — no-op.
pub fn socket_set_keep_alive(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `socket.setEncoding(encoding)` — decode `'data'` chunks to strings.
pub fn socket_set_encoding(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let encoding = args.first().map(execute::to_js_string).transpose()?;
    if let Some(id) = receiver.and_then(net_id) {
        if let Some(sock) = state.borrow().net.sockets.get(&id) {
            sock.borrow_mut().encoding = encoding;
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `socket.pause()` / `socket.resume()` — accepted as a no-op.
pub fn socket_pause(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn socket_resume(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}
