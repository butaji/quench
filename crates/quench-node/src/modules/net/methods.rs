//! `net` server and socket methods: construction, listen/close/address,
//! connect, write/end/destroy, and the socket configuration no-ops.

use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::*;

pub(crate) const SOCKET_TIMEOUT_PROP: &str = "\0quench:net:timeout";

fn write_chunk_type_error(value: &Value) -> VmError {
    let detail = match value {
        Value::Undefined => " Received undefined".into(),
        Value::Boolean(value) => format!(" Received type boolean ({value})"),
        Value::Number(value) => {
            let rendered = if value.is_infinite() {
                if value.is_sign_negative() {
                    "-Infinity"
                } else {
                    "Infinity"
                }
                .to_string()
            } else {
                value.to_string()
            };
            format!(" Received type number ({rendered})")
        }
        Value::Array(_) => " Received an instance of Array".into(),
        Value::Object(_) | Value::ObjectAlias(_) => " Received an instance of Object".into(),
        _ => " Received an invalid value".into(),
    };
    crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"chunk\" argument must be of type string or an instance of Buffer, TypedArray, or DataView.{detail}"
    ))
}

/// `net.createServer([connectionListener])` — a server object backed by
/// an emitter; the listener, if given, registers for `'connection'`.
pub fn create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let Some(first) = args.first() {
        let valid = quench_runtime::is_callable(first)
            || matches!(first, Value::Object(_) | Value::ObjectAlias(_));
        if !valid {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ])));
        }
    }
    let (object, _id) = new_net_object(state, server_props())?;
    execute::set_property_in_place(
        &object,
        "_handle",
        host_api::object(vec![
            (
                "onconnection".into(),
                Value::Builtin(quench_runtime::ops::Builtin::Object),
            ),
            // Internal handles retain a callable close hook even though the
            // owning Server remains responsible for listener lifecycle.
            (
                "close".into(),
                crate::host::capability(crate::registry::SPEC_NET_SOCKET_HANDLE_CLOSE),
            ),
        ]),
    );
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if let Value::Boolean(value) = execute::get_property(options, "allowHalfOpen") {
            execute::set_property_in_place(&object, "allowHalfOpen", Value::Boolean(value));
        }
        if let Value::Boolean(value) = execute::get_property(options, "pauseOnConnect") {
            execute::set_property_in_place(&object, "pauseOnConnect", Value::Boolean(value));
        }
        if let Value::Boolean(value) = execute::get_property(options, "noDelay") {
            execute::set_property_in_place(&object, "noDelay", Value::Boolean(value));
        }
        if let Value::Boolean(value) = execute::get_property(options, "keepAlive") {
            execute::set_property_in_place(&object, "keepAlive", Value::Boolean(value));
        }
        if let Value::Boolean(value) = execute::get_property(options, "ipv6Only") {
            execute::set_property_in_place(&object, "ipv6Only", Value::Boolean(value));
        }
        if let Value::Number(value) = execute::get_property(options, "keepAliveInitialDelay") {
            execute::set_property_in_place(&object, "keepAliveInitialDelay", Value::Number(value));
        }
        let block_list = execute::get_property(options, "blockList");
        if matches!(block_list, Value::Object(_) | Value::ObjectAlias(_)) {
            execute::set_property_in_place(&object, "blockList", block_list);
        }
    }
    register_server(state, &object, None)?;
    let connection_listener = args
        .first()
        .filter(|value| quench_runtime::is_callable(value))
        .or_else(|| {
            args.get(1)
                .filter(|value| quench_runtime::is_callable(value))
        });
    add_listener_cb(state, &object, connection_listener, "connection", false)?;
    Ok(object)
}

/// `new internalBinding('pipe_wrap').Pipe(type)` is a server-owned bound
/// handle. Its fd is stable identity; binding and closing reuse the ordinary
/// net server path and lifecycle.
pub fn pipe_construct(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let object = create_server(state, &[])?;
    let fd = {
        let mut net = state.borrow_mut();
        let fd = net.net.next_pipe_fd;
        net.net.next_pipe_fd += 1;
        fd
    };
    install_methods(
        object,
        vec![
            (PIPE_MARKER_PROP.to_string(), Value::Boolean(true)),
            (PIPE_FD_PROP.to_string(), Value::Number(fd as f64)),
            ("fd".to_string(), Value::Number(fd as f64)),
            (
                "bind".to_string(),
                crate::host::capability(crate::registry::SPEC_NET_PIPE_BIND),
            ),
        ],
    )
}

pub fn pipe_bind(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = server_listen(state, receiver, args)?;
    Ok(Value::Number(0.0))
}

/// Construct an internal TCP listener handle consumed by `server.listen`.
/// Binding port zero eagerly yields a stable descriptor and listener while
/// preserving the ordinary bound-socket adoption path.
pub fn tcp_construct(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let options = host_api::object(vec![
        ("host".into(), Value::String("0.0.0.0".into())),
        ("port".into(), Value::Number(0.0)),
    ]);
    let object = bound_socket_construct(state, &[options])?;
    let object = install_methods(
        object,
        vec![(
            "bind".into(),
            crate::host::capability(crate::registry::SPEC_NET_TCP_BIND),
        )],
    )?;
    // Internal TCP handles expose a numeric fd field, unlike the public
    // BoundSocket accessor method.
    let fd = bound_socket_fd(state, Some(&object), &[])?;
    execute::set_property_in_place(&object, "fd", fd);
    Ok(object)
}

/// Complete the internal TCP handle bind call. The listener was allocated at
/// construction, so adoption only needs the success errno contract.
pub fn tcp_bind(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(0.0))
}

pub fn bound_socket_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = match args.first() {
        None | Some(Value::Undefined) => host_api::object(Vec::new()),
        Some(Value::Object(_) | Value::ObjectAlias(_)) => args[0].clone(),
        _ => {
            return Err(bound_arg_error(
                "options must be an object",
                "ERR_INVALID_ARG_TYPE",
            ))
        }
    };
    let path = execute::get_property(&options, "path");
    if !matches!(path, Value::Undefined) {
        let path = if matches!(path, Value::String(_) | Value::StringUnits(_)) {
            execute::to_js_string(&path)?
        } else {
            return Err(bound_arg_error(
                "path must be a string",
                "ERR_INVALID_ARG_TYPE",
            ));
        };
        for key in ["host", "port", "ipv6Only", "reusePort"] {
            if !matches!(execute::get_property(&options, key), Value::Undefined) {
                return Err(bound_arg_error(
                    "path cannot be combined with TCP options",
                    "ERR_INVALID_ARG_VALUE",
                ));
            }
        }
        if path.starts_with('\0') && !cfg!(target_os = "linux") {
            return Err(bound_arg_error(
                "abstract socket paths are Linux-only",
                "ERR_INVALID_ARG_VALUE",
            ));
        }
        if path.len() > 1023 {
            return Err(bound_bind_error("EINVAL"));
        }
        if Path::new(&path)
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty() && !parent.exists())
        {
            return Err(bound_bind_error("EACCES"));
        }
        if state.borrow().net.paths.contains_key(&path)
            || state
                .borrow()
                .net
                .bound_sockets
                .values()
                .any(|bound| bound.borrow().path.as_deref() == Some(path.as_str()))
        {
            return Err(bound_bind_error("EADDRINUSE"));
        }
        let listener =
            bind_listener(0, None).map_err(|error| bound_bind_error(bind_code(&error)))?;
        if !path.starts_with('\0') {
            create_pipe_placeholder(&path, Some(&options))
                .map_err(|error| bound_bind_error(bind_code(&error)))?;
        }
        let fd = next_bound_fd(state);
        let id = allocate_id(state);
        let object = bound_object(state, id, fd, Some(path.clone()), None)?;
        state.borrow_mut().net.bound_sockets.insert(
            id,
            Rc::new(RefCell::new(NetBoundSocket {
                listener: Some(listener),
                path: Some(path),
                address: None,
                fd,
                adopted: false,
            })),
        );
        return Ok(object);
    }
    let host = match execute::get_property(&options, "host") {
        Value::Undefined => {
            if matches!(
                execute::get_property(&options, "ipv6Only"),
                Value::Boolean(true)
            ) {
                "::".to_string()
            } else {
                "0.0.0.0".to_string()
            }
        }
        Value::String(host) => host,
        _ => {
            return Err(bound_arg_error(
                "host must be a string",
                "ERR_INVALID_ARG_TYPE",
            ))
        }
    };
    if host.parse::<std::net::IpAddr>().is_err() {
        return Err(bound_arg_error(
            "host must be an IP address",
            "ERR_INVALID_ARG_VALUE",
        ));
    }
    let port_value = execute::get_property(&options, "port");
    let port = if matches!(port_value, Value::Undefined) {
        0
    } else {
        parse_port(&port_value)?
    };
    if matches!(
        execute::get_property(&options, "reusePort"),
        Value::Boolean(true)
    ) {
        return Err(bound_bind_error("EADDRINUSE"));
    }
    if !cfg!(windows) && (1..1024).contains(&port) {
        return Err(bound_bind_error("EACCES"));
    }
    let listener =
        bind_listener(port, Some(&host)).map_err(|error| bound_bind_error(bind_code(&error)))?;
    let address = listener.local_addr().ok();
    let fd = address.map(|addr| i64::from(addr.port())).unwrap_or(0);
    let id = allocate_id(state);
    let object = bound_object(state, id, fd, None, address)?;
    state.borrow_mut().net.bound_sockets.insert(
        id,
        Rc::new(RefCell::new(NetBoundSocket {
            listener: Some(listener),
            path: None,
            address,
            fd,
            adopted: false,
        })),
    );
    Ok(object)
}

fn bound_object(
    state: &Rc<RefCell<HostState>>,
    id: u64,
    fd: i64,
    path: Option<String>,
    address: Option<SocketAddr>,
) -> Result<Value, VmError> {
    let object = host_api::object(vec![
        (BOUND_ID_PROP.into(), Value::Number(id as f64)),
        ("isPipe".into(), Value::Boolean(path.is_some())),
        (
            "address".into(),
            crate::host::capability(crate::registry::SPEC_NET_BOUND_SOCKET_ADDRESS),
        ),
        (
            "fd".into(),
            crate::host::capability(crate::registry::SPEC_NET_BOUND_SOCKET_FD),
        ),
        (
            "close".into(),
            crate::host::capability(crate::registry::SPEC_NET_BOUND_SOCKET_CLOSE),
        ),
    ]);
    if let Some(path) = path {
        execute::set_property_in_place(&object, "_path", Value::String(path));
    }
    if let Some(address) = address {
        execute::set_property_in_place(&object, "_address", address_value(address));
    }
    let _ = state;
    Ok(object)
}

fn bound_arg_error(message: &str, code: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("message".into(), Value::String(message.into())),
        ("code".into(), Value::String(code.into())),
    ]))
}

fn bound_bind_error(code: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        ("message".into(), Value::String(format!("{code}: bind"))),
        ("code".into(), Value::String(code.into())),
        ("syscall".into(), Value::String("bind".into())),
    ]))
}

fn next_bound_fd(state: &Rc<RefCell<HostState>>) -> i64 {
    let mut net = state.borrow_mut();
    let fd = net.net.next_pipe_fd;
    net.net.next_pipe_fd += 1;
    fd
}

fn bound_id(receiver: &Value) -> Option<u64> {
    match execute::get_property(receiver, BOUND_ID_PROP) {
        Value::Number(id) if id.is_finite() && id >= 0.0 => Some(id as u64),
        _ => None,
    }
}

fn bound_state(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
) -> Result<Rc<RefCell<NetBoundSocket>>, VmError> {
    let id = receiver
        .and_then(bound_id)
        .ok_or_else(|| execute::type_error("BoundSocket"))?;
    state
        .borrow()
        .net
        .bound_sockets
        .get(&id)
        .cloned()
        .ok_or_else(|| execute::type_error("BoundSocket"))
}

pub fn bound_socket_address(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let bound = bound_state(state, receiver)?;
    let bound = bound.borrow();
    if bound.adopted {
        return Err(bound_arg_error(
            "BoundSocket handle was adopted",
            "ERR_SOCKET_HANDLE_ADOPTED",
        ));
    }
    if bound.listener.is_none() {
        return Err(bound_arg_error(
            "BoundSocket is closed",
            "ERR_SOCKET_CLOSED",
        ));
    }
    Ok(bound
        .path
        .clone()
        .map(Value::String)
        .or_else(|| bound.address.map(address_value))
        .unwrap_or(Value::Undefined))
}

pub fn bound_socket_fd(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let bound = bound_state(state, receiver)?;
    let bound = bound.borrow();
    if bound.adopted {
        return Err(bound_arg_error(
            "BoundSocket handle was adopted",
            "ERR_SOCKET_HANDLE_ADOPTED",
        ));
    }
    if bound.listener.is_none() {
        return Err(bound_arg_error(
            "BoundSocket is closed",
            "ERR_SOCKET_CLOSED",
        ));
    }
    Ok(Value::Number(bound.fd as f64))
}

pub fn bound_socket_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| execute::type_error("BoundSocket"))?;
    let id = bound_id(receiver).ok_or_else(|| execute::type_error("BoundSocket"))?;
    let bound = state
        .borrow()
        .net
        .bound_sockets
        .get(&id)
        .cloned()
        .ok_or_else(|| execute::type_error("BoundSocket"))?;
    let mut bound = bound.borrow_mut();
    if bound.adopted {
        return Err(bound_arg_error(
            "BoundSocket handle was adopted",
            "ERR_SOCKET_HANDLE_ADOPTED",
        ));
    }
    if bound.listener.take().is_none() {
        return Err(bound_arg_error(
            "BoundSocket is closed",
            "ERR_SOCKET_CLOSED",
        ));
    }
    if let Some(path) = bound.path.take() {
        state.borrow_mut().net.paths.remove(&path);
        let _ = std::fs::remove_file(path);
    }
    Ok(receiver.clone())
}

/// `new net.Socket()` creates an unconnected socket whose `connect` method
/// shares the public connection capability and validation path.
pub fn register_fd_stream(state: &Rc<RefCell<HostState>>, fd: i64, stream: TcpStream) {
    state
        .borrow_mut()
        .net
        .fd_streams
        .entry(fd)
        .or_default()
        .push(stream);
}

pub fn socket_construct(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
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
    let fd_stream = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .and_then(|options| match execute::get_property(options, "fd") {
            Value::Number(fd) => Some(fd as i64),
            _ => None,
        })
        .and_then(|fd| {
            state
                .borrow_mut()
                .net
                .fd_streams
                .get_mut(&fd)
                .and_then(Vec::pop)
        });
    let fd_local = fd_stream
        .as_ref()
        .and_then(|stream| stream.local_addr().ok());
    let (object, _id) = new_net_object(state, socket_props())?;
    let global = quench_runtime::vm::current_global_object();
    let prototype = state
        .borrow()
        .net
        .socket_prototype
        .clone()
        .unwrap_or_else(|| execute::get_property(&global, "\0quench:net:socket-prototype"));
    let object = if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&object, &prototype)?
    } else {
        object
    };
    let object = install_socket_counters(object)?;
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if let Value::Number(value) = execute::get_property(options, "highWaterMark") {
            if value.is_finite() && value >= 0.0 {
                execute::set_property_in_place(
                    &object,
                    "writableHighWaterMark",
                    Value::Number(value),
                );
            }
        }
        if let Value::Boolean(value) = execute::get_property(options, "allowHalfOpen") {
            execute::set_property_in_place(&object, "allowHalfOpen", Value::Boolean(value));
        }
        if let Value::Boolean(value) = execute::get_property(options, "writable") {
            super::set_socket_property(&object, "writable", Value::Boolean(value));
        }
    }
    let object = install_methods(
        object,
        vec![(
            "connect".to_string(),
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        )],
    )?;
    // Node installs one socket-owned end listener. Keep the listener count
    // observable without inheriting stream.Duplex's no-half-open enforcer;
    // the host pump owns the actual allowHalfOpen transition.
    let _ = crate::modules::events::method_on(
        state,
        Some(&object),
        &[
            Value::String("end".into()),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ],
    )?;
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if let Value::Object(_) | Value::ObjectAlias(_) = execute::get_property(options, "handle") {
            if let Some(bound_id) = bound_id(&execute::get_property(options, "handle")) {
                execute::set_property_in_place(&object, BOUND_HANDLE_PROP, Value::Boolean(true));
                if let Some(bound) = state.borrow().net.bound_sockets.get(&bound_id).cloned() {
                    let mut bound = bound.borrow_mut();
                    let _ = bound.listener.take();
                    bound.adopted = true;
                    if let Some(path) = bound.path.clone() {
                        execute::set_property_in_place(
                            &object,
                            BOUND_LOCAL_ADDRESS_PROP,
                            Value::String(path),
                        );
                    } else if let Some(address) = bound.address {
                        execute::set_property_in_place(
                            &object,
                            BOUND_LOCAL_ADDRESS_PROP,
                            Value::String(address.ip().to_string()),
                        );
                        execute::set_property_in_place(
                            &object,
                            BOUND_LOCAL_PORT_PROP,
                            Value::Number(address.port() as f64),
                        );
                    }
                }
            }
        }
        if let Value::Number(fd) = execute::get_property(options, "fd") {
            execute::set_property_in_place(&object, PIPE_FD_PROP, Value::Number(fd));
        }
    }
    let object = execute::canonical_value(&object);
    let supplied_handle = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .map(|options| execute::get_property(options, "handle"))
        .filter(|handle| {
            matches!(handle, Value::Object(_) | Value::ObjectAlias(_))
                && quench_runtime::is_callable(&execute::get_property(handle, "readStart"))
        });
    if let Some(handle) = supplied_handle {
        execute::set_property_in_place(&object, "_handle", handle.clone());
        let onread = host_api::bound_capability_with_arguments(
            crate::host::capability_ref(crate::registry::SPEC_NET_SOCKET_ONREAD),
            vec![object.clone()],
        );
        execute::set_property_in_place(&handle, "onread", onread);
        let read_start = execute::get_property(&handle, "readStart");
        execute::call(&read_start, &handle, &[])?;
    }
    let process_scope = state.borrow().cluster.process_scope();
    let owner_worker = state.borrow().cluster.worker_context;
    state.borrow_mut().net.sockets.insert(
        _id,
        Rc::new(RefCell::new(NetSocket {
            id: _id,
            process_scope,
            owner_worker,
            stream: fd_stream,
            js: object.clone(),
            state: SocketState::Open,
            refed: true,
            server_id: None,
            write_buf: Vec::new(),
            write_offset: 0,
            read_buf: Vec::new(),
            bytes_read: 0,
            bytes_written: 0,
            read_eof: false,
            close_emitted: false,
            close_deferred: false,
            write_shutdown_pending: false,
            finish_emitted: false,
            connect_announced: fd_local.is_some(),
            peer: None,
            local: fd_local,
            encoding: None,
            decode_buf: Vec::new(),
        })),
    );
    if fd_local.is_some() {
        set_socket_state(&object, false, false, "open");
        let handle = host_api::object(vec![
            ("fd".into(), Value::Number(4.0)),
            (
                "close".into(),
                crate::host::capability(crate::registry::SPEC_NET_SOCKET_HANDLE_CLOSE),
            ),
        ]);
        execute::set_property_in_place(&object, "_handle", handle);
    }
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let signal = execute::get_property(options, "signal");
        if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
            if matches!(
                execute::get_property(&signal, "aborted"),
                Value::Boolean(true)
            ) {
                state.borrow_mut().net.pending_events.push((
                    object.clone(),
                    "error".into(),
                    vec![abort_error()],
                ));
                socket_destroy(state, Some(&object), &[])?;
            } else {
                let listener = host_api::bound_capability_with_arguments(
                    crate::host::capability_ref(crate::registry::SPEC_NET_SOCKET_ABORT),
                    vec![object.clone()],
                );
                let listener_options =
                    host_api::object(vec![("once".into(), Value::Boolean(true))]);
                crate::modules::event_target::add_event_listener(
                    state,
                    Some(&signal),
                    &[Value::String("abort".into()), listener, listener_options],
                )?;
            }
        }
    }
    Ok(object)
}

fn validate_connect_options(options: &Value) -> Result<(), VmError> {
    let local_address = execute::get_property(options, "localAddress");
    if !matches!(local_address, Value::Undefined | Value::Null) {
        let valid = match &local_address {
            Value::String(_) | Value::StringUnits(_) => execute::to_js_string(&local_address)
                .ok()
                .is_some_and(|value| value.parse::<std::net::IpAddr>().is_ok()),
            _ => false,
        };
        if !valid {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "code".into(),
                    Value::String("ERR_INVALID_IP_ADDRESS".into()),
                ),
            ])));
        }
    }
    let local_port = execute::get_property(options, "localPort");
    if !matches!(
        local_port,
        Value::Undefined | Value::Null | Value::Number(_)
    ) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        ])));
    }
    for key in ["objectMode", "readableObjectMode", "writableObjectMode"] {
        let value = execute::get_property(options, key);
        if execute::is_truthy(&value) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                (
                    "message".into(),
                    Value::String(format!(
                        "The property 'options.{key}' is not supported. Received {}",
                        match value {
                            Value::Boolean(value) => value.to_string(),
                            Value::Number(value) => value.to_string(),
                            _ => format!("{value:?}"),
                        }
                    )),
                ),
            ])));
        }
    }
    let host = execute::get_property(options, "host");
    if !matches!(
        host,
        Value::Undefined | Value::Null | Value::String(_) | Value::StringUnits(_)
    ) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        ])));
    }
    let host_text = match &host {
        Value::String(host) => Some(host.clone()),
        Value::StringUnits(units) => Some(String::from_utf16_lossy(units)),
        _ => None,
    };
    if host_text.is_some_and(|host| host.contains('\0')) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String(
                    "The property 'options.host' must be a string without null bytes.".into(),
                ),
            ),
        ])));
    }
    Ok(())
}

/// `net.connect(port[, host][, cb])` / `net.connect(options, cb)`.
/// Connects (bounded) on loopback and returns a socket object;
/// `'connect'` fires on the next pump tick.
pub fn connect(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    connect_with_receiver(state, None, args)
}

pub fn connect_path(state: &Rc<RefCell<HostState>>, path: &str) -> Result<Value, VmError> {
    let port = state.borrow().net.paths.get(path).copied();
    match port {
        Some(port) => {
            if let Some(code) = path_connect_error_code(path) {
                return path_connect_error(state, path, code);
            }
            connect(
                state,
                &[
                    Value::Number(port as f64),
                    Value::String("127.0.0.1".into()),
                ],
            )
        }
        None => {
            let code = path_connect_error_code(path).unwrap_or_else(|| {
                if Path::new(path).exists() {
                    "ENOTSOCK"
                } else {
                    "ENOENT"
                }
            });
            path_connect_error(state, path, code)
        }
    }
}

fn path_connect_error_code(path: &str) -> Option<&'static str> {
    #[cfg(unix)]
    {
        if path.len() > 108 {
            return Some("EINVAL");
        }
        if std::fs::metadata(path).ok().is_some_and(|meta| {
            use std::os::unix::fs::MetadataExt;
            meta.mode() & 0o777 == 0
        }) {
            return Some("EACCES");
        }
    }
    None
}

fn path_connect_error(
    state: &Rc<RefCell<HostState>>,
    path: &str,
    code: &str,
) -> Result<Value, VmError> {
    let (object, _) = new_net_object(state, socket_props())?;
    let error = host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "message".into(),
            Value::String(format!("connect {code} {path}")),
        ),
        ("code".into(), Value::String(code.into())),
        ("syscall".into(), Value::String("connect".into())),
    ]);
    state
        .borrow_mut()
        .net
        .pending_errors
        .push((object.clone(), error));
    Ok(object)
}

fn path_connect_error_for_receiver(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    path: &str,
    code: &str,
) -> Result<Value, VmError> {
    let error = host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "message".into(),
            Value::String(format!("connect {code} {path}")),
        ),
        ("code".into(), Value::String(code.into())),
        ("syscall".into(), Value::String("connect".into())),
    ]);
    state
        .borrow_mut()
        .net
        .pending_errors
        .push((receiver.clone(), error));
    Ok(receiver.clone())
}

fn connect_path_for_receiver(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    path: &str,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(code) = path_connect_error_code(path) {
        if let Some(receiver) = receiver {
            return path_connect_error_for_receiver(state, receiver, path, code);
        }
        return path_connect_error(state, path, code);
    }
    let Some(port) = state.borrow().net.paths.get(path).copied() else {
        let code = if Path::new(path).exists() {
            "ENOTSOCK"
        } else {
            "ENOENT"
        };
        if let Some(receiver) = receiver {
            return path_connect_error_for_receiver(state, receiver, path, code);
        }
        return connect_path(state, path);
    };
    let mut properties = vec![
        ("port".into(), Value::Number(port as f64)),
        ("host".into(), Value::String(LOCAL_HOST.into())),
    ];
    if let Some(Value::Object(_) | Value::ObjectAlias(_)) = args.first() {
        if let Value::Number(fd) = execute::get_property(args.first().unwrap(), "fd") {
            properties.push(("fd".into(), Value::Number(fd)));
        }
    }
    let options = host_api::object(properties);
    let mut connect_args = vec![options];
    if args.last().is_some_and(quench_runtime::is_callable) {
        connect_args.push(args.last().cloned().unwrap_or(Value::Undefined));
    }
    let result = match receiver {
        Some(receiver) => connect_with_receiver(state, Some(receiver), &connect_args),
        None => connect(state, &connect_args),
    }?;
    // Pipe transports use the same bounded TCP emulation internally, but do
    // not produce `net` TCP performance entries.
    execute::set_property_in_place(&result, PIPE_MARKER_PROP, Value::Boolean(true));
    Ok(result)
}

pub fn connect_existing(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    connect_with_receiver(state, Some(receiver), args)
}

/// Resume a custom lookup that completed from a later event-loop turn.
/// Lookup completion is data-driven: the saved socket/options are consumed,
/// validated once, and then fed back into the ordinary connect path.
pub fn complete_lookup(state: &Rc<RefCell<HostState>>, result: Value) -> Result<Value, VmError> {
    let pending = state.borrow_mut().net.pending_lookups.remove(0);
    let error = execute::get_property(&result, "0");
    if !matches!(error, Value::Undefined | Value::Null) {
        state
            .borrow_mut()
            .net
            .pending_errors
            .push((pending.socket.clone(), error));
        return Ok(pending.socket);
    }
    let Some(address) = custom_lookup_address(&pending.options, &result)
        .ok()
        .flatten()
    else {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "Invalid IP address: lookup returned an invalid address".into(),
            )],
        );
        let error = execute::set_property(
            error,
            "code",
            Value::String("ERR_INVALID_IP_ADDRESS".into()),
        );
        state
            .borrow_mut()
            .net
            .pending_errors
            .push((pending.socket.clone(), error));
        return Ok(pending.socket);
    };
    let addresses = lookup_addresses_for(&result);
    if matches!(
        execute::get_property(&pending.options, "all"),
        Value::Boolean(true)
    ) && !addresses.is_empty()
    {
        let attempted = host_api::array(
            addresses
                .iter()
                .map(|address| {
                    Value::String(format!(
                        "{address}:{}",
                        execute::to_js_string(&execute::get_property(&pending.options, "port"))
                            .unwrap_or_default()
                    ))
                })
                .collect(),
        );
        execute::set_property_in_place(
            &pending.socket,
            "autoSelectFamilyAttemptedAddresses",
            attempted,
        );
    }
    let family = lookup_family(&result).map(|value| Value::Number(value as f64));
    state.borrow_mut().net.pending_events.push((
        pending.socket.clone(),
        "lookup".into(),
        vec![
            Value::Null,
            Value::String(address.clone()),
            family.unwrap_or(Value::Undefined),
        ],
    ));
    let mut options = execute::set_property(
        execute::set_property(pending.options, "lookup", Value::Undefined),
        "host",
        Value::String(address),
    );
    if addresses.len() > 1 {
        options = execute::set_property(
            options,
            LOOKUP_ADDRESSES_PROP,
            host_api::array(addresses.into_iter().map(Value::String).collect()),
        );
    }
    let mut args = pending.args;
    if let Some(first) = args.first_mut() {
        *first = options;
    }
    connect_with_receiver(state, Some(&pending.socket), &args)
}

fn connect_with_receiver(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Capture the bridge while the initiating VM frame is active.  Later
    // delivery runs from the host pump, where `current_global_object()` is
    // intentionally unavailable.
    if state.borrow().net.performance_record.is_none() {
        let record = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__nodePerformanceRecord",
        );
        if quench_runtime::is_callable(&record) {
            state.borrow_mut().net.performance_record = Some(record);
        }
    }
    let mut lookup_socket_for_result: Option<Value> = None;
    let mut lookup_addresses: Option<Vec<String>> = None;
    if let Some(receiver) = receiver {
        let local_address = execute::get_property(receiver, BOUND_LOCAL_ADDRESS_PROP);
        if !matches!(local_address, Value::Undefined) {
            execute::set_property_in_place(receiver, "localAddress", local_address);
        }
        let local_port = execute::get_property(receiver, BOUND_LOCAL_PORT_PROP);
        if !matches!(local_port, Value::Undefined) {
            execute::set_property_in_place(receiver, "localPort", local_port);
        }
    }
    let mut lookup_address = None;
    if let Some(path) = args
        .first()
        .filter(|value| matches!(value, Value::String(_) | Value::StringUnits(_)))
    {
        let path = execute::to_js_string(path)?;
        if path.parse::<u16>().is_err() {
            return connect_path_for_receiver(state, receiver, &path, args);
        }
    }
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let saved_addresses = execute::get_property(options, LOOKUP_ADDRESSES_PROP);
        if let Value::Array(_) = saved_addresses {
            let length = match execute::get_property(&saved_addresses, "length") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
                _ => 0,
            };
            let addresses = (0..length)
                .filter_map(|index| {
                    match execute::get_property(&saved_addresses, &index.to_string()) {
                        Value::String(address) => Some(address),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            if !addresses.is_empty() {
                lookup_addresses = Some(addresses);
            }
        }
        validate_connect_options(options)?;
        if receiver.is_some()
            && matches!(
                execute::get_property(receiver.unwrap(), BOUND_HANDLE_PROP),
                Value::Boolean(true)
            )
            && (!matches!(
                execute::get_property(options, "localPort"),
                Value::Undefined
            ) || !matches!(
                execute::get_property(options, "localAddress"),
                Value::Undefined
            ))
        {
            return Err(bound_arg_error(
                "localAddress/localPort cannot be used with an adopted handle",
                "ERR_INVALID_ARG_VALUE",
            ));
        }
        if let Some(path) = string_property(options, "socketPath") {
            return connect_path_for_receiver(state, receiver, &path, args);
        }
        if let Some(path) = string_property(options, "port") {
            if path.starts_with('/') || state.borrow().net.paths.contains_key(&path) {
                return connect_path_for_receiver(state, receiver, &path, args);
            }
        }
        if matches!(execute::get_property(options, "port"), Value::Undefined) {
            if let Some(path) = string_property(options, "path") {
                if path.parse::<u16>().is_err() {
                    return connect_path_for_receiver(state, receiver, &path, args);
                }
            }
        }
        let auto_select_family = execute::get_property(options, "autoSelectFamily");
        if !matches!(auto_select_family, Value::Undefined | Value::Boolean(_)) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ])));
        }
        if let Value::Number(value) =
            execute::get_property(options, "autoSelectFamilyAttemptTimeout")
        {
            if !value.is_finite() || value.fract() != 0.0 || value <= 0.0 {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("RangeError".into())),
                    ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                ])));
            }
        }
        let auto_select_family = match auto_select_family {
            Value::Boolean(value) => value,
            Value::Undefined => state.borrow().net.auto_select_family,
            _ => false,
        };
        let lookup = execute::get_property(options, "lookup");
        if !matches!(lookup, Value::Undefined) && !quench_runtime::is_callable(&lookup) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ])));
        }
        if quench_runtime::is_callable(&lookup) {
            state.borrow_mut().net.lookup_result = None;
            let lookup_socket = if let Some(receiver) = receiver {
                receiver.clone()
            } else {
                let (object, _) = new_net_object(state, socket_props())?;
                install_socket_counters(object)?
            };
            lookup_socket_for_result = Some(lookup_socket.clone());
            let callback = crate::host::capability(crate::registry::SPEC_NET_LOOKUP_CALLBACK);
            let lookup_options = if auto_select_family {
                execute::set_property(options.clone(), "all", Value::Boolean(true))
            } else {
                options.clone()
            };
            state
                .borrow_mut()
                .net
                .pending_lookups
                .push(super::PendingLookup {
                    socket: lookup_socket.clone(),
                    options: lookup_options.clone(),
                    args: args.to_vec(),
                });
            state.borrow_mut().net.lookup_in_call = true;
            let result = match execute::call(
                &lookup,
                &Value::Undefined,
                &[
                    execute::get_property(options, "host"),
                    lookup_options.clone(),
                    callback,
                ],
            ) {
                Ok(result) => result,
                Err(error) => {
                    let mut host = state.borrow_mut();
                    host.net.lookup_in_call = false;
                    let _ = host.net.pending_lookups.pop();
                    return Err(error);
                }
            };
            state.borrow_mut().net.lookup_in_call = false;
            let callback_result = state.borrow_mut().net.lookup_result.take();
            if callback_result.is_none() {
                return Ok(lookup_socket);
            }
            let _ = state.borrow_mut().net.pending_lookups.pop();
            let result = callback_result.unwrap_or(result);
            if !matches!(
                execute::get_property(&result, "0"),
                Value::Undefined | Value::Null
            ) {
                let error = execute::get_property(&result, "0");
                state
                    .borrow_mut()
                    .net
                    .pending_errors
                    .push((lookup_socket.clone(), error));
                return Ok(lookup_socket);
            }
            // A custom lookup's result shape is part of the API contract:
            // `all: false` returns one address string, while `all: true`
            // returns an array of `{ address, family }` records. Reject any
            // other shape asynchronously on the socket, before attempting
            // a connection with an undefined host.
            if custom_lookup_address(&lookup_options, &result)
                .ok()
                .flatten()
                .is_none()
            {
                let (object, _) = new_net_object(state, socket_props())?;
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String(
                        "Invalid IP address: lookup returned an invalid address".into(),
                    )],
                );
                let error = execute::set_property(
                    error,
                    "code",
                    Value::String("ERR_INVALID_IP_ADDRESS".into()),
                );
                state
                    .borrow_mut()
                    .net
                    .pending_errors
                    .push((object.clone(), error));
                return Ok(object);
            }
            let addresses = lookup_addresses_for(&result);
            if matches!(
                execute::get_property(&lookup_options, "all"),
                Value::Boolean(true)
            ) {
                if !addresses.is_empty() {
                    let attempted = host_api::array(
                        addresses
                            .iter()
                            .map(|address| {
                                Value::String(format!(
                                    "{address}:{}",
                                    execute::to_js_string(&execute::get_property(options, "port"))
                                        .unwrap_or_default()
                                ))
                            })
                            .collect(),
                    );
                    execute::set_property_in_place(
                        &lookup_socket,
                        "autoSelectFamilyAttemptedAddresses",
                        attempted,
                    );
                    lookup_addresses = Some(addresses);
                }
            }
            lookup_address = lookup_address_for(&result);
            if let Some(family) = lookup_family(&result) {
                if family != 4 && family != 6 {
                    let host = execute::to_js_string(&execute::get_property(options, "host"))
                        .unwrap_or_else(|_| LOCAL_HOST.into());
                    let port = execute::to_js_string(&execute::get_property(options, "port"))
                        .unwrap_or_else(|_| "0".into());
                    let (object, _) = new_net_object(state, socket_props())?;
                    let error = quench_runtime::builtins::error(
                        quench_runtime::ops::Builtin::Error,
                        &[Value::String(format!(
                            "Invalid address family: {family} {host}:{port}"
                        ))],
                    );
                    let error = execute::set_property(
                        error,
                        "code",
                        Value::String("ERR_INVALID_ADDRESS_FAMILY".into()),
                    );
                    let error = execute::set_property(error, "host", Value::String(host));
                    let port_value = port.parse::<f64>().unwrap_or(0.0);
                    let error = execute::set_property(error, "port", Value::Number(port_value));
                    state
                        .borrow_mut()
                        .net
                        .pending_errors
                        .push((object.clone(), error));
                    return Ok(object);
                }
            }
        }
        let hints = execute::get_property(options, "hints");
        if let Value::Number(value) = hints {
            let bits = value as i64;
            if value.fract() != 0.0 || bits < 0 || bits & !7 != 0 {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("TypeError".into())),
                    ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                    (
                        "message".into(),
                        Value::String(format!("The argument 'hints' is invalid. Received {value}")),
                    ),
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
        if let Value::String(path) = path {
            let port = execute::get_property(options, "port");
            if (matches!(port, Value::Undefined | Value::Null)
                && (path.starts_with('/') || state.borrow().net.paths.contains_key(&path)))
            {
                return connect_path(state, &path);
            }
        }
    }
    let (port, host) = connect_target(state, args)?;
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
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
    let target_host = lookup_address
        .as_deref()
        .or(host.as_deref())
        .unwrap_or(LOCAL_HOST);
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if let Value::Number(local_port) = execute::get_property(options, "localPort") {
            let local_port = local_port as u16;
            let local_address = string_property(options, "localAddress");
            let conflict = state.borrow().net.servers.values().any(|server| {
                let server = server.borrow();
                server.bind_addr.is_some_and(|address| {
                    address.port() == local_port
                        && local_address
                            .as_deref()
                            .is_none_or(|host| host == address.ip().to_string())
                })
            });
            if conflict {
                let object = match receiver.or(lookup_socket_for_result.as_ref()) {
                    Some(object) => object.clone(),
                    None => {
                        let (object, _) = new_net_object(state, socket_props())?;
                        install_socket_counters(object)?
                    }
                };
                let error = host_api::object(vec![
                    ("name".into(), Value::String("Error".into())),
                    ("message".into(), Value::String("bind EADDRINUSE".into())),
                    ("code".into(), Value::String("EADDRINUSE".into())),
                    ("syscall".into(), Value::String("bind".into())),
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
    if port == 0 {
        let loopback = SocketAddr::new(LOCAL_HOST.parse().expect("loopback"), 0);
        return connect_refused(
            state,
            receiver.or(lookup_socket_for_result.as_ref()),
            &loopback,
        );
    }
    let candidate_addrs = lookup_addresses
        .unwrap_or_else(|| vec![target_host.to_string()])
        .into_iter()
        .filter_map(|host| super::resolve_connect(&host, port))
        .collect::<Vec<_>>();
    let Some(first_addr) = candidate_addrs.first().copied() else {
        let (object, _) = new_net_object(state, socket_props())?;
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(format!(
                "getaddrinfo ENOTFOUND {target_host}"
            ))],
        );
        let error = execute::set_property(error, "code", Value::String("ENOTFOUND".into()));
        state.borrow_mut().net.pending_events.push((
            object.clone(),
            "lookup".into(),
            vec![error.clone(), Value::Undefined, Value::Undefined],
        ));
        state
            .borrow_mut()
            .net
            .pending_errors
            .push((object.clone(), error));
        return Ok(object);
    };
    let ipv6_only_reject = first_addr.is_ipv4()
        && state.borrow().net.servers.values().any(|server| {
            let server = server.borrow();
            server.bind_addr.is_some_and(|address| {
                address.port() == port
                    && address.is_ipv6()
                    && matches!(
                        execute::get_property(&server.js, "ipv6Only"),
                        Value::Boolean(true)
                    )
            })
        });
    if ipv6_only_reject {
        return connect_refused(
            state,
            receiver.or(lookup_socket_for_result.as_ref()),
            &first_addr,
        );
    }
    let mut selected = None;
    for addr in candidate_addrs {
        if let Ok(stream) =
            TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(3000))
        {
            selected = Some((stream, addr));
            break;
        }
    }
    let Some((stream, addr)) = selected else {
        return connect_refused(
            state,
            receiver.or(lookup_socket_for_result.as_ref()),
            &first_addr,
        );
    };
    let _ = stream.set_nonblocking(true);
    let object_receiver = receiver.or(lookup_socket_for_result.as_ref());
    // A reconnect replaces only the transport.  Node keeps stream options
    // configured on the public Socket (notably setEncoding) across that
    // replacement, so carry the fact from the retired host entry forward.
    let existing_encoding = object_receiver
        .and_then(net_id)
        .and_then(|id| {
            state
                .borrow()
                .net
                .sockets
                .get(&id)
                .and_then(|socket| socket.borrow().encoding.clone())
        })
        .or_else(|| match object_receiver {
            Some(object) => match execute::get_property(object, SOCKET_ENCODING_PROP) {
                Value::String(value) => Some(value),
                _ => None,
            },
            None => None,
        });
    let (object, id) = match object_receiver {
        Some(object) => (
            object.clone(),
            net_id(object).unwrap_or_else(|| allocate_id(state)),
        ),
        None => {
            let (object, id) = new_net_object(state, socket_props())?;
            (install_socket_counters(object)?, id)
        }
    };
    if let Some(prototype) = state.borrow().net.socket_prototype.clone() {
        let updated = execute::set_prototype_of(&object, &prototype)?;
        execute::replace_value(&object, &updated);
    }
    let object = execute::canonical_value(&object);
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if let Value::Number(fd) = execute::get_property(options, "fd") {
            execute::set_property_in_place(&object, PIPE_FD_PROP, Value::Number(fd));
        }
        configure_onread(&object, options);
        let no_delay = execute::get_property(options, "noDelay");
        if execute::is_truthy(&no_delay) {
            execute::set_property_in_place(&object, super::NO_DELAY_PROP, Value::Boolean(true));
        }
        if let Value::Boolean(value) = execute::get_property(options, "allowHalfOpen") {
            execute::set_property_in_place(&object, "allowHalfOpen", Value::Boolean(value));
        }
    }
    let handle = host_api::object(vec![
        (
            "setNoDelay".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "setKeepAlive".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "close".into(),
            crate::host::capability(crate::registry::SPEC_NET_SOCKET_HANDLE_CLOSE),
        ),
    ]);
    if let Value::Number(fd) = execute::get_property(&object, PIPE_FD_PROP) {
        execute::set_property_in_place(&handle, "fd", Value::Number(fd));
    }
    execute::set_property_in_place(&object, "_handle", handle);
    let local = stream.local_addr().ok();
    set_socket_state(&object, true, true, "opening");
    let socket = Rc::new(std::cell::RefCell::new(NetSocket {
        id,
        process_scope: state.borrow().cluster.process_scope(),
        owner_worker: state.borrow().cluster.worker_context,
        stream: Some(stream),
        js: object.clone(),
        state: SocketState::Open,
        refed: true,
        server_id: None,
        write_buf: Vec::new(),
        write_offset: 0,
        read_buf: Vec::new(),
        bytes_read: 0,
        bytes_written: 0,
        read_eof: false,
        close_emitted: false,
        close_deferred: false,
        write_shutdown_pending: false,
        finish_emitted: false,
        connect_announced: false,
        peer: Some(addr),
        local,
        encoding: existing_encoding,
        decode_buf: Vec::new(),
    }));
    // Preserve transport negotiation facts before registering callbacks: the
    // first EventEmitter mutation may publish a copy-on-write representative,
    // so connection metadata must already be present on the source object.
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let alpn = execute::get_property(options, "ALPNProtocols");
        if !matches!(alpn, Value::Undefined) {
            execute::set_property_in_place(&object, crate::modules::tls::TLS_ALPN_PROP, alpn);
        }
    }
    let queued_write = state
        .borrow_mut()
        .net
        .pending_connect_writes
        .remove(&id)
        .unwrap_or_default();
    if !queued_write.is_empty() {
        let mut socket = socket.borrow_mut();
        socket.bytes_written = queued_write.len() as u64;
        socket.write_buf = queued_write;
    }
    state.borrow_mut().net.sockets.insert(id, socket);
    // Node publishes every client socket at connection creation; retain the
    // same socket identity that the net registry returns (TLS aliases this
    // object through its TLSSocket prototype).
    let channel = crate::modules::diagnostics_channel::channel(
        state,
        None,
        &[Value::String("net.client.socket".into())],
    )?;
    let message = host_api::object(vec![("socket".into(), object.clone())]);
    crate::modules::diagnostics_channel::publish(state, Some(&channel), &[message])?;
    add_listener_cb(state, &object, args.last(), "connect", true)?;
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let signal = execute::get_property(options, "signal");
        if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
            if matches!(
                execute::get_property(&signal, "aborted"),
                Value::Boolean(true)
            ) {
                state.borrow_mut().net.pending_events.push((
                    object.clone(),
                    "error".into(),
                    vec![abort_error()],
                ));
            } else {
                let listener = host_api::bound_capability_with_arguments(
                    crate::host::capability_ref(crate::registry::SPEC_NET_SOCKET_ABORT),
                    vec![object.clone()],
                );
                let listener_options =
                    host_api::object(vec![("once".into(), Value::Boolean(true))]);
                crate::modules::event_target::add_event_listener(
                    state,
                    Some(&signal),
                    &[Value::String("abort".into()), listener, listener_options],
                )?;
            }
        }
    }
    Ok(object)
}

/// AbortSignal callback for a net socket. The socket is bound as the first
/// argument so the signal remains the event receiver and listener identity.
pub fn socket_abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket) = args.first() else {
        return Ok(Value::Undefined);
    };
    if let Some(id) = net_id(socket) {
        state
            .borrow_mut()
            .net
            .pending_lookups
            .retain(|pending| net_id(&pending.socket) != Some(id));
        state.borrow_mut().net.pending_connect_writes.remove(&id);
    }
    // `AbortSignal` dispatch is synchronous, but net errors are delivered on
    // the next loop turn so listeners attached immediately after `abort()`
    // still observe the failure (the Node contract used by Agent tests).
    state.borrow_mut().net.pending_events.push((
        socket.clone(),
        "error".into(),
        vec![abort_error()],
    ));
    socket_destroy(state, Some(socket), &[])
}

/// `socket.resetAndDestroy()` sends a reset for a live socket. An unconnected
/// socket reports `ERR_SOCKET_CLOSED` asynchronously, after listeners bind.
pub fn socket_reset_and_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let live = net_id(receiver)
        .and_then(|id| state.borrow().net.sockets.get(&id).cloned())
        .is_some_and(|socket| socket.borrow().state != SocketState::Closed);
    if live {
        socket_destroy(state, Some(receiver), &[])?;
    } else {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("Socket is closed".into())],
        );
        let error = execute::set_property(error, "code", Value::String("ERR_SOCKET_CLOSED".into()));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((receiver.clone(), "error".into(), vec![error]));
    }
    Ok(receiver.clone())
}

/// Callback installed on a user-supplied native handle. The handle signals
/// EOF through `onread`; the ordinary pump then delivers end and close.
pub fn socket_onread(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket) = args.first() else {
        return Ok(Value::Undefined);
    };
    let Some(id) = net_id(socket) else {
        return Ok(Value::Undefined);
    };
    let Some(entry) = state.borrow().net.sockets.get(&id).cloned() else {
        return Ok(Value::Undefined);
    };
    let mut guard = entry.borrow_mut();
    if guard.read_eof {
        return Ok(Value::Undefined);
    }
    guard.read_eof = true;
    guard.state = SocketState::Closing;
    drop(guard);
    state
        .borrow_mut()
        .net
        .pending_events
        .push((socket.clone(), "end".into(), Vec::new()));
    Ok(Value::Undefined)
}

fn abort_error() -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("The operation was aborted".into())],
    );
    let error = execute::set_property(error, "name", Value::String("AbortError".into()));
    execute::set_property(error, "code", Value::String("ABORT_ERR".into()))
}

fn configure_onread(socket: &Value, options: &Value) {
    let onread = execute::get_property(options, "onread");
    if !matches!(onread, Value::Object(_) | Value::ObjectAlias(_)) {
        return;
    }
    let buffer = execute::get_property(&onread, "buffer");
    let callback = execute::get_property(&onread, "callback");
    if (matches!(buffer, Value::Uint8Array(_) | Value::DataView(_))
        || quench_runtime::is_callable(&buffer))
        && quench_runtime::is_callable(&callback)
    {
        execute::set_property_in_place(socket, ONREAD_BUFFER_PROP, buffer);
        execute::set_property_in_place(socket, ONREAD_CALLBACK_PROP, callback);
    }
}

/// A refused/absent loopback peer surfaces as an `'error'` on a
/// destroyed socket (never a synchronous throw).
fn connect_refused(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    addr: &SocketAddr,
) -> Result<Value, VmError> {
    let object = if let Some(receiver) = receiver {
        receiver.clone()
    } else {
        let (object, _) = new_net_object(state, socket_props())?;
        object
    };
    // A refused connection is terminal for an already-constructed socket.
    // Mark its connect announcement consumed before the next pump tick so a
    // socket.connect(...).on('connect', cb) cannot report a false success
    // alongside the queued error. The error remains deferred until listeners
    // attached by the caller have been installed.
    if let Some(id) = net_id(&object) {
        if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
            let mut socket = socket.borrow_mut();
            socket.connect_announced = true;
            socket.state = SocketState::Closed;
        }
    }
    let attempted = receiver.and_then(|socket| {
        let value = execute::get_property(socket, "autoSelectFamilyAttemptedAddresses");
        let length = match execute::get_property(&value, "length") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
            _ => 0,
        };
        (length > 1).then(|| {
            (0..length)
                .filter_map(
                    |index| match execute::get_property(&value, &index.to_string()) {
                        Value::String(address) => Some(address),
                        _ => None,
                    },
                )
                .collect::<Vec<_>>()
        })
    });
    let error = if let Some(addresses) = attempted {
        let errors = host_api::array(
            addresses
                .into_iter()
                .map(|address| {
                    let error = quench_runtime::builtins::error(
                        quench_runtime::ops::Builtin::Error,
                        &[Value::String(format!("connect ECONNREFUSED {address}"))],
                    );
                    let error =
                        execute::set_property(error, "code", Value::String("ECONNREFUSED".into()));
                    execute::set_property(error, "syscall", Value::String("connect".into()))
                })
                .collect(),
        );
        let global = quench_runtime::vm::current_global_object();
        let constructor = execute::get_property(&global, "AggregateError");
        execute::construct_value(
            &constructor,
            &[
                errors,
                Value::String("All connection attempts failed".into()),
            ],
        )
        .unwrap_or_else(|_| {
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!(
                    "connect ECONNREFUSED {}:{}",
                    addr.ip(),
                    addr.port()
                ))],
            )
        })
    } else {
        let message = format!("connect ECONNREFUSED {}:{}", addr.ip(), addr.port());
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(message)],
        );
        let error = execute::set_property(error, "code", Value::String("ECONNREFUSED".into()));
        let error = execute::set_property(error, "errno", Value::Number(-61.0));
        execute::set_property(error, "syscall", Value::String("connect".into()))
    };
    state
        .borrow_mut()
        .net
        .pending_errors
        .push((object.clone(), error));
    Ok(object)
}

fn connect_target(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<(u16, Option<String>), VmError> {
    match args.first() {
        Some(Value::Object(_) | Value::ObjectAlias(_)) => {
            let options = args.first().cloned().unwrap_or(Value::Undefined);
            let port_value = execute::get_property_result(&options, "port")?;
            if matches!(port_value, Value::Undefined) {
                return Err(missing_connect_args());
            }
            let port = parse_port(&port_value)?;
            let host = execute::get_property_result(&options, "host")
                .ok()
                .filter(|value| !matches!(value, Value::Undefined | Value::Null))
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

fn lookup_family(result: &Value) -> Option<i64> {
    if let Value::Number(value) = execute::get_property(result, "2") {
        if value.is_finite() && value.fract() == 0.0 {
            return Some(value as i64);
        }
    }
    let addresses = execute::get_property(result, "1");
    let first = execute::get_property(&addresses, "0");
    match execute::get_property(&first, "family") {
        Value::Number(value) if value.is_finite() && value.fract() == 0.0 => Some(value as i64),
        _ => None,
    }
}

fn custom_lookup_address(options: &Value, result: &Value) -> Result<Option<String>, ()> {
    let addresses = execute::get_property(result, "1");
    let all = matches!(execute::get_property(options, "all"), Value::Boolean(true));
    if all {
        let Value::Array(_) = &addresses else {
            return Err(());
        };
        let length = match execute::get_property(&addresses, "length") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
            _ => 0,
        };
        if (0..length).any(|index| {
            let entry = execute::get_property(&addresses, &index.to_string());
            !matches!(execute::get_property(&entry, "address"), Value::String(_))
        }) {
            return Err(());
        }
    } else if !matches!(addresses, Value::String(_)) {
        return Err(());
    }
    Ok(lookup_address_for(result))
}

fn lookup_address_for(result: &Value) -> Option<String> {
    let direct = execute::get_property(result, "1");
    if let Value::String(address) = direct {
        return Some(address);
    }
    let length = match execute::get_property(&direct, "length") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    for index in 0..length {
        let candidate = execute::get_property(&direct, &index.to_string());
        if matches!(execute::get_property(&candidate, "family"), Value::Number(value) if value == 4.0)
        {
            if let Value::String(address) = execute::get_property(&candidate, "address") {
                return Some(address);
            }
        }
    }
    let first = execute::get_property(&direct, "0");
    match execute::get_property(&first, "address") {
        Value::String(address) => Some(address),
        _ => None,
    }
}

fn lookup_addresses_for(result: &Value) -> Vec<String> {
    let direct = execute::get_property(result, "1");
    if let Value::String(address) = direct {
        return vec![address];
    }
    let length = match execute::get_property(&direct, "length") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    (0..length)
        .filter_map(|index| {
            match execute::get_property(
                &execute::get_property(&direct, &index.to_string()),
                "address",
            ) {
                Value::String(address) => Some(address),
                _ => None,
            }
        })
        .collect()
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
    let trimmed = text.trim();
    let parsed = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .and_then(|digits| (!digits.is_empty()).then(|| i64::from_str_radix(digits, 16)))
        .unwrap_or_else(|| trimmed.parse::<i64>());
    let Ok(port) = parsed else {
        return Err(bad_port(value, &text));
    };
    if !(0..=u16::MAX as i64).contains(&port) {
        return Err(bad_port(value, &text));
    }
    Ok(port as u16)
}

fn string_property(object: &Value, key: &str) -> Option<String> {
    match execute::get_property(object, key) {
        Value::String(_) | Value::StringUnits(_) => {
            execute::to_js_string(&execute::get_property(object, key)).ok()
        }
        _ => None,
    }
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
    let signal = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .map(|options| execute::get_property(options, "signal"));
    if let Some(signal) = signal.as_ref() {
        if !matches!(
            signal,
            Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ])));
        }
    }
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if let Value::Boolean(value) = execute::get_property(options, "ipv6Only") {
            execute::set_property_in_place(&receiver, "ipv6Only", Value::Boolean(value));
        }
    }
    // `internalBinding('pipe_wrap').Pipe` is itself a server-owned handle.
    // After `bind(path)`, adopting it through `server.listen(handle)` moves
    // the existing listener instead of treating the handle as connect opts.
    let pipe_handle = args.first().and_then(|value| {
        if matches!(execute::get_property(value, PIPE_MARKER_PROP), Value::Boolean(true)) {
            return Some(value.clone());
        }
        ["handle", "_handle"].into_iter().find_map(|key| {
            let handle = execute::get_property(value, key);
            matches!(execute::get_property(&handle, PIPE_MARKER_PROP), Value::Boolean(true))
                .then_some(handle)
        })
        .or_else(|| {
            let fd = match execute::get_property(value, "fd") {
                Value::Number(fd) if fd.is_finite() && fd.fract() == 0.0 => fd as i64,
                _ => return None,
            };
            state
                .borrow()
                .net
                .servers
                .values()
                .find(|server| {
                    let server = server.borrow();
                    server.listening
                        && matches!(
                            execute::get_property(&server.js, PIPE_MARKER_PROP),
                            Value::Boolean(true)
                        )
                        && matches!(execute::get_property(&server.js, "fd"), Value::Number(value) if value as i64 == fd)
                })
                .map(|server| server.borrow().js.clone())
        })
    });
    if let Some(handle) = pipe_handle.as_ref() {
        if let Some(id) = super::net_id(handle) {
            let existing = { state.borrow().net.servers.get(&id).cloned() };
            if let Some(server) = existing {
                let (listener, path, port) = {
                    let mut server = server.borrow_mut();
                    let listener = server.listener.take();
                    let path = server.path.clone();
                    let port = server.bind_addr.map(|address| address.port()).unwrap_or(0);
                    server.closed = true;
                    (listener, path, port)
                };
                if let Some(listener) = listener {
                    register_server_path(state, &receiver, Some(listener), path.clone())?;
                    if path.is_none() {
                        super::set_server_connection_key(&receiver, port, None)?;
                    }
                    add_listener_cb(state, &receiver, args.get(1), "listening", true)?;
                    configure_server_signal(
                        state,
                        &receiver,
                        signal.as_ref().unwrap_or(&Value::Undefined),
                    )?;
                    return Ok(receiver);
                }
            }
        }
    }
    let fd_bound = args.first().and_then(|value| {
        let fd = match execute::get_property(value, "fd") {
            Value::Number(fd) if fd.is_finite() && fd.fract() == 0.0 => Some(fd as i64),
            _ => None,
        }?;
        state
            .borrow()
            .net
            .bound_sockets
            .iter()
            .find(|(_, bound)| bound.borrow().fd == fd)
            .map(|(id, _)| *id)
    });
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if matches!(execute::get_property(options, "fd"), Value::Number(_))
            && matches!(execute::get_property(options, "handle"), Value::Undefined)
            && fd_bound.is_none()
        {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("listen EINVAL: invalid argument".into())],
            );
            let error = execute::set_property(error, "code", Value::String("EINVAL".into()));
            let error = execute::set_property(error, "syscall", Value::String("listen".into()));
            state
                .borrow_mut()
                .net
                .pending_errors
                .push((receiver.clone(), error));
            return Ok(receiver);
        }
    }
    if let Some(id) = super::net_id(&receiver) {
        let already_listening = state.borrow().net.servers.get(&id).is_some_and(|server| {
            let server = server.borrow();
            server.listening && !server.closed
        });
        if already_listening {
            if state.borrow().cluster.worker_context.is_some() {
                let process_emit = crate::host::capability(crate::registry::SPEC_PROCESS_EMIT);
                let message =
                    host_api::object(vec![("cmd".into(), Value::String("NODE_CLUSTER".into()))]);
                let mut host = state.borrow_mut();
                for _ in 0..2 {
                    host.event_loop.queue_microtask(
                        process_emit.clone(),
                        vec![Value::String("internalMessage".into()), message.clone()],
                    );
                }
                return Ok(receiver);
            }
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                (
                    "code".into(),
                    Value::String("ERR_SERVER_ALREADY_LISTEN".into()),
                ),
            ])));
        }
    }
    let supplied_bound = args
        .first()
        .and_then(|value| {
            bound_id(value).or_else(|| {
                let handle = execute::get_property(value, "handle");
                bound_id(&handle).or_else(|| {
                    let handle = execute::get_property(value, "_handle");
                    bound_id(&handle)
                })
            })
        })
        .or(fd_bound);
    if let Some(bound_id) = supplied_bound {
        let bound = state
            .borrow()
            .net
            .bound_sockets
            .get(&bound_id)
            .cloned()
            .ok_or_else(|| execute::type_error("BoundSocket"))?;
        let (listener, path, port) = {
            let mut bound = bound.borrow_mut();
            if bound.adopted {
                return Err(bound_arg_error(
                    "BoundSocket handle was adopted",
                    "ERR_SOCKET_HANDLE_ADOPTED",
                ));
            }
            let listener = bound
                .listener
                .take()
                .ok_or_else(|| bound_arg_error("BoundSocket is closed", "ERR_SOCKET_CLOSED"))?;
            let path = bound.path.clone();
            let port = bound
                .address
                .or_else(|| listener.local_addr().ok())
                .map(|a| a.port())
                .unwrap_or(0);
            bound.adopted = true;
            (listener, path, port)
        };
        if let Some(path) = path.clone() {
            state.borrow_mut().net.paths.insert(path.clone(), port);
            register_server_path(state, &receiver, Some(listener), Some(path))?;
        } else {
            register_server(state, &receiver, Some(listener))?;
            super::set_server_connection_key(&receiver, port, None)?;
        }
        add_listener_cb(state, &receiver, args.get(1), "listening", true)?;
        configure_server_signal(
            state,
            &receiver,
            signal.as_ref().unwrap_or(&Value::Undefined),
        )?;
        return Ok(receiver);
    }
    if args.len() == 1 && args.first().is_some_and(quench_runtime::is_callable) {
        let listener = match bind_listener(0, None) {
            Ok(listener) => listener,
            Err(error) => {
                state
                    .borrow_mut()
                    .net
                    .pending_errors
                    .push((receiver.clone(), server_bind_error(&error, None, 0)));
                return Ok(receiver);
            }
        };
        register_server(state, &receiver, Some(listener))?;
        add_listener_cb(state, &receiver, args.first(), "listening", true)?;
        configure_server_signal(state, &receiver, &Value::Undefined)?;
        return Ok(receiver);
    }
    let path_argument = args.first().and_then(|value| match value {
        Value::String(_) | Value::StringUnits(_) => execute::to_js_string(value).ok(),
        Value::Object(_) | Value::ObjectAlias(_) => string_property(value, "path"),
        _ => None,
    });
    if let Some(path) = path_argument {
        if path.starts_with('/') || path.parse::<u16>().is_err() {
            if state.borrow().net.paths.contains_key(&path) {
                let error = host_api::object(vec![
                    ("name".into(), Value::String("Error".into())),
                    (
                        "message".into(),
                        Value::String(format!("listen EADDRINUSE: address already in use {path}")),
                    ),
                    ("code".into(), Value::String("EADDRINUSE".into())),
                    ("syscall".into(), Value::String("listen".into())),
                ]);
                state
                    .borrow_mut()
                    .net
                    .pending_errors
                    .push((receiver.clone(), error));
                return Ok(receiver);
            }
            if path.starts_with('/')
                && Path::new(&path)
                    .parent()
                    .is_some_and(|parent| !parent.exists())
            {
                let error = host_api::object(vec![
                    ("name".into(), Value::String("Error".into())),
                    (
                        "message".into(),
                        Value::String(format!("listen ENOENT: no such file or directory {path}")),
                    ),
                    ("code".into(), Value::String("ENOENT".into())),
                    ("address".into(), Value::String(path)),
                    ("syscall".into(), Value::String("listen".into())),
                ]);
                state
                    .borrow_mut()
                    .net
                    .pending_errors
                    .push((receiver.clone(), error));
                return Ok(receiver);
            }
            let listener = match bind_listener(0, None) {
                Ok(listener) => listener,
                Err(error) => {
                    state
                        .borrow_mut()
                        .net
                        .pending_errors
                        .push((receiver.clone(), server_bind_error(&error, None, 0)));
                    return Ok(receiver);
                }
            };
            if let Err(error) = create_pipe_placeholder(&path, args.first()) {
                state
                    .borrow_mut()
                    .net
                    .pending_errors
                    .push((receiver.clone(), server_bind_error(&error, None, 0)));
                return Ok(receiver);
            }
            let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
            state.borrow_mut().net.paths.insert(path.clone(), port);
            if matches!(
                execute::get_property(&receiver, PIPE_MARKER_PROP),
                Value::Boolean(true)
            ) {
                if let Value::Number(fd) = execute::get_property(&receiver, PIPE_FD_PROP) {
                    state
                        .borrow_mut()
                        .net
                        .pipe_fds
                        .insert(fd as i64, path.clone());
                }
            }
            register_server_path(state, &receiver, Some(listener), Some(path.clone()))?;
            add_listener_cb(state, &receiver, args.last(), "listening", true)?;
            configure_server_signal(
                state,
                &receiver,
                signal.as_ref().unwrap_or(&Value::Undefined),
            )?;
            return Ok(receiver);
        }
    }
    let (port, host) = listen_target(state, args)?;
    let cluster_ephemeral_slot = (state.borrow().cluster.worker_context.is_some() && port == 0)
        .then(|| crate::modules::cluster::next_worker_listen_slot(state))
        .flatten();
    // Cluster workers ask the primary for the listener rather than binding a
    // fresh ephemeral port.  The host models workers in one VM, so preserve
    // that shared descriptor identity by cloning the first worker-owned
    // listener for the same address family and construction-order slot. An
    // explicit port matches by port; `listen(0)` matches by logical slot so
    // separate server constructions remain distinct across workers.
    let cluster_listener = (state.borrow().cluster.worker_context.is_some()
        && (port != 0 || cluster_ephemeral_slot.is_some()))
    .then(|| {
        let requested_ipv6 = resolve(host.as_deref().unwrap_or("0.0.0.0"), port).is_ipv6();
        let (process_scope, worker_scopes) = {
            let host = state.borrow();
            (host.cluster.process_scope(), host.cluster.worker_scopes())
        };
        let found = state.borrow().net.servers.values().find_map(|server| {
            let server = server.borrow();
            if server.owner_worker.is_none()
                || !server.listening
                || server.closed
                || server
                    .owner_worker
                    .and_then(|worker| worker_scopes.get(&worker).copied())
                    != Some(process_scope)
                || server
                    .bind_addr
                    .is_none_or(|address| address.is_ipv6() != requested_ipv6)
                || if port != 0 {
                    server
                        .bind_addr
                        .is_none_or(|address| address.port() != port)
                } else {
                    !cluster_ephemeral_slot.is_some_and(|slot| server.ephemeral_slot == Some(slot))
                }
            {
                return None;
            }
            server
                .listener
                .as_ref()
                .and_then(|listener| listener.try_clone().ok())
        });
        found
    })
    .flatten();
    let reuse_port = args.first().is_some_and(|options| {
        matches!(options, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(
                execute::get_property(options, "reusePort"),
                Value::Boolean(true)
            )
    });
    // A reuse-port bind shares the existing listener identity when the host
    // cannot request SO_REUSEPORT directly. Both logical servers still enter
    // the normal net registry and lifecycle; only the kernel descriptor is
    // cloned, which is sufficient for accept/close semantics.
    let shared_listener = reuse_port
        .then(|| {
            state.borrow().net.servers.values().find_map(|server| {
                let server = server.borrow();
                let matches_port = server
                    .bind_addr
                    .is_some_and(|address| address.port() == port);
                matches_port
                    .then(|| {
                        server
                            .listener
                            .as_ref()
                            .and_then(|listener| listener.try_clone().ok())
                    })
                    .flatten()
            })
        })
        .flatten();
    let listener = if let Some(listener) = cluster_listener.or(shared_listener) {
        listener
    } else {
        match bind_listener(port, host.as_deref()) {
            Ok(listener) => listener,
            Err(error) => {
                state.borrow_mut().net.pending_errors.push((
                    receiver.clone(),
                    server_bind_error(&error, host.as_deref(), port),
                ));
                return Ok(receiver);
            }
        }
    };
    register_server(state, &receiver, Some(listener))?;
    if let Some(slot) = cluster_ephemeral_slot {
        if let Some(id) = super::net_id(&receiver) {
            if let Some(server) = state.borrow().net.servers.get(&id) {
                server.borrow_mut().ephemeral_slot = Some(slot);
            }
        }
    }
    super::set_server_connection_key(&receiver, port, host.as_deref())?;
    add_listener_cb(state, &receiver, args.last(), "listening", true)?;
    configure_server_signal(
        state,
        &receiver,
        signal.as_ref().unwrap_or(&Value::Undefined),
    )?;
    Ok(receiver.clone())
}

/// Deprecated internal alias retained for Node's child-process consumers.
pub fn server_listen2(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::emit_warning_with_detail(
        state,
        "DeprecationWarning",
        "Server.prototype._listen2 is deprecated. Use Server.prototype.listen() instead.",
        Some("DEP0208"),
        None,
        true,
    );
    let port = args.get(1).cloned().unwrap_or(Value::Number(0.0));
    server_listen(state, receiver, &[port])
}

fn configure_server_signal(
    state: &Rc<RefCell<HostState>>,
    server: &Value,
    signal: &Value,
) -> Result<(), VmError> {
    if !matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(());
    }
    if matches!(
        execute::get_property(signal, "aborted"),
        Value::Boolean(true)
    ) {
        server_close(state, Some(server), &[])?;
        return Ok(());
    }
    let close = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_NET_SERVER_CLOSE),
        vec![server.clone()],
    );
    let options = host_api::object(vec![("once".into(), Value::Boolean(true))]);
    crate::modules::event_target::add_event_listener(
        state,
        Some(signal),
        &[Value::String("abort".into()), close, options],
    )?;
    Ok(())
}

/// Resolve the `(port, host)` listen target, mirroring `connect_target`.
fn listen_target(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<(u16, Option<String>), VmError> {
    if matches!(args.first(), Some(Value::Object(_) | Value::ObjectAlias(_))) {
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

fn create_pipe_placeholder(path: &str, options: Option<&Value>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // Keep a real filesystem socket at the public path.  The host uses a
        // logical TCP listener for its event-loop transport, but Node's fs
        // surface must still observe the path as a socket (not a regular
        // placeholder file).
        // Unix-domain socket paths have a platform limit; report the same
        // EINVAL that libuv exposes for an overlong pipe name.
        if path.len() > 100 {
            return Err(std::io::Error::from_raw_os_error(22));
        }
        std::os::unix::net::UnixListener::bind(path)?;
    }
    #[cfg(not(unix))]
    {
        use std::fs::OpenOptions;
        let _file = OpenOptions::new().write(true).create_new(true).open(path)?;
    }
    #[cfg(unix)]
    if options.is_some_and(|value| {
        matches!(
            execute::get_property(value, "readableAll"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(value, "writableAll"),
            Value::Boolean(true)
        )
    }) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))?;
    }
    Ok(())
}

/// Register a callable callback for a lifecycle event.
fn add_listener_cb(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    cb: Option<&Value>,
    event: &str,
    once: bool,
) -> Result<(), VmError> {
    if let Some(cb) = cb {
        if quench_runtime::is_callable(cb) {
            let args = &[Value::String(event.to_string()), cb.clone()];
            if once {
                crate::modules::events::method_once(state, Some(receiver), args)?;
            } else {
                crate::modules::events::method_on(state, Some(receiver), args)?;
            }
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
            49 | 99 => "EADDRNOTAVAIL",
            22 | 36 | 63 => "EINVAL",
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
    let receiver = receiver
        .filter(|value| net_id(value).is_some())
        .cloned()
        .or_else(|| {
            args.first()
                .filter(|value| net_id(value).is_some())
                .cloned()
        })
        .unwrap_or(Value::Undefined);
    let Some(id) = net_id(&receiver) else {
        return Ok(receiver);
    };
    let path = state
        .borrow()
        .net
        .servers
        .get(&id)
        .and_then(|server| server.borrow().path.clone());
    if let Some(path) = path {
        state.borrow_mut().net.paths.remove(&path);
        let _ = std::fs::remove_file(&path);
    }
    if matches!(
        execute::get_property(&receiver, PIPE_MARKER_PROP),
        Value::Boolean(true)
    ) {
        if let Value::Number(fd) = execute::get_property(&receiver, PIPE_FD_PROP) {
            state.borrow_mut().net.pipe_fds.remove(&(fd as i64));
        }
    }
    if let Some(server) = state.borrow().net.servers.get(&id).cloned() {
        let mut server = server.borrow_mut();
        server.listener.take();
        server.path.take();
        server.listening = false;
        server.closed = true;
    }
    crate::modules::http::server_close(state, &receiver);
    // Node closes completed keep-alive connections as part of server shutdown;
    // active responses remain owned by their normal EOF/close transitions.
    server_close_idle(state, Some(&receiver), &[])?;
    super::set_server_listening(&receiver, false)?;
    add_listener_cb(state, &receiver, args.first(), "close", true)?;
    let no_connections = !state.borrow().net.sockets.values().any(|socket| {
        socket.borrow().server_id == Some(id) && socket.borrow().state != SocketState::Closed
    });
    if no_connections {
        if let Some(server) = state.borrow().net.servers.get(&id).cloned() {
            server.borrow_mut().close_emitted = true;
        }
        state.borrow_mut().net.servers.remove(&id);
        super::emit(state, &receiver, "close", Vec::new())?;
    }
    Ok(receiver)
}

/// `server.closeIdleConnections()` — terminate established idle sockets
/// without changing the listening state. Active request/response sockets are
/// left to their normal completion path.
pub fn server_close_idle(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let Some(id) = net_id(&receiver) else {
        return Ok(receiver);
    };
    let host = state.borrow();
    let idle: Vec<Value> = host
        .net
        .sockets
        .values()
        .filter_map(|socket| {
            let socket = socket.borrow();
            let is_idle = socket.server_id == Some(id)
                && socket.state != SocketState::Closed
                && host
                    .http
                    .conns
                    .get(&socket.id)
                    .is_some_and(|conn| conn.response_done || conn.req.is_none());
            is_idle.then(|| socket.js.clone())
        })
        .collect();
    drop(host);
    for socket in idle {
        socket_destroy(state, Some(&socket), &[])?;
    }
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

/// `server.getConnections(callback)` reports the currently accepted sockets.
pub fn server_get_connections(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| execute::type_error("server"))?;
    let id = super::net_id(receiver).ok_or_else(|| execute::type_error("server"))?;
    let count = state
        .borrow()
        .net
        .sockets
        .values()
        .filter(|socket| {
            let socket = socket.borrow();
            socket.server_id == Some(id) && socket.state != SocketState::Closed
        })
        .count();
    if let Some(callback) = args.first() {
        if !quench_runtime::is_callable(callback) {
            return Err(execute::type_error("callback"));
        }
        execute::call(
            callback,
            &Value::Undefined,
            &[Value::Null, Value::Number(count as f64)],
        )?;
    }
    Ok(receiver.clone())
}

/// `server.address()` — the bound address object, or null.
pub fn server_address(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = receiver.and_then(net_id) else {
        return Ok(Value::Null);
    };
    let server = state.borrow().net.servers.get(&id).cloned();
    let Some(server) = server else {
        return Ok(Value::Null);
    };
    let server = server.borrow();
    let address = server
        .path
        .clone()
        .map(Value::String)
        .or_else(|| server.bind_addr.map(address_value))
        .unwrap_or(Value::Null);
    Ok(address)
}

/// `socket.write(data[, encoding][, cb])` — buffers bytes and flushes
/// what the socket will take; returns whether everything flushed.
pub fn socket_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if receiver.is_some_and(|socket| {
        matches!(
            execute::get_property(socket, crate::modules::tls::TLS_REJECTED_PROP),
            Value::Boolean(true)
        )
    }) {
        return Ok(Value::Boolean(false));
    }
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let tls_socket = matches!(
        execute::get_property(&receiver, crate::modules::tls::TLS_SOCKET_PROP),
        Value::Boolean(true)
    );
    let Some(id) = net_id(&receiver) else {
        return Ok(Value::Boolean(false));
    };
    let bytes = match args.first() {
        Some(Value::Null) => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_STREAM_NULL_VALUES".into())),
                (
                    "message".into(),
                    Value::String("May not write null values to stream".into()),
                ),
            ])))
        }
        Some(Value::String(s)) if args.get(1).is_some_and(|encoding| {
            matches!(encoding, Value::String(value) if matches!(value.to_ascii_lowercase().as_str(), "latin1" | "binary" | "ascii"))
        }) => s.chars().map(|character| character as u32 as u8).collect(),
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::StringUnits(units)) => String::from_utf16_lossy(units).into_bytes(),
        Some(Value::Uint8Array(view)) => {
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        Some(value) => return Err(write_chunk_type_error(value)),
        None => return Err(write_chunk_type_error(&Value::Undefined)),
    };
    let callback = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
        .or_else(|| {
            args.get(2)
                .filter(|value| quench_runtime::is_callable(value))
        })
        .cloned();
    let Some(sock) = state.borrow().net.sockets.get(&id).cloned() else {
        let pending = state
            .borrow()
            .net
            .pending_lookups
            .iter()
            .any(|lookup| net_id(&lookup.socket) == Some(id));
        if pending {
            state
                .borrow_mut()
                .net
                .pending_connect_writes
                .entry(id)
                .or_default()
                .extend_from_slice(&bytes);
            return Ok(Value::Boolean(true));
        }
        if let Some(callback) = callback.filter(|_| !tls_socket) {
            state.borrow_mut().event_loop.queue_microtask(
                callback,
                vec![super::handle_write_error(
                    "EPIPE",
                    "This socket has been ended by the other party",
                )],
            );
        }
        return Ok(Value::Boolean(false));
    };
    let handle = execute::get_property(&receiver, "_handle");
    let server_owned = sock.borrow().server_id.is_some();
    let http_owned = server_owned
        && (state.borrow().http.conns.contains_key(&id)
            || matches!(
                execute::get_property(&receiver, crate::modules::http::HTTP_SERVER_SOCKET_PROP),
                Value::Boolean(true)
            ));
    let handle_closed = matches!(
        execute::get_property(&handle, super::HANDLE_CLOSED_PROP),
        Value::Boolean(true)
    );
    let handle_missing = !server_owned && matches!(handle, Value::Undefined | Value::Null);
    if handle_closed || handle_missing {
        let destroyed = matches!(
            execute::get_property(&receiver, "destroyed"),
            Value::Boolean(true)
        );
        let error = if destroyed {
            super::handle_write_error(
                "ERR_STREAM_DESTROYED",
                "Cannot call write after a stream was destroyed",
            )
        } else if handle_missing {
            super::handle_write_error("ERR_SOCKET_CLOSED", "Socket is closed")
        } else {
            super::handle_write_error("EBADF", "write EBADF")
        };
        if let Some(callback) = callback.filter(|_| !tls_socket) {
            state
                .borrow_mut()
                .event_loop
                .queue_microtask(callback, vec![error]);
        } else if !http_owned && !tls_socket {
            state.borrow_mut().net.pending_events.push((
                receiver.clone(),
                "error".into(),
                vec![error],
            ));
        }
        socket_destroy(state, Some(&receiver), &[])?;
        return Ok(Value::Boolean(false));
    }
    let mut guard = sock.borrow_mut();
    let allow_half_open = matches!(
        execute::get_property(&guard.js, "allowHalfOpen"),
        Value::Boolean(true)
    );
    let readable_ended = matches!(
        execute::get_property(&guard.js, "readable"),
        Value::Boolean(false)
    );
    if guard.read_eof && readable_ended && guard.finish_emitted && !allow_half_open {
        drop(guard);
        let error =
            super::handle_write_error("EPIPE", "This socket has been ended by the other party");
        if !http_owned && !tls_socket {
            state.borrow_mut().net.pending_events.push((
                receiver.clone(),
                "error".into(),
                vec![error.clone()],
            ));
        }
        if let Some(callback) = callback.filter(|_| !tls_socket) {
            state
                .borrow_mut()
                .event_loop
                .queue_microtask(callback, vec![error]);
        }
        socket_destroy(state, Some(&receiver), &[])?;
        return Ok(Value::Boolean(false));
    }
    if guard.state == SocketState::Closed {
        if let Some(callback) = callback.filter(|_| !tls_socket) {
            state.borrow_mut().event_loop.queue_microtask(
                callback,
                vec![super::handle_write_error(
                    "EPIPE",
                    "This socket has been ended by the other party",
                )],
            );
        }
        return Ok(Value::Boolean(false));
    }
    guard.bytes_written = guard.bytes_written.saturating_add(bytes.len() as u64);
    guard.write_buf.extend_from_slice(&bytes);
    update_socket_counters(&guard);
    super::set_socket_property(
        &receiver,
        "bytesWritten",
        Value::Number(guard.bytes_written as f64),
    );
    let connecting = !guard.connect_announced;
    // Before the first connect turn, libuv reports the write as queued even
    // when the OS could accept it synchronously. Keep bufferSize observable
    // until the pump's connect transition flushes the queue.
    let _flushed = !connecting && !tls_socket && try_flush(&mut guard);
    let pending = super::pending_write_len(&guard);
    super::set_socket_property(&receiver, "bufferSize", Value::Number(pending as f64));
    super::set_socket_property(&receiver, "writableLength", Value::Number(pending as f64));
    let high_water_mark = match execute::get_property(&guard.js, "writableHighWaterMark") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 16_384,
    };
    let result = Value::Boolean(super::pending_write_len(&guard) < high_water_mark);
    drop(guard);
    if let Some(callback) = callback {
        if connecting {
            crate::modules::events::method_once(
                state,
                Some(&receiver),
                &[Value::String("connect".into()), callback.clone()],
            )?;
        } else {
            execute::call(&callback, &receiver, &[])?;
        }
    }
    Ok(result)
}

/// `socket.end([data][, cb])` — write `data` if given, then close the
/// write side; the socket closes fully once the peer also ends.
pub fn socket_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(data) = args
        .first()
        .filter(|data| !matches!(data, Value::Undefined) && !quench_runtime::is_callable(data))
    {
        socket_write(state, receiver, std::slice::from_ref(data))?;
    }
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(id) = net_id(receiver) else {
        return Ok(receiver.clone());
    };
    let connecting = state.borrow().net.sockets.get(&id).is_some_and(|socket| {
        let socket = socket.borrow();
        socket.stream.is_some() && !socket.connect_announced
    });
    let pending = state
        .borrow()
        .net
        .sockets
        .get(&id)
        .map(|socket| super::pending_write_len(&socket.borrow()))
        .unwrap_or(0);
    execute::set_property_in_place(receiver, "bufferSize", Value::Number(pending as f64));
    execute::set_property_in_place(receiver, "writableLength", Value::Number(pending as f64));
    // The JavaScript half-close state is synchronous. Apply it to the
    // receiver before consulting the native registry so aliases and sockets
    // whose host entry is being retired observe the same transition.
    if !connecting {
        super::set_socket_property(receiver, "writable", Value::Boolean(false));
        super::set_socket_property(receiver, "readyState", Value::String("readOnly".into()));
    }
    let mut queue_finish = false;
    let Some(sock) = state.borrow().net.sockets.get(&id).cloned() else {
        // A socket can be observed through an event-delivery alias after its
        // host entry has already been retired. Preserve the public half-close
        // transition on that receiver instead of silently leaving it open.
        state
            .borrow_mut()
            .net
            .pending_events
            .push((receiver.clone(), "finish".into(), Vec::new()));
        return Ok(receiver.clone());
    };
    let mut guard = sock.borrow_mut();
    if !guard.finish_emitted {
        guard.finish_emitted = true;
        if guard.connect_announced {
            execute::set_property_in_place(&guard.js, "writable", Value::Boolean(false));
            execute::set_property_in_place(
                &guard.js,
                "readyState",
                Value::String("readOnly".into()),
            );
        }
        queue_finish = true;
    }
    guard.state = SocketState::Closing;
    guard.write_shutdown_pending = true;
    try_flush(&mut guard);
    let pending = super::pending_write_len(&guard);
    super::set_socket_property(receiver, "bufferSize", Value::Number(pending as f64));
    super::set_socket_property(receiver, "writableLength", Value::Number(pending as f64));
    if queue_finish {
        if let Some(callback) = args
            .iter()
            .rev()
            .find(|value| quench_runtime::is_callable(value))
        {
            crate::modules::events::method_once(
                state,
                Some(receiver),
                &[Value::String("finish".into()), callback.clone()],
            )?;
        }
        state
            .borrow_mut()
            .net
            .pending_events
            .push((receiver.clone(), "finish".into(), Vec::new()));
    }
    let _ = state;
    Ok(receiver.clone())
}

/// `socket.destroy()` — drop the socket and emit `'close'`.
pub fn socket_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let Some(id) = net_id(&receiver) else {
        return Ok(receiver);
    };
    let tracked_timer = state.borrow_mut().net.timeout_timers.remove(&id);
    if let Some(timer) = tracked_timer {
        crate::modules::timers::clear_timeout(state, &[timer])?;
    }
    if let Some(timer) = timeout_timer(&receiver) {
        crate::modules::timers::clear_timeout(state, &[timer])?;
        execute::set_property_in_place(&receiver, SOCKET_TIMEOUT_PROP, Value::Undefined);
    }
    let mut emit_close = false;
    let mut bytes_read = 0;
    let socket_entry = state.borrow().net.sockets.get(&id).cloned();
    if let Some(sock) = socket_entry {
        let mut guard = sock.borrow_mut();
        bytes_read = guard.bytes_read;
        let error = args.first().cloned();
        let was_closed = guard.state == SocketState::Closed;
        if guard.state != SocketState::Closed {
            if let Some(stream) = guard.stream.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            guard.state = SocketState::Closed;
            emit_close = true;
        }
        drop(guard);
        if let Some(error) = error {
            emit(state, &receiver, "error", vec![error])?;
        }
        if was_closed {
            return Ok(receiver);
        }
    }
    if emit_close {
        set_socket_state(&receiver, true, false, "closed");
        super::replace_socket_property(&receiver, "pending", Value::Boolean(true));
        super::set_socket_bytes_read(&receiver, bytes_read);
        super::replace_socket_property(&receiver, "_handle", Value::Null);
        crate::modules::http_client::mark_socket_destroyed_in_agents(state, &receiver);
        // Node delivers socket close on the next loop turn, allowing a
        // listener attached immediately after `destroy()` to observe it.
        state.borrow_mut().net.pending_events.push((
            receiver.clone(),
            "close".into(),
            vec![Value::Boolean(args.first().is_some())],
        ));
    }
    Ok(receiver)
}

pub fn socket_unref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(id) = receiver.and_then(net_id) {
        if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
            socket.borrow_mut().refed = false;
        }
        let timer = { state.borrow().net.timeout_timers.get(&id).cloned() };
        if let Some(timer) = timer {
            let _ = crate::modules::timers::method_unref(state, Some(&timer));
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn socket_ref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(id) = receiver.and_then(net_id) {
        if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
            socket.borrow_mut().refed = true;
        }
        let timer = { state.borrow().net.timeout_timers.get(&id).cloned() };
        if let Some(timer) = timer {
            let _ = crate::modules::timers::method_ref(state, Some(&timer));
        }
    }
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
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        let enabled = args.first().map(execute::is_truthy).unwrap_or(true);
        let previous = matches!(
            execute::get_property(receiver, super::NO_DELAY_PROP),
            Value::Boolean(true)
        );
        let handle = execute::get_property(receiver, "_handle");
        let handle_is_object = matches!(handle, Value::Object(_) | Value::ObjectAlias(_));
        let applied = matches!(
            execute::get_property(&handle, HANDLE_NO_DELAY_PROP),
            Value::Boolean(true)
        );
        if enabled != previous || (handle_is_object && !applied) {
            let binding = state.borrow().tcp_binding.clone().unwrap_or_else(|| {
                let global = quench_runtime::vm::current_global_object();
                execute::get_property(&global, TCP_WRAP_BINDING_PROP)
            });
            let prototype =
                execute::get_property(&execute::get_property(&binding, "TCPWrap"), "prototype");
            let set_no_delay = execute::get_property(&prototype, "setNoDelay");
            let set_no_delay = if handle_is_object && quench_runtime::is_callable(&set_no_delay) {
                set_no_delay
            } else if handle_is_object {
                execute::get_property(&handle, "setNoDelay")
            } else {
                Value::Undefined
            };
            if quench_runtime::is_callable(&set_no_delay) {
                execute::call(&set_no_delay, &handle, &[Value::Boolean(enabled)])?;
                if handle_is_object {
                    execute::set_property_in_place(
                        &handle,
                        HANDLE_NO_DELAY_PROP,
                        Value::Boolean(true),
                    );
                }
            }
        } else {
            return Ok(receiver.clone());
        }
        execute::set_property_in_place(receiver, super::NO_DELAY_PROP, Value::Boolean(enabled));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn socket_set_type_of_service(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => *value,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"tos\" argument must be a number".into(),
            ))
        }
    };
    if !(0.0..=255.0).contains(&value) {
        return Err(crate::modules::buffer_enc::out_of_range(
            "tos",
            ">= 0 && <= 255",
            &value.to_string(),
        ));
    }
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, super::TOS_PROP, Value::Number(value));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn socket_get_type_of_service(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .map(|socket| execute::get_property(socket, super::TOS_PROP))
        .filter(|value| matches!(value, Value::Number(_)))
        .unwrap_or(Value::Number(0.0)))
}

pub fn socket_handle_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, super::HANDLE_CLOSED_PROP, Value::Boolean(true));
    }
    // Native handles invoke their optional completion callback after the
    // close request has been accepted.  This is also used by cluster's
    // round-robin handoff when a worker rejects an incoming handle.
    if let Some(callback) = args.iter().find(|value| quench_runtime::is_callable(value)) {
        state
            .borrow_mut()
            .event_loop
            .queue_microtask(callback.clone(), Vec::new());
    }
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

/// `socket.setTimeout(msecs[, callback])` shares the host timer registry.
pub fn socket_set_timeout(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = execute::canonical_value(&receiver.cloned().unwrap_or(Value::Undefined));
    let timeout = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value,
        Some(Value::Number(value)) => {
            return Err(crate::modules::buffer_enc::out_of_range(
                "timeout",
                ">= 0",
                &value.to_string(),
            ))
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"timeout\" argument must be a number".into(),
            ))
        }
    };
    if let Some(callback) = args.get(1) {
        if !quench_runtime::is_callable(callback) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"callback\" argument must be a function".into(),
            ));
        }
    }
    if let Value::Object(_) | Value::ObjectAlias(_) = receiver {
        // Runtime aliases can expose the same socket with distinct property
        // maps. Resolve the host-owned net record first so timeout state and
        // timer identity are cleared on the canonical socket object.
        let target = net_id(&receiver)
            .and_then(|id| {
                state
                    .borrow()
                    .net
                    .sockets
                    .get(&id)
                    .map(|socket| socket.borrow().js.clone())
            })
            .unwrap_or_else(|| receiver.clone());
        let socket_id = net_id(&target);
        let tracked_timer =
            socket_id.and_then(|id| state.borrow_mut().net.timeout_timers.remove(&id));
        if let Some(timer) = tracked_timer {
            crate::modules::timers::clear_timeout(state, &[timer])?;
        }
        if let Some(timer) = timeout_timer(&target) {
            crate::modules::timers::clear_timeout(state, &[timer])?;
        }
        execute::set_property_in_place(&target, SOCKET_TIMEOUT_PROP, Value::Undefined);
        execute::set_property_in_place(&target, "timeout", Value::Number(timeout));
        if matches!(
            execute::get_property(&target, "destroyed"),
            Value::Boolean(true)
        ) {
            return Ok(receiver);
        }
        if timeout > 0.0 {
            if let Some(callback) = args.get(1) {
                let once = crate::host::capability(crate::registry::SPEC_EVENTS_ONCE);
                execute::call(
                    &once,
                    &target,
                    &[Value::String("timeout".into()), callback.clone()],
                )?;
            }
            let callback = quench_runtime::host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_NET_SOCKET_TIMEOUT_FIRE.cap,
                    ),
                },
                vec![target.clone()],
            );
            let timer =
                crate::modules::timers::set_timeout(state, &[callback, Value::Number(timeout)])?;
            if let Some(id) = socket_id {
                state
                    .borrow_mut()
                    .net
                    .timeout_timers
                    .insert(id, timer.clone());
            }
            execute::set_property_in_place(&target, SOCKET_TIMEOUT_PROP, timer);
        }
        // Keep any VM alias that invoked setTimeout observable in sync with
        // the canonical socket record.
        execute::set_property_in_place(&receiver, "timeout", Value::Number(timeout));
    }
    Ok(receiver)
}

pub fn socket_timeout_fire(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket) = args.first() else {
        return Ok(Value::Undefined);
    };
    execute::set_property_in_place(socket, SOCKET_TIMEOUT_PROP, Value::Undefined);
    if crate::modules::http::is_idle_socket(state, socket)
        && !matches!(
            execute::get_property(socket, "destroyed"),
            Value::Boolean(true)
        )
    {
        socket_destroy(state, Some(socket), &[])?;
    }
    crate::modules::events::method_emit(state, Some(socket), &[Value::String("timeout".into())])
}

fn timeout_timer(socket: &Value) -> Option<Value> {
    match execute::get_property(socket, SOCKET_TIMEOUT_PROP) {
        Value::Undefined | Value::Null => None,
        timer => Some(timer),
    }
}

/// `socket.setEncoding(encoding)` — decode `'data'` chunks to strings.
pub fn socket_set_encoding(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let encoding = args
        .first()
        .map(execute::to_js_string)
        .transpose()?
        .map(|value| value.to_ascii_lowercase());
    if let Some(receiver) = receiver {
        let value = encoding
            .as_ref()
            .map_or(Value::Undefined, |value| Value::String(value.clone()));
        execute::set_property_in_place(receiver, SOCKET_ENCODING_PROP, value);
    }
    if let Some(id) = receiver.and_then(net_id) {
        if let Some(sock) = state.borrow().net.sockets.get(&id) {
            sock.borrow_mut().encoding = encoding;
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `socket.pause()` / `socket.resume()` suspend and resume onread delivery.
pub fn socket_pause(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        super::replace_socket_property(receiver, ONREAD_PAUSED_PROP, Value::Boolean(true));
        if let Some(id) = net_id(receiver) {
            if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
                let canonical = socket.borrow().js.clone();
                execute::set_property_in_place(
                    &canonical,
                    ONREAD_PAUSED_PROP,
                    Value::Boolean(true),
                );
            }
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn socket_resume(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        super::replace_socket_property(receiver, ONREAD_PAUSED_PROP, Value::Boolean(false));
        if let Some(id) = net_id(receiver) {
            if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
                let canonical = socket.borrow().js.clone();
                execute::set_property_in_place(
                    &canonical,
                    ONREAD_PAUSED_PROP,
                    Value::Boolean(false),
                );
            }
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}
