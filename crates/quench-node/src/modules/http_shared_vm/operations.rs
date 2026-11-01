use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError, Value};
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

type Context<'a> = NativeContext<'a, NodeHost>;

const DEFAULT_LISTEN_HOST: &str = "127.0.0.1";

enum HttpListenAddress {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Local(String),
}



/// Node's `ServerResponse` as an `OutgoingMessage`: a legacy `Stream` whose writes go to
/// the native exchange `id` names. Methods live on the prototype and read the exchange from
/// the instance; `end` writes and finishes synchronously, then `finish` and `close` follow on
/// ticks, as Node's socket write completion and `emitCloseNT` deliver them.
/// Node's `getDefaultHighWaterMark()` for byte streams outside Windows
/// (`lib/internal/streams/state.js`); a socket write returns false once its
/// writable length reaches it.
pub(super) const RESPONSE_HIGH_WATER_MARK: usize = 64 * 1024;

/// A ServerResponse's `statusCode` until the application sets one (Node's `ServerResponse`).
pub(super) const DEFAULT_STATUS: u16 = 200;

/// The own data properties the EventEmitter construction reads that a program may write
/// (`objectTemplateCapture`'s probes), as flat (holder, key) pairs: `init`'s reads of the
/// EventEmitter prototype's `_events`, `_maxListeners` and capture flag, `EventEmitter.init`
/// itself, and `Function.prototype.call`, which each `X.call(this)` of a constructor chain
/// reads. Prototype layout (accessors, added keys) a template guards itself.
fn emitter_template_probes(context: &mut Context<'_>, emitter: RootId) -> Result<Vec<RootId>, RootedError> {
    let event_data = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("event listener configuration missing"))?;
    let capture_key = crate::modules::event_constructor_shared_vm::get(context, event_data, "captureKey")?;
    let emitter_prototype = get(context, emitter, "prototype")?;
    let global = context.global_root()?;
    let function = get(context, global, "Function")?;
    let function_prototype = get(context, function, "prototype")?;
    let name = |context: &mut Context<'_>, text: &str| context.string_rooted(text);
    Ok(vec![
        emitter_prototype,
        name(context, "_events"),
        emitter_prototype,
        name(context, "_maxListeners"),
        emitter_prototype,
        capture_key,
        emitter,
        name(context, "init"),
        function_prototype,
        name(context, "call"),
    ])
}

/// The probes of `new IncomingMessage(undefined)` (the pinned library's `IncomingMessage`,
/// `Readable`, `ReadableState` and `Stream`): the EventEmitter's
/// (`emitter_template_probes`), the `prototype` of each constructor `new` reads, and the
/// `push` and `listenerCount` an ended message's push calls. The default high water mark
/// is module state the factory compares.
fn incoming_template_probes(
    context: &mut Context<'_>,
    emitter: RootId,
    incoming: RootId,
) -> Result<RootId, RootedError> {
    let readable_state = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "internal/streams/readable.js",
        "ReadableState",
    )?;
    let readable = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "internal/streams/readable.js",
        "prototype",
    )?;
    let emitter_prototype = get(context, emitter, "prototype")?;
    let mut probes = emitter_template_probes(context, emitter)?;
    for constructor in [incoming, readable_state] {
        let key = context.string_rooted("prototype");
        probes.extend([constructor, key]);
    }
    // The methods an ended message's `push(null)` calls, and the header lines a server
    // request's copy adds after its fields (`incoming::CREATE_FACTORY`).
    let push = context.string_rooted("push");
    let listener_count = context.string_rooted("listenerCount");
    let incoming_prototype = get(context, incoming, "prototype")?;
    let add_header_lines = context.string_rooted("_addHeaderLines");
    probes.extend([readable, push, emitter_prototype, listener_count, incoming_prototype, add_header_lines]);
    context.array_rooted(&probes)
}



pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let status_codes = crate::modules::http_status_shared_vm::codes(context)?;
    let set_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseSetHeader"))?;
    let get_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseGetHeader"))?;
    let get_headers =
        context.host_function(crate::host::shared_vm::operation("httpResponseGetHeaders"))?;
    let remove_header = context.host_function(crate::host::shared_vm::operation(
        "httpResponseRemoveHeader",
    ))?;
    let write = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseWrite"),
        status_codes,
    )?;
    let finish = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseFinish"),
        status_codes,
    )?;
    let write_head = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseWriteHead"),
        status_codes,
    )?;
    let destroy_response =
        context.host_function(crate::host::shared_vm::operation("httpResponseDestroy"))?;
    // Core owns the weak response-to-transport identity. Keeping the method on
    // the prototype preserves ordinary method identity and receiver dispatch.
    let global = context.global_root()?;
    let key = context.string_rooted("WeakMap");
    let weak_map = context.get_property_rooted(global, key)?;
    let responses = context.construct_rooted(weak_map, weak_map, &[])?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(weak_map, key)?;
    let key = context.string_rooted("set");
    let map_set = context.get_property_rooted(prototype, key)?;
    let tracking = super::state::ResponseTracking {
        responses: context.retain(responses)?,
        map_set: context.retain(map_set)?,
    };
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .response_tracking = Some(tracking);
    let initialize_response =
        context.host_function(crate::host::shared_vm::operation("httpResponseInitialize"))?;
    let send = context.host_constructor_with_data(
        crate::host::shared_vm::operation("httpResponseSend"),
        responses,
    )?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("_send");
    context.define_data_property_rooted(send, key, name, false, false, true)?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let legacy_stream = get(context, stream, "Stream")?;
    let process = get(context, global, "process")?;
    let next_tick = get(context, process, "nextTick")?;
    let codes = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "internal/errors.js",
        "internal/errors.js",
        "codes",
    )?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    let emitter_probes = emitter_template_probes(context, emitter)?;
    let emitter_probes = context.array_rooted(&emitter_probes)?;
    let capture = context.host_function(crate::host::shared_vm::operation("objectTemplateCapture"))?;
    let copy = context.host_function(crate::host::shared_vm::operation("objectTemplateCopy"))?;
    let capture_limit = context.number(f64::from(super::incoming::TEMPLATE_CAPTURE_LIMIT));
    let response_factory =
        crate::host::removed_javascript("node:http/response-factory.js")?;
    let undefined = context.undefined();
    let response_api = context.call_rooted(
        response_factory,
        undefined,
        &[
            legacy_stream,
            set_header,
            get_header,
            get_headers,
            remove_header,
            write,
            finish,
            write_head,
            destroy_response,
            initialize_response,
            next_tick,
            codes,
            capture,
            copy,
            emitter_probes,
            capture_limit,
        ],
    )?;
    let response_factory = get(context, response_api, "0")?;
    let respond = get(context, response_api, "1")?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(response_factory, key)?;
    set(context, prototype, "_send", send)?;
    let incoming_constructor = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_incoming.js",
        "IncomingMessage",
    )?;
    // One `_read` and `_destroy` serve every message: each finds its exchange under the
    // host-private key its side records (`incoming::create`).
    let server_exchange = context.private_symbol_rooted(Some("node:http:serverExchange"));
    let client_exchange = context.private_symbol_rooted(Some("node:http:clientExchange"));
    let exchange_keys = context.array_rooted(&[server_exchange, client_exchange])?;
    let incoming_read = context.host_function_named_with_data(
        crate::host::shared_vm::operation("httpIncomingRead"),
        "_read",
        exchange_keys,
    )?;
    let incoming_destroy = context.host_function_named_with_data(
        crate::host::shared_vm::operation("httpIncomingDestroy"),
        "_destroy",
        exchange_keys,
    )?;
    let template_probes = incoming_template_probes(context, emitter, incoming_constructor)?;
    let incoming_factory = super::incoming::create_function(
        context,
        [
            incoming_constructor,
            emitter,
            incoming_read,
            incoming_destroy,
            server_exchange,
            client_exchange,
        ],
        template_probes,
    )?;
    let request_dispatch =
        crate::host::removed_javascript("node:http/request-dispatch.js")?;
    let request_dispatch =
        context.call_rooted(request_dispatch, undefined, &[incoming_factory, respond])?;
    let request_dispatch = context.retain(request_dispatch)?;
    let incoming_factory = context.retain(incoming_factory)?;

    let initialize_server =
        context.host_function(crate::host::shared_vm::operation("httpCreateServer"))?;
    let request = super::client::request_function(context)?;
    let create_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentCreate"))?;
    let destroy_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentDestroy"))?;
    // `Agent.prototype.createConnection` is `net.createConnection`.
    let net = crate::host::shared_vm::commonjs::net_module(context)?;
    let connect = get(context, net, "connect")?;
    let writable = get(context, stream, "Writable")?;
    let outgoing_message = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_outgoing.js",
        "OutgoingMessage",
    )?;
    let k_high_water_mark = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_outgoing.js",
        "kHighWaterMark",
    )?;
    // Node's helper constructs ConnectionsList from internalBinding('http_parser')
    // and starts a JS timeout scan for Node's own HTTPParser. Quench's shared
    // HTTP poll owns accepted sockets and timeout handling, while close(),
    // closeAllConnections(), and closeIdleConnections() below are host-backed.
    // Keep the listening hook expected by Server, but don't call the Node
    // helper: its internal ConnectionsList constructor is not part of Quench's
    // public/shared-VM HTTP parser surface.
    let connections_checking_interval = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_server.js",
        "kConnectionsCheckingInterval",
    )?;
    let connection_listener = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_server.js",
        "_connectionListener",
    )?;
    // The actual Node helper scans its own HTTPParser connection list. Quench
    // owns connection/request timeout scanning in the shared HTTP poll, so we
    // only need the interval's public lifecycle here. Node exposes this timer
    // through kConnectionsCheckingInterval, and Server.close() destroys it.
    let setup_connections_tracking_factory = crate::host::removed_javascript("node:http/setup-connections-tracking.js")?;
    let undefined = context.undefined();
    let setup_connections_tracking = context.call_rooted(
        setup_connections_tracking_factory,
        undefined,
        &[connections_checking_interval],
    )?;
    context.release_root(setup_connections_tracking_factory);
    let factory = crate::host::removed_javascript("node:http/shared-api.js")?;
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            initialize_server,
            emitter,
            request,
            create_agent,
            destroy_agent,
            connect,
            incoming_constructor,
            response_factory,
            k_high_water_mark,
            setup_connections_tracking,
            connections_checking_interval,
            connection_listener,
        ],
    )?;
    let request_key = context.string_rooted("request");
    let request_constructor = context.get_property_rooted(module, request_key)?;
    set(context, module, "ClientRequest", request_constructor)?;
    let implicit_header = crate::host::removed_javascript("node:http/implicit-header.js")?;
    for constructor in [request_constructor, response_factory] {
        let key = context.string_rooted("prototype");
        let prototype = context.get_property_rooted(constructor, key)?;
        set(context, prototype, "_implicitHeader", implicit_header)?;
    }
    let decorate_outgoing = crate::host::removed_javascript("node:http/outgoing-message-header-api.js")?;
    let undefined = context.undefined();
    let outgoing_message =
        context.call_rooted(decorate_outgoing, undefined, &[outgoing_message])?;
    set(context, module, "OutgoingMessage", outgoing_message)?;
    // ClientRequest is an OutgoingMessage in Node. The shared HTTP request
    // implementation is backed by Writable, so retain the Writable methods
    // on its private prototype while linking it into Node's public prototype
    // chain for `instanceof http.OutgoingMessage` and diagnostics payloads.
    let request_prototype = get(context, request_constructor, "prototype")?;
    let writable_prototype = get(context, writable, "prototype")?;
    let outgoing_prototype = get(context, outgoing_message, "prototype")?;
    let link_client_request = crate::host::removed_javascript("node:http/client-request-prototype.js")?;
    context.call_rooted(
        link_client_request,
        undefined,
        &[request_prototype, writable_prototype, outgoing_prototype],
    )?;
    let response = get(context, module, "ServerResponse")?;
    let response_prototype = get(context, response, "prototype")?;
    let outgoing_prototype = get(context, outgoing_message, "prototype")?;
    context.set_prototype_rooted(response_prototype, outgoing_prototype)?;
    set(context, module, "STATUS_CODES", status_codes)?;
    let net_server = get(context, net, "Server")?;
    let server = get(context, module, "Server")?;
    let net_prototype = get(context, net_server, "prototype")?;
    let server_prototype = get(context, server, "prototype")?;
    context.set_prototype_rooted(server, net_server)?;
    context.set_prototype_rooted(server_prototype, net_prototype)?;
    let key = context.string_rooted("Agent");
    let agent = context.get_property_rooted(module, key)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(agent, key)?;
    let native = context.host_function(crate::host::shared_vm::operation("getName"))?;
    let wrapper = crate::host::removed_javascript("node:http/agent-get-name.js")?;
    let get_name = context.call_rooted(wrapper, undefined, &[native])?;
    set(context, prototype, "getName", get_name)?;
    for (name, public_operation, original_operation) in [
        (
            "validateHeaderName",
            "httpValidateHeaderNamePublic",
            "httpValidateHeaderNameOriginal",
        ),
        (
            "validateHeaderValue",
            "httpValidateHeaderValuePublic",
            "httpValidateHeaderValueOriginal",
        ),
    ] {
        let public =
            context.host_constructor(crate::host::shared_vm::operation(public_operation))?;
        let original =
            context.host_function(crate::host::shared_vm::operation(original_operation))?;
        for (function, name) in [(public, name), (original, "")] {
            let key = context.string_rooted("name");
            let name = context.string_rooted(name);
            context.define_data_property_rooted(function, key, name, false, false, true)?;
        }
        set(context, public, "withoutStackTrace", original)?;
        set(context, module, name, public)?;
    }
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
        .request_dispatch = Some(request_dispatch);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .incoming_factory = Some(incoming_factory);
    // Match Node's lazy getters in lib/http.js. Undici imports `node:http`
    // during initialization, so loading it while this builtin is being
    // constructed would create a partial module cycle. Defer the load until
    // one of these properties is actually read.
    let initialize_fetch = context.host_function(crate::host::shared_vm::operation(
        "webFetchInitialize",
    ))?;
    let global = context.global_root()?;
    let install_websocket_getters = crate::host::removed_javascript("node:http/undici-lazy-getters.js")?;
    let undefined = context.undefined();
    context.call_rooted(
        install_websocket_getters,
        undefined,
        &[module, initialize_fetch, global],
    )?;
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
        ("closeAllConnections", "httpServerClose"),
        ("closeIdleConnections", "httpServerClose"),
    ] {
        let function = context
            .host_function_with_data(crate::host::shared_vm::operation(operation), id_value)?;
        let function = if name == "closeAllConnections" || name == "closeIdleConnections" {
            let mode = context.string_rooted(if name == "closeAllConnections" {
                "all"
            } else {
                "idle"
            });
            let factory = crate::host::removed_javascript("node:http/server-close-connections.js")?;
            let undefined = context.undefined();
            let wrapped = context.call_rooted(factory, undefined, &[function, mode])?;
            context.release_root(factory);
            wrapped
        } else if name == "close" {
            let factory = crate::host::removed_javascript("node:http/server-close.js")?;
            let undefined = context.undefined();
            let wrapped = context.call_rooted(factory, undefined, &[function])?;
            context.release_root(factory);
            wrapped
        } else {
            function
        };
        set(context, server, name, function)?;
    }
    let unref = context.boolean(false);
    set(context, server, "_unref", unref)?;
    let handle = context.null();
    set(context, server, "_handle", handle)?;
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

/// Adopt an HTTP transport as a public Node net.Socket when the HTTP parser
/// encounters a protocol upgrade. The existing socket id stays attached to the
/// TCP transport; registering the socket transfers read delivery to net.Socket.
pub(crate) fn upgrade_socket(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("HTTP upgrade socket id is missing"))?;
    let id_number = context
        .rooted_value(id)
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| RootedError::host("HTTP upgrade socket id is invalid"))?
        as u64;
    let undefined = context.undefined();
    let handle =
        super::super::net_reference_shared_vm::create_socket_handle(context, undefined, &[id])?;
    let options = context.object_rooted()?;
    set(context, options, "handle", handle)?;
    let yes = context.boolean(true);
    set(context, options, "readable", yes)?;
    set(context, options, "writable", yes)?;
    let constructor = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_socket_constructor
        .ok_or_else(|| RootedError::host("net.Socket constructor is unavailable"))?;
    let socket = context.construct_rooted(constructor, constructor, &[options])?;
    let id_value = context.number(id_number as f64);
    set(context, socket, "__quenchNetSocketId", id_value)?;
    super::super::net_shared_vm::register_socket(context, socket, id_number)?;
    Ok(socket)
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
    let address = listen_address(context, first, first_is_callback, args.get(1).copied())?;
    let options =
        first.filter(|root| context.is_object_rooted(*root).unwrap_or(false) && !first_is_callback);
    let backlog = if options.is_some() {
        net_shared_vm::listen_backlog(context, options)?
    } else if matches!(&address, HttpListenAddress::Local(_))
        && args.get(1).copied().is_some_and(|value| {
            !context.is_callable_rooted(value).unwrap_or(false)
                && context.truthy_rooted(value).unwrap_or(false)
        })
    {
        net_shared_vm::convert_listen_backlog(context, args[1])?
    } else {
        net_shared_vm::listen_backlog(context, None)?
    };
    let host = context.host_mut().shared_state();
    {
        let guard = host.borrow();
        let Some(server) = guard.http.servers.get(&id) else {
            return Err(RootedError::host("shared HTTP server is closed"));
        };
        if server.listener.is_some() {
            return Err(RootedError::host("shared HTTP server is already listening"));
        }
    }
    let listener = match address {
        HttpListenAddress::Tcp(address) => {
            let result =
                net_shared_vm::listen_with_backlog(&mut host.borrow_mut().tcp, address, backlog);
            match result {
                Ok(listener) => listener,
                Err(error) => {
                    return queue_http_listen_error(context, receiver, error, address);
                }
            }
        }
        #[cfg(unix)]
        HttpListenAddress::Local(path) => {
            net_shared_vm::listen_local_endpoint(context, &path, backlog)
                .map_err(RootedError::host)?
        }
    };
    {
        let mut guard = host.borrow_mut();
        let server = guard.http.servers.get_mut(&id).unwrap();
        server.listener = Some(listener);
        server.listening_pending = true;
    }
    let listener = host.borrow().http.servers[&id].listener.unwrap();
    crate::modules::net_reference_shared_vm::install(context, listener, receiver)?;
    let callback = if first_is_callback {
        first
    } else if let Some(second) = args.get(1).copied() {
        if context.is_callable_rooted(second)? {
            Some(second)
        } else {
            args.get(2).copied()
        }
    } else {
        args.get(2).copied()
    };
    if let Some(callback) = callback {
        if context.is_callable_rooted(callback)? {
            let on_key = context.string_rooted("on");
            let on = context.get_property_rooted(receiver, on_key)?;
            let event = context.string_rooted("listening");
            context.call_rooted(on, receiver, &[event, callback])?;
        }
    }
    Ok(receiver)
}

fn queue_http_listen_error(
    context: &mut Context<'_>,
    server: RootId,
    error: io::Error,
    address: SocketAddr,
) -> Result<RootId, RootedError> {
    let errno = error.raw_os_error().map(|value| -f64::from(value));
    let code = errno
        .map(crate::modules::util_system_error_shared_vm::system_error_name)
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    let reason = error.to_string();
    let reason = reason
        .split(" (os error")
        .next()
        .unwrap_or(&reason)
        .to_lowercase();
    let error = context.error_rooted(&format!(
        "listen {code}: {reason} {}:{}",
        address.ip(),
        address.port()
    ))?;
    for (name, value) in [("code", code.as_str()), ("syscall", "listen")] {
        let key = context.string_rooted(name);
        let value = context.string_rooted(value);
        context.define_data_property_rooted(error, key, value, true, true, true)?;
    }
    let key = context.string_rooted("address");
    let value = context.string_rooted(&address.ip().to_string());
    context.define_data_property_rooted(error, key, value, true, true, true)?;
    let key = context.string_rooted("port");
    let value = context.number(f64::from(address.port()));
    context.define_data_property_rooted(error, key, value, true, true, true)?;
    if let Some(errno) = errno {
        let key = context.string_rooted("errno");
        let value = context.number(errno);
        context.define_data_property_rooted(error, key, value, true, true, true)?;
    }
    let callback = crate::host::removed_javascript("node:http/server-listen-error.js")?;
    let undefined = context.undefined();
    crate::modules::shared_event_loop::queue_next_tick(
        context,
        callback,
        undefined,
        &[server, error],
    )?;
    context.release_root(callback);
    Ok(server)
}

fn listen_address(
    context: &mut Context<'_>,
    first: Option<RootId>,
    first_is_callback: bool,
    second: Option<RootId>,
) -> Result<HttpListenAddress, RootedError> {
    let Some(root) = first.filter(|_| !first_is_callback) else {
        return resolve_listen_address(DEFAULT_LISTEN_HOST, 0).map(HttpListenAddress::Tcp);
    };
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("shared HTTP listen argument is unavailable"))?;
    if let Some(port) = value.as_number() {
        return resolve_listen_address(&listen_host(context, second)?, numeric_port(port)?)
            .map(HttpListenAddress::Tcp);
    }
    if let Some(path) = context.string_text(root)? {
        #[cfg(unix)]
        return Ok(HttpListenAddress::Local(path));
        #[cfg(not(unix))]
        return Err(invalid_listen_port());
    }
    if value.is_null() || value.as_bool().is_some() {
        return Err(invalid_listen_port());
    }
    let path_key = context.string_rooted("path");
    let path = context.get_property_rooted(root, path_key)?;
    if let Some(path) = context.string_text(path)? {
        #[cfg(unix)]
        return Ok(HttpListenAddress::Local(path));
        #[cfg(not(unix))]
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
    resolve_listen_address(&host, port).map(HttpListenAddress::Tcp)
}

fn listen_host(context: &mut Context<'_>, value: Option<RootId>) -> Result<String, RootedError> {
    let Some(value) = value else {
        return Ok(DEFAULT_LISTEN_HOST.to_owned());
    };
    if context.is_callable_rooted(value)? {
        return Ok(DEFAULT_LISTEN_HOST.to_owned());
    }
    match context.rooted_value(value) {
        Some(value) if value.is_undefined() || value.is_null() => {
            Ok(DEFAULT_LISTEN_HOST.to_owned())
        }
        Some(value) if value.as_bool().is_some() || value.as_number().is_some() => {
            Err(RootedError::host("invalid HTTP listen host"))
        }
        _ => context
            .string_text(value)?
            .ok_or_else(|| RootedError::host("invalid HTTP listen host")),
    }
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
    let (local_path, address) = {
        let guard = host.borrow();
        let listener = guard
            .http
            .servers
            .get(&id)
            .and_then(|server| server.listener);
        (
            listener.and_then(|listener| {
                net_shared_vm::listener_path(&guard.tcp, listener).map(str::to_owned)
            }),
            listener.and_then(|listener| net_shared_vm::address(&guard.tcp, listener)),
        )
    };
    if let Some(path) = local_path {
        return Ok(context.string_rooted(&path));
    }
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
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let connection_mode = match args
        .first()
        .and_then(|root| context.string_text(*root).ok().flatten())
    {
        Some(mode) if mode == "all" || mode == "idle" => Some(mode),
        _ => None,
    };
    let host = context.host_mut().shared_state();
    if let Some(mode) = connection_mode {
        let mut guard = host.borrow_mut();
        let sockets = guard
            .http
            .servers
            .get(&id)
            .map(|server| {
                server
                    .connections
                    .iter()
                    .copied()
                    .filter(|socket| {
                        mode == "all"
                            || guard
                                .http
                                .connections
                                .get(socket)
                                .is_some_and(|connection| {
                                    connection.received.is_empty()
                                        && (!connection.request_dispatched
                                            || guard
                                                .http
                                                .responses
                                                .values()
                                                .filter(|response| response.socket == *socket)
                                                .all(|response| response.lifecycle.is_terminal()))
                                })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for socket in sockets {
            net_shared_vm::close_socket(&mut guard.tcp, socket);
        }
        return Ok(context.undefined());
    }
    {
        let mut guard = host.borrow_mut();
        let mut idle_sockets = Vec::new();
        let listener = guard.http.servers.get_mut(&id).and_then(|server| {
            server.closing = true;
            server.listening_pending = false;
            idle_sockets.extend(server.connections.iter().copied());
            server.listener.take()
        });
        if let Some(listener) = listener {
            net_shared_vm::close_listener(&mut guard.tcp, listener);
        }
        // An idle connection: not upgraded, no request bytes pending, and every
        // response it carried is terminal.
        idle_sockets.retain(|socket| {
            guard.http.connections.get(socket).is_some_and(|connection| {
                !connection.upgraded
                    && connection.received.is_empty()
                    && (!connection.request_dispatched
                        || guard
                            .http
                            .responses
                            .values()
                            .filter(|response| response.socket == *socket)
                            .all(|response| response.lifecycle.is_terminal()))
            })
        });
        for socket in idle_sockets {
            // A response can be terminal while its bytes are still queued in
            // the shared transport. Drain those bytes before half-closing;
            // otherwise an already-idle connection can be closed at once.
            if net_shared_vm::socket_write_queue_size(&guard.tcp, socket) == 0 {
                net_shared_vm::close_socket(&mut guard.tcp, socket);
            } else {
                net_shared_vm::end(&mut guard.tcp, socket, false).map_err(RootedError::host)?;
            }
        }
    }
    let handle = context.null();
    set(context, receiver, "_handle", handle)?;
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

/// Node's pooling key, preserving property read order and guest coercions.
pub(crate) fn agent_get_name(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = match args.first().copied() {
        Some(value) if !context.rooted_value(value).is_some_and(Value::is_undefined) => value,
        _ => context.null_object_rooted()?,
    };
    let get = |context: &mut Context<'_>, name: &str| {
        let key = context.string_rooted(name);
        context.get_property_rooted(options, key)
    };
    let host = get(context, "host")?;
    let mut name = if context.truthy_rooted(host)? {
        host
    } else {
        context.string_rooted("localhost")
    };
    for property in ["port", "localAddress"] {
        let colon = context.string_rooted(":");
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, colon)?;
        let value = get(context, property)?;
        if context.truthy_rooted(value)? {
            let value = get(context, property)?;
            name = context.arithmetic_rooted(NativeArithmetic::Add, name, value)?;
        }
    }
    let family = get(context, "family")?;
    let ipv4 = context.rooted_value(family).and_then(Value::as_number) == Some(4.0);
    let ipv6 = if ipv4 {
        false
    } else {
        let family = get(context, "family")?;
        context.rooted_value(family).and_then(Value::as_number) == Some(6.0)
    };
    if ipv4 || ipv6 {
        let family = get(context, "family")?;
        let text = context.to_string(family)?;
        let suffix = context.string_rooted(&format!(":{text}"));
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, suffix)?;
    }
    let socket = get(context, "socketPath")?;
    if context.truthy_rooted(socket)? {
        let socket = get(context, "socketPath")?;
        let text = context.to_string(socket)?;
        let suffix = context.string_rooted(&format!(":{text}"));
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, suffix)?;
    }
    Ok(name)
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
        // Node's `Agent.prototype.destroy` destroys every socket the agent owns,
        // in an exchange or free. Requests the agent queued stay queued: closing
        // each socket starts the next one on a fresh connection.
        let origins = guard
            .http
            .origins
            .keys()
            .filter(|origin| origin.agent == Some(agent))
            .cloned()
            .collect::<Vec<_>>();
        let mut aborted_clients = Vec::new();
        for origin in origins {
            let sockets = guard
                .http
                .origins
                .get_mut(&origin)
                .map(|pool| std::mem::take(&mut pool.sockets))
                .unwrap_or_default();
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
            guard.http.origins.remove(&origin);
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
    let relaxed = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let values = header_values(context, value_root, multiple, &name, relaxed)?;
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
    response.removed_headers.remove(&name.to_ascii_lowercase());
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
        response.removed_headers.insert(name.to_ascii_lowercase());
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

pub(crate) fn response_get_headers(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let headers = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.headers.clone())
        .unwrap_or_default();
    let object = context.null_object_rooted()?;
    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    for (header_name, header_value) in headers {
        let name = header_name.to_ascii_lowercase();
        if let Some((_, values)) = grouped
            .iter_mut()
            .find(|(existing_name, _)| existing_name == &name)
        {
            values.push(header_value);
        } else {
            grouped.push((name, vec![header_value]));
        }
    }
    for (name, values) in grouped {
        let value = if values.len() == 1 && name != "set-cookie" {
            context.string_rooted(&values[0])
        } else {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)?
        };
        set(context, object, &name, value)?;
    }
    Ok(object)
}

pub(crate) fn response_initialize(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument(context, args.first().copied());
    let response = argument(context, args.get(1).copied());
    let requested_id = argument_id(context, Some(id))?;
    let owner = context.host_weak_root(response)?;
    let (numeric_id, replaced) = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let numeric_id = if requested_id == 0 && !host.http.responses.contains_key(&requested_id) {
            let id = host.http.response_id().map_err(RootedError::host)?;
            host.http.responses.insert(
                id,
                super::state::Response {
                    socket: 0,
                    virtual_response: true,
                    async_id: 0,
                    headers: Vec::new(),
                    body: Default::default(),
                    send_date: true,
                    http10: false,
                    close_after_response: false,
                    removed_headers: Default::default(),
                    use_chunked_encoding_by_default: true,
                    lifecycle: super::state::ResponseLifecycle::Open,
                    owner: None,
                },
            );
            id
        } else {
            requested_id
        };
        let replaced = if let Some(record) = host.http.responses.get_mut(&numeric_id) {
            record.owner.replace(owner)
        } else {
            Some(owner)
        };
        (numeric_id, replaced)
    };
    if let Some(stale) = replaced {
        context.release_host_weak_root(stale);
    }
    let tracking = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .response_tracking
        .ok_or_else(|| RootedError::host("HTTP response tracking is unavailable"))?;
    let exchange_id = context.number(numeric_id as f64);
    context.call_rooted(
        tracking.map_set,
        tracking.responses,
        &[response, exchange_id],
    )?;
    Ok(exchange_id)
}

fn response_chunk_bytes(
    context: &mut Context<'_>,
    chunk: RootId,
    encoding: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let bytes = match context.view_bytes_rooted(chunk) {
        Some(bytes) => bytes,
        None => {
            let encoding = match encoding {
                Some(value) if context.truthy_rooted(value)? => context.to_string(value)?,
                _ => "utf8".to_owned(),
            };
            let Some(encoding_name) = crate::modules::buffer_codec::canonical_encoding(&encoding)
            else {
                return throw_response_error(
                    context,
                    true,
                    "ERR_UNKNOWN_ENCODING",
                    &format!("Unknown encoding: {encoding}"),
                );
            };
            let units = context.to_string_code_units(chunk)?;
            crate::modules::buffer_codec::encode_units(&units, encoding_name)
        }
    };
    Ok(bytes)
}

pub(crate) fn response_send(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let responses = context.host_function_data()?;
    let id = context.weak_map_get_rooted(responses, receiver)?;
    let id = argument_id(context, Some(id))?;
    let chunk = argument(context, args.first().copied());
    let bytes = response_chunk_bytes(context, chunk, args.get(1).copied())?;
    let shared = context.host_mut().shared_state();
    let socket = shared
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.socket);
    let Some(socket) = socket else {
        return Ok(context.boolean(false));
    };
    if !bytes.is_empty() {
        net_shared_vm::write(&mut shared.borrow_mut().tcp, socket, &bytes)
            .map_err(RootedError::host)?;
    }
    if let Some(callback) = args.get(2).copied() {
        if context.is_callable_rooted(callback)? {
            crate::modules::process_shared_vm::next_tick(context, receiver, &[callback])?;
        }
    }
    Ok(context.boolean(true))
}

// Public sendDate is consulted when headers are serialized, not after a response
// has already committed them. Header removal can independently suppress it.
fn response_send_date(
    context: &mut Context<'_>,
    id: u64,
    response: Option<RootId>,
) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    let open = shared
        .borrow()
        .http
        .responses
        .get(&id)
        .is_some_and(|response| response.lifecycle == super::state::ResponseLifecycle::Open);
    if open {
        if let Some(response) = response {
            let key = context.string_rooted("sendDate");
            let value = context.get_property_rooted(response, key)?;
            let enabled = context.truthy_rooted(value)?;
            if !enabled {
                if let Some(response) = shared.borrow_mut().http.responses.get_mut(&id) {
                    response.send_date = false;
                }
            }
        }
    }
    Ok(())
}

fn response_status(
    context: &mut Context<'_>,
    id: u64,
    response: Option<RootId>,
    status: Option<RootId>,
    reason: Option<RootId>,
) -> Result<(u16, Vec<u8>), RootedError> {
    let open = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .is_some_and(|response| response.lifecycle == super::state::ResponseLifecycle::Open);
    if !open {
        return Ok((DEFAULT_STATUS, Vec::new()));
    }
    let response = argument(context, response);
    let status = argument(context, status);
    let reason = argument(context, reason);
    let status = crate::modules::http_status_shared_vm::prepare(
        context,
        response,
        status,
        crate::modules::http_status_shared_vm::Reason::Existing(reason),
    )?;
    let reason = crate::modules::http_status_shared_vm::encode(context, response, status)?;
    Ok((status, reason))
}

pub(crate) fn response_write(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    if id == 0 {
        return Ok(context.boolean(true));
    }
    let chunk_root = argument(context, args.get(1).copied());
    let explicit = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let state = context.host_mut().shared_state();
    let lifecycle = state
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    // A view streamed after the head is framed straight into the socket's queue; other
    // chunks are encoded first, before the lifecycle check, as Node validates them.
    let streamed_view = explicit
        && lifecycle != super::state::ResponseLifecycle::Open
        && context.with_view_bytes_rooted(chunk_root, |_| ()).is_some();
    let chunk = if streamed_view {
        Vec::new()
    } else {
        response_chunk_bytes(context, chunk_root, args.get(6).copied())?
    };
    if lifecycle.is_terminal() {
        return throw_response_error(
            context,
            false,
            "ERR_STREAM_WRITE_AFTER_END",
            "write after end",
        );
    }
    if !explicit {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .expect("response checked active above");
        response.body.push(chunk, false);
        return Ok(context.boolean(true));
    }
    let (status, status_message) = response_status(
        context,
        id,
        args.get(5).copied(),
        args.get(3).copied(),
        args.get(4).copied(),
    )?;
    response_send_date(context, id, args.get(5).copied())?;
    let mut host = state.borrow_mut();
    let response = host
        .http
        .responses
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    // A response that removed both framing headers sends its body unframed, as Node's
    // `_storeHeader` does once `removeHeader` has dropped them.
    let omit_framing = response.removed_headers.contains("content-length")
        && response.removed_headers.contains("transfer-encoding");
    if response.virtual_response {
        if response.lifecycle == super::state::ResponseLifecycle::Open {
            if !omit_framing {
                enable_streaming_framing(
                    &mut response.headers,
                    response.use_chunked_encoding_by_default,
                );
            }
            if invalid_trailer(&response.headers) {
                drop(host);
                return throw_invalid_trailer(context);
            }
            response.lifecycle.send_headers();
        }
        drop(host);
        return Ok(context.boolean(true));
    }
    let socket = response.socket;
    if response.lifecycle == super::state::ResponseLifecycle::Open {
        if !omit_framing {
            enable_streaming_framing(
                &mut response.headers,
                response.use_chunked_encoding_by_default,
            );
        }
        if invalid_trailer(&response.headers) {
            drop(host);
            return throw_invalid_trailer(context);
        }
        response.lifecycle.send_headers();
        let head = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &response.headers,
            &chunk,
            &[],
            !response.http10 && !response.close_after_response,
            response.removed_headers.contains("connection"),
            omit_framing,
            response.http10,
            response.send_date,
            std::time::SystemTime::now(),
        );
        net_shared_vm::write(&mut host.tcp, socket, &head).map_err(RootedError::host)?;
    } else {
        let chunked = is_chunked(&response.headers);
        let mut write = |body: &[u8]| {
            use crate::modules::http_protocol::{CHUNK_FRAME_END, ChunkFrameHead};
            if chunked && !body.is_empty() {
                let head = ChunkFrameHead::new(body.len());
                net_shared_vm::write_parts(&mut host.tcp, socket, [head.bytes(), body, CHUNK_FRAME_END])
            } else if chunked {
                Ok(())
            } else {
                net_shared_vm::write_parts(&mut host.tcp, socket, [body])
            }
        };
        let written = if streamed_view {
            context
                .with_view_bytes_rooted(chunk_root, &mut write)
                .unwrap_or_else(|| write(&[]))
        } else {
            write(&chunk)
        };
        written.map_err(RootedError::host)?;
    }
    net_shared_vm::try_flush(&mut host.tcp, socket);
    let below_mark = net_shared_vm::queued_bytes(&host.tcp, socket)
        .is_none_or(|queued| queued < RESPONSE_HIGH_WATER_MARK);
    if !below_mark && !host.http.drain_waiters.contains_key(&socket) {
        drop(host);
        let response = argument(context, args.get(5).copied());
        let waiter = context.retain(response)?;
        state.borrow_mut().http.drain_waiters.insert(socket, waiter);
    }
    Ok(context.boolean(below_mark))
}

pub(crate) fn response_write_head(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    // `writeHead`'s headers object itself (`HEAD_SOURCE_ARGUMENT`) is read here only where the
    // guest loop's reads (`Object.keys`, each value, `headerValue`'s `String`) observe
    // nothing: data properties holding strings, numbers or `undefined`. Anything else
    // answers `false` before any effect, and the guest builds the entries.
    let source = args.get(HEAD_SOURCE_ARGUMENT).and_then(|&root| context.rooted_value(root)).and_then(Value::as_bool) == Some(true);
    let plain = if source {
        let source = argument(context, args.get(3).copied());
        match plain_head_entries(context, source) {
            Some(entries) => Some(entries),
            None => return Ok(context.boolean(false)),
        }
    } else {
        None
    };
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
    let message_root = argument(context, args.get(2).copied());
    let response_root = argument(context, args.get(5).copied());
    let status = crate::modules::http_status_shared_vm::prepare(
        context,
        response_root,
        status_root,
        crate::modules::http_status_shared_vm::Reason::WriteHead(message_root),
    )?;
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
    // A server's `httpValidation: 'relaxed' | 'insecure'` admits header values Node's strict
    // check rejects; the response carries its server's mode.
    let relaxed = if let Some(response) = args.get(5).copied() {
        let key = context.string_rooted("_httpValidation");
        let mode = context.get_property_rooted(response, key)?;
        matches!(
            context.to_string(mode).ok().as_deref(),
            Some("relaxed" | "insecure")
        )
    } else {
        false
    };
    let entries = match plain {
        Some(entries) => {
            let mut result = Vec::with_capacity(entries.len());
            for (name, value) in entries {
                let name = valid_header_name(context, name)?;
                let values = header_values(context, value, false, &name, relaxed)?;
                result.push((name, values));
            }
            result
        }
        None => {
            let entry_root = argument(context, args.get(3).copied());
            write_head_entries(context, entry_root, relaxed)?
        }
    };
    let status_message =
        crate::modules::http_status_shared_vm::encode(context, response_root, status)?;

    response_send_date(context, id, args.get(5).copied())?;
    let state = context.host_mut().shared_state();
    let (socket, bytes, virtual_response) = {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        let mut replaced = Vec::<String>::new();
        for (name, values) in entries {
            response.removed_headers.remove(&name.to_ascii_lowercase());
            if !replaced
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&name))
            {
                response
                    .headers
                    .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
                replaced.push(name.clone());
            }
            let values = limit_header_values(&name, values);
            response
                .headers
                .extend(values.into_iter().map(|value| (name.clone(), value)));
        }
        let omit_framing = response.removed_headers.contains("content-length")
            && response.removed_headers.contains("transfer-encoding");
        if !omit_framing {
            enable_streaming_framing(
                &mut response.headers,
                response.use_chunked_encoding_by_default,
            );
        }
        normalize_no_body_response_headers(status, &mut response.headers);
        if invalid_trailer(&response.headers) {
            drop(host);
            return throw_invalid_trailer(context);
        }
        response.lifecycle.send_headers();
        let virtual_response = response.virtual_response;
        let bytes = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &response.headers,
            &[],
            &[],
            !response.http10 && !response.close_after_response,
            response.removed_headers.contains("connection"),
            omit_framing,
            response.http10,
            response.send_date,
            std::time::SystemTime::now(),
        );
        (response.socket, bytes, virtual_response)
    };
    if virtual_response {
        let header = String::from_utf8_lossy(&bytes);
        return Ok(context.string_rooted(&header));
    }
    net_shared_vm::write(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    Ok(context.undefined())
}

pub(crate) fn response_finish(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let trailers = if let Some(response_root) = args.get(4).copied() {
        let key = context.string_rooted("__quenchTrailerEntries");
        let trailer_entries = context.get_property_rooted(response_root, key)?;
        context.release_root(key);
        // `addTrailers` records the entries; a response that added none has no list.
        let trailers = if context.is_object_rooted(trailer_entries)? {
            super::client::read_headers(context, Some(trailer_entries), false)
        } else {
            Ok(Vec::new())
        };
        context.release_root(trailer_entries);
        trailers?
    } else {
        Vec::new()
    };
    let (status, status_message) = response_status(
        context,
        id,
        args.get(4).copied(),
        args.get(1).copied(),
        args.get(2).copied(),
    )?;
    response_send_date(context, id, args.get(4).copied())?;
    let virtual_response = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        match host.http.responses.get_mut(&id) {
            Some(response) if response.virtual_response => {
                response.lifecycle.end();
                true
            }
            _ => false,
        }
    };
    if virtual_response {
        return Ok(context.undefined());
    }
    let response = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let Some(response) = host.http.responses.get_mut(&id) else {
            // A peer close may retire the host-side response between an
            // end() call and a later idempotent end() from Node middleware.
            drop(host);
            return Ok(context.undefined());
        };
        let headers_sent = response.lifecycle == super::state::ResponseLifecycle::HeadersSent;
        if !headers_sent
            && response
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("trailer"))
        {
            enable_streaming_framing(
                &mut response.headers,
                response.use_chunked_encoding_by_default,
            );
            if invalid_trailer(&response.headers) {
                drop(host);
                return throw_invalid_trailer(context);
            }
        }
        if !response.lifecycle.end() {
            None
        } else {
            Some((
                response.socket,
                response.async_id,
                status,
                status_message.clone(),
                response.headers.clone(),
                headers_sent,
                // An ended response never writes again: its body leaves with this frame.
                std::mem::take(&mut response.body).bytes(),
                is_chunked(&response.headers),
                response.send_date,
                response.http10,
                response.close_after_response,
                response.removed_headers.clone(),
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
        http10,
        request_closes,
        removed_headers,
    )) = response
    else {
        return Ok(context.undefined());
    };
    let (server_closing, peer_ended, server_root) = {
        let host = context.host_mut().shared_state();
        let host = host.borrow();
        host.http
            .connections
            .get(&socket)
            .and_then(|connection| {
                host.http.servers.get(&connection.server).map(|server| {
                    (server.closing, connection.peer_ended, Some(server.root))
                })
            })
            .unwrap_or((false, false, None))
    };
    let allow_half_open = if let Some(server_root) = server_root {
        let property = context.string_rooted("httpAllowHalfOpen");
        let value = context.get_property_rooted(server_root, property)?;
        context.release_root(property);
        let allow = context.rooted_value(value).and_then(Value::as_bool).unwrap_or(false);
        context.release_root(value);
        allow
    } else {
        false
    };
    let close_after_response = request_closes
        || server_closing
        || (peer_ended && !allow_half_open)
        || (removed_headers.contains("content-length")
            && removed_headers.contains("transfer-encoding")
            && !headers.iter().any(|(name, _)| {
                name.eq_ignore_ascii_case("content-length")
                    || name.eq_ignore_ascii_case("transfer-encoding")
            }))
        || (http10
            && !chunked
            && !headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-length")))
        || headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("connection") && value.eq_ignore_ascii_case("close")
        });
    let bytes = if headers_sent {
        if chunked {
            let mut framed = crate::modules::http_protocol::chunk_frame(&body);
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&trailers));
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
            !http10 && !close_after_response,
            removed_headers.contains("connection"),
            removed_headers.contains("content-length")
                && removed_headers.contains("transfer-encoding")
                && !headers.iter().any(|(name, _)| {
                    name.eq_ignore_ascii_case("content-length")
                        || name.eq_ignore_ascii_case("transfer-encoding")
                }),
            http10,
            send_date,
            std::time::SystemTime::now(),
        );
        if chunked {
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&trailers));
        }
        framed
    };
    if !bytes.is_empty() {
        net_shared_vm::write(
            &mut context.host_mut().shared_state().borrow_mut().tcp,
            socket,
            &bytes,
        )
        .map_err(RootedError::host)?;
    }
    if close_after_response {
        net_shared_vm::end(
            &mut context.host_mut().shared_state().borrow_mut().tcp,
            socket,
            false,
        )
        .map_err(RootedError::host)?;
    } else {
        let shared_state = context.host_mut().shared_state();
        let mut host = shared_state.borrow_mut();
        // A pipelined connection keeps the bytes past this request for the next one.
        let buffered = if let Some(connection) = host.http.connections.get_mut(&socket) {
            let consumed = connection.request_bytes.min(connection.received.len());
            connection.received.drain(..consumed);
            connection.request_bytes = 0;
            connection.request_dispatched = false;
            connection.headers_timeout_sent = false;
            connection.request_timeout_sent = false;
            let buffered = !connection.received.is_empty();
            if buffered {
                connection.headers_started = Some(std::time::Instant::now());
                connection.request_started = Some(std::time::Instant::now());
            } else {
                connection.clear_received();
                connection.headers_started = None;
                connection.request_started = None;
            }
            buffered
        } else {
            false
        };
        if buffered {
            net_shared_vm::queue_http_data(&mut host.tcp, socket);
        }
    }
    if let (Some(request), Some(response), Some(server), Some(socket)) = (
        args.get(3).copied(),
        args.get(4).copied(),
        args.get(5).copied(),
        args.get(6).copied(),
    ) {
        let shared_state = context.host_mut().shared_state();
        // The finish message is made only for a channel with subscribers, as Node's server
        // publishes it only then (`hasSubscribers`).
        let subscribed = crate::modules::diagnostics_channel_shared_vm::channel_has_subscribers(
            &shared_state.borrow(),
            RESPONSE_FINISH_CHANNEL,
        );
        let result = if subscribed {
            let message = context.object_rooted()?;
            set(context, message, "request", request)?;
            set(context, message, "response", response)?;
            set(context, message, "server", server)?;
            set(context, message, "socket", socket)?;
            let _scope =
                crate::modules::async_hooks_shared_vm::enter_context(&shared_state, async_id);
            crate::modules::diagnostics_channel_shared_vm::publish_named(
                context,
                RESPONSE_FINISH_CHANNEL,
                message,
            )
        } else {
            Ok(())
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

/// The diagnostics channel a server's response finish publishes to.
const RESPONSE_FINISH_CHANNEL: &str = "http.server.response.finish";

/// The finalization callback of a collected response object: a finished response's host
/// entry goes with it. An unfinished one stays until its socket closes.
pub(crate) fn response_destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let shared_state = context.host_mut().shared_state();
    let mut host = shared_state.borrow_mut();
    let (socket, async_id, owner, virtual_response) = {
        let Some(response) = host.http.responses.remove(&id) else {
            return Ok(context.undefined());
        };
        let mut response = response;
        if !response.lifecycle.destroy() {
            host.http.responses.insert(id, response);
            return Ok(context.undefined());
        }
        (
            response.socket,
            response.async_id,
            response.owner,
            response.virtual_response,
        )
    };
    if virtual_response {
        drop(host);
        if let Some(owner) = owner {
            context.release_host_weak_root(owner);
        }
        return Ok(context.undefined());
    }
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
    if let Some(owner) = owner {
        context.release_host_weak_root(owner);
    }
    for root in crate::modules::async_hooks_shared_vm::retire_context(&shared_state, async_id) {
        context.release_root(root);
    }
    Ok(context.undefined())
}

fn enable_streaming_framing(
    headers: &mut Vec<(String, String)>,
    use_chunked_encoding_by_default: bool,
) {
    let has_length = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let has_transfer_encoding = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"));
    if use_chunked_encoding_by_default && !has_length && !has_transfer_encoding {
        headers.push(("Transfer-Encoding".to_owned(), "chunked".to_owned()));
    }
}

fn normalize_no_body_response_headers(status: u16, headers: &mut Vec<(String, String)>) {
    if !matches!(status, 204 | 304) {
        return;
    }

    // Node rejects transfer framing for bodyless responses. If the caller
    // explicitly requested chunked framing, close the connection after the
    // response rather than leaving the peer waiting for a chunk terminator.
    let had_transfer_encoding = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"));
    if !had_transfer_encoding {
        return;
    }
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("transfer-encoding"));
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("connection"));
    headers.push(("Connection".to_owned(), "close".to_owned()));
}

fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    })
}

fn invalid_trailer(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("trailer"))
        && !is_chunked(headers)
}

fn throw_invalid_trailer<T>(context: &mut Context<'_>) -> Result<T, RootedError> {
    throw_response_error(
        context,
        false,
        "ERR_HTTP_TRAILER_INVALID",
        "Trailers are invalid with this transfer encoding",
    )
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
    valid_header_name(context, name)
}

/// `name` as a header name, or the invalid-token error `writeHead` and `setHeader` throw.
fn valid_header_name(context: &mut Context<'_>, name: String) -> Result<String, RootedError> {
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
    relaxed: bool,
) -> Result<Vec<String>, RootedError> {
    let values = if multiple {
        let items = context.array_like_values_rooted(root)?;
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(header_value(context, item, name, relaxed)?);
        }
        values
    } else {
        vec![header_value(context, root, name, relaxed)?]
    };
    Ok(values)
}

fn header_value(
    context: &mut Context<'_>,
    root: RootId,
    name: &str,
    relaxed: bool,
) -> Result<String, RootedError> {
    if context.rooted_value(root).is_some_and(Value::is_undefined) {
        let issue = crate::modules::http_protocol::HeaderIssue::MissingValue(name.to_owned());
        return throw_header_issue(context, issue);
    }
    let value = context.to_string(root)?;
    let valid = if relaxed {
        value.chars().all(|character| {
            let code = character as u32;
            code <= 0xff && !matches!(code, 0x00 | 0x0a | 0x0d)
        })
    } else {
        crate::modules::http_protocol::valid_header_value(&value)
    };
    if !valid {
        let issue = crate::modules::http_protocol::HeaderIssue::InvalidContent(name.to_owned());
        return throw_header_issue(context, issue);
    }
    Ok(value)
}

/// The `httpResponseWriteHead` argument that marks its entries argument as `writeHead`'s
/// headers object rather than the guest's packed entries.
const HEAD_SOURCE_ARGUMENT: usize = 6;

/// A headers object's names and values where `writeHead`'s guest loop would observe
/// nothing reading them: data properties (`plain_enumerable_entries_rooted`) whose values
/// are strings, numbers or `undefined`, which `headerValue` passes to `String` or as is.
fn plain_head_entries(context: &mut Context<'_>, source: RootId) -> Option<Vec<(String, RootId)>> {
    let entries = context.plain_enumerable_entries_rooted(source)?;
    entries
        .iter()
        .all(|(_, value)| {
            use quench_runtime::ValueType;
            matches!(context.value_type_rooted(*value), Some(ValueType::String | ValueType::Number | ValueType::Undefined))
        })
        .then_some(entries)
}

fn write_head_entries(
    context: &mut Context<'_>,
    root: RootId,
    relaxed: bool,
) -> Result<Vec<(String, Vec<String>)>, RootedError> {
    let items = context.array_like_values_rooted(root)?;
    let (entries, remainder) = items.as_chunks::<HEAD_ENTRY_FIELDS>();
    if !remainder.is_empty() {
        return Err(RootedError::host(
            "shared HTTP header entries are malformed",
        ));
    }
    let mut result = Vec::with_capacity(entries.len());
    for &[name_root, values_root, multiple_root] in entries {
        let name = header_name(context, name_root)?;
        let multiple = context
            .rooted_value(multiple_root)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let values = header_values(context, values_root, multiple, &name, relaxed)?;
        result.push((name, values));
    }
    Ok(result)
}

/// A packed head entry's values: its name, its value or values, and whether those are a list.
const HEAD_ENTRY_FIELDS: usize = 3;

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

fn throw_response_error<T>(
    context: &mut Context<'_>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<T, RootedError> {
    let error = coded_error_rooted(context, type_error, code, message)?;
    Err(context.throw(error))
}

fn set(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    if context.set_named_property_rooted(object, name, value)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install shared http property {name}"
        )))
    }
}

pub(crate) fn validate_header_name(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = argument(context, args.first().copied());
    let text = context.string_text(name)?;
    if text.as_ref().is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(crate::modules::http_protocol::is_http_token_char)
    }) {
        return Ok(context.undefined());
    }
    let label = argument(context, args.get(1).copied());
    let label = if context.truthy_rooted(label)? {
        label
    } else {
        context.string_rooted("Header name")
    };
    formatted_header_error(
        context,
        "ERR_INVALID_HTTP_TOKEN",
        "%s must be a valid HTTP token [\"%s\"]",
        &[label, name],
    )
}

pub(crate) fn validate_header_value(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = argument(context, args.first().copied());
    let value = argument(context, args.get(1).copied());
    if context.rooted_value(value).is_some_and(Value::is_undefined) {
        return formatted_header_error(
            context,
            "ERR_HTTP_INVALID_HEADER_VALUE",
            "Invalid value \"%s\" for header \"%s\"",
            &[value, name],
        );
    }
    let lenient = argument(context, args.get(2).copied());
    let lenient = context.truthy_rooted(lenient)?;
    let units = context.to_string_code_units(value)?;
    if !units
        .iter()
        .all(|unit| crate::modules::http_protocol::valid_header_unit(*unit, lenient))
    {
        let mut message: Vec<u16> = "Invalid character in header content"
            .encode_utf16()
            .collect();
        if !context.rooted_value(name).is_some_and(Value::is_undefined) {
            // ERR_INVALID_CHAR uses template-string coercion rather than %s.
            message.extend(" [\"".encode_utf16());
            message.extend(context.to_string_code_units(name)?);
            message.extend("\"]".encode_utf16());
        }
        let message = context.string_units_rooted(&message);
        return throw_header_message(context, "ERR_INVALID_CHAR", message);
    }
    Ok(context.undefined())
}

fn formatted_header_error(
    context: &mut Context<'_>,
    code: &str,
    template: &str,
    arguments: &[RootId],
) -> Result<RootId, RootedError> {
    let formatters = crate::modules::util_shared_vm::internal_formatters(context)?;
    let key = context.string_rooted("format");
    let format = context.get_property_rooted(formatters, key)?;
    let template = context.string_rooted(template);
    let mut arguments_with_template = Vec::with_capacity(arguments.len() + 1);
    arguments_with_template.push(template);
    arguments_with_template.extend_from_slice(arguments);
    let receiver = context.undefined();
    let message = context.call_rooted(format, receiver, &arguments_with_template)?;
    throw_header_message(context, code, message)
}

// Node has already formatted this message. Keep that original VM string,
// including lone surrogates, as the ordinary Error's non-enumerable message.
fn throw_header_message(
    context: &mut Context<'_>,
    code: &str,
    message: RootId,
) -> Result<RootId, RootedError> {
    let error = coded_error_rooted(context, true, code, "")?;
    set(context, error, "message", message)?;
    Err(context.throw(error))
}
