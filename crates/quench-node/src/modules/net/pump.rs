//! `net` event-loop polling: accepts, socket reads/writes, and close
//! finalization, driven once per pump tick. Emits only after releasing
//! host borrows so JS callbacks can safely mutate sockets.

use std::io::Read;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};

use crate::host::HostState;

use super::*;

/// Poll every server and socket once: accept connections, announce
/// connects, read available bytes, flush writes, and finalize closes.
pub fn poll(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let events = std::mem::take(&mut state.borrow_mut().net.pending_events);
    for (receiver, event, args) in events {
        let socket =
            super::net_id(&receiver).and_then(|id| state.borrow().net.sockets.get(&id).cloned());
        if let Some(socket) = socket {
            emit_socket_scoped(state, &socket, &receiver, &event, args)?;
        } else {
            emit_server_scoped(state, &receiver, &event, args)?;
        }
    }
    let errors = std::mem::take(&mut state.borrow_mut().net.pending_errors);
    for (receiver, error) in errors {
        emit_server_scoped(state, &receiver, "error", vec![error])?;
    }
    // IncomingMessage.destroy() defers transport teardown until its queued
    // error/close observers have run. A close listener may still complete a
    // half-open ServerResponse, so only destroy requests that remain without
    // a response after dispatching the pending events.
    crate::modules::http::finalize_destroyed_requests(state)?;
    let writes = std::mem::take(&mut state.borrow_mut().net.pending_writes);
    for (socket, bytes) in writes {
        socket_write(
            state,
            Some(&socket),
            &[quench_runtime::host_api::bytes(&bytes)],
        )?;
    }
    let request_writes = std::mem::take(&mut state.borrow_mut().net.pending_request_writes);
    for (socket, bytes, request) in request_writes {
        if !crate::modules::http_client::request_write_allowed(state, &request) {
            continue;
        }
        socket_write(
            state,
            Some(&socket),
            &[quench_runtime::host_api::bytes(&bytes)],
        )?;
    }
    poll_accept(state)?;
    poll_sockets(state)?;
    finalize(state)?;
    poll_listening(state)?;
    poll_server_close(state)
}

fn emit_server_scoped(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    event: &str,
    args: Vec<Value>,
) -> Result<(), VmError> {
    let server_id = super::net_id(receiver);
    let scope = server_id.and_then(|id| {
        state
            .borrow()
            .net
            .servers
            .get(&id)
            .map(|server| server.borrow().process_scope)
    });
    let worker = server_id.and_then(|id| {
        state
            .borrow()
            .net
            .servers
            .get(&id)
            .and_then(|server| server.borrow().owner_worker)
            .and_then(|worker_id| {
                state
                    .borrow()
                    .cluster
                    .worker_object(worker_id)
                    .map(|worker| (worker_id, worker))
            })
    });
    let previous = state.borrow().cluster.process_scope();
    let previous_event_scope = state.borrow().event_loop.process_scope();
    let previous_worker = state.borrow().cluster.worker_context;
    if let Some(scope) = scope {
        state.borrow_mut().cluster.set_process_scope(scope);
        state.borrow().event_loop.set_process_scope(scope);
    }
    if let Some((worker_id, worker)) = &worker {
        crate::modules::cluster::set_worker_mode(state, *worker_id, worker, true);
        state.borrow_mut().cluster.worker_context = Some(*worker_id);
    }
    let result = emit(state, receiver, event, args);
    if let Some((worker_id, worker)) = &worker {
        crate::modules::cluster::set_worker_mode(state, *worker_id, worker, false);
    }
    state.borrow_mut().cluster.worker_context = previous_worker;
    state.borrow_mut().cluster.set_process_scope(previous);
    state
        .borrow()
        .event_loop
        .set_process_scope(previous_event_scope);
    result
}

fn emit_socket_scoped(
    state: &Rc<RefCell<HostState>>,
    socket: &Rc<RefCell<NetSocket>>,
    receiver: &Value,
    event: &str,
    args: Vec<Value>,
) -> Result<Value, VmError> {
    let scope = socket.borrow().process_scope;
    let worker = socket
        .borrow()
        .owner_worker
        .or_else(|| {
            socket.borrow().server_id.and_then(|server_id| {
                state
                    .borrow()
                    .net
                    .servers
                    .get(&server_id)
                    .and_then(|server| server.borrow().owner_worker)
            })
        })
        .and_then(|worker_id| {
            state
                .borrow()
                .cluster
                .worker_object(worker_id)
                .map(|worker| (worker_id, worker))
        });
    let previous = state.borrow().cluster.process_scope();
    let previous_event_scope = state.borrow().event_loop.process_scope();
    let previous_worker = state.borrow().cluster.worker_context;
    state.borrow_mut().cluster.set_process_scope(scope);
    state.borrow().event_loop.set_process_scope(scope);
    if let Some((worker_id, worker)) = &worker {
        crate::modules::cluster::set_worker_mode(state, *worker_id, worker, true);
        state.borrow_mut().cluster.worker_context = Some(*worker_id);
    }
    let result = emit(state, receiver, event, args);
    if let Some((worker_id, worker)) = &worker {
        crate::modules::cluster::set_worker_mode(state, *worker_id, worker, false);
    }
    state.borrow_mut().cluster.worker_context = previous_worker;
    state.borrow_mut().cluster.set_process_scope(previous);
    state
        .borrow()
        .event_loop
        .set_process_scope(previous_event_scope);
    result.map(|_| Value::Undefined)
}

fn poll_accept(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    for (server_id, stream, peer) in collect_accepts(state) {
        accept_one(state, server_id, stream, peer)?;
    }
    Ok(())
}

/// Accept every pending connection on every listening server, returning
/// `(server id, stream, peer)` tuples without touching the host state.
fn collect_accepts(state: &Rc<RefCell<HostState>>) -> Vec<(u64, TcpStream, SocketAddr)> {
    let mut accepted: Vec<(u64, TcpStream, SocketAddr)> = Vec::new();
    let host = state.borrow();
    let net = &host.net;
    let ids: Vec<u64> = net.servers.keys().copied().collect();
    for id in ids {
        let Some(server) = net.servers.get(&id) else {
            continue;
        };
        let server_id = server.borrow().id;
        let mut guard = server.borrow_mut();
        if !guard.listening {
            continue;
        }
        let Some(listener) = guard.listener.as_mut() else {
            continue;
        };
        loop {
            match listener.accept() {
                Ok((stream, peer)) => {
                    let _ = stream.set_nonblocking(true);
                    accepted.push((server_id, stream, peer));
                }
                Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }
    accepted
}

/// Register one accepted stream as a socket and emit `'connection'`.
fn accept_one(
    state: &Rc<RefCell<HostState>>,
    server_id: u64,
    stream: TcpStream,
    peer: SocketAddr,
) -> Result<(), VmError> {
    let blocked = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .map(|server| server.borrow().js.clone())
        .is_some_and(|server| {
            let block_list = execute::get_property(&server, "blockList");
            quench_runtime::is_callable(&execute::get_property(&block_list, "check"))
                && execute::call(
                    &execute::get_property(&block_list, "check"),
                    &block_list,
                    &[Value::String(peer.ip().to_string())],
                )
                .is_ok_and(|result| execute::is_truthy(&result))
        });
    if blocked {
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return Ok(());
    }
    let server_js = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .map(|server| server.borrow().js.clone());
    if let Some(server_js) = server_js {
        let max_connections = match execute::get_property(&server_js, "maxConnections") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize),
            _ => None,
        };
        let live_connections = state
            .borrow()
            .net
            .sockets
            .values()
            .filter(|socket| {
                let socket = socket.borrow();
                socket.server_id == Some(server_id) && socket.state != SocketState::Closed
            })
            .count();
        if max_connections.is_some_and(|limit| live_connections >= limit) {
            let local = stream.local_addr().ok();
            let info = host_api::object(net_info_props(peer, local));
            let _ = stream.shutdown(std::net::Shutdown::Both);
            emit(state, &server_js, "drop", vec![info])?;
            return Ok(());
        }
    }
    let (object, id) = new_net_object(state, socket_props())?;
    let object = install_socket_counters(object)?;
    let local = stream.local_addr().ok();
    // Path-backed listeners use the same bounded transport internally, but
    // retain pipe observables at the JS boundary: unix sockets do not expose
    // TCP remoteAddress/remoteFamily/localAddress fields.
    let is_pipe = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .is_some_and(|server| server.borrow().path.is_some());
    let allow_half_open = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .is_some_and(|server| server.borrow().allow_half_open);
    if allow_half_open {
        execute::set_property_in_place(&object, "allowHalfOpen", Value::Boolean(true));
    }
    let pause_on_connect = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .is_some_and(|server| server.borrow().pause_on_connect);
    if pause_on_connect {
        execute::set_property_in_place(&object, ONREAD_PAUSED_PROP, Value::Boolean(true));
    }
    let object = if is_pipe {
        object
    } else {
        install_methods(object, net_info_props(peer, local))?
    };
    let tls_options = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .map(|server| execute::get_property(&server.borrow().js, "_tlsOptions"))
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    let client_facts = tls_options.as_ref().and_then(|_| {
        state
            .borrow()
            .net
            .sockets
            .values()
            .find_map(|candidate| {
                let candidate = candidate.borrow();
                (candidate.peer == local && candidate.local == Some(peer)).then(|| {
                    (
                        execute::get_property(&candidate.js, "servername"),
                        execute::get_property(&candidate.js, crate::modules::tls::TLS_ALPN_PROP),
                    )
                })
            })
            .filter(|(servername, alpn)| {
                matches!(servername, Value::String(_)) || !matches!(alpn, Value::Undefined)
            })
    });
    if let Some(options) = tls_options.as_ref() {
        crate::modules::tls::decorate_socket(&object, Some(options));
        if let Some((servername, client_alpn)) = client_facts {
            if matches!(servername, Value::String(_)) {
                execute::set_property_in_place(&object, "servername", servername.clone());
            }
            let negotiated = crate::modules::tls::negotiate_alpn(
                options,
                &host_api::object(vec![("ALPNProtocols".into(), client_alpn)]),
            );
            execute::set_property_in_place(
                &object,
                crate::modules::tls::TLS_NEGOTIATED_ALPN_PROP,
                negotiated.map_or(Value::Boolean(false), Value::String),
            );
            if matches!(
                execute::get_property(options, "requestCert"),
                Value::Boolean(true)
            ) {
                execute::set_property_in_place(
                    &object,
                    "authorized",
                    Value::Boolean(
                        !matches!(servername, Value::String(ref value) if value == "unknowncontext"),
                    ),
                );
            }
        } else {
            execute::set_property_in_place(
                &object,
                crate::modules::tls::TLS_NEGOTIATED_ALPN_PROP,
                Value::Boolean(false),
            );
        }
    }
    let client_handle = host_api::object(vec![
        (
            "setNoDelay".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "setKeepAlive".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "close".into(),
            crate::host::capability(crate::registry::SPEC_NET_SOCKET_HANDLE_CLOSE),
        ),
    ]);
    execute::set_property_in_place(&object, "_handle", client_handle.clone());
    let object = state
        .borrow()
        .net
        .socket_prototype
        .clone()
        .map_or(Ok(object.clone()), |prototype| {
            execute::set_prototype_of(&object, &prototype)
        })?;
    let negotiated_alpn =
        execute::get_property(&object, crate::modules::tls::TLS_NEGOTIATED_ALPN_PROP);
    if !matches!(negotiated_alpn, Value::Undefined) {
        execute::set_property_in_place(&object, "alpnProtocol", negotiated_alpn);
    }
    set_socket_state(&object, false, false, "open");
    let socket = Rc::new(RefCell::new(NetSocket {
        id,
        process_scope: state
            .borrow()
            .net
            .servers
            .get(&server_id)
            .map(|server| server.borrow().process_scope)
            .unwrap_or_else(|| state.borrow().cluster.process_scope()),
        owner_worker: state.borrow().cluster.worker_context,
        stream: Some(stream),
        js: object.clone(),
        state: SocketState::Open,
        refed: true,
        server_id: Some(server_id),
        write_buf: Vec::new(),
        write_offset: 0,
        read_buf: Vec::new(),
        bytes_read: 0,
        bytes_written: 0,
        read_eof: false,
        close_emitted: false,
        close_deferred: false,
        write_shutdown_pending: false,
        finish_emitted: false,
        connect_announced: true,
        peer: Some(peer),
        local,
        encoding: None,
        decode_buf: Vec::new(),
    }));
    state.borrow_mut().net.sockets.insert(id, socket);
    let server_js = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .map(|server| server.borrow().js.clone());
    let tls_server = server_js.clone();
    if let Some(js) = server_js {
        let previous_scope = state.borrow().cluster.process_scope();
        let previous_event_scope = state.borrow().event_loop.process_scope();
        let server_scope = state
            .borrow()
            .net
            .servers
            .get(&server_id)
            .map(|server| server.borrow().process_scope)
            .unwrap_or(previous_scope);
        state.borrow_mut().cluster.set_process_scope(server_scope);
        state.borrow().event_loop.set_process_scope(server_scope);
        // Accepted sockets expose the exact JS Server instance that owns the
        // transport.  Install this before emitting `connection` so listeners
        // observe stable identity during construction.
        execute::set_property_in_place(&object, "server", js.clone());
        let server_handle = execute::get_property(&js, "_handle");
        let onconnection = execute::get_property(&server_handle, "onconnection");
        let owner_worker = state
            .borrow()
            .net
            .servers
            .get(&server_id)
            .and_then(|server| server.borrow().owner_worker);
        let delivered_to_cluster_worker = if let Some(worker_id) = owner_worker {
            // Round-robin cluster listeners receive accepted handles through
            // the worker IPC channel before net.Server's onconnection hook.
            // Keep this as a transport fact: user internalMessage listeners
            // can close/reject the handle, while ordinary workers fall back
            // to the normal connection callback below.
            let has_internal_listener =
                crate::modules::process::has_listener(state, "internalMessage");
            if has_internal_listener {
                let worker = state
                    .borrow()
                    .cluster
                    .worker_object(worker_id)
                    .ok_or_else(|| execute::type_error("cluster worker"))?;
                let previous_worker = state.borrow().cluster.worker_context;
                crate::modules::cluster::set_worker_mode(state, worker_id, &worker, true);
                state.borrow_mut().cluster.worker_context = Some(worker_id);
                let message =
                    host_api::object(vec![("act".into(), Value::String("newconn".into()))]);
                let result = crate::modules::process::emit(
                    state,
                    &[
                        Value::String("internalMessage".into()),
                        message,
                        client_handle.clone(),
                    ],
                );
                state.borrow_mut().cluster.worker_context = previous_worker;
                crate::modules::cluster::set_worker_mode(state, worker_id, &worker, false);
                result?;

                // Closing the worker's listening server removes its
                // round-robin handle. The primary then closes the accepted
                // handle rather than handing it to net.Server.
                let server_closed = state
                    .borrow()
                    .net
                    .servers
                    .get(&server_id)
                    .is_some_and(|server| server.borrow().closed);
                if server_closed {
                    let close = execute::get_property(&client_handle, "close");
                    if quench_runtime::is_callable(&close) {
                        execute::call(&close, &client_handle, &[])?;
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        if !delivered_to_cluster_worker
            && quench_runtime::is_callable(&onconnection)
            && !matches!(
                onconnection,
                Value::Builtin(quench_runtime::ops::Builtin::Object)
            )
        {
            execute::call(
                &onconnection,
                &server_handle,
                &[Value::Null, client_handle.clone()],
            )?;
        }
        if matches!(execute::get_property(&js, "noDelay"), Value::Boolean(true)) {
            let setter = execute::get_property(&client_handle, "setNoDelay");
            execute::call(&setter, &client_handle, &[Value::Boolean(true)])?;
            execute::set_property_in_place(&object, NO_DELAY_PROP, Value::Boolean(true));
        }
        super::queue_async_value(state, server_id, object.clone());
        let owner = state
            .borrow()
            .net
            .servers
            .get(&server_id)
            .and_then(|server| server.borrow().owner_worker)
            .and_then(|worker_id| {
                state
                    .borrow()
                    .cluster
                    .worker_object(worker_id)
                    .map(|worker| (worker_id, worker))
            });
        let connection_result = if delivered_to_cluster_worker {
            // The IPC handoff above is the worker's connection notification;
            // emitting the server event as well would deliver the same
            // accepted handle twice.
            Ok(())
        } else if let Some((worker_id, worker)) = owner {
            let previous = state.borrow().cluster.worker_context;
            crate::modules::cluster::set_worker_mode(state, worker_id, &worker, true);
            state.borrow_mut().cluster.worker_context = Some(worker_id);
            let result = emit(state, &js, "connection", vec![object.clone()]);
            state.borrow_mut().cluster.worker_context = previous;
            crate::modules::cluster::set_worker_mode(state, worker_id, &worker, false);
            result
        } else {
            emit(state, &js, "connection", vec![object.clone()])
        };
        state.borrow_mut().cluster.set_process_scope(previous_scope);
        state
            .borrow()
            .event_loop
            .set_process_scope(previous_event_scope);
        connection_result?;
        if let Some(server) = tls_server {
            if crate::modules::tls::is_tls_server(&server) {
                emit(state, &server, "secureConnection", vec![object])?;
            }
        }
    }
    Ok(())
}

/// Queued per-tick socket events, gathered without emitting.
struct SocketEvents {
    connects: Vec<Rc<RefCell<NetSocket>>>,
    datas: Vec<(Rc<RefCell<NetSocket>>, Vec<u8>)>,
    eofs: Vec<Rc<RefCell<NetSocket>>>,
    write_failures: Vec<Rc<RefCell<NetSocket>>>,
    drains: Vec<Rc<RefCell<NetSocket>>>,
}

fn poll_sockets(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let events = read_sockets(state);
    for sock in events.connects {
        let (js, peer, local) = {
            let guard = sock.borrow();
            (guard.js.clone(), guard.peer, guard.local)
        };
        if let (Some(peer), Some(local)) = (peer, local) {
            // The socket is already visible to JS callbacks. Add address
            // metadata in place so emitter identity and listener tables stay
            // attached to the same object.
            // An adopted BoundSocket owns the source endpoint observable. The
            // transport stream is created normally, but its public localPort
            // must remain the reserved bound port rather than the kernel's
            // unrelated ephemeral source port.
            let local = match execute::get_property(&js, BOUND_LOCAL_PORT_PROP) {
                Value::Number(port)
                    if port.is_finite() && (0.0..=u16::MAX as f64).contains(&port) =>
                {
                    SocketAddr::new(local.ip(), port as u16)
                }
                _ => local,
            };
            for (key, value) in net_info_props(peer, Some(local)) {
                execute::set_property_in_place(&js, &key, value);
            }
            if let Value::String(address) = execute::get_property(&js, BOUND_LOCAL_ADDRESS_PROP) {
                execute::set_property_in_place(&js, "localAddress", Value::String(address));
            }
        }
        set_socket_state(&js, false, false, "open");
        if matches!(
            execute::get_property(&js, super::NO_DELAY_PROP),
            Value::Boolean(true)
        ) {
            super::socket_set_no_delay(state, Some(&js), &[Value::Boolean(true)])?;
        }
        crate::modules::http_client::apply_deferred_request_timeout(state, &js)?;
        if matches!(
            execute::get_property(&js, crate::modules::tls::TLS_REJECTED_PROP),
            Value::Boolean(true)
        ) {
            continue;
        }
        emit_socket_scoped(state, &sock, &js, "connect", Vec::new())?;
        // Native sockets report the same `net` PerformanceEntry surface as
        // Node.  The queue/observer semantics live in the bootstrap bridge;
        // this edge only supplies transport facts owned by Rust.
        let record = state.borrow().net.performance_record.clone();
        if let Some(record) = record.filter(|value| quench_runtime::is_callable(value)) {
            if matches!(
                execute::get_property(&js, super::PIPE_MARKER_PROP),
                Value::Boolean(true)
            ) {
                continue;
            }
            if let Some(peer) = peer {
                let detail = quench_runtime::host_api::object(vec![
                    ("host".into(), Value::String(peer.ip().to_string())),
                    ("port".into(), Value::Number(peer.port() as f64)),
                ]);
                let _ = execute::call(
                    &record,
                    &Value::Undefined,
                    &[
                        Value::String("net".into()),
                        detail,
                        Value::String("connect".into()),
                    ],
                );
            }
        }
        if matches!(
            execute::get_property(&js, crate::modules::tls::TLS_SOCKET_PROP),
            Value::Boolean(true)
        ) && !matches!(
            execute::get_property(&js, crate::modules::tls::TLS_REJECTED_PROP),
            Value::Boolean(true)
        ) {
            emit_socket_scoped(state, &sock, &js, "secureConnect", Vec::new())?;
        }
    }
    for (sock, bytes) in events.datas {
        let js = sock.borrow().js.clone();
        let visible_read = match execute::get_property(&js, "bytesRead") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value,
            _ => 0.0,
        };
        super::replace_socket_property(
            &js,
            "bytesRead",
            Value::Number(visible_read + bytes.len() as f64),
        );
        super::set_socket_bytes_read(
            &js,
            (visible_read as u64).saturating_add(bytes.len() as u64),
        );
        // A parser-detached HTTP agent socket has no public `data` listener;
        // any bytes arriving while it is in freeSockets are unsolicited and
        // poison the next response. Destroy it before dispatching stream data.
        if crate::modules::http::is_idle_socket(state, &js) {
            execute::set_property_in_place(&js, "destroyed", Value::Boolean(true));
            crate::modules::http_client::mark_socket_destroyed_in_agents(state, &js);
            crate::modules::http_client::req_error(state, Some(&js), &[])?;
            // Keep the safety transition local to the net state as well:
            // a pooled request may already have lost its HTTP association,
            // but unsolicited bytes must still destroy this socket.
            crate::modules::net::socket_destroy(state, Some(&js), &[])?;
            continue;
        }
        let arg = {
            let mut guard = sock.borrow_mut();
            data_value(&mut guard, &bytes)
        };
        super::queue_async_value(state, sock.borrow().id, arg.clone());
        let callback = execute::get_property(&js, ONREAD_CALLBACK_PROP);
        if quench_runtime::is_callable(&callback) {
            let consumed = emit_onread(&js, &bytes, &callback)?;
            if consumed < bytes.len() {
                // A callback may pause the socket. Retain unread bytes until
                // resume makes the same onread source available again.
                sock.borrow_mut()
                    .read_buf
                    .extend_from_slice(&bytes[consumed..]);
            }
        } else {
            emit_socket_scoped(state, &sock, &js, "data", vec![arg])?;
        }
    }
    for sock in events.drains {
        let js = sock.borrow().js.clone();
        emit_socket_scoped(state, &sock, &js, "drain", Vec::new())?;
    }
    for sock in events.eofs {
        let js = sock.borrow().js.clone();
        if !sock.borrow().read_buf.is_empty() {
            execute::set_property_in_place(&js, ONREAD_EOF_PROP, Value::Boolean(true));
            continue;
        }
        let callback = execute::get_property(&js, ONREAD_CALLBACK_PROP);
        if quench_runtime::is_callable(&callback) {
            let source = execute::get_property(&js, ONREAD_BUFFER_PROP);
            let buffer = if quench_runtime::is_callable(&source) {
                execute::call(&source, &js, &[])?
            } else {
                source
            };
            execute::call(&callback, &js, &[Value::Number(0.0), buffer])?;
        }
        super::set_socket_property(&js, "readable", Value::Boolean(false));
        let js = execute::canonical_value(&js);
        execute::set_property_in_place(&js, "readable", Value::Boolean(false));
        super::end_async_stream(state, sock.borrow().id);
        emit_socket_scoped(state, &sock, &js, "end", Vec::new())?;
        // An HTTP/1.1 peer may half-close after pipelined requests. Once the
        // parser has no active request left, finish the server write side so
        // the peer observes its expected final `end` event even when
        // `httpAllowHalfOpen` is enabled.
        let close_http = sock.borrow().server_id.is_some()
            && matches!(
                execute::get_property(&js, crate::modules::http::HTTP_SERVER_SOCKET_PROP),
                Value::Boolean(true)
            )
            && state
                .borrow()
                .http
                .conns
                .get(&sock.borrow().id)
                .is_some_and(|conn| {
                    conn.req.is_none() && !conn.head_parsed && conn.buffer.is_empty()
                });
        if close_http {
            super::socket_end(state, Some(&js), &[])?;
        }
    }
    for sock in events.write_failures {
        let js = sock.borrow().js.clone();
        let http_owned = sock.borrow().server_id.is_some()
            && matches!(
                execute::get_property(&js, crate::modules::http::HTTP_SERVER_SOCKET_PROP),
                Value::Boolean(true)
            );
        if !http_owned {
            emit_socket_scoped(state, &sock, &js, "error", vec![super::peer_write_error()])?;
        }
        super::socket_destroy(state, Some(&js), &[])?;
    }
    Ok(())
}

fn emit_onread(socket: &Value, bytes: &[u8], callback: &Value) -> Result<usize, VmError> {
    let mut offset = 0;
    while offset < bytes.len() {
        let source = execute::get_property(socket, ONREAD_BUFFER_PROP);
        let buffer = if quench_runtime::is_callable(&source) {
            execute::call(&source, socket, &[])?
        } else {
            source
        };
        let (start, capacity) = match &buffer {
            Value::Uint8Array(view) => (view.byte_offset, view.length),
            Value::DataView(view) => (view.byte_offset, view.byte_length),
            _ => return Err(VmError::NotCallable),
        };
        if capacity == 0 {
            break;
        }
        let count = (bytes.len() - offset).min(capacity);
        match &buffer {
            Value::Uint8Array(view) => {
                view.buffer.bytes.borrow_mut()[start..start + count]
                    .copy_from_slice(&bytes[offset..offset + count]);
            }
            Value::DataView(view) => {
                view.buffer.bytes.borrow_mut()[start..start + count]
                    .copy_from_slice(&bytes[offset..offset + count]);
            }
            _ => unreachable!(),
        }
        let result = execute::call(callback, socket, &[Value::Number(count as f64), buffer])?;
        offset += count;
        if matches!(result, Value::Boolean(false)) {
            // Returning false is the onread backpressure signal. Persist the
            // implicit pause on the same socket so the next pump turn cannot
            // re-enter the callback until resume() clears it.
            super::replace_socket_property(socket, ONREAD_PAUSED_PROP, Value::Boolean(true));
            break;
        }
        if matches!(
            execute::get_property(socket, ONREAD_PAUSED_PROP),
            Value::Boolean(true)
        ) {
            break;
        }
    }
    Ok(offset)
}

/// Gather connect / data / end events for one tick, releasing borrows.
fn read_sockets(state: &Rc<RefCell<HostState>>) -> SocketEvents {
    let mut events = SocketEvents {
        connects: Vec::new(),
        datas: Vec::new(),
        eofs: Vec::new(),
        write_failures: Vec::new(),
        drains: Vec::new(),
    };
    let host = state.borrow();
    let net = &host.net;
    let ids: Vec<u64> = net.sockets.keys().copied().collect();
    for id in ids {
        let Some(sock) = net.sockets.get(&id) else {
            continue;
        };
        let mut guard = sock.borrow_mut();
        if guard.state == SocketState::Closed {
            continue;
        }
        if guard.stream.is_some() && guard.state != SocketState::Closed && !guard.connect_announced
        {
            guard.connect_announced = true;
            events.connects.push(sock.clone());
        }
        let paused = matches!(
            execute::get_property(&guard.js, ONREAD_PAUSED_PROP),
            Value::Boolean(true)
        );
        if !paused {
            if !guard.read_buf.is_empty() {
                let pending = std::mem::take(&mut guard.read_buf);
                events.datas.push((sock.clone(), pending));
                continue;
            }
            if guard.read_eof
                && matches!(
                    execute::get_property(&guard.js, ONREAD_EOF_PROP),
                    Value::Boolean(true)
                )
            {
                execute::set_property_in_place(&guard.js, ONREAD_EOF_PROP, Value::Boolean(false));
                events.eofs.push(sock.clone());
                continue;
            }
            if read_available(sock, &mut guard, &mut events.datas) {
                events.eofs.push(sock.clone());
            }
        }
        let pending_before = pending_write_len(&guard);
        let flushed = try_flush(&mut guard);
        if pending_before > 0 && flushed {
            events.drains.push(sock.clone());
        }
        let allow_half_open = matches!(
            execute::get_property(&guard.js, "allowHalfOpen"),
            Value::Boolean(true)
        );
        if guard.read_eof && pending_write_len(&guard) > 0 && !allow_half_open {
            guard.write_buf.clear();
            guard.write_offset = 0;
            events.write_failures.push(sock.clone());
        }
        if flushed
            && guard.state == SocketState::Closing
            && pending_write_len(&guard) == 0
            && guard.write_shutdown_pending
        {
            if let Some(stream) = guard.stream.as_mut() {
                let _ = stream.shutdown(Shutdown::Write);
            }
            guard.write_shutdown_pending = false;
        }
    }
    events
}

/// Drain available readable bytes into `datas`; returns true on EOF.
fn read_available(
    sock: &Rc<RefCell<NetSocket>>,
    guard: &mut std::cell::RefMut<'_, NetSocket>,
    datas: &mut Vec<(Rc<RefCell<NetSocket>>, Vec<u8>)>,
) -> bool {
    if guard.stream.is_none() || guard.read_eof {
        return false;
    }
    let mut had_eof = false;
    loop {
        let mut buf = [0u8; READ_CHUNK];
        let result = guard
            .stream
            .as_mut()
            .expect("stream checked above")
            .read(&mut buf);
        match result {
            Ok(0) => {
                guard.read_eof = true;
                had_eof = true;
                guard.state = SocketState::Closing;
                // Keep the write side open until the queued `data`/`end`
                // callbacks have run.  A single read turn can contain both
                // the final bytes and the peer FIN; shutting down here would
                // discard writes produced by those callbacks.
                break;
            }
            Ok(n) => {
                guard.bytes_read = guard.bytes_read.saturating_add(n as u64);
                datas.push((sock.clone(), buf[..n].to_vec()));
                // Deliver one kernel chunk per pump turn.  A `data` observer
                // may pause the socket; reading ahead here would bypass that
                // observable backpressure boundary before the callback runs.
                break;
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => {
                guard.read_eof = true;
                had_eof = true;
                guard.state = SocketState::Closing;
                break;
            }
        }
    }
    had_eof
}

fn poll_listening(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let mut announce: Vec<Rc<RefCell<NetServer>>> = Vec::new();
    {
        let host = state.borrow();
        for server in host.net.servers.values() {
            let mut guard = server.borrow_mut();
            if guard.listening && !guard.announced {
                guard.announced = true;
                announce.push(server.clone());
            }
        }
    }
    for server in announce {
        let js = server.borrow().js.clone();
        let owner = server.borrow().owner_worker;
        let address = server.borrow().bind_addr.map(|address| {
            host_api::object(vec![
                ("address".into(), Value::String(address.ip().to_string())),
                ("family".into(), Value::String(family(address))),
                ("port".into(), Value::Number(address.port() as f64)),
            ])
        });
        emit_server_scoped(state, &js, "listening", Vec::new())?;
        if let (Some(worker_id), Some(address)) = (owner, address) {
            crate::modules::cluster::notify_listening(state, worker_id, address);
        }
    }
    Ok(())
}

/// A closed server with no remaining connections emits `'close'`.
fn poll_server_close(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let mut closed: Vec<Rc<RefCell<NetServer>>> = Vec::new();
    {
        let host = state.borrow();
        let has_open_sockets = |server_id: u64| {
            host.net.sockets.values().any(|s| {
                s.borrow().server_id == Some(server_id) && s.borrow().state != SocketState::Closed
            })
        };
        for server in host.net.servers.values() {
            let mut guard = server.borrow_mut();
            if guard.closed && !guard.close_emitted && !has_open_sockets(guard.id) {
                guard.close_emitted = true;
                closed.push(server.clone());
            }
        }
    }
    for server in closed {
        let (id, js) = {
            let guard = server.borrow();
            (guard.id, guard.js.clone())
        };
        super::end_async_stream(state, id);
        emit_server_scoped(state, &js, "close", Vec::new())?;
        state.borrow_mut().net.servers.remove(&id);
    }
    Ok(())
}

/// `'close'` for sockets that read EOF and drained their writes; the socket
/// leaves the live set only after both sides of the stream are complete.
pub fn finalize(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let mut to_finish: Vec<Value> = Vec::new();
    let mut to_close: Vec<Rc<RefCell<NetSocket>>> = Vec::new();
    {
        let host = state.borrow();
        for sock in host.net.sockets.values() {
            let mut guard = sock.borrow_mut();
            if guard.close_emitted || guard.state == SocketState::Closed {
                continue;
            }
            let allow_half_open = matches!(
                execute::get_property(&guard.js, "allowHalfOpen"),
                quench_runtime::value::Value::Boolean(true)
            );
            let done = guard.read_eof
                && pending_write_len(&guard) == 0
                && (!allow_half_open || guard.finish_emitted);
            if done {
                if !guard.close_deferred {
                    guard.close_deferred = true;
                    continue;
                }
                if !guard.finish_emitted
                    && !matches!(
                        execute::get_property(&guard.js, "allowHalfOpen"),
                        quench_runtime::value::Value::Boolean(true)
                    )
                {
                    guard.finish_emitted = true;
                    execute::set_property_in_place(&guard.js, "writable", Value::Boolean(false));
                    execute::set_property_in_place(
                        &guard.js,
                        "readyState",
                        Value::String("readOnly".into()),
                    );
                    to_finish.push(guard.js.clone());
                }
                if let Some(stream) = guard.stream.take() {
                    let _ = stream.shutdown(Shutdown::Both);
                }
                guard.state = SocketState::Closed;
                guard.close_emitted = true;
                to_close.push(sock.clone());
            }
        }
    }
    let mut host = state.borrow_mut();
    for sock in &to_close {
        host.net.sockets.remove(&sock.borrow().id);
    }
    drop(host);
    // Finalization is the other terminal path besides `socket.destroy()`.
    // Clear the socket's host timer before dropping the registry entry;
    // otherwise a long idle timeout (for example 120s on a server peer)
    // remains referenced and prevents process exit after `server.close()`.
    for sock in &to_close {
        let (id, js) = {
            let guard = sock.borrow();
            (guard.id, guard.js.clone())
        };
        let timer = { state.borrow_mut().net.timeout_timers.remove(&id) };
        if let Some(timer) = timer {
            crate::modules::timers::clear_timeout(state, &[timer])?;
        }
        execute::set_property_in_place(&js, super::methods::SOCKET_TIMEOUT_PROP, Value::Undefined);
    }
    for js in to_finish {
        emit(state, &js, "finish", Vec::new())?;
    }
    for sock in to_close {
        let js = sock.borrow().js.clone();
        let bytes_read = sock.borrow().bytes_read;
        set_socket_state(&js, true, false, "closed");
        super::replace_socket_property(&js, "pending", Value::Boolean(true));
        super::set_socket_bytes_read(&js, bytes_read);
        super::replace_socket_property(&js, "_handle", quench_runtime::value::Value::Null);
        crate::modules::http::connection_close(state, &js)?;
        // Net socket close carries Node's `hadError` boolean argument.
        emit_socket_scoped(state, &sock, &js, "close", vec![Value::Boolean(false)])?;
    }
    Ok(())
}
