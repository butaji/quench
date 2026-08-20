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
pub(crate) const RES_ID_PROP: &str = "\0quench:http:res:id";

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
    /// The parsed `req` value, while this connection streams a request body.
    pub req: Option<Value>,
    /// Bytes of the request body still expected (from Content-Length).
    pub body_remaining: usize,
    /// Whether the request body fully arrived (`'end'` was emitted).
    pub body_done: bool,
    /// Whether the request head has been consumed.
    pub head_parsed: bool,
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
            req: None,
            body_remaining: 0,
            body_done: true,
            head_parsed: false,
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

/// `'data'` handler: buffer bytes, then parse the head and stream body.
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
    feed_conn(state, socket_id)
}

/// Try to consume a head, then stream whatever body bytes are buffered.
fn feed_conn(state: &Rc<RefCell<HostState>>, socket_id: u64) -> Result<Value, VmError> {
    if !conn_has_head(state, socket_id) {
        if let Some(head) = take_head(state, socket_id) {
            emit_request(state, socket_id, &head)?;
        }
    }
    drain_body(state, socket_id)?;
    Ok(Value::Undefined)
}

fn conn_has_head(state: &Rc<RefCell<HostState>>, socket_id: u64) -> bool {
    state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .map(|conn| conn.head_parsed)
        .unwrap_or(false)
}

/// Extract a buffered request head terminator, leaving the body behind.
fn take_head(state: &Rc<RefCell<HostState>>, socket_id: u64) -> Option<Vec<u8>> {
    let mut guard = state.borrow_mut();
    let conn = guard.http.conns.get_mut(&socket_id)?;
    let (idx, len) = head_end(&conn.buffer)?;
    let head = conn.buffer[..idx].to_vec();
    conn.buffer.drain(..idx + len);
    conn.head_parsed = true;
    Some(head)
}

/// Emit buffered request-body bytes as `'data'`, then `'end'` once the
/// declared Content-Length arrives.
fn drain_body(state: &Rc<RefCell<HostState>>, socket_id: u64) -> Result<(), VmError> {
    let (req, data, done) = {
        let mut guard = state.borrow_mut();
        let Some(conn) = guard.http.conns.get_mut(&socket_id) else {
            return Ok(());
        };
        if conn.req.is_none() || conn.body_done {
            return Ok(());
        }
        let take = conn.buffer.len().min(conn.body_remaining);
        let data = conn.buffer[..take].to_vec();
        conn.buffer.drain(..take);
        conn.body_remaining -= take;
        (conn.req.clone(), data, conn.body_remaining == 0)
    };
    if let Some(req) = req {
        if !data.is_empty() {
            net::emit(state, &req, "data", vec![make_buffer(&data)])?;
        }
        if done {
            if let Some(conn) = state.borrow_mut().http.conns.get_mut(&socket_id) {
                conn.body_done = true;
            }
            net::emit(state, &req, "end", Vec::new())?;
        }
    }
    Ok(())
}

fn make_buffer(bytes: &[u8]) -> Value {
    crate::modules::buffer_proto::make_buffer(bytes)
}

pub(crate) fn chunk_bytes(value: Option<&Value>) -> Vec<u8> {
    match value {
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Uint8Array(view)) => {
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        _ => Vec::new(),
    }
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
    let (req, content_length) = build_req(state, head)?;
    let (res, id) = build_res_object(state)?;
    insert_response(state, socket_id, id);
    {
        let mut guard = state.borrow_mut();
        if let Some(conn) = guard.http.conns.get_mut(&socket_id) {
            conn.req = Some(req.clone());
            conn.body_remaining = content_length;
            conn.body_done = content_length == 0;
        }
    }
    Ok((req, res))
}

/// Build the `req` emitter from the parsed head.
fn build_req(state: &Rc<RefCell<HostState>>, head: &[u8]) -> Result<(Value, usize), VmError> {
    let (method, url, version, headers, content_length) = parse_request_head(head);
    let req = crate::modules::events::new_emitter_object(state)?;
    let req = install_req_props(
        req,
        vec![
            ("method".to_string(), Value::String(method)),
            ("url".to_string(), Value::String(url)),
            ("httpVersion".to_string(), Value::String(version)),
            ("headers".to_string(), host_api::object(headers)),
        ],
    )?;
    Ok((req, content_length))
}

/// Register a fresh response for the socket.
fn insert_response(state: &Rc<RefCell<HostState>>, socket_id: u64, id: u64) {
    let socket = state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .map(|conn| conn.socket.clone())
        .unwrap_or(Value::Undefined);
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
}

fn install_req_props(mut object: Value, props: Vec<(String, Value)>) -> Result<Value, VmError> {
    for (key, value) in props {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        object = execute::define_property(object, &key, descriptor)?;
    }
    Ok(object)
}

/// Parse the request line and headers; return the declared Content-Length.
fn parse_request_head(head: &[u8]) -> (String, String, String, Vec<(String, Value)>, usize) {
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
    let mut content_length = 0usize;
    for line in parts.iter().skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let value = line[colon + 1..].trim();
            if key == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((key, Value::String(value.to_string())));
        }
    }
    (method, url, version.to_string(), headers, content_length)
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

// Response methods live in `http_res`; re-exported here for dispatch.
pub use crate::modules::http_res::{res_end, res_set_header, res_write, res_write_head};

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
