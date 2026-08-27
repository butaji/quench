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

/// `new net.Socket()` creates an unconnected socket whose `connect` method
/// shares the public connection capability and validation path.
pub fn socket_construct(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_)))
    {
        let fd = execute::get_property(options, "fd");
        if let Value::String(_) = fd {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ])));
        }
        if matches!(fd, Value::Number(value) if value < 0.0) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("RangeError".into())),
                ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
            ])));
        }
    }
    let (object, _id) = new_net_object(state, socket_props())?;
    let object = install_socket_counters(object)?;
    install_methods(
        object,
        vec![(
            "connect".to_string(),
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        )],
    )
}

/// `net.connect(port[, host][, cb])` / `net.connect(options, cb)`.
/// Connects (bounded) on loopback and returns a socket object;
/// `'connect'` fires on the next pump tick.
pub fn connect(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    connect_with_receiver(state, None, args)
}

pub fn connect_existing(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    connect_with_receiver(state, Some(receiver), args)
}

fn connect_with_receiver(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(path) = args
        .first()
        .filter(|value| matches!(value, Value::String(_)))
    {
        let path = execute::to_js_string(path)?;
        if path.starts_with('/') {
            let (object, _) = new_net_object(state, socket_props())?;
            let error = host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                ("code".into(), Value::String("ENOENT".into())),
                ("syscall".into(), Value::String("connect".into())),
            ]);
            let error = execute::define_property(
                error,
                "message",
                host_api::object(vec![
                    (
                        "value".into(),
                        Value::String(format!("connect ENOENT {path}")),
                    ),
                    ("enumerable".into(), Value::Boolean(false)),
                ]),
            )?;
            state
                .borrow_mut()
                .net
                .pending_errors
                .push((object.clone(), error));
            return Ok(object);
        }
    }
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_)))
    {
        let auto_select_family = execute::get_property(options, "autoSelectFamily");
        if !matches!(auto_select_family, Value::Undefined | Value::Boolean(_)) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ])));
        }
        let hints = execute::get_property(options, "hints");
        if let Value::Number(value) = hints {
            let bits = value as i64;
            if value.fract() != 0.0 || bits < 0 || bits & !7 != 0 {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("TypeError".into())),
                    ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                    ("message".into(), Value::String(format!(
                        "The argument 'hints' is invalid. Received {value}"
                    ))),
                ])));
            }
        }
        let path = execute::get_property(options, "path");
        if !matches!(path, Value::Undefined | Value::Null) && !matches!(path, Value::String(_)) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                (
                    "message".into(),
                    Value::String("The \"path\" argument must be a string".into()),
                ),
            ])));
        }
    }
    let (port, host) = connect_target(state, args)?;
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_)))
    {
        let block_list = execute::get_property(options, "blockList");
        let address = host.as_deref().unwrap_or(LOCAL_HOST);
        if quench_runtime::is_callable(&execute::get_property(&block_list, "check")) {
            let checked = execute::call(
                &execute::get_property(&block_list, "check"),
                &block_list,
                &[Value::String(if address == "localhost" {
                    LOCAL_HOST.into()
                } else {
                    address.into()
                })],
            )?;
            if execute::is_truthy(&checked) {
                let (object, _) = new_net_object(state, socket_props())?;
                let error = host_api::object(vec![
                    ("name".into(), Value::String("Error".into())),
                    ("code".into(), Value::String("ERR_IP_BLOCKED".into())),
                ]);
                state
                    .borrow_mut()
                    .net
                    .pending_errors
                    .push((object.clone(), error));
                return Ok(object);
            }
        }
    }
    let addr = resolve(host.as_deref().unwrap_or(LOCAL_HOST), port);
    let stream = match TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(3000)) {
        Ok(stream) => stream,
        Err(_) => return connect_refused(state, &addr),
    };
    let _ = stream.set_nonblocking(true);
    let (object, id) = match receiver {
        Some(object) => (
            object.clone(),
            net_id(object).unwrap_or_else(|| allocate_id(state)),
        ),
        None => {
            let (object, id) = new_net_object(state, socket_props())?;
            (install_socket_counters(object)?, id)
        }
    };
    let local = stream.local_addr().ok();
    set_socket_state(&object, true, true, "opening");
    let socket = Rc::new(std::cell::RefCell::new(NetSocket {
        id,
        stream: Some(stream),
        js: object.clone(),
        state: SocketState::Open,
        server_id: None,
        write_buf: Vec::new(),
        bytes_read: 0,
        bytes_written: 0,
        read_eof: false,
        close_emitted: false,
        connect_announced: false,
        peer: Some(addr),
        local,
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
            let port_value = execute::get_property_result(&options, "port")?;
            if matches!(port_value, Value::Undefined) {
                return Err(missing_connect_args());
            }
            let port = parse_port(&port_value)?;
            let host = execute::get_property_result(&options, "host")
                .ok()
                .and_then(|v| execute::to_js_string(&v).ok());
            Ok((port, host))
        }
        _ => {
            let _ = state;
            let Some(value) = args.first() else {
                return Err(missing_connect_args());
            };
            if matches!(value, Value::Undefined) {
                return Err(missing_connect_args());
            }
            let port = parse_port(value)?;
            let host = args.get(1).and_then(|v| match v {
                Value::String(_) => execute::to_js_string(v).ok(),
                _ => None,
            });
            Ok((port, host))
        }
    }
}

fn missing_connect_args() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "code".to_string(),
            Value::String("ERR_MISSING_ARGS".to_string()),
        ),
        (
            "message".to_string(),
            Value::String(
                "The \"options\" or \"port\" or \"path\" argument must be specified".to_string(),
            ),
        ),
    ]))
}

fn parse_port(value: &Value) -> Result<u16, VmError> {
    if !matches!(value, Value::Number(_) | Value::String(_)) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        ])));
    }
    let text = execute::to_js_string(value)?;
    let Ok(port) = text.parse::<i64>() else {
        return Err(execute::type_error("port must be a number"));
    };
    if !(0..=u16::MAX as i64).contains(&port) {
        return Err(bad_port(value, &text));
    }
    Ok(port as u16)
}

fn bad_port(value: &Value, text: &str) -> VmError {
    let kind = match value {
        Value::Number(_) => "number",
        Value::String(_) => "string",
        _ => "object",
    };
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("RangeError".to_string())),
        (
            "code".to_string(),
            Value::String("ERR_SOCKET_BAD_PORT".to_string()),
        ),
        (
            "message".to_string(),
            Value::String(format!(
                "options.port should be >= 0 and < 65536. Received type {kind} ({text})."
            )),
        ),
    ]))
}

/// `server.listen(port[, host][, cb])` (or `listen(options, cb)`).
/// Binds a non-blocking listener; `'listening'` fires next pump tick.
pub fn server_listen(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    if let Some(Value::String(path)) = args.first() {
        if path.starts_with('/') {
            state
                .borrow_mut()
                .net
                .pending_errors
                .push((receiver.clone(), server_path_error(path)));
            return Ok(receiver);
        }
    }
    let (port, host) = listen_target(state, args)?;
    let listener = match bind_listener(port, host.as_deref()) {
        Ok(listener) => listener,
        Err(error) => {
            state.borrow_mut().net.pending_errors.push((
                receiver.clone(),
                server_bind_error(&error, host.as_deref(), port),
            ));
            return Ok(receiver);
        }
    };
    register_server(state, &receiver, Some(listener))?;
    super::set_server_connection_key(&receiver, port, host.as_deref())?;
    add_listener_cb(state, &receiver, args.last(), "listening")?;
    Ok(receiver.clone())
}

fn server_path_error(path: &str) -> Value {
    host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "message".into(),
            Value::String("No such file or directory".into()),
        ),
        ("code".into(), Value::String("ENOENT".into())),
        ("address".into(), Value::String(path.into())),
        ("port".into(), Value::Number(0.0)),
        ("syscall".into(), Value::String("bind".into())),
    ])
}

/// Resolve the `(port, host)` listen target, mirroring `connect_target`.
fn listen_target(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<(u16, Option<String>), VmError> {
    if matches!(args.first(), Some(Value::Object(_))) {
        return connect_target(state, args);
    }
    let value = args.first().cloned().unwrap_or(Value::Number(0.0));
    let port = parse_port(&value)?;
    let host = args.get(1).and_then(|v| match v {
        Value::String(_) => execute::to_js_string(v).ok(),
        _ => None,
    });
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

fn server_bind_error(error: &std::io::Error, host: Option<&str>, port: u16) -> Value {
    let code = bind_code(error);
    let message = format!("{code}: Another server is running on port");
    let props = vec![
        ("name".to_string(), Value::String("Error".to_string())),
        ("message".to_string(), Value::String(message)),
        ("code".to_string(), Value::String(code.to_string())),
        (
            "address".to_string(),
            Value::String(host.unwrap_or("0.0.0.0").to_string()),
        ),
        ("port".to_string(), Value::Number(f64::from(port))),
        ("syscall".to_string(), Value::String("listen".to_string())),
    ];
    host_api::object(props)
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
    super::set_server_listening(&receiver, false)?;
    add_listener_cb(state, &receiver, args.first(), "close")?;
    Ok(receiver)
}

pub fn server_unref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| execute::type_error("server"))?;
    let id = super::net_id(receiver).ok_or_else(|| execute::type_error("server"))?;
    if let Some(server) = state.borrow().net.servers.get(&id) {
        server.borrow_mut().refed = false;
    }
    Ok(receiver.clone())
}

pub fn server_ref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| execute::type_error("server"))?;
    let id = super::net_id(receiver).ok_or_else(|| execute::type_error("server"))?;
    if let Some(server) = state.borrow().net.servers.get(&id) {
        server.borrow_mut().refed = true;
    }
    Ok(receiver.clone())
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
    guard.bytes_written = guard.bytes_written.saturating_add(bytes.len() as u64);
    guard.write_buf.extend_from_slice(&bytes);
    update_socket_counters(&guard);
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

pub fn socket_unref(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn socket_ref(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
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
