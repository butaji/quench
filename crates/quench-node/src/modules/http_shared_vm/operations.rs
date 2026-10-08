use crate::host::NodeHost;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

type Context<'a> = NativeContext<'a, NodeHost>;

const DEFAULT_LISTEN_HOST: &str = "127.0.0.1";

const MODULE_FACTORY: &str = r#"((initializeServer, EventEmitter, request, createAgent, destroyAgent, IncomingMessage, ServerResponse) => {
  const invalidArgument = (name, expected, value) => {
    let received;
    if (Array.isArray(value)) received = "an instance of Array";
    else if (value === null) received = "null";
    else if (typeof value === "string") received = `type string (${JSON.stringify(value)})`;
    else if (typeof value === "number" || typeof value === "boolean") {
      received = `type ${typeof value} (${String(value)})`;
    } else received = `type ${typeof value}`;
    const error = new TypeError(`The \"${name}\" argument must be ${expected}. Received ${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };
  class Server extends EventEmitter {
    constructor(options, requestListener) {
      super();
      if (typeof options === "function") {
        requestListener = options;
        options = undefined;
      } else if (options !== undefined &&
          (options === null || typeof options !== "object" || Array.isArray(options))) {
        throw invalidArgument("options", "of type object", options);
      }
      if (requestListener !== undefined && typeof requestListener !== "function") {
        throw invalidArgument("requestListener", "of type function", requestListener);
      }
      this.timeout = 0;
      initializeServer([this, requestListener]);
    }
    setTimeout(msecs, callback) {
      this.timeout = msecs;
      if (callback !== undefined) this.on("timeout", callback);
      return this;
    }
  }
  const createServer = (...args) => new Server(...args);
  class Agent {
    constructor(options) {
      this._quenchSharedAgentId = createAgent(options);
    }
    destroy() {
      return destroyAgent(this._quenchSharedAgentId);
    }
  }
  const get = (options, callback) => {
    const outgoing = request(options, callback);
    outgoing.end();
    return outgoing;
  };
  return { Server, createServer, request, get, Agent, IncomingMessage, ServerResponse };
})"#;

const SERVER_CLOSE_FACTORY: &str = r#"((closeOperation) => function(callback) {
  if (callback !== undefined) this.once("close", callback);
  closeOperation.call(this);
  return this;
})"#;

const RESPONSE_FACTORY: &str = r#"((Writable, setHeaderOperation, getHeaderOperation, removeHeader, write, finish, writeHeadOperation, destroyOperation) => {
  function ServerResponse(id, statusCode, request, server, socket) {
    if (typeof id !== "number") {
      socket = undefined;
      server = undefined;
      request = id;
      id = 0;
      statusCode = 200;
    }
    let response;
    let ending = false;
    const normalizeHeaderValue = (value) => {
      if (value === undefined) return [value, false];
      if (Array.isArray(value)) return [value.map((item) => String(item)), true];
      return [String(value), false];
    };
    response = new Writable({
      // Writable `finish` completes the response body, while the socket owns
      // the response's transport lifetime.
      autoDestroy: false,
      write(chunk, encoding, callback) { callback(); },
      final(callback) {
        try { finish(id, response.statusCode, response.statusMessage, request, response, server, socket); callback(); }
        catch (error) { callback(error); }
      },
      destroy(error, callback) {
        try { destroyOperation(id); callback(error); }
        catch (failure) { callback(failure); }
      },
    });
    Object.setPrototypeOf(response, ServerResponse.prototype);
    response.finished = false;
    response.statusCode = statusCode;
    response.statusMessage = "OK";
    const writeStream = response.write.bind(response);
    response.write = (chunk, encoding, callback) => {
      const result = writeStream(chunk, encoding, callback);
      write(id, chunk, !ending, response.statusCode, response.statusMessage);
      response.headersSent = true;
      return result;
    };
    const endStream = response.end.bind(response);
    response.end = (chunk, encoding, callback) => {
      ending = true;
      response.finished = true;
      return endStream(chunk, encoding, callback);
    };
    response.setHeader = (name, value) => {
      const [normalized, multiple] = normalizeHeaderValue(value);
      setHeaderOperation(id, name, normalized, multiple);
      return response;
    };
    response.getHeader = (name) => getHeaderOperation(id, name);
    response.removeHeader = (name) => removeHeader(id, name);
    response.writeHead = (status, reasonOrHeaders, headers) => {
      const hasReason = typeof reasonOrHeaders === "string";
      const reason = hasReason ? reasonOrHeaders : undefined;
      const source = hasReason ? headers : reasonOrHeaders;
      const entries = [];
      let invalid = false;
      if (source !== undefined) {
        if (Array.isArray(source)) {
          if (source.length % 2 !== 0) invalid = true;
          else {
            for (let index = 0; index < source.length; index += 2) {
              const [value, multiple] = normalizeHeaderValue(source[index + 1]);
              entries.push(source[index], value, multiple);
            }
          }
        } else if (source !== null && typeof source === "object") {
          for (const name of Object.keys(source)) {
            const [value, multiple] = normalizeHeaderValue(source[name]);
            entries.push(name, value, multiple);
          }
        } else {
          invalid = true;
        }
      }
      const message = writeHeadOperation(id, status, reason, entries, invalid);
      response.statusCode = status;
      response.statusMessage = message;
      response.headersSent = true;
      return response;
    };
    return response;
  }
  ServerResponse.prototype = Object.create(Writable.prototype);
  Object.defineProperty(ServerResponse.prototype, "constructor", {
    value: ServerResponse, writable: true, configurable: true,
  });
  return ServerResponse;
})"#;

const INCOMING_FACTORY: &str = r#"((Readable, Buffer, EventEmitter, destroyClientResponse) => {
  function IncomingMessage(message = {}, body = "", open = false, exchangeId) {
    const options = { read() {} };
    if (open && exchangeId !== undefined) {
      options.destroy = (error, callback) => {
        try { destroyClientResponse(exchangeId); callback(error); }
        catch (failure) { callback(failure); }
      };
    }
    const stream = new Readable(options);
    Object.setPrototypeOf(stream, IncomingMessage.prototype);
    Object.assign(stream, message);
    if (stream.socket && typeof stream.socket.on !== "function") {
      Object.setPrototypeOf(stream.socket, EventEmitter.prototype);
    }
    if (body.length) stream.push(Buffer.from(body, "latin1"));
    if (!open) stream.push(null);
    return stream;
  }
  IncomingMessage.prototype = Object.create(Readable.prototype);
  Object.defineProperty(IncomingMessage.prototype, "constructor", {
    value: IncomingMessage, writable: true, configurable: true,
  });
  return IncomingMessage;
})"#;

pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let set_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseSetHeader"))?;
    let get_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseGetHeader"))?;
    let remove_header = context.host_function(crate::host::shared_vm::operation(
        "httpResponseRemoveHeader",
    ))?;
    let write = context.host_function(crate::host::shared_vm::operation("httpResponseWrite"))?;
    let finish = context.host_function(crate::host::shared_vm::operation("httpResponseFinish"))?;
    let write_head =
        context.host_function(crate::host::shared_vm::operation("httpResponseWriteHead"))?;
    let destroy_response =
        context.host_function(crate::host::shared_vm::operation("httpResponseDestroy"))?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let writable_key = context.string_rooted("Writable");
    let writable = context.get_property_rooted(stream, writable_key)?;
    let response_factory =
        context.evaluate_script_rooted(RESPONSE_FACTORY, "node:http/response-factory.js")?;
    let undefined = context.undefined();
    let response_factory = context.call_rooted(
        response_factory,
        undefined,
        &[
            writable,
            set_header,
            get_header,
            remove_header,
            write,
            finish,
            write_head,
            destroy_response,
        ],
    )?;
    let response_factory = context.retain(response_factory)?;
    let global = context.global_root()?;
    let buffer_key = context.string_rooted("Buffer");
    let buffer_constructor = context.get_property_rooted(global, buffer_key)?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    let destroy_client_response = context.host_function(crate::host::shared_vm::operation(
        "httpClientResponseDestroy",
    ))?;
    let readable_key = context.string_rooted("Readable");
    let readable = context.get_property_rooted(stream, readable_key)?;
    let incoming_factory =
        context.evaluate_script_rooted(INCOMING_FACTORY, "node:http/incoming-message.js")?;
    let incoming_factory = context.call_rooted(
        incoming_factory,
        undefined,
        &[
            readable,
            buffer_constructor,
            emitter,
            destroy_client_response,
        ],
    )?;
    let incoming_factory = context.retain(incoming_factory)?;

    let initialize_server =
        context.host_function(crate::host::shared_vm::operation("httpCreateServer"))?;
    let request = super::client::request_function(context)?;
    let create_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentCreate"))?;
    let destroy_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentDestroy"))?;
    let factory = context.evaluate_script_rooted(MODULE_FACTORY, "node:http/shared-api.js")?;
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            initialize_server,
            emitter,
            request,
            create_agent,
            destroy_agent,
            incoming_factory,
            response_factory,
        ],
    )?;
    let methods = crate::modules::http_protocol::HTTP_METHODS
        .iter()
        .map(|method| context.string_rooted(method))
        .collect::<Vec<_>>();
    let methods = context.array_rooted(&methods)?;
    set(context, module, "METHODS", methods)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .response_factory = Some(response_factory);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .incoming_factory = Some(incoming_factory);
    Ok(module)
}

pub(crate) fn create_server(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let packed = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("shared HTTP server arguments are missing"))?;
    let server = array_item(context, packed, 0)?;
    let listener = array_item(context, packed, 1)?;
    if context.is_callable_rooted(listener)? {
        let on_key = context.string_rooted("on");
        let on = context.get_property_rooted(server, on_key)?;
        let event = context.string_rooted("request");
        context.call_rooted(on, server, &[event, listener])?;
    }
    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
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
        let function = if name == "close" {
            let factory = context
                .evaluate_script_rooted(SERVER_CLOSE_FACTORY, "node:http/server-close.js")?;
            let undefined = context.undefined();
            let wrapped = context.call_rooted(factory, undefined, &[function])?;
            context.release_root(factory);
            wrapped
        } else {
            function
        };
        set(context, server, name, function)?;
    }
    let listening = context.boolean(false);
    set(context, server, "listening", listening)?;
    let retained = context.retain(server)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
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
    let first = args.first().copied();
    let first_is_callback = match first {
        Some(root) => context.is_callable_rooted(root)?,
        None => false,
    };
    let address = listen_address(context, first, first_is_callback)?;
    let host = context.host_mut().shared_state();
    {
        let mut guard = host.borrow_mut();
        let Some(server) = guard.http.servers.get(&id) else {
            return Err(RootedError::host("shared HTTP server is closed"));
        };
        if server.listener.is_some() {
            return Err(RootedError::host("shared HTTP server is already listening"));
        }
        let listener = net_shared_vm::listen(&mut guard.tcp, address).map_err(RootedError::host)?;
        let server = guard.http.servers.get_mut(&id).unwrap();
        server.listener = Some(listener);
        server.listening_pending = true;
    }
    let callback = if first_is_callback {
        first
    } else {
        args.get(1).copied()
    };
    if let Some(callback) = callback {
        if context.is_callable_rooted(callback)? {
            let on_key = context.string_rooted("on");
            let on = context.get_property_rooted(receiver, on_key)?;
            let event = context.string_rooted("listening");
            context.call_rooted(on, receiver, &[event, callback])?;
        }
    }
    let listening = context.boolean(true);
    set(context, receiver, "listening", listening)?;
    Ok(receiver)
}

fn listen_address(
    context: &mut Context<'_>,
    first: Option<RootId>,
    first_is_callback: bool,
) -> Result<SocketAddr, RootedError> {
    let Some(root) = first.filter(|_| !first_is_callback) else {
        return resolve_listen_address(DEFAULT_LISTEN_HOST, 0);
    };
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("shared HTTP listen argument is unavailable"))?;
    if let Some(port) = value.as_number() {
        return resolve_listen_address(DEFAULT_LISTEN_HOST, numeric_port(port)?);
    }
    if let Some(port) = context.string_text(root)? {
        return resolve_listen_address(DEFAULT_LISTEN_HOST, string_port(&port)?);
    }
    if value.is_null() || value.as_bool().is_some() {
        return Err(invalid_listen_port());
    }
    let key = context.string_rooted("port");
    let port = context.get_property_rooted(root, key)?;
    let port = listen_port(context, port)?;
    let host_key = context.string_rooted("host");
    let host = context.get_property_rooted(root, host_key)?;
    let host = match context.rooted_value(host) {
        Some(value) if value.is_undefined() || value.is_null() => DEFAULT_LISTEN_HOST.to_owned(),
        _ => context.string_text(host)?.ok_or_else(invalid_listen_port)?,
    };
    resolve_listen_address(&host, port)
}

fn listen_port(context: &mut Context<'_>, root: RootId) -> Result<u16, RootedError> {
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("shared HTTP listen port is unavailable"))?;
    if value.is_undefined() {
        return Ok(0);
    }
    if let Some(port) = value.as_number() {
        return numeric_port(port);
    }
    if let Some(port) = context.string_text(root)? {
        return string_port(&port);
    }
    Err(invalid_listen_port())
}

fn resolve_listen_address(host: &str, port: u16) -> Result<SocketAddr, RootedError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| RootedError::host(format!("cannot resolve HTTP listen host: {error}")))?
        .next()
        .ok_or_else(|| RootedError::host("HTTP listen host resolved to no addresses"))
}

fn numeric_port(port: f64) -> Result<u16, RootedError> {
    if port.is_finite() && port.fract() == 0.0 && (0.0..=u16::MAX as f64).contains(&port) {
        Ok(port as u16)
    } else {
        Err(invalid_listen_port())
    }
}

fn string_port(port: &str) -> Result<u16, RootedError> {
    port.parse().map_err(|_| invalid_listen_port())
}

fn invalid_listen_port() -> RootedError {
    RootedError::host("shared HTTP listen requires a numeric port or { port, host } options")
}

pub(crate) fn server_address(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let host = context.host_mut().shared_state();
    let address = {
        let guard = host.borrow();
        guard
            .http
            .servers
            .get(&id)
            .and_then(|server| server.listener)
            .and_then(|listener| net_shared_vm::address(&guard.tcp, listener))
    };
    let Some(address) = address else {
        return Ok(context.null());
    };
    let object = context.object_rooted()?;
    let address_value = context.string_rooted(&address.ip().to_string());
    set(context, object, "address", address_value)?;
    let family = context.string_rooted(match address.ip() {
        IpAddr::V4(_) => "IPv4",
        IpAddr::V6(_) => "IPv6",
    });
    set(context, object, "family", family)?;
    let port = context.number(address.port() as f64);
    set(context, object, "port", port)?;
    Ok(object)
}

pub(crate) fn server_close(
    context: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let host = context.host_mut().shared_state();
    {
        let mut guard = host.borrow_mut();
        if let Some(server) = guard.http.servers.get_mut(&id) {
            server.closing = true;
            if let Some(listener) = server.listener.take() {
                net_shared_vm::close_listener(&mut guard.tcp, listener);
            }
        }
    }
    let listening = context.boolean(false);
    set(context, receiver, "listening", listening)?;
    Ok(receiver)
}

pub(crate) fn agent_create(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
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
    let aborted_clients = {
        let host = context.host_mut().shared_state();
        let mut guard = host.borrow_mut();
        let Some(sockets) = guard.http.agents.remove(&agent) else {
            return Ok(context.undefined());
        };
        let mut aborted_clients = Vec::new();
        for socket in sockets {
            let response_is_complete = guard
                .http
                .clients
                .get(&socket)
                .is_some_and(|client| client.response_parser.is_complete());
            if response_is_complete {
                // The poller has parsed the full response and may currently be
                // emitting its head or body. Let that terminal transition finish;
                // it will close the now-unowned socket after guest callbacks return.
                continue;
            }
            net_shared_vm::close_socket(&mut guard.tcp, socket);
            if let Some(client) = guard.http.clients.remove(&socket) {
                aborted_clients.push(client);
            }
            guard
                .http
                .responses
                .retain(|_, response| response.socket != socket);
        }
        aborted_clients
    };
    let mut first_error = None;
    for client in aborted_clients {
        if let Err(error) = super::client::abort_exchange(context, client) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(context.undefined())
}

pub(crate) fn response_set_header(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let value_root = argument(context, args.get(2).copied());
    let name = header_name(context, name_root)?;
    let multiple = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let values = header_values(context, value_root, multiple, &name)?;
    let lifecycle = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle);
    if lifecycle.is_some_and(|state| state != super::state::ResponseLifecycle::Open) {
        return throw_response_error(
            context,
            false,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot set headers after they are sent to the client",
        );
    }
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    let response = host
        .http
        .responses
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    response
        .headers
        .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
    let values = limit_header_values(&name, values);
    response
        .headers
        .extend(values.into_iter().map(|value| (name.clone(), value)));
    Ok(receiver)
}

pub(crate) fn response_remove_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let name = context.to_string(name_root)?;
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    if let Some(response) = host.http.responses.get_mut(&id) {
        if response.lifecycle != super::state::ResponseLifecycle::Open {
            drop(host);
            return throw_response_error(
                context,
                false,
                "ERR_HTTP_HEADERS_SENT",
                "Cannot remove headers after they are sent to the client",
            );
        }
        response
            .headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        if name.eq_ignore_ascii_case("date") {
            response.send_date = false;
        }
    }
    Ok(context.undefined())
}

pub(crate) fn response_get_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let name = context.to_string(name_root)?;
    let values = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| {
            response
                .headers
                .iter()
                .filter(|(header, _)| header.eq_ignore_ascii_case(&name))
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match values.len() {
        0 => Ok(context.undefined()),
        1 => Ok(context.string_rooted(&values[0])),
        _ => {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)
        }
    }
}

pub(crate) fn response_write(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let chunk_root = argument(context, args.get(1).copied());
    let chunk = context.to_string(chunk_root)?.into_bytes();
    let explicit = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !explicit {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        if response.lifecycle.is_terminal() {
            drop(host);
            return throw_response_error(
                context,
                false,
                "ERR_STREAM_WRITE_AFTER_END",
                "write after end",
            );
        }
        response.body.push(chunk, false);
        return Ok(context.boolean(true));
    }
    let status = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .and_then(crate::modules::http_protocol::valid_status_number)
        .unwrap_or(200);
    let status_message = args
        .get(4)
        .copied()
        .map(|root| context.string_text(root))
        .transpose()?
        .flatten()
        .unwrap_or_else(|| {
            crate::modules::http_protocol::default_status_message(status).to_owned()
        });
    let state = context.host_mut().shared_state();
    let (socket, bytes) = {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        if response.lifecycle.is_terminal() {
            drop(host);
            return throw_response_error(
                context,
                false,
                "ERR_STREAM_WRITE_AFTER_END",
                "write after end",
            );
        }
        let first_write = response.lifecycle == super::state::ResponseLifecycle::Open;
        let bytes = if first_write {
            enable_streaming_framing(&mut response.headers);
            response.lifecycle.send_headers();
            crate::modules::http_protocol::compose(
                status,
                &status_message,
                &response.headers,
                &chunk,
                &[],
                true,
                false,
                response.send_date,
            )
        } else if is_chunked(&response.headers) {
            crate::modules::http_protocol::chunk_frame(&chunk)
        } else {
            chunk
        };
        (response.socket, bytes)
    };
    net_shared_vm::write(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    Ok(context.boolean(true))
}

pub(crate) fn response_write_head(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let lifecycle = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle);
    if lifecycle.is_some_and(|state| state != super::state::ResponseLifecycle::Open) {
        return throw_response_error(
            context,
            false,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot write headers after they are sent to the client",
        );
    }
    let status_root = argument(context, args.get(1).copied());
    let status_number = context
        .rooted_value(status_root)
        .and_then(Value::as_number)
        .unwrap_or(f64::NAN);
    let Some(status) = crate::modules::http_protocol::valid_status_number(status_number) else {
        let value = context.to_string(status_root)?;
        return throw_range_error(
            context,
            "ERR_HTTP_INVALID_STATUS_CODE",
            &format!("Invalid status code: {value}"),
        );
    };
    let invalid_headers = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if invalid_headers {
        return throw_response_error(
            context,
            true,
            "ERR_INVALID_ARG_VALUE",
            "The argument 'headers' is invalid",
        );
    }
    let message_root = argument(context, args.get(2).copied());
    let status_message = context.string_text(message_root)?.unwrap_or_else(|| {
        crate::modules::http_protocol::default_status_message(status).to_owned()
    });
    let entry_root = argument(context, args.get(3).copied());
    let entries = write_head_entries(context, entry_root)?;

    let state = context.host_mut().shared_state();
    let (socket, bytes) = {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        for (name, values) in entries {
            response
                .headers
                .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
            let values = limit_header_values(&name, values);
            response
                .headers
                .extend(values.into_iter().map(|value| (name.clone(), value)));
        }
        enable_streaming_framing(&mut response.headers);
        response.lifecycle.send_headers();
        let bytes = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &response.headers,
            &[],
            &[],
            true,
            false,
            response.send_date,
        );
        (response.socket, bytes)
    };
    net_shared_vm::write(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    Ok(context.string_rooted(&status_message))
}

pub(crate) fn response_finish(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let status = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .and_then(crate::modules::http_protocol::valid_status_number)
        .unwrap_or(200);
    let status_message = args
        .get(2)
        .copied()
        .map(|root| context.string_text(root))
        .transpose()?
        .flatten();
    let response = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        let headers_sent = response.lifecycle == super::state::ResponseLifecycle::HeadersSent;
        if !response.lifecycle.end() {
            None
        } else {
            Some((
                response.socket,
                response.async_id,
                status,
                status_message.clone().unwrap_or_else(|| {
                    crate::modules::http_protocol::default_status_message(status).to_owned()
                }),
                response.headers.clone(),
                headers_sent,
                response.body.bytes(),
                is_chunked(&response.headers),
                response.send_date,
            ))
        }
    };
    let Some((
        socket,
        async_id,
        status,
        status_message,
        headers,
        headers_sent,
        body,
        chunked,
        send_date,
    )) = response
    else {
        return Ok(context.undefined());
    };
    let bytes = if headers_sent {
        if chunked {
            let mut framed = crate::modules::http_protocol::chunk_frame(&body);
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&[]));
            framed
        } else {
            body
        }
    } else {
        let chunked = is_chunked(&headers);
        let mut framed = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &headers,
            &body,
            &[],
            true,
            false,
            send_date,
        );
        if chunked {
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&[]));
        }
        framed
    };
    if bytes.is_empty() {
        return Ok(context.undefined());
    }
    net_shared_vm::write(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    if let (Some(request), Some(response), Some(server), Some(socket)) = (
        args.get(3).copied(),
        args.get(4).copied(),
        args.get(5).copied(),
        args.get(6).copied(),
    ) {
        let message = context.object_rooted()?;
        set(context, message, "request", request)?;
        set(context, message, "response", response)?;
        set(context, message, "server", server)?;
        set(context, message, "socket", socket)?;
        let shared_state = context.host_mut().shared_state();
        let result = {
            let _scope =
                crate::modules::async_hooks_shared_vm::enter_context(&shared_state, async_id);
            crate::modules::diagnostics_channel_shared_vm::publish_named(
                context,
                "http.server.response.finish",
                message,
            )
        };
        for root in
            crate::modules::async_hooks_shared_vm::take_context_stores(&shared_state, async_id)
        {
            context.release_root(root);
        }
        result?;
    }
    Ok(context.undefined())
}

pub(crate) fn response_destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let shared_state = context.host_mut().shared_state();
    let mut host = shared_state.borrow_mut();
    let (socket, async_id) = {
        let Some(response) = host.http.responses.remove(&id) else {
            return Ok(context.undefined());
        };
        let mut response = response;
        if !response.lifecycle.destroy() {
            host.http.responses.insert(id, response);
            return Ok(context.undefined());
        }
        (response.socket, response.async_id)
    };
    let server_id = host
        .http
        .connections
        .remove(&socket)
        .map(|connection| connection.server);
    if let Some(server_id) = server_id {
        if let Some(server) = host.http.servers.get_mut(&server_id) {
            server.connections.remove(&socket);
        }
    }
    net_shared_vm::close_socket(&mut host.tcp, socket);
    drop(host);
    for root in crate::modules::async_hooks_shared_vm::take_context_stores(&shared_state, async_id)
    {
        context.release_root(root);
    }
    Ok(context.undefined())
}

fn enable_streaming_framing(headers: &mut Vec<(String, String)>) {
    let has_length = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let has_transfer_encoding = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"));
    if !has_length && !has_transfer_encoding {
        headers.push(("Transfer-Encoding".to_owned(), "chunked".to_owned()));
    }
}

fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    })
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

fn argument_id(context: &mut Context<'_>, root: Option<RootId>) -> Result<u64, RootedError> {
    root.and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP response identifier"))
}

fn argument(context: &mut Context<'_>, root: Option<RootId>) -> RootId {
    root.unwrap_or_else(|| context.undefined())
}

fn header_name(context: &mut Context<'_>, root: RootId) -> Result<String, RootedError> {
    let name = context.string_text(root)?;
    let Some(name) = name else {
        let display = context.to_string(root)?;
        return throw_header_issue(
            context,
            crate::modules::http_protocol::HeaderIssue::InvalidName(display),
        );
    };
    if name.is_empty()
        || !name
            .chars()
            .all(crate::modules::http_protocol::is_http_token_char)
    {
        return throw_header_issue(
            context,
            crate::modules::http_protocol::HeaderIssue::InvalidName(name),
        );
    }
    Ok(name)
}

fn header_values(
    context: &mut Context<'_>,
    root: RootId,
    multiple: bool,
    name: &str,
) -> Result<Vec<String>, RootedError> {
    let values = if multiple {
        let length_key = context.string_rooted("length");
        let length = context.get_property_rooted(root, length_key)?;
        let length = context
            .rooted_value(length)
            .and_then(Value::as_number)
            .filter(|length| length.is_finite() && *length >= 0.0)
            .unwrap_or(0.0) as usize;
        let mut values = Vec::with_capacity(length);
        for index in 0..length {
            let item = array_item(context, root, index)?;
            values.push(header_value(context, item, name)?);
        }
        values
    } else {
        vec![header_value(context, root, name)?]
    };
    Ok(values)
}

fn header_value(
    context: &mut Context<'_>,
    root: RootId,
    name: &str,
) -> Result<String, RootedError> {
    if context.rooted_value(root).is_some_and(Value::is_undefined) {
        let issue = crate::modules::http_protocol::HeaderIssue::MissingValue(name.to_owned());
        return throw_header_issue(context, issue);
    }
    let value = context.to_string(root)?;
    if !crate::modules::http_protocol::valid_header_value(&value) {
        let issue = crate::modules::http_protocol::HeaderIssue::InvalidContent(name.to_owned());
        return throw_header_issue(context, issue);
    }
    Ok(value)
}

fn write_head_entries(
    context: &mut Context<'_>,
    root: RootId,
) -> Result<Vec<(String, Vec<String>)>, RootedError> {
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(root, length_key)?;
    let length = context
        .rooted_value(length)
        .and_then(Value::as_number)
        .filter(|length| length.is_finite() && *length >= 0.0)
        .unwrap_or(0.0) as usize;
    if !length.is_multiple_of(3) {
        return Err(RootedError::host(
            "shared HTTP header entries are malformed",
        ));
    }
    let mut result = Vec::with_capacity(length / 3);
    for index in (0..length).step_by(3) {
        let name_root = array_item(context, root, index)?;
        let name = header_name(context, name_root)?;
        let values_root = array_item(context, root, index + 1)?;
        let multiple_root = array_item(context, root, index + 2)?;
        let multiple = context
            .rooted_value(multiple_root)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let values = header_values(context, values_root, multiple, &name)?;
        result.push((name, values));
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

fn limit_header_values(name: &str, mut values: Vec<String>) -> Vec<String> {
    if crate::modules::http_protocol::NON_REPEATABLE_HEADERS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        values.truncate(1);
    }
    values
}

fn coded_error_rooted(
    context: &mut Context<'_>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = if type_error {
        context.type_error_rooted(message)?
    } else {
        context.error_rooted(message)?
    };
    let code_value = context.string_rooted(code);
    set(context, error, "code", code_value)?;
    Ok(error)
}

fn throw_header_issue(
    context: &mut Context<'_>,
    issue: crate::modules::http_protocol::HeaderIssue,
) -> Result<String, RootedError> {
    let error = coded_error_rooted(context, true, issue.code(), &issue.message())?;
    Err(context.throw(error))
}

fn throw_response_error(
    context: &mut Context<'_>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = coded_error_rooted(context, type_error, code, message)?;
    Err(context.throw(error))
}

fn throw_range_error(
    context: &mut Context<'_>,
    code: &str,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = context.range_error_rooted(message)?;
    let code_value = context.string_rooted(code);
    set(context, error, "code", code_value)?;
    Err(context.throw(error))
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
