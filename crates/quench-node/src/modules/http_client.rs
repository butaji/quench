//! `http_client` — minimal `http.request` / `http.get`. Builds a
//! ClientRequest emitter; `end()` connects via `net`, writes the
//! request head + body, parses the response, and emits `'response'`
//! with an IncomingMessage that streams `'data'`/`'end'`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::{ObjectAliasValue, Value};

use crate::host::HostState;
use crate::modules::net;

/// Hidden property mapping a ClientRequest object to its state.
const CLIENT_ID_PROP: &str = "\0quench:http:req:id";
pub(crate) const CLIENT_ASYNC_RESOURCE_PROP: &str = "\0quench:http:req:async-resource";
pub(crate) const RES_ASYNC_RESOURCE_PROP: &str = "\0quench:http:res:async-resource";
const CLIENT_CLOSE_PENDING_PROP: &str = "\0quench:http:req:close-pending";
const CLIENT_EXPECT_CONTINUE_PROP: &str = "\0quench:http:req:expect-continue";
const CLIENT_TIMEOUT_PROP: &str = "\0quench:http:req:timeout";
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
    pub lookup: Option<Value>,
    pub omit_host: bool,
    pub socket: Option<Value>,
    pub dispatched: bool,
    pub timeout: Option<Value>,
    pub timeout_set: bool,
    pub buffer: Vec<u8>,
    pub res: Option<Value>,
    pub head_parsed: bool,
    pub aborted: bool,
    pub response_ended: bool,
    pub response_closed: bool,
    /// Raw body bytes received after the response head.
    pub response_received: usize,
    pub response_chunked_done: bool,
    pub parse_error: bool,
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
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or_else(|| host_api::object(Vec::new()));
    let option = |name: &str| execute::get_property(&options, name);
    let mut object = crate::modules::events::new_emitter_object(state)?;
    if let Some(prototype) = state.borrow().http.agent_prototype.clone() {
        object = execute::set_prototype_of(&object, &prototype)?;
    }
    object = execute::set_property(object, "sockets", host_api::object(Vec::new()));
    object = execute::set_property(object, "freeSockets", host_api::object(Vec::new()));
    object = execute::set_property(object, "requests", host_api::object(Vec::new()));
    object = execute::set_property(object, "options", options.clone());
    object = execute::set_property(object, "keepAlive", match option("keepAlive") {
        Value::Boolean(value) => Value::Boolean(value),
        _ => Value::Boolean(false),
    });
    object = execute::set_property(object, "keepAliveMsecs", match option("keepAliveMsecs") {
        Value::Number(value) => Value::Number(value),
        _ => Value::Number(1000.0),
    });
    object = execute::set_property(object, "maxSockets", match option("maxSockets") {
        Value::Number(value) => Value::Number(value),
        _ => Value::Number(f64::INFINITY),
    });
    object = execute::set_property(object, "maxTotalSockets", match option("maxTotalSockets") {
        Value::Number(value) => Value::Number(value),
        _ => Value::Number(f64::INFINITY),
    });
    object = execute::set_property(object, "scheduling", match option("scheduling") {
        Value::String(value) => Value::String(value),
        _ => Value::String("lifo".into()),
    });
    object = execute::set_property(object, "defaultPort", Value::Number(80.0));
    object = execute::set_property(object, "protocol", Value::String("http:".into()));
    object = execute::set_property(object, "timeout", match option("timeout") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => Value::Number(value),
        _ => Value::Number(0.0),
    });
    object = execute::set_property(
        object,
        "agentKeepAliveTimeoutBuffer",
        match option("agentKeepAliveTimeoutBuffer") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => Value::Number(value),
            _ => Value::Number(1000.0),
        },
    );
    object = execute::set_property(object, "totalSocketCount", Value::Number(0.0));
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

pub fn agent_connect(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(request) = args.first().cloned() else {
        return Ok(Value::Undefined);
    };
    let error = args.get(1).cloned().unwrap_or(Value::Null);
    if !matches!(error, Value::Undefined | Value::Null) {
        set_request_property(Some(&request), "destroyed", Value::Boolean(true));
        net::emit(state, &request, "error", vec![error])?;
        return Ok(Value::Undefined);
    }
    let Some(socket) = args.get(2).cloned() else {
        return Ok(Value::Undefined);
    };
    if !matches!(socket, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(Value::Undefined);
    }
    if let Some(id) = client_id(Some(&request)) {
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
            req.socket = Some(socket);
        }
    }
    set_request_property(Some(&request), "finished", Value::Undefined);
    req_end(state, Some(&request), &[])
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
    let default_agent = state
        .borrow()
        .module_cache
        .get("http")
        .map(|module| execute::get_property(module, "globalAgent"))
        .filter(|value| !matches!(value, Value::Undefined | Value::Null));
    let agent = agent
        .or(default_agent)
        .or_else(|| state.borrow().http.global_agent.clone());
    let lookup = args.first().and_then(|options| {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return None;
        }
        let lookup = execute::get_property(options, "lookup");
        quench_runtime::is_callable(&lookup).then_some(lookup)
    });
    let omit_host = args.first().is_some_and(|options| {
        matches!(
            execute::get_property(options, "headers"),
            Value::Array(_)
        )
    });
    let expect_continue = opts
        .headers
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("expect") && value.eq_ignore_ascii_case("100-continue"));
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
            lookup,
            omit_host,
            socket: None,
            dispatched: false,
            timeout: None,
            timeout_set: false,
            buffer: Vec::new(),
            res: None,
            head_parsed: false,
            aborted: false,
            response_ended: false,
            response_closed: false,
            response_received: 0,
            response_chunked_done: false,
            parse_error: false,
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
    if let Some(options) = args.first().filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_))) {
        let timeout = execute::get_property(options, "timeout");
        if !matches!(timeout, Value::Undefined) {
            if !matches!(timeout, Value::Number(_)) {
                return Err(crate::modules::buffer_enc::invalid_arg_type(
                    "The \"timeout\" argument must be of type number".into(),
                ));
            }
            req_set_timeout(state, Some(&req), &[timeout])?;
        }
    }
    if expect_continue {
        set_request_property(Some(&req), CLIENT_EXPECT_CONTINUE_PROP, Value::Boolean(true));
        let _ = req_end(state, Some(&req), &[])?;
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
    let (request, socket, target, custom_connection_pending) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        if req.aborted || matches!(execute::get_property(&req.req, "destroyed"), Value::Boolean(true)) {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        }
        req.aborted = true;
        (
            req.req.clone(),
            req.socket.clone(),
            req.target.clone(),
            req.agent.as_ref().and_then(custom_connection).is_some(),
        )
    };
    clear_request_timeout(state, id)?;
    set_request_property(receiver, "aborted", Value::Boolean(true));
    set_request_property(receiver, "destroyed", Value::Boolean(true));
    net::emit(state, &request, "abort", Vec::new())?;
    if let Some(socket) = socket.filter(|_| !custom_connection_pending) {
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
    clear_request_timeout(state, id)?;
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
        let close_pending = matches!(
            execute::get_property(&response, crate::modules::http::INCOMING_CLOSE_PENDING_PROP),
            Value::Boolean(true)
        );
        if !close_pending {
            net::emit(state, &response, "close", Vec::new())?;
        }
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
    let bytes = request_chunk_bytes(args)?;
    if bytes.is_empty() {
        invoke_write_callback(receiver, args)?;
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    let (open, custom, dispatched) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        req.body.extend_from_slice(&bytes);
        (
            req.socket.is_none(),
            req.agent.as_ref().and_then(custom_connection).is_some(),
            req.dispatched,
        )
    };
    if open && custom && !dispatched {
        // A write starts an HTTP request even when the caller delays `end`.
        // Reuse the ordinary dispatch path so Agent connection hooks, socket
        // bookkeeping, and async resource identity stay identical to `end`.
        let request = receiver.cloned().unwrap_or(Value::Undefined);
        req_end(state, Some(&request), &[])?;
    } else if open {
        let target = state.borrow().http.clientreqs.get(&id).map(|req| req.target.clone());
        if let Some(target) = target {
            let socket = open_socket(state, &target)?;
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
                req.socket = Some(socket.clone());
                set_request_property(Some(&req.req), "socket", socket.clone());
                if let Some(agent) = req.agent.clone() {
                    let name = agent_name(&target, &agent);
                    add_agent_socket(&agent, &name, &socket);
                }
            }
            if let Some(socket_id) = net::net_id(&socket) {
                state.borrow_mut().http.clients.insert(socket_id, id);
            }
        }
    }
    invoke_write_callback(receiver, args)?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// Preserve `req.end(chunk, encoding, callback)` conversion while keeping the
/// completion callback out of the write path.
fn end_chunk_args(args: &[Value]) -> &[Value] {
    match args.get(1) {
        Some(Value::String(_) | Value::StringUnits(_)) => &args[..2],
        _ => &args[..1],
    }
}

fn request_chunk_bytes(args: &[Value]) -> Result<Vec<u8>, VmError> {
    let Some(value) = args.first() else {
        return Ok(Vec::new());
    };
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        let encoding = args
            .get(1)
            .filter(|value| matches!(value, Value::String(_) | Value::StringUnits(_)))
            .map(execute::to_js_string)
            .transpose()?
            .unwrap_or_else(|| "utf8".into());
        let encoding = crate::modules::buffer_enc::canonical_encoding(&encoding)
            .ok_or_else(|| crate::modules::buffer_enc::unknown_encoding(&encoding))?;
        return crate::modules::buffer_enc::encode_value(value, encoding);
    }
    Ok(chunk_bytes(Some(value)))
}

fn invoke_write_callback(receiver: Option<&Value>, args: &[Value]) -> Result<(), VmError> {
    let callback = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
        .or_else(|| args.get(2).filter(|value| quench_runtime::is_callable(value)));
    if let Some(callback) = callback {
        execute::call(callback, receiver.unwrap_or(&Value::Undefined), &[])?;
    }
    Ok(())
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

/// `req.setTimeout(msecs[, callback])` — schedule the request timeout event.
/// The timer is attached to the request itself so it works before a socket is
/// dispatched and follows the same lifecycle as the ClientRequest.
pub fn req_set_timeout(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = client_id(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let timeout = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value,
        Some(Value::Number(_)) => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The value of \"msecs\" is out of range".into(),
            ))
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"msecs\" argument must be a number".into(),
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
    let request = receiver.cloned().unwrap_or(Value::Undefined);
    let old_timer = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .and_then(|req| req.timeout.clone());
    if let Some(timer) = old_timer {
        crate::modules::timers::clear_timeout(state, &[timer])?;
    }
    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
        req.timeout = None;
        req.timeout_set = true;
    }
    set_request_property(Some(&request), "timeout", Value::Number(timeout));
    let timeout_cb = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_HTTP_REQ_TIMEOUT_FIRE.cap,
            ),
        },
        vec![request.clone()],
    );
    set_request_property(Some(&request), "timeoutCb", timeout_cb.clone());
    if let Some(callback) = args.get(1) {
        let callback = if let Ok(bind) = execute::get_property_result(callback, "bind") {
            execute::call(&bind, callback, &[request.clone()]).unwrap_or_else(|_| callback.clone())
        } else {
            callback.clone()
        };
        crate::modules::events::method_once(
            state,
            Some(&request),
            &[Value::String("timeout".into()), callback],
        )?;
    }
    let socket = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .and_then(|req| req.socket.clone());
    if let Some(socket) = socket {
        if timeout > 0.0 {
            net::socket_set_timeout(
                state,
                Some(&socket),
                &[Value::Number(timeout), timeout_cb],
            )?;
        } else {
            subscribe_event(state, &socket, "timeout", timeout_cb)?;
            net::socket_set_timeout(state, Some(&socket), &[Value::Number(timeout)])?;
        }
    } else if timeout > 0.0 {
        let timer = crate::modules::timers::set_timeout(
            state,
            &[timeout_cb, Value::Number(timeout)],
        )?;
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
            req.timeout = Some(timer.clone());
        }
        set_request_property(Some(&request), CLIENT_TIMEOUT_PROP, timer);
    } else {
        set_request_property(Some(&request), CLIENT_TIMEOUT_PROP, Value::Undefined);
    }
    Ok(request)
}

/// Timer callback for one ClientRequest. Event listeners own the observable
/// response (typically aborting the request); this transition only emits once.
pub fn req_timeout_fire(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if matches!(args.get(1), Some(Value::Boolean(false))) {
        return Ok(Value::Undefined);
    }
    let Some(request) = args.first() else {
        return Ok(Value::Undefined);
    };
    let Some(id) = client_id(Some(request)) else {
        return Ok(Value::Undefined);
    };
    let active = state.borrow().http.clientreqs.get(&id).is_some_and(|req| {
        !req.aborted && !matches!(execute::get_property(&req.req, "destroyed"), Value::Boolean(true))
    });
    if !active {
        return Ok(Value::Undefined);
    }
    let has_socket = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .and_then(|req| req.socket.as_ref())
        .is_some();
    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
        req.timeout = None;
        if !has_socket {
            req.timeout_set = false;
        }
    }
    let socket = {
        state
            .borrow()
            .http
            .clientreqs
            .get(&id)
            .and_then(|req| req.socket.clone())
    };
    if let Some(socket) = socket {
        state
            .borrow_mut()
            .net
            .pending_lookups
            .retain(|pending| !execute::same_identity(&pending.socket, &socket));
    }
    set_request_property(Some(request), CLIENT_TIMEOUT_PROP, Value::Undefined);
    net::emit(state, request, "timeout", Vec::new())?;
    Ok(Value::Boolean(true))
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
    let expect_started = state.borrow().http.clientreqs.get(&id).is_some_and(|req| {
        matches!(execute::get_property(&req.req, CLIENT_EXPECT_CONTINUE_PROP), Value::Boolean(true))
            && req.socket.is_some()
    });
    if expect_started {
        if args.first().is_some_and(|data| !matches!(data, Value::Undefined)) {
            req_write(state, receiver, end_chunk_args(args))?;
        }
        let (socket, body) = {
            let mut guard = state.borrow_mut();
            let Some(req) = guard.http.clientreqs.get_mut(&id) else {
                return Ok(receiver.cloned().unwrap_or(Value::Undefined));
            };
            (req.socket.clone(), std::mem::take(&mut req.body))
        };
        if let Some(socket) = socket {
            let mut payload = Vec::new();
            if !body.is_empty() {
                payload.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
                payload.extend_from_slice(&body);
                payload.extend_from_slice(b"\r\n");
            }
            payload.extend_from_slice(b"0\r\n\r\n");
            net::socket_write(state, Some(&socket), &[host_api::bytes(&payload)])?;
        }
        if let Some(request) = receiver {
            set_request_property(Some(request), "finished", Value::Boolean(true));
            set_request_property(Some(request), CLIENT_EXPECT_CONTINUE_PROP, Value::Boolean(false));
        }
        invoke_end_callback(receiver, args)?;
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    let queued = {
        let mut guard = state.borrow_mut();
        let Some(current) = guard.http.clientreqs.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        let current_agent = current.agent.clone();
        let current_target = current.target.clone();
        let current_req = current.req.clone();
        drop(current);
        match current_agent.as_ref() {
            None => false,
            Some(agent) => {
                let max = match execute::get_property(agent, "maxSockets") {
                    Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
                    _ => usize::MAX,
                };
                if max == usize::MAX {
                    false
                } else {
                    let active = guard
                        .http
                        .clientreqs
                        .values()
                        .filter(|request| {
                            request.dispatched
                                && !request.response_closed
                                && !request.aborted
                                && request.agent.as_ref().is_some_and(|candidate| {
                                    execute::same_identity(candidate, agent)
                                        && agent_name(&request.target, agent)
                                            == agent_name(&current_target, agent)
                                })
                        })
                        .count();
                    if active >= max {
                        if !guard.http.agent_pending.contains(&id) {
                            guard.http.agent_pending.push(id);
                        }
                        let name = agent_name(&current_target, agent);
                        add_agent_request(agent, &name, &current_req);
                        true
                    } else {
                        false
                    }
                }
            }
        }
    };
    if queued {
        if let Some(request) = receiver {
            set_request_property(Some(request), "finished", Value::Boolean(true));
        }
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    if let Some(request) = state.borrow_mut().http.clientreqs.get_mut(&id) {
        request.dispatched = true;
    }
    if let Some(data) = args.first() {
        if !matches!(data, Value::Undefined) {
            req_write(state, receiver, end_chunk_args(args))?;
        }
    }
    let (target, method, path, headers, body, agent, lookup, omit_host) = {
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
            req.lookup.clone(),
            req.omit_host,
        )
    };
    let expect_header_start = receiver.is_some_and(|request| {
        matches!(execute::get_property(request, CLIENT_EXPECT_CONTINUE_PROP), Value::Boolean(true))
    });
    if let Some(req_value) = receiver.filter(|_| !expect_header_start) {
        set_request_property(Some(req_value), "finished", Value::Boolean(true));
    }
    let host = target_host(&target);
    let head_host = request_host(&target);
    let head = request_head(&head_host, &method, &path, &headers, body.len(), omit_host);
    if let Some(req) = state.borrow().http.clientreqs.get(&id) {
        set_request_property(Some(&req.req), "_header", Value::String(head.clone()));
    }
    let custom = agent.as_ref().and_then(custom_connection);
    let existing = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .and_then(|req| req.socket.clone());
    let pooled = if existing.is_none() {
        agent
            .as_ref()
            .and_then(|agent| take_agent_socket(agent, &agent_name(&target, agent)))
    } else {
        None
    };
    let socket = match (existing.or(pooled), custom) {
        (Some(socket), _) => {
            net::socket_ref(state, Some(&socket), &[])?;
            let mut bytes = head.into_bytes();
            bytes.extend_from_slice(&body);
            net::socket_write(state, Some(&socket), &[host_api::bytes(&bytes)])?;
            socket
        }
        (None, Some(connection)) => {
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
            let callback = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(crate::registry::SPEC_HTTP_AGENT_CONNECT),
                vec![receiver.cloned().unwrap_or(Value::Undefined)],
            );
            let socket = execute::call(&connection, agent.as_ref().unwrap(), &[options, callback])?;
            if matches!(socket, Value::Undefined | Value::Null) {
                return Ok(receiver.cloned().unwrap_or(Value::Undefined));
            }
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
        (None, None) if lookup.is_some() => {
            let host = target_host(&target);
            let mut options = vec![
                ("host".into(), Value::String(host.clone())),
                ("hostname".into(), Value::String(host)),
                ("lookup".into(), lookup.clone().unwrap()),
            ];
            if let RequestTarget::Tcp { port, .. } = &target {
                options.push(("port".into(), Value::Number(*port as f64)));
            }
            net::connect(state, &[host_api::object(options)])?
        }
        (None, None) => send_request(state, &target, &method, &path, &headers, &body, omit_host)?,
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
    let (request, request_timeout, timeout_cb, agent_timeout) = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        (
            req.req.clone(),
            execute::get_property(&req.req, "timeout"),
            execute::get_property(&req.req, "timeoutCb"),
            req.agent.as_ref().map(|agent| execute::get_property(agent, "timeout")),
        )
    };
    let pending_timer = {
        state
            .borrow()
            .http
            .clientreqs
            .get(&id)
            .and_then(|req| req.timeout.clone())
    };
    if let Some(timer) = pending_timer {
        crate::modules::timers::clear_timeout(state, &[timer])?;
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
            req.timeout = None;
        }
    }
    let request_timeout_set = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .is_some_and(|req| req.timeout_set);
    let request_timeout_cb = if quench_runtime::is_callable(&timeout_cb) {
        timeout_cb
    } else {
        quench_runtime::host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_HTTP_REQ_TIMEOUT_FIRE.cap,
                ),
            },
            vec![request.clone()],
        )
    };
    set_request_property(Some(&request), "timeoutCb", request_timeout_cb.clone());
    let effective_timeout = if request_timeout_set {
        request_timeout
    } else {
        agent_timeout.unwrap_or(Value::Number(0.0))
    };
    if let Value::Number(value) = effective_timeout {
        if value.is_finite() && value > 0.0 {
            let internal_cb = quench_runtime::host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_HTTP_REQ_TIMEOUT_FIRE.cap,
                    ),
                },
                vec![request.clone(), Value::Boolean(false)],
            );
            subscribe_event(state, &socket, "timeout", internal_cb)?;
            net::socket_set_timeout(
                state,
                Some(&socket),
                &[Value::Number(value), request_timeout_cb.clone()],
            )?;
            if request_timeout_set && lookup.is_none() {
                let response_cb = quench_runtime::host_api::bound_capability_with_arguments(
                    quench_runtime::ops::HostCapabilityRef {
                        realm: quench_runtime::ops::RealmId::ROOT,
                        kind: quench_runtime::ops::HostCapabilityKind::Custom(
                            crate::registry::SPEC_HTTP_REQ_TIMEOUT_FIRE.cap,
                        ),
                    },
                    vec![request.clone(), Value::Boolean(false)],
                );
                subscribe_event(state, &socket, "timeout", response_cb)?;
            }
        } else if request_timeout_set {
            subscribe_event(state, &socket, "timeout", request_timeout_cb.clone())?;
        }
    }
    state
        .borrow_mut()
        .net
        .pending_events
        .push((request, "socket".into(), vec![socket]));
    invoke_end_callback(receiver, args)?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn invoke_end_callback(receiver: Option<&Value>, args: &[Value]) -> Result<(), VmError> {
    let callback = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
        .or_else(|| args.get(2).filter(|value| quench_runtime::is_callable(value)));
    if let Some(callback) = callback {
        execute::call(callback, receiver.unwrap_or(&Value::Undefined), &[])?;
    }
    Ok(())
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
    let socket = open_socket(state, target)?;
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

fn open_socket(
    state: &Rc<RefCell<HostState>>,
    target: &RequestTarget,
) -> Result<Value, VmError> {
    match target {
        RequestTarget::Tcp { host, port } => net::connect(
            state,
            &[Value::Number(*port as f64), Value::String(host.clone())],
        ),
        RequestTarget::Unix { path } => net::connect_path(state, path),
    }
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
    let expect_continue = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("expect") && value.eq_ignore_ascii_case("100-continue")
    });
    if !has_content_length && expect_continue {
        if !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
        {
            head.push_str("Transfer-Encoding: chunked\r\n");
        }
    } else if !has_content_length && (body_len > 0 || default_empty_body(method)) {
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
        let requests = state.borrow().http.conns.values()
            .flat_map(|conn| conn.requests.iter().cloned())
            .collect::<Vec<_>>();
        for request in requests {
            if matches!(execute::get_property(&request, crate::modules::http::REQ_CLOSE_PROP), Value::Boolean(true)) {
                continue;
            }
            execute::set_property_in_place(&request, crate::modules::http::REQ_CLOSE_PROP, Value::Boolean(true));
            net::emit(state, &request, "close", Vec::new())?;
            let resource = execute::get_property(&request, crate::modules::http::REQ_ASYNC_RESOURCE_PROP);
            crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
        }
        return Ok(Value::Undefined);
    };
    clear_request_timeout(state, client_id)?;
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
        let resource = execute::get_property(&request, CLIENT_ASYNC_RESOURCE_PROP);
        crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
    }
    if response.as_ref().is_some_and(|value| {
        matches!(execute::get_property(value, "complete"), Value::Boolean(true))
    }) {
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
            req.response_closed = true;
        }
    }
    if let (Some(agent), Some(target)) = (agent.as_ref(), target.as_ref()) {
        let name = agent_name(target, agent);
        let has_pending = state.borrow().http.agent_pending.iter().any(|id| {
            state.borrow().http.clientreqs.get(id).is_some_and(|request| {
                request.agent.as_ref().is_some_and(|candidate| {
                    execute::same_identity(candidate, agent)
                        && agent_name(&request.target, agent) == name
                })
            })
        });
        if has_pending {
            emit_agent_free(state, agent, target, socket)?;
        }
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
        drain_agent_pending(state, agent, &name);
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

fn drain_agent_pending(state: &Rc<RefCell<HostState>>, agent: &Value, name: &str) {
    let pending = {
        let Ok(mut guard) = state.try_borrow_mut() else {
            return;
        };
        let position = guard.http.agent_pending.iter().position(|id| {
            guard.http.clientreqs.get(id).is_some_and(|request| {
                request.agent.as_ref().is_some_and(|candidate| {
                    execute::same_identity(candidate, agent)
                        && agent_name(&request.target, agent) == name
                })
            })
        });
        position.and_then(|index| {
            guard.http.agent_pending.get(index).copied().map(|id| {
                guard.http.agent_pending.remove(index);
                id
            })
        })
    };
    let Some(id) = pending else { return; };
    let (request, agent_request) = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .map(|request| {
            (
                request.req.clone(),
                request
                    .agent
                    .as_ref()
                    .map(|agent| (agent.clone(), agent_name(&request.target, agent))),
            )
        })
        .unwrap_or((Value::Undefined, None));
    if let Some((agent, name)) = agent_request {
        remove_agent_request(&agent, &name, &request);
    }
    if matches!(request, Value::Undefined) { return; }
    set_request_property(Some(&request), "finished", Value::Boolean(false));
    let _ = req_end(state, Some(&request), &[]);
}

fn agent_name(target: &RequestTarget, agent: &Value) -> String {
    let options = match target {
        RequestTarget::Tcp { host, port } => host_api::object(vec![
            ("host".into(), Value::String(host.clone())),
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

fn emit_agent_free(
    state: &Rc<RefCell<HostState>>,
    agent: &Value,
    target: &RequestTarget,
    socket: &Value,
) -> Result<(), VmError> {
    let options = match target {
        RequestTarget::Tcp { host, port } => host_api::object(vec![
            ("host".into(), Value::String(host.clone())),
            ("port".into(), Value::Number(*port as f64)),
        ]),
        RequestTarget::Unix { path } => {
            host_api::object(vec![("socketPath".into(), Value::String(path.clone()))])
        }
    };
    net::emit(state, agent, "free", vec![socket.clone(), options])
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
    if matches!(list, Value::Array(_)) {
        execute::set_array_element_in_place(&list, length, socket.clone());
        execute::set_array_length_in_place(&list, length + 1);
    } else {
        execute::set_property_in_place(&list, &length.to_string(), socket.clone());
        execute::set_property_in_place(&list, "length", Value::Number((length + 1) as f64));
    }
}

fn add_agent_request(agent: &Value, name: &str, request: &Value) {
    let pools = execute::get_property(agent, "requests");
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
    if matches!(list, Value::Array(_)) {
        execute::set_array_element_in_place(&list, length, request.clone());
        execute::set_array_length_in_place(&list, length + 1);
    } else {
        execute::set_property_in_place(&list, &length.to_string(), request.clone());
        execute::set_property_in_place(&list, "length", Value::Number((length + 1) as f64));
    }
}

fn remove_agent_request(agent: &Value, name: &str, request: &Value) {
    let pools = execute::get_property(agent, "requests");
    let list = execute::get_property(&pools, name);
    let values: Vec<Value> = execute::own_enumerable_keys(&list)
        .into_iter()
        .filter_map(|key| {
            let value = execute::get_property(&list, &key);
            (!execute::same_identity(&value, request)).then_some(value)
        })
        .collect();
    if values.is_empty() {
        let (updated, _) = execute::delete_property(pools.clone(), name);
        execute::replace_value(&pools, &updated);
    } else {
        execute::set_property_in_place(&pools, name, host_api::array(values));
    }
}

fn take_agent_socket(agent: &Value, name: &str) -> Option<Value> {
    let pools = execute::get_property(agent, "freeSockets");
    let list = execute::get_property(&pools, name);
    let length = match execute::get_property(&list, "length") {
        Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
        _ => return None,
    };
    let socket = execute::get_property(&list, "0");
    if !matches!(socket, Value::Object(_) | Value::ObjectAlias(_)) {
        return None;
    }
    let shift = execute::get_property(&list, "shift");
    if quench_runtime::is_callable(&shift) {
        let _ = execute::call(&shift, &list, &[]);
    } else {
        for index in 1..length {
            let value = execute::get_property(&list, &index.to_string());
            execute::set_property_in_place(&list, &(index - 1).to_string(), value);
        }
        execute::set_property_in_place(
            &list,
            "length",
            Value::Number((length - 1) as f64),
        );
    }
    Some(socket)
}

fn move_agent_socket_to_free(agent: &Value, name: &str, socket: &Value) {
    let sockets_pools = execute::get_property(agent, "sockets");
    let sockets = execute::get_property(&sockets_pools, name);
    let remaining: Vec<Value> = execute::own_enumerable_keys(&sockets)
        .into_iter()
        .filter_map(|key| {
            let value = execute::get_property(&sockets, &key);
            (!execute::same_identity(&value, socket)).then_some(value)
        })
        .filter(|value| !matches!(value, Value::Undefined))
        .collect();
    let found = remaining.len() < execute::own_enumerable_keys(&sockets).len();
    if !found {
        return;
    }
    execute::set_property_in_place(&sockets_pools, name, host_api::array(remaining));
    let free_pools = execute::get_property(agent, "freeSockets");
    let free = match execute::get_property(&free_pools, name) {
        Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_) => {
            execute::get_property(&free_pools, name)
        }
        _ => {
            let list = host_api::array(Vec::new());
            execute::set_property_in_place(&free_pools, name, list.clone());
            list
        }
    };
    let push = execute::get_property(&free, "push");
    if quench_runtime::is_callable(&push) {
        let _ = execute::call(&push, &free, &[socket.clone()]);
    } else {
        execute::set_property_in_place(&free, "0", socket.clone());
        execute::set_property_in_place(&free, "length", Value::Number(1.0));
    }
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
            if response_status(&head) == Some(100) {
                if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                    req.head_parsed = false;
                }
                if let Some(request) = client_value(state, client_id, true) {
                    net::emit(state, &request, "continue", Vec::new())?;
                }
                return Ok(Value::Undefined);
            }
            let raw_body = state
                .borrow()
                .http
                .clientreqs
                .get(&client_id)
                .map(|req| req.buffer.clone())
                .unwrap_or_default();
            if let Some(error) = invalid_response_framing(&head, &raw_body) {
                let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
                net::emit(state, &request, "error", vec![error])?;
                net::socket_destroy(state, Some(socket), &[])?;
                return Ok(Value::Undefined);
            }
            let res = build_incoming(state, client_id, &head)?;
            if let Some(socket) = state
                .borrow()
                .http
                .clientreqs
                .get(&client_id)
                .and_then(|req| req.socket.clone())
            {
                set_response_property(&res, "socket", socket.clone());
                // IncomingMessage exposes the same connection identity via
                // both historical `connection` and modern `socket` names.
                set_response_property(&res, "connection", socket);
            }
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.res = Some(res.clone());
            }
            let req_value = client_value(state, client_id, true).unwrap_or(Value::Undefined);
            net::emit(state, &req_value, "response", vec![res])?;
            if let Some(response) = client_value(state, client_id, false) {
                net::emit(state, &response, "readable", Vec::new())?;
            }
            flush_body(state, client_id)?;
            finish_known_response(state, client_id)
        }
        None if head_parsed => {
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.response_received = req.response_received.saturating_add(bytes.len());
                if bytes.windows(5).any(|window| window == b"0\r\n\r\n") {
                    req.response_chunked_done = true;
                }
            }
            if let Some(res) = client_value(state, client_id, false) {
                net::emit(state, &res, "readable", Vec::new())?;
                let body = response_body_bytes(&res, &bytes);
                if !body.is_empty() {
                    net::emit(state, &res, "data", vec![response_data(&res, &body)])?;
                }
            }
            finish_known_response(state, client_id)?;
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
    let (res, rest, consumed) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
            return Ok(());
        };
        let consumed = req
            .res
            .as_ref()
            .and_then(response_content_length)
            .map(|expected| expected.saturating_sub(req.response_received).min(req.buffer.len()))
            .unwrap_or(req.buffer.len());
        req.response_received = req.response_received.saturating_add(consumed);
        if req.buffer.windows(5).any(|window| window == b"0\r\n\r\n") {
            req.response_chunked_done = true;
        }
        let buffer = std::mem::take(&mut req.buffer);
        (req.res.clone(), buffer, consumed)
    };
    if let Some(res) = res {
        if consumed > 0 {
            let body = response_body_bytes(&res, &rest[..consumed]);
            if !body.is_empty() {
                net::emit(state, &res, "data", vec![response_data(&res, &body)])?;
            }
        }
    }
    Ok(())
}

fn response_content_length(response: &Value) -> Option<usize> {
    let headers = execute::get_property(response, "headers");
    execute::to_js_string(&execute::get_property(&headers, "content-length"))
        .ok()
        .and_then(|value| value.parse().ok())
}

fn response_allows_reuse(response: &Value) -> bool {
    let headers = execute::get_property(response, "headers");
    !execute::to_js_string(&execute::get_property(&headers, "connection"))
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("close"))
}

fn response_status(head: &[u8]) -> Option<u16> {
    String::from_utf8_lossy(head)
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
}

fn finish_known_response(
    state: &Rc<RefCell<HostState>>,
    client_id: u64,
) -> Result<(), VmError> {
    let socket = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&client_id) else {
            return Ok(());
        };
        let Some(response) = req.res.as_ref() else {
            return Ok(());
        };
        let headers = execute::get_property(response, "headers");
        let expected = execute::to_js_string(&execute::get_property(&headers, "content-length"))
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        let chunked = execute::to_js_string(&execute::get_property(&headers, "transfer-encoding"))
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
        let complete = expected.is_some_and(|length| req.response_received >= length)
            || (chunked && req.response_chunked_done);
        complete.then(|| req.socket.clone()).flatten()
    };
    if let Some(socket) = socket {
        res_end_handler(state, Some(&socket), &[])?;
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

fn invalid_response_framing(head: &[u8], body: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(head);
    let has_length = text.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    });
    let has_encoding = text.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            key.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
        })
    });
    if !(has_length && has_encoding) {
        return None;
    }
    let reason = "Transfer-Encoding can't be present with Content-Length";
    let mut raw = head.to_vec();
    raw.extend_from_slice(b"\r\n\r\n");
    raw.extend_from_slice(body);
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(format!("Parse Error: {reason}"))],
    );
    let error = execute::set_property(error, "code", Value::String("HPE_INVALID_TRANSFER_ENCODING".into()));
    let error = execute::set_property(error, "reason", Value::String(reason.into()));
    let error = execute::set_property(error, "bytesParsed", Value::Number((head.len() + 4) as f64));
    Some(execute::set_property(error, "rawPacket", crate::modules::buffer_proto::make_buffer(&raw)))
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
        if expected.is_some_and(|expected| received > expected) {
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.parse_error = true;
            }
            let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
            net::emit(state, &request, "error", vec![invalid_response_constant()])?;
            net::socket_destroy(state, Some(socket), &[])?;
            return Ok(Value::Undefined);
        }
        if expected.is_some_and(|expected| expected != received) || (chunked && !chunked_done) {
            return abort_incomplete_response(state, client_id, &res);
        }
        set_response_property(&res, "complete", Value::Boolean(true));
        set_response_property(&res, "readable", Value::Boolean(false));
        if let Some(request) = client_value(state, client_id, true) {
            set_request_property(Some(&request), "destroyed", Value::Boolean(true));
        }
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
            set_response_property(&res, "destroyed", Value::Boolean(true));
            net::emit(state, &res, "close", Vec::new())?;
            let resource = execute::get_property(&request, CLIENT_ASYNC_RESOURCE_PROP);
            crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
        } else if let Some(request) = client_value(state, client_id, true) {
            set_request_property(Some(&request), "destroyed", Value::Boolean(true));
        }
        let agent_target = {
            let guard = state.borrow();
            guard.http.clientreqs.get(&client_id).and_then(|request| {
                request
                    .agent
                    .clone()
                    .map(|agent| (agent, request.target.clone()))
            })
        };
        if let Some((agent, target)) = agent_target {
            // The socket is observable to `free` listeners. Put it in the
            // Agent's free pool before emitting that event so a listener that
            // submits another request synchronously observes the same socket,
            // matching Node's reuse ordering.
            let keep_alive = state
                .borrow()
                .http
                .clientreqs
                .get(&client_id)
                .and_then(|request| {
                    request
                        .agent
                        .as_ref()
                        .filter(|agent| matches!(agent, Value::Object(_) | Value::ObjectAlias(_)))
                        .and_then(|_| request.res.as_ref())
                })
                .is_some_and(response_allows_reuse);
            let alive = net::net_id(socket).is_some_and(|id| {
                state
                    .borrow()
                    .net
                    .sockets
                    .get(&id)
                    .is_some_and(|value| value.borrow().state != net::SocketState::Closed)
            });
            if keep_alive && alive {
                let name = agent_name(&target, &agent);
                move_agent_socket_to_free(&agent, &name, socket);
                if matches!(execute::get_property(&agent, "timeout"), Value::Number(timeout) if timeout.is_finite() && timeout > 0.0) {
                    // Agent idle timeouts destroy the pooled socket after the
                    // timeout event. Install this host transition before
                    // exposing `free`, so user listeners observe
                    // `socket.destroyed === true` and cannot reuse it.
                    let destroy = crate::host::capability(crate::registry::SPEC_NET_SOCKET_DESTROY);
                    subscribe_event(state, socket, "timeout", destroy)?;
                }
            }
            net::emit(state, socket, "free", Vec::new())?;
            emit_agent_free(state, &agent, &target, socket)?;
        }
        let pooled = state
            .borrow()
            .http
            .clientreqs
            .get(&client_id)
            .is_some_and(|request| {
                request
                    .agent
                    .as_ref()
                    .is_some_and(|agent| matches!(agent, Value::Object(_) | Value::ObjectAlias(_)))
                    && request.res.as_ref().is_some_and(response_allows_reuse)
            });
        if pooled {
            // Idle HTTP agent sockets are retained for reuse but do not keep
            // the process alive. A later request refs the socket when reused.
            if let (Some(agent), Some(target)) = (
                state
                    .borrow()
                    .http
                    .clientreqs
                    .get(&client_id)
                    .and_then(|request| request.agent.clone()),
                state
                    .borrow()
                    .http
                    .clientreqs
                    .get(&client_id)
                    .map(|request| request.target.clone()),
            ) {
                let name = agent_name(&target, &agent);
                drain_agent_pending(state, &agent, &name);
            }
            net::socket_unref(state, Some(socket), &[])?;
        } else {
            net::socket_destroy(state, Some(socket), &[])?;
        }
    } else if let Some(error) = invalid_response_start(state, client_id) {
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
            req.parse_error = true;
        }
        let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
        net::emit(state, &request, "error", vec![error])?;
        // A parser error is terminal for this connection.  Let the normal
        // socket close transition release the request and server resources.
        net::socket_destroy(state, Some(socket), &[])?;
    }
    Ok(Value::Undefined)
}

fn invalid_response_start(state: &Rc<RefCell<HostState>>, client_id: u64) -> Option<Value> {
    let (parsed, raw) = state.borrow().http.clientreqs.get(&client_id).map(|req| {
        (req.head_parsed, req.buffer.clone())
    })?;
    if parsed || raw.is_empty() || raw.starts_with(b"HTTP/") {
        return None;
    }
    let error = invalid_response_constant();
    Some(execute::set_property(
        error,
        "rawPacket",
        crate::modules::buffer_proto::make_buffer(&raw),
    ))
}

fn invalid_response_constant() -> Value {
    let message = "Parse Error: Expected HTTP/, RTSP/ or ICE/";
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    execute::set_property(error, "code", Value::String("HPE_INVALID_CONSTANT".into()))
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
    if let Some(prototype) = state.borrow().http.client_request_prototype.clone() {
        object = execute::set_prototype_of(&object, &prototype)?;
    }
    let async_resource = crate::modules::async_hooks::new_resource(
        state,
        &[Value::String("HTTPCLIENTREQUEST".into())],
    )?;
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
                "setTimeout".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_TIMEOUT),
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
            ("timeout".to_string(), Value::Number(0.0)),
            (CLIENT_TIMEOUT_PROP.to_string(), Value::Undefined),
            (
                CLIENT_CLOSE_PENDING_PROP.to_string(),
                Value::Boolean(false),
            ),
            (CLIENT_ASYNC_RESOURCE_PROP.to_string(), async_resource),
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
    // The built-in Agent exposes `createConnection` on its prototype. That
    // method is the ordinary transport path, not a user-owned custom hook;
    // only an own property opts a request into the callback contract.
    let descriptor = execute::get_own_property_descriptor(agent, "createConnection").ok()?;
    let method = execute::get_property(&descriptor, "value");
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
    }
}

fn clear_request_timeout(state: &Rc<RefCell<HostState>>, id: u64) -> Result<(), VmError> {
    let timer = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .and_then(|req| req.timeout.clone());
    if let Some(timer) = timer {
        crate::modules::timers::clear_timeout(state, &[timer])?;
    }
    let request = {
        let mut guard = state.borrow_mut();
        let request = guard.http.clientreqs.get(&id).map(|req| req.req.clone());
        if let Some(req) = guard.http.clientreqs.get_mut(&id) {
            req.timeout = None;
        }
        request
    };
    if let Some(request) = request {
        set_request_property(Some(&request), CLIENT_TIMEOUT_PROP, Value::Undefined);
    }
    Ok(())
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
fn build_incoming(
    state: &Rc<RefCell<HostState>>,
    client_id: u64,
    head: &[u8],
) -> Result<Value, VmError> {
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
    let (request_resource, request_value) = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|request| {
            (
                execute::get_property(&request.req, CLIENT_ASYNC_RESOURCE_PROP),
                request.req.clone(),
            )
        })
        .unwrap_or((Value::Undefined, Value::Undefined));
    // Node exposes the originating ClientRequest as `res.req`. Keep this
    // relationship identity-preserving without retaining a request↔response
    // strong cycle: the host state owns the request, while the alias follows
    // that same object whenever JavaScript reads the property.
    let request_alias = match request_value {
        Value::Object(object) => Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(
            Rc::downgrade(&object),
        )))),
        other => other,
    };
    let props = vec![
        ("statusCode".to_string(), Value::Number(status as f64)),
        ("statusMessage".to_string(), Value::String(message)),
        ("httpVersion".to_string(), Value::String("1.1".to_string())),
        ("headers".to_string(), host_api::object(headers)),
        ("req".to_string(), request_alias),
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
        ("errored".to_string(), Value::Null),
        ("closed".to_string(), Value::Boolean(false)),
        (
            RES_ASYNC_RESOURCE_PROP.to_string(),
            request_resource,
        ),
        (
            crate::modules::http::INCOMING_CLOSE_PENDING_PROP.to_string(),
            Value::Boolean(false),
        ),
    ];
    install_methods(res, props)
}

fn request_options(value: Option<&Value>) -> Result<RequestOptions, VmError> {
    match value {
        Some(Value::String(url)) => http_url(url),
        Some(Value::Object(_) | Value::ObjectAlias(_)) => {
            let options = value.cloned().unwrap_or(Value::Undefined);
            let parser_option = execute::get_property(&options, "insecureHTTPParser");
            if !matches!(parser_option, Value::Undefined | Value::Null | Value::Boolean(_)) {
                return Err(invalid_boolean_option_error(
                    "options.insecureHTTPParser",
                    &parser_option,
                ));
            }
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

fn invalid_boolean_option_error(name: &str, value: &Value) -> VmError {
    let rendered = execute::to_js_string(value).unwrap_or_default();
    let error = execute::type_error(&format!(
        "The \"{name}\" property must be of type boolean. Received type string (\"{rendered}\")"
    ));
    match error {
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
