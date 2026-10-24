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
    let (pending_failures, started) = {
        let mut host = shared_state.borrow_mut();
        super::client::expire_idle_sockets(&mut host);
        super::client::drain_pending(&mut host)
    };
    for (client, message) in pending_failures {
        fail_client_exchange(runtime, program, client, &message)?;
    }
    let mut progressed = started;
    progressed |= emit_listening(runtime, program, shared_state)?;
    progressed |= emit_net_server_events(runtime, shared_state)?;
    progressed |= emit_response_drains(runtime, shared_state)?;
    for event in events {
        match event {
            net_shared_vm::TransportEvent::Accepted {
                listener,
                socket,
            } => {
                let (server_id, net_server, native_handle) = {
                    let host = shared_state.borrow();
                    (
                        host.http
                            .servers
                            .iter()
                            .find_map(|(id, server)| (server.listener == Some(listener)).then_some(*id)),
                        host.net_servers.get(&listener).map(|server| server.root),
                        host.native_tcp_listeners.get(&listener).copied(),
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
                            upgraded: false,
                        },
                    );
                    if let Some(server) = host.http.servers.get_mut(&server_id) {
                        server.connections.insert(socket);
                    }
                    progressed = true;
                } else if net_server.is_some() || native_handle.is_some() {
                    let server_handle = if let Some(handle) = native_handle {
                        runtime.root(runtime.rooted_value(handle).ok_or("native TCP listener expired")?)
                    } else {
                        let key = runtime.string_rooted("_handle");
                        let handle = runtime.get_property_rooted(net_server.expect("public server"), key).map_err(|error| error.to_string())?;
                        runtime.release_root(key);
                        handle
                    };
                    let create = runtime.host_function(crate::host::shared_vm::operation("netSocketCreateHandle"))
                        .map_err(|error| error.to_string())?;
                    let undefined = runtime.root(Value::UNDEFINED);
                    let id = runtime.root(Value::number(socket as f64));
                    let client_handle = runtime.call_rooted(create, undefined, &[id,server_handle]);
                    runtime.release_root(create);
                    runtime.release_root(undefined);
                    runtime.release_root(id);
                    let client_handle = client_handle.map_err(|error| error.to_string())?;

                    let success = runtime.root(Value::number(0.0));
                    if let Some(resource)=super::super::native_async_wrap::tcp_resource(shared_state,listener) {
                        let key=runtime.string_rooted("onconnection");
                        let receiver=runtime.root(runtime.rooted_value(server_handle).expect("retained native listener"));
                        let resource_root=runtime.root(runtime.rooted_value(server_handle).expect("retained native listener resource"));
                        let status=runtime.root(Value::number(0.0));
                        let client=runtime.root(runtime.rooted_value(client_handle).expect("accepted native TCP handle"));
                        let callback=runtime.root(Value::UNDEFINED);
                        let invoked=super::super::process_shared_vm::invoke_async_callback(runtime,program,
                            crate::modules::shared_event_loop::SharedCallback {
                                execution:Some(crate::modules::shared_event_loop::CallbackExecution {
                                    lifetime:crate::modules::shared_event_loop::CallbackLifetime::Resource,
                                    optional_callback:Some(key),id:resource.id,resource:resource_root,
                                }),callback,receiver,args:vec![status,client],
                            });
                        net_shared_vm::release_delivery(&mut shared_state.borrow_mut().tcp,socket);
                        invoked?;
                    }else{
                        queue_net_method_completion(runtime, shared_state, server_handle, "onconnection", &[success, client_handle], Some(NetDelivery::Connection(socket)))?;
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
                let root = shared_state.borrow().net_sockets.get(&socket).map(|socket| socket.root);
                if let Some(root) = root { queue_net_event(runtime, shared_state, root, "secure", &[])?; progressed = true; }
            }
            net_shared_vm::TransportEvent::NativeConnectComplete { request, status } => {
                progressed |= complete_native_request(runtime, program, shared_state, request, status)?;
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
                if queue_native_read(runtime, shared_state, socket, Some(&bytes), bytes.len() as i32)? { progressed=true; continue; }
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
                        if message.upgrade {
                            dispatch_server_upgrade(
                                runtime,
                                program,
                                shared_state,
                                socket,
                                message,
                            )?;
                            progressed = true;
                            continue;
                        }
                        if matches!(
                            message.body,
                            Err(super::protocol::RequestFramingError::InvalidTransferCodingOrder)
                        ) {
                            reject_request(shared_state, socket)?;
                            progressed = true;
                            continue;
                        }
                        let body_invalid = message.body.is_err();
                        prune_collected_responses(runtime, shared_state);
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
                                    virtual_response: false,
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
                                    owner: None,
                                },
                            );
                            (server_root, response_id, response_factory, incoming_factory)
                        };
                        let socket_object =
                            runtime.object_rooted().map_err(|error| error.to_string())?;
                        let head = match request_head(
                            runtime,
                            message.method,
                            message.target,
                            message.version,
                            message.headers,
                            message.raw_headers,
                            socket_object,
                        ) {
                            Ok(head) => head,
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
                            &[head.as_slice(), &[body, open, transport, server_side]].concat(),
                        ) {
                            Ok(readable) => readable,
                            Err(error) => {
                                let message = runtime.format_error(program, &error.error);
                                if let Some(exception) = error.exception {
                                    runtime.release_root(exception);
                                }
                                release_roots(runtime, &head);
                                runtime.release_root(body);
                                runtime.release_root(open);
                                runtime.release_root(transport);
                                runtime.release_root(server_side);
                                runtime.release_root(undefined);
                                runtime.release_root(socket_object);
                                return Err(message);
                            }
                        };
                        release_roots(runtime, &head);
                        runtime.release_root(body);
                        runtime.release_root(open);
                        runtime.release_root(transport);
                        runtime.release_root(server_side);
                        runtime.release_root(undefined);
                        let id = runtime.root(Value::number(response_id as f64));
                        let status = runtime.root(Value::number(200.0));
                        let undefined = runtime.root(Value::UNDEFINED);
                        let response = match runtime.construct_rooted(
                            response_factory,
                            response_factory,
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
                        // The start message is made only for a channel with subscribers, as
                        // Node's server publishes it only then (`hasSubscribers`).
                        let subscribed = crate::modules::diagnostics_channel_shared_vm::channel_has_subscribers(
                            &shared_state.borrow(),
                            REQUEST_START_CHANNEL,
                        );
                        let diagnostic_message = match subscribed
                            .then(|| diagnostics_message(runtime, readable, response, server_root, socket_object))
                            .transpose()
                        {
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
                            let diagnostic_result = match diagnostic_message {
                                Some(message) => crate::modules::diagnostics_channel_shared_vm::publish_named_runtime(
                                    runtime,
                                    program,
                                    REQUEST_START_CHANNEL,
                                    message,
                                ),
                                None => Ok(()),
                            };
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
                        if let Some(message) = diagnostic_message {
                            runtime.release_root(message);
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
                    if progress
                        .head
                        .as_ref()
                        .is_some_and(|head| head.status == 101)
                    {
                        let head = progress.head.expect("101 response head was checked");
                        dispatch_client_upgrade(
                            runtime,
                            program,
                            shared_state,
                            socket,
                            request_id,
                            request_root,
                            head,
                            progress.upgrade_head.unwrap_or_default(),
                        )?;
                        progressed = true;
                        continue;
                    }
                    if let Some(head) = progress.head.as_ref() {
                        let reusable =
                            super::protocol::response_keep_alive(&head.version, &head.headers);
                        if !reusable {
                            if let Some(client) = shared_state
                                .borrow_mut()
                                .http
                                .clients
                                .get_mut(&socket)
                            {
                                client.keep_alive = false;
                            }
                        }
                    }
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
                            super::client::release_agent_socket(
                                &mut shared_state.borrow_mut(),
                                socket,
                                &client,
                            );
                            finish_result?;
                        }
                        progressed = true;
                    } else if progressed_response {
                        progressed = true;
                    }
                }
            }
            net_shared_vm::TransportEvent::End { socket } => {
                if queue_native_read(runtime, shared_state, socket, None, super::super::stream_binding::UV_EOF)? { progressed=true; continue; }
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
                    if client.reused && !client.response_parser.has_seen_data() {
                        // The free socket was closed by the peer before it answered.
                        // Node retries a reused socket once instead of surfacing a
                        // stale ECONNRESET to the guest.
                        if let Some(agent) = client.agent {
                            if let Some(sockets) =
                                shared_state.borrow_mut().http.agents.get_mut(&agent)
                            {
                                sockets.remove(&socket);
                            }
                        }
                        shared_state.borrow_mut().http.remove_idle_socket(socket);
                        net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                        client.reused = false;
                        let retry = {
                            let mut host = shared_state.borrow_mut();
                            super::client::retry_client(&mut host, client)
                        };
                        match retry {
                            Ok(()) => progressed = true,
                            Err((client, message)) => {
                                fail_client_exchange(runtime, program, client, &message)?;
                                progressed = true;
                            }
                        }
                        continue;
                    }
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
                if shared_state.borrow_mut().http.remove_idle_socket(socket) {
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
                    let contexts = host.http.take_socket_responses(socket);
                    if let Some(server_id) = server_id {
                        if let Some(server) = host.http.servers.get_mut(&server_id) {
                            server.connections.remove(&socket);
                        }
                    }
                    (server_id, contexts)
                };
                for response in contexts {
                    retire_response(runtime, shared_state, response);
                }
                if server_id.is_some() {
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::DatagramCloseCompleted { id } => {progressed |= crate::modules::udp_binding::queue_close(runtime,shared_state,id)?;}
            net_shared_vm::TransportEvent::CloseCompleted { socket } => {
                cancel_native_requests(runtime, program, shared_state, socket, -libc::ECANCELED)?;
                let reader=shared_state.borrow_mut().native_stream.readers.remove(&socket);
                if let Some(reader)=reader { runtime.release_root(reader); }
                let (root, callback) = {
                    let mut state = shared_state.borrow_mut();
                    (
                        state.net_closing_sockets.remove(&socket),
                        state.net_socket_close_completed,
                    )
                };
                let raw_callback = shared_state.borrow_mut().net_handle_close_callbacks.remove(&socket);
                let (server_id, contexts) = {
                    let mut host = shared_state.borrow_mut();
                    let server_id = host
                        .http
                        .connections
                        .remove(&socket)
                        .map(|connection| connection.server);
                    let contexts = host.http.take_socket_responses(socket);
                    host.http.remove_idle_socket(socket);
                    if let Some(server_id) = server_id {
                        if let Some(server) = host.http.servers.get_mut(&server_id) {
                            server.connections.remove(&socket);
                        }
                    }
                    (server_id, contexts)
                };
                for response in contexts {
                    retire_response(runtime, shared_state, response);
                }
                if server_id.is_some() {
                    progressed = true;
                }
                if let Some(handle) = raw_callback {
                    let callback = runtime.host_function(crate::host::shared_vm::operation("netHandleCloseComplete"))
                        .map_err(|error| error.to_string())?;
                    let execution=if let Some(resource)=crate::modules::native_async_wrap::tcp_resource(shared_state,socket){
                        let owner=runtime.root(runtime.rooted_value(handle).ok_or("native close handle expired")?);
                        let symbol=shared_state.borrow().net_handle_onclose_symbol.ok_or("native TCP close symbol missing")?;
                        let key=runtime.root(runtime.rooted_value(symbol).ok_or("native TCP close symbol expired")?);
                        Some(crate::modules::shared_event_loop::CallbackExecution{id:resource.id,resource:owner,lifetime:crate::modules::shared_event_loop::CallbackLifetime::Operation,optional_callback:Some(key)})
                    }else{None};
                    shared_state.borrow_mut().scheduler.queue_shared_next_tick(
                        crate::modules::shared_event_loop::SharedCallback { execution, callback, receiver: handle, args: Vec::new() },
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
                        crate::modules::shared_event_loop::SharedCallback { execution: None,
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
                let raw=shared_state.borrow().native_stream.readers.contains_key(&socket)
                    || shared_state.borrow().native_stream.requests.iter().any(|(_,pending)|pending.socket==socket);
                if raw {
                    let status=-error.raw_os_error().unwrap_or(libc::EIO);
                    cancel_native_requests(runtime,program,shared_state,socket,status)?;
                    if syscall=="read" { queue_native_read(runtime,shared_state,socket,None,status)?; }
                    progressed=true;continue;
                }
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
                        shared_state.borrow_mut().scheduler.queue_shared_next_tick(crate::modules::shared_event_loop::SharedCallback{execution: None,callback,receiver,args:vec![id]});
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
                    let contexts = host.http.take_socket_responses(socket);
                    if let Some(server_id) = server_id {
                        if let Some(server) = host.http.servers.get_mut(&server_id) {
                            server.connections.remove(&socket);
                        }
                    }
                    (client, server_id, contexts)
                };
                let client_was_present = client.is_some();
                if let Some(mut client) = client {
                    if client.reused && !client.response_parser.has_seen_data() {
                        if let Some(agent) = client.agent {
                            if let Some(sockets) =
                                shared_state.borrow_mut().http.agents.get_mut(&agent)
                            {
                                sockets.remove(&socket);
                            }
                        }
                        net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
                        client.reused = false;
                        let retry = {
                            let mut host = shared_state.borrow_mut();
                            super::client::retry_client(&mut host, client)
                        };
                        if let Err((client, message)) = retry {
                            fail_client_exchange(runtime, program, client, &message)?;
                        }
                    } else {
                        fail_client_exchange(runtime, program, client, &message)?;
                    }
                }
                shared_state.borrow_mut().http.remove_idle_socket(socket);
                for response in contexts {
                    retire_response(runtime, shared_state, response);
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
    let head = response_head(
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
        &[head.as_slice(), &[body, open, exchange_id]].concat(),
    ) {
        Ok(readable) => readable,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            release_roots(runtime, &head);
            runtime.release_root(body);
            runtime.release_root(open);
            runtime.release_root(exchange_id);
            runtime.release_root(undefined);
            return Err(message);
        }
    };
    let emitted = emit_event(runtime, program, request_root, "response", &[readable]);
    release_roots(runtime, &head);
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

fn dispatch_server_upgrade(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    socket_id: u64,
    message: super::protocol::RequestMessage,
) -> Result<(), String> {
    let (server_id, incoming_factory) = {
        let mut host = shared_state.borrow_mut();
        let server_id = host
            .http
            .connections
            .get(&socket_id)
            .map(|connection| connection.server)
            .ok_or_else(|| "HTTP upgrade connection disappeared".to_owned())?;
        if !host.http.servers.contains_key(&server_id) {
            return Err("HTTP upgrade server was released early".to_owned());
        }
        let incoming_factory = host
            .http
            .incoming_factory
            .ok_or_else(|| "HTTP incoming-message factory is unavailable".to_owned())?;
        let connection = host.http.connections.get_mut(&socket_id).unwrap();
        connection.request_dispatched = true;
        connection.upgraded = true;
        connection.received.clear();
        (server_id, incoming_factory)
    };
    let create_socket = runtime
        .host_function(crate::host::shared_vm::operation("httpUpgradeSocket"))
        .map_err(|error| error.to_string())?;
    let undefined = runtime.root(Value::UNDEFINED);
    let id = runtime.root(Value::number(socket_id as f64));
    let socket = match runtime.call_rooted(create_socket, undefined, &[id]) {
        Ok(socket) => socket,
        Err(error) => {
            runtime.release_root(create_socket);
            runtime.release_root(undefined);
            runtime.release_root(id);
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            return Err(message);
        }
    };
    runtime.release_root(create_socket);
    runtime.release_root(id);
    // The socket adoption host call can run constructors and user hooks. Fetch
    // the server's retained root after that call, then make an operation-local
    // root for the remaining dispatch.
    let server_root = shared_state
        .borrow()
        .http
        .servers
        .get(&server_id)
        .map(|server| server.root)
        .ok_or("HTTP upgrade server was released during socket adoption")?;
    let server_value = runtime
        .rooted_value(server_root)
        .ok_or("HTTP upgrade server retained root is no longer live")?;
    let server = runtime.root(server_value);
    let socket_server = runtime.root(server_value);
    set_named(runtime, socket, "server", server)?;
    set_named(runtime, socket, "_server", socket_server)?;
    let head = match request_head(
        runtime,
        message.method,
        message.target,
        message.version,
        message.headers,
        message.raw_headers,
        socket,
    ) {
        Ok(head) => head,
        Err(error) => {
            runtime.release_root(socket);
            runtime.release_root(server);
            runtime.release_root(undefined);
            return Err(error);
        }
    };
    let body = runtime.string_rooted("");
    let open = runtime.root(Value::TRUE);
    let transport = runtime.root(Value::number(socket_id as f64));
    let server_side = runtime.root(Value::TRUE);
    let readable = match runtime.call_rooted(
        incoming_factory,
        undefined,
        &[head.as_slice(), &[body, open, transport, server_side]].concat(),
    ) {
        Ok(readable) => readable,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            release_roots(runtime, &head);
            runtime.release_root(body);
            runtime.release_root(open);
            runtime.release_root(transport);
            runtime.release_root(server_side);
            runtime.release_root(socket);
            runtime.release_root(server);
            runtime.release_root(undefined);
            return Err(message);
        }
    };
    release_roots(runtime, &head);
    runtime.release_root(body);
    runtime.release_root(open);
    runtime.release_root(transport);
    runtime.release_root(server_side);
    let upgrade_head = buffer_from_bytes(runtime, &message.upgrade_head)?;
    let request_async_id = crate::modules::async_hooks_shared_vm::create_context(shared_state);
    let emitted = {
        let _scope =
            crate::modules::async_hooks_shared_vm::enter_context(shared_state, request_async_id);
        emit_event(
            runtime,
            program,
            server_root,
            "upgrade",
            &[readable, socket, upgrade_head],
        )
    };
    runtime.release_root(upgrade_head);
    runtime.release_root(readable);
    runtime.release_root(socket);
    runtime.release_root(server);
    runtime.release_root(undefined);
    emitted
}

fn dispatch_client_upgrade(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    socket_id: u64,
    request_id: u64,
    request_root: RootId,
    message: super::protocol::ResponseMessage,
    upgrade_head: Vec<u8>,
) -> Result<(), String> {
    let incoming_factory = shared_state
        .borrow()
        .http
        .incoming_factory
        .ok_or_else(|| "HTTP incoming-message factory is unavailable".to_owned())?;
    let create_socket = runtime
        .host_function(crate::host::shared_vm::operation("httpUpgradeSocket"))
        .map_err(|error| error.to_string())?;
    let undefined = runtime.root(Value::UNDEFINED);
    let id = runtime.root(Value::number(socket_id as f64));
    let socket = match runtime.call_rooted(create_socket, undefined, &[id]) {
        Ok(socket) => socket,
        Err(error) => {
            runtime.release_root(create_socket);
            runtime.release_root(undefined);
            runtime.release_root(id);
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            return Err(message);
        }
    };
    runtime.release_root(create_socket);
    runtime.release_root(id);
    let head = super::incoming::MessageHead {
        request: None,
        status: Some((message.status, message.message)),
        version: CLIENT_RESPONSE_VERSION.to_owned(),
        raw_headers: message.raw_headers,
        socket: Some(socket),
        complete: true,
    }
    .arguments(runtime)?;
    let body = runtime.string_rooted("");
    let closed = runtime.root(Value::FALSE);
    let exchange_id = runtime.root(Value::number(request_id as f64));
    let readable = match runtime.call_rooted(
        incoming_factory,
        undefined,
        &[head.as_slice(), &[body, closed, exchange_id]].concat(),
    ) {
        Ok(readable) => readable,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            release_roots(runtime, &head);
            runtime.release_root(body);
            runtime.release_root(closed);
            runtime.release_root(exchange_id);
            runtime.release_root(socket);
            runtime.release_root(undefined);
            return Err(message);
        }
    };
    release_roots(runtime, &head);
    runtime.release_root(body);
    runtime.release_root(closed);
    runtime.release_root(exchange_id);
    let raw_head = buffer_from_bytes(runtime, &upgrade_head)?;
    let upgraded = {
        let mut host = shared_state.borrow_mut();
        let client = host.http.clients.remove(&socket_id);
        host.http.remove_idle_socket(socket_id);
        for sockets in host.http.agents.values_mut() {
            sockets.remove(&socket_id);
        }
        client
    };
    if let Some(response) = upgraded.and_then(|client| client.response_root) {
        runtime.release_root(response);
    }
    let emitted = emit_event(
        runtime,
        program,
        request_root,
        "upgrade",
        &[readable, socket, raw_head],
    );
    runtime.release_root(raw_head);
    runtime.release_root(readable);
    runtime.release_root(socket);
    runtime.release_root(undefined);
    runtime.release_root(request_root);
    emitted
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
    let (roots, responses) =
        {
            let mut host = shared_state.borrow_mut();
            let shared = std::mem::take(&mut host.http);
            let mut roots = Vec::new();
            roots.extend(shared.servers.into_values().map(|server| server.root));
            roots.extend(shared.clients.into_values().flat_map(|client| {
                std::iter::once(client.request_root).chain(client.response_root)
            }));
            let responses = shared.responses.into_values().collect::<Vec<_>>();
            roots.extend(shared.response_factory);
            roots.extend(shared.incoming_factory);
            roots.extend(shared.response_tracking.into_iter().flat_map(super::state::ResponseTracking::roots));
            for socket in host.net_sockets.values_mut() {
                roots.extend(socket.write_callbacks.drain(..));
            }
            roots.extend(host.net_closing_sockets.drain().map(|(_, root)| root));
            net_shared_vm::cleanup(&mut host.tcp);
            (roots, responses)
        };
    for root in roots {
        runtime.release_root(root);
    }
    for response in responses {
        retire_response(runtime, shared_state, response);
    }
}

/// Forget a response record: its owner reference and its request context.
fn retire_response(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
    response: super::state::Response,
) {
    if let Some(owner) = response.owner {
        runtime.release_host_weak_root(owner);
    }
    crate::modules::async_hooks_shared_vm::clear_context(runtime, shared_state, response.async_id);
}

/// Retire the records of ended responses whose guest objects were collected: no operation can
/// name them again. A keep-alive connection otherwise keeps every response it served until it
/// closes. Owners can only die in a collection, so this runs once per collection epoch.
fn prune_collected_responses(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) {
    let epoch = runtime.collection_epoch();
    let collected = {
        let mut host = shared_state.borrow_mut();
        if host.http.responses_pruned_epoch == epoch {
            return;
        }
        host.http.responses_pruned_epoch = epoch;
        let ids = host
            .http
            .responses
            .iter()
            .filter(|(_, response)| {
                response.lifecycle.is_terminal()
                    && response.owner.is_some_and(|owner| !runtime.host_weak_root_is_live(owner))
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| host.http.responses.remove(&id))
            .collect::<Vec<_>>()
    };
    for response in collected {
        retire_response(runtime, shared_state, response);
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
        crate::modules::shared_event_loop::SharedCallback { execution: None,
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

/// Settle responses waiting on socket backpressure: a socket that flushed
/// below the high-water mark owes its response `drain`; a closed socket owes
/// nothing, as Node never drains a destroyed stream.
fn emit_response_drains(
    runtime: &mut Runtime<NodeHost>,
    shared_state: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let settled = {
        let mut guard = shared_state.borrow_mut();
        let host = &mut *guard;
        if host.http.drain_waiters.is_empty() {
            return Ok(false);
        }
        let tcp = &host.tcp;
        let mut settled = Vec::new();
        host.http.drain_waiters.retain(|socket, waiter| {
            let queued = net_shared_vm::queued_bytes(tcp, *socket);
            let waiting = queued.is_some_and(|queued| {
                queued >= super::operations::RESPONSE_HIGH_WATER_MARK
            });
            if !waiting {
                settled.push((*waiter, queued.is_some()));
            }
            waiting
        });
        settled
    };
    let progressed = !settled.is_empty();
    for (response, open) in settled {
        if open {
            queue_net_event(runtime, shared_state, response, "drain", &[])?;
        }
        runtime.release_root(response);
    }
    Ok(progressed)
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
        crate::modules::shared_event_loop::SharedCallback { execution: None,
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

/// A server request's head as the incoming-message factory's leading arguments
/// (`incoming::MessageHead`), after marking the connection's socket object readable and
/// writable.
fn request_head(
    runtime: &mut Runtime<NodeHost>,
    method: String,
    path: String,
    version: String,
    _headers: Vec<(String, String)>,
    raw_headers: Vec<(String, String)>,
    socket: RootId,
) -> Result<Vec<RootId>, String> {
    // IncomingMessage consumers use socket.readable to distinguish a fully
    // received message from a finished socket. This connection remains
    // readable after the request framing has completed, just as Node's
    // keep-alive socket does.
    set_bool(runtime, socket, "readable", true)?;
    set_bool(runtime, socket, "writable", true)?;
    super::incoming::MessageHead {
        request: Some((method, path)),
        status: None,
        version,
        raw_headers,
        socket: Some(socket),
        complete: true,
    }
    .arguments(runtime)
}

/// The diagnostics channel a server's request start publishes to.
const REQUEST_START_CHANNEL: &str = "http.server.request.start";

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

/// A client response's head as the incoming-message factory's leading arguments
/// (`incoming::MessageHead`).
fn response_head(
    runtime: &mut Runtime<NodeHost>,
    status: u16,
    message: String,
    _headers: Vec<(String, String)>,
    raw_headers: Vec<(String, String)>,
    complete: bool,
) -> Result<Vec<RootId>, String> {
    super::incoming::MessageHead {
        request: None,
        status: Some((status, message)),
        version: CLIENT_RESPONSE_VERSION.to_owned(),
        raw_headers,
        socket: None,
        complete,
    }
    .arguments(runtime)
}

/// The HTTP version a client response reports.
const CLIENT_RESPONSE_VERSION: &str = "1.1";

fn release_roots(runtime: &mut Runtime<NodeHost>, roots: &[RootId]) {
    for root in roots {
        runtime.release_root(*root);
    }
}

pub(super) fn raw_header_array(
    runtime: &mut Runtime<NodeHost>,
    raw_headers: Vec<(String, String)>,
) -> Result<RootId, String> {
    let mut raw = Vec::with_capacity(raw_headers.len() * 2);
    for (name, value) in raw_headers {
        raw.push(runtime.string_rooted(&name));
        raw.push(runtime.string_rooted(&value));
    }
    let result = runtime.array_rooted(&raw);
    for value in raw {
        runtime.release_root(value);
    }
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
    let result = runtime.set_named_property_rooted(object, name, value);
    runtime.release_root(value);
    accepted(result.map_err(|error| error.to_string())?)
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
    if shared_state.borrow().net_sockets.contains_key(&socket) { return Ok(false); }
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
    let execution=if let Some(resource)=crate::modules::native_async_wrap::tcp_resource(shared_state,socket){
        let owner=runtime.root(runtime.rooted_value(handle).ok_or("native read handle expired")?);
        Some(crate::modules::shared_event_loop::CallbackExecution{id:resource.id,resource:owner,lifetime:crate::modules::shared_event_loop::CallbackLifetime::Resource,optional_callback:None})
    }else{None};
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
    let admitted=pending.lease.resource().expect("queued native request has an admitted AsyncWrap resource");
    let resource=runtime.root(runtime.rooted_value(request).expect("retained native request root"));
    let status_root=runtime.root(Value::number(f64::from(status)));
    let connect=pending.kind==super::super::stream_binding::RequestKind::Connect;
    let publish_status=runtime.root(if !connect {Value::TRUE}else{Value::FALSE});
    let prepare=runtime.host_function(crate::host::shared_vm::operation("nativeStreamPrepareCompletion"))
        .expect("registered native stream completion preparation");
    let prepared=runtime.call_rooted(prepare,request,&[status_root,publish_status]);
    runtime.release_root(prepare);runtime.release_root(publish_status);
    let undefined=runtime.root(Value::UNDEFINED);
    let mut callback=crate::modules::shared_event_loop::SharedCallback {
        execution:Some(crate::modules::shared_event_loop::CallbackExecution {
            lifetime:crate::modules::shared_event_loop::CallbackLifetime::Operation,
            optional_callback:None,id:admitted.id,resource,
        }),
        callback:undefined,receiver:request,args:vec![status_root,pending.handle],
    };
    if connect {
        callback.args.push(runtime.root(runtime.rooted_value(request).expect("retained connect request")));
        callback.args.push(runtime.root(if status==0 {Value::TRUE}else{Value::FALSE}));
        callback.args.push(runtime.root(if status==0 {Value::TRUE}else{Value::FALSE}));
    }else{callback.args.push(runtime.root(Value::UNDEFINED));}
    let result=match prepared {
        Ok(value) if runtime.rooted_value(value).is_some_and(|v|v.is_undefined())=>{
            runtime.release_root(value);
            super::super::process_shared_vm::release_callback(runtime,callback);
            Ok(())
        }
        Ok(value)=>{
            runtime.release_root(callback.callback);callback.callback=value;
            super::super::process_shared_vm::invoke_async_callback(runtime,program,callback)
        }
        Err(error)=>{
            let result=super::super::process_shared_vm::route_callback_failure(runtime,program,error);
            super::super::process_shared_vm::release_callback(runtime,callback);
            result
        }
    };
    if connect {net_shared_vm::release_delivery(&mut shared_state.borrow_mut().tcp,pending.socket);}
    result.map(|()|true)
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
