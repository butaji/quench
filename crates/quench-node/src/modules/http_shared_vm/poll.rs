use super::state::{Response, ServerConnection};
use crate::host::{NodeHost, SharedNodeState};
use crate::modules::net_shared_vm;
use quench_runtime::{RootId, Runtime, Value};
use std::cell::RefCell;
use std::rc::Rc;

const REQUEST_HEAD_LIMIT: usize = 64 * 1024;
const RESPONSE_HEAD_LIMIT: usize = 64 * 1024;
const BAD_REQUEST_RESPONSE: &[u8] = b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n";

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
    net_shared_vm::end(&mut state.tcp, socket)
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
        for (socket, bytes) in pending_writes {
            if net_shared_vm::is_connected(&host.tcp, socket) {
                net_shared_vm::write_with_completion(&mut host.tcp, socket, &bytes)?;
            } else if host.net_sockets.contains_key(&socket) {
                deferred_writes.push((socket, bytes));
            }
        }
        for socket in pending_ends {
            if net_shared_vm::is_connected(&host.tcp, socket) {
                net_shared_vm::end(&mut host.tcp, socket)?;
            } else if host.net_sockets.contains_key(&socket) {
                deferred_ends.push(socket);
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
                        .then(|| host.net_servers.remove(&server_id).map(|server| server.root))
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
            net_shared_vm::TransportEvent::Accepted {
                listener,
                socket,
            } => {
                let (server_id, net_server) = {
                    let host = shared_state.borrow();
                    (
                        host.http
                            .servers
                            .iter()
                            .find_map(|(id, server)| (server.listener == Some(listener)).then_some(*id)),
                        host.net_servers.get(&listener).map(|server| server.root),
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
                        },
                    );
                    if let Some(server) = host.http.servers.get_mut(&server_id) {
                        server.connections.insert(socket);
                    }
                    progressed = true;
                } else if let Some(server_root) = net_server {
                    let create = runtime.host_function(crate::host::shared_vm::operation("netSocketCreateHandle"))
                        .map_err(|error| error.to_string())?;
                    let undefined = runtime.root(Value::UNDEFINED);
                    let id = runtime.root(Value::number(socket as f64));
                    let client_handle = runtime.call_rooted(create, undefined, &[id]);
                    runtime.release_root(create);
                    runtime.release_root(undefined);
                    runtime.release_root(id);
                    let client_handle = client_handle.map_err(|error| error.to_string())?;
                    let key = runtime.string_rooted("_handle");
                    let server_handle = runtime.get_property_rooted(server_root, key).map_err(|error| error.to_string())?;
                    runtime.release_root(key);
                    let success = runtime.root(Value::number(0.0));
                    queue_net_method_completion(runtime, shared_state, server_handle, "onconnection", &[success, client_handle], Some(NetDelivery::Connection(socket)))?;
                    runtime.release_root(success);
                    runtime.release_root(client_handle);
                    runtime.release_root(server_handle);
                    progressed = true;
                } else {
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                }
            }
            net_shared_vm::TransportEvent::Secure { socket } => {
                let root = shared_state.borrow().net_sockets.get(&socket).map(|socket| socket.root);
                if let Some(root) = root { queue_net_event(runtime, shared_state, root, "secure", &[])?; progressed = true; }
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
                    fail_client_exchange(runtime, program, client, &message)?;
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
                let net_socket = shared_state.borrow().net_sockets.get(&socket).map(|socket| {
                    (socket.root, socket.encoding.clone().unwrap_or_default())
                });
                if let Some((root, _encoding)) = net_socket {
                    let chunk = buffer_from_bytes(runtime, &bytes)?;
                    queue_net_method_completion(runtime, shared_state, root, "push", &[chunk], Some(NetDelivery::Data(socket, bytes.len())))?;
                    runtime.release_root(chunk);
                    progressed = true;
                    continue;
                }
                net_shared_vm::release_delivery(&mut shared_state.borrow_mut().tcp, socket);
                if shared_state.borrow().http.connections.contains_key(&socket) {
                    let dispatch = {
                        let mut host = shared_state.borrow_mut();
                        let Some(connection) = host.http.connections.get_mut(&socket) else {
                            continue;
                        };
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
                        if matches!(
                            message.body,
                            Err(super::protocol::RequestFramingError::InvalidTransferCodingOrder)
                        ) {
                            reject_request(shared_state, socket)?;
                            progressed = true;
                            continue;
                        }
                        let body_invalid = message.body.is_err();
                        let request_async_id =
                            crate::modules::async_hooks_shared_vm::create_context(shared_state);
                        let (server_root, response_id, response_factory, incoming_factory) = {
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
                            let response_factory = host
                                .http
                                .response_factory
                                .ok_or_else(|| "HTTP response factory is unavailable".to_owned())?;
                            let incoming_factory = host.http.incoming_factory.ok_or_else(|| {
                                "HTTP incoming-message factory is unavailable".to_owned()
                            })?;
                            let http10 = message.version == "1.0";
                            let close_after_response = http10 || message.headers.iter().any(|(name, value)| {
                                name.eq_ignore_ascii_case("connection")
                                    && value.eq_ignore_ascii_case("close")
                            });
                            host.http
                                .connections
                                .get_mut(&socket)
                                .unwrap()
                                .request_dispatched = true;
                            host.http.responses.insert(
                                response_id,
                                Response {
                                    socket,
                                    async_id: request_async_id,
                                    headers: if close_after_response {
                                        vec![("Connection".to_owned(), "close".to_owned())]
                                    } else {
                                        Vec::new()
                                    },
                                    body: Default::default(),
                                    send_date: true,
                                    http10,
                                    use_chunked_encoding_by_default: !http10 || message.headers.iter().any(|(name, value)| name.eq_ignore_ascii_case("te") && value.split(',').any(|token| token.trim().eq_ignore_ascii_case("chunked"))),
                                    lifecycle: super::state::ResponseLifecycle::Open,
                                },
                            );
                            (server_root, response_id, response_factory, incoming_factory)
                        };
                        let socket_object =
                            runtime.object_rooted().map_err(|error| error.to_string())?;
                        let request = match request_object(
                            runtime,
                            message.method,
                            message.target,
                            message.version,
                            message.headers,
                            message.raw_headers,
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
                            &[request, body, open, transport, server_side],
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
                        let id = runtime.root(Value::number(response_id as f64));
                        let status = runtime.root(Value::number(200.0));
                        let undefined = runtime.root(Value::UNDEFINED);
                        let response = match runtime.call_rooted(
                            response_factory,
                            undefined,
                            &[id, status, readable, server_root, socket_object],
                        ) {
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
                            let _scope = crate::modules::async_hooks_shared_vm::enter_context(
                                shared_state,
                                request_async_id,
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
                    let (progress, request_id, request_root, response_root) = {
                        let mut host = shared_state.borrow_mut();
                        let client = host.http.clients.get_mut(&socket).unwrap();
                        let progress = client.response_parser.push(&bytes)?;
                        if client.response_parser.pending_head_len() > RESPONSE_HEAD_LIMIT {
                            return Err("HTTP response head exceeds the shared parser limit".into());
                        }
                        (
                            progress,
                            client.request_id,
                            client.request_root,
                            client.response_root,
                        )
                    };
                    let progressed_response =
                        progress.head.is_some() || !progress.body_chunks.is_empty();
                    let response_root = match (response_root, progress.head) {
                        (Some(response_root), _) => Some(response_root),
                        (None, Some(message)) => {
                            let response_root = deliver_client_response_head(
                                runtime,
                                program,
                                shared_state,
                                request_id,
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
                        deliver_client_body(
                            runtime,
                            program,
                            response_root,
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
                            let finish_result = if let Some(response_root) = client.response_root {
                                let result =
                                    finish_client_response(runtime, program, response_root);
                                runtime.release_root(response_root);
                                result
                            } else {
                                Ok(())
                            };
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
                        }
                        progressed = true;
                    } else if progressed_response {
                        progressed = true;
                    }
                }
            }
            net_shared_vm::TransportEvent::End { socket } => {
                let net_root = {
                    shared_state
                        .borrow()
                        .net_sockets
                        .get(&socket)
                        .map(|socket| socket.root)
                };
                if let Some(root) = net_root {
                    let eof = runtime.root(Value::NULL);
                    queue_net_method_completion(runtime, shared_state, root, "push", &[eof], Some(NetDelivery::Eof))?;
                    runtime.release_root(eof);
                    progressed = true;
                    continue;
                }
                let client = shared_state.borrow_mut().http.clients.remove(&socket);
                if let Some(mut client) = client {
                    match client.response_parser.finish() {
                        Ok(progress) => {
                            let mut response_root = client.response_root;
                            let result: Result<(), String> = (|| {
                                if response_root.is_none() {
                                    if let Some(message) = progress.head {
                                        response_root = Some(deliver_client_response_head(
                                            runtime,
                                            program,
                                            shared_state,
                                            client.request_id,
                                            client.request_root,
                                            message,
                                        )?);
                                    }
                                }
                                if let Some(response_root) = response_root {
                                    deliver_client_body(
                                        runtime,
                                        program,
                                        response_root,
                                        &progress.body_chunks,
                                    )?;
                                    if progress.complete {
                                        finish_client_response(runtime, program, response_root)?;
                                    }
                                }
                                Ok(())
                            })();
                            if let Some(response_root) = response_root {
                                runtime.release_root(response_root);
                            }
                            runtime.release_root(client.request_root);
                            result?;
                        }
                        Err(_) => {
                            fail_client_exchange(runtime, program, client, "socket hang up")?;
                        }
                    }
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                    progressed = true;
                }
                let (server_id, contexts) = {
                    let mut host = shared_state.borrow_mut();
                    let server_id = host
                        .http
                        .connections
                        .remove(&socket)
                        .map(|connection| connection.server);
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
                    (server_id, contexts)
                };
                for (_, async_id) in contexts {
                    crate::modules::async_hooks_shared_vm::clear_context(
                        runtime,
                        shared_state,
                        async_id,
                    );
                }
                if server_id.is_some() {
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::CloseCompleted { socket } => {
                let (root, callback) = {
                    let mut state = shared_state.borrow_mut();
                    (
                        state.net_closing_sockets.remove(&socket),
                        state.net_socket_close_completed,
                    )
                };
                let raw_callback = shared_state.borrow_mut().net_handle_close_callbacks.remove(&socket);
                if let Some(handle) = raw_callback {
                    let callback = runtime.host_function(crate::host::shared_vm::operation("netHandleCloseComplete"))
                        .map_err(|error| error.to_string())?;
                    shared_state.borrow_mut().scheduler.queue_shared_next_tick(
                        crate::modules::shared_event_loop::SharedCallback { callback, receiver: handle, args: Vec::new() },
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
                // The OS error owns errno; presentation never recovers it from
                // localized text. Derive the Node code and syscall message.
                let ssl_error=error.get_ref().and_then(|error|error.downcast_ref::<openssl::ssl::Error>());
                let native_io=ssl_error.and_then(|error|error.io_error()).unwrap_or(&error);
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
                    if let Some(stack)=ssl_error.and_then(|error|error.ssl_error()) {
                        shared_state.borrow_mut().tls_contexts.transport_errors.push_back((socket,stack.clone()));
                        let callback=runtime.host_function(crate::host::shared_vm::operation("tlsSocketTransportError")).map_err(|error|error.to_string())?;
                        let receiver=runtime.root(runtime.rooted_value(net_root).ok_or("TLS error socket unavailable")?);
                        let id=runtime.root(Value::number(socket as f64));
                        shared_state.borrow_mut().scheduler.queue_shared_next_tick(crate::modules::shared_event_loop::SharedCallback{callback,receiver,args:vec![id]});
                        progressed=true;continue;
                    }
                    let error = net_socket_error_with_code(runtime, &message, &code)?;
                    set_text(runtime, error, "syscall", syscall)?;
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
                    fail_client_exchange(runtime, program, client, &message)?;
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
    progressed |= finish_closed_servers(runtime, program, shared_state)?;
    Ok(progressed)
}

fn deliver_client_response_head(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    request_id: u64,
    request_root: RootId,
    message: super::protocol::ResponseMessage,
) -> Result<RootId, String> {
    let response = response_object(
        runtime,
        message.status,
        message.message,
        message.headers,
        message.raw_headers,
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
    let emitted = emit_event(runtime, program, request_root, "response", &[readable]);
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

fn deliver_client_body(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    readable: RootId,
    chunks: &[Vec<u8>],
) -> Result<(), String> {
    for bytes in chunks {
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
    readable: RootId,
) -> Result<(), String> {
    set_bool(runtime, readable, "complete", true)?;
    let key = runtime.string_rooted("push");
    let push = runtime
        .get_property_rooted(readable, key)
        .map_err(|error| error.to_string())?;
    let end = runtime.root(Value::NULL);
    let result = runtime.call_rooted(push, readable, &[end]);
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

fn fail_client_exchange(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    client: super::state::Client,
    message: &str,
) -> Result<(), String> {
    let error = match client_exchange_error(runtime, program, message) {
        Ok(error) => error,
        Err(message) => {
            if let Some(response) = client.response_root {
                runtime.release_root(response);
            }
            runtime.release_root(client.request_root);
            return Err(message);
        }
    };
    let stream = client.response_root.unwrap_or(client.request_root);
    let result = destroy_stream_with_error(runtime, program, stream, error);
    runtime.release_root(error);
    if let Some(response) = client.response_root {
        runtime.release_root(response);
    }
    runtime.release_root(client.request_root);
    result
}

fn client_exchange_error(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    message: &str,
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
    let set_code = set_text(runtime, error, "code", "ECONNRESET");
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
enum NetDelivery { Connection(u64), Connect(u64), Data(u64, usize), Eof }
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
    // IncomingMessage consumers use socket.readable to distinguish a fully
    // received message from a finished socket. This connection remains
    // readable after the request framing has completed, just as Node's
    // keep-alive socket does.
    set_bool(runtime, socket, "readable", true)?;
    set_bool(runtime, socket, "writable", true)?;
    let socket = duplicate_root(runtime, socket)?;
    set_named(runtime, request, "socket", socket)?;
    set_bool(runtime, request, "complete", true)?;
    set_bool(runtime, request, "readable", true)?;
    Ok(request)
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
    set(runtime, object, key, value)
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
