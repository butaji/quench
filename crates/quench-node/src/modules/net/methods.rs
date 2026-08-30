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

const SOCKET_TIMEOUT_PROP: &str = "\0quench:net:timeout";

/// `net.createServer([connectionListener])` — a server object backed by
/// an emitter; the listener, if given, registers for `'connection'`.
pub fn create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let (object, _id) = new_net_object(state, server_props())?;
    register_server(state, &object, None)?;
    add_listener_cb(state, &object, args.first(), "connection", false)?;
    Ok(object)
}

/// `new net.Socket()` creates an unconnected socket whose `connect` method
/// shares the public connection capability and validation path.
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
    let (object, _id) = new_net_object(state, socket_props())?;
    let global = quench_runtime::vm::current_global_object();
    let prototype = execute::get_property(&global, "\0quench:net:socket-prototype");
    let object = if matches!(prototype, Value::Object(_)) {
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
    }
    let object = install_methods(
        object,
        vec![(
            "connect".to_string(),
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        )],
    )?;
    let object = execute::canonical_value(&object);
    state.borrow_mut().net.sockets.insert(
        _id,
        Rc::new(RefCell::new(NetSocket {
            id: _id,
            stream: None,
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
            finish_emitted: false,
            connect_announced: false,
            peer: None,
            local: None,
            encoding: None,
        })),
    );
    Ok(object)
}

fn validate_connect_options(options: &Value) -> Result<(), VmError> {
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
    if !matches!(host, Value::Undefined | Value::Null | Value::String(_) | Value::StringUnits(_)) {
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
                    Value::String("The property 'options.host' must be a string without null bytes.".into()),
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
        Some(port) => connect(
            state,
            &[
                Value::Number(port as f64),
                Value::String("127.0.0.1".into()),
            ],
        ),
        None => {
            let (object, _) = new_net_object(state, socket_props())?;
            let error = host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                ("message".into(), Value::String(format!("connect ENOENT {path}"))),
                ("code".into(), Value::String("ENOENT".into())),
                ("syscall".into(), Value::String("connect".into())),
            ]);
            state
                .borrow_mut()
                .net
                .pending_errors
                .push((object.clone(), error));
            Ok(object)
        }
    }
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
pub fn complete_lookup(
    state: &Rc<RefCell<HostState>>,
    result: Value,
) -> Result<Value, VmError> {
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
    let Some(address) = custom_lookup_address(&pending.options, &result).ok().flatten() else {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("Invalid IP address: lookup returned an invalid address".into())],
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
    let family = lookup_family(&result).map(|value| Value::Number(value as f64));
    state.borrow_mut().net.pending_events.push((
        pending.socket.clone(),
        "lookup".into(),
        vec![Value::Null, Value::String(address.clone()), family.unwrap_or(Value::Undefined)],
    ));
    let options = execute::set_property(
        execute::set_property(pending.options, "lookup", Value::Undefined),
        "host",
        Value::String(address),
    );
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
    let mut lookup_address = None;
    if let Some(path) = args.first().filter(|value| {
        matches!(value, Value::String(_) | Value::StringUnits(_))
    }) {
        let path = execute::to_js_string(path)?;
        if path.starts_with('/') || state.borrow().net.paths.contains_key(&path) {
            return connect_path(state, &path);
        }
    }
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        validate_connect_options(options)?;
        if let Some(path) = string_property(options, "socketPath") {
            if path.starts_with('/') || state.borrow().net.paths.contains_key(&path) {
                return connect_path(state, &path);
            }
        }
        if let Some(path) = string_property(options, "port") {
            if path.starts_with('/') || state.borrow().net.paths.contains_key(&path) {
                return connect_path(state, &path);
            }
        }
        if matches!(execute::get_property(options, "port"), Value::Undefined) {
            if let Some(path) = string_property(options, "path") {
                if path.starts_with('/') || state.borrow().net.paths.contains_key(&path) {
                    return connect_path(state, &path);
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
            let callback = crate::host::capability(crate::registry::SPEC_NET_LOOKUP_CALLBACK);
            let lookup_options = if matches!(
                execute::get_property(options, "autoSelectFamily"),
                Value::Boolean(true)
            ) {
                execute::set_property(options.clone(), "all", Value::Boolean(true))
            } else {
                options.clone()
            };
            state.borrow_mut().net.pending_lookups.push(super::PendingLookup {
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
            if !matches!(execute::get_property(&result, "0"), Value::Undefined | Value::Null) {
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
                    &[Value::String("Invalid IP address: lookup returned an invalid address".into())],
                );
                let error = execute::set_property(
                    error,
                    "code",
                    Value::String("ERR_INVALID_IP_ADDRESS".into()),
                );
                state.borrow_mut().net.pending_errors.push((object.clone(), error));
                return Ok(object);
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
    if port == 0 {
        let loopback = SocketAddr::new(LOCAL_HOST.parse().expect("loopback"), 0);
        return connect_refused(state, &loopback);
    }
    let Some(addr) = super::resolve_connect(target_host, port) else {
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
    let object = execute::canonical_value(&object);
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        configure_onread(&object, options);
        let no_delay = execute::get_property(options, "noDelay");
        if execute::is_truthy(&no_delay) {
            execute::set_property_in_place(
                &object,
                super::NO_DELAY_PROP,
                Value::Boolean(true),
            );
        }
    }
    let handle = host_api::object(vec![
        ("setNoDelay".into(), Value::Builtin(quench_runtime::ops::Builtin::Object)),
        ("setKeepAlive".into(), Value::Builtin(quench_runtime::ops::Builtin::Object)),
    ]);
    execute::set_property_in_place(&object, "_handle", handle);
    let local = stream.local_addr().ok();
    set_socket_state(&object, true, true, "opening");
    let socket = Rc::new(std::cell::RefCell::new(NetSocket {
        id,
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
        finish_emitted: false,
        connect_announced: false,
        peer: Some(addr),
        local,
        encoding: None,
    }));
    state.borrow_mut().net.sockets.insert(id, socket);
    add_listener_cb(state, &object, args.last(), "connect", true)?;
    if let Some(options) = args.first().filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_))) {
        let signal = execute::get_property(options, "signal");
        if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
            if matches!(execute::get_property(&signal, "aborted"), Value::Boolean(true)) {
                state.borrow_mut().net.pending_errors.push((object.clone(), abort_error()));
            } else {
                let listener = host_api::bound_capability_with_arguments(
                    crate::host::capability_ref(crate::registry::SPEC_NET_SOCKET_ABORT),
                    vec![object.clone()],
                );
                let listener_options = host_api::object(vec![("once".into(), Value::Boolean(true))]);
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
    // `AbortSignal` dispatch is synchronous, but net errors are delivered on
    // the next loop turn so listeners attached immediately after `abort()`
    // still observe the failure (the Node contract used by Agent tests).
    state
        .borrow_mut()
        .net
        .pending_events
        .push((socket.clone(), "error".into(), vec![abort_error()]));
    socket_destroy(state, Some(socket), &[])
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
fn connect_refused(state: &Rc<RefCell<HostState>>, addr: &SocketAddr) -> Result<Value, VmError> {
    let (object, _id) = new_net_object(state, socket_props())?;
    let message = format!("connect ECONNREFUSED {addr}");
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message)],
    );
    let error = execute::set_property(error, "code", Value::String("ECONNREFUSED".into()));
    let error = execute::set_property(error, "errno", Value::Number(-61.0));
    let error = execute::set_property(error, "syscall", Value::String("connect".into()));
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
        return Ok(receiver);
    }
    if let Some(path) = args.first().filter(|value| {
        matches!(value, Value::String(_) | Value::StringUnits(_))
    }).and_then(|value| execute::to_js_string(value).ok()) {
        if path.starts_with('/') || path.parse::<u16>().is_err() {
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
            let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
            state.borrow_mut().net.paths.insert(path.clone(), port);
            register_server_path(state, &receiver, Some(listener), Some(path.clone()))?;
            add_listener_cb(state, &receiver, args.last(), "listening", true)?;
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
    add_listener_cb(state, &receiver, args.last(), "listening", true)?;
    Ok(receiver.clone())
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
    let path = state
        .borrow()
        .net
        .servers
        .get(&id)
        .and_then(|server| server.borrow().path.clone());
    if let Some(path) = path {
        state.borrow_mut().net.paths.remove(&path);
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
    Ok(server
        .path
        .clone()
        .map(Value::String)
        .or_else(|| server.bind_addr.map(address_value))
        .unwrap_or(Value::Null))
}

/// `socket.write(data[, encoding][, cb])` — buffers bytes and flushes
/// what the socket will take; returns whether everything flushed.
pub fn socket_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let Some(id) = net_id(&receiver) else {
        return Ok(Value::Boolean(false));
    };
    let Some(sock) = state.borrow().net.sockets.get(&id).cloned() else {
        return Ok(Value::Boolean(false));
    };
    let bytes = match args.first() {
        Some(Value::String(s)) if args.get(1).is_some_and(|encoding| {
            matches!(encoding, Value::String(value) if matches!(value.to_ascii_lowercase().as_str(), "latin1" | "binary" | "ascii"))
        }) => s.chars().map(|character| character as u32 as u8).collect(),
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::StringUnits(units)) => String::from_utf16_lossy(units).into_bytes(),
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
    super::set_socket_property(
        &receiver,
        "bytesWritten",
        Value::Number(guard.bytes_written as f64),
    );
    let connecting = !guard.connect_announced;
    // Before the first connect turn, libuv reports the write as queued even
    // when the OS could accept it synchronously. Keep bufferSize observable
    // until the pump's connect transition flushes the queue.
    let flushed = !connecting && try_flush(&mut guard);
    let pending = super::pending_write_len(&guard);
    super::set_socket_property(
        &receiver,
        "bufferSize",
        Value::Number(pending as f64),
    );
    super::set_socket_property(
        &receiver,
        "writableLength",
        Value::Number(pending as f64),
    );
    let high_water_mark = match execute::get_property(&guard.js, "writableHighWaterMark") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 16_384,
    };
    let result = Value::Boolean(
        (connecting || flushed) && super::pending_write_len(&guard) < high_water_mark,
    );
    drop(guard);
    let callback = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
        .or_else(|| args.get(2).filter(|value| quench_runtime::is_callable(value)));
    if let Some(callback) = callback {
        if connecting {
            crate::modules::events::method_once(
                state,
                Some(&receiver),
                &[Value::String("connect".into()), callback.clone()],
            )?;
        } else {
            execute::call(callback, &receiver, &[])?;
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
    if let Some(data) = args.first() {
        if !matches!(data, Value::Undefined) {
            socket_write(state, receiver, std::slice::from_ref(data))?;
        }
    }
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(id) = net_id(receiver) else {
        return Ok(receiver.clone());
    };
    let connecting = state
        .borrow()
        .net
        .sockets
        .get(&id)
        .is_some_and(|socket| !socket.borrow().connect_announced);
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
        execute::set_property_in_place(receiver, "writable", Value::Boolean(false));
        execute::set_property_in_place(
            receiver,
            "readyState",
            Value::String("readOnly".into()),
        );
    }
    let mut queue_finish = false;
    let Some(sock) = state.borrow().net.sockets.get(&id).cloned() else {
        // A socket can be observed through an event-delivery alias after its
        // host entry has already been retired. Preserve the public half-close
        // transition on that receiver instead of silently leaving it open.
        state.borrow_mut().net.pending_events.push((
            receiver.clone(),
            "finish".into(),
            Vec::new(),
        ));
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
    try_flush(&mut guard);
    let pending = super::pending_write_len(&guard);
    super::set_socket_property(
        receiver,
        "bufferSize",
        Value::Number(pending as f64),
    );
    super::set_socket_property(
        receiver,
        "writableLength",
        Value::Number(pending as f64),
    );
    if pending == 0 {
        if let Some(stream) = guard.stream.as_mut() {
            let _ = stream.shutdown(Shutdown::Write);
        }
    }
    if queue_finish {
        state.borrow_mut().net.pending_events.push((
            receiver.clone(),
            "finish".into(),
            Vec::new(),
        ));
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
    let socket_entry = state.borrow().net.sockets.get(&id).cloned();
    if let Some(sock) = socket_entry {
        let mut guard = sock.borrow_mut();
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
        crate::modules::http_client::mark_socket_destroyed_in_agents(state, &receiver);
        // Node delivers socket close on the next loop turn, allowing a
        // listener attached immediately after `destroy()` to observe it.
        state
            .borrow_mut()
            .net
            .pending_events
            .push((receiver.clone(), "close".into(), Vec::new()));
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
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        let handle = execute::get_property(receiver, "_handle");
        let set_no_delay = execute::get_property(&handle, "setNoDelay");
        if quench_runtime::is_callable(&set_no_delay) {
            let enabled = args.first().map(execute::is_truthy).unwrap_or(true);
            execute::call(&set_no_delay, &handle, &[Value::Boolean(enabled)])?;
        }
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
        Some(Value::Number(_)) => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The \"timeout\" value is out of range".into(),
            ))
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"timeout\" argument must be a number".into(),
            ))
        }
    };
    if let Value::Object(_) | Value::ObjectAlias(_) = receiver {
        // Runtime aliases can expose the same socket with distinct property
        // maps. Resolve the host-owned net record first so timeout state and
        // timer identity are cleared on the canonical socket object.
        let target = net_id(&receiver)
            .and_then(|id| state.borrow().net.sockets.get(&id).map(|socket| socket.borrow().js.clone()))
            .unwrap_or_else(|| receiver.clone());
        let socket_id = net_id(&target);
        let tracked_timer = socket_id.and_then(|id| state.borrow_mut().net.timeout_timers.remove(&id));
        if let Some(timer) = tracked_timer {
            crate::modules::timers::clear_timeout(state, &[timer])?;
        }
        if let Some(timer) = timeout_timer(&target) {
            crate::modules::timers::clear_timeout(state, &[timer])?;
        }
        execute::set_property_in_place(&target, SOCKET_TIMEOUT_PROP, Value::Undefined);
        execute::set_property_in_place(&target, "timeout", Value::Number(timeout));
        if timeout > 0.0 {
            if let Some(callback) = args.get(1) {
                if !quench_runtime::is_callable(callback) {
                    return Err(crate::modules::buffer_enc::invalid_arg_type(
                        "The \"callback\" argument must be a function".into(),
                    ));
                }
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
                state.borrow_mut().net.timeout_timers.insert(id, timer.clone());
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
        && !matches!(execute::get_property(socket, "destroyed"), Value::Boolean(true))
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
    let encoding = args.first().map(execute::to_js_string).transpose()?;
    if let Some(id) = receiver.and_then(net_id) {
        if let Some(sock) = state.borrow().net.sockets.get(&id) {
            sock.borrow_mut().encoding = encoding;
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `socket.pause()` / `socket.resume()` suspend and resume onread delivery.
pub fn socket_pause(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, ONREAD_PAUSED_PROP, Value::Boolean(true));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn socket_resume(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, ONREAD_PAUSED_PROP, Value::Boolean(false));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}
