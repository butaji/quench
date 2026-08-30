//! `net` event-loop polling: accepts, socket reads/writes, and close
//! finalization, driven once per pump tick. Emits only after releasing
//! host borrows so JS callbacks can safely mutate sockets.

use std::io::Read;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::rc::Rc;

use quench_runtime::execute::VmError;

use crate::host::HostState;

use super::*;

/// Poll every server and socket once: accept connections, announce
/// connects, read available bytes, flush writes, and finalize closes.
pub fn poll(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let events = std::mem::take(&mut state.borrow_mut().net.pending_events);
    for (receiver, event, args) in events {
        emit(state, &receiver, &event, args)?;
    }
    let errors = std::mem::take(&mut state.borrow_mut().net.pending_errors);
    for (receiver, error) in errors {
        emit(state, &receiver, "error", vec![error])?;
    }
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
    let (object, id) = new_net_object(state, socket_props())?;
    let object = install_socket_counters(object)?;
    let local = stream.local_addr().ok();
    let object = install_methods(object, net_info_props(peer, local))?;
    set_socket_state(&object, false, false, "open");
    let socket = Rc::new(RefCell::new(NetSocket {
        id,
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
        finish_emitted: false,
        connect_announced: true,
        peer: Some(peer),
        local,
        encoding: None,
    }));
    state.borrow_mut().net.sockets.insert(id, socket);
    let server_js = state
        .borrow()
        .net
        .servers
        .get(&server_id)
        .map(|server| server.borrow().js.clone());
    if let Some(js) = server_js {
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
        if let Some((worker_id, worker)) = owner {
            let previous = state.borrow().cluster.worker_context;
            crate::modules::cluster::set_worker_mode(state, worker_id, &worker, true);
            state.borrow_mut().cluster.worker_context = Some(worker_id);
            let result = emit(state, &js, "connection", vec![object]);
            state.borrow_mut().cluster.worker_context = previous;
            crate::modules::cluster::set_worker_mode(state, worker_id, &worker, false);
            result?;
        } else {
            emit(state, &js, "connection", vec![object])?;
        }
    }
    Ok(())
}

/// Queued per-tick socket events, gathered without emitting.
struct SocketEvents {
    connects: Vec<Rc<RefCell<NetSocket>>>,
    datas: Vec<(Rc<RefCell<NetSocket>>, Vec<u8>)>,
    eofs: Vec<Rc<RefCell<NetSocket>>>,
}

fn poll_sockets(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let events = read_sockets(state);
    for sock in events.connects {
        let (js, peer, local) = {
            let guard = sock.borrow();
            (guard.js.clone(), guard.peer, guard.local)
        };
        if let (Some(peer), Some(local)) = (peer, local) {
            install_methods(js.clone(), net_info_props(peer, Some(local)))?;
        }
        set_socket_state(&js, false, false, "open");
        crate::modules::http_client::apply_deferred_request_timeout(state, &js)?;
        emit(state, &js, "connect", Vec::new())?;
    }
    for (sock, bytes) in events.datas {
        let js = sock.borrow().js.clone();
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
            let guard = sock.borrow();
            data_value(&guard, &bytes)
        };
        let callback = execute::get_property(&js, ONREAD_CALLBACK_PROP);
        if quench_runtime::is_callable(&callback) {
            let consumed = emit_onread(&js, &bytes, &callback)?;
            if consumed < bytes.len() {
                // A callback may pause the socket. Retain unread bytes until
                // resume makes the same onread source available again.
                sock.borrow_mut().read_buf.extend_from_slice(&bytes[consumed..]);
            }
        } else {
            emit(state, &js, "data", vec![arg])?;
        }
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
        emit(state, &js, "end", Vec::new())?;
    }
    Ok(())
}

fn emit_onread(
    socket: &Value,
    bytes: &[u8],
    callback: &Value,
) -> Result<usize, VmError> {
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
        let result = execute::call(
            callback,
            socket,
            &[Value::Number(count as f64), buffer],
        )?;
        offset += count;
        if matches!(result, Value::Boolean(false))
            || matches!(execute::get_property(socket, ONREAD_PAUSED_PROP), Value::Boolean(true))
        {
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
        if matches!(execute::get_property(&guard.js, ONREAD_PAUSED_PROP), Value::Boolean(true)) {
            continue;
        }
        if !guard.read_buf.is_empty() {
            let pending = std::mem::take(&mut guard.read_buf);
            events.datas.push((sock.clone(), pending));
            continue;
        }
        if guard.read_eof
            && matches!(execute::get_property(&guard.js, ONREAD_EOF_PROP), Value::Boolean(true))
        {
            execute::set_property_in_place(&guard.js, ONREAD_EOF_PROP, Value::Boolean(false));
            events.eofs.push(sock.clone());
            continue;
        }
        if guard.state != SocketState::Closed && !guard.connect_announced {
            guard.connect_announced = true;
            events.connects.push(sock.clone());
        }
        if read_available(sock, &mut guard, &mut events.datas) {
            events.eofs.push(sock.clone());
        }
        try_flush(&mut guard);
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
                update_socket_counters(&guard);
                datas.push((sock.clone(), buf[..n].to_vec()));
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
        emit(state, &js, "listening", Vec::new())?;
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
    let mut host = state.borrow_mut();
    for server in &closed {
        host.net.servers.remove(&server.borrow().id);
    }
    drop(host);
    for server in closed {
        let js = server.borrow().js.clone();
        emit(state, &js, "close", Vec::new())?;
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
    for js in to_finish {
        emit(state, &js, "finish", Vec::new())?;
    }
    for sock in to_close {
        let js = sock.borrow().js.clone();
        set_socket_state(&js, true, false, "closed");
        super::replace_socket_property(&js, "pending", Value::Boolean(true));
        crate::modules::http::connection_close(state, &js)?;
        emit(state, &js, "close", Vec::new())?;
    }
    Ok(())
}
