use super::state::{Response, ServerConnection};
use crate::host::{NodeHost, SharedNodeState};
use crate::modules::net_shared_vm;
use quench_runtime::{RootId, Runtime, Value};
use std::cell::RefCell;
use std::rc::Rc;

const REQUEST_HEAD_LIMIT: usize = 64 * 1024;
const RESPONSE_HEAD_LIMIT: usize = 64 * 1024;

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
        for (socket, bytes) in pending_writes {
            net_shared_vm::write(&mut host.tcp, socket, &bytes)?;
        }
        for socket in pending_ends {
            net_shared_vm::end(&mut host.tcp, socket)?;
        }
    }
    let destroyed = {
        let mut host = shared_state.borrow_mut();
        pending_destroys
            .into_iter()
            .filter_map(|socket_id| {
                let socket = host.net_sockets.remove(&socket_id)?;
                let closed_server = socket.parent_server.and_then(|server_id| {
                    let closed = host.net_servers.get_mut(&server_id).is_some_and(|server| {
                        server.connections.remove(&socket_id);
                        server.closing && server.connections.is_empty()
                    });
                    closed
                        .then(|| host.net_servers.remove(&server_id).map(|server| server.root))
                        .flatten()
                });
                Some((socket.root, closed_server))
            })
            .collect::<Vec<_>>()
    };
    for (socket, server) in destroyed {
        queue_net_event(runtime, shared_state, socket, "close", &[])?;
        runtime.release_root(socket);
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
                remote,
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
                    let constructor = shared_state
                        .borrow()
                        .net_socket_constructor
                        .ok_or_else(|| "net.Socket constructor is unavailable".to_owned())?;
                    let accepted = runtime
                        .construct_rooted(constructor, constructor, &[])
                        .map_err(|error| error.to_string())?;
                    let key = runtime.string_rooted("__quenchNetSocketId");
                    let id = runtime.root(Value::number(socket as f64));
                    if !runtime
                        .set_property_rooted(accepted, key, id, accepted)
                        .map_err(|error| error.to_string())?
                    {
                        return Err("cannot tag accepted net socket".into());
                    }
                    runtime.release_root(key);
                    runtime.release_root(id);
                    let remote_address = runtime.string_rooted(&remote.ip().to_string());
                    let remote_key = runtime.string_rooted("remoteAddress");
                    runtime
                        .set_property_rooted(accepted, remote_key, remote_address, accepted)
                        .map_err(|error| error.to_string())?;
                    runtime.release_root(remote_key);
                    runtime.release_root(remote_address);
                    {
                        let mut host = shared_state.borrow_mut();
                        host.net_sockets.insert(
                            socket,
                            crate::host::node_host::NetSocket {
                                root: accepted,
                                encoding: None,
                                parent_server: Some(listener),
                            },
                        );
                        if let Some(server) = host.net_servers.get_mut(&listener) {
                            server.connections.insert(socket);
                        }
                    }
                    queue_net_event(runtime, shared_state, server_root, "connection", &[accepted])?;
                    progressed = true;
                } else {
                    net_shared_vm::close_socket(&mut shared_state.borrow_mut().tcp, socket);
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
                    net_shared_vm::write(&mut shared_state.borrow_mut().tcp, socket, &request)
                        .map_err(|error| format!("HTTP client write failed: {error}"))?;
                    progressed = true;
                } else if let Some(root) = net_root {
                    queue_net_event(runtime, shared_state, root, "connect", &[])?;
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::ConnectError { socket, message } => {
                let client = shared_state.borrow_mut().http.clients.remove(&socket);
                if let Some(client) = client {
                    fail_client_exchange(runtime, program, client, &message)?;
                } else {
                    let net_socket = shared_state.borrow_mut().net_sockets.remove(&socket);
                    if let Some(net_socket) = net_socket {
                        let error = net_socket_error(runtime, &message)?;
                        queue_net_event(runtime, shared_state, net_socket.root, "error", &[error])?;
                        queue_net_event(runtime, shared_state, net_socket.root, "close", &[])?;
                        runtime.release_root(error);
                        runtime.release_root(net_socket.root);
                    }
                }
                progressed = true;
            }
            net_shared_vm::TransportEvent::Data { socket, bytes } => {
                let net_socket = shared_state.borrow().net_sockets.get(&socket).map(|socket| {
                    (socket.root, socket.encoding.clone().unwrap_or_default())
                });
                if let Some((root, _encoding)) = net_socket {
                    let text = String::from_utf8_lossy(&bytes);
                    let chunk = runtime.string_rooted(&text);
                    queue_net_event(runtime, shared_state, root, "data", &[chunk])?;
                    runtime.release_root(chunk);
                    progressed = true;
                    continue;
                }
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
                            let close_after_response = message.headers.iter().any(|(name, value)| {
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
                        let body_text = latin1_text(&message.body);
                        let body = runtime.string_rooted(&body_text);
                        let undefined = runtime.root(Value::UNDEFINED);
                        let readable = match runtime.call_rooted(
                            incoming_factory,
                            undefined,
                            &[request, body],
                        ) {
                            Ok(readable) => readable,
                            Err(error) => {
                                let message = runtime.format_error(program, &error.error);
                                if let Some(exception) = error.exception {
                                    runtime.release_root(exception);
                                }
                                runtime.release_root(request);
                                runtime.release_root(body);
                                runtime.release_root(undefined);
                                runtime.release_root(socket_object);
                                return Err(message);
                            }
                        };
                        runtime.release_root(request);
                        runtime.release_root(body);
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
                    queue_net_event(runtime, shared_state, root, "end", &[])?;
                    runtime.host_mut().net_pending_ends.push(socket);
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
            net_shared_vm::TransportEvent::Closed { socket } => {
                let (net_socket, closed_server) = {
                    let mut host = shared_state.borrow_mut();
                    let net_socket = host.net_sockets.remove(&socket);
                    let closed_server = net_socket
                        .as_ref()
                        .and_then(|net_socket| net_socket.parent_server)
                        .and_then(|server_id| {
                            let should_close = host.net_servers.get_mut(&server_id).is_some_and(
                                |server| {
                                    server.connections.remove(&socket);
                                    server.closing && server.connections.is_empty()
                                },
                            );
                            should_close
                                .then(|| host.net_servers.remove(&server_id).map(|server| server.root))
                                .flatten()
                        });
                    (net_socket, closed_server)
                };
                if let Some(net_socket) = net_socket {
                    queue_net_event(runtime, shared_state, net_socket.root, "close", &[])?;
                    runtime.release_root(net_socket.root);
                    progressed = true;
                }
                if let Some(server_root) = closed_server {
                    queue_net_event(runtime, shared_state, server_root, "close", &[])?;
                    runtime.release_root(server_root);
                    progressed = true;
                }
            }
            net_shared_vm::TransportEvent::Error { socket, message } => {
                let net_socket = shared_state.borrow_mut().net_sockets.remove(&socket);
                if let Some(net_socket) = net_socket {
                    let error = runtime.string_rooted(&message);
                    queue_net_event(runtime, shared_state, net_socket.root, "error", &[error])?;
                    queue_net_event(runtime, shared_state, net_socket.root, "close", &[])?;
                    runtime.release_root(error);
                    runtime.release_root(net_socket.root);
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

fn net_socket_error(runtime: &mut Runtime<NodeHost>, message: &str) -> Result<RootId, String> {
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let key = runtime.string_rooted("Error");
    let constructor = runtime
        .get_property_rooted(global, key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(key);
    runtime.release_root(global);
    let message_root = runtime.string_rooted(message);
    let error = runtime
        .construct_rooted(constructor, constructor, &[message_root])
        .map_err(|error| error.to_string())?;
    runtime.release_root(constructor);
    runtime.release_root(message_root);
    let code = message.split_whitespace().nth(1).unwrap_or("ECONNRESET");
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
    let key = runtime.string_rooted("emit");
    let callback = runtime
        .get_property_rooted(receiver, key)
        .map_err(|error| error.to_string())?;
    runtime.release_root(key);
    let callback = runtime.root(
        runtime
            .rooted_value(callback)
            .ok_or("net socket emit is unavailable")?,
    );
    let receiver = runtime.root(
        runtime
            .rooted_value(receiver)
            .ok_or("net socket is unavailable")?,
    );
    let event = runtime.string_rooted(name);
    let mut retained_args = vec![event];
    for arg in args {
        retained_args.push(runtime.root(
            runtime
                .rooted_value(*arg)
                .ok_or("net event argument is unavailable")?,
        ));
    }
    shared_state
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(crate::modules::shared_event_loop::SharedCallback {
            callback,
            receiver,
            args: retained_args,
        });
    Ok(())
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
    headers: Vec<(String, String)>,
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
    let headers_object = runtime
        .null_object_rooted()
        .map_err(|error| error.to_string())?;
    for (name, value) in headers {
        set_text(runtime, headers_object, &name, &value)?;
    }
    set_named(runtime, request, "headers", headers_object)?;
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
    headers: Vec<(String, String)>,
    raw_headers: Vec<(String, String)>,
    complete: bool,
) -> Result<RootId, String> {
    let response = runtime.object_rooted().map_err(|error| error.to_string())?;
    set_number(runtime, response, "statusCode", status as f64)?;
    set_text(runtime, response, "statusMessage", &message)?;
    set_text(runtime, response, "httpVersion", "1.1")?;
    let headers_object = runtime
        .null_object_rooted()
        .map_err(|error| error.to_string())?;
    for (name, value) in headers {
        set_text(runtime, headers_object, &name, &value)?;
    }
    set_named(runtime, response, "headers", headers_object)?;
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
