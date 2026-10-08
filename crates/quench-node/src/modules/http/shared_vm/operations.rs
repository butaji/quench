use crate::host::NodeHost;
use crate::modules::net;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

type Context<'a> = NativeContext<'a, NodeHost>;

const MODULE_FACTORY: &str = r#"((createServer, get, createAgent, destroyAgent, EventEmitter) => {
  class Server extends EventEmitter {
    constructor(optionsOrListener, listener) {
      const callback = typeof optionsOrListener === 'function' ? optionsOrListener : listener;
      const server = createServer(callback);
      server.setTimeout = () => server;
      Object.setPrototypeOf(server, new.target.prototype);
      return server;
    }
  }

  class IncomingMessage extends EventEmitter {
    constructor(response) {
      super();
      Object.assign(this, response);
      if (this.headers) Object.setPrototypeOf(this.headers, null);
      this.readable = true;
      this._quenchEnded = false;
      this._quenchBody = response.body || '';
      this._quenchEncoding = undefined;
    }
    resume() {
      if (!this._quenchEnded) {
        this._quenchEnded = true;
        queueMicrotask(() => {
          if (this._quenchBody.length > 0) this.emit('data', this._quenchBody);
          this.readable = false;
          this.emit('end');
        });
      }
      return this;
    }
    setEncoding(encoding) { this._quenchEncoding = String(encoding); return this; }
    pause() { return this; }
    on(name, listener) {
      const result = super.on(name, listener);
      if (name === 'data') this.resume();
      return result;
    }
  }

  class ServerResponse extends EventEmitter {}

  const createServerExport = (optionsOrListener, listener) => new Server(optionsOrListener, listener);
  const getExport = (options, callback, method = 'GET', body = '', chunked = false) => get(options, (response) => {
    if (typeof callback === 'function') callback(new IncomingMessage(response));
  }, method, body, chunked);

  class ClientRequest extends EventEmitter {
    constructor(options, callback) {
      super();
      this.options = options;
      this.callback = callback;
      this._chunks = [];
      this._chunked = false;
    }
    write(chunk) {
      this._chunks.push(String(chunk));
      this._chunked = true;
      return true;
    }
    removeHeader() { return this; }
    end(chunk) {
      if (chunk !== undefined && chunk !== null) this._chunks.push(String(chunk));
      const body = this._chunks.join('');
      const method = this.options.method || 'GET';
      getExport(this.options, (response) => {
        this.emit('response', response);
        if (typeof this.callback === 'function') this.callback(response);
      }, method, body, this._chunked);
      return this;
    }
  }

  class Agent {
    constructor(options) {
      this._quenchSharedAgentId = createAgent(options);
    }
    destroy() {
      return destroyAgent(this._quenchSharedAgentId);
    }
  }
  return {
    createServer: createServerExport,
    Server,
    IncomingMessage,
    ServerResponse,
    request: (options, callback) => new ClientRequest(options, callback),
    get: getExport,
    Agent,
    requestFactory: (method, path, version, headersJSON, body) => {
      const request = new EventEmitter();
      request.method = method;
      request.url = path;
      request.httpVersion = version;
      request.httpVersionMajor = 1;
      request.httpVersionMinor = 1;
      request.headers = Object.create(null);
      for (const [name, value] of JSON.parse(headersJSON)) request.headers[name] = value;
      request.complete = true;
      request.readable = true;
      request.socket = Object.assign(new EventEmitter(), {
        readable: true,
        writable: true,
        remoteAddress: '127.0.0.1',
        remotePort: 0,
        localAddress: '127.0.0.1',
        localPort: 0,
      });
      request.connection = request.socket;
      request.setEncoding = (encoding) => {
        request._quenchEncoding = String(encoding);
        return request;
      };
      request.unpipe = () => request;
      request.resume = () => request;
      request.pause = () => request;
      request._quenchBody = body;
      request._quenchBodyBuffer = Buffer.from(body);
      request[Symbol.asyncIterator] = () => {
        let delivered = false;
        return {
          next() {
            if (delivered) return { value: undefined, done: true };
            delivered = true;
            return { value: request._quenchBodyBuffer, done: false };
          },
        };
      };
      return request;
    },
  };
})"#;

const RESPONSE_FACTORY: &str = r#"((setHeader, removeHeader, end, ServerResponse) => (id, statusCode) => {
  const response = new ServerResponse();
  Object.assign(response, {
    statusCode,
    statusMessage: 'OK',
    headersSent: false,
    writable: true,
    writableEnded: false,
    writableFinished: false,
    finished: false,
    destroyed: false,
    closed: false,
  });
  const chunks = [];
  const headers = Object.create(null);
  response.setHeader = (name, value) => {
    setHeader(id, name, value);
    const key = String(name).toLowerCase();
    headers[key] = value;
    return response;
  };
  response.getHeader = (name) => headers[String(name).toLowerCase()];
  response.hasHeader = (name) => Object.hasOwn(headers, String(name).toLowerCase());
  response.getHeaders = () => ({ ...headers });
  response.removeHeader = (name) => {
    const key = String(name).toLowerCase();
    delete headers[key];
    removeHeader(id, key);
    return response;
  };
  response.write = (chunk) => {
    if (!response.headersSent) response.setHeader('Transfer-Encoding', 'chunked');
    chunks.push(String(chunk));
    return true;
  };
  response.writeHead = (status, headers = undefined) => {
    if (response.headersSent) {
      throw Object.assign(new Error('Cannot write headers after they are sent to the client'), {
        code: 'ERR_HTTP_HEADERS_SENT',
      });
    }
    if (Array.isArray(headers)) {
      if (headers.length % 2 !== 0) {
        throw Object.assign(new TypeError('Invalid headers argument'), {
          code: 'ERR_INVALID_ARG_VALUE',
        });
      }
      for (let index = 0; index < headers.length; index += 2) {
        response.setHeader(headers[index], headers[index + 1]);
      }
    } else if (headers && typeof headers === 'object') {
      for (const [name, value] of Object.entries(headers)) response.setHeader(name, value);
    } else if (headers !== undefined) {
      throw Object.assign(new TypeError('Invalid headers argument'), {
        code: 'ERR_INVALID_ARG_VALUE',
      });
    }
    response.statusCode = status;
    response.statusMessage = status === 220 ? 'unknown' : 'OK';
    response.headersSent = true;
    return response;
  };
  response.end = (chunk) => {
    if (chunk !== undefined && chunk !== null) chunks.push(String(chunk));
    end(id, response.statusCode, chunks.join(''));
    response.headersSent = true;
    response.writableEnded = true;
    response.writableFinished = true;
    response.finished = true;
    response.emit('finish');
    return response;
  };
  return response;
})"#;

pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let create_server =
        context.host_function(crate::host::shared_vm::operation("httpCreateServer"))?;
    let get = context.host_function(crate::host::shared_vm::operation("httpGet"))?;
    let create_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentCreate"))?;
    let destroy_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentDestroy"))?;
    let global = context.global_root()?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    let factory = context.evaluate_script_rooted(MODULE_FACTORY, "node:http/shared-api.js")?;
    let undefined = context.undefined();
    let module = context.call_rooted(
        factory,
        undefined,
        &[create_server, get, create_agent, destroy_agent, emitter],
    )?;
    let methods = crate::modules::http::HTTP_METHODS
        .iter()
        .map(|method| context.string_rooted(method))
        .collect::<Vec<_>>();
    let methods = context.array_rooted(&methods)?;
    set(context, module, "METHODS", methods)?;
    let set_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseSetHeader"))?;
    let remove_header = context.host_function(crate::host::shared_vm::operation(
        "httpResponseRemoveHeader",
    ))?;
    let end = context.host_function(crate::host::shared_vm::operation("httpResponseEnd"))?;
    let response_class_key = context.string_rooted("ServerResponse");
    let response_class = context.get_property_rooted(module, response_class_key)?;
    let factory =
        context.evaluate_script_rooted(RESPONSE_FACTORY, "node:http/response-factory.js")?;
    let undefined = context.undefined();
    let factory = context.call_rooted(
        factory,
        undefined,
        &[set_header, remove_header, end, response_class],
    )?;
    let factory = context.retain(factory)?;
    context
        .host_mut()
        .state()
        .borrow_mut()
        .http
        .shared
        .response_factory = Some(factory);
    let request_factory_key = context.string_rooted("requestFactory");
    let request_factory = context.get_property_rooted(module, request_factory_key)?;
    let request_factory = context.retain(request_factory)?;
    context
        .host_mut()
        .state()
        .borrow_mut()
        .http
        .shared
        .request_factory = Some(request_factory);
    Ok(module)
}

pub(crate) fn create_server(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    if !context.is_callable_rooted(emitter)? {
        return Err(RootedError::host(
            "shared EventEmitter constructor is missing",
        ));
    }
    let server = context.construct_rooted(emitter, emitter, &[])?;
    if let Some(listener) = args.first().copied() {
        if context.is_callable_rooted(listener)? {
            let on_key = context.string_rooted("on");
            let on = context.get_property_rooted(server, on_key)?;
            let event = context.string_rooted("request");
            context.call_rooted(on, server, &[event, listener])?;
        }
    }
    let id = context
        .host_mut()
        .state()
        .borrow_mut()
        .http
        .shared
        .server_id()
        .map_err(RootedError::host)?;
    let id_value = context.number(id as f64);
    for (name, operation) in [
        ("listen", "httpServerListen"),
        ("address", "httpServerAddress"),
        ("close", "httpServerClose"),
    ] {
        let function = context
            .host_function_with_data(crate::host::shared_vm::operation(operation), id_value)?;
        set(context, server, name, function)?;
    }
    let listening = context.boolean(false);
    set(context, server, "listening", listening)?;
    let retained = context.retain(server)?;
    context
        .host_mut()
        .state()
        .borrow_mut()
        .http
        .shared
        .servers
        .insert(
            id,
            super::state::Server {
                root: retained,
                listener: None,
                listening_pending: false,
                closing: false,
                connections: Default::default(),
            },
        );
    Ok(server)
}

pub(crate) fn server_listen(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let listen_options = args.first().copied();
    let port = listen_options
        .and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .or(match listen_options {
            Some(options) => numeric_property(context, options, "port")?,
            None => None,
        })
        .filter(|port| port.is_finite() && (0.0..=u16::MAX as f64).contains(port))
        .map(|port| port as u16)
        .ok_or_else(|| RootedError::host("shared HTTP listen requires a numeric port"))?;
    let host = context.host_mut().state();
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    {
        let mut guard = host.borrow_mut();
        let Some(server) = guard.http.shared.servers.get(&id) else {
            return Err(RootedError::host("shared HTTP server is closed"));
        };
        if server.listener.is_some() {
            return Err(RootedError::host("shared HTTP server is already listening"));
        }
        let listener =
            net::shared_vm::listen(&mut guard.net, address).map_err(RootedError::host)?;
        let server = guard.http.shared.servers.get_mut(&id).unwrap();
        server.listener = Some(listener);
        server.listening_pending = true;
    }
    let listening = context.boolean(true);
    set(context, receiver, "listening", listening)?;
    if let Some(callback) = args.get(1).copied() {
        if context.is_callable_rooted(callback)? {
            let once_key = context.string_rooted("once");
            let once = context.get_property_rooted(receiver, once_key)?;
            let event = context.string_rooted("listening");
            context.call_rooted(once, receiver, &[event, callback])?;
        }
    }
    Ok(receiver)
}

pub(crate) fn server_address(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let host = context.host_mut().state();
    let address = {
        let guard = host.borrow();
        guard
            .http
            .shared
            .servers
            .get(&id)
            .and_then(|server| server.listener)
            .and_then(|listener| net::shared_vm::address(&guard.net, listener))
    };
    let Some(address) = address else {
        return Ok(context.null());
    };
    let object = context.object_rooted()?;
    let address_value = context.string_rooted("127.0.0.1");
    set(context, object, "address", address_value)?;
    let family = context.string_rooted("IPv4");
    set(context, object, "family", family)?;
    let port = context.number(address.port() as f64);
    set(context, object, "port", port)?;
    Ok(object)
}

pub(crate) fn server_close(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let host = context.host_mut().state();
    let mut guard = host.borrow_mut();
    if let Some(server) = guard.http.shared.servers.get_mut(&id) {
        server.closing = true;
        if let Some(listener) = server.listener.take() {
            net::shared_vm::close_listener(&mut guard.net, listener);
        }
    }
    drop(guard);
    let listening = context.boolean(false);
    set(context, receiver, "listening", listening)?;
    if let Some(callback) = args.first().copied() {
        if context.is_callable_rooted(callback)? {
            let once_key = context.string_rooted("once");
            let once = context.get_property_rooted(receiver, once_key)?;
            let event = context.string_rooted("close");
            context.call_rooted(once, receiver, &[event, callback])?;
        }
    }
    Ok(receiver)
}

pub(crate) fn agent_create(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context
        .host_mut()
        .state()
        .borrow_mut()
        .http
        .shared
        .agent_id()
        .map_err(RootedError::host)?;
    Ok(context.number(id as f64))
}

pub(crate) fn agent_destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(agent) = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
    else {
        return Ok(context.undefined());
    };
    let callbacks = {
        let host = context.host_mut().state();
        let mut guard = host.borrow_mut();
        let Some(sockets) = guard.http.shared.agents.remove(&agent) else {
            return Ok(context.undefined());
        };
        let mut callbacks = Vec::new();
        for socket in sockets {
            net::shared_vm::close_socket(&mut guard.net, socket);
            if let Some(client) = guard.http.shared.clients.remove(&socket) {
                callbacks.push(client.callback);
            }
            guard
                .http
                .shared
                .responses
                .retain(|_, response| response.socket != socket);
        }
        callbacks
    };
    for callback in callbacks {
        context.release_root(callback);
    }
    Ok(context.undefined())
}

pub(crate) fn get(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("http.get options are missing"))?;
    let port = numeric_property(context, options, "port")?
        .filter(|port| *port > 0.0 && *port <= u16::MAX as f64)
        .ok_or_else(|| RootedError::host("shared http.get requires a valid port"))?
        as u16;
    let request_host = text_property(context, options, "host")?
        .or(text_property(context, options, "hostname")?)
        .unwrap_or_else(|| "localhost".to_owned());
    let path = text_property(context, options, "path")?.unwrap_or_else(|| "/".to_owned());
    let method = args
        .get(2)
        .filter(|root| {
            context
                .rooted_value(**root)
                .is_some_and(|value| !value.is_undefined())
        })
        .map(|root| context.to_string(*root))
        .transpose()?
        .unwrap_or_else(|| "GET".to_owned());
    let body = args
        .get(3)
        .filter(|root| {
            context
                .rooted_value(**root)
                .is_some_and(|value| !value.is_undefined())
        })
        .map(|root| context.to_string(*root))
        .transpose()?
        .unwrap_or_default();
    let chunked = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut headers = text_headers_property(context, options, "headers")?;
    let host_header = if port == 80 {
        request_host.clone()
    } else {
        format!("{request_host}:{port}")
    };
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        headers.push(("Host".to_owned(), host_header));
    }
    if chunked
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    {
        headers.push(("Transfer-Encoding".to_owned(), "chunked".to_owned()));
    } else if !chunked
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        && (!body.is_empty() || matches!(method.to_ascii_uppercase().as_str(), "POST" | "PUT"))
    {
        headers.push(("Content-Length".to_owned(), body.len().to_string()));
    }
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("connection"))
    {
        headers.push(("Connection".to_owned(), "keep-alive".to_owned()));
    }
    let mut request = format!("{method} {path} HTTP/1.1\r\n").into_bytes();
    for (name, value) in &headers {
        request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    if chunked {
        if !body.is_empty() {
            request.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
            request.extend_from_slice(body.as_bytes());
            request.extend_from_slice(b"\r\n");
        }
        request.extend_from_slice(b"0\r\n\r\n");
    } else {
        request.extend_from_slice(body.as_bytes());
    }
    let callback = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("http.get callback is missing"))?;
    if !context.is_callable_rooted(callback)? {
        let error = context.type_error_rooted("http.get callback must be a function")?;
        return Err(context.throw(error));
    }
    let agent_object = object_property(context, options, "agent")?;
    let requested_agent = match agent_object {
        Some(agent) => property_id(context, agent, "_quenchSharedAgentId")?,
        None => None,
    };
    let agent = requested_agent.filter(|agent| {
        context
            .host_mut()
            .state()
            .borrow()
            .http
            .shared
            .agents
            .contains_key(agent)
    });
    let socket = {
        let state = context.host_mut().state();
        let socket = net::shared_vm::connect(&mut state.borrow_mut().net, request_host, port)
            .map_err(RootedError::host)?;
        socket
    };
    let callback = match context.retain(callback) {
        Ok(callback) => callback,
        Err(error) => {
            let state = context.host_mut().state();
            net::shared_vm::close_socket(&mut state.borrow_mut().net, socket);
            return Err(error);
        }
    };
    {
        let host = context.host_mut().state();
        let mut guard = host.borrow_mut();
        if let Some(agent_id) = agent {
            if let Some(sockets) = guard.http.shared.agents.get_mut(&agent_id) {
                sockets.insert(socket);
            }
        }
        guard.http.shared.clients.insert(
            socket,
            super::state::Client {
                callback,
                request,
                received: Vec::new(),
            },
        );
    }
    context.object_rooted()
}

pub(crate) fn response_set_header(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP response identifier"))?;
    let name_root = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("response header name is missing"))?;
    let value_root = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("response header value is missing"))?;
    let Some(name) = context.string_text(name_root)? else {
        let rendered = context.to_string(name_root)?;
        let error = context.type_error_rooted(&format!(
            "Header name must be a valid HTTP token [{rendered:?}]"
        ))?;
        let code = context.string_rooted("ERR_INVALID_HTTP_TOKEN");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    };
    if name.is_empty()
        || !name
            .chars()
            .all(crate::modules::http_res::is_http_token_char)
    {
        let error = context.type_error_rooted(&format!(
            "Header name must be a valid HTTP token [{name:?}]"
        ))?;
        let code = context.string_rooted("ERR_INVALID_HTTP_TOKEN");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    if context
        .rooted_value(value_root)
        .is_some_and(|value| value.is_undefined())
    {
        let error = context.type_error_rooted(&format!(
            "Invalid value \"undefined\" for header \"{name}\""
        ))?;
        let code = context.string_rooted("ERR_HTTP_INVALID_HEADER_VALUE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let value = context.to_string(value_root)?;
    if !crate::modules::http_res::valid_header_value(&value) {
        let error = context.type_error_rooted("Invalid HTTP header value")?;
        return Err(context.throw(error));
    }
    let ended = context
        .host_mut()
        .state()
        .borrow()
        .http
        .shared
        .responses
        .get(&id)
        .is_some_and(|response| response.ended);
    if ended {
        let error = context.type_error_rooted("Cannot set headers after they are sent")?;
        return Err(context.throw(error));
    }
    let state = context.host_mut().state();
    let mut host = state.borrow_mut();
    let response = host
        .http
        .shared
        .responses
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    response
        .headers
        .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
    response.headers.push((name, value));
    Ok(receiver)
}

pub(crate) fn response_remove_header(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP response identifier"))?;
    let name = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("response header name is missing"))?;
    let name = context.to_string(name)?;
    let ended = context
        .host_mut()
        .state()
        .borrow()
        .http
        .shared
        .responses
        .get(&id)
        .is_some_and(|response| response.ended);
    if ended {
        let error = context.type_error_rooted("Cannot remove headers after they are sent")?;
        let code = context.string_rooted("ERR_HTTP_HEADERS_SENT");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let state = context.host_mut().state();
    let mut host = state.borrow_mut();
    let response = host
        .http
        .shared
        .responses
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    if name.eq_ignore_ascii_case("date") {
        response.send_date = false;
    }
    response
        .headers
        .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
    Ok(receiver)
}

pub(crate) fn response_end(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP response identifier"))?;
    let status = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|status| (100.0..600.0).contains(status))
        .map(|status| status as u16)
        .unwrap_or(200);
    let mut body = Vec::new();
    if let Some(chunk) = args.get(2).copied() {
        let value = context.rooted_value(chunk);
        if value.is_some_and(|value| !value.is_undefined() && !value.is_null()) {
            body = context.to_string(chunk)?.into_bytes();
        }
    }
    let response = {
        let state = context.host_mut().state();
        let mut host = state.borrow_mut();
        let response = host
            .http
            .shared
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        if response.ended {
            None
        } else {
            response.ended = true;
            Some((
                response.socket,
                response.headers.clone(),
                response.send_date,
            ))
        }
    };
    let Some((socket, headers, send_date)) = response else {
        return Ok(context.undefined());
    };
    let bytes = crate::modules::http_res::compose(
        status,
        "OK",
        &headers,
        &body,
        &[],
        true,
        false,
        send_date,
    );
    net::shared_vm::write(
        &mut context.host_mut().state().borrow_mut().net,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    Ok(context.undefined())
}

fn function_data_id(context: &mut Context<'_>) -> Result<u64, RootedError> {
    let data = context.host_function_data()?;
    context
        .rooted_value(data)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP operation identifier"))
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
    let value_root = context.get_property_rooted(object, key)?;
    match context.rooted_value(value_root) {
        Some(value) if !value.is_undefined() && !value.is_null() => {
            context.to_string(value_root).map(Some)
        }
        _ => Ok(None),
    }
}

fn text_headers_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Vec<(String, String)>, RootedError> {
    let Some(headers) = object_property(context, object, name)? else {
        return Ok(Vec::new());
    };
    let global = context.global_root()?;
    let object_key = context.string_rooted("Object");
    let object_constructor = context.get_property_rooted(global, object_key)?;
    let keys_key = context.string_rooted("keys");
    let keys_function = context.get_property_rooted(object_constructor, keys_key)?;
    let keys = context.call_rooted(keys_function, object_constructor, &[headers])?;
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(keys, length_key)?;
    let count = context
        .rooted_value(length)
        .and_then(Value::as_number)
        .filter(|length| length.is_finite() && *length >= 0.0)
        .unwrap_or(0.0)
        .trunc() as usize;
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let index_key = context.string_rooted(&index.to_string());
        let name_root = context.get_property_rooted(keys, index_key)?;
        let name = context.to_string(name_root)?;
        let key = context.string_rooted(&name);
        let value_root = context.get_property_rooted(headers, key)?;
        let value = context.to_string(value_root)?;
        result.push((name, value));
    }
    Ok(result)
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
            "cannot install shared http property {name}"
        )))
    }
}
