//! The shared TCP and local-socket transport's event pump: each turn it flushes the writes,
//! ends and destroys the sockets queued, then delivers the transport's events to the public
//! `net` objects and native stream handles that own them.

use crate::host::{NodeHost, SharedNodeState};
use crate::modules::net_shared_vm;
use quench_runtime::{RootId, Runtime, Value};
use std::cell::RefCell;
use std::rc::Rc;

#[inline(never)]
pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let quiescent = {
        let host = runtime.host_mut();
        let state = shared_state.borrow();
        // Zero capacity also proves emptiness, while the general path still releases
        // allocated empty command buffers as before.
        host.net_pending_writes.capacity() == 0
            && host.net_pending_ends.capacity() == 0
            && host.net_pending_destroys.capacity() == 0
            && state.net_servers.is_empty()
            && state.tcp.is_quiescent()
    };
    if quiescent {
        return Ok(false);
    }
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
    let mut progressed = emit_net_server_events(runtime, shared_state)?;
    for event in events {
        match event {
            net_shared_vm::TransportEvent::Accepted { listener, socket } => {
                let (net_server, native_handle) = {
                    let host = shared_state.borrow();
                    (
                        host.net_servers.get(&listener).map(|server| server.root),
                        host.native_tcp_listeners.get(&listener).copied(),
                    )
                };
                if net_server.is_some() || native_handle.is_some() {
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
            net_shared_vm::TransportEvent::Session { socket, session } => {
                progressed |= queue_tls_announcement(runtime, shared_state, socket, "session", &session)?;
            }
            net_shared_vm::TransportEvent::Keylog { socket, line } => {
                progressed |= queue_tls_announcement(runtime, shared_state, socket, "keylog", &line)?;
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
                let net_root = shared_state.borrow().net_sockets.get(&socket).map(|socket| socket.root);
                if let Some(root) = net_root {
                    queue_connection_event(runtime, shared_state, root, "connect", &[], socket)?;
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::ConnectError { socket, failure } => {
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
                let net_socket = shared_state.borrow().net_sockets.get(&socket).map(|socket| socket.root);
                if let Some(root) = net_socket {
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
            }
            net_shared_vm::TransportEvent::End { socket } => {
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
                let net_root = shared_state.borrow().net_sockets.get(&socket).map(|socket| socket.root);
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
                }
            }
            net_shared_vm::TransportEvent::DatagramCloseCompleted { id } => {
                progressed |= crate::modules::udp_binding::queue_close(runtime, shared_state, id)?;
            }
            net_shared_vm::TransportEvent::CloseCompleted { socket } => {
                cancel_native_requests(runtime, program, shared_state, socket, -libc::ECANCELED)?;
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
                let raw_callback = shared_state.borrow_mut().net_handle_close_callbacks.remove(&socket);
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
                return Err(format!("shared socket failed: {message}"));
            }
        }
    }
    Ok(progressed)
}

pub(crate) fn cleanup(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) {
    let roots = {
        let mut host = shared_state.borrow_mut();
        let mut roots = Vec::new();
        for socket in host.net_sockets.values_mut() {
            roots.extend(socket.write_callbacks.drain(..));
        }
        roots.extend(host.net_closing_sockets.drain().map(|(_, root)| root));
        net_shared_vm::cleanup(&mut host.tcp);
        roots
    };
    for root in roots {
        runtime.release_root(root);
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

/// Queue `socket.emit(name, Buffer)` for bytes its TLS session reported; whether the socket
/// was still live to hear it.
fn queue_tls_announcement(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    socket: u64,
    name: &str,
    bytes: &[u8],
) -> Result<bool, String> {
    let root = shared_state.borrow().net_sockets.get(&socket).map(|socket| socket.root);
    let Some(root) = root else {
        return Ok(false);
    };
    let chunk = buffer_from_bytes(runtime, bytes)?;
    let queued = queue_net_event(runtime, shared_state, root, name, &[chunk]);
    runtime.release_root(chunk);
    queued.map(|()| true)
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

/// A Buffer holding a copy of `bytes`: a Uint8Array owning the one copy, and
/// `Buffer.from(arrayBuffer)` over its store, which Buffer shares rather than copies.
fn buffer_from_bytes(runtime: &mut Runtime<NodeHost>, bytes: &[u8]) -> Result<RootId, String> {
    let view = runtime.uint8_array_from_rooted(bytes.to_vec()).map_err(|error| error.to_string())?;
    let store_key = runtime.string_rooted("buffer");
    let store = runtime.get_property_rooted(view, store_key);
    runtime.release_root(store_key);
    runtime.release_root(view);
    let store = store.map_err(|error| error.to_string())?;
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let buffer_key = runtime.string_rooted("Buffer");
    let buffer = runtime.get_property_rooted(global, buffer_key);
    runtime.release_root(buffer_key);
    runtime.release_root(global);
    let buffer = buffer.map_err(|error| error.to_string())?;
    let from_key = runtime.string_rooted("from");
    let from = runtime.get_property_rooted(buffer, from_key);
    runtime.release_root(from_key);
    let chunk = from.and_then(|from| {
        let chunk = runtime.call_rooted(from, buffer, &[store]);
        runtime.release_root(from);
        chunk
    });
    runtime.release_root(buffer);
    runtime.release_root(store);
    chunk.map_err(|error| error.to_string())
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
    let result = runtime.set_named_property_rooted(object, name, value);
    runtime.release_root(value);
    accepted(result.map_err(|error| format!("HTTP property {name}: {error}"))?)
}

/// A required property write's outcome: a refusal is an error.
fn accepted(written: bool) -> Result<(), String> {
    if written {
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
