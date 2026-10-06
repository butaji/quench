use super::state::{Response, ServerConnection};
use crate::host::{HostState, NodeHost};
use crate::modules::net;
use rqj::{RootId, Runtime, Value};
use std::cell::RefCell;
use std::rc::Rc;

const REQUEST_HEAD_LIMIT: usize = 64 * 1024;
const RESPONSE_HEAD_LIMIT: usize = 64 * 1024;

pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<HostState>>,
) -> Result<bool, String> {
    let events = {
        let mut host = state.borrow_mut();
        net::shared_vm::poll(&mut host.net)
    };
    let mut progressed = false;
    progressed |= emit_listening(runtime, program, state)?;
    for event in events {
        match event {
            net::shared_vm::TransportEvent::Accepted { listener, socket } => {
                let server_id = state
                    .borrow()
                    .http
                    .shared
                    .servers
                    .iter()
                    .find_map(|(id, server)| (server.listener == Some(listener)).then_some(*id));
                if let Some(server_id) = server_id {
                    let mut host = state.borrow_mut();
                    host.http.shared.connections.insert(
                        socket,
                        ServerConnection {
                            server: server_id,
                            received: Vec::new(),
                            request_dispatched: false,
                        },
                    );
                    if let Some(server) = host.http.shared.servers.get_mut(&server_id) {
                        server.connections.insert(socket);
                    }
                    progressed = true;
                } else {
                    net::shared_vm::close_socket(&mut state.borrow_mut().net, socket);
                }
            }
            net::shared_vm::TransportEvent::Connected { socket } => {
                let request = state
                    .borrow()
                    .http
                    .shared
                    .clients
                    .get(&socket)
                    .map(|client| client.request.clone());
                if let Some(request) = request {
                    net::shared_vm::write(&mut state.borrow_mut().net, socket, &request)
                        .map_err(|error| format!("HTTP client write failed: {error}"))?;
                    progressed = true;
                }
            }
            net::shared_vm::TransportEvent::ConnectError { socket, message } => {
                let callback = state
                    .borrow_mut()
                    .http
                    .shared
                    .clients
                    .remove(&socket)
                    .map(|client| client.callback);
                if let Some(callback) = callback {
                    runtime.release_root(callback);
                }
                return Err(format!("HTTP client connection failed: {message}"));
            }
            net::shared_vm::TransportEvent::Data { socket, bytes } => {
                if state.borrow().http.shared.connections.contains_key(&socket) {
                    let dispatch = {
                        let mut host = state.borrow_mut();
                        let Some(connection) = host.http.shared.connections.get_mut(&socket) else {
                            continue;
                        };
                        if connection.request_dispatched {
                            None
                        } else {
                            connection.received.extend_from_slice(&bytes);
                            if connection.received.len() > REQUEST_HEAD_LIMIT {
                                return Err(
                                    "HTTP request head exceeds the shared parser limit".into()
                                );
                            }
                            parse_head(&connection.received).filter(|(_, _, _, _, body_start)| {
                                let content_length =
                                    content_length(&connection.received[..*body_start]);
                                connection.received.len() >= *body_start + content_length
                            })
                        }
                    };
                    if let Some((method, path, version, headers, body_start)) = dispatch {
                        let (server_root, response_id, response_factory) = {
                            let mut host = state.borrow_mut();
                            let server_id = host.http.shared.connections[&socket].server;
                            let server_root = host
                                .http
                                .shared
                                .servers
                                .get(&server_id)
                                .map(|server| server.root)
                                .ok_or_else(|| "HTTP server root was released early".to_owned())?;
                            let response_id = host
                                .http
                                .shared
                                .response_id()
                                .map_err(|error| error.to_owned())?;
                            let response_factory =
                                host.http.shared.response_factory.ok_or_else(|| {
                                    "HTTP response factory is unavailable".to_owned()
                                })?;
                            host.http
                                .shared
                                .connections
                                .get_mut(&socket)
                                .unwrap()
                                .request_dispatched = true;
                            host.http
                                .shared
                                .connections
                                .get_mut(&socket)
                                .unwrap()
                                .received
                                .drain(..body_start);
                            host.http.shared.responses.insert(
                                response_id,
                                Response {
                                    socket,
                                    headers: Vec::new(),
                                    ended: false,
                                },
                            );
                            (server_root, response_id, response_factory)
                        };
                        let request = request_object(runtime, method, path, version, headers)?;
                        let id = runtime.root(Value::number(response_id as f64));
                        let status = runtime.root(Value::number(200.0));
                        let undefined = runtime.root(Value::UNDEFINED);
                        let response =
                            match runtime.call_rooted(response_factory, undefined, &[id, status]) {
                                Ok(response) => response,
                                Err(error) => {
                                    runtime.release_root(request);
                                    runtime.release_root(id);
                                    runtime.release_root(status);
                                    runtime.release_root(undefined);
                                    return Err(runtime.format_error(program, &error.error));
                                }
                            };
                        let emitted = emit_event(
                            runtime,
                            program,
                            server_root,
                            "request",
                            &[request, response],
                        );
                        runtime.release_root(request);
                        runtime.release_root(response);
                        runtime.release_root(id);
                        runtime.release_root(status);
                        runtime.release_root(undefined);
                        emitted?;
                        progressed = true;
                    }
                } else if state.borrow().http.shared.clients.contains_key(&socket) {
                    let response = {
                        let mut host = state.borrow_mut();
                        let client = host.http.shared.clients.get_mut(&socket).unwrap();
                        client.received.extend_from_slice(&bytes);
                        if client.received.len() > RESPONSE_HEAD_LIMIT {
                            return Err("HTTP response head exceeds the shared parser limit".into());
                        }
                        parse_response(&client.received)
                    };
                    if let Some((status, message, headers, raw_headers)) = response {
                        let client = state
                            .borrow_mut()
                            .http
                            .shared
                            .clients
                            .remove(&socket)
                            .ok_or_else(|| {
                                "HTTP client completion was already consumed".to_owned()
                            })?;
                        let response =
                            match response_object(runtime, status, message, headers, raw_headers) {
                                Ok(response) => response,
                                Err(error) => {
                                    runtime.release_root(client.callback);
                                    return Err(error);
                                }
                            };
                        let undefined = runtime.root(Value::UNDEFINED);
                        let result = runtime.call_rooted(client.callback, undefined, &[response]);
                        runtime.release_root(undefined);
                        runtime.release_root(response);
                        let callback_error = result.err().map(|error| {
                            let message = runtime.format_error(program, &error.error);
                            if let Some(exception) = error.exception {
                                runtime.release_root(exception);
                            }
                            message
                        });
                        runtime.release_root(client.callback);
                        if let Some(error) = callback_error {
                            return Err(error);
                        }
                        let agent_owns_socket = state
                            .borrow()
                            .http
                            .shared
                            .agents
                            .values()
                            .any(|sockets| sockets.contains(&socket));
                        if !agent_owns_socket {
                            net::shared_vm::close_socket(&mut state.borrow_mut().net, socket);
                        }
                        progressed = true;
                    }
                }
            }
            net::shared_vm::TransportEvent::End { socket } => {
                let server_id = state
                    .borrow_mut()
                    .http
                    .shared
                    .connections
                    .remove(&socket)
                    .map(|connection| connection.server);
                if let Some(server_id) = server_id {
                    let mut host = state.borrow_mut();
                    if let Some(server) = host.http.shared.servers.get_mut(&server_id) {
                        server.connections.remove(&socket);
                    }
                    host.http
                        .shared
                        .responses
                        .retain(|_, response| response.socket != socket);
                    progressed = true;
                }
            }
            net::shared_vm::TransportEvent::Error { socket, message } => {
                state.borrow_mut().http.shared.connections.remove(&socket);
                if let Some(client) = state.borrow_mut().http.shared.clients.remove(&socket) {
                    runtime.release_root(client.callback);
                }
                return Err(format!("shared HTTP socket failed: {message}"));
            }
        }
    }
    progressed |= finish_closed_servers(runtime, program, state)?;
    Ok(progressed)
}

pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>, state: &Rc<RefCell<HostState>>) {
    let roots = {
        let mut host = state.borrow_mut();
        let shared = std::mem::take(&mut host.http.shared);
        let mut roots = Vec::new();
        roots.extend(shared.servers.into_values().map(|server| server.root));
        roots.extend(shared.clients.into_values().map(|client| client.callback));
        roots.extend(shared.response_factory);
        net::shared_vm::cleanup(&mut host.net);
        roots
    };
    for root in roots {
        runtime.release_root(root);
    }
}

fn emit_listening(
    runtime: &mut Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<HostState>>,
) -> Result<bool, String> {
    let pending = {
        let mut host = state.borrow_mut();
        let ids = host
            .http
            .shared
            .servers
            .iter()
            .filter_map(|(id, server)| server.listening_pending.then_some(*id))
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| {
                let server = host.http.shared.servers.get_mut(&id)?;
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

fn finish_closed_servers(
    runtime: &mut Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<HostState>>,
) -> Result<bool, String> {
    let closed = {
        let mut host = state.borrow_mut();
        let ids = host
            .http
            .shared
            .servers
            .iter()
            .filter_map(|(id, server)| {
                (server.closing && server.connections.is_empty()).then_some(*id)
            })
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| {
                host.http
                    .shared
                    .servers
                    .remove(&id)
                    .map(|server| server.root)
            })
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
    program: &rqj::ResidualProgram,
    receiver: RootId,
    name: &str,
    args: &[RootId],
) -> Result<(), String> {
    let event = runtime.string_rooted(name);
    let result = emit(runtime, program, receiver, event, args);
    runtime.release_root(event);
    result
}

fn emit(
    runtime: &mut Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
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
    let headers_object = runtime.object_rooted().map_err(|error| error.to_string())?;
    for (name, value) in headers {
        set_text(runtime, headers_object, &name, &value)?;
    }
    set_named(runtime, request, "headers", headers_object)?;
    set_bool(runtime, request, "complete", true)?;
    set_bool(runtime, request, "readable", true)?;
    Ok(request)
}

fn response_object(
    runtime: &mut Runtime<NodeHost>,
    status: u16,
    message: String,
    headers: Vec<(String, String)>,
    raw_headers: Vec<(String, String)>,
) -> Result<RootId, String> {
    let response = runtime.object_rooted().map_err(|error| error.to_string())?;
    set_number(runtime, response, "statusCode", status as f64)?;
    set_text(runtime, response, "statusMessage", &message)?;
    set_text(runtime, response, "httpVersion", "1.1")?;
    let headers_object = runtime.object_rooted().map_err(|error| error.to_string())?;
    for (name, value) in headers {
        set_text(runtime, headers_object, &name, &value)?;
    }
    set_named(runtime, response, "headers", headers_object)?;
    let mut raw = Vec::with_capacity(raw_headers.len() * 2);
    for (name, value) in raw_headers {
        raw.push(runtime.string_rooted(&name));
        raw.push(runtime.string_rooted(&value));
    }
    let global = runtime.global_root().map_err(|error| error.to_string())?;
    let array_key = runtime.string_rooted("Array");
    let array = runtime
        .get_property_rooted(global, array_key)
        .map_err(|error| error.to_string())?;
    let raw_headers = runtime
        .construct_rooted(array, array, &raw)
        .map_err(|error| error.to_string())?;
    for value in raw {
        runtime.release_root(value);
    }
    runtime.release_root(array);
    runtime.release_root(array_key);
    runtime.release_root(global);
    set_named(runtime, response, "rawHeaders", raw_headers)?;
    set_bool(runtime, response, "complete", true)?;
    Ok(response)
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

fn parse_head(bytes: &[u8]) -> Option<(String, String, String, Vec<(String, String)>, usize)> {
    let boundary = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n")?;
    let body_start = boundary + 4;
    let text = String::from_utf8_lossy(&bytes[..boundary]);
    let mut lines = text.split("\r\n");
    let mut request = lines.next()?.split_whitespace();
    let method = request.next()?.to_owned();
    let path = request.next()?.to_owned();
    let version = request.next()?.strip_prefix("HTTP/")?.to_owned();
    let headers = parse_headers(lines);
    Some((method, path, version, headers, body_start))
}

fn parse_response(
    bytes: &[u8],
) -> Option<(u16, String, Vec<(String, String)>, Vec<(String, String)>)> {
    let boundary = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n")?;
    let text = String::from_utf8_lossy(&bytes[..boundary]);
    let mut lines = text.split("\r\n");
    let mut status_line = lines.next()?.split_whitespace();
    let _version = status_line.next()?;
    let status = status_line.next()?.parse().ok()?;
    let message = status_line.collect::<Vec<_>>().join(" ");
    let lines = lines.collect::<Vec<_>>();
    let raw_headers = lines
        .iter()
        .filter_map(|line| {
            let colon = line.find(':')?;
            Some((
                line[..colon].trim().to_owned(),
                line[colon + 1..].trim().to_owned(),
            ))
        })
        .collect::<Vec<_>>();
    let mut headers = Vec::<(String, String)>::new();
    for (name, value) in &raw_headers {
        let name = name.to_ascii_lowercase();
        if let Some((_, current)) = headers.iter_mut().find(|(key, _)| *key == name) {
            current.push_str(", ");
            current.push_str(value);
        } else {
            headers.push((name, value.clone()));
        }
    }
    Some((status, message, headers, raw_headers))
}

fn parse_headers<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut headers = Vec::<(String, String)>::new();
    for line in lines {
        let Some(colon) = line.find(':') else {
            continue;
        };
        let name = line[..colon].trim().to_ascii_lowercase();
        let value = line[colon + 1..].trim();
        if let Some((_, current)) = headers.iter_mut().find(|(key, _)| *key == name) {
            current.push_str(if name == "cookie" { "; " } else { ", " });
            current.push_str(value);
        } else {
            headers.push((name, value.to_owned()));
        }
    }
    headers
}

fn content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head);
    text.split("\r\n")
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
        .next()
        .unwrap_or(0)
}
