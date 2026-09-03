//! `http` module — minimal `http.createServer` over the real `net`
//! layer. Each accepted connection parses one HTTP/1.1 request head,
//! emits `'request'` with a `req`/`res` pair, and `res.end()` writes a
//! response (Content-Length + Connection: close) and closes the socket.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::net;

/// Hidden property mapping a `res` object to its host-side state.
pub(crate) const RES_ID_PROP: &str = "\0quench:http:res:id";
const REQ_ENCODING_PROP: &str = "\0quench:http:res:encoding";
pub(crate) const REQ_ASYNC_RESOURCE_PROP: &str = "\0quench:http:req:async-resource";
pub(crate) const REQ_CLOSE_PROP: &str = "\0quench:http:req:close";
pub(crate) const HTTP_SERVER_SOCKET_PROP: &str = "\0quench:http:server-socket";
const CONNECTIONS_CHECKING_INTERVAL_PROP: &str = "\0quench:http:connections-checking-interval";
pub(crate) const INCOMING_CLOSE_PENDING_PROP: &str = "\0quench:http:incoming:close-pending";
/// Hidden response transition bit; `ServerResponse` emits `close` once after
/// `end()` even when the HTTP/1.1 socket remains keep-alive.
pub(crate) const RESPONSE_CLOSE_PENDING_PROP: &str = "\0quench:http:response:close-pending";
const REQUIRE_HOST_HEADER_PROP: &str = "\0quench:http:require-host";
const SERVER_RESPONSE_PROP: &str = "\0quench:http:server-response";
const SERVER_REQUEST_PROP: &str = "\0quench:http:server-request";

pub struct HttpState {
    next_res: u64,
    pub next_client: u64,
    pub conns: HashMap<u64, Conn>,
    pub res: HashMap<u64, Res>,
    pub server_responses: HashMap<u64, Value>,
    pub server_requests: HashMap<u64, Value>,
    pub clientreqs: HashMap<u64, crate::modules::http_client::ClientReq>,
    /// socket net id -> ClientRequest id.
    pub clients: HashMap<u64, u64>,
    /// AbortSignal target id -> ClientRequest id.
    pub client_signals: HashMap<u64, u64>,
    /// Requests deferred by an Agent's maxSockets limit, in submission order.
    pub agent_pending: Vec<u64>,
    pub global_agent: Option<Value>,
    pub agent_prototype: Option<Value>,
    pub client_request_prototype: Option<Value>,
    pub outgoing_prototype: Option<Value>,
    /// HTTP agent sockets which are idle and must reject unsolicited bytes.
    /// Keep this host-side so the parser-detached guard does not add public
    /// stream listeners or alter observable listener counts.
    pub idle_sockets: HashSet<u64>,
}

/// Inbound connection parse state, keyed by socket net id.
pub struct Conn {
    pub server: Value,
    pub socket: Value,
    pub buffer: Vec<u8>,
    /// The parsed `req` value, while this connection streams a request body.
    pub req: Option<Value>,
    /// Every request identity observed on this connection. Keep-alive replaces
    /// `req` for parsing, but close/destroy must still reach each message.
    pub requests: Vec<Value>,
    /// Bytes of the request body still expected (from Content-Length).
    pub body_remaining: usize,
    /// Whether the request body uses HTTP chunked framing.
    pub body_chunked: bool,
    pub body_chunked_done: bool,
    /// Whether the request body fully arrived (`'end'` was emitted).
    pub body_done: bool,
    /// Whether the request head has been consumed.
    pub head_parsed: bool,
    /// Whether the response already ended (request handler called res.end).
    pub response_done: bool,
    /// Whether the peer keeps this connection alive for the next request.
    pub keep_alive: bool,
    pub require_host_header: bool,
}

/// One pending response, keyed by `RES_ID_PROP` on the `res` object.
pub struct Res {
    pub status: u16,
    pub text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub socket: Value,
    pub keep_alive: bool,
    pub http10: bool,
    pub headers_sent: bool,
    pub sent_body: usize,
    pub chunked: bool,
    pub ended: bool,
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
            server_responses: HashMap::new(),
            server_requests: HashMap::new(),
            clientreqs: HashMap::new(),
            clients: HashMap::new(),
            client_signals: HashMap::new(),
            agent_pending: Vec::new(),
            global_agent: None,
            agent_prototype: None,
            client_request_prototype: None,
            outgoing_prototype: None,
            idle_sockets: HashSet::new(),
        }
    }
}

pub(crate) fn mark_idle_socket(state: &Rc<RefCell<HostState>>, socket: &Value) {
    if let Some(id) = net::net_id(socket) {
        state.borrow_mut().http.idle_sockets.insert(id);
    }
}

pub(crate) fn clear_idle_socket(state: &Rc<RefCell<HostState>>, socket: &Value) {
    if let Some(id) = net::net_id(socket) {
        state.borrow_mut().http.idle_sockets.remove(&id);
    }
}

pub(crate) fn is_idle_socket(state: &Rc<RefCell<HostState>>, socket: &Value) -> bool {
    net::net_id(socket).is_some_and(|id| state.borrow().http.idle_sockets.contains(&id))
}

/// `http.createServer([requestListener])` — a net server object that
/// parses HTTP on each connection and emits `'request'`.
pub fn create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let object = net::create_server(state, &[])?;
    let (options, callback) = match args.first() {
        None => (None, None),
        Some(value) if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) => {
            (Some(value), args.get(1))
        }
        Some(value) if quench_runtime::is_callable(value) => (None, Some(value)),
        Some(_) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"options\" argument must be an object or a function".into(),
            ))
        }
    };
    let require_host_header = options
        .map(|value| {
            !matches!(
                execute::get_property(value, "requireHostHeader"),
                Value::Boolean(false)
            )
        })
        .unwrap_or(true);
    if let Some(options) = options {
        if let Value::Object(_) | Value::ObjectAlias(_) = options {
            let constructor = execute::get_property(options, "ServerResponse");
            if quench_runtime::is_callable(&constructor) {
                execute::set_property_in_place(&object, SERVER_RESPONSE_PROP, constructor);
                let server_id = state
                    .borrow()
                    .net
                    .servers
                    .iter()
                    .find(|(_, server)| execute::same_identity(&server.borrow().js, &object))
                    .map(|(id, _)| *id);
                if let Some(server_id) = server_id {
                    state
                        .borrow_mut()
                        .http
                        .server_responses
                        .insert(server_id, execute::get_property(options, "ServerResponse"));
                }
            }
            let request_constructor = execute::get_property(options, "IncomingMessage");
            if quench_runtime::is_callable(&request_constructor) {
                execute::set_property_in_place(&object, SERVER_REQUEST_PROP, request_constructor);
                let server_id = state
                    .borrow()
                    .net
                    .servers
                    .iter()
                    .find(|(_, server)| execute::same_identity(&server.borrow().js, &object))
                    .map(|(id, _)| *id);
                if let Some(server_id) = server_id {
                    state
                        .borrow_mut()
                        .http
                        .server_requests
                        .insert(server_id, execute::get_property(options, "IncomingMessage"));
                }
            }
        }
    }
    // This is host-owned metadata on the same server identity. Mutate the
    // existing object so method receivers and the net registry stay aligned.
    execute::set_property_in_place(
        &object,
        REQUIRE_HOST_HEADER_PROP,
        Value::Boolean(require_host_header),
    );
    // Node exposes the private connection-check timer through
    // `_http_server.kConnectionsCheckingInterval`. Keep the key sourced from
    // the realm's single bootstrap symbol so Rust and JS observe one identity.
    let interval = host_api::object(vec![("_destroyed".into(), Value::Boolean(false))]);
    execute::set_property_in_place(
        &object,
        CONNECTIONS_CHECKING_INTERVAL_PROP,
        interval.clone(),
    );
    let global = quench_runtime::vm::current_global_object();
    if let Value::String(key) =
        execute::get_property(&global, "__nodeHttpConnectionsCheckingInterval")
    {
        let _ = execute::set_property_in_place(&object, &key, interval);
    }
    if let Some(cb) = callback {
        if quench_runtime::is_callable(cb) {
            crate::modules::events::method_on(
                state,
                Some(&object),
                &[Value::String("request".to_string()), cb.clone()],
            )?;
        }
    }
    // Internal wiring: each accepted connection feeds request parsing.
    let conn_cap = crate::host::capability(crate::registry::SPEC_HTTP_CONN);
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
    execute::set_property_in_place(&socket, HTTP_SERVER_SOCKET_PROP, Value::Boolean(true));
    let require_host_header = receiver
        .map(|server| {
            matches!(
                execute::get_property(server, REQUIRE_HOST_HEADER_PROP),
                Value::Boolean(true)
            )
        })
        .unwrap_or(true);
    let allow_half_open = receiver.is_some_and(|server| {
        matches!(
            execute::get_property(server, "httpAllowHalfOpen"),
            Value::Boolean(true)
        )
    });
    execute::set_property_in_place(&socket, "allowHalfOpen", Value::Boolean(allow_half_open));
    state.borrow_mut().http.conns.insert(
        socket_id,
        Conn {
            server: receiver.cloned().unwrap_or(Value::Undefined),
            socket: socket.clone(),
            buffer: Vec::new(),
            req: None,
            requests: Vec::new(),
            body_remaining: 0,
            body_chunked: false,
            body_chunked_done: false,
            body_done: true,
            head_parsed: false,
            response_done: false,
            keep_alive: true,
            require_host_header,
        },
    );
    let data_cap = crate::host::capability(crate::registry::SPEC_HTTP_DATA);
    crate::modules::events::method_on(
        state,
        Some(&socket),
        &[Value::String("data".to_string()), data_cap],
    )?;
    let close_cap = crate::host::capability(crate::registry::SPEC_HTTP_REQCLOSE);
    crate::modules::events::method_on(
        state,
        Some(&socket),
        &[Value::String("close".to_string()), close_cap],
    )?;
    Ok(Value::Undefined)
}

/// Stop keep-alive reuse for established HTTP connections when the server is
/// closing. Existing responses still drain, then `res.end` closes the socket.
pub(crate) fn server_close(state: &Rc<RefCell<HostState>>, server: &Value) {
    let interval = execute::get_property(server, CONNECTIONS_CHECKING_INTERVAL_PROP);
    if matches!(interval, Value::Object(_) | Value::ObjectAlias(_)) {
        let _ = execute::set_property_in_place(&interval, "_destroyed", Value::Boolean(true));
    }
    let global = quench_runtime::vm::current_global_object();
    if let Value::String(key) =
        execute::get_property(&global, "__nodeHttpConnectionsCheckingInterval")
    {
        let interval = execute::get_property(server, &key);
        if matches!(interval, Value::Object(_) | Value::ObjectAlias(_)) {
            let _ = execute::set_property_in_place(&interval, "_destroyed", Value::Boolean(true));
        }
    }
    let mut guard = state.borrow_mut();
    let server_id = net::net_id(server);
    let socket_ids: std::collections::HashSet<u64> = guard
        .http
        .conns
        .iter_mut()
        .filter_map(|(id, conn)| {
            if execute::same_identity(&conn.server, server)
                || server_id.is_some_and(|value| net::net_id(&conn.server) == Some(value))
            {
                conn.keep_alive = false;
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    for response in guard.http.res.values_mut() {
        if net::net_id(&response.socket).is_some_and(|id| socket_ids.contains(&id)) {
            response.keep_alive = false;
        }
    }
    let idle_sockets = guard
        .http
        .res
        .values()
        .filter(|response| {
            response.ended
                && net::net_id(&response.socket)
                    .is_some_and(|id| socket_ids.contains(&id))
        })
        .map(|response| response.socket.clone())
        .collect::<Vec<_>>();
    drop(guard);
    for socket in idle_sockets {
        let _ = net::socket_destroy(state, Some(&socket), &[]);
    }
}

/// Complete every server-side IncomingMessage when its transport closes.
/// The connection owns the request identities, so teardown is emitted once
/// per request and each async resource is destroyed after its observers run.
pub(crate) fn connection_close(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
) -> Result<(), VmError> {
    let Some(socket_id) = net::net_id(socket) else {
        return Ok(());
    };
    // Finalization removes the connection record before the socket's public
    // `close` listeners run. Transition an incomplete request first so its
    // `aborted` event remains observable exactly once.
    abort_server_signal(state, socket)?;
    let requests = state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .map(|conn| conn.requests.clone())
        .unwrap_or_default();
    for request in requests {
        if matches!(
            execute::get_property(&request, REQ_CLOSE_PROP),
            Value::Boolean(true)
        ) {
            continue;
        }
        execute::set_property_in_place(&request, REQ_CLOSE_PROP, Value::Boolean(true));
        net::emit(state, &request, "close", Vec::new())?;
        let resource = execute::get_property(&request, REQ_ASYNC_RESOURCE_PROP);
        crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
    }
    state.borrow_mut().http.conns.remove(&socket_id);
    Ok(())
}

/// Finish deferred `IncomingMessage.destroy()` transport teardown after the
/// queued message listeners have run. A close listener may still produce a
/// half-open response, so the socket decision belongs to this post-dispatch
/// edge rather than the destroy method itself.
pub(crate) fn finalize_destroyed_requests(
    state: &Rc<RefCell<HostState>>,
) -> Result<(), VmError> {
    let sockets = state
        .borrow()
        .http
        .conns
        .values()
        .filter(|conn| {
            conn.req.as_ref().is_some_and(|request| {
                matches!(execute::get_property(request, "destroyed"), Value::Boolean(true))
            }) && !conn.response_done
        })
        .map(|conn| conn.socket.clone())
        .collect::<Vec<_>>();
    for socket in sockets {
        net::socket_destroy(state, Some(&socket), &[])?;
    }
    Ok(())
}

/// Allocate the one internal AbortSignal representation used by HTTP
/// IncomingMessage instances. The signal's own `aborted` property is the
/// canonical mutable fact; callers only retain the object identity.
pub(crate) fn new_http_signal(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    let signal = crate::modules::event_target::new_target(state, &[])?;
    let signal = execute::set_property(
        signal,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    let signal = execute::set_property(signal, "aborted", Value::Boolean(false));
    let signal = execute::set_property(signal, "reason", Value::Undefined);
    let signal = execute::set_property(
        signal,
        "throwIfAborted",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_THROW_IF_ABORTED),
    );
    Ok(execute::set_property(
        signal,
        "Symbol.toStringTag",
        Value::String("AbortSignal".into()),
    ))
}

/// Transition an HTTP signal exactly once and deliver its observable abort
/// notification. EventTarget listeners and the `onabort` property share the
/// same transition, so no subsystem invents a second abort state.
pub(crate) fn abort_http_signal(
    state: &Rc<RefCell<HostState>>,
    signal: &Value,
) -> Result<(), VmError> {
    if matches!(
        execute::get_property(signal, "aborted"),
        Value::Boolean(true)
    ) {
        return Ok(());
    }
    execute::set_property_in_place(signal, "aborted", Value::Boolean(true));
    let event = host_api::object(vec![("type".into(), Value::String("abort".into()))]);
    crate::modules::event_target::dispatch_event(state, Some(signal), &[event])?;
    Ok(())
}

/// Abort an in-flight server request when its socket is destroyed. A normal
/// completed response deliberately leaves the signal non-aborted.
pub(crate) fn abort_server_signal(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
) -> Result<(), VmError> {
    let Some(socket_id) = net::net_id(socket) else {
        return Ok(());
    };
    let request = {
        let guard = state.borrow();
        guard
            .http
            .conns
            .get(&socket_id)
            .and_then(|conn| (!conn.response_done).then(|| conn.req.clone()))
            .flatten()
    };
    if let Some(request) = request {
        if !matches!(
            execute::get_property(&request, "aborted"),
            Value::Boolean(true)
        ) {
            execute::set_property_in_place(&request, "aborted", Value::Boolean(true));
            let signal = execute::get_property(&request, "signal");
            if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
                abort_http_signal(state, &signal)?;
            }
            net::emit(state, &request, "aborted", Vec::new())?;
        }
    }
    Ok(())
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
    // A keep-alive peer sending a bare empty line after a completed response
    // is idle protocol traffic. Node's timeout path closes this connection;
    // ending it here preserves the same observable close/close-server order.
    let idle_empty_line = state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .is_some_and(|conn| conn.response_done && conn.buffer.as_slice() == b"\r\n");
    if idle_empty_line {
        if let Some(socket) = state
            .borrow()
            .net
            .sockets
            .get(&socket_id)
            .map(|socket| socket.borrow().js.clone())
        {
            net::socket_destroy(state, Some(&socket), &[])?;
        }
        return Ok(Value::Undefined);
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
    reset_keep_alive_conn(state, socket_id);
    Ok(Value::Undefined)
}

fn reset_keep_alive_conn(state: &Rc<RefCell<HostState>>, socket_id: u64) {
    let reset = state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .is_some_and(|conn| conn.response_done && conn.keep_alive && conn.body_done);
    if !reset {
        return;
    }
    if let Some(conn) = state.borrow_mut().http.conns.get_mut(&socket_id) {
        conn.req = None;
        conn.body_remaining = 0;
        conn.body_done = true;
        conn.head_parsed = false;
        conn.response_done = false;
    }
}

/// Resume parsing after a delayed response completes. Pipelined bytes may
/// already be buffered while the request listener waits on a timer; the
/// normal socket `'data'` edge has then already fired, so feed that buffer
/// through the same state machine explicitly.
pub(crate) fn resume_connection(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
) -> Result<(), VmError> {
    reset_keep_alive_conn(state, socket_id);
    let buffered = state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .is_some_and(|conn| !conn.buffer.is_empty() && !conn.head_parsed);
    if buffered {
        feed_conn(state, socket_id)?;
    }
    Ok(())
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
    if state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .is_some_and(|conn| conn.body_chunked)
    {
        return drain_chunked_body(state, socket_id);
    }
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
            let chunk = match execute::get_property_result(&req, REQ_ENCODING_PROP).ok() {
                Some(Value::String(encoding))
                    if encoding.eq_ignore_ascii_case("utf8")
                        || encoding.eq_ignore_ascii_case("utf-8") =>
                {
                    Value::String(String::from_utf8_lossy(&data).into_owned())
                }
                _ => make_buffer(&data),
            };
            net::emit(state, &req, "data", vec![chunk])?;
        }
        if done {
            if let Some(conn) = state.borrow_mut().http.conns.get_mut(&socket_id) {
                conn.body_done = true;
                if let Some(req) = conn.req.as_ref() {
                    execute::set_property_in_place(req, "complete", Value::Boolean(true));
                }
            }
            net::emit(state, &req, "end", Vec::new())?;
        }
    }
    Ok(())
}

fn drain_chunked_body(state: &Rc<RefCell<HostState>>, socket_id: u64) -> Result<(), VmError> {
    let (req, chunks, done) = {
        let mut guard = state.borrow_mut();
        let Some(conn) = guard.http.conns.get_mut(&socket_id) else {
            return Ok(());
        };
        if conn.req.is_none() || conn.body_done {
            return Ok(());
        }
        let mut chunks = Vec::new();
        let mut done = false;
        loop {
            let Some(line_end) = conn.buffer.windows(2).position(|pair| pair == b"\r\n") else {
                break;
            };
            let Ok(size) = usize::from_str_radix(
                String::from_utf8_lossy(&conn.buffer[..line_end])
                    .split(';')
                    .next()
                    .unwrap_or_default(),
                16,
            ) else {
                break;
            };
            let frame_end = line_end + 2 + size + 2;
            if conn.buffer.len() < frame_end {
                break;
            }
            conn.buffer.drain(..line_end + 2);
            if size == 0 {
                conn.buffer.drain(..2);
                done = true;
                break;
            }
            chunks.push(conn.buffer[..size].to_vec());
            conn.buffer.drain(..size + 2);
        }
        conn.body_chunked_done |= done;
        conn.body_done = conn.body_chunked_done;
        (conn.req.clone(), chunks, conn.body_done)
    };
    if let Some(req) = req {
        for chunk in chunks {
            if !chunk.is_empty() {
                let value = match execute::get_property_result(&req, REQ_ENCODING_PROP).ok() {
                    Some(Value::String(encoding))
                        if encoding.eq_ignore_ascii_case("utf8")
                            || encoding.eq_ignore_ascii_case("utf-8") =>
                    {
                        Value::String(String::from_utf8_lossy(&chunk).into_owned())
                    }
                    _ => make_buffer(&chunk),
                };
                net::emit(state, &req, "data", vec![value])?;
            }
        }
        if done {
            execute::set_property_in_place(&req, "complete", Value::Boolean(true));
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
        Some(Value::StringUnits(units)) => String::from_utf16_lossy(units).into_bytes(),
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
    let missing_host = matches!(
        execute::get_property(&req, "headers"),
        Value::Object(_) | Value::ObjectAlias(_)
    ) && matches!(
        execute::get_property(&execute::get_property(&req, "headers"), "host"),
        Value::Undefined
    );
    let require_host_header = state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .map(|conn| conn.require_host_header)
        .unwrap_or(true);
    if require_host_header && missing_host {
        execute::set_property_in_place(&res, "statusCode", Value::Number(400.0));
        execute::set_property_in_place(&res, "statusMessage", Value::String("Bad Request".into()));
        res_end(state, Some(&res), &[])?;
        return Ok(Value::Undefined);
    }
    let expect = headers_value(&req, "expect");
    if let Some(expect) = expect
        .as_ref()
        .filter(|value| !value.eq_ignore_ascii_case("100-continue"))
    {
        let has_listener = crate::modules::emitter::emitter_id(&server)
            .and_then(|id| state.borrow().emitters.get(id))
            .is_some_and(|emitter| !emitter.borrow().listeners_of("checkExpectation").is_empty());
        if has_listener {
            net::emit(state, &server, "checkExpectation", vec![req.clone(), res])?;
        } else {
            execute::set_property_in_place(&res, "statusCode", Value::Number(417.0));
            execute::set_property_in_place(
                &res,
                "statusMessage",
                Value::String("Expectation Failed".into()),
            );
            res_end(state, Some(&res), &[])?;
        }
        return Ok(Value::Undefined);
    }
    let event = expect.is_some().then_some("checkContinue").unwrap_or("request");
    net::emit(state, &server, event, vec![req.clone(), res])?;
    let body_done = state
        .borrow()
        .http
        .conns
        .get(&socket_id)
        .map(|conn| conn.body_done)
        .unwrap_or(false);
    if body_done {
        net::emit(state, &req, "end", Vec::new())?;
    }
    // With keep-alive, reset the connection so the next pipelined request
    // on the same socket parses; otherwise the net layer's socket_end
    // closes it after the response.
    let reset = {
        let guard = state.borrow();
        guard
            .http
            .conns
            .get(&socket_id)
            .map(|conn| conn.response_done && conn.keep_alive)
            .unwrap_or(false)
    };
    if reset {
        let mut guard = state.borrow_mut();
        if let Some(conn) = guard.http.conns.get_mut(&socket_id) {
            conn.req = None;
            conn.body_remaining = 0;
            conn.body_done = true;
            conn.head_parsed = false;
            conn.response_done = false;
        }
    }
    Ok(Value::Undefined)
}

/// Report one inbound HTTP request through the shared performance bridge.
/// The bridge owns observer delivery; Rust supplies the exact request/response
/// identities and descriptors already exposed by this module.
pub(crate) fn record_http_entry(
    state: &Rc<RefCell<HostState>>,
    name: &str,
    req: Value,
    res: Value,
) {
    let Some(record) = state.borrow().net.performance_record.clone() else {
        return;
    };
    if !quench_runtime::is_callable(&record) {
        return;
    }
    if matches!(execute::get_property(&res, "headers"), Value::Undefined) {
        execute::set_property_in_place(&res, "headers", host_api::object(Vec::new()));
    }
    if matches!(execute::get_property(&res, "statusCode"), Value::Undefined) {
        execute::set_property_in_place(&res, "statusCode", Value::Number(200.0));
    }
    if matches!(
        execute::get_property(&res, "statusMessage"),
        Value::Undefined
    ) {
        execute::set_property_in_place(&res, "statusMessage", Value::String("OK".into()));
    }
    let detail = host_api::object(vec![("req".into(), req), ("res".into(), res)]);
    let _ = execute::call(
        &record,
        &Value::Undefined,
        &[
            Value::String("http".into()),
            detail,
            Value::String(name.into()),
        ],
    );
}

fn build_req_res(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
    head: &[u8],
) -> Result<(Value, Value), VmError> {
    let (mut req, content_length, keep_alive, body_chunked) = build_req(state, head)?;
    let server_request = {
        let guard = state.borrow();
        match guard.http.conns.get(&socket_id) {
            Some(conn) => {
                let direct = execute::get_property(&conn.server, SERVER_REQUEST_PROP);
                if quench_runtime::is_callable(&direct) {
                    Some(direct)
                } else {
                    guard
                        .net
                        .servers
                        .iter()
                        .find(|(_, server)| {
                            execute::same_identity(&server.borrow().js, &conn.server)
                        })
                        .and_then(|(id, _)| guard.http.server_requests.get(id).cloned())
                }
            }
            None => None,
        }
    };
    if let Some(constructor) = server_request {
        let prototype = execute::get_property(&constructor, "prototype");
        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
            req = execute::set_prototype_of(&req, &prototype)?;
        }
    }
    // IncomingMessage.socket/connection are aliases of the accepted net
    // socket. Stamp both on the request before exposing it to user code.
    if let Some(socket) = state
        .borrow()
        .net
        .sockets
        .get(&socket_id)
        .map(|socket| socket.borrow().js.clone())
    {
        execute::set_property_in_place(&socket, HTTP_SERVER_SOCKET_PROP, Value::Boolean(true));
        execute::set_property_in_place(&req, "socket", socket.clone());
        execute::set_property_in_place(&req, "connection", socket);
    }
    let (mut res, id) = build_res_object(state)?;
    if let Some(socket) = state
        .borrow()
        .net
        .sockets
        .get(&socket_id)
        .map(|socket| socket.borrow().js.clone())
    {
        execute::set_property_in_place(&res, "socket", socket.clone());
        execute::set_property_in_place(&res, "connection", socket);
    }
    let server_response = {
        let guard = state.borrow();
        match guard.http.conns.get(&socket_id) {
            Some(conn) => {
                let direct = execute::get_property(&conn.server, SERVER_RESPONSE_PROP);
                if quench_runtime::is_callable(&direct) {
                    Some(direct)
                } else {
                    guard
                        .net
                        .servers
                        .iter()
                        .find(|(_, server)| {
                            execute::same_identity(&server.borrow().js, &conn.server)
                        })
                        .and_then(|(id, _)| guard.http.server_responses.get(id).cloned())
                }
            }
            None => None,
        }
    };
    if let Some(constructor) = server_response {
        let prototype = execute::get_property(&constructor, "prototype");
        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
            res = execute::set_prototype_of(&res, &prototype)?;
        }
    }
    // Node exposes the incoming message on the response as `res.req`; keep
    // the exact request identity that is emitted to user listeners.
    execute::set_property_in_place(&res, "req", req.clone());
    let http10 = matches!(
        execute::get_property(&req, "httpVersion"),
        Value::String(version) if version == "1.0"
    );
    insert_response(state, socket_id, id, keep_alive, http10);
    {
        let mut guard = state.borrow_mut();
        if let Some(conn) = guard.http.conns.get_mut(&socket_id) {
            conn.req = Some(req.clone());
            conn.requests.push(req.clone());
            conn.body_remaining = content_length;
            conn.body_chunked = body_chunked;
            conn.body_chunked_done = false;
            conn.body_done = !body_chunked && content_length == 0;
            conn.response_done = false;
            conn.keep_alive = keep_alive;
        }
    }
    Ok((req, res))
}

/// Build the `req` emitter from the parsed head.
fn build_req(
    state: &Rc<RefCell<HostState>>,
    head: &[u8],
) -> Result<(Value, usize, bool, bool), VmError> {
    let (method, url, version, headers, content_length, keep_alive) = parse_request_head(head);
    let (version_major, version_minor) = version
        .split_once('.')
        .map(|(major, minor)| {
            (
                major.parse::<u32>().unwrap_or(1),
                minor.parse::<u32>().unwrap_or(1),
            )
        })
        .unwrap_or((1, 1));
    let async_resource = crate::modules::async_hooks::new_resource(
        state,
        &[Value::String("HTTPINCOMINGMESSAGE".into())],
    )?;
    let req = crate::modules::events::new_emitter_object(state)?;
    let req = install_req_props(
        req,
        vec![
            ("method".to_string(), Value::String(method)),
            ("url".to_string(), Value::String(url)),
            ("httpVersion".to_string(), Value::String(version)),
            (
                "httpVersionMajor".to_string(),
                Value::Number(version_major as f64),
            ),
            (
                "httpVersionMinor".to_string(),
                Value::Number(version_minor as f64),
            ),
            (
                "headers".to_string(),
                execute::set_prototype_of(&host_api::object(headers), &Value::Null)?,
            ),
            (
                "pause".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_RESUME),
            ),
            (
                "resume".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_RESUME),
            ),
            (
                "setEncoding".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_RES_SET_ENCODING),
            ),
            (
                "destroy".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_REQ_DESTROY),
            ),
            (
                "pipe".to_string(),
                crate::host::capability(crate::registry::SPEC_HTTP_RES_PIPE),
            ),
            ("signal".to_string(), new_http_signal(state)?),
            ("readable".to_string(), Value::Boolean(true)),
            ("aborted".to_string(), Value::Boolean(false)),
            ("complete".to_string(), Value::Boolean(false)),
            ("destroyed".to_string(), Value::Boolean(false)),
            (REQ_CLOSE_PROP.to_string(), Value::Boolean(false)),
            (REQ_ASYNC_RESOURCE_PROP.to_string(), async_resource),
        ],
    )?;
    let chunked = headers_value(&req, "transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
    Ok((req, content_length, keep_alive, chunked))
}

fn headers_value(request: &Value, key: &str) -> Option<String> {
    let value = execute::get_property(&execute::get_property(request, "headers"), key);
    if matches!(value, Value::Undefined | Value::Null) {
        return None;
    }
    execute::to_js_string(&value).ok()
}

/// `req.destroy([error])` — abort the server-side message. Its half-open
/// response socket remains available for a handler that writes a response;
/// the error/close pair is queued for Node's asynchronous ordering.
pub fn request_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some((socket_id, req, _socket)) = state
        .borrow()
        .http
        .conns
        .iter()
        .find_map(|(socket_id, conn)| {
            conn.req
                .as_ref()
                .filter(|req| execute::same_identity(req, receiver))
                .map(|req| (*socket_id, req.clone(), conn.socket.clone()))
        })
    else {
        return Ok(receiver.clone());
    };
    if matches!(
        execute::get_property(&req, "destroyed"),
        Value::Boolean(true)
    ) {
        return Ok(receiver.clone());
    }
    execute::set_property_in_place(&req, "destroyed", Value::Boolean(true));
    execute::set_property_in_place(&req, "aborted", Value::Boolean(true));
    execute::set_property_in_place(&req, "readable", Value::Boolean(false));
    let signal = execute::get_property(&req, "signal");
    if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
        abort_http_signal(state, &signal)?;
    }
    let body_complete = state
        .borrow()
        .http
        .conns
        .values()
        .find(|conn| {
            conn.req
                .as_ref()
                .is_some_and(|value| execute::same_identity(value, &req))
        })
        .map(|conn| conn.body_done)
        .unwrap_or(true);
    if let Some(conn) = state.borrow_mut().http.conns.values_mut().find(|conn| {
        conn.req
            .as_ref()
            .is_some_and(|value| execute::same_identity(value, &req))
    }) {
        conn.body_done = true;
    }
    execute::set_property_in_place(&req, REQ_CLOSE_PROP, Value::Boolean(true));
    let error = if body_complete {
        None
    } else {
        Some(args.first().cloned().unwrap_or_else(|| {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("The operation was aborted".into())],
            );
            execute::set_property(error, "name", Value::String("AbortError".into()))
        }))
    };
    let mut pending = Vec::with_capacity(2);
    if let Some(error) = error {
        pending.push((req.clone(), "error".into(), vec![error]));
    }
    pending.push((req, "close".into(), Vec::new()));
    state.borrow_mut().net.pending_events.extend(pending);
    Ok(receiver.clone())
}

/// Register a fresh response for the socket.
fn insert_response(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
    id: u64,
    keep_alive: bool,
    http10: bool,
) {
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
            keep_alive,
            http10,
            headers_sent: false,
            sent_body: 0,
            chunked: false,
            ended: false,
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
fn parse_request_head(head: &[u8]) -> (String, String, String, Vec<(String, Value)>, usize, bool) {
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
    let mut connection = String::new();
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
            if key == "connection" {
                connection = value.to_lowercase();
            }
            if let Some((_, Value::String(existing))) =
                headers.iter_mut().find(|(name, _)| name == &key)
            {
                if forbidden_duplicate_header(&key) {
                    continue;
                }
                existing.push_str(if key == "cookie" { "; " } else { ", " });
                existing.push_str(value);
                continue;
            }
            headers.push((key, Value::String(value.to_string())));
        }
    }
    // HTTP/1.1 defaults to keep-alive unless `Connection: close`; HTTP/1.0
    // defaults to close unless the client asks to keep the connection.
    let http10 = version == "1.0";
    let keep_alive =
        !connection.contains("close") && (!http10 || connection.contains("keep-alive"));
    (
        method,
        url,
        version.to_string(),
        headers,
        content_length,
        keep_alive,
    )
}

fn forbidden_duplicate_header(name: &str) -> bool {
    matches!(
        name,
        "content-length"
            | "content-type"
            | "user-agent"
            | "referer"
            | "host"
            | "authorization"
            | "proxy-authorization"
            | "if-modified-since"
            | "if-unmodified-since"
            | "from"
            | "location"
            | "max-forwards"
    )
}

fn build_res_object(state: &Rc<RefCell<HostState>>) -> Result<(Value, u64), VmError> {
    let id = {
        let mut guard = state.borrow_mut();
        let id = guard.http.next_res;
        guard.http.next_res += 1;
        id
    };
    let mut res = crate::modules::events::new_emitter_object(state)?;
    for (name, value) in [
        (
            "setHeader".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_SET_HEADER),
        ),
        (
            "setHeaders".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_SET_HEADERS),
        ),
        (
            "removeHeader".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_REMOVE_HEADER),
        ),
        (
            "writeHead".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE_HEAD),
        ),
        (
            "write".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE),
        ),
        // Legacy Node fixtures use the internal `_send('')` hook to flush a
        // fragment. The observable effect is the same as an empty write;
        // expose it through the canonical response writer capability.
        (
            "_send".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE),
        ),
        (
            "writeContinue".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE_CONTINUE),
        ),
        (
            "writeInformation".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE_INFORMATION),
        ),
        (
            "writeProcessing".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_WRITE_PROCESSING),
        ),
        (
            "end".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_END),
        ),
        (
            "destroy".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_DESTROY),
        ),
        (
            "flushHeaders".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_FLUSH_HEADERS),
        ),
        (
            "cork".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_CORK),
        ),
        (
            "uncork".to_string(),
            res_cap(crate::registry::SPEC_HTTP_RES_UNCORK),
        ),
        ("writableCorked".to_string(), Value::Number(0.0)),
        ("writableLength".to_string(), Value::Number(0.0)),
        ("writableNeedDrain".to_string(), Value::Boolean(false)),
        ("writableHighWaterMark".to_string(), Value::Number(16_384.0)),
        ("sendDate".to_string(), Value::Boolean(true)),
        ("finished".to_string(), Value::Boolean(false)),
        ("writable".to_string(), Value::Boolean(true)),
        ("writableEnded".to_string(), Value::Boolean(false)),
        ("destroyed".to_string(), Value::Boolean(false)),
        ("statusCode".to_string(), Value::Number(200.0)),
        (RES_ID_PROP.to_string(), Value::Number(id as f64)),
    ] {
        res = execute::set_property(res, &name, value);
    }
    Ok((res, id))
}

fn res_cap(spec: crate::registry::NodeSpec) -> Value {
    crate::host::capability(spec)
}

// Response methods live in `http_res`; re-exported here for dispatch.
pub use crate::modules::http_res::{
    res_cork, res_end, res_remove_header, res_set_header, res_set_headers, res_uncork, res_write,
    res_write_continue, res_write_head, res_write_information, res_write_processing,
};
pub use crate::modules::http_res::{res_destroy, res_flush_headers};

/// Construct an IncomingMessage with the same signal/destroy state used by
/// network-created messages. This keeps the public constructor useful for
/// detached messages without inventing a second object model.
pub fn incoming_construct(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let object = crate::modules::events::new_emitter_object(state)?;
    install_req_props(
        object,
        vec![
            ("signal".into(), new_http_signal(state)?),
            (
                "destroy".into(),
                crate::host::capability(crate::registry::SPEC_HTTP_INCOMING_DESTROY),
            ),
            ("aborted".into(), Value::Boolean(false)),
            ("complete".into(), Value::Boolean(false)),
        ],
    )
}

pub fn incoming_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    if matches!(
        execute::get_property(receiver, "destroyed"),
        Value::Boolean(true)
    ) {
        return Ok(receiver.clone());
    }
    let error = args.first().cloned();
    execute::set_property_in_place(receiver, "destroyed", Value::Boolean(true));
    execute::set_property_in_place(receiver, "errored", error.clone().unwrap_or(Value::Null));
    let signal = execute::get_property(receiver, "signal");
    if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
        abort_http_signal(state, &signal)?;
    }
    let client_response = matches!(
        execute::get_property(receiver, "__httpClientResponse"),
        Value::Boolean(true)
    ) || state.borrow().http.clientreqs.values().any(|request| {
        request
            .res
            .as_ref()
            .is_some_and(|response| execute::same_identity(response, receiver))
    });
    let response_complete = matches!(
        execute::get_property(receiver, "complete"),
        Value::Boolean(true)
    );
    if !client_response || error.is_some() {
        execute::set_property_in_place(receiver, INCOMING_CLOSE_PENDING_PROP, Value::Boolean(true));
    }
    if !client_response {
        execute::set_property_in_place(receiver, REQ_CLOSE_PROP, Value::Boolean(true));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((receiver.clone(), "close".into(), Vec::new()));
    }
    if client_response {
        execute::set_property_in_place(receiver, INCOMING_CLOSE_PENDING_PROP, Value::Boolean(true));
        let socket = state
            .borrow()
            .http
            .clientreqs
            .values()
            .find(|request| {
                request
                    .res
                    .as_ref()
                    .is_some_and(|response| execute::same_identity(response, receiver))
            })
            .and_then(|request| request.socket.clone());
        state
            .borrow_mut()
            .net
            .pending_events
            .push((receiver.clone(), "close".into(), Vec::new()));
        if !response_complete {
            if let Some(socket) = socket {
                net::socket_destroy(state, Some(&socket), &[])?;
            }
        }
    }
    if let Some(error) = error {
        net::emit(state, receiver, "error", vec![error])?;
    }
    Ok(receiver.clone())
}

/// `http.request(options[, cb])` — an outbound ClientRequest.
pub fn request(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http_client::request(state, args)
}

pub fn request_resume(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `http.get(options[, cb])` — `request` with method GET, auto-ended.
pub fn get(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http_client::get(state, args)
}

pub fn https_request(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http_client::https_request(state, args)
}

pub fn https_get(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http_client::https_get(state, args)
}

/// The `http` module namespace.
pub fn build(state: &Rc<RefCell<HostState>>) -> Value {
    if let Some(cached) = state.borrow().module_cache.get("http") {
        return cached.clone();
    }
    if state.borrow().net.performance_record.is_none() {
        let record = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__nodePerformanceRecord",
        );
        if quench_runtime::is_callable(&record) {
            state.borrow_mut().net.performance_record = Some(record);
        }
    }
    let agent = crate::host::capability(crate::registry::SPEC_HTTP_AGENT);
    let agent_prototype = host_api::object(vec![
        (
            "createConnection".into(),
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        ),
        (
            "createSocket".into(),
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        ),
        (
            "getName".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_AGENT_GET_NAME),
        ),
        (
            "addRequest".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_AGENT_ADD_REQUEST),
        ),
        (
            "keepSocketAlive".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_AGENT_KEEP_SOCKET_ALIVE),
        ),
    ]);
    state.borrow_mut().http.agent_prototype = Some(agent_prototype.clone());
    let agent = quench_runtime::execute::set_property(agent, "prototype", agent_prototype);
    let global_agent = crate::modules::http_client::agent_construct(
        state,
        &[host_api::object(vec![(
            "keepAlive".into(),
            Value::Boolean(true),
        )])],
    )
    .unwrap_or(Value::Undefined);
    state.borrow_mut().http.global_agent = Some(global_agent.clone());
    let client_request_prototype = host_api::object(vec![
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_WRITE),
        ),
        (
            "end".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_END),
        ),
        (
            "setHeader".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_HEADER),
        ),
        (
            "getHeader".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_GET_HEADER),
        ),
        (
            "getHeaders".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_GET_HEADERS),
        ),
        (
            "getHeaderNames".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_GET_HEADER_NAMES),
        ),
        (
            "hasHeader".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_HAS_HEADER),
        ),
        (
            "removeHeader".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_REMOVE_HEADER),
        ),
        (
            "setNoDelay".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_NO_DELAY),
        ),
        (
            "setSocketKeepAlive".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_KEEP_ALIVE),
        ),
        (
            "setSocketTimeout".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_TIMEOUT_SOCKET),
        ),
        (
            "setTimeout".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_SET_TIMEOUT),
        ),
        (
            "abort".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_ABORT),
        ),
        (
            "destroy".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_CLIENT_DESTROY),
        ),
        (
            "cork".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_CORK),
        ),
        (
            "uncork".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_REQ_UNCORK),
        ),
    ]);
    state.borrow_mut().http.client_request_prototype = Some(client_request_prototype);
    let client_request = quench_runtime::execute::set_property(
        crate::host::capability(crate::registry::SPEC_HTTP_CLIENT_REQUEST),
        "prototype",
        state
            .borrow()
            .http
            .client_request_prototype
            .clone()
            .unwrap_or_else(|| host_api::object(Vec::new())),
    );
    let incoming = quench_runtime::execute::set_property(
        crate::host::capability(crate::registry::SPEC_HTTP_INCOMING),
        "prototype",
        crate::modules::events::emitter_prototype()
            .unwrap_or_else(|_| host_api::object(Vec::new())),
    );
    let mut module = crate::host::namespace_object(vec![
        (
            "createServer",
            crate::host::capability(crate::registry::SPEC_HTTP_SERVER),
        ),
        (
            "Server",
            crate::host::capability(crate::registry::SPEC_HTTP_SERVER),
        ),
        (
            "request",
            crate::host::capability(crate::registry::SPEC_HTTP_REQUEST),
        ),
        ("ClientRequest", client_request),
        (
            "get",
            crate::host::capability(crate::registry::SPEC_HTTP_GET),
        ),
        ("Agent", agent),
        ("globalAgent", global_agent),
        ("IncomingMessage", incoming),
        (
            "OutgoingMessage",
            crate::host::capability(crate::registry::SPEC_HTTP_OUTGOING),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined);
    let outgoing_prototype = host_api::object(vec![
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_OUTGOING_WRITE),
        ),
        (
            "end".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_OUTGOING_END),
        ),
        (
            "destroy".into(),
            crate::host::capability(crate::registry::SPEC_HTTP_OUTGOING_DESTROY),
        ),
    ]);
    state.borrow_mut().http.outgoing_prototype = Some(outgoing_prototype.clone());
    let outgoing = quench_runtime::execute::set_property(
        crate::host::capability(crate::registry::SPEC_HTTP_OUTGOING),
        "prototype",
        outgoing_prototype,
    );
    module = quench_runtime::execute::set_property(module, "OutgoingMessage", outgoing.clone());
    module = quench_runtime::execute::set_property(module, "ServerResponse", outgoing.clone());
    let methods = [
        "ACL",
        "BIND",
        "CHECKOUT",
        "CONNECT",
        "COPY",
        "DELETE",
        "GET",
        "HEAD",
        "LINK",
        "LOCK",
        "M-SEARCH",
        "MERGE",
        "MKACTIVITY",
        "MKCALENDAR",
        "MKCOL",
        "MOVE",
        "NOTIFY",
        "OPTIONS",
        "PATCH",
        "POST",
        "PROPFIND",
        "PROPPATCH",
        "PURGE",
        "PUT",
        "QUERY",
        "REBIND",
        "REPORT",
        "SEARCH",
        "SOURCE",
        "SUBSCRIBE",
        "TRACE",
        "UNBIND",
        "UNLINK",
        "UNLOCK",
        "UNSUBSCRIBE",
    ];
    let values = methods
        .into_iter()
        .map(|method| Value::String(method.to_string()))
        .collect();
    module = quench_runtime::execute::set_property(
        module,
        "METHODS",
        quench_runtime::host_api::array(values),
    );
    state
        .borrow_mut()
        .module_cache
        .insert("http".into(), module.clone());
    module
}
