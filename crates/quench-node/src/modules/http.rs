//! `http` module — minimal `http.createServer` over the real `net`
//! layer. Each accepted connection parses one HTTP/1.1 request head,
//! emits `'request'` with a `req`/`res` pair, and `res.end()` writes a
//! response (Content-Length + Connection: close) and closes the socket.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::net;

/// Hidden property mapping a `res` object to its host-side state.
const RES_ID_PROP: &str = "\0quench:http:res:id";

pub struct HttpState {
    next_res: u64,
    pub next_client: u64,
    pub conns: HashMap<u64, Conn>,
    pub res: HashMap<u64, Res>,
    pub clientreqs: HashMap<u64, crate::modules::http_client::ClientReq>,
    /// socket net id -> ClientRequest id.
    pub clients: HashMap<u64, u64>,
}

/// Inbound connection parse state, keyed by socket net id.
pub struct Conn {
    pub server: Value,
    pub socket: Value,
    pub buffer: Vec<u8>,
}

/// One pending response, keyed by `RES_ID_PROP` on the `res` object.
pub struct Res {
    pub status: u16,
    pub text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub socket: Value,
}

impl Default for HttpState {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpState {
    pub fn new() -> Self {
        Self {
            next_res: 1,
            next_client: 1,
            conns: HashMap::new(),
            res: HashMap::new(),
            clientreqs: HashMap::new(),
            clients: HashMap::new(),
        }
    }
}

/// `http.createServer([requestListener])` — a net server object that
/// parses HTTP on each connection and emits `'request'`.
pub fn create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let object = net::create_server(state, &[])?;
    if let Some(cb) = args.first() {
        if quench_runtime::is_callable(cb) {
            crate::modules::events::method_on(
                state,
                Some(&object),
                &[Value::String("request".to_string()), cb.clone()],
            )?;
        }
    }
    // Internal wiring: each accepted connection feeds request parsing.
    let conn_cap = crate::host::capability(crate::registry::NodeSpec::new("http:conn", 0x0F07));
    crate::modules::events::method_on(
        state,
        Some(&object),
        &[Value::String("connection".to_string()), conn_cap],
    )?;
    Ok(object)
}

/// `'connection'` handler: record the socket and subscribe `'data'`.
pub fn connection_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let socket = args.first().cloned().unwrap_or(Value::Undefined);
    let Some(socket_id) = net::net_id(&socket) else {
        return Ok(Value::Undefined);
    };
    state.borrow_mut().http.conns.insert(
        socket_id,
        Conn {
            server: receiver.cloned().unwrap_or(Value::Undefined),
            socket: socket.clone(),
            buffer: Vec::new(),
        },
    );
    let data_cap = crate::host::capability(crate::registry::NodeSpec::new("http:data", 0x0F08));
    crate::modules::events::method_on(
        state,
        Some(&socket),
        &[Value::String("data".to_string()), data_cap],
    )?;
    Ok(Value::Undefined)
}

/// `'data'` handler: buffer bytes and parse a request head.
pub fn data_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket_id) = receiver.and_then(net::net_id) else {
        return Ok(Value::Undefined);
    };
    let bytes = chunk_bytes(args.first());
    {
        let mut guard = state.borrow_mut();
        if let Some(conn) = guard.http.conns.get_mut(&socket_id) {
            conn.buffer.extend_from_slice(&bytes);
        }
    }
    try_parse(state, socket_id)
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

/// If a full request head is buffered, parse and dispatch `'request'`.
fn try_parse(state: &Rc<RefCell<HostState>>, socket_id: u64) -> Result<Value, VmError> {
    let head = {
        let mut guard = state.borrow_mut();
        let Some(conn) = guard.http.conns.get_mut(&socket_id) else {
            return Ok(Value::Undefined);
        };
        match head_end(&conn.buffer) {
            Some((idx, len)) => {
                let head = conn.buffer[..idx].to_vec();
                let rest: Vec<u8> = conn.buffer[idx + len..].to_vec();
                conn.buffer = rest;
                Some(head)
            }
            None => None,
        }
    };
    let Some(head) = head else {
        return Ok(Value::Undefined);
    };
    emit_request(state, socket_id, &head)
}

/// Locate the header/body terminator (`\r\n\r\n` or `\n\n`).
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

/// Build `req`/`res` and emit `'request'` on the serving server.
fn emit_request(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
    head: &[u8],
) -> Result<Value, VmError> {
    let (req, res) = build_req_res(state, socket_id, head)?;
    let server = {
        let guard = state.borrow();
        guard
            .http
            .conns
            .get(&socket_id)
            .map(|conn| conn.server.clone())
            .unwrap_or(Value::Undefined)
    };
    net::emit(state, &server, "request", vec![req, res])?;
    Ok(Value::Undefined)
}

fn build_req_res(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
    head: &[u8],
) -> Result<(Value, Value), VmError> {
    let (method, url, version, headers) = parse_request_head(head);
    let req = host_api::object(vec![
        ("method".to_string(), Value::String(method)),
        ("url".to_string(), Value::String(url)),
        (
            "httpVersion".to_string(),
            Value::String(version.to_string()),
        ),
        ("headers".to_string(), host_api::object(headers)),
    ]);

    let (res, id) = build_res_object(state)?;
    let socket = {
        let guard = state.borrow();
        guard
            .http
            .conns
            .get(&socket_id)
            .map(|conn| conn.socket.clone())
            .unwrap_or(Value::Undefined)
    };
    state.borrow_mut().http.res.insert(
        id,
        Res {
            status: 200,
            text: "OK".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
            socket,
        },
    );
    Ok((req, res))
}

/// Parse the request line and headers from a request head.
fn parse_request_head(head: &[u8]) -> (String, String, String, Vec<(String, Value)>) {
    let head_str = String::from_utf8_lossy(head);
    let parts: Vec<&str> = head_str.split("\r\n").collect();
    let mut fields = parts[0].split_whitespace();
    let method = fields.next().unwrap_or("").to_string();
    let url = fields.next().unwrap_or("/").to_string();
    let version = fields
        .next()
        .and_then(|v| v.strip_prefix("HTTP/"))
        .unwrap_or("1.1");

    let mut headers: Vec<(String, Value)> = Vec::new();
    for line in parts.iter().skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let value = line[colon + 1..].trim();
            headers.push((key, Value::String(value.to_string())));
        }
    }
    (method, url, version.to_string(), headers)
}

fn build_res_object(state: &Rc<RefCell<HostState>>) -> Result<(Value, u64), VmError> {
    let id = {
        let mut guard = state.borrow_mut();
        let id = guard.http.next_res;
        guard.http.next_res += 1;
        id
    };
    let res = host_api::object(vec![
        (
            "setHeader".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_SET_HEADER),
        ),
        (
            "writeHead".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE_HEAD),
        ),
        (
            "write".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE),
        ),
        (
            "end".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_END),
        ),
        ("statusCode".to_string(), Value::Number(200.0)),
        (RES_ID_PROP.to_string(), Value::Number(id as f64)),
    ]);
    Ok((res, id))
}

fn res_cap(spec: crate::registry::NodeSpec) -> Value {
    crate::host::capability(spec)
}

// ---- response methods ----

fn res_state(receiver: Option<&Value>) -> Option<u64> {
    let receiver = receiver?;
    match quench_runtime::vm::get_property(receiver, RES_ID_PROP) {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as u64),
        _ => None,
    }
}

/// `res.setHeader(name, value)` — replace any existing header of that name.
pub fn res_set_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(Value::Undefined);
    };
    let name = args.first().map(execute::to_js_string).transpose()?;
    let value = args.get(1).map(execute::to_js_string).transpose()?;
    let Some((name, value)) = name.zip(value) else {
        return Ok(Value::Undefined);
    };
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.headers.retain(|(key, _)| key != &name);
        res.headers.push((name, value));
    }
    Ok(Value::Undefined)
}

/// `res.writeHead(statusCode[, reasonPhrase][, headers])`.
pub fn res_write_head(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(Value::Undefined);
    };
    let status = args.first().and_then(number).unwrap_or(200).clamp(100, 599);
    let mut text = String::new();
    if let Some(Value::String(s)) = args.get(1) {
        text = s.clone();
    } else if let Some(obj) = args.get(1) {
        if matches!(obj, Value::Object(_)) {
            let mut guard = state.borrow_mut();
            if let Some(res) = guard.http.res.get_mut(&id) {
                res.status = status;
                merge_headers(res, obj)?;
            }
            return Ok(Value::Undefined);
        }
    }
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.status = status;
        if !text.is_empty() {
            res.text = text;
        }
    }
    Ok(Value::Undefined)
}

fn merge_headers(res: &mut Res, object: &Value) -> Result<(), VmError> {
    res.headers.clear();
    for key in execute::own_enumerable_keys(object) {
        if let Ok(item) = execute::get_property_result(object, &key) {
            if let Ok(value) = execute::to_js_string(&item) {
                res.headers.push((key, value));
            }
        }
    }
    Ok(())
}

/// `res.write(chunk)` — buffer a body fragment.
pub fn res_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(Value::Undefined);
    };
    let bytes = if matches!(args.first(), Some(Value::Undefined)) {
        Vec::new()
    } else {
        let value = args
            .first()
            .ok_or_else(|| execute::type_error("chunk required"))?;
        chunk_bytes(Some(value))
    };
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.body.extend_from_slice(&bytes);
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `res.end([chunk])` — compose and send the response, then close the
/// socket.
pub fn res_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    if let Some(data) = args.first() {
        if !matches!(data, Value::Undefined) {
            res_write(state, receiver, std::slice::from_ref(data))?;
        }
    }
    let (status, text, headers, body, socket) = {
        let guard = state.borrow();
        let Some(res) = guard.http.res.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        (
            res.status,
            res.text.clone(),
            res.headers.clone(),
            res.body.clone(),
            res.socket.clone(),
        )
    };
    let status = status_code(receiver, status);
    let payload = host_api::bytes(&compose(status, &text, &headers, &body));
    crate::modules::net::socket_write(state, Some(&socket), std::slice::from_ref(&payload))?;
    crate::modules::net::socket_end(state, Some(&socket), &[])?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// The effective status code, honoring a `res.statusCode = n` write.
fn status_code(receiver: Option<&Value>, default: u16) -> u16 {
    let Some(receiver) = receiver else {
        return default;
    };
    match quench_runtime::vm::get_property(receiver, "statusCode") {
        Value::Number(n) if n.is_finite() && (100.0..600.0).contains(&n) => n as u16,
        _ => default,
    }
}

/// Serialize an HTTP/1.1 response.
fn compose(status: u16, text: &str, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let text = if text.is_empty() { "OK" } else { text };
    let mut out = format!("HTTP/1.1 {status} {text}\r\n").into_bytes();
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    for (key, value) in headers {
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(body);
    out
}

fn number(value: &Value) -> Option<u16> {
    match value {
        Value::Number(n) if n.is_finite() => Some(*n as u16),
        _ => None,
    }
}

/// `http.request(options[, cb])` — an outbound ClientRequest.
pub fn request(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http_client::request(state, args)
}

/// `http.get(options[, cb])` — `request` with method GET, auto-ended.
pub fn get(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http_client::get(state, args)
}

/// The `http` module namespace.
pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "createServer",
            crate::host::capability(crate::registry::SPEC_HTTP_SERVER),
        ),
        (
            "request",
            crate::host::capability(crate::registry::SPEC_HTTP_REQUEST),
        ),
        (
            "get",
            crate::host::capability(crate::registry::SPEC_HTTP_GET),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
