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
const AGENT_MARKER_PROP: &str = "\0quench:http:agent";
pub(crate) const CLIENT_ASYNC_RESOURCE_PROP: &str = "\0quench:http:req:async-resource";
pub(crate) const RES_ASYNC_RESOURCE_PROP: &str = "\0quench:http:res:async-resource";
const CLIENT_CLOSE_PENDING_PROP: &str = "\0quench:http:req:close-pending";
const CLIENT_EXPECT_CONTINUE_PROP: &str = "\0quench:http:req:expect-continue";
const CLIENT_TIMEOUT_PROP: &str = "\0quench:http:req:timeout";
const RESPONSE_ENCODING_PROP: &str = "\0quench:http:res:encoding";
const RESPONSE_READ_BUFFER_PROP: &str = "\0quench:http:res:read-buffer";
const CLIENT_SOCKET_SUBSCRIBED_PROP: &str = "\0quench:http:req:socket-subscribed";
const CLIENT_SOCKET_EVENT_QUEUED_PROP: &str = "\0quench:http:req:socket-event-queued";
const CLIENT_PATH_PROP: &str = "\0quench:http:req:path";

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
    secure: bool,
    tls_options: Option<Value>,
}

/// One outbound HTTP request, keyed by `CLIENT_ID_PROP`.
pub struct ClientReq {
    pub target: RequestTarget,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub secure: bool,
    pub tls_options: Option<Value>,
    pub tls_rejected: bool,
    pub body: Vec<u8>,
    /// Preserve individual `write()` boundaries for server-side data events.
    pub body_chunks: Vec<Vec<u8>>,
    pub body_started: bool,
    pub req: Value,
    pub agent: Option<Value>,
    pub lookup: Option<Value>,
    pub omit_host: bool,
    pub high_water_mark: Option<f64>,
    pub socket: Option<Value>,
    pub dispatched: bool,
    pub timeout: Option<Value>,
    pub timeout_set: bool,
    /// The first timeout supplied through request options. Node exposes this
    /// value on the socket event before a later req.setTimeout override.
    pub initial_timeout: Option<f64>,
    /// Explicit req.setTimeout value waiting for a connecting socket to open.
    pub pending_timeout: Option<f64>,
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Node's Agent constructor is callable without `new`; the call path must
    // preserve the same instance/prototype contract as construction.
    agent_construct(state, args)
}

pub fn https_agent_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    https_agent_construct(state, args)
}

pub fn https_agent_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let agent = agent_construct(state, args)?;
    let marked = execute::set_property(agent, "\0quench:https-agent", Value::Boolean(true));
    Ok(marked)
}

/// `agent.addRequest(request, options)` is an observable extension point.
/// Transport dispatch is owned by ClientRequest itself; an uninitialized
/// manually-seeded free socket must not be invoked by the host.
pub fn agent_add_request(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(agent) = receiver else {
        return Ok(Value::Undefined);
    };
    let request = args.first().cloned().unwrap_or(Value::Undefined);
    if let Some(id) = client_id(Some(&request)) {
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
            req.agent = Some(agent.clone());
        }
        set_request_property(Some(&request), "agent", agent.clone());
        let name = execute::call(
            &execute::get_property(&agent, "getName"),
            &agent,
            std::slice::from_ref(&request),
        )
        .ok()
        .and_then(|value| execute::to_js_string(&value).ok());
        if let Some(name) = name {
            let free = execute::get_property(&execute::get_property(&agent, "freeSockets"), &name);
            let has_free = matches!(
                execute::get_property(&free, "length"),
                Value::Number(value) if value.is_finite() && value > 0.0
            );
            if has_free {
                set_request_property(Some(&request), "reusedSocket", Value::Boolean(true));
            }
        }
    }
    let (host, port, local) = match args.get(1) {
        Some(Value::Object(_) | Value::ObjectAlias(_)) => (
            execute::to_js_string(&execute::get_property(args.get(1).unwrap(), "host"))
                .unwrap_or_else(|_| "localhost".into()),
            execute::to_js_string(&execute::get_property(args.get(1).unwrap(), "port"))
                .unwrap_or_default(),
            execute::to_js_string(&execute::get_property(args.get(1).unwrap(), "localAddress"))
                .unwrap_or_default(),
        ),
        Some(host) => (
            execute::to_js_string(host).unwrap_or_else(|_| "localhost".into()),
            args.get(2)
                .map(execute::to_js_string)
                .transpose()?
                .unwrap_or_default(),
            args.get(3)
                .map(execute::to_js_string)
                .transpose()?
                .unwrap_or_default(),
        ),
        None => ("localhost".into(), String::new(), String::new()),
    };
    let key = format!("{host}:{port}:{local}");
    let requests = execute::get_property(agent, "requests");
    let current = execute::get_property(&requests, &key);
    let updated = if matches!(current, Value::Array(_)) {
        let length = execute::get_property(&current, "length");
        let index = match length {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
            _ => 0,
        };
        let _ = execute::set_property_in_place(&current, &index.to_string(), request);
        current
    } else {
        host_api::array(vec![request])
    };
    execute::set_property_in_place(&requests, &key, updated);
    Ok(Value::Undefined)
}

/// `agent.keepSocketAlive(socket)` applies the default idle-socket policy.
/// Subclasses may override it; the pool invokes the receiver's method so
/// custom timeout policies remain observable.
pub fn agent_keep_socket_alive(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let agent = receiver.cloned().unwrap_or(Value::Undefined);
    let socket = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(socket, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(Value::Boolean(false));
    }
    let keep_alive_msecs = execute::get_property(&agent, "keepAliveMsecs");
    net::socket_set_keep_alive(
        state,
        Some(&socket),
        &[Value::Boolean(true), keep_alive_msecs],
    )?;
    let timeout = execute::get_property(&agent, "timeout");
    if matches!(timeout, Value::Number(value) if value.is_finite() && value >= 0.0) {
        execute::set_property_in_place(&socket, "timeout", timeout);
    }
    net::socket_unref(state, Some(&socket), &[])?;
    Ok(Value::Boolean(true))
}

pub fn agent_construct(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let options = args
        .first()
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    let option = |name: &str| execute::get_property(&options, name);
    let max_total_sockets = validate_agent_limit("maxTotalSockets", option("maxTotalSockets"))?;
    let mut object = crate::modules::events::new_emitter_object(state)?;
    if let Some(prototype) = state.borrow().http.agent_prototype.clone() {
        // Agent mechanics are native capabilities on the shared prototype;
        // do not import closure-backed methods from the legacy JS facade.
        object = execute::set_prototype_of(&object, &prototype)?;
    }
    object = execute::set_property(object, "sockets", host_api::object(Vec::new()));
    object = execute::set_property(object, "freeSockets", host_api::object(Vec::new()));
    object = execute::set_property(object, "requests", host_api::object(Vec::new()));
    object = execute::set_property(object, AGENT_MARKER_PROP, Value::Boolean(true));
    object = execute::set_property(object, "options", options.clone());
    object = execute::set_property(
        object,
        "keepAlive",
        match option("keepAlive") {
            Value::Boolean(value) => Value::Boolean(value),
            _ => Value::Boolean(false),
        },
    );
    object = execute::set_property(
        object,
        "keepAliveMsecs",
        match option("keepAliveMsecs") {
            Value::Number(value) => Value::Number(value),
            _ => Value::Number(1000.0),
        },
    );
    object = execute::set_property(
        object,
        "maxSockets",
        match option("maxSockets") {
            Value::Number(value) => Value::Number(value),
            _ => Value::Number(f64::INFINITY),
        },
    );
    object = execute::set_property(object, "maxTotalSockets", Value::Number(max_total_sockets));
    object = execute::set_property(
        object,
        "scheduling",
        match option("scheduling") {
            Value::String(value) if value == "fifo" || value == "lifo" => Value::String(value),
            Value::String(value) => {
                return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                    "The argument 'scheduling' must be one of: 'fifo', 'lifo'. Received '{value}'"
                )));
            }
            Value::Null | Value::Undefined => Value::String("lifo".into()),
            value => {
                return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                    "The argument 'scheduling' must be one of: 'fifo', 'lifo'. Received {}",
                    execute::to_js_string(&value).unwrap_or_else(|_| "undefined".into())
                )));
            }
        },
    );
    object = execute::set_property(object, "defaultPort", Value::Number(80.0));
    object = execute::set_property(object, "protocol", Value::String("http:".into()));
    object = execute::set_property(
        object,
        "timeout",
        match option("timeout") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => Value::Number(value),
            _ => Value::Number(0.0),
        },
    );
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

fn validate_agent_limit(name: &str, value: Value) -> Result<f64, VmError> {
    match value {
        Value::Undefined => Ok(f64::INFINITY),
        Value::Number(value) if value.is_infinite() && value.is_sign_positive() => Ok(value),
        Value::Number(value) if value.is_finite() && value > 0.0 => Ok(value),
        Value::Number(value) => Err(crate::modules::buffer_enc::out_of_range(
            name,
            "a positive number",
            &execute::number_to_js_string(value),
        )),
        other => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(&other)
        ))),
    }
}

/// `agent.getName(options)` — stable pool key derived from connection facts.
fn agent_name_part(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) if value.is_finite() => execute::number_to_js_string(*value),
        Value::Boolean(value) => value.to_string(),
        Value::Uint8Array(view) => {
            let bytes = view.buffer.bytes.borrow();
            let end = view
                .byte_offset
                .saturating_add(view.byte_length())
                .min(bytes.len());
            String::from_utf8_lossy(&bytes[view.byte_offset.min(end)..end]).into_owned()
        }
        Value::Array(values) => (0..values.logical_len())
            .map(|index| agent_name_part(&values.index_value(index)))
            .collect::<Vec<_>>()
            .join(","),
        Value::Undefined | Value::Null => String::new(),
        other => format!("{other:?}"),
    }
}

pub fn agent_get_name(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    let host = if own_option(&options, "host") {
        match execute::get_property(&options, "host") {
            Value::String(value) if !value.is_empty() => value,
            _ => "localhost".into(),
        }
    } else if own_option(&options, "hostname") {
        match execute::get_property(&options, "hostname") {
            Value::String(value) if !value.is_empty() => value,
            _ => "localhost".into(),
        }
    } else {
        "localhost".into()
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
    if !matches!(
        receiver.map(|value| execute::get_property(value, "\0quench:https-agent")),
        Some(Value::Boolean(true))
    ) {
        return Ok(Value::String(format!(
            "{host}:{port}:{local}{family}{socket_path}"
        )));
    }
    let part = |name: &str| match execute::get_property(&options, name) {
        value => agent_name_part(&value),
    };
    let fields = [
        host,
        port,
        local,
        part("ca"),
        part("cert"),
        part("clientCertEngine"),
        part("ciphers"),
        part("key"),
        part("pfx"),
        part("rejectUnauthorized"),
        part("servername"),
        String::new(),
        String::new(),
        part("secureProtocol"),
        part("crl"),
        part("honorCipherOrder"),
        part("ecdhCurve"),
        part("dhparam"),
        part("secureOptions"),
        part("sessionIdContext"),
        {
            let sigalgs = part("sigalgs");
            if sigalgs.is_empty() {
                sigalgs
            } else {
                format!("\"{sigalgs}\"")
            }
        },
        part("privateKeyIdentifier"),
        part("privateKeyEngine"),
    ];
    Ok(Value::String(fields.join(":")))
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

pub(crate) fn agent_keylog_attach(
    state: &Rc<RefCell<HostState>>,
    agent: &Value,
    listener: &Value,
) -> Result<(), VmError> {
    let sockets = ["freeSockets", "sockets"]
        .into_iter()
        .flat_map(|pool_name| {
            let pools = execute::get_property(agent, pool_name);
            execute::own_enumerable_keys(&pools)
                .into_iter()
                .flat_map(move |name| {
                    let list = execute::get_property(&pools, &name);
                    execute::own_enumerable_keys(&list)
                        .into_iter()
                        .map(move |index| execute::get_property(&list, &index))
                })
        })
        .filter(|socket| matches!(socket, Value::Object(_) | Value::ObjectAlias(_)))
        .collect::<Vec<_>>();
    for socket in sockets {
        crate::modules::events::method_on(
            state,
            Some(&socket),
            &[Value::String("keylog".into()), listener.clone()],
        )?;
    }
    Ok(())
}

/// Mark every observable Agent-pool view of a socket as destroyed. Runtime
/// values can cross the VM boundary as aliases, so updating only the net
/// record is insufficient for a freeSockets entry held by user code.
pub(crate) fn mark_socket_destroyed_in_agents(state: &Rc<RefCell<HostState>>, socket: &Value) {
    let agents = state
        .borrow()
        .http
        .clientreqs
        .values()
        .filter_map(|request| request.agent.clone())
        .collect::<Vec<_>>();
    for agent in agents {
        for pool_name in ["sockets", "freeSockets"] {
            let pools = execute::get_property(&agent, pool_name);
            for name in execute::own_enumerable_keys(&pools) {
                let list = execute::get_property(&pools, &name);
                for index in execute::own_enumerable_keys(&list) {
                    let entry = execute::get_property(&list, &index);
                    if same_socket(&entry, socket) {
                        execute::set_property_in_place(&entry, "destroyed", Value::Boolean(true));
                    }
                }
            }
        }
    }
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

/// Consume one body chunk for the IncomingMessage `read()` contract. The
/// parser queues the same value it emits as `data`, so readable listeners see
/// one identity-preserving stream of chunks regardless of consumption mode.
pub fn res_read(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(response) = receiver else {
        return Ok(Value::Null);
    };
    let queue = execute::get_property(response, RESPONSE_READ_BUFFER_PROP);
    let Value::Array(array) = queue else {
        return Ok(Value::Null);
    };
    if array.logical_len() == 0 {
        return Ok(Value::Null);
    }
    let value = array.index_value(0);
    let remaining = (1..array.logical_len())
        .map(|index| array.index_value(index))
        .collect();
    set_response_property(
        response,
        RESPONSE_READ_BUFFER_PROP,
        host_api::array(remaining),
    );
    Ok(value)
}

pub fn res_pipe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let response = receiver.cloned().unwrap_or(Value::Undefined);
    let destination = args.first().cloned().unwrap_or(Value::Undefined);
    let data = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_HTTP_RES_PIPE_DATA),
        vec![destination.clone()],
    );
    crate::modules::events::method_on(
        state,
        Some(&response),
        &[Value::String("data".into()), data],
    )?;
    let end = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_HTTP_RES_PIPE_END),
        vec![destination.clone()],
    );
    crate::modules::events::method_once(
        state,
        Some(&response),
        &[Value::String("end".into()), end],
    )?;
    net::emit(state, &destination, "pipe", vec![response])?;
    Ok(destination)
}

pub fn res_pipe_data(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let destination = args.first().cloned().unwrap_or(Value::Undefined);
    let write = execute::get_property(&destination, "write");
    if quench_runtime::is_callable(&write) {
        execute::call(
            &write,
            &destination,
            &[args.get(1).cloned().unwrap_or(Value::Undefined)],
        )?;
    }
    Ok(Value::Undefined)
}

pub fn res_pipe_end(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let destination = args.first().cloned().unwrap_or(Value::Undefined);
    let end = execute::get_property(&destination, "end");
    if quench_runtime::is_callable(&end) {
        execute::call(&end, &destination, &[])?;
    }
    Ok(Value::Undefined)
}

/// `IncomingMessage.setTimeout(msecs[, callback])` delegates the timer to the
/// response's socket and relays its timeout transition back to the response.
pub fn res_set_timeout(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(response) = receiver else {
        return Ok(Value::Undefined);
    };
    let timeout = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value,
        Some(Value::Number(_)) => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The \"msecs\" value is out of range".into(),
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
        crate::modules::events::method_once(
            state,
            Some(response),
            &[Value::String("timeout".into()), callback.clone()],
        )?;
    }
    let socket = execute::get_property(response, "socket");
    if matches!(socket, Value::Object(_) | Value::ObjectAlias(_)) {
        let emit = execute::get_property(response, "emit");
        let bind = execute::get_property_result(&emit, "bind")?;
        let relay = execute::call(
            &bind,
            &emit,
            &[response.clone(), Value::String("timeout".into())],
        )?;
        net::socket_set_timeout(state, Some(&socket), &[Value::Number(timeout), relay])?;
    }
    Ok(response.clone())
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

fn queue_response_data(response: &Value, data: Value) {
    let queue = execute::get_property(response, RESPONSE_READ_BUFFER_PROP);
    let values = match queue {
        Value::Array(array) => {
            let mut values = (0..array.logical_len())
                .map(|index| array.index_value(index))
                .collect::<Vec<_>>();
            values.push(data);
            values
        }
        _ => vec![data],
    };
    set_response_property(response, RESPONSE_READ_BUFFER_PROP, host_api::array(values));
}

/// `http.request(options[, cb])` — an outbound ClientRequest.
pub fn request(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    request_inner(state, args, false)
}

pub fn https_request(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    request_inner(state, args, true)
}

fn request_inner(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    secure_hint: bool,
) -> Result<Value, VmError> {
    let opts = request_options(args.first())?;
    let secure = secure_hint || opts.secure;
    let tls_options = if secure {
        option_source_object(args).or_else(|| opts.tls_options.clone()).map(|options| {
            // https.Agent carries TLS defaults in its own `options` object.
            // Requests inherit those defaults even when the per-request
            // options omit `rejectUnauthorized`.
            if matches!(execute::get_property(&options, "rejectUnauthorized"), Value::Undefined)
                && matches!(execute::get_property(&options, "agent"), Value::Object(_) | Value::ObjectAlias(_))
            {
                let agent = execute::get_property(&options, "agent");
                let agent_options = execute::get_property(&agent, "options");
                if !matches!(execute::get_property(&agent_options, "rejectUnauthorized"), Value::Undefined) {
                    return execute::set_property(
                        options,
                        "rejectUnauthorized",
                        execute::get_property(&agent_options, "rejectUnauthorized"),
                    );
                }
            }
            options
        })
    } else {
        None
    };
    let tls_rejected = secure
        && tls_options.as_ref().is_none_or(|options| {
            !matches!(
                execute::get_property(options, "ca"),
                Value::String(_) | Value::StringUnits(_) | Value::Array(_) | Value::Uint8Array(_)
            ) && !matches!(
                execute::get_property(options, "rejectUnauthorized"),
                Value::Boolean(false)
            )
        })
        && !tls_verification_disabled();
    let option_source = match args.first() {
        Some(Value::String(_) | Value::StringUnits(_)) => args.get(1),
        _ => args.first(),
    };
    let callback = match args.first() {
        Some(Value::String(_) | Value::StringUnits(_)) => args
            .iter()
            .skip(1)
            .find(|value| quench_runtime::is_callable(value)),
        _ => args.get(1),
    };
    let high_water_mark = option_source
        .and_then(|options| {
            matches!(options, Value::Object(_) | Value::ObjectAlias(_)).then(|| {
                match execute::get_property(options, "highWaterMark") {
                    Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value),
                    _ => None,
                }
            })
        })
        .flatten();
    let signal = option_source
        .and_then(|options| {
            matches!(options, Value::Object(_) | Value::ObjectAlias(_))
                .then(|| execute::get_property(options, "signal"))
        })
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    let agent = option_source.and_then(|options| {
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
        .or_else(|| state.borrow().http.global_agent.clone())
        .map(|value| execute::canonical_value(&value));
    let lookup = option_source.and_then(|options| {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return None;
        }
        let lookup = execute::get_property(options, "lookup");
        quench_runtime::is_callable(&lookup).then_some(lookup)
    });
    let omit_host = option_source.is_some_and(|options| {
        matches!(execute::get_property(options, "headers"), Value::Array(_))
    });
    let expect_continue = opts.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("expect") && value.eq_ignore_ascii_case("100-continue")
    });
    let (req, id) = build_req_object(state)?;
    req_path_set(state, Some(&req), &[Value::String(opts.path.clone())])?;
    set_request_property(Some(&req), "method", Value::String(opts.method.clone()));
    set_request_property(
        Some(&req),
        "host",
        Value::String(target_host(&opts.target)),
    );
    set_request_property(
        Some(&req),
        "port",
        Value::Number(target_port(&opts.target) as f64),
    );
    let mut guard = state.borrow_mut();
    guard.http.clientreqs.insert(
        id,
        ClientReq {
            target: opts.target,
            method: opts.method,
            path: opts.path,
            headers: opts.headers,
            secure,
            tls_options,
            tls_rejected,
            body: Vec::new(),
            body_chunks: Vec::new(),
            body_started: false,
            req: req.clone(),
            agent,
            lookup,
            omit_host,
            high_water_mark,
            socket: None,
            dispatched: false,
            timeout: None,
            timeout_set: false,
            initial_timeout: None,
            pending_timeout: None,
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
            if matches!(
                execute::get_property(&signal, "aborted"),
                Value::Boolean(true)
            ) {
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
    if let Some(cb) = callback {
        if quench_runtime::is_callable(cb) {
            crate::modules::events::method_on(
                state,
                Some(&req),
                &[Value::String("response".to_string()), cb.clone()],
            )?;
        }
    }
    if let Some(options) =
        option_source.filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
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
        set_request_property(
            Some(&req),
            CLIENT_EXPECT_CONTINUE_PROP,
            Value::Boolean(true),
        );
        let _ = req_end(state, Some(&req), &[])?;
    } else {
        start_request_socket(state, &req)?;
    }
    Ok(req)
}

/// Node reserves a ClientRequest's socket as soon as the request is created.
/// Keep the request body deferred until `end()` while exposing the same
/// socket identity through the ordinary pending `"socket"` transition.
fn start_request_socket(state: &Rc<RefCell<HostState>>, request: &Value) -> Result<(), VmError> {
    let Some(id) = client_id(Some(request)) else {
        return Ok(());
    };
    let (
        target,
        agent,
        lookup,
        high_water_mark,
        initial_timeout,
        aborted,
        existing,
        secure,
        tls_options,
        tls_rejected,
    ) = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&id) else {
            return Ok(());
        };
        (
            req.target.clone(),
            req.agent.clone(),
            req.lookup.clone(),
            req.high_water_mark,
            req.initial_timeout,
            req.aborted,
            req.socket.clone(),
            req.secure,
            req.tls_options.clone(),
            req.tls_rejected,
        )
    };
    if aborted || existing.is_some() || lookup.is_some() {
        return Ok(());
    }
    // A user supplied connector owns its own construction and callback
    // contract; it is started by `end()` where the full request options are
    // available. The ordinary net path can reserve its socket immediately.
    if agent.as_ref().is_some_and(|agent| {
        custom_connection(state, agent).is_some() || custom_socket(state, agent).is_some()
    }) {
        return Ok(());
    }
    let pooled = agent
        .as_ref()
        .and_then(|agent| take_agent_socket(agent, &agent_name(&target, agent)));
    let blocked = pooled.is_none()
        && agent
            .as_ref()
            .is_some_and(|agent| !agent_socket_capacity_available(agent, &target));
    if blocked {
        return Ok(());
    }
    let socket = if let Some(socket) = pooled {
        crate::modules::http::clear_idle_socket(state, &socket);
        net::socket_ref(state, Some(&socket), &[])?;
        net::socket_set_timeout(state, Some(&socket), &[Value::Number(0.0)])?;
        // Timeout listeners are request-scoped.  Pool checkout happens
        // before `req.end()`, so clear the prior request's listeners here;
        // the existing-socket path below cannot otherwise distinguish this
        // reuse from a socket supplied by a custom connector.
        crate::modules::events::method_remove_all_listeners(
            state,
            Some(&socket),
            &[Value::String("timeout".into())],
        )?;
        if let Some(value) = high_water_mark {
            execute::set_property_in_place(&socket, "writableHighWaterMark", Value::Number(value));
        }
        if let Some(value) = initial_timeout {
            execute::set_property_in_place(&socket, "timeout", Value::Number(value));
        }
        socket
    } else {
        let socket = open_socket(state, &target)?;
        if let Some(value) = initial_timeout {
            execute::set_property_in_place(&socket, "timeout", Value::Number(value));
        }
        socket
    };
    if secure {
        crate::modules::tls::decorate_socket(&socket, tls_options.as_ref());
        if tls_rejected {
            crate::modules::tls::mark_rejected(&socket);
        }
    }
    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
        req.socket = Some(socket.clone());
        set_request_property(Some(&req.req), "socket", socket.clone());
        set_request_property(Some(&socket), "_httpMessage", req.req.clone());
        if let Some(agent) = req.agent.clone() {
            let name = agent_name(&target, &agent);
            add_agent_socket(&agent, &name, &socket);
        }
    }
    if secure {
        crate::modules::tls::decorate_socket(&socket, tls_options.as_ref());
        if tls_rejected {
            crate::modules::tls::mark_rejected(&socket);
        }
        if let Some(req) = state.borrow().http.clientreqs.get(&id) {
            set_request_property(Some(&req.req), "socket", socket.clone());
        }
    }
    if let Some(socket_id) = net::net_id(&socket) {
        state.borrow_mut().http.clients.insert(socket_id, id);
    }
    subscribe_socket(state, &socket)?;
    set_request_property(
        Some(request),
        CLIENT_SOCKET_EVENT_QUEUED_PROP,
        Value::Boolean(true),
    );
    state
        .borrow_mut()
        .net
        .pending_events
        .push((request.clone(), "socket".into(), vec![socket]));
    if tls_rejected {
        let rejected_socket = state
            .borrow()
            .http
            .clientreqs
            .get(&id)
            .and_then(|req| req.socket.clone());
        if let Some(socket) = rejected_socket {
            state.borrow_mut().net.pending_events.push((
                socket,
                "error".into(),
                vec![tls_verification_error()],
            ));
        }
    }
    Ok(())
}

fn agent_socket_capacity_available(agent: &Value, target: &RequestTarget) -> bool {
    let name = agent_name(target, agent);
    let sockets = execute::get_property(agent, "sockets");
    let active = match execute::get_property(&sockets, &name) {
        Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_) => {
            execute::get_property(&sockets, &name)
        }
        _ => return true,
    };
    let active_count = execute::get_property(&active, "length");
    let active_count = match active_count {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let max = match execute::get_property(agent, "maxSockets") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => usize::MAX,
    };
    if active_count >= max {
        return false;
    }
    let total = execute::own_enumerable_keys(&sockets)
        .into_iter()
        .map(|key| execute::get_property(&sockets, &key))
        .map(|list| match execute::get_property(&list, "length") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
            _ => 0,
        })
        .sum::<usize>();
    let max_total = match execute::get_property(agent, "maxTotalSockets") {
        Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
        _ => usize::MAX,
    };
    total < max_total
}

fn preabort_request(state: &Rc<RefCell<HostState>>, request: &Value) {
    if let Some(id) = client_id(Some(request)) {
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
            req.aborted = true;
            req.response_closed = true;
        }
    }
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
    let request = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .map(|req| req.req.clone());
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
        if req.aborted
            || matches!(
                execute::get_property(&req.req, "destroyed"),
                Value::Boolean(true)
            )
        {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        }
        req.aborted = true;
        (req.req.clone(), req.socket.clone(), req.target.clone())
    };
    clear_request_timeout(state, id)?;
    state
        .borrow_mut()
        .net
        .pending_request_writes
        .retain(|(_, _, pending)| !execute::same_identity(pending, &request));
    // A request can be aborted before the event-loop announces its socket.
    // The socket event belongs to the request's connection transition, so a
    // terminal abort removes that deferred transition rather than exposing a
    // socket after the request has become destroyed.
    state
        .borrow_mut()
        .net
        .pending_events
        .retain(|(pending, event, _)| {
            !(execute::same_identity(pending, &request) && event == "socket")
        });
    set_request_property(receiver, "aborted", Value::Boolean(true));
    set_request_property(receiver, "destroyed", Value::Boolean(true));
    net::emit(state, &request, "abort", Vec::new())?;
    if let Some(socket) = socket {
        let connected = net::net_id(&socket).is_some_and(|socket_id| {
            state
                .borrow()
                .net
                .sockets
                .get(&socket_id)
                .is_some_and(|entry| entry.borrow().stream.is_some())
        });
        if connected {
            net::socket_destroy(state, Some(&socket), &[])?;
        }
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
        // Preserve an explicit destroy error as the request's terminal
        // failure. The socket close path must not replace it with its
        // synthetic ECONNRESET.
        set_request_property(receiver, "errored", error.clone());
        state
            .borrow_mut()
            .net
            .pending_events
            .push((request.clone(), "error".into(), vec![error]));
    } else if response.is_none() && socket.is_some() {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("socket hang up".into())],
        );
        let error = execute::set_property(error, "code", Value::String("ECONNRESET".into()));
        let error_ctor =
            execute::get_property(&quench_runtime::vm::current_global_object(), "Error");
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

pub fn https_get(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let req = https_request(state, args)?;
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
    let (open, agent, dispatched) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        req.body.extend_from_slice(&bytes);
        req.body_chunks.push(bytes.clone());
        req.body_started = true;
        (req.socket.is_none(), req.agent.clone(), req.dispatched)
    };
    let custom = agent
        .as_ref()
        .and_then(|agent| custom_connection(state, agent))
        .is_some();
    if open && custom && !dispatched {
        // A write starts an HTTP request even when the caller delays `end`.
        // Reuse the ordinary dispatch path so Agent connection hooks, socket
        // bookkeeping, and async resource identity stay identical to `end`.
        let request = receiver.cloned().unwrap_or(Value::Undefined);
        req_end(state, Some(&request), &[])?;
    } else if open {
        let target = state
            .borrow()
            .http
            .clientreqs
            .get(&id)
            .map(|req| req.target.clone());
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
        .or_else(|| {
            args.get(2)
                .filter(|value| quench_runtime::is_callable(value))
        });
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

fn req_header_name(args: &[Value]) -> Result<String, VmError> {
    args.first()
        .map(execute::to_js_string)
        .transpose()?
        .map(|name| name.to_ascii_lowercase())
        .ok_or_else(|| crate::modules::buffer_enc::invalid_arg_type(
            "The \"name\" argument must be of type string".into(),
        ))
}

fn req_header_values(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Vec<(String, String)> {
    let Some(id) = client_id(receiver) else {
        return Vec::new();
    };
    state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .map(|request| request.headers.clone())
        .unwrap_or_default()
}

pub fn req_get_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = req_header_name(args)?;
    let values = req_header_values(state, receiver)
        .into_iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(&name))
        .map(|(_, value)| Value::String(value))
        .collect::<Vec<_>>();
    Ok(match values.as_slice() {
        [] => Value::Undefined,
        [value] => value.clone(),
        _ => host_api::array(values),
    })
}

pub fn req_has_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = req_header_name(args)?;
    Ok(Value::Boolean(
        req_header_values(state, receiver)
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case(&name)),
    ))
}

pub fn req_get_header_names(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let mut names = Vec::<String>::new();
    for (key, _) in req_header_values(state, receiver) {
        let key = key.to_ascii_lowercase();
        if !names.iter().any(|current| current == &key) {
            names.push(key);
        }
    }
    if !names.iter().any(|name| name == "connection") {
        names.push("connection".into());
    }
    Ok(host_api::array(names.into_iter().map(Value::String).collect()))
}

pub fn req_get_headers(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let mut headers = Vec::new();
    for (key, value) in req_header_values(state, receiver) {
        let key = key.to_ascii_lowercase();
        let entry = headers.iter_mut().find(|(name, _)| name == &key);
        if let Some((_, current)) = entry {
            *current = match &*current {
                Value::Array(array) => {
                    let mut values = (0..array.logical_len())
                        .map(|index| array.index_value(index))
                        .collect::<Vec<_>>();
                    values.push(Value::String(value));
                    host_api::array(values)
                }
                previous => host_api::array(vec![previous.clone(), Value::String(value)]),
            };
        } else {
            headers.push((key, Value::String(value)));
        }
    }
    if !headers.iter().any(|(name, _)| name == "connection") {
        headers.push(("connection".into(), Value::String("keep-alive".into())));
    }
    Ok(host_api::object(headers))
}

pub fn req_remove_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = req_header_name(args)?;
    if let Some(id) = client_id(receiver) {
        if let Some(request) = state.borrow_mut().http.clientreqs.get_mut(&id) {
            request.headers.retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
        }
    }
    Ok(Value::Undefined)
}

fn req_socket(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Option<Value> {
    let id = client_id(receiver)?;
    state.borrow().http.clientreqs.get(&id)?.socket.clone()
}

pub fn req_set_no_delay(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(socket) = req_socket(state, receiver) {
        let _ = net::socket_set_no_delay(state, Some(&socket), args)?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn req_set_keep_alive(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(socket) = req_socket(state, receiver) {
        let _ = net::socket_set_keep_alive(state, Some(&socket), args)?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn req_set_socket_timeout(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(socket) = req_socket(state, receiver) {
        let _ = net::socket_set_timeout(state, Some(&socket), args)?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn req_cork(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let depth = match execute::get_property(receiver, "writableCorked") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as u64,
        _ => 0,
    } + 1;
    execute::set_property_in_place(receiver, "writableCorked", Value::Number(depth as f64));
    Ok(receiver.clone())
}

pub fn req_uncork(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let depth = match execute::get_property(receiver, "writableCorked") {
        Value::Number(value) if value.is_finite() && value > 0.0 => value as u64 - 1,
        _ => 0,
    };
    execute::set_property_in_place(receiver, "writableCorked", Value::Number(depth as f64));
    Ok(receiver.clone())
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
    let had_timeout = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .is_some_and(|req| req.timeout_set);
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
        if !had_timeout && req.socket.is_none() && timeout > 0.0 {
            req.initial_timeout = Some(timeout);
        }
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
    // `http.request({ timeout })` is applied while the request is still
    // acquiring its socket.  Keep the fact on ClientReq and let `req.end()`
    // install the single socket timer; starting a request timer here would
    // race that transition and emit `timeout` twice.
    let defer_until_socket = socket.is_none()
        && state
            .borrow()
            .http
            .clientreqs
            .get(&id)
            .is_some_and(|req| req.initial_timeout.is_some());
    if let Some(socket) = socket {
        if timeout > 0.0 {
            // Node keeps the Agent/request timeout installed while a socket
            // is connecting. An explicit req.setTimeout() supersedes that
            // timer at the connect transition, so the socket event still
            // observes the Agent timeout and the connect event observes the
            // request timeout. Bind the ordinary net capability rather than
            // inventing another transport path.
            let connecting = net::net_id(&socket).is_some_and(|socket_id| {
                state
                    .borrow()
                    .net
                    .sockets
                    .get(&socket_id)
                    .is_some_and(|entry| !entry.borrow().connect_announced)
            });
            if connecting {
                if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&id) {
                    req.pending_timeout = Some(timeout);
                }
                // Replace the option-installed timeout listeners while the
                // socket remains in its connecting state. The one request
                // listener is retained; the connect transition below only
                // changes the timer value, so listener identity/order stays
                // observable and stable.
                crate::modules::events::method_remove_all_listeners(
                    state,
                    Some(&socket),
                    &[Value::String("timeout".into())],
                )?;
                subscribe_event(state, &socket, "timeout", timeout_cb)?;
            } else {
                net::socket_set_timeout(
                    state,
                    Some(&socket),
                    &[Value::Number(timeout), timeout_cb],
                )?;
            }
        } else {
            subscribe_event(state, &socket, "timeout", timeout_cb)?;
            net::socket_set_timeout(state, Some(&socket), &[Value::Number(timeout)])?;
        }
    } else if timeout > 0.0 && defer_until_socket {
        // `start_request_socket()` will attach the socket before the request
        // is observable by user code; `req.end()` owns timer installation.
    } else if timeout > 0.0 {
        let timer =
            crate::modules::timers::set_timeout(state, &[timeout_cb, Value::Number(timeout)])?;
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
        !req.aborted
            && !matches!(
                execute::get_property(&req.req, "destroyed"),
                Value::Boolean(true)
            )
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
        matches!(
            execute::get_property(&req.req, CLIENT_EXPECT_CONTINUE_PROP),
            Value::Boolean(true)
        ) && req.socket.is_some()
    });
    let already_dispatched = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .is_some_and(|req| req.dispatched);
    if already_dispatched && !expect_started {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    if expect_started {
        if args
            .first()
            .is_some_and(|data| !matches!(data, Value::Undefined))
        {
            req_write(state, receiver, end_chunk_args(args))?;
        }
        let (socket, body_chunks, body) = {
            let mut guard = state.borrow_mut();
            let Some(req) = guard.http.clientreqs.get_mut(&id) else {
                return Ok(receiver.cloned().unwrap_or(Value::Undefined));
            };
            (
                req.socket.clone(),
                std::mem::take(&mut req.body_chunks),
                std::mem::take(&mut req.body),
            )
        };
        if let Some(socket) = socket {
            let mut payload = Vec::new();
            for chunk in body_chunks {
                payload.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
                payload.extend_from_slice(&chunk);
                payload.extend_from_slice(b"\r\n");
                net::socket_write(state, Some(&socket), &[host_api::bytes(&payload)])?;
                payload.clear();
            }
            if payload.is_empty() && body.is_empty() {
                payload.extend_from_slice(b"0\r\n\r\n");
                net::socket_write(state, Some(&socket), &[host_api::bytes(&payload)])?;
            } else {
                net::socket_write(state, Some(&socket), &[host_api::bytes(b"0\r\n\r\n")])?;
            }
        }
        if let Some(request) = receiver {
            set_request_property(Some(request), "finished", Value::Boolean(true));
            set_request_property(
                Some(request),
                CLIENT_EXPECT_CONTINUE_PROP,
                Value::Boolean(false),
            );
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
        match current_agent.as_ref() {
            None => false,
            Some(agent) => {
                let max = match execute::get_property(agent, "maxSockets") {
                    Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
                    _ => usize::MAX,
                };
                let max_total = match execute::get_property(agent, "maxTotalSockets") {
                    Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
                    _ => usize::MAX,
                };
                if max == usize::MAX && max_total == usize::MAX {
                    false
                } else {
                    let active_for_name = guard
                        .http
                        .clientreqs
                        .values()
                        .filter(|request| {
                            request.dispatched
                                && !request.response_closed
                                && request.agent.as_ref().is_some_and(|candidate| {
                                    execute::same_identity(candidate, agent)
                                        && agent_name(&request.target, agent)
                                            == agent_name(&current_target, agent)
                                })
                        })
                        .count();
                    let active_total = guard
                        .http
                        .clientreqs
                        .values()
                        .filter(|request| {
                            request.dispatched
                                && !request.response_closed
                                && request.agent.as_ref().is_some_and(|candidate| {
                                    execute::same_identity(candidate, agent)
                                })
                        })
                        .count();
                    if active_for_name >= max || active_total >= max_total {
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
    // `request()` may reserve a socket before `get()` performs its implicit
    // `end()`. Re-check the physical Agent pool here as well: this boundary
    // closes the race where another request filled `maxTotalSockets` between
    // those two calls, and keeps the queue fact aligned with `sockets`.
    let capacity_blocked = {
        let (agent, target, has_socket) = {
            let guard = state.borrow();
            guard
                .http
                .clientreqs
                .get(&id)
                .map(|request| {
                    (
                        request.agent.clone(),
                        Some(request.target.clone()),
                        request.socket.is_some(),
                    )
                })
                .unwrap_or((None, None, true))
        };
        agent
            .zip(target)
            .filter(|(_, _)| !has_socket)
            .is_some_and(|(agent, target)| !agent_socket_capacity_available(&agent, &target))
    };
    if capacity_blocked {
        let (agent, target, request) = {
            let guard = state.borrow();
            guard
                .http
                .clientreqs
                .get(&id)
                .map(|current| {
                    (
                        current.agent.clone(),
                        current.target.clone(),
                        current.req.clone(),
                    )
                })
                .unwrap_or((None, RequestTarget::Tcp { host: String::new(), port: 0 }, Value::Undefined))
        };
        if let Some(agent) = agent {
            let name = agent_name(&target, &agent);
            let mut guard = state.borrow_mut();
            if !guard.http.agent_pending.contains(&id) {
                guard.http.agent_pending.push(id);
                add_agent_request(&agent, &name, &request);
            }
        }
        if let Some(request) = receiver {
            set_request_property(Some(request), "finished", Value::Boolean(true));
        }
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    if let Some(request) = state.borrow_mut().http.clientreqs.get_mut(&id) {
        request.dispatched = true;
    }
    if args
        .first()
        .is_some_and(|data| !matches!(data, Value::Undefined))
    {
        let bytes = request_chunk_bytes(end_chunk_args(args))?;
        if !bytes.is_empty() {
            if let Some(request) = state.borrow_mut().http.clientreqs.get_mut(&id) {
                request.body.extend_from_slice(&bytes);
                request.body_chunks.push(bytes);
            }
        }
    }
    let (
        target,
        method,
        path,
        headers,
        body,
        body_chunks,
        body_started,
        agent,
        lookup,
        omit_host,
        secure,
        tls_options,
        tls_rejected,
    ) = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        if req.aborted {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        }
        if matches!(
            execute::get_property(&req.req, "finished"),
            Value::Boolean(true)
        ) {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        }
        (
            req.target.clone(),
            req.method.clone(),
            req.path.clone(),
            req.headers.clone(),
            req.body.clone(),
            req.body_chunks.clone(),
            req.body_started,
            req.agent.clone(),
            req.lookup.clone(),
            req.omit_host,
            req.secure,
            req.tls_options.clone(),
            req.tls_rejected,
        )
    };
    let expect_header_start = receiver.is_some_and(|request| {
        matches!(
            execute::get_property(request, CLIENT_EXPECT_CONTINUE_PROP),
            Value::Boolean(true)
        )
    });
    if let Some(req_value) = receiver.filter(|_| !expect_header_start) {
        set_request_property(Some(req_value), "finished", Value::Boolean(true));
    }
    let host = target_host(&target);
    let head_host = request_host(&target);
    let head = request_head_with_chunking(
        &head_host,
        &method,
        &path,
        &headers,
        body.len(),
        omit_host,
        body_started,
    );
    if let Some(req) = state.borrow().http.clientreqs.get(&id) {
        set_request_property(Some(&req.req), "_header", Value::String(head.clone()));
    }
    let custom = agent
        .as_ref()
        .and_then(|agent| custom_connection(state, agent));
    let custom_socket = agent.as_ref().and_then(|agent| {
        custom
            .is_none()
            .then(|| custom_socket(state, agent))
            .flatten()
    });
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
    if let Some(socket) = pooled.as_ref() {
        crate::modules::http::clear_idle_socket(state, socket);
        // A pooled socket carries the previous idle timer. Reusing it makes
        // that timer stale; clear it before attaching the new request's
        // listeners so an idle expiry cannot destroy an active request.
        net::socket_set_timeout(state, Some(socket), &[Value::Number(0.0)])?;
    }
    let reused = pooled.is_some()
        || matches!(
            receiver.map(|request| execute::get_property(request, "reusedSocket")),
            Some(Value::Boolean(true))
        );
    set_request_property(receiver, "reusedSocket", Value::Boolean(reused));
    let pooled_transport = pooled.filter(|socket| net::net_id(socket).is_some());
    let socket = match (existing.or(pooled_transport), custom.or(custom_socket.clone())) {
        (Some(socket), _) => {
            net::socket_ref(state, Some(&socket), &[])?;
            net::socket_write(state, Some(&socket), &[host_api::bytes(head.as_bytes())])?;
            if body_started {
                for chunk in &body_chunks {
                    let mut frame = format!("{:x}\r\n", chunk.len()).into_bytes();
                    frame.extend_from_slice(chunk);
                    frame.extend_from_slice(b"\r\n");
                    net::socket_write(state, Some(&socket), &[host_api::bytes(&frame)])?;
                }
                net::socket_write(state, Some(&socket), &[host_api::bytes(b"0\r\n\r\n")])?;
            } else if body_chunks.is_empty() && !body.is_empty() {
                net::socket_write(state, Some(&socket), &[host_api::bytes(&body)])?;
            } else {
                for chunk in &body_chunks {
                    net::socket_write(state, Some(&socket), &[host_api::bytes(chunk)])?;
                }
            }
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
            let call_args = if custom_socket.is_some() {
                vec![
                    receiver.cloned().unwrap_or(Value::Undefined),
                    options,
                    callback,
                ]
            } else {
                vec![options, callback]
            };
            let socket = execute::call(&connection, agent.as_ref().unwrap(), &call_args)?;
            if matches!(socket, Value::Undefined | Value::Null) {
                return Ok(receiver.cloned().unwrap_or(Value::Undefined));
            }
            let mut bytes = request_head_with_chunking(
                &request_host(&target),
                &method,
                &path,
                &headers,
                body.len(),
                omit_host,
                body_started,
            )
            .into_bytes();
            state.borrow_mut().net.pending_request_writes.push((
                socket.clone(),
                bytes,
                receiver.cloned().unwrap_or(Value::Undefined),
            ));
            if body_started {
                for chunk in &body_chunks {
                    let mut frame = format!("{:x}\r\n", chunk.len()).into_bytes();
                    frame.extend_from_slice(chunk);
                    frame.extend_from_slice(b"\r\n");
                    state.borrow_mut().net.pending_request_writes.push((
                        socket.clone(),
                        frame,
                        receiver.cloned().unwrap_or(Value::Undefined),
                    ));
                }
                state.borrow_mut().net.pending_request_writes.push((
                    socket.clone(),
                    b"0\r\n\r\n".to_vec(),
                    receiver.cloned().unwrap_or(Value::Undefined),
                ));
            } else if !body.is_empty() {
                state.borrow_mut().net.pending_request_writes.push((
                    socket.clone(),
                    body.clone(),
                    receiver.cloned().unwrap_or(Value::Undefined),
                ));
            }
            let resume = execute::get_property(&socket, "resume");
            if quench_runtime::is_callable(&resume) {
                execute::call(&resume, &socket, &[])?;
            }
            if secure {
                crate::modules::tls::decorate_socket(&socket, tls_options.as_ref());
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
            let socket = net::connect(state, &[host_api::object(options)])?;
            if secure {
                crate::modules::tls::decorate_socket(&socket, tls_options.as_ref());
            }
            socket
        }
        (None, None) => send_request(
            state,
            &target,
            &method,
            &path,
            &headers,
            &body,
            omit_host,
            receiver.cloned().unwrap_or(Value::Undefined),
        )?,
    };
    if secure {
        crate::modules::tls::decorate_socket(&socket, tls_options.as_ref());
    }
    let high_water_mark = state
        .borrow()
        .http
        .clientreqs
        .get(&id)
        .and_then(|req| req.high_water_mark);
    if reused {
        if let Some(value) = high_water_mark {
            execute::set_property_in_place(&socket, "writableHighWaterMark", Value::Number(value));
        }
    }
    let socket_id = net::net_id(&socket);
    let mut guard = state.borrow_mut();
    if let Some(req) = guard.http.clientreqs.get_mut(&id) {
        req.socket = Some(socket.clone());
        set_request_property(Some(&req.req), "socket", socket.clone());
        // The socket carries the one current HTTP message identity. Expose
        // it before the first response event so agent/remove and destroy
        // observers see the same request object as the host state.
        set_request_property(Some(&socket), "_httpMessage", req.req.clone());
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
    if tls_rejected {
        state.borrow_mut().net.pending_events.push((
            socket.clone(),
            "error".into(),
            vec![tls_verification_error()],
        ));
    }
    let (request, request_timeout, timeout_cb, agent_timeout) = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        (
            req.req.clone(),
            execute::get_property(&req.req, "timeout"),
            execute::get_property(&req.req, "timeoutCb"),
            req.agent
                .as_ref()
                .map(|agent| execute::get_property(agent, "timeout")),
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
    if reused {
        // A pooled socket carries the previous request's timeout listeners.
        // They are request-scoped, so clear that listener set before the new
        // request installs its ordinary timeout transition.
        crate::modules::events::method_remove_all_listeners(
            state,
            Some(&socket),
            &[Value::String("timeout".into())],
        )?;
    }
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
            if request_timeout_set {
                let connected = net::net_id(&socket).is_some_and(|socket_id| {
                    state
                        .borrow()
                        .net
                        .sockets
                        .get(&socket_id)
                        .is_some_and(|entry| entry.borrow().stream.is_some())
                });
                if connected {
                    net::socket_set_timeout(
                        state,
                        Some(&socket),
                        &[Value::Number(value), request_timeout_cb.clone()],
                    )?;
                } else {
                    // A lookup may leave the socket unconnected. Keep the
                    // request callback observable now; the connect transition
                    // installs the actual timer once a transport exists.
                    execute::set_property_in_place(&socket, "timeout", Value::Number(value));
                    subscribe_event(state, &socket, "timeout", request_timeout_cb.clone())?;
                }
            } else {
                execute::set_property_in_place(&socket, "timeout", Value::Number(value));
                subscribe_event(state, &socket, "timeout", request_timeout_cb.clone())?;
                // A custom lookup may return before invoking its callback,
                // leaving an unconnected socket as the only pending handle.
                // Give that socket the Agent timeout so the ordinary timeout
                // transition can retire the pending lookup; connected
                // sockets receive their idle timer when released to the pool.
                let connected = net::net_id(&socket).is_some_and(|socket_id| {
                    state
                        .borrow()
                        .net
                        .sockets
                        .get(&socket_id)
                        .is_some_and(|entry| entry.borrow().stream.is_some())
                });
                let unconnected = !connected;
                if unconnected {
                    net::socket_set_timeout(state, Some(&socket), &[Value::Number(value)])?;
                }
            }
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
    if !matches!(
        execute::get_property(&request, CLIENT_SOCKET_EVENT_QUEUED_PROP),
        Value::Boolean(true)
    ) {
        set_request_property(
            Some(&request),
            CLIENT_SOCKET_EVENT_QUEUED_PROP,
            Value::Boolean(true),
        );
        state
            .borrow_mut()
            .net
            .pending_events
            .push((request, "socket".into(), vec![socket]));
    }
    invoke_end_callback(receiver, args)?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn invoke_end_callback(receiver: Option<&Value>, args: &[Value]) -> Result<(), VmError> {
    let callback = args.iter().find(|value| quench_runtime::is_callable(value));
    if let Some(callback) = callback {
        execute::call(callback, receiver.unwrap_or(&Value::Undefined), &[])?;
    }
    Ok(())
}

fn tls_verification_error() -> Value {
    host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "message".into(),
            Value::String("unable to verify the first certificate".into()),
        ),
        (
            "code".into(),
            Value::String("UNABLE_TO_VERIFY_LEAF_SIGNATURE".into()),
        ),
    ])
}

fn tls_verification_disabled() -> bool {
    let global = quench_runtime::vm::current_global_object();
    let process = execute::get_property(&global, "process");
    let env = execute::get_property(&process, "env");
    matches!(execute::get_property(&env, "NODE_TLS_REJECT_UNAUTHORIZED"), Value::String(value) if value == "0")
}

fn send_request(
    state: &Rc<RefCell<HostState>>,
    target: &RequestTarget,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
    omit_host: bool,
    request: Value,
) -> Result<Value, VmError> {
    let socket = open_socket(state, target)?;
    let head = request_head(
        &request_host(target),
        method,
        path,
        headers,
        body.len(),
        omit_host,
    );
    let mut payload = head.into_bytes();
    payload.extend_from_slice(body);
    state
        .borrow_mut()
        .net
        .pending_request_writes
        .push((socket.clone(), payload, request));
    Ok(socket)
}

fn open_socket(state: &Rc<RefCell<HostState>>, target: &RequestTarget) -> Result<Value, VmError> {
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

fn target_port(target: &RequestTarget) -> u16 {
    match target {
        RequestTarget::Tcp { port, .. } => *port,
        RequestTarget::Unix { .. } => 0,
    }
}

fn client_performance_request(state: &Rc<RefCell<HostState>>, client_id: u64) -> Value {
    let (target, method, path, headers) = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| {
            (
                req.target.clone(),
                req.method.clone(),
                req.path.clone(),
                req.headers.clone(),
            )
        })
        .unwrap_or((
            RequestTarget::Tcp {
                host: "localhost".into(),
                port: 80,
            },
            "GET".into(),
            "/".into(),
            Vec::new(),
        ));
    host_api::object(vec![
        ("method".into(), Value::String(method)),
        (
            "url".into(),
            Value::String(format!(
                "http://{}:{}{}",
                target_host(&target),
                target_port(&target),
                path
            )),
        ),
        (
            "headers".into(),
            host_api::object(
                headers
                    .into_iter()
                    .map(|(key, value)| (key, Value::String(value)))
                    .collect(),
            ),
        ),
    ])
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
    request_head_with_chunking(host, method, path, headers, body_len, omit_host, false)
}

fn request_head_with_chunking(
    host: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body_len: usize,
    omit_host: bool,
    force_chunked: bool,
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
    } else if force_chunked && !has_content_length {
        head.push_str("Transfer-Encoding: chunked\r\n");
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
    if matches!(
        execute::get_property(socket, CLIENT_SOCKET_SUBSCRIBED_PROP),
        Value::Boolean(true)
    ) {
        return Ok(());
    }
    execute::set_property_in_place(socket, CLIENT_SOCKET_SUBSCRIBED_PROP, Value::Boolean(true));
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
    execute::set_property_in_place(
        socket,
        "Symbol(async_id_symbol)\0quench",
        Value::Number(-1.0),
    );
    crate::modules::http::clear_idle_socket(state, socket);
    let Some(client_id) = client_id_for_socket(state, socket) else {
        return Ok(Value::Undefined);
    };
    let request = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| req.req.clone());
    let request_closed = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .is_some_and(|req| req.response_closed);
    let agent_target = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .and_then(|req| req.agent.clone().map(|agent| (agent, req.target.clone())));
    if let (Some((agent, target)), Some(request)) = (agent_target, request.as_ref()) {
        let name = agent_name(&target, &agent);
        remove_idle_agent_socket(&agent, &name, socket, request);
    }
    let destroy = execute::get_property(socket, "destroy");
    if quench_runtime::is_callable(&destroy) {
        let _ = execute::call(&destroy, socket, &[])?;
    }
    if !request_closed {
        if let (Some(request), Some(error)) = (request, args.first().cloned()) {
            let suppress_tls_pipe = state
                .borrow()
                .http
                .clientreqs
                .get(&client_id)
                .is_some_and(|req| req.secure && !req.tls_rejected)
                && matches!(execute::get_property(&error, "code"), Value::String(ref code) if code == "EPIPE");
            if suppress_tls_pipe {
                return Ok(Value::Undefined);
            }
            // A transport-supplied error is already the request's terminal
            // failure; req_close must not append a generic ECONNRESET.
            set_request_property(Some(&request), "errored", error.clone());
            net::emit(state, &request, "error", vec![error])?;
        }
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
            execute::call(&on, socket, &[Value::String(event.to_string()), listener])?;
        }
        Ok(())
    }
}

/// Apply an explicit request timeout at the socket's connect transition.
/// Keeping this in the net pump makes the ordering fact observable: the
/// socket event still sees the Agent timeout, while connect observers see the
/// request timeout already installed.
pub(crate) fn apply_deferred_request_timeout(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
) -> Result<(), VmError> {
    let Some(id) = client_id_for_socket(state, socket) else {
        return Ok(());
    };
    let timeout = state
        .borrow_mut()
        .http
        .clientreqs
        .get_mut(&id)
        .and_then(|req| req.pending_timeout.take());
    if let Some(timeout) = timeout {
        net::socket_set_timeout(state, Some(socket), &[Value::Number(timeout)])?;
        // The request may hold a VM alias of the canonical net socket. Keep
        // the observable timeout value synchronized on that alias before the
        // connect event is emitted.
        execute::set_property_in_place(socket, "timeout", Value::Number(timeout));
        if let Some(request_socket) = state
            .borrow()
            .http
            .clientreqs
            .get(&id)
            .map(|request| execute::get_property(&request.req, "socket"))
            .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        {
            execute::set_property_in_place(&request_socket, "timeout", Value::Number(timeout));
        }
    }
    Ok(())
}

pub(crate) fn request_write_allowed(state: &Rc<RefCell<HostState>>, request: &Value) -> bool {
    let Some(id) = client_id(Some(request)) else {
        return false;
    };
    let allowed = state.borrow().http.clientreqs.get(&id).is_some_and(|req| {
        !req.aborted
            && !req.tls_rejected
            && matches!(execute::get_property(&req.req, "errored"), Value::Undefined)
            && !matches!(
                execute::get_property(&req.req, "destroyed"),
                Value::Boolean(true)
            )
    });
    allowed
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
    crate::modules::http::clear_idle_socket(state, socket);
    crate::modules::http::abort_server_signal(state, socket)?;
    let Some(client_id) = client_id_for_socket(state, socket) else {
        let requests = state
            .borrow()
            .http
            .conns
            .values()
            .flat_map(|conn| conn.requests.iter().cloned())
            .collect::<Vec<_>>();
        for request in requests {
            if matches!(
                execute::get_property(&request, crate::modules::http::REQ_CLOSE_PROP),
                Value::Boolean(true)
            ) {
                continue;
            }
            execute::set_property_in_place(
                &request,
                crate::modules::http::REQ_CLOSE_PROP,
                Value::Boolean(true),
            );
            net::emit(state, &request, "close", Vec::new())?;
            let resource =
                execute::get_property(&request, crate::modules::http::REQ_ASYNC_RESOURCE_PROP);
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
    let parse_error = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .is_some_and(|req| req.parse_error);
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
    if response.is_none() && !parse_error {
        if let Some(request) = request.as_ref() {
            let explicit_error = !matches!(
                execute::get_property(request, "errored"),
                Value::Null | Value::Undefined
            );
            let has_error_listener = crate::modules::emitter::emitter_id(request)
                .and_then(|id| state.borrow().emitters.get(id))
                .is_some_and(|emitter| !emitter.borrow().listeners_of("error").is_empty());
            let explicitly_aborted = matches!(
                execute::get_property(request, "aborted"),
                Value::Boolean(true)
            );
            if has_error_listener && !explicit_error && !explicitly_aborted {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String("socket hang up".into())],
                );
                let error =
                    execute::set_property(error, "code", Value::String("ECONNRESET".into()));
                net::emit(state, request, "error", vec![error])?;
            }
        }
    }
    if let Some(request) = request {
        let close_already_emitted = matches!(
            execute::get_property(&request, CLIENT_CLOSE_PENDING_PROP),
            Value::Boolean(true)
        );
        if !close_already_emitted {
            net::emit(state, &request, "close", Vec::new())?;
            let resource = execute::get_property(&request, CLIENT_ASYNC_RESOURCE_PROP);
            crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
        }
    }
    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
        // A socket close is terminal for an aborted/no-response request too.
        // Mark it closed before Agent queue accounting so the released
        // physical slot can be reused exactly once.
        req.response_closed = true;
    }
    if let (Some(agent), Some(target)) = (agent.as_ref(), target.as_ref()) {
        let name = agent_name(target, agent);
        let has_pending = state.borrow().http.agent_pending.iter().any(|id| {
            state
                .borrow()
                .http
                .clientreqs
                .get(id)
                .is_some_and(|request| {
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
                    &[Value::String("aborted".into())],
                );
                let error =
                    execute::set_property(error, "code", Value::String("ECONNRESET".into()));
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
        let max_total = match execute::get_property(agent, "maxTotalSockets") {
            Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
            _ => usize::MAX,
        };
        let active_total = guard
            .http
            .clientreqs
            .values()
            .filter(|request| {
                request.dispatched
                    && !request.response_closed
                    && request
                        .agent
                        .as_ref()
                        .is_some_and(|candidate| execute::same_identity(candidate, agent))
            })
            .count();
        let position = guard.http.agent_pending.iter().position(|id| {
            let Some(request) = guard.http.clientreqs.get(id) else {
                return false;
            };
            if request.aborted
                || matches!(
                    execute::get_property(&request.req, "destroyed"),
                    Value::Boolean(true)
                )
            {
                return false;
            }
            let same_agent = request
                .agent
                .as_ref()
                .is_some_and(|candidate| execute::same_identity(candidate, agent));
            if !same_agent || active_total >= max_total {
                return false;
            }
            let request_name = agent_name(&request.target, agent);
            let max_sockets = match execute::get_property(agent, "maxSockets") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
                _ => usize::MAX,
            };
            let active_name = guard
                .http
                .clientreqs
                .values()
                .filter(|candidate| {
                    candidate.dispatched
                        && !candidate.response_closed
                        && candidate.agent.as_ref().is_some_and(|value| {
                            execute::same_identity(value, agent)
                                && agent_name(&candidate.target, agent) == request_name
                        })
                })
                .count();
            active_name < max_sockets && (request_name == name || active_total < max_total)
        });
        position.and_then(|index| {
            guard.http.agent_pending.get(index).copied().map(|id| {
                guard.http.agent_pending.remove(index);
                id
            })
        })
    };
    let Some(id) = pending else {
        return;
    };
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
    if matches!(request, Value::Undefined) {
        return;
    }
    set_request_property(Some(&request), "finished", Value::Boolean(false));
    let _ = req_end(state, Some(&request), &[]);
}

fn agent_name(target: &RequestTarget, agent: &Value) -> String {
    let options = match target {
        RequestTarget::Tcp { host, port } => host_api::object(vec![
            ("host".into(), Value::String(host.clone())),
            ("port".into(), Value::Number(*port as f64)),
        ]),
        RequestTarget::Unix { path } => {
            host_api::object(vec![("socketPath".into(), Value::String(path.clone()))])
        }
    };
    execute::to_js_string(
        &execute::call(&execute::get_property(agent, "getName"), agent, &[options])
            .unwrap_or(Value::String(String::new())),
    )
    .unwrap_or_default()
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
    if execute::own_enumerable_keys(&list)
        .into_iter()
        .map(|key| execute::get_property(&list, &key))
        .any(|value| same_socket(&value, socket))
    {
        return;
    }
    let total = execute::own_enumerable_keys(&pools)
        .into_iter()
        .map(|key| execute::get_property(&pools, &key))
        .map(|entry| match execute::get_property(&entry, "length") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
            _ => 0,
        })
        .sum::<usize>();
    if matches!(execute::get_property(agent, "maxTotalSockets"), Value::Number(limit) if limit.is_finite() && limit > 0.0 && total >= limit as usize)
    {
        return;
    }
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
    let lifo = matches!(execute::get_property(agent, "scheduling"), Value::String(value) if value == "lifo");
    loop {
        let length = match execute::get_property(&list, "length") {
            Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
            _ => return None,
        };
        let key = if lifo {
            (length - 1).to_string()
        } else {
            "0".into()
        };
        let socket = execute::get_property(&list, &key);
        if let Some(id) = net::net_id(&socket) {
            execute::set_property_in_place(
                &socket,
                "Symbol(async_id_symbol)\0quench",
                Value::Number(id as f64),
            );
        }
        if lifo {
            let pop = execute::get_property(&list, "pop");
            if quench_runtime::is_callable(&pop) {
                let _ = execute::call(&pop, &list, &[]);
            } else {
                execute::set_array_length_in_place(&list, length - 1);
            }
        } else {
            let shift = execute::get_property(&list, "shift");
            if quench_runtime::is_callable(&shift) {
                let _ = execute::call(&shift, &list, &[]);
            } else {
                for index in 1..length {
                    let value = execute::get_property(&list, &index.to_string());
                    execute::set_property_in_place(&list, &(index - 1).to_string(), value);
                }
                execute::set_property_in_place(&list, "length", Value::Number((length - 1) as f64));
            }
        }
        if matches!(socket, Value::Object(_) | Value::ObjectAlias(_))
            && !matches!(
                execute::get_property(&socket, "destroyed"),
                Value::Boolean(true)
            )
        {
            return Some(socket);
        }
    }
}

fn move_agent_socket_to_free(agent: &Value, name: &str, socket: &Value) {
    let sockets_pools = execute::get_property(agent, "sockets");
    let sockets = execute::get_property(&sockets_pools, name);
    let remaining: Vec<Value> = execute::own_enumerable_keys(&sockets)
        .into_iter()
        .filter_map(|key| {
            let value = execute::get_property(&sockets, &key);
            (!same_socket(&value, socket)).then_some(value)
        })
        .filter(|value| !matches!(value, Value::Undefined))
        .collect();
    let found = remaining.len() < execute::own_enumerable_keys(&sockets).len();
    if !found {
        return;
    }
    execute::set_property_in_place(
        socket,
        "Symbol(async_id_symbol)\0quench",
        Value::Number(-1.0),
    );
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
    for pool_name in ["sockets", "freeSockets"] {
        let pools = execute::get_property(agent, pool_name);
        let list = execute::get_property(&pools, name);
        let keys = execute::own_enumerable_keys(&list);
        let remaining: Vec<Value> = keys
            .iter()
            .filter_map(|key| {
                let value = execute::get_property(&list, key);
                (!same_socket(&value, socket))
                    .then_some(value)
                    .filter(|value| !matches!(value, Value::Undefined))
            })
            .collect();
        if remaining.len() == keys.len() {
            continue;
        }
        if remaining.is_empty() {
            let (updated, _) = execute::delete_property(pools, name);
            execute::set_property_in_place(agent, pool_name, updated);
        } else {
            execute::set_property_in_place(&pools, name, host_api::array(remaining));
        }
    }
}

fn remove_idle_agent_socket(agent: &Value, name: &str, socket: &Value, request: &Value) {
    for pool_name in ["sockets", "freeSockets"] {
        let pools = execute::get_property(agent, pool_name);
        let list = execute::get_property(&pools, name);
        let keys = execute::own_enumerable_keys(&list);
        let remaining: Vec<Value> = keys
            .iter()
            .filter_map(|key| {
                let value = execute::get_property(&list, key);
                let belongs_to_error = same_socket(&value, socket)
                    || execute::same_identity(
                        &execute::get_property(&value, "_httpMessage"),
                        request,
                    );
                (!belongs_to_error)
                    .then_some(value)
                    .filter(|value| !matches!(value, Value::Undefined))
            })
            .collect();
        if remaining.len() == keys.len() {
            continue;
        }
        if remaining.is_empty() {
            let (updated, _) = execute::delete_property(pools, name);
            execute::set_property_in_place(agent, pool_name, updated);
        } else {
            execute::set_property_in_place(&pools, name, host_api::array(remaining));
        }
    }
}

fn same_socket(left: &Value, right: &Value) -> bool {
    execute::same_identity(left, right)
        || matches!((net::net_id(left), net::net_id(right)), (Some(a), Some(b)) if a == b)
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
            if let Some(error) = invalid_response_header(&head) {
                if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                    req.parse_error = true;
                }
                let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
                net::emit(state, &request, "error", vec![error])?;
                net::socket_destroy(state, Some(socket), &[])?;
                return Ok(Value::Undefined);
            }
            if let Some(status) = response_status(&head).filter(|status| (100..200).contains(status)) {
                if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                    req.head_parsed = false;
                }
                if let Some(request) = client_value(state, client_id, true) {
                    if status == 100 {
                        net::emit(state, &request, "continue", Vec::new())?;
                    } else if status != 101 {
                        let info = build_incoming(state, client_id, &head)?;
                        net::emit(state, &request, "information", vec![info])?;
                    }
                    // A socket read may contain both the interim head and the
                    // final response head/body.  Re-enter the parser for the
                    // buffered remainder instead of waiting for another read.
                    let remainder = {
                        let mut guard = state.borrow_mut();
                        guard
                            .http
                            .clientreqs
                            .get_mut(&client_id)
                            .map(|req| std::mem::take(&mut req.buffer))
                            .unwrap_or_default()
                    };
                    if !remainder.is_empty() {
                        return data_handler(
                            state,
                            Some(socket),
                            &[host_api::bytes(&remainder)],
                        );
                    }
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
                if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                    // The protocol error is the observable terminal failure;
                    // suppress the socket teardown's synthetic ECONNRESET.
                    req.parse_error = true;
                }
                let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
                net::emit(state, &request, "error", vec![error])?;
                net::socket_destroy(state, Some(socket), &[])?;
                return Ok(Value::Undefined);
            }
            if let Some(error) = duplicate_content_length(&head) {
                if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                    req.parse_error = true;
                }
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
                ensure_http_handle(&socket);
                set_response_property(&res, "socket", socket.clone());
                // IncomingMessage exposes the same connection identity via
                // both historical `connection` and modern `socket` names.
                set_response_property(&res, "connection", socket.clone());
            }
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.res = Some(res.clone());
            }
            let req_value = client_value(state, client_id, true).unwrap_or(Value::Undefined);
            net::emit(state, &req_value, "response", vec![res])?;
            flush_body(state, client_id)?;
            if let Some(response) = client_value(state, client_id, false) {
                net::emit(state, &response, "readable", Vec::new())?;
            }
            finish_known_response(state, client_id)
        }
        None if head_parsed => {
            let chunked = client_value(state, client_id, false)
                .map(|response| response_is_chunked(&response))
                .unwrap_or(false);
            let body = if chunked {
                let body = {
                    let mut guard = state.borrow_mut();
                    let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
                        return Ok(Value::Undefined);
                    };
                    req.buffer.extend_from_slice(&bytes);
                    let (body, consumed, done) = decode_chunked_prefix(&req.buffer);
                    let invalid = !done && invalid_chunked_prefix(&req.buffer[consumed..]);
                    let remainder = req.buffer[consumed..].to_vec();
                    req.buffer = remainder;
                    req.response_received = req.response_received.saturating_add(body.len());
                    if done {
                        req.response_chunked_done = true;
                    }
                    (body, invalid)
                };
                if body.1 {
                    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                        req.parse_error = true;
                        req.buffer.clear();
                    }
                    let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
                    net::emit(
                        state,
                        &request,
                        "error",
                        vec![invalid_chunked_response()],
                    )?;
                    net::socket_destroy(state, Some(socket), &[])?;
                    return Ok(Value::Undefined);
                }
                body.0
            } else {
                if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                    req.response_received = req.response_received.saturating_add(bytes.len());
                }
                bytes
            };
            if let Some(res) = client_value(state, client_id, false) {
                net::emit(state, &res, "readable", Vec::new())?;
                let body = if chunked {
                    body
                } else {
                    response_body_bytes(&res, &body)
                };
                if !body.is_empty() {
                    let data = response_data(&res, &body);
                    queue_response_data(&res, data.clone());
                    net::emit(state, &res, "data", vec![data])?;
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
    if req.head_parsed {
        return (None, true);
    }
    req.buffer.extend_from_slice(bytes);
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
    let (res, body, invalid, socket) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
            return Ok(());
        };
        let chunked = req.res.as_ref().is_some_and(response_is_chunked);
        let (body, consumed, done) = if chunked {
            decode_chunked_prefix(&req.buffer)
        } else {
            let consumed = req
                .res
                .as_ref()
                .and_then(response_content_length)
                .map(|expected| {
                    expected
                        .saturating_sub(req.response_received)
                        .min(req.buffer.len())
                })
                .unwrap_or(req.buffer.len());
                (req.buffer[..consumed].to_vec(), consumed, false)
            };
        let invalid = chunked && !done && invalid_chunked_prefix(&req.buffer[consumed..]);
        req.response_received = req.response_received.saturating_add(body.len());
        if done {
            req.response_chunked_done = true;
        }
        req.buffer.drain(..consumed);
        (req.res.clone(), body, invalid, req.socket.clone())
    };
    if invalid {
        if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
            req.parse_error = true;
            req.buffer.clear();
        }
        let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
        net::emit(
            state,
            &request,
            "error",
            vec![invalid_chunked_response()],
        )?;
        if let Some(socket) = socket {
            net::socket_destroy(state, Some(&socket), &[])?;
        }
        return Ok(());
    }
    if let Some(res) = res {
        if !body.is_empty() {
            let body = if response_is_chunked(&res) {
                body
            } else {
                response_body_bytes(&res, &body)
            };
            if !body.is_empty() {
                let data = response_data(&res, &body);
                queue_response_data(&res, data.clone());
                net::emit(state, &res, "data", vec![data])?;
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

fn response_is_chunked(response: &Value) -> bool {
    let headers = execute::get_property(response, "headers");
    execute::to_js_string(&execute::get_property(&headers, "transfer-encoding"))
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
}

fn ensure_http_handle(socket: &Value) {
    if !matches!(
        execute::get_property(socket, "_handle"),
        Value::Null | Value::Undefined
    ) {
        return;
    }
    let handle = host_api::object(vec![
        (
            "close".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "asyncReset".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
    ]);
    let updated = execute::set_property(socket.clone(), "_handle", handle);
    execute::replace_value(socket, &updated);
}

fn response_allows_reuse(response: &Value) -> bool {
    let headers = execute::get_property(response, "headers");
    if execute::to_js_string(&execute::get_property(&headers, "connection"))
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("close"))
    {
        return false;
    }
    let has_length = !matches!(
        execute::get_property(&headers, "content-length"),
        Value::Undefined
    );
    let has_encoding = !matches!(
        execute::get_property(&headers, "transfer-encoding"),
        Value::Undefined
    );
    has_length || has_encoding
}

fn response_status(head: &[u8]) -> Option<u16> {
    String::from_utf8_lossy(head)
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
}

fn finish_known_response(state: &Rc<RefCell<HostState>>, client_id: u64) -> Result<(), VmError> {
    let socket = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&client_id) else {
            return Ok(());
        };
        let Some(response) = req.res.as_ref() else {
            return Ok(());
        };
        let headers = execute::get_property(response, "headers");
        let status = execute::get_property(response, "statusCode");
        let no_body_status = matches!(
            status,
            Value::Number(value)
                if (100.0..200.0).contains(&value) || value == 204.0 || value == 304.0
        );
        let expected = execute::to_js_string(&execute::get_property(&headers, "content-length"))
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        let chunked = execute::to_js_string(&execute::get_property(&headers, "transfer-encoding"))
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
        let complete = no_body_status
            || expected.is_some_and(|length| req.response_received >= length)
            || (chunked && req.response_chunked_done);
        complete.then(|| req.socket.clone()).flatten()
    };
    if let Some(socket) = socket {
        if let Some(error) = invalid_trailing_response_start(state, client_id) {
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.parse_error = true;
                req.buffer.clear();
            }
            detach_parser_data_listener(state, &socket)?;
            let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
            net::emit(state, &request, "error", vec![error])?;
            net::socket_destroy(state, Some(&socket), &[])?;
            return Ok(());
        }
        let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
        if !matches!(
            execute::get_property(&request, "\0quench:http-perf-recorded"),
            Value::Boolean(true)
        ) {
            let response = state
                .borrow()
                .http
                .clientreqs
                .get(&client_id)
                .and_then(|req| req.res.clone())
                .unwrap_or(Value::Undefined);
            crate::modules::http::record_http_entry(
                state,
                "HttpClient",
                client_performance_request(state, client_id),
                response,
            );
            set_request_property(
                Some(&request),
                "\0quench:http-perf-recorded",
                Value::Boolean(true),
            );
        }
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
    let error = execute::set_property(
        error,
        "code",
        Value::String("HPE_INVALID_TRANSFER_ENCODING".into()),
    );
    let error = execute::set_property(error, "reason", Value::String(reason.into()));
    let error = execute::set_property(error, "bytesParsed", Value::Number((head.len() + 4) as f64));
    Some(execute::set_property(
        error,
        "rawPacket",
        crate::modules::buffer_proto::make_buffer(&raw),
    ))
}

fn duplicate_content_length(head: &[u8]) -> Option<Value> {
    let count = String::from_utf8_lossy(head)
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        })
        .count();
    if count < 2 {
        return None;
    }
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(
            "Parse Error: Duplicate Content-Length".into(),
        )],
    );
    Some(execute::set_property(
        error,
        "code",
        Value::String("HPE_UNEXPECTED_CONTENT_LENGTH".into()),
    ))
}

fn invalid_chunked_response() -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("Parse Error: Invalid character in chunk size".into())],
    );
    execute::set_property(
        execute::set_property(error, "code", Value::String("HPE_INVALID_CHUNK_SIZE".into())),
        "reason",
        Value::String("Invalid character in chunk size".into()),
    )
}

fn invalid_chunked_prefix(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let line = bytes
        .iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')
        .map_or(bytes, |end| &bytes[..end]);
    let token = line.split(|byte| *byte == b';').next().unwrap_or_default();
    token.is_empty() || token.iter().any(|byte| !byte.is_ascii_hexdigit())
}

fn decode_chunked(bytes: &[u8]) -> Vec<u8> {
    decode_chunked_prefix(bytes).0
}

/// Decode complete HTTP/1.1 chunks from the front of `bytes`, retaining any
/// partial framing for the next transport read. The returned cursor advances
/// only across complete chunks (and the terminating zero chunk).
fn decode_chunked_prefix(bytes: &[u8]) -> (Vec<u8>, usize, bool) {
    let mut output = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let chunk_start = cursor;
        let Some(line_end) = bytes[cursor..].windows(2).position(|pair| pair == b"\r\n") else {
            return (output, cursor, false);
        };
        let line_end = cursor + line_end;
        let Ok(size) = usize::from_str_radix(
            String::from_utf8_lossy(&bytes[cursor..line_end])
                .split(';')
                .next()
                .unwrap_or(""),
            16,
        ) else {
            return (output, cursor, false);
        };
        cursor = line_end + 2;
        if cursor + size > bytes.len() {
            return (output, chunk_start, false);
        }
        if size == 0 {
            return if bytes.get(cursor..cursor + 2) == Some(b"\r\n") {
                (output, cursor + 2, true)
            } else {
                (output, chunk_start, false)
            };
        }
        let body_end = cursor + size;
        if bytes.get(body_end..body_end + 2) != Some(b"\r\n") {
            return (output, chunk_start, false);
        }
        output.extend_from_slice(&bytes[cursor..body_end]);
        cursor = body_end + 2;
    }
    (output, cursor, false)
}

fn pool_response_socket(
    state: &Rc<RefCell<HostState>>,
    client_id: u64,
    socket: &Value,
) -> Result<(), VmError> {
    let agent_target = {
        let guard = state.borrow();
        guard.http.clientreqs.get(&client_id).and_then(|request| {
            request
                .agent
                .clone()
                .map(|agent| (agent, request.target.clone()))
        })
    };
    let Some((agent, target)) = agent_target else {
        return Ok(());
    };
    let keep_alive = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .and_then(|request| {
            if request.aborted {
                return None;
            }
            request
                .agent
                .as_ref()
                .filter(|agent| matches!(agent, Value::Object(_) | Value::ObjectAlias(_)))
                .and_then(|_| request.res.as_ref())
        })
        .is_some_and(|response| {
            matches!(
                execute::get_property(&agent, "keepAlive"),
                Value::Boolean(true)
            ) && response_allows_reuse(response)
        });
    let drained = net::net_id(socket).is_some_and(|id| {
        state
            .borrow()
            .net
            .sockets
            .get(&id)
            .is_some_and(|value| net::pending_write_len(&value.borrow()) == 0)
    });
    let keep_alive = keep_alive && drained;
    let alive = net::net_id(socket).is_some_and(|id| {
        state
            .borrow()
            .net
            .sockets
            .get(&id)
            .is_some_and(|value| value.borrow().state != net::SocketState::Closed)
    });
    if keep_alive && alive {
        // A request timeout belongs to the request, not to an idle pooled
        // socket. Clear the host timer before invoking the Agent policy;
        // custom keepSocketAlive implementations may then install their own
        // reusable-socket timeout.
        if !matches!(
            execute::get_property(&agent, "timeout"),
            Value::Number(value) if value.is_finite() && value > 0.0
        ) {
            net::socket_set_timeout(state, Some(socket), &[Value::Number(0.0)])?;
        }
        let keep_socket_alive = execute::get_property(&agent, "keepSocketAlive");
        let keep_result = if quench_runtime::is_callable(&keep_socket_alive) {
            Some(execute::call(
                &keep_socket_alive,
                &agent,
                &[socket.clone()],
            )?)
        } else {
            None
        };
        if matches!(keep_result, Some(Value::Boolean(false))) {
            return Ok(());
        }
        let name = agent_name(&target, &agent);
        move_agent_socket_to_free(&agent, &name, socket);
        crate::modules::http::mark_idle_socket(state, socket);
        if matches!(execute::get_property(&agent, "timeout"), Value::Number(timeout) if timeout.is_finite() && timeout > 0.0)
        {
            // `keepSocketAlive` may replace the Agent timeout (for example a
            // subclass can install a custom idle timeout). Schedule using
            // the socket's resulting value instead of overwriting it.
            let timeout = execute::get_property(socket, "timeout");
            net::socket_set_timeout(state, Some(socket), &[timeout])?;
            let destroy = crate::host::capability(crate::registry::SPEC_NET_SOCKET_DESTROY);
            subscribe_event(state, socket, "timeout", destroy)?;
        }
    }
    // Agent callbacks may destroy the socket while the response event is
    // still unwinding.  In that case Node does not deliver a late `free`
    // notification for the dead socket (nor allow it to re-enter the pool).
    let still_alive = !matches!(
        execute::get_property(socket, "destroyed"),
        Value::Boolean(true)
    ) && net::net_id(socket).is_some_and(|id| {
        state
            .borrow()
            .net
            .sockets
            .get(&id)
            .is_some_and(|value| value.borrow().state != net::SocketState::Closed)
    });
    if still_alive {
        net::emit(state, socket, "free", Vec::new())?;
        emit_agent_free(state, &agent, &target, socket)?;
    }
    Ok(())
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
    execute::set_property_in_place(
        socket,
        "Symbol(async_id_symbol)\0quench",
        Value::Number(-1.0),
    );
    let Some(client_id) = client_id_for_socket(state, socket) else {
        return Ok(Value::Undefined);
    };
    let (res, received, chunked_done) = {
        let mut guard = state.borrow_mut();
        let Some(req) = guard.http.clientreqs.get_mut(&client_id) else {
            return Ok(Value::Undefined);
        };
        if req.response_ended {
            drop(guard);
            reject_trailing_response(state, client_id, socket)?;
            return Ok(Value::Undefined);
        }
        req.response_ended = true;
        (
            req.res.clone(),
            req.response_received,
            req.response_chunked_done,
        )
    };
    if let Some(res) = res {
        if matches!(
            execute::get_property(&res, "complete"),
            Value::Boolean(true)
        ) {
            reject_trailing_response(state, client_id, socket)?;
            return Ok(Value::Undefined);
        }
        let expected = match execute::get_property(&res, "headers") {
            headers @ (Value::Object(_) | Value::ObjectAlias(_)) => {
                execute::to_js_string(&execute::get_property(&headers, "content-length"))
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
            }
            _ => None,
        };
        let status = execute::get_property(&res, "statusCode");
        let no_body_status = matches!(
            status,
            Value::Number(value)
                if (100.0..200.0).contains(&value) || value == 204.0 || value == 304.0
        );
        let chunked = match execute::get_property(&res, "headers") {
            headers @ (Value::Object(_) | Value::ObjectAlias(_)) => {
                execute::to_js_string(&execute::get_property(&headers, "transfer-encoding"))
                    .ok()
                    .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
            }
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
        if !no_body_status
            && (expected.is_some_and(|expected| expected != received)
                || (chunked && !chunked_done))
        {
            return abort_incomplete_response(state, client_id, &res);
        }
        set_response_property(&res, "complete", Value::Boolean(true));
        set_response_property(&res, "readable", Value::Boolean(false));
        if let Some(request) = client_value(state, client_id, true) {
            if !matches!(
                execute::get_property(&request, "\0quench:http-perf-recorded"),
                Value::Boolean(true)
            ) {
                crate::modules::http::record_http_entry(
                    state,
                    "HttpClient",
                    client_performance_request(state, client_id),
                    res.clone(),
                );
                set_request_property(
                    Some(&request),
                    "\0quench:http-perf-recorded",
                    Value::Boolean(true),
                );
            }
            set_request_property(Some(&request), "destroyed", Value::Boolean(true));
        }
        pool_response_socket(state, client_id, socket)?;
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
            // A ClientRequest closes when its response completes, even when
            // the underlying keep-alive socket remains in Agent.freeSockets.
            // Mark the terminal request transition so the later socket close
            // cannot emit a duplicate request `'close'` event.
            set_request_property(
                Some(&request),
                CLIENT_CLOSE_PENDING_PROP,
                Value::Boolean(true),
            );
            net::emit(state, &request, "close", Vec::new())?;
            set_response_property(&res, "destroyed", Value::Boolean(true));
            set_response_property(&res, "closed", Value::Boolean(true));
            net::emit(state, &res, "close", Vec::new())?;
            let resource = execute::get_property(&request, CLIENT_ASYNC_RESOURCE_PROP);
            crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
        } else if let Some(request) = client_value(state, client_id, true) {
            set_request_property(Some(&request), "destroyed", Value::Boolean(true));
        }
        let pooled = state
            .borrow()
            .http
            .clientreqs
            .get(&client_id)
            .is_some_and(|request| {
                !request.aborted
                    && request.agent.as_ref().is_some_and(|agent| {
                        matches!(agent, Value::Object(_) | Value::ObjectAlias(_))
                    })
                    && request.agent.as_ref().is_some_and(|agent| {
                        matches!(
                            execute::get_property(agent, "keepAlive"),
                            Value::Boolean(true)
                        )
                    })
                    && request.res.as_ref().is_some_and(response_allows_reuse)
            });
        if pooled {
            // Idle HTTP agent sockets are retained for reuse but do not keep
            // the process alive. A later request refs the socket when reused.
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
        detach_parser_data_listener(state, socket)?;
        let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
        net::emit(state, &request, "error", vec![error])?;
        // A parser error is terminal for this connection.  Let the normal
        // socket close transition release the request and server resources.
        net::socket_destroy(state, Some(socket), &[])?;
    }
    Ok(Value::Undefined)
}

fn reject_trailing_response(
    state: &Rc<RefCell<HostState>>,
    client_id: u64,
    socket: &Value,
) -> Result<(), VmError> {
    let trailing = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| !req.buffer.is_empty() && !req.parse_error)
        .unwrap_or(false);
    if !trailing {
        return Ok(());
    }
    // Bytes after a completed response are either a valid pipelined response
    // (which Node drops with the connection) or an invalid response start,
    // which is reported as a parser error on the request.
    let error = invalid_response_start(state, client_id);
    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
        req.parse_error = true;
        req.buffer.clear();
    }
    detach_parser_data_listener(state, socket)?;
    if let Some(error) = error {
        let request = client_value(state, client_id, true).unwrap_or(Value::Undefined);
        net::emit(state, &request, "error", vec![error])?;
    }
    net::socket_destroy(state, Some(socket), &[]).map(|_| ())
}

fn detach_parser_data_listener(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
) -> Result<(), VmError> {
    crate::modules::events::method_remove_all_listeners(
        state,
        Some(socket),
        &[Value::String("data".into())],
    )
    .map(|_| ())
}

fn invalid_response_start(state: &Rc<RefCell<HostState>>, client_id: u64) -> Option<Value> {
    let (parsed, raw) = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| (req.head_parsed, req.buffer.clone()))?;
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

fn invalid_trailing_response_start(
    state: &Rc<RefCell<HostState>>,
    client_id: u64,
) -> Option<Value> {
    let raw = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| req.buffer.clone())?;
    if raw.is_empty() || raw.starts_with(b"HTTP/") {
        return None;
    }
    Some(execute::set_property(
        invalid_response_constant(),
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

fn invalid_response_header(head: &[u8]) -> Option<Value> {
    if !head
        .windows(2)
        .enumerate()
        .any(|(index, pair)| pair[0] == b'\r' && (index + 1 >= head.len() || pair[1] != b'\n'))
        && !head.last().is_some_and(|byte| *byte == b'\r')
    {
        return None;
    }
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(
            "Parse Error: Missing expected LF after CR".into(),
        )],
    );
    Some(execute::set_property(
        error,
        "code",
        Value::String("HPE_LF_EXPECTED".into()),
    ))
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
            &[Value::String("aborted".into())],
        );
        let error = execute::set_property(error, "code", Value::String("ECONNRESET".into()));
        net::emit(state, response, "error", vec![error])?;
    }
    if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
        req.response_closed = true;
    }
    set_response_property(response, "closed", Value::Boolean(true));
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
                "flushHeaders".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_END),
            ),
            (
                "setHeader".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_HEADER),
            ),
            (
                "getHeader".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_GET_HEADER),
            ),
            (
                "getHeaders".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_GET_HEADERS),
            ),
            (
                "getHeaderNames".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_GET_HEADER_NAMES),
            ),
            (
                "hasHeader".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_HAS_HEADER),
            ),
            (
                "removeHeader".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_REMOVE_HEADER),
            ),
            (
                "setNoDelay".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_NO_DELAY),
            ),
            (
                "setSocketKeepAlive".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_KEEP_ALIVE),
            ),
            (
                "setSocketTimeout".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_TIMEOUT_SOCKET),
            ),
            (
                "cork".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_CORK),
            ),
            (
                "uncork".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_UNCORK),
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
            ("writable".to_string(), Value::Boolean(true)),
            ("writableCorked".to_string(), Value::Number(0.0)),
            ("finished".to_string(), Value::Boolean(false)),
            ("timeout".to_string(), Value::Number(0.0)),
            (CLIENT_TIMEOUT_PROP.to_string(), Value::Undefined),
            (CLIENT_CLOSE_PENDING_PROP.to_string(), Value::Boolean(false)),
            (
                CLIENT_SOCKET_EVENT_QUEUED_PROP.to_string(),
                Value::Boolean(false),
            ),
            (CLIENT_ASYNC_RESOURCE_PROP.to_string(), async_resource),
        ],
    )?;
    object = execute::define_property(
        object,
        "path",
        host_api::object(vec![
            (
                "get".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_PATH_GET),
            ),
            (
                "set".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_PATH_SET),
            ),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]),
    )?;
    execute::set_property_in_place(&object, CLIENT_PATH_PROP, Value::String(String::new()));
    Ok((object, id))
}

pub fn req_path_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .map(|request| execute::get_property(request, CLIENT_PATH_PROP))
        .unwrap_or(Value::Undefined))
}

pub fn req_path_set(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let path = execute::to_js_string(&value)?;
    if path
        .chars()
        .any(|character| !(('\u{21}'..='\u{FF}').contains(&character)))
    {
        return Err(unescaped_path_error());
    }
    if let Some(request) = receiver {
        execute::set_property_in_place(request, CLIENT_PATH_PROP, Value::String(path));
    }
    Ok(Value::Undefined)
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

fn custom_connection(state: &Rc<RefCell<HostState>>, agent: &Value) -> Option<Value> {
    let default_prototype = state.borrow().http.agent_prototype.clone();
    let visible = execute::get_property(agent, "createConnection");
    if !quench_runtime::is_callable(&visible) {
        return None;
    }
    let default_method = default_prototype
        .as_ref()
        .map(|prototype| execute::get_property(prototype, "createConnection"));
    let custom = !default_method
        .as_ref()
        .is_some_and(|value| execute::same_identity(value, &visible));
    custom.then_some(visible)
}

fn custom_socket(state: &Rc<RefCell<HostState>>, agent: &Value) -> Option<Value> {
    let default_prototype = state.borrow().http.agent_prototype.clone();
    let visible = execute::get_property(agent, "createSocket");
    if !quench_runtime::is_callable(&visible) {
        return None;
    }
    if execute::own_enumerable_keys(agent)
        .iter()
        .any(|key| key == "createSocket")
    {
        return Some(visible);
    }
    let default_method = default_prototype
        .as_ref()
        .map(|prototype| execute::get_property(prototype, "createSocket"));
    let custom = !default_method
        .as_ref()
        .is_some_and(|value| execute::same_identity(value, &visible));
    custom.then_some(visible)
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
        .find(|(_, req)| {
            req.socket
                .as_ref()
                .is_some_and(|value| same_socket(value, socket))
        })
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
    let mut raw_headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let value = line[colon + 1..].trim();
            raw_headers.push(Value::String(line[..colon].trim().to_string()));
            raw_headers.push(Value::String(value.to_string()));
            if let Some((_, existing)) = headers.iter_mut().find(|(name, _)| name == &key) {
                if crate::modules::http_res::NON_REPEATABLE_HEADERS
                    .iter()
                    .any(|name| *name == key)
                {
                    continue;
                }
                if key == "set-cookie" {
                    let updated = match existing {
                        Value::Array(_) => {
                            let mut values = execute::own_enumerable_keys(existing)
                                .into_iter()
                                .map(|key| execute::get_property(existing, &key))
                                .collect::<Vec<_>>();
                            values.push(Value::String(value.to_string()));
                            host_api::array(values)
                        }
                        Value::String(previous) => host_api::array(vec![
                            Value::String(previous.clone()),
                            Value::String(value.to_string()),
                        ]),
                        _ => Value::String(value.to_string()),
                    };
                    *existing = updated;
                } else if let Value::String(previous) = existing {
                    previous.push_str(", ");
                    previous.push_str(value);
                }
            } else if key == "set-cookie" {
                headers.push((key, host_api::array(vec![Value::String(value.to_string())])));
            } else {
                headers.push((key, Value::String(value.to_string())));
            }
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
    let request_alias = match request_value.clone() {
        Value::Object(object) => Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(
            Rc::downgrade(&object),
        )))),
        other => other,
    };
    if !matches!(request_value, Value::Undefined | Value::Null) {
        let domain = execute::get_property(&request_value, "domain");
        if !matches!(domain, Value::Undefined | Value::Null) {
            execute::set_property_in_place(&res, "domain", domain);
        }
    }
    let props = vec![
        ("statusCode".to_string(), Value::Number(status as f64)),
        ("statusMessage".to_string(), Value::String(message)),
        ("httpVersion".to_string(), Value::String("1.1".to_string())),
        ("headers".to_string(), host_api::object(headers)),
        ("rawHeaders".to_string(), host_api::array(raw_headers)),
        ("req".to_string(), request_alias),
        (
            RESPONSE_READ_BUFFER_PROP.to_string(),
            host_api::array(Vec::new()),
        ),
        (
            "setEncoding".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_ENCODING),
        ),
        (
            "read".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_READ),
        ),
        (
            "pipe".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_PIPE),
        ),
        (
            "setTimeout".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_TIMEOUT),
        ),
        (
            "destroy".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_INCOMING_DESTROY),
        ),
        // Resuming an IncomingMessage only switches it into flowing mode;
        // the host already drains response bytes eagerly, so the same
        // identity-preserving capability is sufficient here.
        (
            "pause".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_ENCODING),
        ),
        (
            "resume".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_ENCODING),
        ),
        (
            "signal".to_string(),
            crate::modules::http::new_http_signal(state)?,
        ),
        ("complete".to_string(), Value::Boolean(false)),
        ("readable".to_string(), Value::Boolean(true)),
        ("aborted".to_string(), Value::Boolean(false)),
        ("destroyed".to_string(), Value::Boolean(false)),
        ("errored".to_string(), Value::Null),
        ("closed".to_string(), Value::Boolean(false)),
        (RES_ASYNC_RESOURCE_PROP.to_string(), request_resource),
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
        Some(Value::StringUnits(url)) => http_url(&String::from_utf16_lossy(url)),
        Some(Value::Object(_) | Value::ObjectAlias(_)) => {
            let options = value.cloned().unwrap_or(Value::Undefined);
            validate_agent_option(&options)?;
            let parser_option = execute::get_property(&options, "insecureHTTPParser");
            if !matches!(
                parser_option,
                Value::Undefined | Value::Null | Value::Boolean(_)
            ) {
                return Err(invalid_boolean_option_error(
                    "options.insecureHTTPParser",
                    &parser_option,
                ));
            }
            for key in ["host", "hostname"] {
                if own_option(&options, key) {
                    let value = execute::get_property(&options, key);
                    if !matches!(value, Value::String(_) | Value::StringUnits(_)) {
                        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                            "The options.{key} property must be of type string"
                        )));
                    }
                }
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
                        || !matches!(
                            host_header,
                            Value::Undefined | Value::Null | Value::String(_)
                        )
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
                            if name
                                .as_deref()
                                .is_some_and(|key| key.eq_ignore_ascii_case("cookie"))
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
                .map_or(RequestTarget::Tcp { host, port }, |path| {
                    RequestTarget::Unix { path }
                });
            let secure = matches!(execute::get_property(&options, "protocol"), Value::String(ref p) if p.eq_ignore_ascii_case("https:"));
            Ok(RequestOptions {
                target,
                method,
                path,
                headers,
                secure,
                tls_options: Some(options),
            })
        }
        _ => Err(execute::type_error("options must be a string or object")),
    }
}

fn unescaped_path_error() -> VmError {
    let error = execute::type_error("Request path contains unescaped characters");
    let error = match error {
        VmError::Thrown(value) => execute::set_property(
            execute::set_property(
                value,
                "code",
                Value::String("ERR_UNESCAPED_CHARACTERS".into()),
            ),
            "name",
            Value::String("TypeError".into()),
        ),
        other => return other,
    };
    VmError::Thrown(error)
}

fn validate_agent_option(options: &Value) -> Result<(), VmError> {
    let agent = execute::get_property(options, "agent");
    let valid = matches!(
        agent,
        Value::Undefined | Value::Null | Value::Boolean(false)
    ) || matches!(agent, Value::Object(_) | Value::ObjectAlias(_))
        && (matches!(
            execute::get_property(&agent, AGENT_MARKER_PROP),
            Value::Boolean(true)
        ) || ["addRequest", "createConnection", "createSocket"]
            .into_iter()
            .any(|name| quench_runtime::is_callable(&execute::get_property(&agent, name))));
    if valid {
        return Ok(());
    }
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"options.agent\" property must be one of Agent-like Object, undefined, or false.{}",
        crate::modules::util::invalid_arg_received(&agent)
    )))
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
    let error = execute::type_error(&format!("Method must be a valid HTTP token [\"{method}\"]"));
    let error = match error {
        VmError::Thrown(value) => execute::set_property(
            execute::set_property(
                value,
                "code",
                Value::String("ERR_INVALID_HTTP_TOKEN".into()),
            ),
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
        && method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
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
        output.push(if chunk.len() > 2 {
            TABLE[third & 63] as char
        } else {
            '='
        });
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
    // Prefer own options before consulting the prototype chain. This is
    // observable for null-prototype option bags and avoids invoking an
    // unrelated inherited accessor when `host` already supplies a value.
    for key in keys.iter().copied().filter(|key| own_option(options, key)) {
        if let Some(value) = opt(options, key)? {
            if !value.is_empty() {
                return Ok(Some(value));
            }
        }
    }
    Ok(None)
}

fn own_option(options: &Value, key: &str) -> bool {
    execute::get_own_property_descriptor(options, key)
        .is_ok_and(|descriptor| !matches!(descriptor, Value::Undefined))
}

fn http_url(value: &str) -> Result<RequestOptions, VmError> {
    let secure = value.starts_with("https://");
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .unwrap_or(value);
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
        secure,
        tls_options: None,
    })
}

fn option_source_object(args: &[Value]) -> Option<Value> {
    args.first().and_then(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_)).then(|| value.clone())
    })
}
