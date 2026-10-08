use crate::host::NodeHost;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError, Value};

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
  const request = (input = {}, callback) => {
    let options = input;
    if (typeof input === "string" || (input && typeof input.href === "string")) {
      options = normalizeUrl(String(input));
    }
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
    let port = numeric_property(context, options, "port")?
        .filter(|port| port.is_finite() && (0.0..=u16::MAX as f64).contains(port))
        .unwrap_or(80.0) as u16;
    let method = text_property(context, options, "method")?.unwrap_or_else(|| "GET".to_owned());
    let path = text_property(context, options, "path")?.unwrap_or_else(|| "/".to_owned());
    let headers = read_headers(context, args.get(1).copied())?;
    let agent = object_property(context, options, "agent")?
        .map(|agent| property_id(context, agent, "_quenchSharedAgentId"))
        .transpose()?
        .flatten();
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
            agent,
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
    let socket = {
        let state = context.host_mut().shared_state();
        let result = net_shared_vm::connect(
            &mut state.borrow_mut().tcp,
            request.host.clone(),
            request.port,
        );
        result.map_err(RootedError::host)?
    };
    let request_root = match context.retain(request_root) {
        Ok(root) => root,
        Err(error) => {
            let state = context.host_mut().shared_state();
            net_shared_vm::close_socket(&mut state.borrow_mut().tcp, socket);
            return Err(error);
        }
    };
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    if let Some(agent) = request.agent {
        if let Some(sockets) = host.http.agents.get_mut(&agent) {
            sockets.insert(socket);
        }
    }
    host.http.clients.insert(
        socket,
        super::state::Client {
            request_id: id,
            request_root,
            request: request_bytes,
            response_parser: super::protocol::ResponseParser::for_method(&request.method),
            response_root: None,
        },
    );
    Ok(context.undefined())
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
        let socket = host
            .http
            .clients
            .iter()
            .find_map(|(socket, client)| (client.request_id == id).then_some(*socket));
        socket.and_then(|socket| {
            let client = host.http.clients.remove(&socket)?;
            for sockets in host.http.agents.values_mut() {
                sockets.remove(&socket);
            }
            net_shared_vm::close_socket(&mut host.tcp, socket);
            Some(client)
        })
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
            for sockets in host.http.agents.values_mut() {
                sockets.remove(&socket);
            }
            net_shared_vm::close_socket(&mut host.tcp, socket);
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
    let chunked = has_chunked || (request.body.has_writes() && !has_length);
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
    let length = numeric_property(context, headers, "length")?
        .filter(|length| length.is_finite() && *length >= 0.0)
        .unwrap_or(0.0) as usize;
    if length % 2 != 0 {
        return Err(RootedError::host("HTTP header pair list is malformed"));
    }
    let mut result = Vec::with_capacity(length / 2);
    for index in (0..length).step_by(2) {
        let name = array_item(context, headers, index)?;
        let value = array_item(context, headers, index + 1)?;
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

fn array_item(
    context: &mut Context<'_>,
    array: RootId,
    index: usize,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(&index.to_string());
    context.get_property_rooted(array, key)
}

fn argument_id(context: &mut Context<'_>, root: Option<RootId>) -> Result<u64, RootedError> {
    root.and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP request identifier"))
}

fn numeric_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<f64>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    Ok(context.rooted_value(value).and_then(Value::as_number))
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
