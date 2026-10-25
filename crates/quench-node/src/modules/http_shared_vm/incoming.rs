//! A real library IncomingMessage uses the native HTTP exchange as its producer.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::net::SocketAddr;
type Context<'a> = NativeContext<'a, NodeHost>;
fn set(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let api = c
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("HTTP assignment configuration missing"))?;
    crate::modules::event_constructor_shared_vm::named_set(c, api, object, name, value).map_err(
        |error| {
            if error.to_string() == "shared HTTP object rejected a required property" {
                RootedError::host(format!(
                    "shared HTTP object rejected required property '{name}'"
                ))
            } else {
                error
            }
        },
    )
}
fn call(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let method = get(c, object, name)?;
    c.call_rooted(method, object, args)
}

fn define_socket_address(
    c: &mut Context<'_>,
    socket: RootId,
    prefix: &str,
    address: SocketAddr,
) -> Result<(), RootedError> {
    for (suffix, value) in [
        ("Address", c.string_rooted(&address.ip().to_string())),
        (
            "Family",
            c.string_rooted(if address.is_ipv4() { "IPv4" } else { "IPv6" }),
        ),
        ("Port", c.number(f64::from(address.port()))),
    ] {
        let key = c.string_rooted(&format!("{prefix}{suffix}"));
        c.define_data_property_rooted(socket, key, value, false, false, true)?;
    }
    Ok(())
}

fn initialize_accepted_socket(
    c: &mut Context<'_>,
    configuration: RootId,
    socket: RootId,
    transport: RootId,
) -> Result<(), RootedError> {
    let socket_constructor = get(c, configuration, "1")?;
    let prototype = get(c, socket_constructor, "prototype")?;
    c.set_prototype_rooted(socket, prototype)?;
    for (name, value) in [
        ("readable", true),
        ("writable", true),
        ("destroyed", false),
        ("connecting", false),
    ] {
        let key = c.string_rooted(name);
        let value = c.boolean(value);
        // Socket/stream prototypes expose some of these as getter-only
        // properties. A fresh facade has no initialized native stream state,
        // so install the visible initial value as an own data property.
        c.define_data_property_rooted(socket, key, value, true, true, true)?;
    }

    let install_parser = c.evaluate_script_rooted(
        r#"((socket) => {
          if (socket.parser) return;
          const parser = { free() {}, close() {} };
          socket.parser = parser;
          socket.once('close', () => {
            if (typeof parser.free === 'function') parser.free();
          });
        })"#,
        "node:http/accepted-socket-parser.js",
    )?;
    let undefined = c.undefined();
    c.call_rooted(install_parser, undefined, &[socket])?;
    c.release_root(install_parser);

    if let Some(id) = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|value| value as u64)
    {
        let endpoint = {
            let shared = c.host_mut().shared_state();
            let state = shared.borrow();
            (
                net_shared_vm::socket_address(&state.tcp, id, false).ok(),
                net_shared_vm::socket_address(&state.tcp, id, true).ok(),
            )
        };
        if let Some(address) = endpoint.0 {
            define_socket_address(c, socket, "local", address)?;
        }
        if let Some(address) = endpoint.1 {
            define_socket_address(c, socket, "remote", address)?;
        }
        let retained_socket = c.retain(socket)?;
        let shared = c.host_mut().shared_state();
        let mut host = shared.borrow_mut();
        let stored = host
            .http
            .connections
            .get_mut(&id)
            .is_some_and(|connection| {
                if connection.socket_root.is_none() {
                    connection.socket_root = Some(retained_socket);
                    true
                } else {
                    false
                }
            });
        drop(host);
        if !stored {
            c.release_root(retained_socket);
        }
    }

    set(c, socket, "__quenchHttpSocketId", transport)?;
    let write = c.host_function(crate::host::shared_vm::operation(
        "httpServerSocketWriteById",
    ))?;
    set(c, socket, "write", write)?;
    let destroy_data = c.array_rooted(&[transport])?;
    let destroy = c.host_function_with_data(
        crate::host::shared_vm::operation("httpServerSocketDestroy"),
        destroy_data,
    )?;
    set(c, socket, "destroy", destroy)?;
    let end_data = c.array_rooted(&[transport])?;
    let end = c.host_function_with_data(
        crate::host::shared_vm::operation("httpServerSocketEnd"),
        end_data,
    )?;
    set(c, socket, "end", end)?;
    let resume = c.host_function(crate::host::shared_vm::operation(
        "httpServerSocketResume",
    ))?;
    set(c, socket, "resume", resume)?;
    Ok(())
}

/// Construct the accepted socket facade before parsing a request so Node's
/// `Server` `connection` event receives the same socket used by IncomingMessage.
fn create_accepted_socket(
    c: &mut Context<'_>,
    configuration: RootId,
    transport: RootId,
) -> Result<RootId, RootedError> {
    let socket = c.object_rooted()?;
    initialize_accepted_socket(c, configuration, socket, transport)?;
    Ok(socket)
}

pub(crate) fn create(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let configuration = c.host_function_data()?;
    if args.len() == 1
        && args
            .first()
            .and_then(|value| c.rooted_value(*value))
            .and_then(|value| value.as_number())
            .is_some()
    {
        return create_accepted_socket(c, configuration, args[0]);
    }
    let missing = c.undefined();
    let server_object = args.get(5).copied().unwrap_or(missing);
    let configured_constructor = if c.is_object_rooted(server_object)? {
        get(c, server_object, "_quenchIncomingMessage")?
    } else {
        missing
    };
    let constructor = if c.is_callable_rooted(configured_constructor)? {
        configured_constructor
    } else {
        get(c, configuration, "0")?
    };
    let message = args[0];
    let id = args.get(3).copied().unwrap_or(missing);
    let server = args.get(4).copied().unwrap_or(missing);
    let socket = if c.truthy_rooted(server)? {
        // Server-side IncomingMessages are created from parsed requests, which
        // already carry the accepted Socket object.
        get(c, message, "socket")?
    } else {
        // Client responses need to share the ClientRequest's socket identity.
        // The exchange id is the request id used by the host-side client table.
        let request_id = c
            .rooted_value(id)
            .and_then(|value| value.as_number())
            .map(|number| number as u64);
        let request_root = request_id.and_then(|request_id| {
            c.host_mut()
                .shared_state()
                .borrow()
                .http
                .clients
                .values()
                .find(|client| client.request_id == request_id)
                .map(|client| client.request_root)
        });
        if let Some(request_root) = request_root {
            get(c, request_root, "socket")?
        } else {
            missing
        }
    };
    let stream = c.construct_rooted(constructor, constructor, &[socket])?;
    if c.truthy_rooted(server)? {
        let install_abort = c.evaluate_script_rooted(
            r#"((request) => {
              request._quenchAbort = (error) => {
                if (request.listenerCount('error') === 0) request.on('error', () => {});
                request.destroy(error);
              };
            })"#,
            "node:http/server-request-abort.js",
        )?;
        let undefined = c.undefined();
        c.call_rooted(install_abort, undefined, &[stream])?;
        c.release_root(install_abort);
    }
    let raw = get(c, message, "rawHeaders")?;
    let length = get(c, raw, "length")?;
    call(c, stream, "_addHeaderLines", &[raw, length])?;
    for key in c.enumerable_own_keys_rooted(message, false, true)? {
        let Some(name) = c.string_text(key)? else {
            continue;
        };
        if name == "headers" {
            continue;
        }
        let value = c.get_property_rooted(message, key)?;
        set(c, stream, &name, value)?;
    }
    let open = args.get(2).copied().unwrap_or(missing);
    if c.truthy_rooted(open)? {
        let no = c.boolean(false);
        set(c, stream, "complete", no)?;
    }
    if c.is_object_rooted(server_object)? {
        let high_water_mark = get(c, server_object, "highWaterMark")?;
        let readable_state = get(c, stream, "_readableState")?;
        let has_high_water_mark = c
            .rooted_value(high_water_mark)
            .is_some_and(|value| !value.is_undefined());
        if has_high_water_mark && c.is_object_rooted(readable_state)? {
            set(c, readable_state, "highWaterMark", high_water_mark)?;
        }
    }
    let data = c.array_rooted(&[stream, id, server])?;
    for (name, operation) in [
        ("_read", "httpIncomingRead"),
        ("_destroy", "httpIncomingDestroy"),
    ] {
        let function =
            c.host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
        let key = c.string_rooted("name");
        let text = c.string_rooted(name);
        c.define_data_property_rooted(function, key, text, false, false, true)?;
        set(c, stream, name, function)?;
    }
    let socket = get(c, stream, "socket")?;
    if c.is_object_rooted(socket)? && c.truthy_rooted(server)? {
        let on = get(c, socket, "on")?;
        if !c.is_callable_rooted(on)? {
            let socket_constructor = get(c, configuration, "1")?;
            let prototype = get(c, socket_constructor, "prototype")?;
            c.set_prototype_rooted(socket, prototype)?;
        }
        // Keep Node's accepted-socket parser lifecycle observable. Callers
        // may wrap parser.free while handling a request; release it when the
        // shared socket facade emits close.
        let install_parser = c.evaluate_script_rooted(
            r#"((socket) => {
              if (socket.parser) return;
              const parser = { free() {}, close() {} };
              socket.parser = parser;
              socket.once('close', () => {
                if (typeof parser.free === 'function') parser.free();
              });
            })"#,
            "node:http/accepted-socket-parser.js",
        )?;
        let undefined = c.undefined();
        c.call_rooted(install_parser, undefined, &[socket])?;
        c.release_root(install_parser);
        let transport = args.get(3).copied().unwrap_or(missing);
        if let Some(id) = c
            .rooted_value(transport)
            .and_then(|value| value.as_number())
            .map(|value| value as u64)
        {
            let endpoint = {
                let shared = c.host_mut().shared_state();
                let state = shared.borrow();
                (
                    net_shared_vm::socket_address(&state.tcp, id, false).ok(),
                    net_shared_vm::socket_address(&state.tcp, id, true).ok(),
                )
            };
            if let Some(address) = endpoint.0 {
                define_socket_address(c, socket, "local", address)?;
            }
            if let Some(address) = endpoint.1 {
                define_socket_address(c, socket, "remote", address)?;
            }
            let retained_socket = c.retain(socket)?;
            let shared = c.host_mut().shared_state();
            let mut host = shared.borrow_mut();
            let stored = host
                .http
                .connections
                .get_mut(&id)
                .is_some_and(|connection| {
                    if connection.socket_root.is_none() {
                        connection.socket_root = Some(retained_socket);
                        true
                    } else {
                        false
                    }
                });
            drop(host);
            if !stored {
                c.release_root(retained_socket);
            }
        }
        // The accepted server socket is a facade over the native TCP
        // transport, not a Socket constructed through net.Socket's Duplex
        // constructor. Keep its EventEmitter methods from Socket.prototype,
        // but route writes straight to the transport instead of inheriting
        // Writable.write, which requires a private _writableState.
        set(c, socket, "__quenchHttpSocketId", transport)?;
        let write = c.host_function(crate::host::shared_vm::operation(
            "httpServerSocketWriteById",
        ))?;
        set(c, socket, "write", write)?;
        let destroy_data = c.array_rooted(&[transport])?;
        let destroy = c.host_function_with_data(
            crate::host::shared_vm::operation("httpServerSocketDestroy"),
            destroy_data,
        )?;
        set(c, socket, "destroy", destroy)?;
        let end_data = c.array_rooted(&[transport])?;
        let end = c.host_function_with_data(
            crate::host::shared_vm::operation("httpServerSocketEnd"),
            end_data,
        )?;
        set(c, socket, "end", end)?;
        let resume = c.host_function(crate::host::shared_vm::operation(
            "httpServerSocketResume",
        ))?;
        set(c, socket, "resume", resume)?;
        let parser = get(c, socket, "parser")?;
        if c.is_object_rooted(parser)? {
            set(c, parser, "incoming", stream)?;
            let attach_lifecycle = c.evaluate_script_rooted(
                r#"((incoming, parser) => {
                  const clearIncoming = () => {
                    process.nextTick(() => {
                      if (parser.incoming === incoming) parser.incoming = null;
                    });
                  };
                  incoming.once('end', clearIncoming);
                  incoming.once('close', clearIncoming);
                })"#,
                "node:http/accepted-socket-parser-incoming.js",
            )?;
            let undefined = c.undefined();
            c.call_rooted(attach_lifecycle, undefined, &[stream, parser])?;
            c.release_root(attach_lifecycle);
        }
        c.release_root(parser);
    }
    let body = args.get(1).copied().unwrap_or(missing);
    if let Some(units) = c.string_code_units(body)? {
        if !units.is_empty() {
            let bytes: Vec<_> = units.into_iter().map(|unit| unit as u8).collect();
            let buffer = crate::modules::buffer_shared_vm::from_bytes(c, &bytes)?;
            call(c, stream, "push", &[buffer])?;
        }
    }
    if !c.truthy_rooted(open)? {
        let eof = c.null();
        call(c, stream, "push", &[eof])?;
    }
    Ok(stream)
}

pub(crate) fn destroy_socket(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let error = args.first().copied().unwrap_or(missing);
    let had_error = c.truthy_rooted(error)?;
    let data = c.host_function_data()?;
    let transport = get(c, data, "0")?;
    if let Some(id) = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64)
    {
        let state = c.host_mut().shared_state();
        let mut state = state.borrow_mut();
        if net_shared_vm::socket_write_queue_size(&state.tcp, id) > 0 {
            let _ = net_shared_vm::end(&mut state.tcp, id, false);
        } else {
            net_shared_vm::close_socket(&mut state.tcp, id);
        }
    }
    let no = c.boolean(false);
    set(c, receiver, "readable", no)?;
    set(c, receiver, "writable", no)?;
    let yes = c.boolean(true);
    set(c, receiver, "destroyed", yes)?;
    if had_error {
        let error_event = c.string_rooted("error");
        call(c, receiver, "emit", &[error_event, error])?;
    }
    let close = c.string_rooted("close");
    let had_error = c.boolean(had_error);
    call(c, receiver, "emit", &[close, had_error])?;
    Ok(receiver)
}

pub(crate) fn destroy_socket_by_id(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let key = c.string_rooted("__quenchHttpSocketId");
    let transport = c.get_property_rooted(receiver, key)?;
    let id = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64);
    c.release_root(transport);
    c.release_root(key);
    if let Some(id) = id {
        let state = c.host_mut().shared_state();
        let mut state = state.borrow_mut();
        // The synthetic socket used for `clientError` is backed by the native
        // transport but its `write()` only queues bytes there. Node's
        // Socket.destroy() after write still lets already accepted writes
        // drain; closing the transport immediately here would discard them.
        if net_shared_vm::socket_write_queue_size(&state.tcp, id) > 0 {
            let _ = net_shared_vm::end(&mut state.tcp, id, false);
        } else {
            net_shared_vm::close_socket(&mut state.tcp, id);
        }
    }
    let no = c.boolean(false);
    set(c, receiver, "readable", no)?;
    set(c, receiver, "writable", no)?;
    let yes = c.boolean(true);
    set(c, receiver, "destroyed", yes)?;
    let emit_key = c.string_rooted("emit");
    let emit = c.get_property_rooted(receiver, emit_key)?;
    c.release_root(emit_key);
    if c.is_callable_rooted(emit)? {
        let close = c.string_rooted("close");
        call(c, receiver, "emit", &[close])?;
        c.release_root(close);
    }
    c.release_root(emit);
    Ok(receiver)
}

pub(crate) fn end_socket_by_id(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let transport_key = c.string_rooted("__quenchHttpSocketId");
    let transport = c.get_property_rooted(receiver, transport_key)?;
    c.release_root(transport_key);
    if let Some(id) = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64)
    {
        if let Some(chunk) = args.first().copied().filter(|chunk| {
            c.rooted_value(*chunk)
                .is_some_and(|value| !value.is_null() && !value.is_undefined())
        }) {
            let bytes = c
                .view_bytes_rooted(chunk)
                .unwrap_or_else(|| c.to_string(chunk).unwrap_or_default().into_bytes());
            net_shared_vm::write_immediate(
                &mut c.host_mut().shared_state().borrow_mut().tcp,
                id,
                &bytes,
            )
            .map_err(RootedError::host)?;
        }
        net_shared_vm::end(&mut c.host_mut().shared_state().borrow_mut().tcp, id, false)
            .map_err(RootedError::host)?;
    }
    let no = c.boolean(false);
    set(c, receiver, "writable", no)?;
    Ok(receiver)
}

pub(crate) fn resume_socket(
    _c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // HTTP accepted sockets are driven by the shared transport poller. The
    // facade's resume() exists so stream.pipe() can install its data listener;
    // the poller emits those data events when upgraded socket bytes arrive.
    Ok(receiver)
}

pub(crate) fn write_socket_by_id(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key = c.string_rooted("__quenchHttpSocketId");
    let transport = c.get_property_rooted(receiver, key)?;
    let id = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64);
    c.release_root(transport);
    c.release_root(key);
    let id = id.ok_or_else(|| RootedError::host("HTTP client-error socket is unavailable"))?;
    let bytes = if let Some(value) = args.first().copied() {
        c.view_bytes_rooted(value)
            .unwrap_or_else(|| c.to_string(value).unwrap_or_default().into_bytes())
    } else {
        Vec::new()
    };
    let shared = c.host_mut().shared_state();
    net_shared_vm::write_immediate(&mut shared.borrow_mut().tcp, id, &bytes)
        .map_err(RootedError::host)?;
    Ok(c.boolean(true))
}

fn owner(
    c: &mut Context<'_>,
    receiver: RootId,
) -> Result<(RootId, bool, Option<u64>), RootedError> {
    let data = c.host_function_data()?;
    let stream = get(c, data, "0")?;
    if !c.strict_equal_rooted(stream, receiver)? {
        let error = c.type_error_rooted("Illegal invocation")?;
        return Err(c.throw(error));
    }
    let id = get(c, data, "1")?;
    let server = get(c, data, "2")?;
    let server = c.truthy_rooted(server)?;
    let number = c
        .rooted_value(id)
        .and_then(|value| value.as_number())
        .map(|number| number as u64);
    Ok((id, server, number))
}
pub(crate) fn read(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (_, server, id) = owner(c, receiver)?;
    let complete = get(c, receiver, "complete")?;
    if c.truthy_rooted(complete)? {
        return Ok(c.undefined());
    }
    if let Some(id) = id {
        let shared = c.host_mut().shared_state();
        let mut state = shared.borrow_mut();
        let socket = if server {
            Some(id)
        } else {
            state
                .http
                .clients
                .iter()
                .find_map(|(socket, client)| (client.request_id == id).then_some(*socket))
        };
        if let Some(socket) =
            socket.filter(|socket| !net_shared_vm::read_active(&state.tcp, *socket))
        {
            net_shared_vm::start_read(&mut state.tcp, socket)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    Ok(c.undefined())
}
pub(crate) fn destroy(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (id_root, server, id) = owner(c, receiver)?;
    let missing = c.undefined();
    let error = args.first().copied().unwrap_or(missing);
    let callback = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP incoming destroy callback missing"))?;
    let ended = get(c, receiver, "readableEnded")?;
    let complete = get(c, receiver, "complete")?;
    let aborted = !c.truthy_rooted(ended)? || !c.truthy_rooted(complete)?;
    if aborted {
        let yes = c.boolean(true);
        set(c, receiver, "aborted", yes)?;
        let event = c.string_rooted("aborted");
        call(c, receiver, "emit", &[event])?;
    }
    if let Some(id) = id.filter(|_| aborted) {
        if server {
            // stream.destroy() has a special server-request path that clears
            // req.socket before forwarding the abort error. Node keeps the
            // accepted connection available to ServerResponse in this case;
            // direct req.destroy(err) still owns and closes its socket.
            let socket = get(c, receiver, "socket")?;
            if c.truthy_rooted(socket)? {
                net_shared_vm::close_socket(&mut c.host_mut().shared_state().borrow_mut().tcp, id);
            }
        } else {
            super::client::destroy_response(c, missing, &[id_root])?;
        }
    }
    c.call_rooted(callback, missing, &[error])?;
    Ok(missing)
}
