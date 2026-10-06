use crate::host::NodeHost;
use crate::modules::net;
use rqj::{NativeContext, RootId, RootedError, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

type Context<'a> = NativeContext<'a, NodeHost>;

const MODULE_FACTORY: &str = r#"((createServer, get, createAgent, destroyAgent) => {
  class Agent {
    constructor(options) {
      this._quenchSharedAgentId = createAgent(options);
    }
    destroy() {
      return destroyAgent(this._quenchSharedAgentId);
    }
  }
  return { createServer, get, Agent };
})"#;

const RESPONSE_FACTORY: &str = r#"((setHeader, end) => (id, statusCode) => {
  const response = { statusCode };
  response.setHeader = (name, value) => { setHeader(id, name, value); return response; };
  response.end = (chunk) => { end(id, response.statusCode, chunk); return response; };
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
    let factory = context.evaluate_script_rooted(MODULE_FACTORY, "node:http/shared-api.js")?;
    let undefined = context.undefined();
    let module = context.call_rooted(
        factory,
        undefined,
        &[create_server, get, create_agent, destroy_agent],
    )?;
    let methods = crate::modules::http::HTTP_METHODS
        .iter()
        .map(|method| context.string_rooted(method))
        .collect::<Vec<_>>();
    let methods = context.array_rooted(&methods)?;
    set(context, module, "METHODS", methods)?;
    let set_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseSetHeader"))?;
    let end = context.host_function(crate::host::shared_vm::operation("httpResponseEnd"))?;
    let factory =
        context.evaluate_script_rooted(RESPONSE_FACTORY, "node:http/response-factory.js")?;
    let undefined = context.undefined();
    let factory = context.call_rooted(factory, undefined, &[set_header, end])?;
    let factory = context.retain(factory)?;
    context
        .host_mut()
        .state()
        .borrow_mut()
        .http
        .shared
        .response_factory = Some(factory);
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
    let port = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
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
    _: &[RootId],
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
    let host_header = if port == 80 {
        request_host.clone()
    } else {
        format!("{request_host}:{port}")
    };
    let request =
        crate::modules::http_client::request_head(&host_header, "GET", &path, &[], 0, false);
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
                request: request.into_bytes(),
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
    let name = context.to_string(name_root)?;
    let value = context.to_string(value_root)?;
    if name.is_empty()
        || !name
            .chars()
            .all(crate::modules::http_res::is_http_token_char)
    {
        let error = context.type_error_rooted("Invalid HTTP header name")?;
        return Err(context.throw(error));
    }
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
            Some((response.socket, response.headers.clone()))
        }
    };
    let Some((socket, headers)) = response else {
        return Ok(context.undefined());
    };
    let bytes =
        crate::modules::http_res::compose(status, "OK", &headers, &body, &[], true, false, true);
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
