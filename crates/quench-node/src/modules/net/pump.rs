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
    // The QUIC transport shares the host pump with TCP.  It only moves bytes
    // between non-blocking UDP sockets and the host-owned queue here; session
    // and packet semantics remain above this transport boundary.
    crate::modules::quic_transport::poll(state);
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
        // A server response may call `end(chunk)` before `respond()` and then
        // write more data in the same callback turn. Once its queued
        // END_STREAM frame reaches the transport, the temporary allowance for
        // those same-turn writes is no longer valid.
        if let Ok(Some(header)) = crate::modules::http2_protocol::FrameHeader::decode(&bytes) {
            if header.flags & 0x1 != 0 {
                if let Some(socket_id) = super::net_id(&socket) {
                    let stream = state
                        .borrow()
                        .net
                        .http2_streams
                        .get(&(socket_id, header.stream_id))
                        .cloned();
                    if let Some(stream) = stream {
                        execute::set_property_in_place(
                            &stream,
                            crate::modules::http2_util::HTTP2_PENDING_FINAL_PROP,
                            quench_runtime::value::Value::Boolean(false),
                        );
                        let canonical = execute::canonical_value(&stream);
                        execute::set_property_in_place(
                            &canonical,
                            crate::modules::http2_util::HTTP2_PENDING_FINAL_PROP,
                            quench_runtime::value::Value::Boolean(false),
                        );
                    }
                }
            }
        }
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
    // A paused HTTP/2 stream may have received complete DATA frames while
    // the transport was still readable. Deliver those frames only after the
    // stream's shared pause fact is cleared by `resume()`.
    flush_paused_http2_data(state)?;
    // HTTP/2 stream errors are held until after socket polling so a buffered
    // RST_STREAM is flushed (and can be read by the peer) before user error
    // handlers are allowed to close the client session.
    let http2_events = std::mem::take(&mut state.borrow_mut().net.pending_http2_events);
    for (receiver, event, args) in http2_events {
        let socket = super::net_id(&receiver)
            .or_else(|| {
                let owner = execute::get_property(&receiver, "\0quench:http2-socket");
                super::net_id(&owner)
            })
            .and_then(|id| state.borrow().net.sockets.get(&id).cloned());
        if let Some(socket) = socket {
            emit_socket_scoped(state, &socket, &receiver, &event, args)?;
        } else {
            emit_server_scoped(state, &receiver, &event, args)?;
        }
    }
    finalize(state)?;
    // A cluster worker's process `disconnect` event is ordered after its
    // listening servers' `close` events. Emit closed servers before the
    // cluster finalizer can retire their records, while retaining the final
    // checkpoint below for servers that become idle later in this tick.
    poll_server_close(state)?;
    crate::modules::cluster::finalize_disconnected_workers(state);
    let fork_scopes = state.borrow().cluster.fork_scopes();
    for scope in fork_scopes {
        let _ = crate::modules::cluster::finish_idle_fork_process(state, scope)?;
    }
    retire_deferred_scopes(state);
    poll_listening(state)?;
    poll_server_close(state)
}

/// Release process-local listeners only after the last referenced network
/// handle has reached its terminal state.  IPC disconnect can precede a
/// socket's EOF, and those callbacks remain part of the child's observable
/// lifecycle until the host transport has drained.
fn retire_deferred_scopes(state: &Rc<RefCell<HostState>>) {
    let pending = state
        .borrow()
        .deferred_emitter_scopes
        .iter()
        .copied()
        .collect::<Vec<_>>();
    let scopes = pending
        .into_iter()
        .filter(|scope| !super::has_live_scope(state, *scope))
        .collect::<Vec<_>>();
    {
        let mut host = state.borrow_mut();
        for scope in &scopes {
            host.deferred_emitter_scopes.remove(scope);
        }
    }
    for scope in scopes {
        state.borrow_mut().emitters.remove_scope(scope);
    }
}

fn emit_server_scoped(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    event: &str,
    args: Vec<Value>,
) -> Result<(), VmError> {
    // Host lifecycle queues may retain a pre-COW stream representative while
    // listener registration has already published its live replacement.
    // Resolve that identity before looking up scoped listeners or invoking
    // callbacks so deferred stream errors reach the object observers use.
    let receiver = execute::canonical_value(receiver);
    let server_id = super::net_id(&receiver);
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
    let result = emit(state, &receiver, event, args);
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
    // Deferred HTTP/2 stream errors are delivered through the transport's
    // canonical representative. Apply the terminal stream fact to the
    // queued receiver before canonicalization so copy-on-write aliases see
    // `destroyed === true` inside their error callback.
    if event == "error"
        && !matches!(
            execute::get_property(receiver, "\0quench:http2-stream-id"),
            Value::Undefined
        )
        && args.first().is_some_and(|error| {
            matches!(
                execute::get_property(error, "code"),
                Value::String(code)
                    if code == "ERR_HTTP2_STREAM_CANCEL" || code == "ERR_HTTP2_GOAWAY_SESSION"
            )
        })
    {
        execute::set_property_in_place(receiver, "destroyed", Value::Boolean(true));
    }
    let receiver = execute::canonical_value(receiver);
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
    let result = emit(state, &receiver, event, args);
    if event == "close" {
        let callback = execute::get_property(&receiver, "\0quench:http2-close-callback");
        if quench_runtime::is_callable(&callback) {
            execute::set_property_in_place(
                &receiver,
                "\0quench:http2-close-callback",
                Value::Undefined,
            );
            execute::call(&callback, &receiver, &[])?;
        }
    }
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
        let canonical_id = {
            let server = server.borrow();
            server.path.as_ref().map_or(id, |path| {
                net.servers
                    .values()
                    .filter_map(|candidate| {
                        let candidate = candidate.borrow();
                        (candidate.path.as_ref() == Some(path)
                            && candidate.listening
                            && !candidate.closed
                            && candidate.listener.is_some())
                            .then_some(candidate.id)
                    })
                    .min()
                    .unwrap_or(id)
            })
        };
        if canonical_id != id {
            continue;
        }
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

fn connection_limit(server: &Value) -> Option<usize> {
    match execute::get_property(server, "maxConnections") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize),
        _ => None,
    }
}

fn live_connections(state: &Rc<RefCell<HostState>>, server_id: u64) -> usize {
    state
        .borrow()
        .net
        .sockets
        .values()
        .filter(|socket| {
            let socket = socket.borrow();
            socket.server_id == Some(server_id) && socket.state != SocketState::Closed
        })
        .count()
}

/// Route an accepted shared-path connection to one logical worker.  The
/// listener is a single host fact; worker capacity is derived from each
/// server's public `maxConnections` value, with construction order providing
/// a deterministic round-robin fallback.
fn shared_path_target(state: &Rc<RefCell<HostState>>, source_id: u64) -> u64 {
    let candidates = {
        let host = state.borrow();
        let Some(source) = host.net.servers.get(&source_id) else {
            return source_id;
        };
        let Some(path) = source.borrow().path.clone() else {
            return source_id;
        };
        let mut candidates = host
            .net
            .servers
            .values()
            .filter_map(|server| {
                let server = server.borrow();
                (server.path.as_ref() == Some(&path) && server.listening && !server.closed)
                    .then_some((server.id, server.js.clone()))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(id, _)| *id);
        candidates
    };
    if candidates.len() < 2 {
        return source_id;
    }
    candidates
        .iter()
        .find(|(id, server)| {
            connection_limit(server).is_none_or(|limit| live_connections(state, *id) < limit)
        })
        .map(|(id, _)| *id)
        .unwrap_or_else(|| candidates[0].0)
}

/// Register one accepted stream as a socket and emit `'connection'`.
fn accept_one(
    state: &Rc<RefCell<HostState>>,
    source_server_id: u64,
    stream: TcpStream,
    peer: SocketAddr,
) -> Result<(), VmError> {
    let server_id = shared_path_target(state, source_server_id);
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
        if let Some((servername, client_alpn)) = client_facts.clone() {
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
    let http2_server = state.borrow().net.http2_servers.contains(&server_id);
    let tls_server = matches!(
        execute::get_property(&object, crate::modules::tls::TLS_SOCKET_PROP),
        Value::Boolean(true)
    );
    let negotiated_h2 = matches!(
        execute::get_property(&object, crate::modules::tls::TLS_NEGOTIATED_ALPN_PROP),
        Value::String(protocol) if protocol == "h2"
    );
    let is_http2 = http2_server && (!tls_server || negotiated_h2);
    if is_http2 {
        execute::set_property_in_place(
            &object,
            crate::modules::http2_protocol::SERVER_MARKER,
            Value::Boolean(true),
        );
        crate::modules::net::register_http2_session(
            state,
            &object,
            crate::modules::http2_protocol::Role::Server,
        );
        crate::modules::http2_util::decorate_server_session(&object)?;
        let server_settings = state
            .borrow()
            .net
            .http2_server_settings
            .get(&server_id)
            .cloned()
            .or_else(|| {
                server_js
                    .as_ref()
                    .map(|server| execute::get_property(server, "\0quench:http2-settings"))
            });
        crate::modules::http2_util::configure_session_settings(
            &object,
            server_settings
                .as_ref()
                .filter(|settings| matches!(settings, Value::Object(_) | Value::ObjectAlias(_))),
        );
        let server_remote_custom = state
            .borrow()
            .net
            .http2_server_remote_custom
            .get(&server_id)
            .cloned();
        if let Some(custom) = server_remote_custom {
            execute::set_property_in_place(&object, "\0quench:http2-remote-custom", custom);
        }
        // A server sends its initial SETTINGS frame after accepting the
        // transport; the client preface is sent by `http2.connect()`.
        let settings_payload = server_settings
            .as_ref()
            .filter(|settings| matches!(settings, Value::Object(_) | Value::ObjectAlias(_)))
            .and_then(|settings| crate::modules::http2_util::packed_settings_payload(settings))
            .unwrap_or_default();
        let settings = crate::modules::http2_protocol::Frame::new(
            crate::modules::http2_protocol::FrameType::Settings,
            0,
            0,
            settings_payload,
        );
        let write = execute::get_property(&object, "write");
        if quench_runtime::is_callable(&write) {
            execute::call(
                &write,
                &object,
                &[crate::modules::buffer_proto::make_buffer(
                    &settings.encode(),
                )],
            )?;
        }
    }
    // A secure HTTP/2 server with `allowHTTP1` accepts an ordinary HTTP/1.1
    // ALPN result on the same listener.  Only negotiated `h2` sessions use
    // the frame parser above; attach the canonical HTTP parser for the
    // alternate protocol so request/response events retain normal identity
    // and lifecycle semantics.
    let allow_http1 = http2_server
        && tls_server
        && !negotiated_h2
        && server_js.as_ref().is_some_and(|server| {
            matches!(
                execute::get_property(&execute::get_property(server, "_tlsOptions"), "allowHTTP1"),
                Value::Boolean(true)
            )
        });
    if allow_http1 {
        if let Some(server) = server_js.as_ref() {
            crate::modules::http::connection_handler(state, Some(server), &[object.clone()])?;
        }
    }
    let tls_server = server_js.clone();
    if is_http2 {
        if let Some(server) = server_js.clone() {
            // A session is established when the accepted HTTP/2 transport
            // reaches the protocol boundary, before request streams arrive.
            // Keep the accepted socket as the session identity so control
            // callbacks and teardown share one host-owned resource.
            emit_server_scoped(state, &server, "session", vec![object.clone()])?;
        }
    }
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
            let has_internal_listener = state
                .borrow()
                .cluster
                .worker_event_scope(worker_id)
                .is_some_and(|scope| {
                    crate::modules::process::has_listener_in_scope(state, "internalMessage", scope)
                });
            if has_internal_listener {
                let worker = state
                    .borrow()
                    .cluster
                    .worker_object(worker_id)
                    .ok_or_else(|| execute::type_error("cluster worker"))?;
                let previous_worker = state.borrow().cluster.worker_context;
                let previous_event_scope = state.borrow().event_loop.process_scope();
                let worker_event_scope = state
                    .borrow()
                    .cluster
                    .worker_event_scope(worker_id)
                    .unwrap_or(server_scope);
                crate::modules::cluster::set_worker_mode(state, worker_id, &worker, true);
                state.borrow_mut().cluster.worker_context = Some(worker_id);
                state
                    .borrow()
                    .event_loop
                    .set_process_scope(worker_event_scope);
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
                state
                    .borrow()
                    .event_loop
                    .set_process_scope(previous_event_scope);
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
            let previous_event_scope = state.borrow().event_loop.process_scope();
            let worker_event_scope = state
                .borrow()
                .cluster
                .worker_event_scope(worker_id)
                .unwrap_or(server_scope);
            crate::modules::cluster::set_worker_mode(state, worker_id, &worker, true);
            state.borrow_mut().cluster.worker_context = Some(worker_id);
            state
                .borrow()
                .event_loop
                .set_process_scope(worker_event_scope);
            let result = emit(state, &js, "connection", vec![object.clone()]);
            state.borrow_mut().cluster.worker_context = previous;
            state
                .borrow()
                .event_loop
                .set_process_scope(previous_event_scope);
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

fn http2_capability(kind: &str) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
        vec![Value::String(kind.into())],
    )
}

pub(crate) fn http2_headers_value(fields: &[(Vec<u8>, Vec<u8>)]) -> Value {
    let mut headers = host_api::object(Vec::new());
    // Header names such as `constructor` and `__proto__` are ordinary wire
    // fields.  Remove Object.prototype before duplicate aggregation so an
    // inherited property cannot masquerade as an already-seen header.
    headers = execute::set_prototype_of(&headers, &Value::Null).unwrap_or(headers);
    for (name, value) in fields {
        let key = String::from_utf8_lossy(name).into_owned();
        // Empty header names are ignored by Node's HTTP/2 header decoder.
        // Keep this at the single wire-to-JS boundary so ordinary headers,
        // pseudo-headers, and compatibility request views share the same
        // validation rather than each filtering independently.
        if key.is_empty() {
            continue;
        }
        // Node exposes the response `:status` pseudo-header as a number;
        // ordinary and request pseudo-headers remain strings.  Keeping this
        // conversion at the single wire-to-JS boundary prevents every
        // response consumer from having to reinterpret the HPACK bytes.
        let value = if key == ":status" {
            String::from_utf8_lossy(value)
                .parse::<f64>()
                .map(Value::Number)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(value).into_owned()))
        } else {
            Value::String(String::from_utf8_lossy(value).into_owned())
        };
        let previous = execute::get_property(&headers, &key);
        let next = match (key.as_str(), previous) {
            (_, Value::Undefined) => value,
            ("set-cookie", array @ Value::Array(_)) => {
                let length = execute::get_property(&array, "length");
                if let Value::Number(length) = length {
                    let _ = execute::set_array_index_in_place(&array, length as usize, value);
                }
                array
            }
            ("set-cookie", current) => host_api::array(vec![current, value]),
            ("cookie", Value::String(current)) => {
                let incoming = execute::to_js_string(&value).unwrap_or_default();
                Value::String(format!("{current}; {incoming}"))
            }
            (_, Value::String(current)) => {
                let incoming = execute::to_js_string(&value).unwrap_or_default();
                Value::String(format!("{current}, {incoming}"))
            }
            (_, current) => current,
        };
        let _ = execute::set_property_in_place(&headers, &key, next);
    }
    if let Value::String(sensitive) = crate::modules::http2_util::sensitive_headers() {
        headers = http2_define_symbol_property(
            headers,
            &Value::String(sensitive),
            host_api::array(Vec::new()),
        );
    }
    headers
}

fn http2_define_symbol_property(object: Value, key: &Value, value: Value) -> Value {
    let global = quench_runtime::vm::current_global_object();
    let object_constructor = execute::get_property(&global, "Object");
    let define_property = execute::get_property(&object_constructor, "defineProperty");
    let descriptor = host_api::object(vec![
        ("value".into(), value),
        ("writable".into(), Value::Boolean(true)),
        ("enumerable".into(), Value::Boolean(true)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    execute::call(
        &define_property,
        &object_constructor,
        &[object.clone(), key.clone(), descriptor],
    )
    .unwrap_or(object)
}

/// Preserve the wire header sequence for Node's `stream` event.  The object
/// form intentionally coalesces duplicate names; `rawHeaders` must not, since
/// callers use it to inspect ordering and repeated fields.
fn http2_raw_headers_value(fields: &[(Vec<u8>, Vec<u8>)]) -> Value {
    let mut raw = Vec::with_capacity(fields.len() * 2);
    for (name, value) in fields {
        let name = String::from_utf8_lossy(name);
        raw.push(Value::String(if name.starts_with(':') {
            name.into_owned()
        } else {
            name.to_ascii_lowercase()
        }));
        raw.push(Value::String(String::from_utf8_lossy(value).into_owned()));
    }
    host_api::array(raw)
}

/// Return the stream emitter associated with a wire stream, creating it once.
/// The same object is used for client response/data events and server stream
/// callbacks so listener identity remains stable across pump ticks.
fn http2_stream(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
    stream_id: u32,
) -> Result<(Value, bool), VmError> {
    if let Some(socket_id) = crate::modules::net::net_id(socket) {
        if let Some(existing) = state
            .borrow()
            .net
            .http2_streams
            .get(&(socket_id, stream_id))
            .cloned()
        {
            return Ok((existing, false));
        }
    }
    let streams = match execute::get_property(socket, "\0quench:http2-streams") {
        Value::Object(_) | Value::ObjectAlias(_) => {
            execute::get_property(socket, "\0quench:http2-streams")
        }
        _ => {
            let map = host_api::object(Vec::new());
            execute::set_property_in_place(socket, "\0quench:http2-streams", map.clone());
            map
        }
    };
    let key = stream_id.to_string();
    let existing = execute::get_property(&streams, &key);
    if matches!(existing, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok((existing, false));
    }
    let stream = crate::modules::events::new_emitter_object(state)?;
    execute::set_property_in_place(&stream, "\0quench:http2-socket", socket.clone());
    execute::set_property_in_place(
        &stream,
        "\0quench:http2-stream-id",
        Value::Number(stream_id as f64),
    );
    execute::set_property_in_place(&stream, "id", Value::Number(stream_id as f64));
    execute::set_property_in_place(&stream, "write", http2_capability("streamWrite"));
    execute::set_property_in_place(&stream, "end", http2_capability("streamEnd"));
    execute::set_property_in_place(&stream, "close", http2_capability("streamClose"));
    execute::set_property_in_place(&stream, "destroy", http2_capability("streamDestroy"));
    execute::set_property_in_place(&stream, "respond", http2_capability("streamRespond"));
    execute::set_property_in_place(&stream, "pushStream", http2_capability("streamPushStream"));
    execute::set_property_in_place(
        &stream,
        "setEncoding",
        http2_capability("streamSetEncoding"),
    );
    execute::set_property_in_place(&stream, "resume", http2_capability("streamResume"));
    execute::set_property_in_place(&stream, "pause", http2_capability("streamPause"));
    let session_socket = crate::modules::net::net_id(socket)
        .and_then(|id| {
            state
                .borrow()
                .net
                .sockets
                .get(&id)
                .map(|entry| entry.borrow().js.clone())
        })
        .unwrap_or_else(|| socket.clone());
    execute::set_property_in_place(&stream, "session", session_socket);
    execute::set_property_in_place(&stream, "rstCode", Value::Number(0.0));
    crate::modules::http2_util::decorate_http2_stream(state, &stream, false);
    // A client-side even stream is a server push response, not a writable
    // request. Mark its local side complete so END_STREAM closes it instead of
    // waiting forever for a nonexistent upload `.end()`.
    let push_response = state
        .borrow()
        .net
        .http2_sessions
        .get(&crate::modules::net::net_id(socket).unwrap_or_default())
        .is_some_and(|session| {
            matches!(session.role(), crate::modules::http2_protocol::Role::Client)
                && stream_id % 2 == 0
        });
    if push_response {
        execute::set_property_in_place(&stream, "\0quench:http2-end-stream", Value::Boolean(true));
        execute::set_property_in_place(&stream, "writableEnded", Value::Boolean(true));
        execute::set_property_in_place(&stream, "writableFinished", Value::Boolean(true));
    }
    if let Some(socket_id) = crate::modules::net::net_id(socket) {
        state
            .borrow_mut()
            .net
            .http2_streams
            .insert((socket_id, stream_id), stream.clone());
    }
    execute::set_property_in_place(&streams, &key, stream.clone());
    Ok((stream, true))
}

fn emit_http2_stream_close(
    state: &Rc<RefCell<HostState>>,
    socket: &Rc<RefCell<NetSocket>>,
    stream: &Value,
    server: bool,
    emit_end: bool,
) -> Result<(), VmError> {
    let stream = execute::canonical_value(stream);
    if matches!(
        execute::get_property(&stream, "__quenchHttp2CloseEmitted"),
        Value::Boolean(true)
    ) {
        return Ok(());
    }
    let request_writable_open = !matches!(
        execute::get_property(&stream, "\0quench:http2:end-stream"),
        Value::Boolean(true)
    ) && !matches!(
        execute::get_property(&stream, "writableEnded"),
        Value::Boolean(true)
    );
    // An inbound END_STREAM closes only the readable/request half of a server
    // stream.  The compatibility response is still allowed to write after
    // the request body ends (and may do so from a later `drain` callback), so
    // retain the stream's writable half until the response sends its own
    // terminal END_STREAM.  RST_STREAM and already-ended responses remain
    // terminal below.
    let defer_server_close = server && emit_end && request_writable_open;
    // A peer RST is terminal for both halves even when the request body was
    // still open; only a clean response END_STREAM permits a full-duplex
    // writable half-close to be deferred until the upload calls `end()`.
    let defer_client_close = !server && emit_end && request_writable_open;
    // Client response END_STREAM closes the readable side of the shared
    // request/response stream. Keep the local writable side open until the
    // upload sends its own END_STREAM (for example, a response can arrive
    // while a file is still being piped into the request).
    if defer_client_close {
        execute::set_property_in_place(
            &stream,
            crate::modules::http2_util::HTTP2_RESPONSE_CLOSED_PROP,
            Value::Boolean(true),
        );
    }
    if !defer_client_close && !defer_server_close {
        execute::set_property_in_place(&stream, "__quenchHttp2CloseEmitted", Value::Boolean(true));
    }
    if defer_server_close {
        execute::set_property_in_place(&stream, "\0quench:http2-remote-end", Value::Boolean(true));
    } else {
        execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
    }
    crate::modules::http2_util::publish_http2_stream_diagnostic(
        state,
        &stream,
        server,
        crate::modules::http2_util::HTTP2_DIAG_CLOSE,
        None,
        None,
        None,
    )?;
    if emit_end {
        execute::set_property_in_place(
            &stream,
            "\0quench:http2-end-dispatch",
            Value::Boolean(true),
        );
        emit_socket_scoped(state, socket, &stream, "end", Vec::new())?;
        if server {
            crate::modules::http2_util::emit_compat_request_end(state, &stream)?;
        }
        execute::set_property_in_place(
            &stream,
            "\0quench:http2-end-dispatch",
            Value::Boolean(false),
        );
    }
    // A client response without a `response` listener is auto-discarded by
    // Node and reaches its terminal destroyed state before `close`. Keep that
    // state transition tied to the observable listener fact; ordinary
    // response streams retain their existing close ordering.
    let response_listeners = crate::modules::events::method_listener_count(
        state,
        Some(&stream),
        &[Value::String("response".into())],
    )
    .ok()
    .and_then(|value| match value {
        Value::Number(count) if count.is_finite() && count >= 0.0 => Some(count as usize),
        _ => None,
    })
    .unwrap_or(0);
    if !server && response_listeners == 0 {
        execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
    }
    if !defer_client_close && !defer_server_close {
        emit_socket_scoped(state, socket, &stream, "close", Vec::new())?;
        crate::modules::http2_util::queue_compat_response_close(state, &stream);
        let session_socket = socket.borrow().js.clone();
        crate::modules::http2_util::maybe_finalize_session_close(state, &session_socket)?;
    }
    Ok(())
}

fn http2_data_value(stream: &Value, payload: &[u8]) -> Value {
    match execute::get_property(stream, "encoding") {
        Value::String(encoding) if encoding == "utf8" || encoding == "utf-8" => {
            Value::String(String::from_utf8_lossy(payload).into_owned())
        }
        // Node delivers HTTP/2 DATA callbacks as Buffers unless the stream
        // has an explicit text encoding. Preserve that one conversion rule
        // for both immediate and paused delivery.
        _ => crate::modules::buffer_proto::make_buffer(payload),
    }
}

fn queue_paused_http2_data(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
    stream_id: u32,
    data: Value,
    end: bool,
) {
    state
        .borrow_mut()
        .net
        .http2_paused_data
        .entry((socket_id, stream_id))
        .or_default()
        .push(data);
    if end {
        state
            .borrow_mut()
            .net
            .http2_paused_end
            .insert((socket_id, stream_id));
    }
}

/// Flush body frames accumulated while one or more HTTP/2 streams were
/// paused. The queue is attached to the canonical stream, so a compatibility
/// request and its raw `Http2Stream` view cannot observe divergent bodies.
pub(crate) fn flush_paused_http2_data(
    state: &Rc<RefCell<HostState>>,
) -> Result<(), VmError> {
    let streams = state
        .borrow()
        .net
        .http2_streams
        .iter()
        .map(|(&(socket_id, stream_id), stream)| (socket_id, stream_id, stream.clone()))
        .collect::<Vec<_>>();
    for (socket_id, stream_id, stream) in streams {
        if matches!(
            execute::get_property(&stream, crate::modules::http2_util::HTTP2_STREAM_PAUSED_PROP),
            Value::Boolean(true)
        ) {
            continue;
        }
        let (queue, end) = {
            let mut host = state.borrow_mut();
            let queue = host
                .net
                .http2_paused_data
                .remove(&(socket_id, stream_id))
                .unwrap_or_default();
            let end = host.net.http2_paused_end.remove(&(socket_id, stream_id));
            (queue, end)
        };
        if queue.is_empty() && !end {
            continue;
        }
        let Some(socket) = state.borrow().net.sockets.get(&socket_id).cloned() else {
            continue;
        };
        let is_server = state
            .borrow()
            .net
            .http2_sessions
            .get(&socket_id)
            .is_some_and(|session| {
                matches!(session.role(), crate::modules::http2_protocol::Role::Server)
        });
        for data in queue {
            emit_socket_scoped(state, &socket, &stream, "data", vec![data.clone()])?;
            if is_server {
                crate::modules::http2_util::emit_compat_request_data(state, &stream, data)?;
            }
        }
        if end {
            emit_http2_stream_close(state, &socket, &stream, is_server, true)?;
        }
        // Keep the stream id read in this loop's identity check; the map key
        // remains the authoritative association if a callback decorates a
        // replacement stream object.
        let _ = stream_id;
    }
    Ok(())
}

pub(crate) fn dispatch_http2_frames(
    state: &Rc<RefCell<HostState>>,
    socket: &Rc<RefCell<NetSocket>>,
    frames: &[crate::modules::http2_protocol::Frame],
) -> Result<(), VmError> {
    let socket_js = socket.borrow().js.clone();
    let socket_id = socket.borrow().id;
    // The protocol session role is the canonical source of direction.  Socket
    // markers can be shared through aliases when a server and client are
    // created in one VM, but a Session owns exactly one HTTP/2 role.
    let is_server = state
        .borrow()
        .net
        .http2_sessions
        .get(&socket_id)
        .is_some_and(|session| {
            matches!(session.role(), crate::modules::http2_protocol::Role::Server)
        });
    // `Session::feed` may complete a header block on a CONTINUATION frame;
    // consume completed blocks once per stream instead of assuming the first
    // HEADERS frame is self-contained.  Preserve the stream/end flags from
    // all fragments for the observable event dispatch below.
    let mut completed_headers = state
        .borrow_mut()
        .net
        .http2_sessions
        .get_mut(&socket.borrow().id)
        .map(|session| session.take_new_headers())
        .unwrap_or_default()
        .into_iter()
        .fold(
            std::collections::HashMap::<u32, std::collections::VecDeque<_>>::new(),
            |mut headers, (id, fields)| {
                headers.entry(id).or_default().push_back(fields);
                headers
            },
        );
    let push_promise_headers = state
        .borrow_mut()
        .net
        .http2_sessions
        .get_mut(&socket.borrow().id)
        .map(|session| session.take_push_promises())
        .unwrap_or_default();
    let mut header_flags = std::collections::HashMap::<u32, u8>::new();
    // A peer may coalesce request HEADERS and an immediate RST_STREAM (as
    // AbortSignal cancellation does) into one read. Record those resets up
    // front so the server's `stream` callback observes the terminal rstCode,
    // matching Node's stream lifecycle ordering.
    let batch_resets = frames
        .iter()
        .filter_map(|frame| {
            (frame.header.kind == crate::modules::http2_protocol::FrameType::RstStream).then(|| {
                let code = frame
                    .payload
                    .get(..4)
                    .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
                    .unwrap_or(0);
                (frame.header.stream_id, code)
            })
        })
        .collect::<std::collections::HashMap<_, _>>();
    for frame in frames {
        if matches!(
            frame.header.kind,
            crate::modules::http2_protocol::FrameType::Headers
                | crate::modules::http2_protocol::FrameType::Continuation
        ) {
            *header_flags.entry(frame.header.stream_id).or_default() |= frame.header.flags;
        } else if matches!(
            frame.header.kind,
            crate::modules::http2_protocol::FrameType::Data
        ) && frame.header.flags & 1 != 0
        {
            // A zero-length DATA with END_STREAM is how `request().end()`
            // terminates a header-only request in this transport. Surface
            // the canonical Node stream flag (END_HEADERS|END_STREAM) on the
            // preceding `stream` event.
            *header_flags.entry(frame.header.stream_id).or_default() |= 1;
        }
    }
    // A CONTINUATION belongs to the preceding HEADERS block and must not
    // consume a second queued block. Once END_HEADERS arrives, the next
    // HEADERS on the same stream is a distinct response/trailer block.
    let mut open_header_blocks = std::collections::HashSet::new();
    for frame in frames {
        let stream_id = frame.header.stream_id;
        match frame.header.kind {
            crate::modules::http2_protocol::FrameType::Settings => {
                if frame.header.flags & 1 == 0 {
                    if matches!(
                        execute::get_property(&socket_js, "destroyed"),
                        Value::Boolean(true)
                    ) {
                        continue;
                    }
                    let ack = crate::modules::http2_protocol::Frame::new(
                        crate::modules::http2_protocol::FrameType::Settings,
                        1,
                        0,
                        Vec::new(),
                    );
                    let write = execute::get_property(&socket_js, "write");
                    if quench_runtime::is_callable(&write) {
                        execute::call(
                            &write,
                            &socket_js,
                            &[crate::modules::buffer_proto::make_buffer(&ack.encode())],
                        )?;
                    }
                    let remote = crate::modules::http2_util::settings_from_payload(&frame.payload);
                    let allowed = {
                        let hidden =
                            execute::get_property(&socket_js, "\0quench:http2-remote-custom");
                        if matches!(hidden, Value::Array(_)) {
                            hidden
                        } else {
                            let server_id = state
                                .borrow()
                                .net
                                .sockets
                                .get(&socket_id)
                                .and_then(|entry| entry.borrow().server_id);
                            server_id
                                .and_then(|id| {
                                    state
                                        .borrow()
                                        .net
                                        .http2_server_remote_custom
                                        .get(&id)
                                        .cloned()
                                })
                                .unwrap_or(Value::Undefined)
                        }
                    };
                    crate::modules::http2_util::filter_custom_settings(
                        &remote,
                        matches!(allowed, Value::Array(_)).then_some(&allowed),
                    );
                    super::replace_socket_property(&socket_js, "remoteSettings", remote.clone());
                    super::emit(state, &socket_js, "remoteSettings", vec![remote])?;
                    if !is_server {
                        let local = execute::get_property(&socket_js, "localSettings");
                        super::emit(state, &socket_js, "localSettings", vec![local])?;
                    }
                } else {
                    execute::set_property_in_place(
                        &socket_js,
                        "pendingSettingsAck",
                        Value::Boolean(false),
                    );
                }
            }
            crate::modules::http2_protocol::FrameType::PushPromise => {
                if is_server || frame.payload.len() < 4 {
                    continue;
                }
                let promised_id =
                    u32::from_be_bytes(frame.payload[..4].try_into().unwrap()) & 0x7fff_ffff;
                let fields = push_promise_headers
                    .get(&promised_id)
                    .cloned()
                    .or_else(|| {
                        completed_headers
                            .get(&promised_id)
                            .and_then(|fields| fields.front().cloned())
                    })
                    .unwrap_or_default();
                let (push_stream, push_fresh) = http2_stream(state, &socket_js, promised_id)?;
                // Compatibility push responses expose the underlying stream
                // through `.stream`, matching ServerHttp2Stream wrappers.
                execute::set_property_in_place(&push_stream, "stream", push_stream.clone());
                crate::modules::http2_util::decorate_http2_stream(state, &push_stream, false);
                execute::set_property_in_place(
                    &push_stream,
                    "__quenchHttp2PushStream",
                    Value::Boolean(true),
                );
                execute::set_property_in_place(
                    &push_stream,
                    "\0quench:http2:end-stream",
                    Value::Boolean(true),
                );
                let mut entries = fields
                    .iter()
                    .map(|(name, value)| {
                        let key = String::from_utf8_lossy(name).into_owned();
                        let value = if key == ":status" {
                            String::from_utf8_lossy(value)
                                .parse::<f64>()
                                .map(Value::Number)
                                .unwrap_or_else(|_| {
                                    Value::String(String::from_utf8_lossy(value).into_owned())
                                })
                        } else {
                            Value::String(String::from_utf8_lossy(value).into_owned())
                        };
                        (key, value)
                    })
                    .collect::<Vec<_>>();
                if let Value::String(sensitive) = crate::modules::http2_util::sensitive_headers() {
                    let pseudo_count = fields
                        .iter()
                        .take_while(|(name, _)| name.first() == Some(&b':'))
                        .count();
                    entries.insert(pseudo_count, (sensitive, host_api::array(Vec::new())));
                }
                let headers = host_api::object(entries);
                let headers = execute::set_prototype_of(&headers, &Value::Null).unwrap_or(headers);
                if let Value::String(sensitive) = crate::modules::http2_util::sensitive_headers() {
                    let _ = execute::set_property_in_place(
                        &headers,
                        &sensitive,
                        host_api::array(Vec::new()),
                    );
                }
                // A pushed stream receives a second HEADERS frame when the
                // server sends its response.  Keep the original request
                // headers from PUSH_PROMISE attached to the stream so the
                // client diagnostics `created` event reports the same
                // metadata as Node instead of the response headers.
                execute::set_property_in_place(
                    &push_stream,
                    "__quenchHttp2PushDiagnostics",
                    headers.clone(),
                );
                emit_socket_scoped(
                    state,
                    socket,
                    &socket_js,
                    "stream",
                    vec![
                        push_stream.clone(),
                        headers.clone(),
                        Value::Number(frame.header.flags as f64),
                        http2_raw_headers_value(&fields),
                    ],
                )?;
                // Defer `push` until the promised stream's response HEADERS
                // arrive. Node emits `stream` with the PUSH_PROMISE request
                // headers first, then emits `push` with response headers.
                let _ = push_fresh;
            }
            crate::modules::http2_protocol::FrameType::Headers
            | crate::modules::http2_protocol::FrameType::Continuation => {
                let is_continuation =
                    frame.header.kind == crate::modules::http2_protocol::FrameType::Continuation;
                if is_continuation {
                    if !open_header_blocks.remove(&stream_id) {
                        continue;
                    }
                    if frame.header.flags & 0x4 == 0 {
                        open_header_blocks.insert(stream_id);
                        continue;
                    }
                } else if open_header_blocks.contains(&stream_id) {
                    continue;
                } else if frame.header.flags & 0x4 == 0 {
                    open_header_blocks.insert(stream_id);
                    continue;
                }
                let fields = completed_headers
                    .get_mut(&stream_id)
                    .and_then(std::collections::VecDeque::pop_front);
                let Some(fields) = fields else { continue };
                let (stream, fresh) = http2_stream(state, &socket_js, stream_id)?;
                if let Some(code) = batch_resets.get(&stream_id).copied() {
                    execute::set_property_in_place(&stream, "rstCode", Value::Number(code as f64));
                    execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
                }
                let locally_reset = state
                    .borrow()
                    .net
                    .http2_reset_codes
                    .contains_key(&(socket_id, stream_id));
                crate::modules::http2_util::decorate_http2_stream(state, &stream, is_server);
                if is_server && fresh {
                    // Node reports GET request streams as ending with their
                    // initial headers, even though the wire implementation
                    // may deliver the END_STREAM bit on a separate frame.
                    // Derive the public fact from the decoded request method,
                    // which remains stable across split header blocks.
                    let end_after_headers = fields.iter().any(|(name, value)| {
                        name.as_slice() == b":method" && value.as_slice() == b"GET"
                    });
                    execute::set_property_in_place(
                        &stream,
                        "endAfterHeaders",
                        Value::Boolean(end_after_headers),
                    );
                    let canonical = execute::canonical_value(&stream);
                    execute::set_property_in_place(
                        &canonical,
                        "endAfterHeaders",
                        Value::Boolean(end_after_headers),
                    );
                }
                if !is_server
                    && matches!(
                        execute::get_property(&stream, "\0quench:http2:end-stream"),
                        Value::Boolean(true)
                    )
                {
                    execute::set_property_in_place(&stream, "writableEnded", Value::Boolean(true));
                    execute::set_property_in_place(
                        &stream,
                        "writableFinished",
                        Value::Boolean(true),
                    );
                }
                let mut headers = if !is_server {
                    match execute::get_property(&stream, "__quenchHttp2RequestDiagnostics") {
                        Value::Object(_) | Value::ObjectAlias(_) => {
                            execute::get_property(&stream, "__quenchHttp2RequestDiagnostics")
                        }
                        _ => match execute::get_property(
                            &socket_js,
                            "\0quench:http2-request-diagnostics-map",
                        ) {
                            Value::Object(_) | Value::ObjectAlias(_) => {
                                match execute::get_property(
                                    &execute::get_property(
                                        &socket_js,
                                        "\0quench:http2-request-diagnostics-map",
                                    ),
                                    &stream_id.to_string(),
                                ) {
                                    Value::Object(_) | Value::ObjectAlias(_) => {
                                        execute::get_property(
                                            &execute::get_property(
                                                &socket_js,
                                                "\0quench:http2-request-diagnostics-map",
                                            ),
                                            &stream_id.to_string(),
                                        )
                                    }
                                    _ => http2_headers_value(&fields),
                                }
                            }
                            _ => http2_headers_value(&fields),
                        },
                    }
                } else {
                    http2_headers_value(&fields)
                };
                if !is_server
                    && (stream_id % 2 == 0
                        || matches!(
                            execute::get_property(&stream, "__quenchHttp2PushStream"),
                            Value::Boolean(true)
                        ))
                {
                    if stream_id % 2 == 0 {
                        execute::set_property_in_place(
                            &stream,
                            "__quenchHttp2PushStream",
                            Value::Boolean(true),
                        );
                    }
                    if let Value::String(sensitive) =
                        crate::modules::http2_util::sensitive_headers()
                    {
                        let _ = execute::set_property_in_place(
                            &headers,
                            &sensitive,
                            host_api::array(Vec::new()),
                        );
                    }
                    headers = execute::set_prototype_of(&headers, &Value::Null).unwrap_or(headers);
                }
                if is_server {
                    execute::set_property_in_place(
                        &stream,
                        "__quenchHttp2RequestDiagnostics",
                        headers.clone(),
                    );
                    let map = match execute::get_property(
                        &socket_js,
                        "\0quench:http2-request-diagnostics-map",
                    ) {
                        Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(
                            &socket_js,
                            "\0quench:http2-request-diagnostics-map",
                        ),
                        _ => {
                            let map = host_api::object(Vec::new());
                            execute::set_property_in_place(
                                &socket_js,
                                "\0quench:http2-request-diagnostics-map",
                                map.clone(),
                            );
                            map
                        }
                    };
                    execute::set_property_in_place(&map, &stream_id.to_string(), headers.clone());
                }
                let args = vec![
                    stream.clone(),
                    headers.clone(),
                    Value::Number(
                        header_flags
                            .get(&stream_id)
                            .copied()
                            .unwrap_or(frame.header.flags) as f64,
                    ),
                    http2_raw_headers_value(&fields),
                ];
                // Mark the initial server request headers before invoking the
                // user callback. The callback may synchronously end the
                // request and cause a trailer HEADERS block to arrive on the
                // next pump tick; publishing the fact up front keeps that
                // block on the same stream state even if callback mutations
                // publish a copy-on-write representative.
                if is_server && fresh {
                    execute::set_property_in_place(
                        &stream,
                        "__quenchHttp2RequestHeadersEmitted",
                        Value::Boolean(true),
                    );
                    let canonical = execute::canonical_value(&stream);
                    execute::set_property_in_place(
                        &canonical,
                        "__quenchHttp2RequestHeadersEmitted",
                        Value::Boolean(true),
                    );
                }
                if is_server && fresh {
                    if fresh {
                        crate::modules::http2_util::publish_http2_stream_diagnostic(
                            state,
                            &stream,
                            true,
                            crate::modules::http2_util::HTTP2_DIAG_CREATED,
                            Some(headers.clone()),
                            None,
                            None,
                        )?;
                        crate::modules::http2_util::publish_http2_stream_diagnostic(
                            state,
                            &stream,
                            true,
                            crate::modules::http2_util::HTTP2_DIAG_START,
                            Some(headers.clone()),
                            None,
                            None,
                        )?;
                    }
                    let server = socket
                        .borrow()
                        .server_id
                        .and_then(|server_id| {
                            state
                                .borrow()
                                .net
                                .servers
                                .get(&server_id)
                                .map(|entry| entry.borrow().js.clone())
                        })
                        .unwrap_or_else(|| execute::get_property(&socket_js, "server"));
                    if fresh && matches!(server, Value::Object(_) | Value::ObjectAlias(_)) {
                        let request_listener = socket
                            .borrow()
                            .server_id
                            .and_then(|server_id| {
                                state
                                    .borrow()
                                    .net
                                    .http2_request_listeners
                                    .get(&server_id)
                                    .cloned()
                            })
                            .unwrap_or_else(|| {
                                execute::get_property(&server, "\0quench:http2-request-listener")
                            });
                        let request_event_listeners =
                            crate::modules::events::method_listener_count(
                                state,
                                Some(&server),
                                &[Value::String("request".into())],
                            )
                            .ok()
                            .and_then(|value| match value {
                                Value::Number(count) if count.is_finite() && count > 0.0 => {
                                    Some(count as usize)
                                }
                                _ => None,
                            })
                            .unwrap_or(0);
                        let check_continue_listeners =
                            crate::modules::events::method_listener_count(
                                state,
                                Some(&server),
                                &[Value::String("checkContinue".into())],
                            )
                            .ok()
                            .and_then(|value| match value {
                                Value::Number(count) if count.is_finite() && count > 0.0 => {
                                    Some(count as usize)
                                }
                                _ => None,
                            })
                            .unwrap_or(0);
                        let check_expectation_listeners =
                            crate::modules::events::method_listener_count(
                                state,
                                Some(&server),
                                &[Value::String("checkExpectation".into())],
                            )
                            .ok()
                            .and_then(|value| match value {
                                Value::Number(count) if count.is_finite() && count > 0.0 => {
                                    Some(count as usize)
                                }
                                _ => None,
                            })
                            .unwrap_or(0);
                        let expectation = match execute::get_property(&headers, "expect") {
                            Value::Undefined | Value::Null => String::new(),
                            value => execute::to_js_string(&value).unwrap_or_default(),
                        };
                        let check_event = if expectation.eq_ignore_ascii_case("100-continue")
                            && check_continue_listeners > 0
                        {
                            Some("checkContinue")
                        } else if !expectation.is_empty() && check_expectation_listeners > 0 {
                            Some("checkExpectation")
                        } else {
                            None
                        };
                        if let Some(event) = check_event {
                            let (request, response) =
                                crate::modules::http2_util::compat_server_request_response(
                                    state, &stream, &headers, &args[3],
                                )?;
                            emit_server_scoped(state, &server, event, vec![request, response])?;
                        } else if !expectation.is_empty()
                            && !expectation.eq_ignore_ascii_case("100-continue")
                        {
                            // Without a checkExpectation listener Node
                            // rejects an unsupported expectation with 417
                            // and does not invoke the ordinary request hook.
                            let (_request, response) =
                                crate::modules::http2_util::compat_server_request_response(
                                    state, &stream, &headers, &args[3],
                                )?;
                            crate::modules::http2_util::compat_response_write_head(
                                state,
                                Some(&response),
                                &[Value::Number(417.0), host_api::object(Vec::new())],
                            )?;
                            crate::modules::http2_util::compat_response_end(
                                state,
                                Some(&response),
                                &[],
                            )?;
                        } else if quench_runtime::is_callable(&request_listener) {
                            // `createServer` is the compatibility API: its
                            // callback receives request/response views, while
                            // the raw stream remains available through the
                            // server's `stream` event. Both views retain the
                            // canonical transport socket identity.
                            let (request, response) =
                                crate::modules::http2_util::compat_server_request_response(
                                    state, &stream, &headers, &args[3],
                                )?;
                            if expectation.eq_ignore_ascii_case("100-continue") {
                                let _ = crate::modules::http2_util::compat_response_write_continue(
                                    state,
                                    Some(&response),
                                    &[],
                                )?;
                            }
                            execute::call(&request_listener, &server, &[request, response])?;
                        } else if request_event_listeners > 0 {
                            let (request, response) =
                                crate::modules::http2_util::compat_server_request_response(
                                    state, &stream, &headers, &args[3],
                                )?;
                            if expectation.eq_ignore_ascii_case("100-continue") {
                                let _ = crate::modules::http2_util::compat_response_write_continue(
                                    state,
                                    Some(&response),
                                    &[],
                                )?;
                            }
                            emit_server_scoped(state, &server, "request", vec![request, response])?;
                        } else {
                            emit_server_scoped(state, &server, "stream", args.clone())?;
                        }
                        // HTTP/2 exposes the same request stream through the
                        // owning session as well as the server. The accepted
                        // socket is the session identity retained by the
                        // host, so dispatch the event on both emitters.
                        emit_socket_scoped(state, socket, &socket_js, "stream", args)?;
                    }
                } else if !is_server
                    && fields.iter().any(|(name, value)| {
                        name.as_slice() == b":status"
                            && String::from_utf8_lossy(value)
                                .parse::<u16>()
                                .is_ok_and(|status| (100..200).contains(&status))
                    })
                {
                    // A 100 response is the request stream's dedicated
                    // `continue` event. Other informational blocks retain
                    // the generic `headers` event and never start the final
                    // response lifecycle.
                    let status = fields
                        .iter()
                        .find(|(name, _)| name.as_slice() == b":status")
                        .and_then(|(_, value)| String::from_utf8_lossy(value).parse::<u16>().ok());
                    if status == Some(100) {
                        emit_socket_scoped(state, socket, &stream, "continue", Vec::new())?;
                    } else {
                        emit_socket_scoped(
                            state,
                            socket,
                            &stream,
                            "headers",
                            vec![
                                http2_headers_value(&fields),
                                Value::Number(
                                    header_flags
                                        .get(&stream_id)
                                        .copied()
                                        .unwrap_or(frame.header.flags)
                                        as f64,
                                ),
                                http2_raw_headers_value(&fields),
                            ],
                        )?;
                    }
                } else if fresh {
                    crate::modules::http2_util::publish_http2_stream_diagnostic(
                        state,
                        &stream,
                        false,
                        crate::modules::http2_util::HTTP2_DIAG_CREATED,
                        Some(headers.clone()),
                        None,
                        None,
                    )?;
                    crate::modules::http2_util::publish_http2_stream_diagnostic(
                        state,
                        &stream,
                        false,
                        crate::modules::http2_util::HTTP2_DIAG_START,
                        Some(headers.clone()),
                        None,
                        None,
                    )?;
                    emit_socket_scoped(state, socket, &socket_js, "stream", args)?;
                } else if !is_server
                    && matches!(
                        execute::get_property(&stream, "__quenchHttp2ResponseEmitted"),
                        Value::Boolean(true)
                    )
                {
                    // A second HEADERS block on a client stream is the
                    // response's trailing headers. It is not another
                    // `response` event and must not overwrite the initial
                    // response metadata.
                    emit_socket_scoped(
                        state,
                        socket,
                        &stream,
                        "trailers",
                        vec![http2_headers_value(&fields)],
                    )?;
                } else if is_server
                    && matches!(
                        execute::get_property(&stream, "__quenchHttp2RequestHeadersEmitted"),
                        Value::Boolean(true)
                    )
                {
                    // Server-side request trailers are delivered to both the
                    // raw stream and the compatibility request view.  Update
                    // the IncomingMessage-style fields before emitting the
                    // event so listeners see the same dictionary and wire
                    // order through `trailers` and `rawTrailers`.
                    let trailers = http2_headers_value(&fields);
                    let raw_trailers = http2_raw_headers_value(&fields);
                    let request =
                        execute::get_property(&stream, "\0quench:http2-compat-request");
                    if matches!(request, Value::Object(_) | Value::ObjectAlias(_)) {
                        execute::set_property_in_place(&request, "trailers", trailers.clone());
                        execute::set_property_in_place(
                            &request,
                            "rawTrailers",
                            raw_trailers.clone(),
                        );
                    }
                    emit_socket_scoped(state, socket, &stream, "trailers", vec![trailers.clone()])?;
                    if matches!(request, Value::Object(_) | Value::ObjectAlias(_)) {
                        emit_socket_scoped(state, socket, &request, "trailers", vec![trailers])?;
                    }
                } else {
                    crate::modules::http2_util::publish_http2_stream_diagnostic(
                        state,
                        &stream,
                        false,
                        crate::modules::http2_util::HTTP2_DIAG_FINISH,
                        Some(headers.clone()),
                        Some(
                            header_flags
                                .get(&stream_id)
                                .copied()
                                .unwrap_or(frame.header.flags),
                        ),
                        None,
                    )?;
                    if !locally_reset
                        && !matches!(
                            execute::get_property(&stream, "__quenchHttp2ResponseEmitted"),
                            Value::Boolean(true)
                        )
                    {
                        execute::set_property_in_place(
                            &stream,
                            "__quenchHttp2ResponseEmitted",
                            Value::Boolean(true),
                        );
                        emit_socket_scoped(
                            state,
                            socket,
                            &stream,
                            "response",
                            vec![
                                http2_headers_value(&fields),
                                Value::Number(
                                    header_flags
                                        .get(&stream_id)
                                        .copied()
                                        .unwrap_or(frame.header.flags)
                                        as f64,
                                ),
                                http2_raw_headers_value(&fields),
                            ],
                        )?;
                        if matches!(
                            execute::get_property(&stream, "__quenchHttp2PushStream"),
                            Value::Boolean(true)
                        ) && !matches!(
                            execute::get_property(&stream, "__quenchHttp2PushEmitted"),
                            Value::Boolean(true)
                        ) {
                            execute::set_property_in_place(
                                &stream,
                                "__quenchHttp2PushEmitted",
                                Value::Boolean(true),
                            );
                            state.borrow_mut().net.pending_events.push((
                                stream.clone(),
                                "push".into(),
                                vec![headers.clone()],
                            ));
                        }
                    }
                }
                if is_server && fresh {
                    execute::set_property_in_place(
                        &stream,
                        "__quenchHttp2RequestHeadersEmitted",
                        Value::Boolean(true),
                    );
                }
                // `header_flags` also includes a later DATA frame so the
                // callback can observe Node's flags value (5).  END_STREAM
                // belongs to this HEADERS frame only when it is set on the
                // frame itself; using the aggregate here would emit a
                // duplicate `end`/`close` before DATA is dispatched.
                // A same-read RST_STREAM supersedes END_STREAM on the
                // request HEADERS. Defer close until the RST branch so the
                // peer observes the protocol error before close, matching
                // Node's stream lifecycle ordering.
                if frame.header.flags & 1 != 0 && !batch_resets.contains_key(&stream_id) {
                    emit_http2_stream_close(state, socket, &stream, is_server, true)?;
                }
            }
            crate::modules::http2_protocol::FrameType::Data => {
                let (stream, _) = http2_stream(state, &socket_js, stream_id)?;
                let locally_reset = state
                    .borrow()
                    .net
                    .http2_reset_codes
                    .contains_key(&(socket_id, stream_id));
                let paused = matches!(
                    execute::get_property(
                        &stream,
                        crate::modules::http2_util::HTTP2_STREAM_PAUSED_PROP,
                    ),
                    Value::Boolean(true)
                );
                if paused {
                    if !frame.payload.is_empty() {
                        queue_paused_http2_data(
                            state,
                            socket_id,
                            stream_id,
                            http2_data_value(&stream, &frame.payload),
                            frame.header.flags & 1 != 0,
                        );
                    } else if frame.header.flags & 1 != 0 {
                        queue_paused_http2_data(
                            state,
                            socket_id,
                            stream_id,
                            Value::Undefined,
                            true,
                        );
                    }
                    continue;
                }
                if frame.payload.is_empty() {
                    if frame.header.flags & 1 != 0 {
                        emit_http2_stream_close(state, socket, &stream, is_server, !locally_reset)?;
                    }
                    continue;
                }
                if locally_reset {
                    continue;
                }
                let data = http2_data_value(&stream, &frame.payload);
                emit_socket_scoped(state, socket, &stream, "data", vec![data.clone()])?;
                if is_server {
                    crate::modules::http2_util::emit_compat_request_data(
                        state,
                        &stream,
                        data,
                    )?;
                }
                if frame.header.flags & 1 != 0 {
                    emit_http2_stream_close(state, socket, &stream, is_server, true)?;
                }
            }
            crate::modules::http2_protocol::FrameType::RstStream => {
                let (stream_value, _) = http2_stream(state, &socket_js, stream_id)?;
                let stream = execute::canonical_value(&stream_value);
                let code = frame
                    .payload
                    .get(..4)
                    .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
                    .unwrap_or(0);
                // A peer RST can acknowledge a locally closed stream.  In
                // that case Node reports close without synthesizing a second
                // client error (notably for `stream.end(); stream.close()`).
                let locally_reset = state
                    .borrow()
                    .net
                    .http2_reset_codes
                    .contains_key(&(socket_id, stream_id));
                state
                    .borrow_mut()
                    .net
                    .http2_reset_codes
                    .insert((socket_id, stream_id), code);
                // A host-created stream can have a copy-on-write public
                // representative distinct from the transport canonical. Keep
                // the reset facts visible through both identities before the
                // close event canonicalizes its receiver.
                execute::set_property_in_place(
                    &stream_value,
                    "rstCode",
                    Value::Number(code as f64),
                );
                execute::set_property_in_place(&stream_value, "destroyed", Value::Boolean(true));
                execute::set_property_in_place(&stream, "rstCode", Value::Number(code as f64));
                execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
                // Keep the socket's public stream table in sync as well. A
                // stream event may have handed JavaScript a table value that
                // predates a copy-on-write canonical replacement.
                if let Value::Object(_) | Value::ObjectAlias(_) =
                    execute::get_property(&socket_js, "\0quench:http2-streams")
                {
                    let public = execute::get_property(
                        &execute::get_property(&socket_js, "\0quench:http2-streams"),
                        &stream_id.to_string(),
                    );
                    execute::set_property_in_place(&public, "rstCode", Value::Number(code as f64));
                    execute::set_property_in_place(&public, "destroyed", Value::Boolean(true));
                }
                // NGHTTP2_NO_ERROR is still surfaced as an aborted client
                // stream when the peer resets it before END_STREAM. CANCEL
                // remains the quiet AbortSignal path; protocol/internal reset
                // codes retain the normal stream-error diagnostic.
                let reset_error_emitted = matches!(
                    execute::get_property(&stream, "\0quenchHttp2ResetErrorEmitted"),
                    Value::Boolean(true)
                );
                let close_already_emitted = matches!(
                    execute::get_property(&stream, "__quenchHttp2CloseEmitted"),
                    Value::Boolean(true)
                );
                let clean_peer_reset_after_end = is_server
                    && code == 0
                    && matches!(
                        execute::get_property(&stream, "\0quench:http2-remote-end"),
                        Value::Boolean(true)
                    );
                if code != 8
                    && !clean_peer_reset_after_end
                    && !locally_reset
                    && !close_already_emitted
                    && !reset_error_emitted
                {
                    execute::set_property_in_place(
                        &stream,
                        "\0quenchHttp2ResetErrorEmitted",
                        Value::Boolean(true),
                    );
                    let (error_code, message) = if code == 0 {
                        (
                            "ERR_HTTP2_STREAM_ABORTED",
                            "The stream was aborted".to_owned(),
                        )
                    } else {
                        let code_name = crate::modules::http2_facts::error_name(code)
                            .map_or_else(|| code.to_string(), str::to_owned);
                        (
                            "ERR_HTTP2_STREAM_ERROR",
                            format!("Stream closed with error code {code_name}"),
                        )
                    };
                    let error = quench_runtime::builtins::error(
                        quench_runtime::ops::Builtin::Error,
                        &[Value::String(message)],
                    );
                    let error =
                        execute::set_property(error, "code", Value::String(error_code.into()));
                    emit_socket_scoped(state, socket, &stream, "error", vec![error])?;
                }
                emit_http2_stream_close(state, socket, &stream, is_server, false)?;
            }
            crate::modules::http2_protocol::FrameType::GoAway => {
                // GOAWAY carries the peer's terminal stream boundary and an
                // optional opaque diagnostic payload.  Deliver the decoded
                // wire facts on the session/socket object; this is the same
                // object exposed by `http2.connect()` and the server's
                // `session` event.
                if frame.payload.len() >= 8 {
                    let last_stream_id =
                        u32::from_be_bytes(frame.payload[0..4].try_into().unwrap()) & 0x7fff_ffff;
                    let error_code = u32::from_be_bytes(frame.payload[4..8].try_into().unwrap());
                    emit_socket_scoped(
                        state,
                        socket,
                        &socket_js,
                        "goaway",
                        vec![
                            Value::Number(error_code as f64),
                            Value::Number(last_stream_id as f64),
                            crate::modules::buffer_proto::make_buffer(&frame.payload[8..]),
                        ],
                    )?;
                }
            }
            crate::modules::http2_protocol::FrameType::AltSvc => {
                if frame.payload.len() < 2 {
                    continue;
                }
                let origin_len =
                    u16::from_be_bytes(frame.payload[..2].try_into().unwrap()) as usize;
                if frame.payload.len() < 2 + origin_len {
                    continue;
                }
                let origin_start = 2;
                let alt_start = origin_start + origin_len;
                let origin =
                    String::from_utf8_lossy(&frame.payload[origin_start..alt_start]).into_owned();
                let alt = String::from_utf8_lossy(&frame.payload[alt_start..]).into_owned();
                emit_socket_scoped(
                    state,
                    socket,
                    &socket_js,
                    "altsvc",
                    vec![
                        Value::String(alt),
                        Value::String(origin),
                        Value::Number(frame.header.stream_id as f64),
                    ],
                )?;
            }
            crate::modules::http2_protocol::FrameType::Ping => {
                let payload = frame.payload.as_slice();
                if frame.header.flags & 0x1 != 0 {
                    if !crate::modules::http2_util::complete_http2_ping(state, socket_id, payload)?
                    {
                        let error = quench_runtime::builtins::error(
                            quench_runtime::ops::Builtin::Error,
                            &[Value::String("Protocol error".into())],
                        );
                        let error = execute::set_property(
                            error,
                            "code",
                            Value::String("ERR_HTTP2_ERROR".into()),
                        );
                        crate::modules::net::socket_destroy(state, Some(&socket_js), &[error])?;
                    }
                } else if payload.len() == 8 {
                    let ack = crate::modules::http2_protocol::Frame::new(
                        crate::modules::http2_protocol::FrameType::Ping,
                        0x1,
                        0,
                        payload.to_vec(),
                    );
                    let write = execute::get_property(&socket_js, "write");
                    if quench_runtime::is_callable(&write) {
                        execute::call(
                            &write,
                            &socket_js,
                            &[crate::modules::buffer_proto::make_buffer(&ack.encode())],
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Feed bytes from an arbitrary EventEmitter/Readable transport into the
/// same HTTP/2 state machine used by polled TCP sockets. `duplexPair()` is a
/// valid `http2.connect({ createConnection })` transport but has no
/// `NetSocket` record, so this short-lived view supplies only the lifecycle
/// context required by the shared dispatcher; session and stream state remain
/// keyed by the transport's canonical net id.
pub(crate) fn dispatch_external_http2_bytes(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
    bytes: &[u8],
) -> Result<(), VmError> {
    let id = super::net_id(socket).ok_or(VmError::NotCallable)?;
    let fake = Rc::new(RefCell::new(NetSocket {
        id,
        process_scope: state.borrow().cluster.process_scope(),
        owner_worker: state.borrow().cluster.worker_context,
        stream: None,
        js: socket.clone(),
        state: SocketState::Open,
        refed: false,
        server_id: None,
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
        peer: None,
        local: None,
        encoding: None,
        decode_buf: Vec::new(),
    }));
    let frames = {
        let mut host = state.borrow_mut();
        let session = host
            .net
            .http2_sessions
            .get_mut(&id)
            .ok_or(VmError::NotCallable)?;
        session.feed(bytes).map_err(|_| VmError::NotCallable)?
    };
    dispatch_http2_frames(state, &fake, &frames)
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
        let (protocol_error, protocol_frames) = {
            let id = sock.borrow().id;
            let result = state
                .borrow_mut()
                .net
                .http2_sessions
                .get_mut(&id)
                .map(|session| session.feed(&bytes));
            match result {
                Some(Ok(frames)) => (None, frames),
                Some(Err(error)) => (Some(error), Vec::new()),
                None => (None, Vec::new()),
            }
        };
        if protocol_error.is_some() {
            // Preserve ordinary net data delivery while retaining a
            // host-owned diagnostic fact.  The eventual HTTP/2 session layer
            // can turn this fact into a protocol error/GOAWAY without having
            // to reparse the same TCP chunk.
            execute::set_property_in_place(
                &js,
                crate::modules::http2_protocol::PROTOCOL_ERROR_MARKER,
                Value::Boolean(true),
            );
            let goaway_code = if matches!(
                protocol_error,
                Some(crate::modules::http2_protocol::ProtocolError::FlowControlError)
            ) {
                3_u32 // FLOW_CONTROL_ERROR
            } else {
                1_u32 // PROTOCOL_ERROR
            };
            let mut goaway_payload = 0_u32.to_be_bytes().to_vec();
            goaway_payload.extend_from_slice(&goaway_code.to_be_bytes());
            let goaway = crate::modules::http2_protocol::Frame::new(
                crate::modules::http2_protocol::FrameType::GoAway,
                0,
                0,
                goaway_payload,
            );
            let _ = crate::modules::net::socket_write(
                state,
                Some(&js),
                &[crate::modules::buffer_proto::make_buffer(&goaway.encode())],
            );
            // A malformed connection-level frame is a session error, not a
            // recoverable net.Socket data event.  Surface Node's common
            // HTTP/2 protocol error and tear down the transport so peer-side
            // users observe EOF/close instead of waiting forever.
            let error = crate::modules::http2_util::construct_nghttp_error(
                state,
                &[Value::Undefined, Value::Number(-523.0)],
            )?;
            execute::set_property_in_place(
                &js,
                crate::modules::http2_protocol::SESSION_ERROR_PROP,
                error.clone(),
            );
            crate::modules::net::socket_destroy(state, Some(&js), &[error])?;
        }
        let is_http2 = state
            .borrow()
            .net
            .http2_sessions
            .contains_key(&sock.borrow().id)
            || matches!(
                execute::get_property(&js, crate::modules::http2_protocol::CLIENT_MARKER),
                Value::Boolean(true)
            )
            || matches!(
                execute::get_property(&js, crate::modules::http2_protocol::SERVER_MARKER),
                Value::Boolean(true)
            );
        if is_http2 && !protocol_frames.is_empty() {
            dispatch_http2_frames(state, &sock, &protocol_frames)?;
        }
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
        // HTTP/2 consumers receive decoded stream events above. Keep the
        // connection-level wire bytes private instead of exposing them as a
        // second, competing `net.Socket` data stream.
        if is_http2 {
            continue;
        }
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
    let active_worker = state.borrow().cluster.worker_context;
    {
        let host = state.borrow();
        for server in host.net.servers.values() {
            let mut guard = server.borrow_mut();
            // Worker bootstrap re-enters the shared VM synchronously. Do not
            // announce a primary-owned listener from that nested turn: its
            // callback may run before the surrounding `cluster.fork()` has
            // assigned the returned Worker object.
            if active_worker.is_some() && guard.owner_worker != active_worker {
                continue;
            }
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
        let id = sock.borrow().id;
        host.net.sockets.remove(&id);
        host.net.http2_sessions.remove(&id);
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
