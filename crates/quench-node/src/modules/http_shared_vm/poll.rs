use super::state::{Response, ServerConnection};
use crate::host::{NodeHost, SharedNodeState};
use crate::modules::net_shared_vm;
use quench_runtime::{RootId, Runtime, Value};
use std::cell::RefCell;
use std::rc::Rc;

const REQUEST_HEAD_LIMIT: usize = 64 * 1024;
const RESPONSE_HEAD_LIMIT: usize = 64 * 1024;
const DEFAULT_HTTP_SOCKET_HIGH_WATER_MARK: f64 = 64.0 * 1024.0;
const BAD_REQUEST_RESPONSE: &[u8] = b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n";
const REQUEST_TIMEOUT_RESPONSE: &[u8] =
    b"HTTP/1.1 408 Request Timeout\r\nConnection: close\r\n\r\n";

fn abort_server_request(
    runtime: &mut Runtime<NodeHost>,
    request: RootId,
) -> Result<(), String> {
    let error = runtime.error_rooted("aborted").map_err(|error| error.to_string())?;
    let code = runtime.string_rooted("ECONNRESET");
    let code_key = runtime.string_rooted("code");
    let set_code = runtime.set_property_rooted(error, code_key, code, error);
    runtime.release_root(code_key);
    runtime.release_root(code);
    set_code.map_err(|error| error.to_string())?;
    let abort_key = runtime.string_rooted("_quenchAbort");
    let abort = runtime
        .get_property_rooted(request, abort_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(abort_key);
    let result = runtime.call_rooted(abort, request, &[error]);
    runtime.release_root(abort);
    runtime.release_root(error);
    result
        .map(|result| { runtime.release_root(result); })
        .map_err(|error| error.to_string())
}

fn reject_request(shared_state: &Rc<RefCell<SharedNodeState>>, socket: u64) -> Result<(), String> {
    let mut state = shared_state.borrow_mut();
    let Some(connection) = state.http.connections.get_mut(&socket) else {
        return Ok(());
    };
    connection.request_dispatched = true;
    // An application may have already responded or destroyed the exchange in
    // its request callback. Never append a second response to committed output.
    let committed = state.http.responses.values().any(|response| {
        response.socket == socket && response.lifecycle != super::state::ResponseLifecycle::Open
    });
    if !committed {
        net_shared_vm::write(&mut state.tcp, socket, BAD_REQUEST_RESPONSE)?;
    }
    net_shared_vm::end(&mut state.tcp, socket, false)
}

fn emit_client_error(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    socket_id: u64,
    code: &str,
) -> Result<bool, String> {
    let server = {
        let host = shared_state.borrow();
        let server_id = host
            .http
            .connections
            .get(&socket_id)
            .map(|connection| connection.server)
            .ok_or("HTTP client-error connection is unavailable")?;
        host.http
            .servers
            .get(&server_id)
            .map(|server| server.root)
            .ok_or("HTTP client-error server is unavailable")?
    };
    let socket = runtime.object_rooted().map_err(|error| error.to_string())?;
    set_bool(runtime, socket, "readable", true)?;
    set_bool(runtime, socket, "writable", true)?;
    set_number(runtime, socket, "__quenchHttpSocketId", socket_id as f64)?;
    let destroy = runtime
        .host_function(crate::host::shared_vm::operation(
            "httpServerSocketDestroyById",
        ))
        .map_err(|error| error.to_string())?;
    set_named(runtime, socket, "destroy", destroy)?;
    let write = runtime
        .host_function(crate::host::shared_vm::operation(
            "httpServerSocketWriteById",
        ))
        .map_err(|error| error.to_string())?;
    set_named(runtime, socket, "write", write)?;
    let end = runtime
        .host_function(crate::host::shared_vm::operation("httpServerSocketEnd"))
        .map_err(|error| error.to_string())?;
    set_named(runtime, socket, "end", end)?;

    let error = runtime
        .error_rooted("Parse Error")
        .map_err(|error| error.to_string())?;
    if let Err(error) = set_text(runtime, error, "code", code) {
        runtime.release_root(socket);
        return Err(error);
    }
    if code == "HPE_PAUSED_H2_UPGRADE" {
        if let Err(message) = set_number(runtime, error, "bytesParsed", 24.0) {
            runtime.release_root(socket);
            runtime.release_root(error);
            return Err(message);
        }
    }
    let emit_key = runtime.string_rooted("emit");
    let emit = runtime
        .get_property_rooted(server, emit_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(emit_key);
    let event = runtime.string_rooted("clientError");
    let result = runtime.call_rooted(emit, server, &[event, error, socket]);
    runtime.release_root(event);
    runtime.release_root(emit);
    runtime.release_root(error);
    runtime.release_root(socket);
    let result = match result {
        Ok(result) => {
            let handled = runtime
                .rooted_value(result)
                .and_then(Value::as_bool)
                .unwrap_or(false);
            runtime.release_root(result);
            Ok(handled)
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    };
    result
}

fn server_request_rules(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    socket: u64,
) -> Result<(bool, bool, bool, Option<usize>, usize), String> {
    let server_root = {
        let host = shared_state.borrow();
        let server_id = host
            .http
            .connections
            .get(&socket)
            .map(|connection| connection.server)
            .ok_or("HTTP request connection is unavailable")?;
        host.http
            .servers
            .get(&server_id)
            .map(|server| server.root)
            .ok_or("HTTP request server is unavailable")?
    };

    let require_key = runtime.string_rooted("requireHostHeader");
    let require_value = runtime
        .get_property_rooted(server_root, require_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(require_key);
    let require_host = runtime
        .rooted_value(require_value)
        .and_then(Value::as_bool)
        .unwrap_or(true);
    runtime.release_root(require_value);

    let validation_key = runtime.string_rooted("httpValidation");
    let validation_value = runtime
        .get_property_rooted(server_root, validation_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(validation_key);
    let validation = runtime
        .string_text_rooted(validation_value)
        .map_err(|error| error.to_string())?;
    runtime.release_root(validation_value);

    let insecure_key = runtime.string_rooted("insecureHTTPParser");
    let insecure_value = runtime
        .get_property_rooted(server_root, insecure_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(insecure_key);
    let insecure = runtime
        .rooted_value(insecure_value)
        .and_then(Value::as_bool)
        == Some(true);
    runtime.release_root(insecure_value);

    let max_headers_count_key = runtime.string_rooted("maxHeadersCount");
    let max_headers_count_value = runtime
        .get_property_rooted(server_root, max_headers_count_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(max_headers_count_key);
    let max_headers_count = runtime
        .rooted_value(max_headers_count_value)
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as usize);
    runtime.release_root(max_headers_count_value);

    let max_header_size_key = runtime.string_rooted("maxHeaderSize");
    let max_header_size_value = runtime
        .get_property_rooted(server_root, max_header_size_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(max_header_size_key);
    let max_header_size = runtime
        .rooted_value(max_header_size_value)
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value as usize)
        .unwrap_or(16 * 1024);
    runtime.release_root(max_header_size_value);

    Ok((
        require_host,
        insecure || matches!(validation.as_deref(), Some("relaxed" | "insecure")),
        insecure || validation.as_deref() == Some("insecure"),
        max_headers_count,
        max_header_size,
    ))
}

fn valid_request_header_value(value: &str, relaxed: bool) -> bool {
    value.chars().all(|character| {
        let code = character as u32;
        code <= 0xff
            && if relaxed {
                !matches!(code, 0x00 | 0x0a | 0x0d)
            } else {
                code == 0x09 || (0x20..=0x7e).contains(&code) || (0x80..=0xff).contains(&code)
            }
    })
}

pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let (pending_writes, pending_ends, pending_destroys) = {
        let host = runtime.host_mut();
        (
            std::mem::take(&mut host.net_pending_writes),
            std::mem::take(&mut host.net_pending_ends),
            std::mem::take(&mut host.net_pending_destroys),
        )
    };
    if !pending_writes.is_empty() || !pending_ends.is_empty() {
        let mut host = shared_state.borrow_mut();
        let mut deferred_writes = Vec::new();
        let mut deferred_ends = Vec::new();
        for (socket, bytes, raw) in pending_writes {
            if net_shared_vm::is_connected(&host.tcp, socket) {
                net_shared_vm::write_with_completion(&mut host.tcp, socket, &bytes, raw)?;
            } else if host.net_sockets.contains_key(&socket) {
                deferred_writes.push((socket, bytes, raw));
            }
        }
        for (socket, raw) in pending_ends {
            if net_shared_vm::is_connected(&host.tcp, socket) {
                net_shared_vm::end(&mut host.tcp, socket, raw)?;
            } else if host.net_sockets.contains_key(&socket) {
                deferred_ends.push((socket, raw));
            }
        }
        drop(host);
        let host = runtime.host_mut();
        host.net_pending_writes.extend(deferred_writes);
        host.net_pending_ends.extend(deferred_ends);
    }
    let destroyed = {
        let mut host = shared_state.borrow_mut();
        pending_destroys
            .into_iter()
            .filter_map(|(socket_id, was_connected)| {
                let socket = host.net_sockets.remove(&socket_id)?;
                host.net_closing_sockets.insert(socket_id, socket.root);
                let closed_server = socket.parent_server.and_then(|server_id| {
                    let closed = host.net_servers.get_mut(&server_id).is_some_and(|server| {
                        server.connections.remove(&socket_id);
                        server.closing && server.connections.is_empty()
                    });
                    closed
                        .then(|| {
                            host.net_servers
                                .remove(&server_id)
                                .map(|server| server.root)
                        })
                        .flatten()
                });
                Some((
                    socket.root,
                    socket.write_callbacks,
                    closed_server,
                    was_connected,
                ))
            })
            .collect::<Vec<_>>()
    };
    for (socket, callbacks, server, was_connected) in destroyed {
        if !callbacks.is_empty() {
            // Destruction has already delivered its stream error. Completing
            // a canceled write reports its callback error without emitting
            // another error from the writable half of this Duplex.
            let key = runtime.string_rooted("_writableState");
            let writable = runtime
                .get_property_rooted(socket, key)
                .map_err(|error| error.to_string())?;
            runtime.release_root(key);
            set_bool(runtime, writable, "errorEmitted", true)?;
            runtime.release_root(writable);
            let error = if was_connected {
                let error = net_socket_error_with_code(runtime, "write ECANCELED", "ECANCELED")?;
                set_text(runtime, error, "syscall", "write")?;
                #[cfg(unix)]
                set_number(runtime, error, "errno", -f64::from(libc::ECANCELED))?;
                error
            } else {
                net_socket_error_with_code(
                    runtime,
                    "Socket closed before the connection was established",
                    "ERR_SOCKET_CLOSED_BEFORE_CONNECTION",
                )?
            };
            for callback in callbacks {
                queue_write_callback(runtime, shared_state, callback, Some(error));
            }
            runtime.release_root(error);
        }
        // The socket root transferred to the close-completion projection.
        if let Some(server) = server {
            queue_net_event(runtime, shared_state, server, "close", &[])?;
            runtime.release_root(server);
        }
    }
    let events = {
        let mut host = shared_state.borrow_mut();
        net_shared_vm::poll(&mut host.tcp)
    };
    let mut progressed = false;
    progressed |= emit_listening(runtime, program, shared_state)?;
    progressed |= emit_net_server_events(runtime, shared_state)?;
    for event in events {
        match event {
            net_shared_vm::TransportEvent::Accepted { listener, socket } => {
                let (server_id, server_root, net_server, native_handle, incoming_factory) = {
                    let host = shared_state.borrow();
                    let server_id = host.http.servers.iter().find_map(|(id, server)| {
                        (server.listener == Some(listener)).then_some(*id)
                    });
                    (
                        server_id,
                        server_id
                            .and_then(|id| host.http.servers.get(&id).map(|server| server.root)),
                        host.net_servers.get(&listener).map(|server| server.root),
                        host.native_tcp_listeners.get(&listener).copied(),
                        host.http.incoming_factory,
                    )
                };
                if let Some(server_id) = server_id {
                    let mut host = shared_state.borrow_mut();
                    net_shared_vm::release_delivery(&mut host.tcp, socket);
                    host.http.connections.insert(
                        socket,
                        ServerConnection {
                            server: server_id,
                            received: Vec::new(),
                            request_dispatched: false,
                            request_bytes: 0,
                            peer_ended: false,
                            headers_started: Some(std::time::Instant::now()),
                            headers_timeout_sent: false,
                            request_started: Some(std::time::Instant::now()),
                            request_timeout_sent: false,
                            upgraded: false,
                            socket_root: None,
                            request_root: None,
                        },
                    );
                    if let Some(server) = host.http.servers.get_mut(&server_id) {
                        server.connections.insert(socket);
                    }
                    drop(host);
                    // Build and retain the socket facade before request
                    // dispatch so the server's connection event and later
                    // IncomingMessage share the same public Socket object.
                    if let Some(server_root) = server_root {
                        let incoming_factory = incoming_factory
                            .ok_or("HTTP incoming-message factory is unavailable")?;
                        let undefined = runtime.root(Value::UNDEFINED);
                        let transport = runtime.root(Value::number(socket as f64));
                        let accepted_socket = match runtime
                            .call_rooted(incoming_factory, undefined, &[transport])
                        {
                            Ok(socket) => socket,
                            Err(error) => {
                                let message = runtime.format_error(program, &error.error);
                                if let Some(exception) = error.exception {
                                    runtime.release_root(exception);
                                }
                                runtime.release_root(undefined);
                                runtime.release_root(transport);
                                return Err(message);
                            }
                        };
                        runtime.release_root(undefined);
                        runtime.release_root(transport);
                        let result = emit_event(
                            runtime,
                            program,
                            server_root,
                            "connection",
                            &[accepted_socket],
                        );
                        runtime.release_root(accepted_socket);
                        result?;
                    }
                    progressed = true;
                } else if net_server.is_some() || native_handle.is_some() {
                    let server_handle = if let Some(handle) = native_handle {
                        runtime.root(
                            runtime
                                .rooted_value(handle)
                                .ok_or("native TCP listener expired")?,
                        )
                    } else {
                        let key = runtime.string_rooted("_handle");
                        let handle = runtime
                            .get_property_rooted(net_server.expect("public server"), key)
                            .map_err(|error| error.to_string())?;
                        runtime.release_root(key);
                        handle
                    };
                    let create = runtime
                        .host_function(crate::host::shared_vm::operation("netSocketCreateHandle"))
                        .map_err(|error| error.to_string())?;
                    let undefined = runtime.root(Value::UNDEFINED);
                    let id = runtime.root(Value::number(socket as f64));
                    let client_handle =
                        runtime.call_rooted(create, undefined, &[id, server_handle]);
                    runtime.release_root(create);
                    runtime.release_root(undefined);
                    runtime.release_root(id);
                    let client_handle = client_handle.map_err(|error| error.to_string())?;

                    let success = runtime.root(Value::number(0.0));
                    if let Some(resource) =
                        super::super::native_async_wrap::tcp_resource(shared_state, listener)
                    {
                        let key = runtime.string_rooted("onconnection");
                        let receiver = runtime.root(
                            runtime
                                .rooted_value(server_handle)
                                .expect("retained native listener"),
                        );
                        let resource_root = runtime.root(
                            runtime
                                .rooted_value(server_handle)
                                .expect("retained native listener resource"),
                        );
                        let status = runtime.root(Value::number(0.0));
                        let client = runtime.root(
                            runtime
                                .rooted_value(client_handle)
                                .expect("accepted native TCP handle"),
                        );
                        let callback = runtime.root(Value::UNDEFINED);
                        let invoked=super::super::process_shared_vm::invoke_async_callback(runtime,program,
                            crate::modules::shared_event_loop::SharedCallback {
                                execution:Some(crate::modules::shared_event_loop::CallbackExecution {
                                    lifetime:crate::modules::shared_event_loop::CallbackLifetime::Resource,
                                    optional_callback:Some(key),id:resource.id,resource:resource_root,
                                }),callback,receiver,args:vec![status,client],
                            });
                        net_shared_vm::release_delivery(&mut shared_state.borrow_mut().tcp, socket);
                        invoked?;
                    } else {
                        queue_net_method_completion(
                            runtime,
                            shared_state,
                            server_handle,
                            "onconnection",
                            &[success, client_handle],
                            Some(NetDelivery::Connection(socket)),
                        )?;
                    }
                    runtime.release_root(success);
                    runtime.release_root(client_handle);
                    runtime.release_root(server_handle);
                    progressed = true;
                } else {
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                }
            }
            net_shared_vm::TransportEvent::Secure { socket } => {
                let root = shared_state
                    .borrow()
                    .net_sockets
                    .get(&socket)
                    .map(|socket| socket.root);
                if let Some(root) = root {
                    queue_net_event(runtime, shared_state, root, "secure", &[])?;
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::NativeConnectComplete { request, status } => {
                progressed |=
                    complete_native_request(runtime, program, shared_state, request, status)?;
            }
            net_shared_vm::TransportEvent::NativeShutdownComplete { request } => {
                progressed |= complete_native_request(runtime, program, shared_state, request, 0)?;
            }
            net_shared_vm::TransportEvent::NativeWriteComplete { request } => {
                progressed |= complete_native_request(runtime, program, shared_state, request, 0)?;
            }
            net_shared_vm::TransportEvent::WriteComplete { socket } => {
                let callback = shared_state
                    .borrow_mut()
                    .net_sockets
                    .get_mut(&socket)
                    .and_then(|socket| socket.write_callbacks.pop_front());
                if let Some(callback) = callback {
                    queue_write_callback(runtime, shared_state, callback, None);
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::Connected { socket } => {
                let (request, net_root) = {
                    let host = shared_state.borrow();
                    (
                        host.http
                            .clients
                            .get(&socket)
                            .map(|client| client.request.clone()),
                        host.net_sockets.get(&socket).map(|socket| socket.root),
                    )
                };
                if let Some(request) = request {
                    // The HTTP client shares this socket with the public
                    // net.Socket. Publish the connect event so Node-visible
                    // state (including `connecting` and socket timeouts)
                    // advances. Its deferred connect callback starts the
                    // socket's reader; starting it here as well races that
                    // callback and causes an EALREADY read failure.
                    if let Some(root) = net_root {
                        queue_connection_event(
                            runtime,
                            shared_state,
                            root,
                            "connect",
                            &[],
                            socket,
                        )?;
                    }
                    // A public net.Socket starts reads in its connect callback;
                    // HTTP owns and parses its data while this client record is
                    // active. Sockets without a public net.Socket have no such
                    // callback, so the HTTP transport starts their reads here.
                    if net_root.is_none()
                        && !net_shared_vm::read_active(&shared_state.borrow().tcp, socket)
                    {
                        net_shared_vm::start_read(&mut shared_state.borrow_mut().tcp, socket)
                            .map_err(|error| format!("HTTP client read start failed: {error}"))?;
                    }
                    net_shared_vm::release_delivery(&mut shared_state.borrow_mut().tcp, socket);
                    net_shared_vm::write(&mut shared_state.borrow_mut().tcp, socket, &request)
                        .map_err(|error| format!("HTTP client write failed: {error}"))?;
                    progressed = true;
                } else if let Some(root) = net_root {
                    queue_connection_event(runtime, shared_state, root, "connect", &[], socket)?;
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::ConnectError { socket, failure } => {
                let message = failure.message();
                let client = shared_state.borrow_mut().http.clients.remove(&socket);
                if let Some(client) = client {
                    fail_client_exchange(runtime, program, shared_state, client, &message)?;
                } else {
                    let net_root = shared_state
                        .borrow()
                        .net_sockets
                        .get(&socket)
                        .map(|net_socket| net_socket.root);
                    if let Some(net_root) = net_root {
                        let error = tcp_connect_error(runtime, &failure)?;
                        queue_net_method(runtime, shared_state, net_root, "destroy", &[error])?;
                        runtime.release_root(error);
                    }
                }
                progressed = true;
            }
            #[cfg(unix)]
            net_shared_vm::TransportEvent::LocalConnectError {
                socket,
                error,
                path,
            } => {
                let root = shared_state
                    .borrow()
                    .net_sockets
                    .get(&socket)
                    .map(|socket| socket.root);
                if let Some(root) = root {
                    let errno = error.raw_os_error().map(|errno| -f64::from(errno));
                    let code = errno
                        .map(crate::modules::util_system_error_shared_vm::system_error_name)
                        .unwrap_or_else(|| "UNKNOWN".to_owned());
                    let error = net_socket_error_with_code(
                        runtime,
                        &format!("connect {code} {path}"),
                        &code,
                    )?;
                    if let Some(errno) = errno {
                        set_number(runtime, error, "errno", errno)?;
                    }
                    set_text(runtime, error, "syscall", "connect")?;
                    set_text(runtime, error, "address", &path)?;
                    queue_net_method(runtime, shared_state, root, "destroy", &[error])?;
                    runtime.release_root(error);
                }
                progressed = true;
            }
            net_shared_vm::TransportEvent::Data { socket, bytes } => {
                if queue_native_read(
                    runtime,
                    shared_state,
                    socket,
                    Some(&bytes),
                    bytes.len() as i32,
                )? {
                    progressed = true;
                    continue;
                }
                let net_socket = {
                    let host = shared_state.borrow();
                    // An Agent-owned ClientRequest uses a public net.Socket
                    // as its transport, but HTTP owns reads while a response
                    // is active. Sending those bytes to the socket's generic
                    // Readable path bypasses the HTTP response parser.
                    if host.http.clients.contains_key(&socket) {
                        None
                    } else {
                        host.net_sockets.get(&socket).map(|socket| {
                            (socket.root, socket.encoding.clone().unwrap_or_default())
                        })
                    }
                };
                if let Some((root, _encoding)) = net_socket {
                    let chunk = buffer_from_bytes(runtime, &bytes)?;
                    queue_net_method_completion(
                        runtime,
                        shared_state,
                        root,
                        "push",
                        &[chunk],
                        Some(NetDelivery::Data(socket, bytes.len())),
                    )?;
                    runtime.release_root(chunk);
                    progressed = true;
                    continue;
                }
                net_shared_vm::release_delivery(&mut shared_state.borrow_mut().tcp, socket);
                if shared_state.borrow().http.connections.contains_key(&socket) {
                    let upgraded_socket = {
                        let host = shared_state.borrow();
                        host.http.connections.get(&socket).and_then(|connection| {
                            connection
                                .upgraded
                                .then_some(connection.socket_root)
                                .flatten()
                        })
                    };
                    if let Some(upgraded_socket) = upgraded_socket {
                        let chunk = buffer_from_bytes(runtime, &bytes)?;
                        emit_event(runtime, program, upgraded_socket, "data", &[chunk])?;
                        runtime.release_root(chunk);
                        progressed = true;
                        continue;
                    }
                    let dispatch = {
                        let mut host = shared_state.borrow_mut();
                        let Some(connection) = host.http.connections.get_mut(&socket) else {
                            continue;
                        };
                        if !connection.request_dispatched && connection.received.is_empty() {
                            connection.headers_started = Some(std::time::Instant::now());
                            connection.headers_timeout_sent = false;
                            if connection.request_started.is_none() {
                                connection.request_started = Some(std::time::Instant::now());
                                connection.request_timeout_sent = false;
                            }
                        }
                        connection.received.extend_from_slice(&bytes);
                        if connection.request_dispatched {
                            None
                        } else {
                            let head_end = super::protocol::head_size(&connection.received);
                            if head_end.is_none() && connection.received.len() > REQUEST_HEAD_LIMIT
                                || head_end.is_some_and(|end| end > REQUEST_HEAD_LIMIT)
                            {
                                return Err(
                                    "HTTP request head exceeds the shared parser limit".into()
                                );
                            }
                            super::protocol::request(&connection.received)
                        }
                    };
                    if let Some(message) = dispatch {
                        let (
                            require_host_header,
                            relaxed_headers,
                            insecure,
                            max_headers_count,
                            max_header_size,
                        ) =
                            server_request_rules(runtime, shared_state, socket)?;
                        let header_overflow = message.head_size > max_header_size
                            || max_headers_count.is_some_and(|count| {
                                count > 0 && message.raw_headers.len() > count
                            });
                        let invalid_headers = message
                            .raw_headers
                            .iter()
                            .any(|(_, value)| !valid_request_header_value(value, relaxed_headers));
                        let is_upgrade = message.headers.iter().any(|(name, value)| {
                            name.eq_ignore_ascii_case("upgrade") && !value.trim().is_empty()
                        }) && message.headers.iter().any(|(name, value)| {
                            name.eq_ignore_ascii_case("connection")
                                && value
                                    .split(',')
                                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
                        });
                        let missing_host = require_host_header
                            && message.version == "1.1"
                            && !is_upgrade
                            && !message
                                .headers
                                .iter()
                                .any(|(name, _)| name.eq_ignore_ascii_case("host"));
                        let framing_error = match &message.body {
                            Err(
                                super::protocol::RequestFramingError::InvalidTransferCodingOrder,
                            )
                            | Err(super::protocol::RequestFramingError::MissingFinalChunked) => {
                                Some("HPE_INVALID_TRANSFER_ENCODING")
                            }
                            Err(super::protocol::RequestFramingError::InvalidChunkedBody) => {
                                Some("HPE_INVALID_CHUNK_SIZE")
                            }
                            Ok(_) => None,
                        };
                        let error_code = message
                            .parse_error
                            .or_else(|| {
                                if header_overflow {
                                    Some("HPE_HEADER_OVERFLOW")
                                } else if invalid_headers || missing_host {
                                    Some("HPE_INVALID_HEADER_TOKEN")
                                } else {
                                    None
                                }
                            })
                            .or(framing_error);
                        if !insecure {
                            if let Some(error_code) = error_code {
                                let handled = emit_client_error(
                                    runtime,
                                    program,
                                    shared_state,
                                    socket,
                                    error_code,
                                )?;
                                if !handled {
                                    reject_request(shared_state, socket)?;
                                } else {
                                    let mut host = shared_state.borrow_mut();
                                    if let Some(connection) = host.http.connections.get_mut(&socket)
                                    {
                                        connection.request_dispatched = true;
                                    }
                                }
                                progressed = true;
                                continue;
                            }
                        }
                        if message.headers.iter().any(|(name, value)| {
                            name.eq_ignore_ascii_case("expect")
                                && value
                                    .split(',')
                                    .any(|item| item.trim().eq_ignore_ascii_case("100-continue"))
                        }) {
                            // The default Node server path acknowledges
                            // Expect: 100-continue before dispatching the
                            // request listener. Header lookup is case
                            // insensitive, including raw mixed-case input.
                            net_shared_vm::write(
                                &mut shared_state.borrow_mut().tcp,
                                socket,
                                b"HTTP/1.1 100 Continue\r\n\r\n",
                            )?;
                        }
                        let body_invalid = message.body.is_err();
                        let request_async_id =
                            crate::modules::async_hooks_shared_vm::create_context(shared_state);
                        let (server_root, response_id, incoming_factory) = {
                            let mut host = shared_state.borrow_mut();
                            let server_id = host.http.connections[&socket].server;
                            let server_root = host
                                .http
                                .servers
                                .get(&server_id)
                                .map(|server| server.root)
                                .ok_or_else(|| "HTTP server root was released early".to_owned())?;
                            let response_id =
                                host.http.response_id().map_err(|error| error.to_owned())?;
                            let incoming_factory = host.http.incoming_factory.ok_or_else(|| {
                                "HTTP incoming-message factory is unavailable".to_owned()
                            })?;
                            let http10 = message.version == "1.0";
                            let close_after_response = http10
                                || message.headers.iter().any(|(name, value)| {
                                    name.eq_ignore_ascii_case("connection")
                                        && value.eq_ignore_ascii_case("close")
                                });
                            host.http
                                .connections
                                .get_mut(&socket)
                                .unwrap()
                                .request_dispatched = true;
                            let request_bytes = host
                                .http
                                .connections
                                .get(&socket)
                                .map(|connection| {
                                    super::protocol::request_wire_size(&connection.received)
                                })
                                .unwrap_or(0);
                            if let Some(connection) = host.http.connections.get_mut(&socket) {
                                connection.request_bytes = request_bytes;
                            }
                            host.http.responses.insert(
                                response_id,
                                Response {
                                    socket,
                                    async_id: request_async_id,
                                    headers: Vec::new(),
                                    body: Default::default(),
                                    send_date: true,
                                    http10,
                                    close_after_response,
                                    removed_headers: Default::default(),
                                    use_chunked_encoding_by_default: !http10
                                        || message.headers.iter().any(|(name, value)| {
                                            name.eq_ignore_ascii_case("te")
                                                && value.split(',').any(|token| {
                                                    token.trim().eq_ignore_ascii_case("chunked")
                                                })
                                        }),
                                    lifecycle: super::state::ResponseLifecycle::Open,
                                },
                            );
                            (server_root, response_id, incoming_factory)
                        };
                        let socket_object = {
                            let accepted_socket = shared_state
                                .borrow()
                                .http
                                .connections
                                .get(&socket)
                                .and_then(|connection| connection.socket_root);
                            if let Some(accepted_socket) = accepted_socket {
                                let value = runtime
                                    .rooted_value(accepted_socket)
                                    .ok_or_else(|| {
                                        "HTTP accepted socket root was released early".to_owned()
                                    })?;
                                runtime.root(value)
                            } else {
                                runtime
                                    .object_rooted()
                                    .map_err(|error| error.to_string())?
                            }
                        };
                        let request = match request_object(
                            runtime,
                            message.method,
                            message.target,
                            message.version,
                            message.headers,
                            message.raw_headers,
                            message.raw_trailers,
                            socket_object,
                        ) {
                            Ok(request) => request,
                            Err(error) => {
                                runtime.release_root(socket_object);
                                return Err(error);
                            }
                        };
                        let body_text = latin1_text(message.body.as_deref().unwrap_or_default());
                        let body = runtime.string_rooted(&body_text);
                        let undefined = runtime.root(Value::UNDEFINED);
                        let open = runtime.root(if body_invalid {
                            Value::TRUE
                        } else {
                            Value::FALSE
                        });
                        let transport = runtime.root(Value::number(socket as f64));
                        let server_side = runtime.root(Value::TRUE);
                        let readable = match runtime.call_rooted(
                            incoming_factory,
                            undefined,
                            &[request, body, open, transport, server_side, server_root],
                        ) {
                            Ok(readable) => readable,
                            Err(error) => {
                                let message = runtime.format_error(program, &error.error);
                                if let Some(exception) = error.exception {
                                    runtime.release_root(exception);
                                }
                                runtime.release_root(request);
                                runtime.release_root(body);
                                runtime.release_root(open);
                                runtime.release_root(transport);
                                runtime.release_root(server_side);
                                runtime.release_root(undefined);
                                runtime.release_root(socket_object);
                                return Err(message);
                            }
                        };
                        runtime.release_root(request);
                        runtime.release_root(body);
                        runtime.release_root(open);
                        runtime.release_root(transport);
                        runtime.release_root(server_side);
                        runtime.release_root(undefined);
                        if is_upgrade {
                            let socket_key = runtime.string_rooted("socket");
                            let accepted_socket = runtime
                                .get_property_rooted(readable, socket_key)
                                .map_err(|error| error.to_string())?;
                            runtime.release_root(socket_key);
                            let head_bytes = {
                                let mut host = shared_state.borrow_mut();
                                let connection = host
                                    .http
                                    .connections
                                    .get_mut(&socket)
                                    .ok_or_else(|| "upgraded HTTP socket disappeared".to_owned())?;
                                let offset = connection
                                    .request_bytes
                                    .min(connection.received.len());
                                let head = connection.received[offset..].to_vec();
                                connection.received.clear();
                                connection.upgraded = true;
                                connection.request_started = None;
                                connection.headers_started = None;
                                host.http.responses.remove(&response_id);
                                head
                            };
                            let head = buffer_from_bytes(runtime, &head_bytes)?;
                            let upgrade_result = {
                                let _scope =
                                    crate::modules::async_hooks_shared_vm::enter_resource_context(
                                        shared_state,
                                        request_async_id,
                                        readable,
                                    );
                                emit_event(
                                    runtime,
                                    program,
                                    server_root,
                                    "upgrade",
                                    &[readable, accepted_socket, head],
                                )
                            };
                            crate::modules::async_hooks_shared_vm::clear_context(
                                runtime,
                                shared_state,
                                request_async_id,
                            );
                            upgrade_result?;
                            if !head_bytes.is_empty() {
                                emit_event(
                                    runtime,
                                    program,
                                    accepted_socket,
                                    "data",
                                    &[head],
                                )?;
                            }
                            runtime.release_root(head);
                            runtime.release_root(accepted_socket);
                            runtime.release_root(readable);
                            runtime.release_root(socket_object);
                            progressed = true;
                            continue;
                        }
                        let id = runtime.root(Value::number(response_id as f64));
                        let status = runtime.root(Value::number(200.0));
                        let undefined = runtime.root(Value::UNDEFINED);
                        let response_constructor_key =
                            runtime.string_rooted("_quenchServerResponse");
                        let custom_response_constructor = runtime
                            .get_property_rooted(server_root, response_constructor_key)
                            .map_err(|error| error.to_string())?;
                        runtime.release_root(response_constructor_key);
                        let response_result = runtime.construct_rooted(
                            custom_response_constructor,
                            custom_response_constructor,
                            &[id, status, readable, server_root, socket_object],
                        );
                        runtime.release_root(custom_response_constructor);
                        let response = match response_result {
                            Ok(response) => response,
                            Err(error) => {
                                runtime.release_root(readable);
                                runtime.release_root(id);
                                runtime.release_root(status);
                                runtime.release_root(undefined);
                                runtime.release_root(socket_object);
                                let message = runtime.format_error(program, &error.error);
                                if let Some(exception) = error.exception {
                                    runtime.release_root(exception);
                                }
                                return Err(message);
                            }
                        };
                        let diagnostic_message = match diagnostics_message(
                            runtime,
                            readable,
                            response,
                            server_root,
                            socket_object,
                        ) {
                            Ok(message) => message,
                            Err(error) => {
                                runtime.release_root(readable);
                                runtime.release_root(response);
                                runtime.release_root(id);
                                runtime.release_root(status);
                                runtime.release_root(undefined);
                                runtime.release_root(socket_object);
                                return Err(error);
                            }
                        };
                        let delivered = {
                            // Keep a per-request object visible while dispatching
                            // the listener. Using the server itself would share
                            // executionAsyncResource() state across concurrent
                            // requests on the same server.
                            let _scope =
                                crate::modules::async_hooks_shared_vm::enter_resource_context(
                                    shared_state,
                                    request_async_id,
                                    readable,
                                );
                            let diagnostic_result = crate::modules::diagnostics_channel_shared_vm::publish_named_runtime(
                                runtime,
                                program,
                                "http.server.request.start",
                                diagnostic_message,
                            );
                            match diagnostic_result {
                                Ok(()) => emit_event(
                                    runtime,
                                    program,
                                    server_root,
                                    "request",
                                    &[readable, response],
                                ),
                                Err(error) => Err(error),
                            }
                        };
                        runtime.release_root(diagnostic_message);
                        let retained_request = duplicate_root(runtime, readable)?;
                        let (stored_request, previous_request) = shared_state
                            .borrow_mut()
                            .http
                            .connections
                            .get_mut(&socket)
                            .map(|connection| {
                                (true, connection.request_root.replace(retained_request))
                            })
                            .unwrap_or((false, None));
                        if !stored_request {
                            runtime.release_root(retained_request);
                        }
                        if let Some(previous_request) = previous_request {
                            runtime.release_root(previous_request);
                        }
                        runtime.release_root(readable);
                        runtime.release_root(response);
                        runtime.release_root(id);
                        runtime.release_root(status);
                        runtime.release_root(undefined);
                        runtime.release_root(socket_object);
                        delivered?;
                        if body_invalid {
                            reject_request(shared_state, socket)?;
                        }
                        progressed = true;
                    }
                } else if shared_state.borrow().http.clients.contains_key(&socket) {
                    let (progress, request_id, async_id, request_root, response_root) = {
                        let mut host = shared_state.borrow_mut();
                        let client = host.http.clients.get_mut(&socket).unwrap();
                        let progress = client.response_parser.push(&bytes);
                        if client.response_parser.pending_head_len() > RESPONSE_HEAD_LIMIT {
                            return Err("HTTP response head exceeds the shared parser limit".into());
                        }
                        (
                            progress,
                            client.request_id,
                            client.async_id,
                            client.request_root,
                            client.response_root,
                        )
                    };
                    let progress = match progress {
                        Ok(progress) => progress,
                        Err(code) if code.starts_with("HPE_") => {
                            // End the RefCell borrow before destroying the
                            // request: destroy() can synchronously invoke
                            // user listeners, which enter the async context.
                            let client = with_released_shared_borrow(shared_state, |host| {
                                host.http.clients.remove(&socket)
                            });
                            if let Some(client) = client {
                                fail_client_exchange_with_code(
                                    runtime,
                                    program,
                                    shared_state,
                                    client,
                                    "Parse Error",
                                    &code,
                                )?;
                            }
                            // The parser failure happens while the TCP socket
                            // is still open. Request.destroy() emits the
                            // ClientRequest error, but this shared transport
                            // also needs an explicit close so the peer and
                            // Agent cannot keep the failed exchange alive.
                            net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                            progressed = true;
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    for information in progress.information.iter().cloned() {
                        deliver_client_information(
                            runtime,
                            program,
                            shared_state,
                            async_id,
                            request_root,
                            information,
                        )?;
                    }
                    let progressed_response = !progress.information.is_empty()
                        || progress.head.is_some()
                        || !progress.body_chunks.is_empty();
                    let response_root = match (response_root, progress.head) {
                        (Some(response_root), _) => Some(response_root),
                        (None, Some(message)) => {
                            let response_root = deliver_client_response_head(
                                runtime,
                                program,
                                shared_state,
                                request_id,
                                async_id,
                                request_root,
                                message,
                            )?;
                            let mut host = shared_state.borrow_mut();
                            if let Some(client) = host.http.clients.get_mut(&socket) {
                                client.response_root = Some(response_root);
                                Some(response_root)
                            } else {
                                runtime.release_root(response_root);
                                None
                            }
                        }
                        (None, None) => None,
                    };
                    if let Some(response_root) = response_root {
                        set_incoming_trailers(
                            runtime,
                            response_root,
                            progress.raw_trailers.clone(),
                        )?;
                        deliver_client_body(
                            runtime,
                            program,
                            shared_state,
                            response_root,
                            async_id,
                            &progress.body_chunks,
                        )?;
                    }
                    if progress.complete {
                        // Delivering the response head can run guest callbacks. A
                        // callback may destroy the agent and consume this exchange
                        // before the parser reports completion. Removal is the
                        // exchange's terminal transition, so a missing record here
                        // means another transition already owns its cleanup.
                        let client = {
                            let mut host = shared_state.borrow_mut();
                            host.http.clients.remove(&socket)
                        };
                        if let Some(client) = client {
                            let agent = client.agent;
                            let finish_result = if let Some(response_root) = client.response_root {
                                let result = finish_client_response(
                                    runtime,
                                    program,
                                    shared_state,
                                    response_root,
                                    client.async_id,
                                );
                                runtime.release_root(response_root);
                                result
                            } else {
                                Ok(())
                            };
                            {
                                let _scope = crate::modules::async_hooks_shared_vm::enter_context(
                                    shared_state,
                                    client.async_id,
                                );
                                emit_client_request_close(runtime, program, client.request_root)?;
                            }
                            crate::modules::async_hooks_shared_vm::complete_runtime_context(
                                runtime,
                                client.async_id,
                            );
                            runtime.release_root(client.request_root);
                            let agent_owns_socket = shared_state
                                .borrow()
                                .http
                                .agents
                                .values()
                                .any(|sockets| sockets.contains(&socket));
                            if !agent_owns_socket {
                                net_shared_vm::close_socket(
                                    &mut shared_state.borrow_mut().tcp,
                                    socket,
                                );
                            }
                            finish_result?;
                            if let Some(agent) = agent {
                                let next = {
                                    let mut host = shared_state.borrow_mut();
                                    if host.http.agents.contains_key(&agent) {
                                        host.http
                                            .agent_pending
                                            .get_mut(&agent)
                                            .and_then(|queue| queue.pop_front())
                                    } else {
                                        None
                                    }
                                };
                                if next.is_none()
                                    && !shared_state
                                        .borrow()
                                        .http
                                        .agent_keep_alive
                                        .get(&agent)
                                        .copied()
                                        .unwrap_or(false)
                                {
                                    let mut host = shared_state.borrow_mut();
                                    if let Some(sockets) = host.http.agents.get_mut(&agent) {
                                        sockets.remove(&socket);
                                    }
                                    host.http.agent_endpoints.remove(&(agent, socket));
                                    net_shared_vm::close_socket(&mut host.tcp, socket);
                                }
                                if let Some(next) = next {
                                    let reusable = shared_state
                                        .borrow()
                                        .http
                                        .agent_endpoints
                                        .get(&(agent, socket))
                                        .is_some_and(|(host, port)| {
                                            host == &next.host && *port == next.port
                                        });
                                    let next_socket = if reusable {
                                        socket
                                    } else {
                                        {
                                            let mut host = shared_state.borrow_mut();
                                            if let Some(sockets) = host.http.agents.get_mut(&agent)
                                            {
                                                sockets.remove(&socket);
                                            }
                                            host.http.agent_endpoints.remove(&(agent, socket));
                                            net_shared_vm::close_socket(&mut host.tcp, socket);
                                        }
                                        let next_socket = net_shared_vm::connect(
                                            &mut shared_state.borrow_mut().tcp,
                                            next.host.clone(),
                                            next.port,
                                        )
                                        .map_err(|error| {
                                            format!("HTTP agent queued connect failed: {error}")
                                        })?;
                                        let mut host = shared_state.borrow_mut();
                                        if let Some(sockets) = host.http.agents.get_mut(&agent) {
                                            sockets.insert(next_socket);
                                            host.http.agent_endpoints.insert(
                                                (agent, next_socket),
                                                (next.host.clone(), next.port),
                                            );
                                        }
                                        next_socket
                                    };
                                    if reusable {
                                        net_shared_vm::write(
                                            &mut shared_state.borrow_mut().tcp,
                                            next_socket,
                                            &next.request,
                                        )
                                        .map_err(
                                            |error| {
                                                format!("HTTP agent reuse write failed: {error}")
                                            },
                                        )?;
                                    }
                                    shared_state.borrow_mut().http.clients.insert(
                                        next_socket,
                                        super::state::Client {
                                            request_id: next.request_id,
                                            async_id: next.async_id,
                                            request_root: next.request_root,
                                            request: next.request,
                                            agent: Some(agent),
                                            response_parser:
                                                super::protocol::ResponseParser::for_method(
                                                    &next.method,
                                                ),
                                            response_root: None,
                                        },
                                    );
                                }
                            }
                        }
                        progressed = true;
                    } else if progressed_response {
                        progressed = true;
                    }
                }
            }
            net_shared_vm::TransportEvent::End { socket } => {
                let upgraded_socket = {
                    let host = shared_state.borrow();
                    host.http.connections.get(&socket).and_then(|connection| {
                        connection
                            .upgraded
                            .then_some(connection.socket_root)
                            .flatten()
                    })
                };
                if let Some(upgraded_socket) = upgraded_socket {
                    emit_event(runtime, program, upgraded_socket, "end", &[])?;
                    progressed = true;
                    continue;
                }
                let server_connection = {
                    let server_root = {
                        let host = shared_state.borrow();
                        host.http
                            .connections
                            .get(&socket)
                            .and_then(|connection| {
                                host.http
                                    .servers
                                    .get(&connection.server)
                                    .map(|server| server.root)
                            })
                    };
                    if let Some(server_root) = server_root {
                        let property = runtime.string_rooted("httpAllowHalfOpen");
                        let value = runtime
                            .get_property_rooted(server_root, property)
                            .map_err(|error| error.to_string())?;
                        runtime.release_root(property);
                        let allow = runtime
                            .rooted_value(value)
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        runtime.release_root(value);
                        Some(allow)
                    } else {
                        None
                    }
                };
                if let Some(allow_half_open) = server_connection {
                    let active_response = {
                        let host = shared_state.borrow();
                        host.http.responses.values().any(|response| {
                            response.socket == socket && !response.lifecycle.is_terminal()
                        })
                    };
                    if active_response || allow_half_open {
                        if active_response {
                            let request_root = shared_state
                                .borrow_mut()
                                .http
                                .connections
                                .get_mut(&socket)
                                .and_then(|connection| connection.request_root.take());
                            if let Some(request_root) = request_root {
                                let result = abort_server_request(runtime, request_root);
                                runtime.release_root(request_root);
                                result?;
                            }
                        }
                        if let Some(connection) =
                            shared_state.borrow_mut().http.connections.get_mut(&socket)
                        {
                            connection.peer_ended = true;
                        }
                        // Preserve the write side after peer EOF. Node waits
                        // for an active response to finish before ending it.
                        progressed = true;
                        continue;
                    }
                }
                if queue_native_read(
                    runtime,
                    shared_state,
                    socket,
                    None,
                    super::super::stream_binding::UV_EOF,
                )? {
                    progressed = true;
                    continue;
                }
                let net_root = {
                    let host = shared_state.borrow();
                    (!host.http.clients.contains_key(&socket))
                        .then(|| host.net_sockets.get(&socket).map(|socket| socket.root))
                        .flatten()
                };
                if let Some(root) = net_root {
                    let eof = runtime.root(Value::NULL);
                    queue_net_method_completion(
                        runtime,
                        shared_state,
                        root,
                        "push",
                        &[eof],
                        Some(NetDelivery::Eof),
                    )?;
                    runtime.release_root(eof);
                    progressed = true;
                    continue;
                }
                let client = shared_state.borrow_mut().http.clients.remove(&socket);
                let had_client = client.is_some();
                if let Some(mut client) = client {
                    match client.response_parser.finish() {
                        Ok(progress) => {
                            let mut response_root = client.response_root;
                            let result: Result<(), String> = (|| {
                                for information in progress.information.iter().cloned() {
                                    deliver_client_information(
                                        runtime,
                                        program,
                                        shared_state,
                                        client.async_id,
                                        client.request_root,
                                        information,
                                    )?;
                                }
                                if response_root.is_none() {
                                    if let Some(message) = progress.head {
                                        response_root = Some(deliver_client_response_head(
                                            runtime,
                                            program,
                                            shared_state,
                                            client.request_id,
                                            client.async_id,
                                            client.request_root,
                                            message,
                                        )?);
                                    }
                                }
                                if let Some(response_root) = response_root {
                                    set_incoming_trailers(
                                        runtime,
                                        response_root,
                                        progress.raw_trailers.clone(),
                                    )?;
                                    deliver_client_body(
                                        runtime,
                                        program,
                                        shared_state,
                                        response_root,
                                        client.async_id,
                                        &progress.body_chunks,
                                    )?;
                                    if progress.complete {
                                        finish_client_response(
                                            runtime,
                                            program,
                                            shared_state,
                                            response_root,
                                            client.async_id,
                                        )?;
                                    }
                                }
                                Ok(())
                            })();
                            if let Some(response_root) = response_root {
                                runtime.release_root(response_root);
                            }
                            runtime.release_root(client.request_root);
                            crate::modules::async_hooks_shared_vm::complete_runtime_context(
                                runtime,
                                client.async_id,
                            );
                            result?;
                        }
                        Err(_) => {
                            let message = if client.response_root.is_some() {
                                "aborted"
                            } else {
                                "socket hang up"
                            };
                            fail_client_exchange(
                                runtime,
                                program,
                                shared_state,
                                client,
                                message,
                            )?;
                        }
                    }
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                    progressed = true;
                }
                let (server_id, socket_root, request_root, contexts) = {
                    let mut host = shared_state.borrow_mut();
                    let connection = host.http.connections.remove(&socket);
                    let server_id = connection.as_ref().map(|connection| connection.server);
                    let socket_root = connection.as_ref().and_then(|connection| connection.socket_root);
                    let request_root = connection.and_then(|connection| connection.request_root);
                    let contexts = host
                        .http
                        .responses
                        .iter()
                        .filter_map(|(id, response)| {
                            (response.socket == socket).then_some((*id, response.async_id))
                        })
                        .collect::<Vec<_>>();
                    for (id, _) in &contexts {
                        host.http.responses.remove(id);
                    }
                    if let Some(server_id) = server_id {
                        if let Some(server) = host.http.servers.get_mut(&server_id) {
                            server.connections.remove(&socket);
                        }
                    }
                    (server_id, socket_root, request_root, contexts)
                };
                if let Some(request_root) = request_root {
                    runtime.release_root(request_root);
                }
                for (_, async_id) in contexts {
                    crate::modules::async_hooks_shared_vm::clear_context(
                        runtime,
                        shared_state,
                        async_id,
                    );
                }
                if server_id.is_some() {
                    if let Some(socket_root) = socket_root {
                        emit_event(runtime, program, socket_root, "close", &[])?;
                        runtime.release_root(socket_root);
                    }
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                    progressed = true;
                } else if !had_client {
                    let agent_owned = {
                        let mut host = shared_state.borrow_mut();
                        let http = &mut host.http;
                        let owned = http
                            .agents
                            .values_mut()
                            .fold(false, |owned, sockets| sockets.remove(&socket) || owned);
                        http.agent_endpoints
                            .retain(|(_, socket_id), _| *socket_id != socket);
                        if owned {
                            net_shared_vm::close_socket(&mut host.tcp, socket);
                        }
                        owned
                    };
                    progressed |= agent_owned;
                }
            }
            net_shared_vm::TransportEvent::DatagramCloseCompleted { id } => {
                progressed |= crate::modules::udp_binding::queue_close(runtime, shared_state, id)?;
            }
            net_shared_vm::TransportEvent::CloseCompleted { socket } => {
                cancel_native_requests(runtime, program, shared_state, socket, -libc::ECANCELED)?;
                let (server_connection, contexts) = {
                    let mut host = shared_state.borrow_mut();
                    let connection = host.http.connections.remove(&socket);
                    let server_connection = connection.map(|connection| {
                        if let Some(server) = host.http.servers.get_mut(&connection.server) {
                            server.connections.remove(&socket);
                        }
                        (
                            connection.server,
                            connection.socket_root,
                            connection.request_root,
                        )
                    });
                    let contexts = host
                        .http
                        .responses
                        .iter()
                        .filter_map(|(id, response)| {
                            (response.socket == socket).then_some((*id, response.async_id))
                        })
                        .collect::<Vec<_>>();
                    for (id, _) in &contexts {
                        host.http.responses.remove(id);
                    }
                    (server_connection, contexts)
                };
                for (_, async_id) in contexts {
                    crate::modules::async_hooks_shared_vm::clear_context(
                        runtime,
                        shared_state,
                        async_id,
                    );
                }
                if let Some((_, socket_root, request_root)) = server_connection {
                    if let Some(request_root) = request_root {
                        runtime.release_root(request_root);
                    }
                    if let Some(socket_root) = socket_root {
                        emit_event(runtime, program, socket_root, "close", &[])?;
                        runtime.release_root(socket_root);
                    }
                }
                let reader = shared_state
                    .borrow_mut()
                    .native_stream
                    .readers
                    .remove(&socket);
                if let Some(reader) = reader {
                    runtime.release_root(reader);
                }
                let (root, callback) = {
                    let mut state = shared_state.borrow_mut();
                    (
                        state.net_closing_sockets.remove(&socket),
                        state.net_socket_close_completed,
                    )
                };
                let raw_callback = shared_state
                    .borrow_mut()
                    .net_handle_close_callbacks
                    .remove(&socket);
                if let Some(handle) = raw_callback {
                    let callback = runtime
                        .host_function(crate::host::shared_vm::operation("netHandleCloseComplete"))
                        .map_err(|error| error.to_string())?;
                    let execution = if let Some(resource) =
                        crate::modules::native_async_wrap::tcp_resource(shared_state, socket)
                    {
                        let owner = runtime.root(
                            runtime
                                .rooted_value(handle)
                                .ok_or("native close handle expired")?,
                        );
                        let symbol = shared_state
                            .borrow()
                            .net_handle_onclose_symbol
                            .ok_or("native TCP close symbol missing")?;
                        let key = runtime.root(
                            runtime
                                .rooted_value(symbol)
                                .ok_or("native TCP close symbol expired")?,
                        );
                        Some(crate::modules::shared_event_loop::CallbackExecution {
                            id: resource.id,
                            resource: owner,
                            lifetime:
                                crate::modules::shared_event_loop::CallbackLifetime::Operation,
                            optional_callback: Some(key),
                        })
                    } else {
                        None
                    };
                    shared_state.borrow_mut().scheduler.queue_shared_next_tick(
                        crate::modules::shared_event_loop::SharedCallback {
                            execution,
                            callback,
                            receiver: handle,
                            args: Vec::new(),
                        },
                    );
                    progressed = true;
                }
                if let Some(root) = root {
                    let callback = callback.ok_or("native socket close callback unavailable")?;
                    let callback = runtime.root(
                        runtime
                            .rooted_value(callback)
                            .ok_or("socket close callback expired")?,
                    );
                    let receiver = runtime.root(Value::UNDEFINED);
                    // Transfer the retained socket root into the scheduled callback.
                    shared_state.borrow_mut().scheduler.queue_shared_next_tick(
                        crate::modules::shared_event_loop::SharedCallback {
                            execution: None,
                            callback,
                            receiver,
                            args: vec![root, runtime.root(Value::TRUE)],
                        },
                    );
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::Error {
                socket,
                error,
                syscall,
            } => {
                let raw = shared_state
                    .borrow()
                    .native_stream
                    .readers
                    .contains_key(&socket)
                    || shared_state
                        .borrow()
                        .native_stream
                        .requests
                        .iter()
                        .any(|(_, pending)| pending.socket == socket);
                if raw {
                    let status = -error.raw_os_error().unwrap_or(libc::EIO);
                    cancel_native_requests(runtime, program, shared_state, socket, status)?;
                    if syscall == "read" {
                        queue_native_read(runtime, shared_state, socket, None, status)?;
                    }
                    progressed = true;
                    continue;
                }
                // The OS error owns errno; presentation never recovers it from
                // localized text. Derive the Node code and syscall message.
                let ssl_error = error
                    .get_ref()
                    .and_then(|error| error.downcast_ref::<openssl::ssl::Error>());
                let native_io = ssl_error
                    .and_then(|error| error.io_error())
                    .unwrap_or(&error);
                let errno = native_io.raw_os_error().map(|errno| -f64::from(errno));
                let code = errno
                    .map(crate::modules::util_system_error_shared_vm::system_error_name)
                    .unwrap_or_else(|| "EIO".to_owned());
                let message = format!("{syscall} {code}");
                let net_root = shared_state
                    .borrow()
                    .net_sockets
                    .get(&socket)
                    .map(|socket| socket.root);
                if let Some(net_root) = net_root {
                    if let Some(stack) = ssl_error.and_then(|error| error.ssl_error()) {
                        shared_state
                            .borrow_mut()
                            .tls_contexts
                            .transport_errors
                            .push_back((socket, stack.clone()));
                        let callback = runtime
                            .host_function(crate::host::shared_vm::operation(
                                "tlsSocketTransportError",
                            ))
                            .map_err(|error| error.to_string())?;
                        let receiver = runtime.root(
                            runtime
                                .rooted_value(net_root)
                                .ok_or("TLS error socket unavailable")?,
                        );
                        let id = runtime.root(Value::number(socket as f64));
                        shared_state.borrow_mut().scheduler.queue_shared_next_tick(
                            crate::modules::shared_event_loop::SharedCallback {
                                execution: None,
                                callback,
                                receiver,
                                args: vec![id],
                            },
                        );
                        progressed = true;
                        continue;
                    }
                    let error = net_socket_error_with_code(runtime, &message, &code)?;
                    set_text(runtime, error, "syscall", syscall)?;
                    if let Some(path) = tls_socket_path(runtime, net_root)? {
                        set_text(runtime, error, "path", &path)?;
                    }
                    if let Some(errno) = errno {
                        set_number(runtime, error, "errno", errno)?;
                    }
                    queue_net_method(runtime, shared_state, net_root, "destroy", &[error])?;
                    runtime.release_root(error);
                    progressed = true;
                    continue;
                }
                let (client, server_id, contexts) = {
                    let mut host = shared_state.borrow_mut();
                    let server_id = host
                        .http
                        .connections
                        .remove(&socket)
                        .map(|connection| connection.server);
                    let client = host.http.clients.remove(&socket);
                    let contexts = host
                        .http
                        .responses
                        .iter()
                        .filter_map(|(id, response)| {
                            (response.socket == socket).then_some((*id, response.async_id))
                        })
                        .collect::<Vec<_>>();
                    for (id, _) in &contexts {
                        host.http.responses.remove(id);
                    }
                    if let Some(server_id) = server_id {
                        if let Some(server) = host.http.servers.get_mut(&server_id) {
                            server.connections.remove(&socket);
                        }
                    }
                    (client, server_id, contexts)
                };
                let client_was_present = client.is_some();
                if let Some(client) = client {
                    fail_client_exchange(runtime, program, shared_state, client, &message)?;
                }
                for (_, async_id) in contexts {
                    crate::modules::async_hooks_shared_vm::clear_context(
                        runtime,
                        shared_state,
                        async_id,
                    );
                }
                if server_id.is_some() {
                    progressed = true;
                } else if client_was_present {
                    progressed = true;
                } else {
                    return Err(format!("shared HTTP socket failed: {message}"));
                }
            }
        }
    }
    progressed |= enforce_headers_timeouts(runtime, shared_state)?;
    progressed |= enforce_request_timeouts(runtime, shared_state)?;
    progressed |= finish_closed_servers(runtime, program, shared_state)?;
    Ok(progressed)
}

fn enforce_headers_timeouts(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let candidates = {
        let host = shared_state.borrow();
        host.http
            .connections
            .iter()
            .filter_map(|(socket, connection)| {
                (!connection.request_dispatched
                    && !connection.headers_timeout_sent
                    && connection.headers_started.is_some()
                    && super::protocol::head_size(&connection.received).is_none())
                    .then(|| {
                        host.http
                            .servers
                            .get(&connection.server)
                            .map(|server| {
                                (
                                    *socket,
                                    server.root,
                                    connection.headers_started.expect("checked above"),
                                )
                            })
                    })
                    .flatten()
            })
            .collect::<Vec<_>>()
    };
    let now = std::time::Instant::now();
    let mut timed_out = false;
    for (socket, server_root, started) in candidates {
        let key = runtime.string_rooted("headersTimeout");
        let timeout = runtime
            .get_property_rooted(server_root, key)
            .map_err(|error| error.to_string())?;
        runtime.release_root(key);
        let milliseconds = runtime
            .rooted_value(timeout)
            .and_then(Value::as_number)
            .unwrap_or(0.0);
        runtime.release_root(timeout);
        if milliseconds <= 0.0 || now.duration_since(started).as_millis() < milliseconds as u128 {
            continue;
        }
        {
            let mut host = shared_state.borrow_mut();
            let Some(connection) = host.http.connections.get_mut(&socket) else {
                continue;
            };
            if connection.headers_timeout_sent || connection.request_dispatched {
                continue;
            }
            connection.headers_timeout_sent = true;
            connection.request_dispatched = true;
            net_shared_vm::write(&mut host.tcp, socket, REQUEST_TIMEOUT_RESPONSE)?;
            net_shared_vm::end(&mut host.tcp, socket, false)?;
        }
        timed_out = true;
    }
    Ok(timed_out)
}

fn enforce_request_timeouts(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let candidates = {
        let host = shared_state.borrow();
        host.http
            .connections
            .iter()
            .filter_map(|(socket, connection)| {
                (!connection.request_dispatched
                    && !connection.request_timeout_sent
                    && connection.request_started.is_some()
                    && super::protocol::request(&connection.received).is_none())
                    .then(|| {
                        host.http
                            .servers
                            .get(&connection.server)
                            .map(|server| {
                                (
                                    *socket,
                                    server.root,
                                    connection.request_started.expect("checked above"),
                                )
                            })
                    })
                    .flatten()
            })
            .collect::<Vec<_>>()
    };
    let now = std::time::Instant::now();
    let mut timed_out = false;
    for (socket, server_root, started) in candidates {
        let key = runtime.string_rooted("requestTimeout");
        let timeout = runtime
            .get_property_rooted(server_root, key)
            .map_err(|error| error.to_string())?;
        runtime.release_root(key);
        let milliseconds = runtime
            .rooted_value(timeout)
            .and_then(Value::as_number)
            .unwrap_or(0.0);
        runtime.release_root(timeout);
        if milliseconds <= 0.0 || now.duration_since(started).as_millis() < milliseconds as u128 {
            continue;
        }
        {
            let mut host = shared_state.borrow_mut();
            let Some(connection) = host.http.connections.get_mut(&socket) else {
                continue;
            };
            if connection.request_timeout_sent || connection.request_dispatched {
                continue;
            }
            connection.request_timeout_sent = true;
            connection.request_dispatched = true;
            net_shared_vm::write(&mut host.tcp, socket, REQUEST_TIMEOUT_RESPONSE)?;
            net_shared_vm::end(&mut host.tcp, socket, false)?;
        }
        timed_out = true;
    }
    Ok(timed_out)
}

fn deliver_client_response_head(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    request_id: u64,
    async_id: u64,
    request_root: RootId,
    message: super::protocol::ResponseMessage,
) -> Result<RootId, String> {
    let max_headers_count_key = runtime.string_rooted("maxHeadersCount");
    let max_headers_count_value = runtime
        .get_property_rooted(request_root, max_headers_count_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(max_headers_count_key);
    let max_headers_count = runtime
        .rooted_value(max_headers_count_value)
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value as usize);
    runtime.release_root(max_headers_count_value);
    let mut raw_headers = message.raw_headers;
    if let Some(max_headers_count) = max_headers_count {
        raw_headers.truncate(max_headers_count);
    }
    let response = response_object(
        runtime,
        message.status,
        message.message,
        message.headers,
        raw_headers,
        false,
    )?;
    let body = runtime.string_rooted("");
    let open = runtime.root(Value::TRUE);
    let exchange_id = runtime.root(Value::number(request_id as f64));
    let undefined = runtime.root(Value::UNDEFINED);
    let incoming_factory = shared_state
        .borrow()
        .http
        .incoming_factory
        .ok_or_else(|| "HTTP incoming-message factory is unavailable".to_owned())?;
    let readable = match runtime.call_rooted(
        incoming_factory,
        undefined,
        &[response, body, open, exchange_id],
    ) {
        Ok(readable) => readable,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            runtime.release_root(response);
            runtime.release_root(body);
            runtime.release_root(open);
            runtime.release_root(exchange_id);
            runtime.release_root(undefined);
            return Err(message);
        }
    };
    let request_response = duplicate_root(runtime, readable)?;
    set_named(runtime, request_root, "res", request_response)?;
    let emitted = {
        let _scope = crate::modules::async_hooks_shared_vm::enter_context(shared_state, async_id);
        emit_event(runtime, program, request_root, "response", &[readable])
    };
    runtime.release_root(response);
    runtime.release_root(body);
    runtime.release_root(open);
    runtime.release_root(exchange_id);
    runtime.release_root(undefined);
    if let Err(error) = emitted {
        runtime.release_root(readable);
        return Err(error);
    }
    Ok(readable)
}

fn deliver_client_information(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    async_id: u64,
    request_root: RootId,
    message: super::protocol::ResponseMessage,
) -> Result<(), String> {
    let information = runtime.object_rooted().map_err(|error| error.to_string())?;
    set_number(runtime, information, "statusCode", message.status as f64)?;
    set_text(runtime, information, "statusMessage", &message.message)?;
    set_text(runtime, information, "httpVersion", "1.1")?;
    set_number(runtime, information, "httpVersionMajor", 1.0)?;
    set_number(runtime, information, "httpVersionMinor", 1.0)?;
    set_bool(runtime, information, "complete", true)?;
    let raw_headers = raw_header_array(runtime, message.raw_headers)?;
    set_named(runtime, information, "rawHeaders", raw_headers)?;

    // IncomingMessage.headers uses lower-case names, joins duplicate headers,
    // and retains Set-Cookie values as an array.
    let headers = runtime
        .null_object_rooted()
        .map_err(|error| error.to_string())?;
    let mut grouped = Vec::<(String, Vec<String>)>::new();
    for (name, value) in message.headers {
        let name = name.to_ascii_lowercase();
        if let Some((_, values)) = grouped.iter_mut().find(|(key, _)| *key == name) {
            values.push(value);
        } else {
            grouped.push((name, vec![value]));
        }
    }
    for (name, values) in grouped {
        if name == "set-cookie" {
            let values = values
                .iter()
                .map(|value| runtime.string_rooted(value))
                .collect::<Vec<_>>();
            let global = runtime.global_root().map_err(|error| error.to_string())?;
            let array_key = runtime.string_rooted("Array");
            let array_ctor = runtime.get_property_rooted(global, array_key);
            let array = match array_ctor {
                Ok(array_ctor) => {
                    let result = runtime.construct_rooted(array_ctor, array_ctor, &values);
                    runtime.release_root(array_ctor);
                    result
                }
                Err(error) => Err(error),
            };
            for value in values {
                runtime.release_root(value);
            }
            runtime.release_root(array_key);
            runtime.release_root(global);
            let array = array.map_err(|error| error.to_string())?;
            set_named(runtime, headers, &name, array)?;
        } else {
            set_text(runtime, headers, &name, &values.join(", "))?;
        }
    }
    set_named(runtime, information, "headers", headers)?;

    let _scope = crate::modules::async_hooks_shared_vm::enter_context(shared_state, async_id);
    if message.status == 100 {
        emit_event(runtime, program, request_root, "continue", &[])?;
    }
    let result = emit_event(
        runtime,
        program,
        request_root,
        "information",
        &[information],
    );
    runtime.release_root(information);
    result
}

fn deliver_client_body(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    readable: RootId,
    async_id: u64,
    chunks: &[Vec<u8>],
) -> Result<(), String> {
    for bytes in chunks {
        // A flowing `push()` can synchronously invoke user code. If that
        // callback aborts/destroys the request, stop delivering the remaining
        // parser chunks from this same transport read.
        let destroyed_key = runtime.string_rooted("destroyed");
        let destroyed_value = match runtime.get_property_rooted(readable, destroyed_key) {
            Ok(value) => value,
            Err(error) => {
                runtime.release_root(destroyed_key);
                return Err(error.to_string());
            }
        };
        let destroyed = match runtime.truthy_rooted(destroyed_value) {
            Ok(destroyed) => destroyed,
            Err(error) => {
                runtime.release_root(destroyed_value);
                runtime.release_root(destroyed_key);
                return Err(error.to_string());
            }
        };
        runtime.release_root(destroyed_value);
        runtime.release_root(destroyed_key);
        if destroyed {
            break;
        }
        let _scope = crate::modules::async_hooks_shared_vm::enter_context(shared_state, async_id);
        deliver_client_body_chunk(runtime, program, readable, bytes)?;
    }
    Ok(())
}

fn deliver_client_body_chunk(
    runtime: &mut Runtime<NodeHost>,
    _program: &quench_runtime::ResidualProgram,
    readable: RootId,
    bytes: &[u8],
) -> Result<(), String> {
    if bytes.is_empty() {
        return Ok(());
    }
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let buffer_key = runtime.string_rooted("Buffer");
    let buffer = runtime.get_property_rooted(global, buffer_key);
    let buffer = match buffer {
        Ok(buffer) => buffer,
        Err(error) => {
            runtime.release_root(buffer_key);
            runtime.release_root(global);
            return Err(error.to_string());
        }
    };
    let from_key = runtime.string_rooted("from");
    let from = runtime.get_property_rooted(buffer, from_key);
    let from = match from {
        Ok(from) => from,
        Err(error) => {
            runtime.release_root(from_key);
            runtime.release_root(buffer);
            runtime.release_root(buffer_key);
            runtime.release_root(global);
            return Err(error.to_string());
        }
    };
    let body = runtime.string_rooted(&latin1_text(bytes));
    let encoding = runtime.string_rooted("latin1");
    let chunk = runtime.call_rooted(from, buffer, &[body, encoding]);
    let chunk = match chunk {
        Ok(chunk) => chunk,
        Err(error) => {
            runtime.release_root(body);
            runtime.release_root(encoding);
            runtime.release_root(from);
            runtime.release_root(from_key);
            runtime.release_root(buffer);
            runtime.release_root(buffer_key);
            runtime.release_root(global);
            return Err(error.to_string());
        }
    };
    let push_key = runtime.string_rooted("push");
    let result = match runtime.get_property_rooted(readable, push_key) {
        Ok(push) => {
            let result = runtime.call_rooted(push, readable, &[chunk]);
            runtime.release_root(push);
            result
        }
        Err(error) => Err(error),
    };
    runtime.release_root(push_key);
    runtime.release_root(chunk);
    runtime.release_root(body);
    runtime.release_root(encoding);
    runtime.release_root(from);
    runtime.release_root(from_key);
    runtime.release_root(buffer);
    runtime.release_root(buffer_key);
    runtime.release_root(global);
    result
        .map(|value| runtime.release_root(value))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn finish_client_response(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    readable: RootId,
    async_id: u64,
) -> Result<(), String> {
    set_bool(runtime, readable, "complete", true)?;
    let key = runtime.string_rooted("push");
    let push = runtime
        .get_property_rooted(readable, key)
        .map_err(|error| error.to_string())?;
    let end = runtime.root(Value::NULL);
    let result = {
        let _scope = crate::modules::async_hooks_shared_vm::enter_context(shared_state, async_id);
        runtime.call_rooted(push, readable, &[end])
    };
    runtime.release_root(end);
    runtime.release_root(push);
    runtime.release_root(key);
    result
        .map(|value| runtime.release_root(value))
        .map(|_| ())
        .map_err(|error| {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            message
        })
}

fn with_released_shared_borrow<T, R>(
    shared_state: &Rc<RefCell<T>>,
    mutate: impl FnOnce(&mut T) -> R,
) -> R {
    let result = {
        let mut state = shared_state.borrow_mut();
        mutate(&mut state)
    };
    result
}

fn fail_client_exchange(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    client: super::state::Client,
    message: &str,
) -> Result<(), String> {
    fail_client_exchange_with_code(
        runtime,
        program,
        shared_state,
        client,
        message,
        "ECONNRESET",
    )
}

fn fail_client_exchange_with_code(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    client: super::state::Client,
    message: &str,
    code: &str,
) -> Result<(), String> {
    let error = match client_exchange_error(runtime, program, message, code) {
        Ok(error) => error,
        Err(message) => {
            if let Some(response) = client.response_root {
                runtime.release_root(response);
            }
            runtime.release_root(client.request_root);
            crate::modules::async_hooks_shared_vm::complete_runtime_context(
                runtime,
                client.async_id,
            );
            return Err(message);
        }
    };
    let stream = client.response_root.unwrap_or(client.request_root);
    let result = {
        let _scope =
            crate::modules::async_hooks_shared_vm::enter_context(shared_state, client.async_id);
        destroy_stream_with_error(runtime, program, stream, error)
    };
    runtime.release_root(error);
    // HTTP's parser owns the exchange, but the public net.Socket still owns
    // its JS close lifecycle. Closing only the native transport skips
    // netSocketDestroy's pending-destroy bookkeeping and leaves the Socket
    // without an observable `close` event.
    let socket_destroy = destroy_request_socket(runtime, program, client.request_root);
    if let Some(response) = client.response_root {
        runtime.release_root(response);
    }
    runtime.release_root(client.request_root);
    crate::modules::async_hooks_shared_vm::complete_runtime_context(runtime, client.async_id);
    socket_destroy?;
    result
}

fn destroy_request_socket(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    request: RootId,
) -> Result<bool, String> {
    let socket_key = runtime.string_rooted("socket");
    let socket = match runtime.get_property_rooted(request, socket_key) {
        Ok(value) => value,
        Err(error) => {
            runtime.release_root(socket_key);
            return Err(runtime.format_error(program, &error.error));
        }
    };
    runtime.release_root(socket_key);
    if !runtime
        .rooted_value(socket)
        .is_some_and(quench_runtime::Value::is_heap)
    {
        runtime.release_root(socket);
        return Ok(false);
    }
    let id_key = runtime.string_rooted("__quenchNetSocketId");
    let id = runtime.get_property_rooted(socket, id_key);
    runtime.release_root(id_key);
    let registered_socket = match id {
        Ok(value) => {
            let registered = runtime.rooted_value(value).and_then(Value::as_number).is_some();
            runtime.release_root(value);
            registered
        }
        Err(error) => {
            runtime.release_root(socket);
            return Err(runtime.format_error(program, &error.error));
        }
    };
    if !registered_socket {
        runtime.release_root(socket);
        return Ok(false);
    }
    let destroy_key = runtime.string_rooted("destroy");
    let destroy = match runtime.get_property_rooted(socket, destroy_key) {
        Ok(value) => value,
        Err(error) => {
            runtime.release_root(socket);
            runtime.release_root(destroy_key);
            return Err(runtime.format_error(program, &error.error));
        }
    };
    runtime.release_root(destroy_key);
    let result = runtime.call_rooted(destroy, socket, &[]);
    runtime.release_root(destroy);
    runtime.release_root(socket);
    match result {
        Ok(value) => {
            runtime.release_root(value);
            Ok(true)
        }
        Err(error) => Err(runtime.format_error(program, &error.error)),
    }
}

fn client_exchange_error(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    message: &str,
    code: &str,
) -> Result<RootId, String> {
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let error_key = runtime.string_rooted("Error");
    let error_constructor = match runtime.get_property_rooted(global, error_key) {
        Ok(value) => value,
        Err(error) => {
            runtime.release_root(error_key);
            runtime.release_root(global);
            return Err(runtime.format_error(program, &error.error));
        }
    };
    let message_root = runtime.string_rooted(message);
    let error =
        match runtime.construct_rooted(error_constructor, error_constructor, &[message_root]) {
            Ok(value) => value,
            Err(error) => {
                if let Some(exception) = error.exception {
                    runtime.release_root(exception);
                }
                runtime.release_root(message_root);
                runtime.release_root(error_constructor);
                runtime.release_root(error_key);
                runtime.release_root(global);
                return Err(runtime.format_error(program, &error.error));
            }
        };
    let set_code = set_text(runtime, error, "code", code);
    runtime.release_root(message_root);
    runtime.release_root(error_constructor);
    runtime.release_root(error_key);
    runtime.release_root(global);
    if let Err(failure) = set_code {
        runtime.release_root(error);
        return Err(failure);
    }
    Ok(error)
}

fn destroy_stream_with_error(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    stream: RootId,
    error: RootId,
) -> Result<(), String> {
    let destroy_key = runtime.string_rooted("destroy");
    let destroy = match runtime.get_property_rooted(stream, destroy_key) {
        Ok(value) => value,
        Err(error) => {
            runtime.release_root(destroy_key);
            return Err(runtime.format_error(program, &error.error));
        }
    };
    let result = runtime.call_rooted(destroy, stream, &[error]);
    runtime.release_root(destroy);
    runtime.release_root(destroy_key);
    result
        .map(|value| runtime.release_root(value))
        .map(|_| ())
        .map_err(|error| {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            message
        })
}

pub(crate) fn cleanup(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) {
    let (roots, async_ids) =
        {
            let mut host = shared_state.borrow_mut();
            let shared = std::mem::take(&mut host.http);
            let mut roots = Vec::new();
            roots.extend(shared.servers.into_values().map(|server| server.root));
            roots.extend(shared.clients.into_values().flat_map(|client| {
                std::iter::once(client.request_root).chain(client.response_root)
            }));
            let async_ids = shared
                .responses
                .into_values()
                .map(|response| response.async_id)
                .collect::<Vec<_>>();
            roots.extend(shared.response_factory);
            roots.extend(shared.incoming_factory);
            for socket in host.net_sockets.values_mut() {
                roots.extend(socket.write_callbacks.drain(..));
            }
            roots.extend(host.net_closing_sockets.drain().map(|(_, root)| root));
            net_shared_vm::cleanup(&mut host.tcp);
            (roots, async_ids)
        };
    for root in roots {
        runtime.release_root(root);
    }
    for async_id in async_ids {
        crate::modules::async_hooks_shared_vm::clear_context(runtime, shared_state, async_id);
    }
}

fn queue_write_callback(
    runtime: &mut Runtime<NodeHost>,
    state: &Rc<RefCell<SharedNodeState>>,
    callback: RootId,
    error: Option<RootId>,
) {
    let receiver = runtime.root(Value::UNDEFINED);
    let args = error
        .into_iter()
        .map(|error| runtime.root(runtime.rooted_value(error).expect("live write error")))
        .collect();
    state.borrow_mut().scheduler.queue_shared_next_tick(
        crate::modules::shared_event_loop::SharedCallback {
            execution: None,
            callback,
            receiver,
            args,
        },
    );
}

fn net_error(runtime: &mut Runtime<NodeHost>, message: &str) -> Result<RootId, String> {
    runtime
        .error_rooted(message)
        .map_err(|error| error.to_string())
}

fn tcp_connect_error(
    runtime: &mut Runtime<NodeHost>,
    failure: &crate::modules::net_connect_error::TcpConnectFailure,
) -> Result<RootId, String> {
    let error = net_error(runtime, &failure.message())?;
    if let Some(errno) = failure.errno() {
        set_number(runtime, error, "errno", errno)?;
    }
    set_text(runtime, error, "code", &failure.code())?;
    set_text(runtime, error, "syscall", failure.syscall())?;
    if let Some(address) = failure.address() {
        set_text(runtime, error, "address", &address.ip().to_string())?;
        set_number(runtime, error, "port", f64::from(address.port()))?;
    }
    if let Some(hostname) = failure.hostname() {
        set_text(runtime, error, "hostname", hostname)?;
    }
    Ok(error)
}

fn net_socket_error_with_code(
    runtime: &mut Runtime<NodeHost>,
    message: &str,
    code: &str,
) -> Result<RootId, String> {
    let error = net_error(runtime, message)?;
    set_text(runtime, error, "code", code)?;
    Ok(error)
}

fn tls_socket_path(
    runtime: &mut Runtime<NodeHost>,
    socket: RootId,
) -> Result<Option<String>, String> {
    let mut roots = Vec::new();
    let result = (|| {
        let options_key = runtime.string_rooted("_tlsOptions");
        roots.push(options_key);
        let options = runtime
            .get_property_rooted(socket, options_key)
            .map_err(|error| error.to_string())?;
        roots.push(options);
        if runtime
            .rooted_value(options)
            .is_none_or(|value| value.is_undefined() || value.is_null())
        {
            return Ok(None);
        }
        let path_key = runtime.string_rooted("path");
        roots.push(path_key);
        let path = runtime
            .get_property_rooted(options, path_key)
            .map_err(|error| error.to_string())?;
        roots.push(path);
        runtime
            .string_text_rooted(path)
            .map_err(|error| error.to_string())
    })();
    for root in roots.into_iter().rev() {
        runtime.release_root(root);
    }
    result
}

fn emit_listening(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let pending = {
        let mut host = shared_state.borrow_mut();
        let ids = host
            .http
            .servers
            .iter()
            .filter_map(|(id, server)| server.listening_pending.then_some(*id))
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| {
                let server = host.http.servers.get_mut(&id)?;
                server.listening_pending = false;
                Some(server.root)
            })
            .collect::<Vec<_>>()
    };
    let mut first_error = None;
    for server in &pending {
        if let Err(error) = emit_event(runtime, program, *server, "listening", &[]) {
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(!pending.is_empty())
}

fn emit_net_server_events(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let (listening, closed) = {
        let mut host = shared_state.borrow_mut();
        let listening = host
            .net_servers
            .iter_mut()
            .filter_map(|(id, server)| {
                if server.listening_pending {
                    server.listening_pending = false;
                    Some((*id, server.root))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let ids = host
            .net_servers
            .iter()
            .filter_map(|(id, server)| {
                (server.closing && server.connections.is_empty()).then_some(*id)
            })
            .collect::<Vec<_>>();
        let closed = ids
            .into_iter()
            .filter_map(|id| host.net_servers.remove(&id).map(|server| server.root))
            .collect::<Vec<_>>();
        (listening, closed)
    };
    for (_, root) in &listening {
        queue_net_event(runtime, shared_state, *root, "listening", &[])?;
    }
    for root in &closed {
        queue_net_event(runtime, shared_state, *root, "close", &[])?;
        runtime.release_root(*root);
    }
    Ok(!listening.is_empty() || !closed.is_empty())
}

fn finish_closed_servers(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let closed = {
        let mut host = shared_state.borrow_mut();
        let ids = host
            .http
            .servers
            .iter()
            .filter_map(|(id, server)| {
                (server.closing && server.connections.is_empty()).then_some(*id)
            })
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| host.http.servers.remove(&id).map(|server| server.root))
            .collect::<Vec<_>>()
    };
    let mut first_error = None;
    for server in &closed {
        if let Err(error) = emit_event(runtime, program, *server, "close", &[]) {
            first_error.get_or_insert(error);
        }
        runtime.release_root(*server);
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(!closed.is_empty())
}

fn emit_event(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    receiver: RootId,
    name: &str,
    args: &[RootId],
) -> Result<(), String> {
    let event = runtime.string_rooted(name);
    let result = emit(runtime, program, receiver, event, args);
    runtime.release_root(event);
    result
}

fn emit_client_request_close(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    request: RootId,
) -> Result<(), String> {
    let key = runtime.string_rooted("__quenchEmitClose");
    let emit_close = match runtime.get_property_rooted(request, key) {
        Ok(emit_close) => emit_close,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            runtime.release_root(key);
            return Err(message);
        }
    };
    runtime.release_root(key);
    let result = runtime.call_rooted(emit_close, request, &[]);
    runtime.release_root(emit_close);
    match result {
        Ok(result) => {
            runtime.release_root(result);
            Ok(())
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    }
}

fn queue_net_event(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    receiver: RootId,
    name: &str,
    args: &[RootId],
) -> Result<(), String> {
    let event = runtime.string_rooted(name);
    let mut event_args = Vec::with_capacity(args.len() + 1);
    event_args.push(event);
    event_args.extend_from_slice(args);
    let result = queue_net_method(runtime, shared_state, receiver, "emit", &event_args);
    runtime.release_root(event);
    result
}

fn queue_connection_event(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    receiver: RootId,
    name: &str,
    args: &[RootId],
    socket: u64,
) -> Result<(), String> {
    let event = runtime.string_rooted(name);
    let mut event_args = Vec::with_capacity(args.len() + 1);
    event_args.push(event);
    event_args.extend_from_slice(args);
    let result = queue_net_method_completion(
        runtime,
        shared_state,
        receiver,
        "emit",
        &event_args,
        Some(NetDelivery::Connect(socket)),
    );
    runtime.release_root(event);
    result
}

fn queue_net_method(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    receiver: RootId,
    method: &str,
    args: &[RootId],
) -> Result<(), String> {
    queue_net_method_completion(runtime, shared_state, receiver, method, args, None)
}
enum NetDelivery {
    Connection(u64),
    Connect(u64),
    Data(u64, usize),
    Eof,
}
fn queue_net_method_completion(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    receiver: RootId,
    method: &str,
    args: &[RootId],
    completion: Option<NetDelivery>,
) -> Result<(), String> {
    let key = runtime.string_rooted(method);
    let mut callback = runtime
        .get_property_rooted(receiver, key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(key);
    let receiver = runtime.root(
        runtime
            .rooted_value(receiver)
            .ok_or("net socket is unavailable")?,
    );
    let mut retained_args =
        Vec::with_capacity(args.len() + if completion.is_some() { 2 } else { 0 });
    if let Some(completion) = completion {
        retained_args.push(callback);
        let operation = match completion {
            NetDelivery::Connection(socket) => {
                retained_args.push(runtime.root(Value::number(socket as f64)));
                "netDeliverConnection"
            }
            NetDelivery::Connect(socket) => {
                retained_args.push(runtime.root(Value::number(socket as f64)));
                "netDeliverConnect"
            }
            NetDelivery::Data(socket, length) => {
                retained_args.push(runtime.root(Value::number(socket as f64)));
                retained_args.push(runtime.root(Value::number(length as f64)));
                "netDeliverData"
            }
            NetDelivery::Eof => "netDeliverEof",
        };
        callback = runtime
            .host_function(crate::host::shared_vm::operation(operation))
            .map_err(|error| error.to_string())?;
    }
    for arg in args {
        retained_args.push(
            runtime.root(
                runtime
                    .rooted_value(*arg)
                    .ok_or("net event argument is unavailable")?,
            ),
        );
    }
    shared_state.borrow_mut().scheduler.queue_shared_next_tick(
        crate::modules::shared_event_loop::SharedCallback {
            execution: None,
            callback,
            receiver,
            args: retained_args,
        },
    );
    Ok(())
}

fn buffer_from_bytes(runtime: &mut Runtime<NodeHost>, bytes: &[u8]) -> Result<RootId, String> {
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let buffer_key = runtime.string_rooted("Buffer");
    let buffer = runtime
        .get_property_rooted(global, buffer_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(buffer_key);
    runtime.release_root(global);
    let from_key = runtime.string_rooted("from");
    let from = runtime
        .get_property_rooted(buffer, from_key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(from_key);
    let text = runtime.string_rooted(&latin1_text(bytes));
    let encoding = runtime.string_rooted("latin1");
    let chunk = runtime
        .call_rooted(from, buffer, &[text, encoding])
        .map_err(|error| error.to_string())?;
    runtime.release_root(encoding);
    runtime.release_root(text);
    runtime.release_root(from);
    runtime.release_root(buffer);
    Ok(chunk)
}

fn emit(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    receiver: RootId,
    event: RootId,
    args: &[RootId],
) -> Result<(), String> {
    let key = runtime.string_rooted("emit");
    let emit = match runtime.get_property_rooted(receiver, key) {
        Ok(emit) => emit,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            runtime.release_root(key);
            return Err(message);
        }
    };
    let mut operands = Vec::with_capacity(args.len() + 1);
    operands.push(event);
    operands.extend_from_slice(args);
    let result = runtime.call_rooted(emit, receiver, &operands);
    runtime.release_root(emit);
    runtime.release_root(key);
    match result {
        Ok(result) => {
            runtime.release_root(result);
            Ok(())
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    }
}

fn request_object(
    runtime: &mut Runtime<NodeHost>,
    method: String,
    path: String,
    version: String,
    _headers: Vec<(String, String)>,
    raw_headers: Vec<(String, String)>,
    raw_trailers: Vec<(String, String)>,
    socket: RootId,
) -> Result<RootId, String> {
    let request = runtime.object_rooted().map_err(|error| error.to_string())?;
    set_text(runtime, request, "method", &method)?;
    set_text(runtime, request, "url", &path)?;
    set_text(runtime, request, "httpVersion", &version)?;
    let (major, minor) = version
        .split_once('.')
        .map(|(major, minor)| (major.parse().unwrap_or(1), minor.parse().unwrap_or(1)))
        .unwrap_or((1, 1));
    set_number(runtime, request, "httpVersionMajor", major as f64)?;
    set_number(runtime, request, "httpVersionMinor", minor as f64)?;
    let raw_headers = raw_header_array(runtime, raw_headers)?;
    set_named(runtime, request, "rawHeaders", raw_headers)?;
    set_incoming_trailers(runtime, request, raw_trailers)?;
    // IncomingMessage consumers use socket.readable to distinguish a fully
    // received message from a finished socket. This connection remains
    // readable after the request framing has completed, just as Node's
    // keep-alive socket does.
    set_bool(runtime, socket, "readable", true)?;
    set_bool(runtime, socket, "writable", true)?;
    define_number(
        runtime,
        socket,
        "readableHighWaterMark",
        DEFAULT_HTTP_SOCKET_HIGH_WATER_MARK,
    )?;
    define_number(
        runtime,
        socket,
        "writableHighWaterMark",
        DEFAULT_HTTP_SOCKET_HIGH_WATER_MARK,
    )?;
    let socket = duplicate_root(runtime, socket)?;
    set_named(runtime, request, "socket", socket)?;
    set_bool(runtime, request, "complete", true)?;
    set_bool(runtime, request, "readable", true)?;
    Ok(request)
}

fn set_incoming_trailers(
    runtime: &mut Runtime<NodeHost>,
    message: RootId,
    raw_trailers: Vec<(String, String)>,
) -> Result<(), String> {
    let raw = raw_header_array(runtime, raw_trailers.clone())?;
    set_named(runtime, message, "rawTrailers", raw)?;
    let trailers = runtime.object_rooted().map_err(|error| error.to_string())?;
    let trailers_distinct = runtime
        .null_object_rooted()
        .map_err(|error| error.to_string())?;
    let mut grouped = Vec::<(String, Vec<String>)>::new();
    for (name, value) in raw_trailers {
        let key = name.to_ascii_lowercase();
        if let Some((_, values)) = grouped.iter_mut().find(|(name, _)| *name == key) {
            values.push(value);
        } else {
            grouped.push((key, vec![value]));
        }
    }
    for (name, values) in grouped {
        set_text(runtime, trailers, &name, &values.join(", "))?;
        let values = values
            .iter()
            .map(|value| runtime.string_rooted(value))
            .collect::<Vec<_>>();
        let global = runtime.global_root().map_err(|error| error.to_string())?;
        let array_key = runtime.string_rooted("Array");
        let constructor = runtime
            .get_property_rooted(global, array_key)
            .map_err(|error| error.to_string())?;
        let array = runtime
            .construct_rooted(constructor, constructor, &values)
            .map_err(|error| error.to_string())?;
        runtime.release_root(constructor);
        runtime.release_root(array_key);
        runtime.release_root(global);
        for value in values {
            runtime.release_root(value);
        }
        set_named(runtime, trailers_distinct, &name, array)?;
    }
    set_named(runtime, message, "trailers", trailers)?;
    set_named(runtime, message, "trailersDistinct", trailers_distinct)
}

fn diagnostics_message(
    runtime: &mut Runtime<NodeHost>,
    request: RootId,
    response: RootId,
    server: RootId,
    socket: RootId,
) -> Result<RootId, String> {
    let message = runtime.object_rooted().map_err(|error| error.to_string())?;
    for (name, value) in [
        ("request", request),
        ("response", response),
        ("server", server),
        ("socket", socket),
    ] {
        let retained = match duplicate_root(runtime, value) {
            Ok(retained) => retained,
            Err(error) => {
                runtime.release_root(message);
                return Err(error);
            }
        };
        if let Err(error) = set_named(runtime, message, name, retained) {
            runtime.release_root(message);
            return Err(error);
        }
    }
    Ok(message)
}

fn duplicate_root(runtime: &mut Runtime<NodeHost>, root: RootId) -> Result<RootId, String> {
    let value = runtime
        .rooted_value(root)
        .ok_or_else(|| "shared HTTP value root is no longer live".to_owned())?;
    Ok(runtime.root(value))
}

fn response_object(
    runtime: &mut Runtime<NodeHost>,
    status: u16,
    message: String,
    _headers: Vec<(String, String)>,
    raw_headers: Vec<(String, String)>,
    complete: bool,
) -> Result<RootId, String> {
    let response = runtime.object_rooted().map_err(|error| error.to_string())?;
    set_number(runtime, response, "statusCode", status as f64)?;
    set_text(runtime, response, "statusMessage", &message)?;
    set_text(runtime, response, "httpVersion", "1.1")?;
    let raw_headers = raw_header_array(runtime, raw_headers)?;
    set_named(runtime, response, "rawHeaders", raw_headers)?;
    set_bool(runtime, response, "complete", complete)?;
    Ok(response)
}

fn raw_header_array(
    runtime: &mut Runtime<NodeHost>,
    raw_headers: Vec<(String, String)>,
) -> Result<RootId, String> {
    let mut raw = Vec::with_capacity(raw_headers.len() * 2);
    for (name, value) in raw_headers {
        raw.push(runtime.string_rooted(&name));
        raw.push(runtime.string_rooted(&value));
    }
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let array_key = runtime.string_rooted("Array");
    let array = runtime.get_property_rooted(global, array_key);
    let result = match array {
        Ok(array) => {
            let result = runtime.construct_rooted(array, array, &raw);
            runtime.release_root(array);
            result
        }
        Err(error) => Err(error),
    };
    for value in raw {
        runtime.release_root(value);
    }
    runtime.release_root(array_key);
    runtime.release_root(global);
    result.map_err(|error| error.to_string())
}

fn latin1_text(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

fn set_text(
    runtime: &mut Runtime<NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), String> {
    let value = runtime.string_rooted(value);
    set_named(runtime, object, name, value)
}

fn define_number(
    runtime: &mut Runtime<NodeHost>,
    object: RootId,
    name: &str,
    value: f64,
) -> Result<(), String> {
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let object_key = runtime.string_rooted("Object");
    let object_constructor = runtime
        .get_property_rooted(global, object_key)
        .map_err(|error| error.to_string())?;
    let define_key = runtime.string_rooted("defineProperty");
    let define = runtime
        .get_property_rooted(object_constructor, define_key)
        .map_err(|error| error.to_string())?;
    let descriptor = runtime.object_rooted().map_err(|error| error.to_string())?;
    set_number(runtime, descriptor, "value", value)?;
    set_bool(runtime, descriptor, "writable", true)?;
    set_bool(runtime, descriptor, "enumerable", true)?;
    set_bool(runtime, descriptor, "configurable", true)?;
    let name = runtime.string_rooted(name);
    let result = runtime.call_rooted(define, object_constructor, &[object, name, descriptor]);
    runtime.release_root(name);
    runtime.release_root(descriptor);
    runtime.release_root(define);
    runtime.release_root(define_key);
    runtime.release_root(object_constructor);
    runtime.release_root(object_key);
    runtime.release_root(global);
    result
        .map(|result| { runtime.release_root(result); })
        .map_err(|error| error.to_string())
}

fn set_number(
    runtime: &mut Runtime<NodeHost>,
    object: RootId,
    name: &str,
    value: f64,
) -> Result<(), String> {
    let value = runtime.root(Value::number(value));
    set_named(runtime, object, name, value)
}

fn set_bool(
    runtime: &mut Runtime<NodeHost>,
    object: RootId,
    name: &str,
    value: bool,
) -> Result<(), String> {
    let value = runtime.root(if value { Value::TRUE } else { Value::FALSE });
    set_named(runtime, object, name, value)
}

fn set_named(
    runtime: &mut Runtime<NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), String> {
    let key = runtime.string_rooted(name);
    set(runtime, object, key, value).map_err(|error| format!("HTTP property {name}: {error}"))
}

fn set(
    runtime: &mut Runtime<NodeHost>,
    object: RootId,
    key: RootId,
    value: RootId,
) -> Result<(), String> {
    let result = runtime.set_property_rooted(object, key, value, object);
    runtime.release_root(key);
    runtime.release_root(value);
    let result = result.map_err(|error| error.to_string())?;
    if result {
        Ok(())
    } else {
        Err("shared HTTP object rejected a required property".into())
    }
}

fn queue_native_read(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    socket: u64,
    bytes: Option<&[u8]>,
    count: i32,
) -> Result<bool, String> {
    // Public Socket ownership is authoritative after admission/adoption.
    if shared_state.borrow().net_sockets.contains_key(&socket) {
        return Ok(false);
    }
    let handle = shared_state
        .borrow()
        .native_stream
        .readers
        .get(&socket)
        .copied();
    let Some(handle) = handle else {
        return Ok(false);
    };
    let payload = if let Some(bytes) = bytes {
        let operation = runtime
            .host_function(crate::host::shared_vm::operation("nativeStreamCopyRead"))
            .map_err(|e| e.to_string())?;
        let undefined = runtime.root(Value::UNDEFINED);
        let text = runtime.string_rooted(&latin1_text(bytes));
        let result = runtime.call_rooted(operation, undefined, &[text]);
        runtime.release_root(operation);
        runtime.release_root(undefined);
        runtime.release_root(text);
        result.map_err(|e| e.to_string())?
    } else {
        runtime.root(Value::UNDEFINED)
    };
    let callback = runtime
        .host_function(crate::host::shared_vm::operation("nativeStreamDeliverRead"))
        .map_err(|e| e.to_string())?;
    let receiver = runtime.root(
        runtime
            .rooted_value(handle)
            .ok_or("raw native handle expired")?,
    );
    let execution = if let Some(resource) =
        crate::modules::native_async_wrap::tcp_resource(shared_state, socket)
    {
        let owner = runtime.root(
            runtime
                .rooted_value(handle)
                .ok_or("native read handle expired")?,
        );
        Some(crate::modules::shared_event_loop::CallbackExecution {
            id: resource.id,
            resource: owner,
            lifetime: crate::modules::shared_event_loop::CallbackLifetime::Resource,
            optional_callback: None,
        })
    } else {
        None
    };
    let count = runtime.root(Value::number(f64::from(count)));
    let id = runtime.root(Value::number(socket as f64));
    shared_state.borrow_mut().scheduler.queue_shared_next_tick(
        crate::modules::shared_event_loop::SharedCallback {
            execution,
            callback,
            receiver,
            args: vec![payload, count, id],
        },
    );
    Ok(true)
}
fn complete_native_request(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    request: RootId,
    status: i32,
) -> Result<bool, String> {
    let pending = {
        let mut state = shared_state.borrow_mut();
        let index = state
            .native_stream
            .requests
            .iter()
            .position(|(id, _)| *id == request);
        index
            .and_then(|index| state.native_stream.requests.remove(index))
            .map(|(_, pending)| pending)
    };
    let Some(pending) = pending else {
        return Ok(false);
    };
    let admitted = pending
        .lease
        .resource()
        .expect("queued native request has an admitted AsyncWrap resource");
    let resource = runtime.root(
        runtime
            .rooted_value(request)
            .expect("retained native request root"),
    );
    let status_root = runtime.root(Value::number(f64::from(status)));
    let connect = pending.kind == super::super::stream_binding::RequestKind::Connect;
    let publish_status = runtime.root(if !connect { Value::TRUE } else { Value::FALSE });
    let prepare = runtime
        .host_function(crate::host::shared_vm::operation(
            "nativeStreamPrepareCompletion",
        ))
        .expect("registered native stream completion preparation");
    let prepared = runtime.call_rooted(prepare, request, &[status_root, publish_status]);
    runtime.release_root(prepare);
    runtime.release_root(publish_status);
    let undefined = runtime.root(Value::UNDEFINED);
    let mut callback = crate::modules::shared_event_loop::SharedCallback {
        execution: Some(crate::modules::shared_event_loop::CallbackExecution {
            lifetime: crate::modules::shared_event_loop::CallbackLifetime::Operation,
            optional_callback: None,
            id: admitted.id,
            resource,
        }),
        callback: undefined,
        receiver: request,
        args: vec![status_root, pending.handle],
    };
    if connect {
        callback.args.push(
            runtime.root(
                runtime
                    .rooted_value(request)
                    .expect("retained connect request"),
            ),
        );
        callback.args.push(runtime.root(if status == 0 {
            Value::TRUE
        } else {
            Value::FALSE
        }));
        callback.args.push(runtime.root(if status == 0 {
            Value::TRUE
        } else {
            Value::FALSE
        }));
    } else {
        callback.args.push(runtime.root(Value::UNDEFINED));
    }
    let result = match prepared {
        Ok(value)
            if runtime
                .rooted_value(value)
                .is_some_and(|v| v.is_undefined()) =>
        {
            runtime.release_root(value);
            super::super::process_shared_vm::release_callback(runtime, callback);
            Ok(())
        }
        Ok(value) => {
            runtime.release_root(callback.callback);
            callback.callback = value;
            super::super::process_shared_vm::invoke_async_callback(runtime, program, callback)
        }
        Err(error) => {
            let result =
                super::super::process_shared_vm::route_callback_failure(runtime, program, error);
            super::super::process_shared_vm::release_callback(runtime, callback);
            result
        }
    };
    if connect {
        net_shared_vm::release_delivery(&mut shared_state.borrow_mut().tcp, pending.socket);
    }
    result.map(|()| true)
}

fn cancel_native_requests(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    socket: u64,
    status: i32,
) -> Result<(), String> {
    let requests = shared_state
        .borrow()
        .native_stream
        .requests
        .iter()
        .filter_map(|(request, p)| (p.socket == socket).then_some(*request))
        .collect::<Vec<_>>();
    for request in requests {
        complete_native_request(runtime, program, shared_state, request, status)?;
    }
    Ok(())
}

#[cfg(test)]
mod borrow_tests {
    use super::with_released_shared_borrow;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn shared_state_borrow_is_released_before_reentrant_callback() {
        let state = Rc::new(RefCell::new(vec![1]));
        let removed = with_released_shared_borrow(&state, |values| values.pop());
        assert_eq!(removed, Some(1));

        // Parser error delivery may synchronously invoke listeners which
        // access the shared state again.
        state.borrow_mut().push(2);
        assert_eq!(*state.borrow(), vec![2]);
    }
}
