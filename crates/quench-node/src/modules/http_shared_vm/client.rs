use crate::host::{NodeHost, SharedNodeState};
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::time::{Duration, Instant};

type Context<'a> = NativeContext<'a, NodeHost>;

const REQUEST_FUNCTION: &str = r#"((Writable, create, write, removeHeader, end, destroy, normalizeUrl) => {
  const headerPairs = (headers) => {
    const pairs = [];
    const add = (name, value) => pairs.push(String(name), Array.isArray(value)
      ? value.join(String(name).toLowerCase() === "cookie" ? "; " : ", ")
      : String(value));
    if (Array.isArray(headers)) {
      for (let index = 0; index + 1 < headers.length; index += 2) {
        add(headers[index], headers[index + 1]);
      }
    } else if (headers && typeof headers === "object") {
      for (const [name, value] of Object.entries(headers)) add(name, value);
    }
    return pairs;
  };
  const request = (input = {}, overrides, callback) => {
    if (typeof overrides === "function") { callback = overrides; overrides = undefined; }
    let options = input;
    if (typeof input === "string" || (input && typeof input.href === "string")) {
      options = normalizeUrl(String(input));
    }
    if (overrides !== undefined && overrides !== null) options = { ...options, ...overrides };
    const id = create(options, headerPairs(options?.headers));
    let ending = false;
    const request = new Writable({
      // Request-body completion precedes response completion. Auto-destroy
      // would call `_destroy` at `finish` and close the live HTTP exchange.
      autoDestroy: false,
      write(chunk, encoding, callback) {
        try { write(id, chunk, !ending); callback(); }
        catch (error) { callback(error); }
      },
      final(callback) {
        try { end(id, request); callback(); }
        catch (error) { callback(error); }
      },
      destroy(error, callback) {
        try { destroy(id); callback(error); }
        catch (failure) { callback(failure); }
      },
    });
    request.removeHeader = (name) => { removeHeader(id, name); return request; };
    const finish = request.end.bind(request);
    request.end = (chunk, encoding, callback) => {
      ending = true;
      return finish(chunk, encoding, callback);
    };
    request.path = options?.path || "/";
    request.method = options?.method || "GET";
    // Node's ClientRequest starts with `reusedSocket = false` and the agent sets it
    // on a request it places on a free socket.
    request.reusedSocket = false;
    if (typeof callback === "function") request.once("response", callback);
    return request;
  };
  return request;
})"#;

pub(crate) fn request_function(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let create = context.host_function(crate::host::shared_vm::operation("httpRequestCreate"))?;
    let write = context.host_function(crate::host::shared_vm::operation("httpRequestWrite"))?;
    let remove_header =
        context.host_function(crate::host::shared_vm::operation("httpRequestRemoveHeader"))?;
    let end = context.host_function(crate::host::shared_vm::operation("httpRequestEnd"))?;
    let destroy = context.host_function(crate::host::shared_vm::operation("httpRequestDestroy"))?;
    let normalize_url =
        context.host_function(crate::host::shared_vm::operation("httpRequestNormalizeUrl"))?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let writable_key = context.string_rooted("Writable");
    let writable = context.get_property_rooted(stream, writable_key)?;
    let factory = context.evaluate_script_rooted(REQUEST_FUNCTION, "node:http/request.js")?;
    let undefined = context.undefined();
    context.call_rooted(
        factory,
        undefined,
        &[
            writable,
            create,
            write,
            remove_header,
            end,
            destroy,
            normalize_url,
        ],
    )
}

pub(crate) fn normalize_url(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = match args.first().copied() {
        Some(root) => context.string_text(root)?,
        None => None,
    };
    let Some(input) = input else {
        return invalid_url(context, "The \"url\" argument must be of type string");
    };
    let parsed = match url::Url::parse(&input) {
        Ok(parsed) => parsed,
        Err(_) => return invalid_url(context, "Invalid URL"),
    };
    if parsed.scheme() != "http" {
        let error = context.type_error_rooted(&format!(
            "Protocol \"{}:\" not supported. Expected \"http:\"",
            parsed.scheme()
        ))?;
        set_text(context, error, "code", "ERR_INVALID_PROTOCOL")?;
        return Err(context.throw(error));
    }
    let Some(hostname) = parsed.host_str() else {
        return invalid_url(context, "Invalid URL");
    };
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut path = parsed.path().to_owned();
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    let options = context.object_rooted()?;
    set_text(context, options, "hostname", hostname)?;
    let port = context.number(f64::from(port));
    set(context, options, "port", port)?;
    set_text(context, options, "path", &path)?;
    set_text(context, options, "method", "GET")?;
    Ok(options)
}

fn invalid_url(context: &mut Context<'_>, message: &str) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(message)?;
    set_text(context, error, "code", "ERR_INVALID_URL")?;
    Err(context.throw(error))
}

fn set_text(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot initialize shared HTTP request option {name}"
        )))
    }
}

pub(crate) fn create(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("http.request options are missing"))?;
    let host = text_property(context, options, "hostname")?
        .or(text_property(context, options, "host")?)
        .unwrap_or_else(|| "localhost".to_owned());
    let port = port_property(context, options)?
        .filter(|port| port.is_finite() && (0.0..=u16::MAX as f64).contains(port))
        .unwrap_or(80.0) as u16;
    let method = text_property(context, options, "method")?.unwrap_or_else(|| "GET".to_owned());
    let path = text_property(context, options, "path")?.unwrap_or_else(|| "/".to_owned());
    let headers = read_headers(context, args.get(1).copied())?;
    let agent_object = object_property(context, options, "agent")?;
    let agent = agent_object
        .map(|agent| property_id(context, agent, "_quenchSharedAgentId"))
        .transpose()?
        .flatten();
    let (keep_alive, idle_timeout, max_sockets, max_free_sockets) = match agent {
        Some(_) => {
            let agent = agent_object.expect("an agent id has an agent object");
            // Node's `Agent.keepSocketAlive` arms the free-socket timer with
            // `agent.options.timeout` (`keepAliveMsecs` is the TCP probe delay),
            // where 0 leaves it disarmed.
            let timeout = match object_property(context, agent, "options")? {
                Some(options) => number_property(context, options, "timeout")?,
                None => None,
            };
            (
                boolean_property(context, agent, "keepAlive")?.unwrap_or(false),
                timeout
                    .filter(|msecs| msecs.is_finite() && *msecs > 0.0)
                    .map(|msecs| Duration::from_millis(msecs as u64)),
                socket_limit(context, agent, "maxSockets")?.unwrap_or(usize::MAX),
                number_property(context, agent, "maxFreeSockets")?
                    .map(|value| {
                        if value.is_finite() && value >= 0.0 {
                            value as usize
                        } else {
                            usize::MAX
                        }
                    })
                    .unwrap_or(256),
            )
        }
        None => (false, None, usize::MAX, 256),
    };
    let origin = super::state::OriginKey::new(agent, &host, port);
    let state = context.host_mut().shared_state();
    let mut host_state = state.borrow_mut();
    let id = host_state.http.request_id().map_err(RootedError::host)?;
    host_state.http.requests.insert(
        id,
        super::state::OutgoingRequest {
            host,
            port,
            method,
            path,
            headers,
            body: Default::default(),
            origin,
            keep_alive,
            max_sockets,
            max_free_sockets,
            idle_timeout,
        },
    );
    Ok(context.number(id as f64))
}

pub(crate) fn write(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let chunk = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP request chunk is missing"))?;
    let chunk = context.to_string(chunk)?.into_bytes();
    let explicit = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    let Some(request) = host.http.requests.get_mut(&id) else {
        return Err(RootedError::host(
            "shared HTTP request is no longer writable",
        ));
    };
    request.body.push(chunk, explicit);
    Ok(context.boolean(true))
}

pub(crate) fn remove_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP request header name is missing"))?;
    let name = context.to_string(name)?;
    if let Some(request) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .requests
        .get_mut(&id)
    {
        request
            .headers
            .retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
    }
    Ok(context.undefined())
}

pub(crate) fn end(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let request_root = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP ClientRequest is missing"))?;
    let request = {
        let state = context.host_mut().shared_state();
        let mut guard = state.borrow_mut();
        if !guard.http.requests.contains_key(&id) {
            return Err(RootedError::host("shared HTTP request has already ended"));
        }
        guard.http.requests.remove(&id).unwrap()
    };
    let request_bytes = serialize(&request);
    let request_root = context.retain(request_root)?;
    let client = super::state::Client {
        request_id: id,
        request_root,
        request: request_bytes,
        response_parser: super::protocol::ResponseParser::for_method(&request.method),
        response_root: None,
        origin: request.origin,
        keep_alive: request.keep_alive,
        max_sockets: request.max_sockets,
        max_free_sockets: request.max_free_sockets,
        idle_timeout: request.idle_timeout,
        reused: false,
    };
    let outcome = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        start_client(&mut host, client)
    };
    match outcome {
        Ok(StartOutcome::Started { reused: true }) => {
            mark_reused_socket(context, request_root)?;
            Ok(context.undefined())
        }
        Ok(_) => Ok(context.undefined()),
        Err((client, error)) => {
            context.release_root(client.request_root);
            Err(RootedError::host(error))
        }
    }
}

/// Node's `ClientRequest.reusedSocket`: false until the agent hands the request a
/// socket from its free pool (`Agent.prototype.reuseSocket`).
fn mark_reused_socket(context: &mut Context<'_>, request: RootId) -> Result<(), RootedError> {
    let reused = context.boolean(true);
    set(context, request, "reusedSocket", reused)
}

/// Where a completed request runs: a free keep-alive socket, a fresh socket, or
/// the origin's queue when its agent is at `maxSockets`.
enum StartOutcome {
    Started { reused: bool },
    Queued,
}

fn start_client(
    host: &mut SharedNodeState,
    mut client: super::state::Client,
) -> Result<StartOutcome, (super::state::Client, String)> {
    if client.origin.pooled() {
        // Node's agent takes a socket from `freeSockets[name]` for this origin and
        // destroys one it cannot use (peer closed, stray bytes, mid-delivery).
        while let Some(idle) = host.http.take_free_socket(&client.origin) {
            if free_socket_is_usable(host, idle.socket)
                && net_shared_vm::write(&mut host.tcp, idle.socket, &client.request).is_ok()
            {
                client.reused = true;
                // Node's `reuseSocket` re-refs the socket `keepSocketAlive` unref'd.
                net_shared_vm::socket_reference(&mut host.tcp, idle.socket, true);
                start_read_if_idle(host, idle.socket);
                host.http.clients.insert(idle.socket, client);
                return Ok(StartOutcome::Started { reused: true });
            }
            close_agent_socket(host, Some(&client.origin), idle.socket);
        }
        if !host.http.has_capacity(&client.origin, client.max_sockets) {
            host.http
                .pending
                .push_back(super::state::PendingRequest { client });
            return Ok(StartOutcome::Queued);
        }
    }
    let host_name = client.origin.host.clone();
    let port = client.origin.port;
    match net_shared_vm::connect(&mut host.tcp, host_name, port) {
        Ok(socket) => {
            if client.origin.pooled() {
                host.http.own_socket(&client.origin, socket);
            }
            host.http.clients.insert(socket, client);
            Ok(StartOutcome::Started { reused: false })
        }
        Err(error) => Err((client, error)),
    }
}

/// Whether a free socket can carry a new exchange: it is still open, and the
/// transport has delivered every byte it already read for it. A socket in
/// delivery still owns a queued transport event, whose bytes would be parsed as
/// the next exchange's response.
fn free_socket_is_usable(host: &SharedNodeState, socket: u64) -> bool {
    net_shared_vm::socket_is_open(&host.tcp, socket)
        && net_shared_vm::delivery_delivered(&host.tcp, socket)
}

/// Start queued requests whose origin now has a free socket or room under
/// `maxSockets`, as Node's agent does when a request frees a socket. Returns the
/// exchanges whose fresh connection failed, and whether any started.
pub(crate) fn drain_pending(
    host: &mut SharedNodeState,
) -> (Vec<(super::state::Client, String)>, bool) {
    let mut failures = Vec::new();
    let mut progressed = false;
    loop {
        let index = host.http.pending.iter().position(|pending| {
            let origin = &pending.client.origin;
            host.http.has_free_socket(origin)
                || host.http.has_capacity(origin, pending.client.max_sockets)
        });
        let Some(index) = index else { break };
        let Some(pending) = host.http.pending.remove(index) else {
            break;
        };
        // Node's agent hands a freed socket to the queued request through its
        // `free` handler, which leaves `reusedSocket` false: only a request that
        // finds a free socket when the agent adds it is flagged.
        match start_client(host, pending.client) {
            Ok(StartOutcome::Started { .. }) => progressed = true,
            // Capacity disappeared between the check and the start; the client is
            // back in the queue.
            Ok(StartOutcome::Queued) => break,
            Err((client, error)) => {
                progressed = true;
                failures.push((client, error));
            }
        }
    }
    (failures, progressed)
}

/// Apply the two pool facts time and the transport own: a socket the transport
/// already dropped holds no `maxSockets` slot, and a free socket past its agent
/// timeout is destroyed (Node's `keepSocketAlive` timer).
pub(crate) fn maintain_agent_pools(host: &mut SharedNodeState) {
    let now = Instant::now();
    let mut expired = Vec::new();
    for (key, origin) in host.http.origins.iter_mut() {
        origin
            .sockets
            .retain(|socket| net_shared_vm::socket_is_open(&host.tcp, *socket));
        let mut kept = Vec::with_capacity(origin.idle.len());
        for idle in origin.idle.drain(..) {
            if !origin.sockets.contains(&idle.socket) {
                continue;
            }
            if idle.expires_at.is_some_and(|expires_at| expires_at <= now) {
                expired.push((key.clone(), idle.socket));
            } else {
                kept.push(idle);
            }
        }
        origin.idle = kept;
    }
    for (key, socket) in expired {
        close_agent_socket(host, Some(&key), socket);
    }
}

/// Leave `socket` free on its agent when the exchange permits reuse.
pub(crate) fn release_agent_socket(
    host: &mut SharedNodeState,
    socket: u64,
    client: &super::state::Client,
) {
    let origin = &client.origin;
    if !origin.pooled() {
        net_shared_vm::close_socket(&mut host.tcp, socket);
        return;
    }
    // A destroyed agent, a `Connection: close` exchange, or a socket that already
    // left the transport all end the socket instead of pooling it.
    if !client.keep_alive
        || !free_socket_is_usable(host, socket)
        || !host.http.owns_socket(origin, socket)
    {
        close_agent_socket(host, Some(origin), socket);
        return;
    }
    let expires_at = client.idle_timeout.map(|timeout| Instant::now() + timeout);
    if !host
        .http
        .free_socket(origin, socket, expires_at, client.max_free_sockets)
    {
        close_agent_socket(host, Some(origin), socket);
        return;
    }
    // Node's `keepSocketAlive` unrefs a free socket: it keeps no process alive,
    // while the transport still reports its end or stray bytes whenever the loop
    // runs for another reason.
    net_shared_vm::socket_reference(&mut host.tcp, socket, false);
    start_read_if_idle(host, socket);
}

/// Destroy a socket the caller found free on its agent: Node's free-socket guards
/// destroy a socket that ends or receives data instead of reusing it. Returns
/// whether the socket was free.
pub(crate) fn destroy_free_socket(host: &mut SharedNodeState, socket: u64) -> bool {
    let Some(origin) = host.http.take_free_socket_by_id(socket) else {
        return false;
    };
    close_agent_socket(host, Some(&origin), socket);
    true
}

fn start_read_if_idle(host: &mut SharedNodeState, socket: u64) {
    if !net_shared_vm::read_active(&host.tcp, socket) {
        let _ = net_shared_vm::start_read(&mut host.tcp, socket);
    }
}

/// The pool `client`'s socket belongs to, when it has an agent.
pub(crate) fn pooled_origin(client: &super::state::Client) -> Option<&super::state::OriginKey> {
    client.origin.pooled().then_some(&client.origin)
}

/// Drop every pool record of `socket` and close it. A socket no agent owns is
/// simply closed.
fn close_agent_socket(
    host: &mut SharedNodeState,
    origin: Option<&super::state::OriginKey>,
    socket: u64,
) {
    if let Some(origin) = origin {
        host.http.forget_socket(origin, socket);
    }
    net_shared_vm::close_socket(&mut host.tcp, socket);
}

pub(crate) fn destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let state = context.host_mut().shared_state();
    let client = {
        let mut host = state.borrow_mut();
        host.http.requests.remove(&id);
        let pending = host
            .http
            .pending
            .iter()
            .position(|pending| pending.client.request_id == id);
        if let Some(pending) = pending {
            host.http
                .pending
                .remove(pending)
                .map(|pending| pending.client)
        } else {
            let socket = host
                .http
                .clients
                .iter()
                .find_map(|(socket, client)| (client.request_id == id).then_some(*socket));
            socket.and_then(|socket| {
                let client = host.http.clients.remove(&socket)?;
                close_agent_socket(&mut host, pooled_origin(&client), socket);
                Some(client)
            })
        }
    };
    if let Some(client) = client {
        if let Some(response) = client.response_root {
            let error = context.error_rooted("socket hang up")?;
            set_text(context, error, "code", "ECONNRESET")?;
            let destroy_key = context.string_rooted("destroy");
            let destroy = context.get_property_rooted(response, destroy_key)?;
            context.call_rooted(destroy, response, &[error])?;
            context.release_root(destroy);
            context.release_root(destroy_key);
            context.release_root(error);
            context.release_root(response);
        }
        context.release_root(client.request_root);
    }
    Ok(context.undefined())
}

pub(crate) fn abort_exchange(
    context: &mut Context<'_>,
    client: super::state::Client,
) -> Result<(), RootedError> {
    let result = abort_exchange_stream(context, &client);
    if let Some(response) = client.response_root {
        context.release_root(response);
    }
    context.release_root(client.request_root);
    result
}

fn abort_exchange_stream(
    context: &mut Context<'_>,
    client: &super::state::Client,
) -> Result<(), RootedError> {
    let error = context.error_rooted("socket hang up")?;
    if let Err(failure) = set_text(context, error, "code", "ECONNRESET") {
        context.release_root(error);
        return Err(failure);
    }
    let stream = client.response_root.unwrap_or(client.request_root);
    let destroy_key = context.string_rooted("destroy");
    let destroy = match context.get_property_rooted(stream, destroy_key) {
        Ok(destroy) => destroy,
        Err(failure) => {
            context.release_root(destroy_key);
            context.release_root(error);
            return Err(failure);
        }
    };
    let result = context.call_rooted(destroy, stream, &[error]);
    context.release_root(destroy);
    context.release_root(destroy_key);
    context.release_root(error);
    match result {
        Ok(value) => {
            context.release_root(value);
            Ok(())
        }
        Err(failure) => Err(failure),
    }
}

pub(crate) fn destroy_response(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let state = context.host_mut().shared_state();
    let client = {
        let mut host = state.borrow_mut();
        let socket = host
            .http
            .clients
            .iter()
            .find_map(|(socket, client)| (client.request_id == id).then_some(*socket));
        socket.and_then(|socket| {
            let client = host.http.clients.remove(&socket)?;
            close_agent_socket(&mut host, pooled_origin(&client), socket);
            Some(client)
        })
    };
    if let Some(client) = client {
        if let Some(response) = client.response_root {
            context.release_root(response);
        }
        context.release_root(client.request_root);
    }
    Ok(context.undefined())
}

fn serialize(request: &super::state::OutgoingRequest) -> Vec<u8> {
    let body_len = request.body.len();
    let has_length = request
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let has_chunked = request.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    });
    let chunked = has_chunked
        || (request.body.has_writes()
            && !has_length
            && crate::modules::http_protocol::default_request_chunking(&request.method));
    let host_header = if request.port == 80 {
        request.host.clone()
    } else {
        format!("{}:{}", request.host, request.port)
    };
    let head = crate::modules::http_protocol::request_head_with_chunking(
        &host_header,
        &request.method,
        &request.path,
        &request.headers,
        body_len,
        false,
        chunked,
    );
    let mut bytes = head.into_bytes();
    if chunked {
        for chunk in request.body.chunks() {
            bytes.extend_from_slice(&crate::modules::http_protocol::chunk_frame(chunk));
        }
        bytes.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&[]));
    } else {
        bytes.extend_from_slice(&request.body.bytes());
    }
    bytes
}

fn read_headers(
    context: &mut Context<'_>,
    headers: Option<RootId>,
) -> Result<Vec<(String, String)>, RootedError> {
    let Some(headers) = headers else {
        return Ok(Vec::new());
    };
    let items = context.array_like_values_rooted(headers)?;
    let pairs = items.chunks_exact(HEADER_PAIR_FIELDS);
    if !pairs.remainder().is_empty() {
        return Err(RootedError::host("HTTP header pair list is malformed"));
    }
    let mut result = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let &[name, value] = pair else {
            unreachable!("chunks_exact yields name and value pairs");
        };
        let name = context.to_string(name)?;
        let value = context.to_string(value)?;
        if name.is_empty()
            || !name
                .chars()
                .all(crate::modules::http_protocol::is_http_token_char)
        {
            let error = context.type_error_rooted("Invalid HTTP header name")?;
            return Err(context.throw(error));
        }
        if !crate::modules::http_protocol::valid_header_value(&value) {
            let error = context.type_error_rooted("Invalid HTTP header value")?;
            return Err(context.throw(error));
        }
        result.push((name, value));
    }
    Ok(result)
}

/// A header pair list's values per header: its name and its value.
const HEADER_PAIR_FIELDS: usize = 2;

fn argument_id(context: &mut Context<'_>, root: Option<RootId>) -> Result<u64, RootedError> {
    root.and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP request identifier"))
}

fn port_property(context: &mut Context<'_>, object: RootId) -> Result<Option<f64>, RootedError> {
    let key = context.string_rooted("port");
    let value_root = context.get_property_rooted(object, key)?;
    match context.rooted_value(value_root) {
        Some(value) => match value.as_number() {
            Some(number) => Ok(Some(number)),
            None => Ok(context
                .string_text(value_root)?
                .and_then(|port| port.parse::<u16>().ok())
                .map(f64::from)),
        },
        None => Ok(None),
    }
}

fn text_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<String>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    match context.rooted_value(value) {
        Some(rooted_value) if !rooted_value.is_undefined() && !rooted_value.is_null() => {
            context.to_string(value).map(Some)
        }
        _ => Ok(None),
    }
}

fn object_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<RootId>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    Ok(context
        .rooted_value(value)
        .filter(|value| !value.is_undefined() && !value.is_null())
        .map(|_| value))
}

fn number_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<f64>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    Ok(context.rooted_value(value).and_then(Value::as_number))
}

fn boolean_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<bool>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    Ok(context.rooted_value(value).and_then(Value::as_bool))
}

/// A positive socket count; non-finite or non-positive values mean no limit, as
/// Node's `Agent.defaultMaxSockets` does.
fn socket_limit(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<usize>, RootedError> {
    Ok(number_property(context, object, name)?.map(|value| {
        if value.is_finite() && value >= 1.0 {
            value as usize
        } else {
            usize::MAX
        }
    }))
}

fn property_id(
    context: &mut Context<'_>,
    object: RootId,
    property: &str,
) -> Result<Option<u64>, RootedError> {
    let key = context.string_rooted(property);
    let value = context.get_property_rooted(object, key)?;
    Ok(context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64))
}
