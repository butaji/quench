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
    let local = stream.local_addr().ok();
    let object = install_methods(object, net_info_props(peer, local))?;
    let socket = Rc::new(RefCell::new(NetSocket {
        id,
        stream: Some(stream),
        js: object.clone(),
        state: SocketState::Open,
        server_id: Some(server_id),
        write_buf: Vec::new(),
        read_eof: false,
        close_emitted: false,
        connect_announced: true,
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
        emit(state, &js, "connection", vec![object])?;
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
        let js = sock.borrow().js.clone();
        emit(state, &js, "connect", Vec::new())?;
    }
    for (sock, bytes) in events.datas {
        let (js, arg) = {
            let guard = sock.borrow();
            let arg = data_value(&guard, &bytes);
            (guard.js.clone(), arg)
        };
        emit(state, &js, "data", vec![arg])?;
    }
    for sock in events.eofs {
        let js = sock.borrow().js.clone();
        emit(state, &js, "end", Vec::new())?;
    }
    Ok(())
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
        if guard.state == SocketState::Open && !guard.connect_announced {
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
    let Some(stream) = guard.stream.as_mut() else {
        return false;
    };
    let mut had_eof = false;
    loop {
        let mut buf = [0u8; READ_CHUNK];
        match stream.read(&mut buf) {
            Ok(0) => {
                guard.read_eof = true;
                had_eof = true;
                break;
            }
            Ok(n) => datas.push((sock.clone(), buf[..n].to_vec())),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => {
                guard.read_eof = true;
                had_eof = true;
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

/// `'close'` for sockets that read EOF and drained their writes (or
/// were asked to close); the socket leaves the live set.
pub fn finalize(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let mut to_close: Vec<Rc<RefCell<NetSocket>>> = Vec::new();
    {
        let host = state.borrow();
        for sock in host.net.sockets.values() {
            let mut guard = sock.borrow_mut();
            if guard.close_emitted || guard.state == SocketState::Closed {
                continue;
            }
            let done = guard.read_eof && guard.write_buf.is_empty();
            let asked_close = guard.state == SocketState::Closing && guard.write_buf.is_empty();
            if done || asked_close {
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
    for sock in to_close {
        let js = sock.borrow().js.clone();
        emit(state, &js, "close", Vec::new())?;
    }
    Ok(())
}
