//! `http_client` — minimal `http.request` / `http.get`. Builds a
//! ClientRequest emitter; `end()` connects via `net`, writes the
//! request head + body, parses the response, and emits `'response'`
//! with an IncomingMessage that streams `'data'`/`'end'`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::net;

/// Hidden property mapping a ClientRequest object to its state.
const CLIENT_ID_PROP: &str = "\0quench:http:req:id";
const CLIENT_CLOSE_PENDING_PROP: &str = "\0quench:http:req:close-pending";
const RESPONSE_ENCODING_PROP: &str = "\0quench:http:res:encoding";

/// `(host, port, method, path, headers)` for one outbound request.
#[derive(Clone)]
pub(crate) enum RequestTarget {
    Tcp { host: String, port: u16 },
    Unix { path: String },
}

struct RequestOptions {
    target: RequestTarget,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

/// One outbound HTTP request, keyed by `CLIENT_ID_PROP`.
pub struct ClientReq {
    pub target: RequestTarget,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub req: Value,
    pub agent: Option<Value>,
    pub omit_host: bool,
    pub socket: Option<Value>,
    pub buffer: Vec<u8>,
    pub res: Option<Value>,
    pub head_parsed: bool,
    pub aborted: bool,
    pub response_ended: bool,
    pub response_closed: bool,
    /// Raw body bytes received after the response head.
    pub response_received: usize,
    pub response_chunked_done: bool,
}

pub fn agent_call(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(agent) = receiver else {
        return Ok(Value::Undefined);
    };
    for key in ["sockets", "freeSockets"] {
        let pools = execute::get_property(agent, key);
        for name in execute::own_enumerable_keys(&pools) {
            let sockets = execute::get_property(&pools, &name);
            for index in execute::own_enumerable_keys(&sockets) {
                if let Some(socket) = match execute::get_property(&sockets, &index) {
                    Value::Object(_) | Value::ObjectAlias(_) => {
                        Some(execute::get_property(&sockets, &index))
                    }
                    _ => None,
                } {
                    crate::modules::net::socket_destroy(_state, Some(&socket), &[])?;
                }
            }
            let (updated, _) = execute::delete_property(pools.clone(), &name);
            execute::replace_value(&pools, &updated);
        }
    }
    Ok(agent.clone())
}

pub fn agent_construct(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let mut object = crate::modules::events::new_emitter_object(state)?;
    object = execute::set_property(object, "sockets", host_api::object(Vec::new()));
    object = execute::set_property(object, "freeSockets", host_api::object(Vec::new()));
    object = execute::set_property(object, "requests", host_api::object(Vec::new()));
    object = execute::set_property(object, "options", host_api::object(Vec::new()));
    object = install_methods(
        object,
        vec![
            (
                "destroy".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_AGENT),
            ),
            (
                "getName".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_AGENT_GET_NAME),
            ),
        ],
    )?;
    Ok(object)
}

/// `agent.getName(options)` — stable pool key derived from connection facts.
pub fn agent_get_name(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    let host = match execute::get_property(&options, "host") {
        Value::String(value) if !value.is_empty() => value,
        _ => match execute::get_property(&options, "hostname") {
            Value::String(value) if !value.is_empty() => value,
            _ => "localhost".into(),
        },
    };
    let port = match execute::get_property(&options, "port") {
        Value::String(value) => value,
        Value::Number(value) if value.is_finite() => (value as i64).to_string(),
        _ => String::new(),
    };
    let local = match execute::get_property(&options, "localAddress") {
        Value::String(value) => value,
        _ => String::new(),
    };
    let family = match execute::get_property(&options, "family") {
        Value::Number(value) if value == 4.0 || value == 6.0 => format!(":{value}"),
        _ => String::new(),
    };
    let socket_path = match execute::get_property(&options, "socketPath") {
        Value::String(value) if !value.is_empty() => format!(":{value}"),
        _ => String::new(),
    };
    Ok(Value::String(format!("{host}:{port}:{local}{family}{socket_path}")))
}

pub fn res_set_encoding(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    if let Some(receiver) = receiver {
        let updated = execute::set_property(receiver.clone(), RESPONSE_ENCODING_PROP, value);
        execute::replace_value(receiver, &updated);
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn response_data(response: &Value, bytes: &[u8]) -> Value {
    match execute::get_property_result(response, RESPONSE_ENCODING_PROP).ok() {
        Some(Value::String(encoding))
            if encoding.eq_ignore_ascii_case("utf8") || encoding.eq_ignore_ascii_case("utf-8") =>
        {
            Value::String(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => crate::modules::buffer_proto::make_buffer(bytes),
    }
}

/// `http.request(options[, cb])` — an outbound ClientRequest.
pub fn request(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let opts = request_options(args.first())?;
    let signal = args.first().and_then(|options| {
        matches!(options, Value::Object(_) | Value::ObjectAlias(_)).then(|| {
            execute::get_property(options, "signal")
        })
    }).filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    let agent = args.first().and_then(|options| {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return None;
        }
        let configured = execute::get_property(options, "agent");
        if !matches!(configured, Value::Undefined | Value::Null) {
            return Some(configured);
        }
        let connector = execute::get_property(options, "createConnection");
        quench_runtime::is_callable(&connector).then(|| options.clone())
    });
    let omit_host = args.first().is_some_and(|options| {
        matches!(
            execute::get_property(options, "headers"),
            Value::Array(_)
        )
    });
    let (req, id) = build_req_object(state)?;
    set_request_property(Some(&req), "path", Value::String(opts.path.clone()));
    set_request_property(Some(&req), "method", Value::String(opts.method.clone()));
    let mut guard = state.borrow_mut();
    guard.http.clientreqs.insert(
        id,
        ClientReq {
            target: opts.target,
            method: opts.method,
            path: opts.path,
            headers: opts.headers,
            body: Vec::new(),
            req: req.clone(),
            agent,
            omit_host,
            socket: None,
            buffer: Vec::new(),
            res: None,
            head_parsed: false,
            aborted: false,
            response_ended: false,
            response_closed: false,
            response_received: 0,
            response_chunked_done: false,
        },
    );
    drop(guard);
    if let Some(signal) = signal {
        if let Some(target) = crate::modules::event_target::target_identity(&signal) {
            state.borrow_mut().http.client_signals.insert(target, id);
            let listener = crate::host::capability(crate::registry::SPEC_HTTP_REQ_SIGNAL_ABORT);
            let options = host_api::object(vec![("once".into(), Value::Boolean(true))]);
            if matches!(execute::get_property(&signal, "aborted"), Value::Boolean(true)) {
                state.borrow_mut().http.client_signals.remove(&target);
                preabort_request(state, &req);
            } else {
                crate::modules::event_target::add_event_listener(
                    state,
                    Some(&signal),
                    &[Value::String("abort".into()), listener, options],
                )?;
            }
        }
    }
    if let Some(cb) = args.get(1) {
        if quench_runtime::is_callable(cb) {
            crate::modules::events::method_on(
                state,
                Some(&req),
                &[Value::String("response".to_string()), cb.clone()],
            )?;
        }
    }
    Ok(req)
}

fn preabort_request(state: &Rc<RefCell<HostState>>, request: &Value) {
    set_request_property(Some(request), "destroyed", Value::Boolean(true));
    let error = abort_error();
    state.borrow_mut().net.pending_events.extend([
        (request.clone(), "error".into(), vec![error]),
        (request.clone(), "close".into(), Vec::new()),
    ]);
}

fn abort_error() -> Value {
    let error = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("The operation was aborted".into())],
        ),
        "name",
        Value::String("AbortError".into()),
    );
    execute::set_property(error, "code", Value::String("ABORT_ERR".into()))
}

/// AbortSignal transition for one pending ClientRequest. The signal-to-request
/// map is the sole association fact; the ordinary destroy transition owns all
/// request state and event ordering.
pub fn req_signal_abort(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(signal) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(target) = crate::modules::event_target::target_identity(signal) else {
        return Ok(Value::Undefined);
    };
    let Some(id) = state.borrow_mut().http.client_signals.remove(&target) else {
        return Ok(Value::Undefined);
    };
    let request = state.borrow().http.clientreqs.get(&id).map(|req| req.req.clone());
    if let Some(request) = request {
        let error = abort_error();
        req_destroy(state, Some(&request), &[error])?;
    }
    Ok(Value::Undefined)
}

/// `req.abort()` — transition the request to its terminal aborted state and
/// notify listeners once. The socket is closed through the normal net path so
/// its close event remains the source of the request's final close event.
pub fn req_abort(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = client_id(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let (request, socket, target) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        if req.aborted || matches!(execute::get_property(&req.req, "destroyed"), Value::Boolean(true)) {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        }
        req.aborted = true;
        (req.req.clone(), req.socket.clone(), req.target.clone())
    };
    set_request_property(receiver, "aborted", Value::Boolean(true));
    set_request_property(receiver, "destroyed", Value::Boolean(true));
    net::emit(state, &request, "abort", Vec::new())?;
    if let Some(socket) = socket {
        net::socket_destroy(state, Some(&socket), &[])?;
    }
    if let RequestTarget::Unix { path } = target {
        let active = state
            .borrow()
            .http
            .clientreqs
            .values()
            .filter(|req| !req.aborted)
            .count();
        if active == 0 {
            let server = state
                .borrow()
                .net
                .servers
                .values()
                .find(|server| server.borrow().path.as_deref() == Some(path.as_str()))
                .map(|server| server.borrow().js.clone());
            if let Some(server) = server {
                crate::modules::net::server_close(state, Some(&server), &[])?;
            }
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `req.destroy([error])` — terminally destroy a client request. An error is
/// observable only while no response has been delivered; once a response is
/// flowing, destroying the request closes that response without a synthetic
/// request error.
pub fn req_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = client_id(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let (request, socket, response, already_destroyed) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        let already_destroyed = matches!(
            execute::get_property(&req.req, "destroyed"),
            Value::Boolean(true)
        );
        (
            req.req.clone(),
            req.socket.clone(),
            req.res.clone(),
            already_destroyed,
        )
    };
    if already_destroyed {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    state
        .borrow_mut()
        .http
        .client_signals
        .retain(|_, request_id| *request_id != id);
    set_request_property(receiver, "destroyed", Value::Boolean(true));
    if let Some(error) = args.first().cloned() {
        state
            .borrow_mut()
            .net
            .pending_events
            .push((request.clone(), "error".into(), vec![error]));
    } else if response.is_none() {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("socket hang up".into())],
        );
        let error = execute::set_property(error, "code", Value::String("ECONNRESET".into()));
        let error_ctor = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "Error",
        );
        let error = execute::set_property(error, "constructor", error_ctor);
        state
            .borrow_mut()
            .net
            .pending_events
            .push((request.clone(), "error".into(), vec![error]));
    }
    if response.is_none() {
        set_request_property(receiver, CLIENT_CLOSE_PENDING_PROP, Value::Boolean(true));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((request.clone(), "close".into(), Vec::new()));
    }
    if let Some(response) = response {
        let signal = execute::get_property(&response, "signal");
        if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
            crate::modules::http::abort_http_signal(state, &signal)?;
        }
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
            req.response_closed = true;
        }
        net::emit(state, &response, "close", Vec::new())?;
    }
    if let Some(socket) = socket {
        net::socket_destroy(state, Some(&socket), &[])?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `http.get(options[, cb])` — `request` with method GET, auto-ended.
pub fn get(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let req = request(state, args)?;
    req_end(state, Some(&req), &[])
}

/// `req.write(chunk)` — buffer a request body fragment.
pub fn req_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = client_id(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let bytes = chunk_bytes(args.first());
    let mut guard = state.borrow_mut();
    if let Some(req) = guard.http.clientreqs.get_mut(&id) {
        req.body.extend_from_slice(&bytes);
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn req_set_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = client_id(receiver) else {
        return Ok(Value::Undefined);
    };
    let name = args
        .first()
        .map(execute::to_js_string)
        .transpose()?
        .unwrap_or_default();
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);
    let values = if matches!(value, Value::Array(_)) {
        let items: Vec<String> = execute::own_enumerable_keys(&value)
            .into_iter()
            .filter_map(|key| execute::get_property_result(&value, &key).ok())
            .filter_map(|v| execute::to_js_string(&v).ok())
            .collect();
        if name.eq_ignore_ascii_case("cookie") {
            vec![items.join("; ")]
        } else {
            items
        }
    } else {
        vec![execute::to_js_string(&value)?]
    };
    let mut guard = state.borrow_mut();
    if let Some(req) = guard.http.clientreqs.get_mut(&id) {
        req.headers
            .retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
        req.headers
            .extend(values.into_iter().map(|value| (name.clone(), value)));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `req.end([chunk])` — write the body, connect, and send the request.
pub fn req_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = client_id(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    if let Some(data) = args.first() {
        if !matches!(data, Value::Undefined) {
            req_write(state, receiver, std::slice::from_ref(data))?;
        }
    }
    let (target, method, path, headers, body, agent, omit_host) = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        if req.aborted {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        }
        if matches!(execute::get_property(&req.req, "finished"), Value::Boolean(true)) {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        }
        (
            req.target.clone(),
            req.method.clone(),
            req.path.clone(),
            req.headers.clone(),
            req.body.clone(),
            req.agent.clone(),
            req.omit_host,
        )
    };
    if let Some(req_value) = receiver {
        set_request_property(Some(req_value), "finished", Value::Boolean(true));
    }
    let host = target_host(&target);
    let head_host = request_host(&target);
    let head = request_head(&head_host, &method, &path, &headers, body.len(), omit_host);
    if let Some(req) = state.borrow().http.clientreqs.get(&id) {
        let updated =
            execute::set_property(req.req.clone(), "_header", Value::String(head.clone()));
        execute::replace_value(&req.req, &updated);
    }
    let custom = agent.as_ref().and_then(custom_connection).filter(|_| {
        let Some(agent) = agent.as_ref() else { return false; };
        let key = target_key(&target);
        let mut guard = state.borrow_mut();
        if guard.http.agent_connections.iter().any(|(known, known_key)| {
            known_key == &key && execute::same_identity(known, agent)
        }) {
            false
        } else {
            guard.http.agent_connections.push((agent.clone(), key));
            true
        }
    });
    let socket = match custom {
        Some(connection) => {
            let mut option_props = vec![
                ("host".into(), Value::String(host.clone())),
                ("hostname".into(), Value::String(host.clone())),
                ("path".into(), Value::String(path.clone())),
            ];
            if let RequestTarget::Tcp { port, .. } = &target {
                option_props.push(("port".into(), Value::Number(*port as f64)));
            }
            if let RequestTarget::Unix { path } = &target {
                option_props.push(("socketPath".into(), Value::String(path.clone())));
                option_props.push(("port".into(), Value::String(path.clone())));
            }
            let options = host_api::object(option_props);
            let callback = host_api::object(Vec::new());
            let socket = execute::call(&connection, agent.as_ref().unwrap(), &[options, callback])?;
            let mut bytes = request_head(&request_host(&target), &method, &path, &headers, body.len(), omit_host).into_bytes();
            bytes.extend_from_slice(&body);
            if matches!(target, RequestTarget::Unix { .. }) {
                state.borrow_mut().net.pending_writes.push((socket.clone(), bytes));
            } else {
                let payload = host_api::bytes(&bytes);
                let write = execute::get_property(&socket, "write");
                if quench_runtime::is_callable(&write) {
                    execute::call(&write, &socket, &[payload])?;
                }
            }
            let resume = execute::get_property(&socket, "resume");
            if quench_runtime::is_callable(&resume) {
                execute::call(&resume, &socket, &[])?;
            }
            socket
        }
        None => send_request(state, &target, &method, &path, &headers, &body, omit_host)?,
    };
    let socket_id = net::net_id(&socket);
    let mut guard = state.borrow_mut();
    if let Some(req) = guard.http.clientreqs.get_mut(&id) {
        req.socket = Some(socket.clone());
        set_request_property(Some(&req.req), "socket", socket.clone());
        if let Some(agent) = req.agent.clone() {
            let name = agent_name(&target, &agent);
            add_agent_socket(&agent, &name, &socket);
        }
    }
    if let Some(socket_id) = socket_id {
        guard.http.clients.insert(socket_id, id);
    }
    drop(guard);
    subscribe_socket(state, &socket)?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn send_request(
    state: &Rc<RefCell<HostState>>,
    target: &RequestTarget,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
    omit_host: bool,
) -> Result<Value, VmError> {
    let host = target_host(target);
    let socket = match target {
        RequestTarget::Tcp { host, port } => net::connect(
            state,
            &[Value::Number(*port as f64), Value::String(host.clone())],
        )?,
        RequestTarget::Unix { path } => net::connect_path(state, path)?,
    };
    let head = request_head(&request_host(target), method, path, headers, body.len(), omit_host);
    let mut payload = head.into_bytes();
    payload.extend_from_slice(body);
    if matches!(target, RequestTarget::Unix { .. }) {
        state.borrow_mut().net.pending_writes.push((socket.clone(), payload));
    } else {
        net::socket_write(state, Some(&socket), &[host_api::bytes(&payload)])?;
    }
    Ok(socket)
}

fn target_host(target: &RequestTarget) -> String {
    match target {
        RequestTarget::Tcp { host, .. } => host.clone(),
        RequestTarget::Unix { .. } => "localhost".into(),
    }
}

fn request_host(target: &RequestTarget) -> String {
    match target {
        RequestTarget::Tcp { host, port } if *port != 80 => format!("{host}:{port}"),
        RequestTarget::Tcp { host, .. } => host.clone(),
        RequestTarget::Unix { .. } => "localhost".into(),
    }
}

fn target_key(target: &RequestTarget) -> String {
    match target {
        RequestTarget::Tcp { host, port } => format!("tcp:{host}:{port}"),
        RequestTarget::Unix { path } => format!("unix:{path}"),
    }
}

fn request_head(
    host: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body_len: usize,
    omit_host: bool,
) -> String {
    let mut head = format!("{method} {path} HTTP/1.1\r\n");
    if !omit_host {
        head.push_str(&format!("Host: {host}\r\n"));
    }
    for (key, value) in headers {
        head.push_str(&format!("{key}: {value}\r\n"));
    }
    // Node's implicit zero-length framing is method-dependent: bodyless
    // methods omit `content-length`, while methods that conventionally carry
    // an entity advertise an empty body as `content-length: 0`.
    let has_content_length = headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("content-length"));
    if !has_content_length && (body_len > 0 || default_empty_body(method)) {
        head.push_str(&format!("Content-Length: {body_len}\r\n"));
    }
    if !headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("connection"))
    {
        head.push_str("Connection: keep-alive\r\n");
    }
    head.push_str("\r\n");
    head
}

fn default_empty_body(method: &str) -> bool {
    matches!(method.to_ascii_uppercase().as_str(), "POST" | "PUT")
}

fn subscribe_socket(state: &Rc<RefCell<HostState>>, socket: &Value) -> Result<(), VmError> {
    let data_cap = crate::host::capability(crate::registry::SPEC_HTTP_RESDATA);
    subscribe_event(state, socket, "data", data_cap)?;
    let end_cap = crate::host::capability(crate::registry::SPEC_HTTP_RESEND);
    subscribe_event(state, socket, "end", end_cap)?;
    let close_cap = crate::host::capability(crate::registry::SPEC_HTTP_REQCLOSE);
    subscribe_event(state, socket, "close", close_cap)?;
    let error_cap = crate::host::capability(crate::registry::SPEC_HTTP_REQ_ERROR);
    subscribe_event(state, socket, "error", error_cap)?;
    Ok(())
}

pub fn req_error(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(client_id) = client_id_for_socket(state, socket) else {
        return Ok(Value::Undefined);
    };
    let request = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| req.req.clone());
    if let (Some(request), Some(error)) = (request, args.first().cloned()) {
        net::emit(state, &request, "error", vec![error])?;
    }
    Ok(Value::Undefined)
}

fn subscribe_event(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
    event: &str,
    listener: Value,
) -> Result<(), VmError> {
    if net::net_id(socket).is_some() {
        crate::modules::events::method_on(
            state,
            Some(socket),
            &[Value::String(event.to_string()), listener],
        )
        .map(|_| ())
    } else {
        let on = execute::get_property(socket, "on");
        if quench_runtime::is_callable(&on) {
            execute::call(
                &on,
                socket,
                &[Value::String(event.to_string()), listener],
            )?;
        }
        Ok(())
    }
}

/// Socket close completes the corresponding ClientRequest lifecycle.
pub fn req_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket) = receiver else {
        return Ok(Value::Undefined);
    };
    crate::modules::http::abort_server_signal(state, socket)?;
    let Some(client_id) = client_id_for_socket(state, socket) else {
        let request = state.borrow().http.conns.values().find_map(|conn| {
            conn.req.clone().filter(|req| {
                !matches!(execute::get_property(req, crate::modules::http::REQ_CLOSE_PROP), Value::Boolean(true))
            })
        });
        if let Some(request) = request {
            execute::set_property_in_place(&request, crate::modules::http::REQ_CLOSE_PROP, Value::Boolean(true));
            net::emit(state, &request, "close", Vec::new())?;
        }
        return Ok(Value::Undefined);
    };
    let request = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| req.req.clone());
    let response = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .and_then(|req| req.res.clone());
    let agent = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .and_then(|req| req.agent.clone());
    let target = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| req.target.clone());
    state
        .borrow_mut()
        .http
        .client_signals
        .retain(|_, request_id| *request_id != client_id);
    if let Some(request) = request {
        if matches!(
            execute::get_property(&request, CLIENT_CLOSE_PENDING_PROP),
            Value::Boolean(true)
        ) {
            return Ok(Value::Undefined);
        }
        net::emit(state, &request, "close", Vec::new())?;
    }
    if let (Some(agent), Some(target)) = (agent.as_ref(), target.as_ref()) {
        let name = agent_name(target, agent);
        remove_agent_socket(agent, &name, socket);
        let has_active = state.borrow().http.clientreqs.values().any(|req| {
            req.agent
                .as_ref()
                .is_some_and(|candidate| execute::same_identity(candidate, agent))
                && agent_name(&req.target, agent) == name
                && !req.response_closed
        });
        if !has_active {
            let pools = execute::get_property(agent, "sockets");
            let (updated, _) = execute::delete_property(pools.clone(), &name);
            execute::replace_value(&pools, &updated);
        }
    }
    // A peer closing before the response completed is an aborted response,
    // not a normal end. Keep the response identity and signal as the single
    // state transition, then report ECONNRESET only when an error listener is
    // present (Node does not crash for an unobserved response error).
    if let Some(response) = response {
        let complete = matches!(
            execute::get_property(&response, "complete"),
            Value::Boolean(true)
        );
        if !complete
            && !matches!(
                execute::get_property(&response, "aborted"),
                Value::Boolean(true)
            )
        {
            execute::set_property_in_place(&response, "aborted", Value::Boolean(true));
            let signal = execute::get_property(&response, "signal");
            if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
                crate::modules::http::abort_http_signal(state, &signal)?;
            }
            net::emit(state, &response, "aborted", Vec::new())?;
            let has_error_listener = crate::modules::emitter::emitter_id(&response)
                .and_then(|id| state.borrow().emitters.get(id))
                .is_some_and(|emitter| !emitter.borrow().listeners_of("error").is_empty());
            if has_error_listener {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String("socket hang up".into())],
                );
                let error = execute::set_property(error, "code", Value::String("ECONNRESET".into()));
                net::emit(state, &response, "error", vec![error])?;
            }
            net::emit(state, &response, "close", Vec::new())?;
        }
    }
    Ok(Value::Undefined)
}

fn agent_name(target: &RequestTarget, agent: &Value) -> String {
    let options = match target {
        RequestTarget::Tcp { host, port } => host_api::object(vec![
            (
                "host".into(),
                Value::String(if host == "127.0.0.1" {
                    "localhost".into()
                } else {
                    host.clone()
                }),
            ),
            ("port".into(), Value::Number(*port as f64)),
        ]),
        RequestTarget::Unix { path } => host_api::object(vec![(
            "socketPath".into(),
            Value::String(path.clone()),
        )]),
    };
    execute::to_js_string(&execute::call(
        &execute::get_property(agent, "getName"),
        agent,
        &[options],
    ).unwrap_or(Value::String(String::new()))).unwrap_or_default()
}

fn add_agent_socket(agent: &Value, name: &str, socket: &Value) {
    let pools = execute::get_property(agent, "sockets");
    let list = match execute::get_property(&pools, name) {
        Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_) => {
            execute::get_property(&pools, name)
        }
        _ => {
            let list = host_api::array(Vec::new());
            execute::set_property_in_place(&pools, name, list.clone());
            list
        }
    };
    let length = match execute::get_property(&list, "length") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    execute::set_property_in_place(&list, &length.to_string(), socket.clone());
    execute::set_property_in_place(&list, "length", Value::Number((length + 1) as f64));
}

fn remove_agent_socket(agent: &Value, name: &str, socket: &Value) {
    let pools = execute::get_property(agent, "sockets");
    let list = execute::get_property(&pools, name);
    let keys = execute::own_enumerable_keys(&list);
    for key in keys {
        let value = execute::get_property(&list, &key);
        if execute::same_identity(&value, socket) {
            let _ = execute::set_property_in_place(&list, &key, Value::Undefined);
        }
    }
}

/// Response parser: buffer bytes, then stream `'data'` chunks.
pub fn data_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(client_id) = client_id_for_socket(state, socket) else {
        return Ok(Value::Undefined);
    };
    let bytes = chunk_bytes(args.first());
    let (pending_head, head_parsed) = append_and_probe(state, client_id, &bytes);
    match pending_head {
        Some(head) => {
            let res = build_incoming(state, &head)?;
            if let Some(socket) = state
                .borrow()
                .http
                .clientreqs
                .get(&client_id)
                .and_then(|req| req.socket.clone())
            {
                set_response_property(&res, "socket", socket);
            }
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.res = Some(res.clone());
            }
            let req_value = client_value(state, client_id, true).unwrap_or(Value::Undefined);
            net::emit(state, &req_value, "response", vec![res])?;
            flush_body(state, client_id)
        }
        None if head_parsed => {
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.response_received = req.response_received.saturating_add(bytes.len());
                if bytes.windows(5).any(|window| window == b"0\r\n\r\n") {
                    req.response_chunked_done = true;
                }
            }
            if let Some(res) = client_value(state, client_id, false) {
                let body = response_body_bytes(&res, &bytes);
                if !body.is_empty() {
                    net::emit(state, &res, "data", vec![response_data(&res, &body)])?;
                }
            }
            Ok(())
        }
        None => Ok(()),
    }
    .map(|_| Value::Undefined)
}

/// Append bytes and, if a full head just became available, return it.
fn append_and_probe(
    state: &Rc<RefCell<HostState>>,
    client_id: u64,
    bytes: &[u8],
) -> (Option<Vec<u8>>, bool) {
    let mut guard = state.borrow_mut();
    let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
        return (None, false);
    };
    req.buffer.extend_from_slice(bytes);
    if req.head_parsed {
        return (None, true);
    }
    match head_end(&req.buffer) {
        Some((idx, len)) => {
            let head = req.buffer[..idx].to_vec();
            let rest: Vec<u8> = req.buffer[idx + len..].to_vec();
            req.buffer = rest;
            req.head_parsed = true;
            (Some(head), true)
        }
        None => (None, false),
    }
}

/// Clone the ClientRequest (`req=true`) or the response (`req=false`).
fn client_value(state: &Rc<RefCell<HostState>>, client_id: u64, req: bool) -> Option<Value> {
    state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|r| {
            if req {
                r.req.clone()
            } else {
                r.res.clone().unwrap_or(Value::Undefined)
            }
        })
        .filter(|v| !matches!(v, Value::Undefined))
}

/// Emit leftover buffered bytes as `'data'` once `'response'` fired.
fn flush_body(state: &Rc<RefCell<HostState>>, client_id: u64) -> Result<(), VmError> {
    let (res, rest) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
            return Ok(());
        };
        req.response_received = req.response_received.saturating_add(req.buffer.len());
        if req.buffer.windows(5).any(|window| window == b"0\r\n\r\n") {
            req.response_chunked_done = true;
        }
        (req.res.clone(), std::mem::take(&mut req.buffer))
    };
    if let Some(res) = res {
        if !rest.is_empty() {
            let body = response_body_bytes(&res, &rest);
            if !body.is_empty() {
                net::emit(state, &res, "data", vec![response_data(&res, &body)])?;
            }
        }
    }
    Ok(())
}

fn response_body_bytes(response: &Value, bytes: &[u8]) -> Vec<u8> {
    let chunked = execute::get_property_result(response, "headers")
        .ok()
        .and_then(|headers| execute::get_property_result(&headers, "transfer-encoding").ok())
        .and_then(|value| execute::to_js_string(&value).ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
    if chunked {
        decode_chunked(bytes)
    } else {
        bytes.to_vec()
    }
}

fn decode_chunked(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(line_end) = bytes[cursor..].windows(2).position(|pair| pair == b"\r\n") else {
            break;
        };
        let line_end = cursor + line_end;
        let Ok(size) = usize::from_str_radix(
            String::from_utf8_lossy(&bytes[cursor..line_end])
                .split(';')
                .next()
                .unwrap_or(""),
            16,
        ) else {
            break;
        };
        cursor = line_end + 2;
        if cursor + size > bytes.len() {
            break;
        }
        if size == 0 {
            break;
        }
        output.extend_from_slice(&bytes[cursor..cursor + size]);
        cursor += size;
        if bytes.get(cursor..cursor + 2) != Some(b"\r\n") {
            break;
        }
        cursor += 2;
    }
    output
}

/// Socket `'end'` → the response's `'end'`.
pub fn res_end_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(client_id) = client_id_for_socket(state, socket) else {
        return Ok(Value::Undefined);
    };
    let (res, received, chunked_done) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
            return Ok(Value::Undefined);
        };
        if req.response_ended {
            return Ok(Value::Undefined);
        }
        req.response_ended = true;
        (req.res.clone(), req.response_received, req.response_chunked_done)
    };
    if let Some(res) = res {
        if matches!(execute::get_property(&res, "complete"), Value::Boolean(true)) {
            return Ok(Value::Undefined);
        }
        let expected = match execute::get_property(&res, "headers") {
            headers @ (Value::Object(_) | Value::ObjectAlias(_)) => {
                execute::to_js_string(&execute::get_property(&headers, "content-length")).ok()
                    .and_then(|value| value.parse::<usize>().ok())
            }
            _ => None,
        };
        let chunked = match execute::get_property(&res, "headers") {
            headers @ (Value::Object(_) | Value::ObjectAlias(_)) => execute::to_js_string(
                &execute::get_property(&headers, "transfer-encoding"),
            )
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("chunked")),
            _ => false,
        };
        if expected.is_some_and(|expected| expected != received) || (chunked && !chunked_done) {
            return abort_incomplete_response(state, client_id, &res);
        }
        set_response_property(&res, "complete", Value::Boolean(true));
        set_response_property(&res, "readable", Value::Boolean(false));
        net::emit(state, &res, "end", Vec::new())?;
        let should_close = {
            let mut guard = state.borrow_mut();
            let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
                return Ok(Value::Undefined);
            };
            if req.response_closed {
                false
            } else {
                req.response_closed = true;
                true
            }
        };
        if should_close {
            let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
            set_request_property(Some(&request), "destroyed", Value::Boolean(true));
            net::emit(state, &res, "close", Vec::new())?;
        }
    }
    Ok(Value::Undefined)
}

fn abort_incomplete_response(
    state: &Rc<RefCell<HostState>>,
    client_id: u64,
    response: &Value,
) -> Result<Value, VmError> {
    set_response_property(response, "aborted", Value::Boolean(true));
    let signal = execute::get_property(response, "signal");
    if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
        crate::modules::http::abort_http_signal(state, &signal)?;
    }
    net::emit(state, response, "aborted", Vec::new())?;
    let has_error_listener = crate::modules::emitter::emitter_id(response)
        .and_then(|id| state.borrow().emitters.get(id))
        .is_some_and(|emitter| !emitter.borrow().listeners_of("error").is_empty());
    if has_error_listener {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("socket hang up".into())],
        );
        let error = execute::set_property(error, "code", Value::String("ECONNRESET".into()));
        net::emit(state, response, "error", vec![error])?;
    }
    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
        req.response_closed = true;
    }
    net::emit(state, response, "close", Vec::new())?;
    Ok(Value::Undefined)
}

// ---- helpers ----

fn build_req_object(state: &Rc<RefCell<HostState>>) -> Result<(Value, u64), VmError> {
    let mut object = crate::modules::events::new_emitter_object(state)?;
    let id = {
        let mut guard = state.borrow_mut();
        let id = guard.http.next_client;
        guard.http.next_client += 1;
        id
    };
    object = install_methods(
        object,
        vec![
            (
                "write".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_WRITE),
            ),
            (
                "end".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_END),
            ),
            (
                "setHeader".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_HEADER),
            ),
            (
                "abort".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_ABORT),
            ),
            (
                "destroy".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_CLIENT_DESTROY),
            ),
            (CLIENT_ID_PROP.to_string(), Value::Number(id as f64)),
            ("_header".to_string(), Value::String(String::new())),
            ("aborted".to_string(), Value::Boolean(false)),
            ("destroyed".to_string(), Value::Boolean(false)),
            ("finished".to_string(), Value::Boolean(false)),
            (
                CLIENT_CLOSE_PENDING_PROP.to_string(),
                Value::Boolean(false),
            ),
        ],
    )?;
    Ok((object, id))
}

fn set_response_property(response: &Value, key: &str, value: Value) {
    execute::set_property_in_place(response, key, value.clone());
    let updated = execute::set_property(response.clone(), key, value);
    execute::replace_value(response, &updated);
}

fn install_methods(mut object: Value, props: Vec<(String, Value)>) -> Result<Value, VmError> {
    for (key, value) in props {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        object = execute::define_property(object, &key, descriptor)?;
    }
    Ok(object)
}

fn client_id(receiver: Option<&Value>) -> Option<u64> {
    let receiver = receiver?;
    match quench_runtime::vm::get_property(receiver, CLIENT_ID_PROP) {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as u64),
        _ => None,
    }
}

fn custom_connection(agent: &Value) -> Option<Value> {
    let method = execute::get_property(agent, "createConnection");
    quench_runtime::is_callable(&method).then_some(method)
}

fn client_id_for_socket(state: &Rc<RefCell<HostState>>, socket: &Value) -> Option<u64> {
    if let Some(id) = net::net_id(socket) {
        if let Some(client_id) = state.borrow().http.clients.get(&id).copied() {
            return Some(client_id);
        }
    }
    state
        .borrow()
        .http
        .clientreqs
        .iter()
        .find(|(_, req)| req.socket.as_ref().is_some_and(|value| execute::same_identity(value, socket)))
        .map(|(id, _)| *id)
}

fn set_request_property(receiver: Option<&Value>, key: &str, value: Value) {
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, key, value.clone());
        let updated = execute::set_property(receiver.clone(), key, value);
        execute::replace_value(receiver, &updated);
    }
}

fn chunk_bytes(value: Option<&Value>) -> Vec<u8> {
    match value {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Uint8Array(view)) => {
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        _ => Vec::new(),
    }
}

fn head_end(buffer: &[u8]) -> Option<(usize, usize)> {
    for i in 0..buffer.len() {
        if buffer.get(i..i + 4) == Some(b"\r\n\r\n") {
            return Some((i, 4));
        }
        if buffer.get(i..i + 2) == Some(b"\n\n") {
            return Some((i, 2));
        }
    }
    None
}

/// Parse the response head into an IncomingMessage emitter.
fn build_incoming(state: &Rc<RefCell<HostState>>, head: &[u8]) -> Result<Value, VmError> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut fields = status_line.split_whitespace();
    let _version = fields.next().unwrap_or("HTTP/1.1");
    let status = fields
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let message = fields.collect::<Vec<_>>().join(" ");
    let mut headers: Vec<(String, Value)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let value = line[colon + 1..].trim();
            headers.push((key, Value::String(value.to_string())));
        }
    }
    let res = crate::modules::events::new_emitter_object(state)?;
    let props = vec![
        ("statusCode".to_string(), Value::Number(status as f64)),
        ("statusMessage".to_string(), Value::String(message)),
        ("httpVersion".to_string(), Value::String("1.1".to_string())),
        ("headers".to_string(), host_api::object(headers)),
        (
            "setEncoding".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_ENCODING),
        ),
        (
            "destroy".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_INCOMING_DESTROY),
        ),
        // Resuming an IncomingMessage only switches it into flowing mode;
        // the host already drains response bytes eagerly, so the same
        // identity-preserving capability is sufficient here.
        (
            "resume".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_ENCODING),
        ),
        ("signal".to_string(), crate::modules::http::new_http_signal(state)?),
        ("complete".to_string(), Value::Boolean(false)),
        ("readable".to_string(), Value::Boolean(true)),
        ("aborted".to_string(), Value::Boolean(false)),
        ("destroyed".to_string(), Value::Boolean(false)),
    ];
    install_methods(res, props)
}

fn request_options(value: Option<&Value>) -> Result<RequestOptions, VmError> {
    match value {
        Some(Value::String(url)) => http_url(url),
        Some(Value::Object(_) | Value::ObjectAlias(_)) => {
            let options = value.cloned().unwrap_or(Value::Undefined);
            // Legacy url.parse() exposes `host`/`path`, while WHATWG URL
            // exposes `hostname`/`pathname`/`search`; both are request facts.
            let raw_host = opt_first(&options, &["host", "hostname"])?
                .unwrap_or_else(|| "localhost".to_string());
            let socket_path = opt(&options, "socketPath")?;
            let explicit_port = opt(&options, "port")?.and_then(|p| p.parse().ok());
            let (host, port) = split_host_port(&raw_host, explicit_port);
            let raw_method = execute::get_property(&options, "method");
            if execute::is_symbol(&raw_method) {
                return Err(invalid_method_type_error(&raw_method));
            }
            let method = match raw_method {
                Value::Undefined | Value::Null => "GET".to_string(),
                Value::String(value) => value,
                other => return Err(invalid_method_type_error(&other)),
            };
            let method = if method.is_empty() {
                "GET".to_string()
            } else {
                method
            };
            if !is_http_token(&method) {
                return Err(invalid_method_error(&method));
            }
            let path = match opt(&options, "path")? {
                Some(path) if !path.is_empty() => path,
                None => {
                    let pathname = opt(&options, "pathname")?.unwrap_or_else(|| "/".to_string());
                    format!("{pathname}{}", opt(&options, "search")?.unwrap_or_default())
                }
                Some(_) => "/".to_string(),
            };
            if path
                .chars()
                .any(|character| !(('\u{21}'..='\u{FF}').contains(&character)))
            {
                return Err(unescaped_path_error());
            }
            let mut headers: Vec<(String, String)> = Vec::new();
            let hv = execute::get_property(&options, "headers");
            if !matches!(hv, Value::Undefined | Value::Null) {
                if let Ok(host_header) = execute::get_property_result(&hv, "host") {
                    if is_array_value(&host_header)
                        || !matches!(host_header, Value::Undefined | Value::Null | Value::String(_))
                    {
                        return Err(invalid_header_type_error());
                    }
                }
                if matches!(hv, Value::Array(_)) {
                    for key in execute::own_enumerable_keys(&hv) {
                        let Ok(pair) = execute::get_property_result(&hv, &key) else {
                            continue;
                        };
                        let name = execute::get_property_result(&pair, "0")
                            .ok()
                            .and_then(|v| execute::to_js_string(&v).ok());
                        let value = execute::get_property_result(&pair, "1").ok().and_then(|v| {
                            if name.as_deref().is_some_and(|key| key.eq_ignore_ascii_case("cookie"))
                                && matches!(v, Value::Array(_))
                            {
                                Some(
                                    execute::own_enumerable_keys(&v)
                                        .into_iter()
                                        .filter_map(|key| {
                                            execute::get_property_result(&v, &key)
                                                .ok()
                                                .and_then(|item| execute::to_js_string(&item).ok())
                                        })
                                        .collect::<Vec<_>>()
                                        .join("; "),
                                )
                            } else {
                                execute::to_js_string(&v).ok()
                            }
                        });
                        if let (Some(name), Some(value)) = (name, value) {
                            headers.push((name, value));
                        }
                    }
                } else {
                    for key in execute::own_enumerable_keys(&hv) {
                        let Ok(item) = execute::get_property_result(&hv, &key) else {
                            continue;
                        };
                        if key.eq_ignore_ascii_case("host") && is_array_value(&item) {
                            return Err(invalid_header_type_error());
                        }
                        let value = if key.eq_ignore_ascii_case("cookie")
                            && matches!(item, Value::Array(_))
                        {
                            execute::own_enumerable_keys(&item)
                                .into_iter()
                                .filter_map(|i| {
                                    execute::get_property_result(&item, &i)
                                        .ok()
                                        .and_then(|v| execute::to_js_string(&v).ok())
                                })
                                .collect::<Vec<_>>()
                                .join("; ")
                        } else {
                            execute::to_js_string(&item)?
                        };
                        headers.push((key, value));
                    }
                }
            }
            if !matches!(execute::get_property(&options, "headers"), Value::Array(_)) {
                if let Some(auth) = opt(&options, "auth")? {
                    headers.push((
                        "Authorization".into(),
                        format!("Basic {}", base64_encode(auth.as_bytes())),
                    ));
                }
            }
            let target = socket_path
                .filter(|path| !path.is_empty())
                .map_or(RequestTarget::Tcp { host, port }, |path| RequestTarget::Unix { path });
            Ok(RequestOptions { target, method, path, headers })
        }
        _ => Err(execute::type_error("options must be a string or object")),
    }
}

fn unescaped_path_error() -> VmError {
    let error = execute::type_error("Request path contains unescaped characters");
    let error = match error {
        VmError::Thrown(value) => execute::set_property(
            execute::set_property(value, "code", Value::String("ERR_UNESCAPED_CHARACTERS".into())),
            "name",
            Value::String("TypeError".into()),
        ),
        other => return other,
    };
    VmError::Thrown(error)
}

fn is_array_value(value: &Value) -> bool {
    matches!(
        execute::execute_builtin_with_receiver(
            quench_runtime::ops::Builtin::ArrayIsArray,
            &[],
            Some(value),
        ),
        Ok(Value::Boolean(true))
    )
}

fn invalid_header_type_error() -> VmError {
    match execute::type_error("The \"options.headers.host\" property must be of type string") {
        VmError::Thrown(value) => VmError::Thrown(execute::set_property(
            value,
            "code",
            Value::String("ERR_INVALID_ARG_TYPE".into()),
        )),
        other => other,
    }
}

fn invalid_method_error(method: &str) -> VmError {
    let error = execute::type_error(&format!(
        "Method must be a valid HTTP token [\"{method}\"]"
    ));
    let error = match error {
        VmError::Thrown(value) => execute::set_property(
            execute::set_property(value, "code", Value::String("ERR_INVALID_HTTP_TOKEN".into())),
            "name",
            Value::String("TypeError".into()),
        ),
        other => return other,
    };
    VmError::Thrown(error)
}

fn invalid_method_type_error(value: &Value) -> VmError {
    let received = if execute::is_symbol(value) {
        "type symbol (Symbol())".to_string()
    } else {
        match value {
        Value::Null => "null".to_string(),
        Value::Boolean(value) => format!("type boolean ({value})"),
        Value::Number(value) => format!("type number ({value})"),
        Value::String(value) => format!("type string ({value})"),
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_) | Value::Uint8Array(_) => {
            let constructor = execute::get_property(value, "constructor");
            let name = execute::to_js_string(&execute::get_property(&constructor, "name"))
                .unwrap_or_else(|_| "Object".into());
            format!("an instance of {name}")
        }
        _ => "type unknown".to_string(),
        }
    };
    let error = execute::type_error(&format!(
        "The \"options.method\" property must be of type string. Received {received}"
    ));
    match error {
        VmError::Thrown(value) => VmError::Thrown(execute::set_property(
            execute::set_property(value, "code", Value::String("ERR_INVALID_ARG_TYPE".into())),
            "name",
            Value::String("TypeError".into()),
        )),
        other => other,
    }
}

fn is_http_token(method: &str) -> bool {
    !method.is_empty()
        && method.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
        })
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0] as usize;
        let second = chunk.get(1).copied().unwrap_or(0) as usize;
        let third = chunk.get(2).copied().unwrap_or(0) as usize;
        output.push(TABLE[first >> 2] as char);
        output.push(TABLE[((first & 3) << 4) | (second >> 4)] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((second & 15) << 2) | (third >> 6)] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 { TABLE[third & 63] as char } else { '=' });
    }
    output
}

fn opt(options: &Value, key: &str) -> Result<Option<String>, VmError> {
    match execute::get_property_result(options, key)? {
        Value::Undefined => Ok(None),
        other => execute::to_js_string(&other).map(Some),
    }
}

fn split_host_port(raw_host: &str, explicit_port: Option<u16>) -> (String, u16) {
    if let Some(port) = explicit_port {
        return (host_without_port(raw_host), port);
    }
    if let Some((host, port)) = raw_host.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            return (host.trim_matches(['[', ']']).to_string(), port);
        }
    }
    (raw_host.trim_matches(['[', ']']).to_string(), 80)
}

fn host_without_port(raw_host: &str) -> String {
    raw_host
        .rsplit_once(':')
        .filter(|(_, port)| port.parse::<u16>().is_ok())
        .map(|(host, _)| host)
        .unwrap_or(raw_host)
        .trim_matches(['[', ']'])
        .to_string()
}

fn opt_first(options: &Value, keys: &[&str]) -> Result<Option<String>, VmError> {
    for key in keys {
        if let Some(value) = opt(options, key)? {
            if !value.is_empty() {
                return Ok(Some(value));
            }
        }
    }
    Ok(None)
}

fn http_url(value: &str) -> Result<RequestOptions, VmError> {
    let rest = value.strip_prefix("http://").unwrap_or(value);
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.parse().unwrap()),
        _ => (authority.to_string(), 80),
    };
    Ok(RequestOptions {
        target: RequestTarget::Tcp { host, port },
        method: "GET".to_string(),
        path: path.to_string(),
        headers: Vec::new(),
    })
}
