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
const RESPONSE_ENCODING_PROP: &str = "\0quench:http:res:encoding";

/// `(host, port, method, path, headers)` for one outbound request.
type RequestOptions = (String, u16, String, String, Vec<(String, String)>);

/// One outbound HTTP request, keyed by `CLIENT_ID_PROP`.
pub struct ClientReq {
    pub host: String,
    pub port: u16,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub req: Value,
    pub socket: Option<Value>,
    pub buffer: Vec<u8>,
    pub res: Option<Value>,
    pub head_parsed: bool,
}

pub fn agent_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::object(Vec::new()))
}

pub fn agent_construct(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(host_api::object(Vec::new()))
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
    let (req, id) = build_req_object(state)?;
    let mut guard = state.borrow_mut();
    guard.http.clientreqs.insert(
        id,
        ClientReq {
            host: opts.0,
            port: opts.1,
            method: opts.2,
            path: opts.3,
            headers: opts.4,
            body: Vec::new(),
            req: req.clone(),
            socket: None,
            buffer: Vec::new(),
            res: None,
            head_parsed: false,
        },
    );
    drop(guard);
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
    let (host, port, method, path, headers, body) = {
        let guard = state.borrow();
        let Some(req) = guard.http.clientreqs.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        (
            req.host.clone(),
            req.port,
            req.method.clone(),
            req.path.clone(),
            req.headers.clone(),
            req.body.clone(),
        )
    };
    let head = request_head(&host, &method, &path, &headers, body.len());
    if let Some(req) = state.borrow().http.clientreqs.get(&id) {
        let updated =
            execute::set_property(req.req.clone(), "_header", Value::String(head.clone()));
        execute::replace_value(&req.req, &updated);
    }
    let socket = send_request(state, &host, port, &method, &path, &headers, &body)?;
    let socket_id = net::net_id(&socket);
    let mut guard = state.borrow_mut();
    if let Some(req) = guard.http.clientreqs.get_mut(&id) {
        req.socket = Some(socket.clone());
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
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Value, VmError> {
    let socket = net::connect(
        state,
        &[Value::Number(port as f64), Value::String(host.to_string())],
    )?;
    let head = request_head(host, method, path, headers, body.len());
    let mut payload = head.into_bytes();
    payload.extend_from_slice(body);
    let payload = host_api::bytes(&payload);
    net::socket_write(state, Some(&socket), std::slice::from_ref(&payload))?;
    Ok(socket)
}

fn request_head(
    host: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body_len: usize,
) -> String {
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
    for (key, value) in headers {
        head.push_str(&format!("{key}: {value}\r\n"));
    }
    if !headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    {
        head.push_str(&format!("Content-Length: {body_len}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    head
}

fn subscribe_socket(state: &Rc<RefCell<HostState>>, socket: &Value) -> Result<(), VmError> {
    let data_cap = crate::host::capability(crate::registry::SPEC_HTTP_RESDATA);
    crate::modules::events::method_on(
        state,
        Some(socket),
        &[Value::String("data".to_string()), data_cap],
    )?;
    let end_cap = crate::host::capability(crate::registry::SPEC_HTTP_RESEND);
    crate::modules::events::method_on(
        state,
        Some(socket),
        &[Value::String("end".to_string()), end_cap],
    )?;
    let close_cap = crate::host::capability(crate::registry::SPEC_HTTP_REQCLOSE);
    crate::modules::events::method_on(
        state,
        Some(socket),
        &[Value::String("close".to_string()), close_cap],
    )?;
    Ok(())
}

/// Socket close completes the corresponding ClientRequest lifecycle.
pub fn req_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket_id) = receiver.and_then(net::net_id) else {
        return Ok(Value::Undefined);
    };
    let Some(client_id) = state.borrow().http.clients.get(&socket_id).copied() else {
        return Ok(Value::Undefined);
    };
    let request = state
        .borrow()
        .http
        .clientreqs
        .get(&client_id)
        .map(|req| req.req.clone());
    if let Some(request) = request {
        net::emit(state, &request, "close", Vec::new())?;
    }
    Ok(Value::Undefined)
}

/// Response parser: buffer bytes, then stream `'data'` chunks.
pub fn data_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket_id) = receiver.and_then(net::net_id) else {
        return Ok(Value::Undefined);
    };
    let Some(client_id) = state.borrow().http.clients.get(&socket_id).copied() else {
        return Ok(Value::Undefined);
    };
    let bytes = chunk_bytes(args.first());
    let (pending_head, head_parsed) = append_and_probe(state, client_id, &bytes);
    match pending_head {
        Some(head) => {
            let res = build_incoming(state, &head)?;
            if let Some(req) = state.borrow_mut().http.clientreqs.get_mut(&client_id) {
                req.res = Some(res.clone());
            }
            let req_value = client_value(state, client_id, true).unwrap_or(Value::Undefined);
            net::emit(state, &req_value, "response", vec![res])?;
            flush_body(state, client_id)
        }
        None if head_parsed => {
            if let Some(res) = client_value(state, client_id, false) {
                net::emit(state, &res, "data", vec![response_data(&res, &bytes)])?;
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
        (req.res.clone(), std::mem::take(&mut req.buffer))
    };
    if let Some(res) = res {
        if !rest.is_empty() {
            net::emit(state, &res, "data", vec![response_data(&res, &rest)])?;
        }
    }
    Ok(())
}

/// Socket `'end'` → the response's `'end'`.
pub fn res_end_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(socket_id) = receiver.and_then(net::net_id) else {
        return Ok(Value::Undefined);
    };
    let Some(client_id) = state.borrow().http.clients.get(&socket_id).copied() else {
        return Ok(Value::Undefined);
    };
    let res = {
        let guard = state.borrow();
        guard
            .http
            .clientreqs
            .get(&client_id)
            .and_then(|r| r.res.clone())
    };
    if let Some(res) = res {
        net::emit(state, &res, "end", Vec::new())?;
    }
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
            (CLIENT_ID_PROP.to_string(), Value::Number(id as f64)),
            ("_header".to_string(), Value::String(String::new())),
        ],
    )?;
    Ok((object, id))
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
        // Resuming an IncomingMessage only switches it into flowing mode;
        // the host already drains response bytes eagerly, so the same
        // identity-preserving capability is sufficient here.
        (
            "resume".to_string(),
            crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_ENCODING),
        ),
    ];
    install_methods(res, props)
}

fn request_options(value: Option<&Value>) -> Result<RequestOptions, VmError> {
    match value {
        Some(Value::String(url)) => http_url(url),
        Some(Value::Object(_)) => {
            let options = value.cloned().unwrap_or(Value::Undefined);
            // Legacy url.parse() exposes `host`/`path`, while WHATWG URL
            // exposes `hostname`/`pathname`/`search`; both are request facts.
            let host = opt_first(&options, &["host", "hostname"])?
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let port = opt(&options, "port")?
                .and_then(|p| p.parse().ok())
                .unwrap_or(80);
            let method = opt(&options, "method")?.unwrap_or_else(|| "GET".to_string());
            let path = match opt(&options, "path")? {
                Some(path) => path,
                None => {
                    let pathname = opt(&options, "pathname")?.unwrap_or_else(|| "/".to_string());
                    format!("{pathname}{}", opt(&options, "search")?.unwrap_or_default())
                }
            };
            let mut headers: Vec<(String, String)> = Vec::new();
            if let Ok(hv) = execute::get_property_result(&options, "headers") {
                if matches!(hv, Value::Array(_)) {
                    for key in execute::own_enumerable_keys(&hv) {
                        let Ok(pair) = execute::get_property_result(&hv, &key) else {
                            continue;
                        };
                        let name = execute::get_property_result(&pair, "0")
                            .ok()
                            .and_then(|v| execute::to_js_string(&v).ok());
                        let value = execute::get_property_result(&pair, "1")
                            .ok()
                            .and_then(|v| execute::to_js_string(&v).ok());
                        if let (Some(name), Some(value)) = (name, value) {
                            headers.push((name, value));
                        }
                    }
                } else {
                    for key in execute::own_enumerable_keys(&hv) {
                        let Ok(item) = execute::get_property_result(&hv, &key) else {
                            continue;
                        };
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
            Ok((host, port, method, path, headers))
        }
        _ => Err(execute::type_error("options must be a string or object")),
    }
}

fn opt(options: &Value, key: &str) -> Result<Option<String>, VmError> {
    match execute::get_property_result(options, key)? {
        Value::Undefined => Ok(None),
        other => execute::to_js_string(&other).map(Some),
    }
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
    Ok((host, port, "GET".to_string(), path.to_string(), Vec::new()))
}
