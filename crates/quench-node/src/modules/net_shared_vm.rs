use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const FIRST_TRANSPORT_ID: u64 = 1;
const TCP_READ_CHUNK: usize = 16 * 1024;
const TCP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const NET_MODULE_FACTORY: &str = r#"((connectOperation, writeOperation, endOperation, encodingOperation, listenOperation, closeOperation, addressOperation) => {
  const listeners = (socket) => socket._listeners;
  class Socket {
    constructor() { this._listeners = new Map(); this._encoding = null; }
    on(event, callback) { const list = listeners(this).get(event) || []; list.push(callback); listeners(this).set(event, list); return this; }
    addListener(event, callback) { return this.on(event, callback); }
    once(event, callback) {
      const wrapper = (...args) => { this.removeListener(event, wrapper); callback.apply(this, args); };
      wrapper.listener = callback; return this.on(event, wrapper);
    }
    removeListener(event, callback) { const list = listeners(this).get(event) || []; listeners(this).set(event, list.filter((item) => item !== callback && item.listener !== callback)); return this; }
    off(event, callback) { return this.removeListener(event, callback); }
    emit(event, ...args) { for (const callback of [...(listeners(this).get(event) || [])]) callback.apply(this, args); return true; }
    setEncoding(encoding) { this._encoding = String(encoding); encodingOperation(this, this._encoding); return this; }
    write(chunk) { writeOperation(this, typeof chunk === 'string' ? chunk : Buffer.from(chunk).toString('utf8')); return true; }
    end(chunk) { if (chunk !== undefined) this.write(chunk); endOperation(this); return this; }
    destroy() { endOperation(this); return this; }
    ref() { return this; }
    unref() { return this; }
  }
  const connect = function connect(port, host, callback) {
    if (typeof host === 'function') { callback = host; host = undefined; }
    const socket = new Socket();
    if (typeof callback === 'function') socket.once('connect', callback);
    connectOperation(socket, port, host);
    return socket;
  };
  class Server {
    constructor(listener) { this._listeners = new Map(); this.listening = false; if (typeof listener === 'function') this.on('connection', listener); }
    on(event, callback) { const list = this._listeners.get(event) || []; list.push(callback); this._listeners.set(event, list); return this; }
    addListener(event, callback) { return this.on(event, callback); }
    once(event, callback) { const wrapped = (...args) => { this.removeListener(event, wrapped); callback.apply(this, args); }; wrapped.listener = callback; return this.on(event, wrapped); }
    removeListener(event, callback) { const list = this._listeners.get(event) || []; this._listeners.set(event, list.filter((item) => item !== callback && item.listener !== callback)); return this; }
    emit(event, ...args) { for (const callback of [...(this._listeners.get(event) || [])]) callback.apply(this, args); return true; }
    listen(port, host, callback) { if (typeof host === 'function') { callback = host; host = undefined; } if (typeof callback === 'function') this.once('listening', callback); listenOperation(this, port, host); this.listening = true; return this; }
    address() { return addressOperation(this); }
    close(callback) { if (typeof callback === 'function') this.once('close', callback); closeOperation(this); this.listening = false; return this; }
  }
  const createServer = function createServer(listener) { return new Server(listener); };
  return { Socket, Server, connect, createConnection: connect, createServer };
})"#;

/// TCP resources for the shared-VM projection. Protocol modules keep only
/// endpoint IDs and parsed state; this is the one owner of socket bytes and
/// OS handles for that execution path.
pub(crate) struct Transport {
    next_id: u64,
    listeners: HashMap<u64, TcpListener>,
    sockets: HashMap<u64, SharedSocket>,
    connecting: HashMap<u64, Receiver<Result<TcpStream, String>>>,
}

struct SharedSocket {
    stream: TcpStream,
    pending_write: Vec<u8>,
    write_offset: usize,
    end_after_write: bool,
    read_ended: bool,
    write_ended: bool,
}

pub(crate) enum TransportEvent {
    Accepted {
        listener: u64,
        socket: u64,
        remote: SocketAddr,
    },
    Connected { socket: u64 },
    ConnectError { socket: u64, message: String },
    Data { socket: u64, bytes: Vec<u8> },
    End { socket: u64 },
    Closed { socket: u64 },
    Error { socket: u64, message: String },
}

impl Transport {
    pub(crate) fn new() -> Self {
        Self {
            next_id: FIRST_TRANSPORT_ID,
            listeners: HashMap::new(),
            sockets: HashMap::new(),
            connecting: HashMap::new(),
        }
    }

    fn id(&mut self) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| "shared TCP resource identifier space exhausted".to_owned())?;
        Ok(id)
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn listen(transport: &mut Transport, address: SocketAddr) -> Result<u64, String> {
    let listener = TcpListener::bind(address).map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let id = transport.id()?;
    transport.listeners.insert(id, listener);
    Ok(id)
}

pub(crate) fn connect(transport: &mut Transport, host: String, port: u16) -> Result<u64, String> {
    let id = transport.id()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("quench-node-tcp-{id}"))
        .spawn(move || {
            let result = connect_with_timeout(&host, port).and_then(|stream| {
                stream
                    .set_nonblocking(true)
                    .map(|()| stream)
                    .map_err(|error| error.to_string())
            });
            let _ = sender.send(result);
        })
        .map_err(|error| format!("cannot start TCP connect: {error}"))?;
    transport.connecting.insert(id, receiver);
    Ok(id)
}

pub(crate) fn address(transport: &Transport, listener: u64) -> Option<SocketAddr> {
    transport
        .listeners
        .get(&listener)
        .and_then(|listener| listener.local_addr().ok())
}

pub(crate) fn write(transport: &mut Transport, socket: u64, bytes: &[u8]) -> Result<(), String> {
    let stream = transport
        .sockets
        .get_mut(&socket)
        .ok_or_else(|| "shared TCP socket is not connected".to_owned())?;
    stream.pending_write.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn end(transport: &mut Transport, socket: u64) -> Result<(), String> {
    if let Some(stream) = transport.sockets.get_mut(&socket) {
        stream.end_after_write = true;
    }
    Ok(())
}

pub(crate) fn close_listener(transport: &mut Transport, listener: u64) {
    transport.listeners.remove(&listener);
}

pub(crate) fn close_socket(transport: &mut Transport, socket: u64) {
    transport.connecting.remove(&socket);
    transport.sockets.remove(&socket);
}

pub(crate) fn poll(transport: &mut Transport) -> Vec<TransportEvent> {
    let mut events = poll_connects(transport);
    events.extend(poll_accepts(transport));
    events.extend(poll_sockets(transport));
    events
}

pub(crate) fn has_work(transport: &Transport) -> bool {
    !transport.listeners.is_empty()
        || !transport.sockets.is_empty()
        || !transport.connecting.is_empty()
}

pub(crate) fn cleanup(transport: &mut Transport) {
    transport.listeners.clear();
    transport.sockets.clear();
    transport.connecting.clear();
}

pub(crate) const fn poll_interval() -> std::time::Duration {
    std::time::Duration::from_millis(1)
}

fn poll_connects(transport: &mut Transport) -> Vec<TransportEvent> {
    let mut events = Vec::new();
    let ids = transport.connecting.keys().copied().collect::<Vec<_>>();
    for id in ids {
        let Some(receiver) = transport.connecting.get(&id) else {
            continue;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                continue;
            }
            Err(TryRecvError::Disconnected) => Err("TCP connect worker stopped".to_owned()),
        };
        transport.connecting.remove(&id);
        match result {
            Ok(stream) => {
                transport.sockets.insert(
                    id,
                    SharedSocket {
                        stream,
                        pending_write: Vec::new(),
                        write_offset: 0,
                        end_after_write: false,
                        read_ended: false,
                        write_ended: false,
                    },
                );
                events.push(TransportEvent::Connected { socket: id });
            }
            Err(message) => events.push(TransportEvent::ConnectError {
                socket: id,
                message,
            }),
        }
    }
    events
}

fn poll_accepts(transport: &mut Transport) -> Vec<TransportEvent> {
    let listeners = transport
        .listeners
        .iter_mut()
        .flat_map(|(listener_id, listener)| {
            let mut accepted = Vec::new();
            loop {
                match listener.accept() {
                    Ok((stream, remote)) => accepted.push((*listener_id, stream, remote)),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
            accepted
        })
        .collect::<Vec<_>>();
    let mut events = Vec::with_capacity(listeners.len());
    for (listener, stream, remote) in listeners {
        if stream.set_nonblocking(true).is_err() {
            continue;
        }
        let Ok(socket) = transport.id() else {
            continue;
        };
        transport.sockets.insert(
            socket,
            SharedSocket {
                stream,
                pending_write: Vec::new(),
                write_offset: 0,
                end_after_write: false,
                read_ended: false,
                write_ended: false,
            },
        );
        events.push(TransportEvent::Accepted {
            listener,
            socket,
            remote,
        });
    }
    events
}

fn poll_sockets(transport: &mut Transport) -> Vec<TransportEvent> {
    let mut events = Vec::new();
    let mut terminal = Vec::new();
    let ids = transport.sockets.keys().copied().collect::<Vec<_>>();
    let mut buffer = [0; TCP_READ_CHUNK];
    for id in ids {
        let Some(socket) = transport.sockets.get_mut(&id) else {
            continue;
        };
        if socket.write_offset < socket.pending_write.len() {
            match socket
                .stream
                .write(&socket.pending_write[socket.write_offset..])
            {
                Ok(0) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        message: std::io::Error::from(std::io::ErrorKind::WriteZero).to_string(),
                    });
                    terminal.push(id);
                    continue;
                }
                Ok(written) => socket.write_offset += written,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        message: error.to_string(),
                    });
                    terminal.push(id);
                    continue;
                }
            }
            if socket.write_offset == socket.pending_write.len() {
                socket.pending_write.clear();
                socket.write_offset = 0;
            }
        }
        if socket.end_after_write && socket.pending_write.is_empty() {
            let _ = socket.stream.shutdown(std::net::Shutdown::Write);
            socket.end_after_write = false;
            socket.write_ended = true;
        }
        let mut received = Vec::new();
        let mut ended = false;
        let mut failure = None;
        while !socket.read_ended {
            match socket.stream.read(&mut buffer) {
                Ok(0) => {
                    ended = true;
                    socket.read_ended = true;
                    break;
                }
                Ok(count) => received.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    failure = Some(error.to_string());
                    break;
                }
            }
        }
        if !received.is_empty() {
            events.push(TransportEvent::Data {
                socket: id,
                bytes: received,
            });
        }
        if let Some(message) = failure {
            events.push(TransportEvent::Error {
                socket: id,
                message,
            });
            terminal.push(id);
        } else if ended {
            events.push(TransportEvent::End { socket: id });
        }
        if socket.read_ended && socket.write_ended {
            events.push(TransportEvent::Closed { socket: id });
            terminal.push(id);
        }
    }
    for id in terminal {
        transport.sockets.remove(&id);
    }
    events
}

fn connect_with_timeout(host: &str, port: u16) -> Result<TcpStream, String> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .collect::<Vec<_>>();
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    Err(last_error.unwrap_or_else(|| "host resolved to no TCP addresses".into()))
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    for (name, operation) in [
        ("isIP", "netIsIP"),
        ("isIPv4", "netIsIPv4"),
        ("isIPv6", "netIsIPv6"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        let key = context.string_rooted(name);
        if !context.set_property_rooted(module, key, function, module)? {
            return Err(RootedError::host(format!("cannot install net.{name}")));
        }
    }
    for (name, operation) in [
        (
            "getDefaultAutoSelectFamilyAttemptTimeout",
            "netGetAutoSelectFamilyAttemptTimeout",
        ),
        (
            "setDefaultAutoSelectFamilyAttemptTimeout",
            "netSetAutoSelectFamilyAttemptTimeout",
        ),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        let key = context.string_rooted(name);
        if !context.set_property_rooted(module, key, function, module)? {
            return Err(RootedError::host("cannot install shared net binding"));
        }
    }
    let connect = context.host_function(crate::host::shared_vm::operation("netConnect"))?;
    let write = context.host_function(crate::host::shared_vm::operation("netSocketWrite"))?;
    let end = context.host_function(crate::host::shared_vm::operation("netSocketEnd"))?;
    let encoding = context.host_function(crate::host::shared_vm::operation("netSocketSetEncoding"))?;
    let listen = context.host_function(crate::host::shared_vm::operation("netServerListen"))?;
    let close = context.host_function(crate::host::shared_vm::operation("netServerClose"))?;
    let address = context.host_function(crate::host::shared_vm::operation("netServerAddress"))?;
    let factory = context.evaluate_script_rooted(NET_MODULE_FACTORY, "node:net/module.js")?;
    let undefined = context.undefined();
    let surface = context.call_rooted(
        factory,
        undefined,
        &[connect, write, end, encoding, listen, close, address],
    )?;
    for name in ["Socket", "Server", "connect", "createConnection", "createServer"] {
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(surface, key)?;
        if !context.set_property_rooted(module, key, value, module)? {
            return Err(RootedError::host(format!("cannot install net.{name}")));
        }
    }
    let socket_key = context.string_rooted("Socket");
    let socket_constructor = context.get_property_rooted(surface, socket_key)?;
    let retained = context.retain(socket_constructor)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_socket_constructor = Some(retained);
    Ok(module)
}

pub(crate) fn is_ip(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().and_then(|root| context.string_text(root).ok().flatten());
    let family = input.as_deref().and_then(|input| input.parse::<IpAddr>().ok());
    Ok(context.number(match family {
        Some(IpAddr::V4(_)) => 4.0,
        Some(IpAddr::V6(_)) => 6.0,
        None => 0.0,
    }))
}

pub(crate) fn is_ipv4(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().and_then(|root| context.string_text(root).ok().flatten());
    let is_ipv4 = input.is_some_and(|input| input.parse::<Ipv4Addr>().is_ok());
    Ok(context.boolean(is_ipv4))
}

pub(crate) fn is_ipv6(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().and_then(|root| context.string_text(root).ok().flatten());
    let is_ipv6 = input.is_some_and(|input| input.parse::<Ipv6Addr>().is_ok());
    Ok(context.boolean(is_ipv6))
}

pub(crate) fn connect_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.connect requires a socket"))?;
    let port = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|port| port.fract() == 0.0 && (1.0..=65535.0).contains(port))
        .ok_or_else(|| RootedError::host("invalid TCP port"))? as u16;
    let host = match args.get(2).copied() {
        Some(host) => context.string_text(host)?.unwrap_or_else(|| "localhost".to_owned()),
        None => "localhost".to_owned(),
    };
    let shared = context.host_mut().shared_state();
    let id = crate::modules::net_shared_vm::connect(&mut shared.borrow_mut().tcp, host, port)
        .map_err(RootedError::host)?;
    let key = context.string_rooted("__quenchNetSocketId");
    let id_value = context.number(id as f64);
    if !context.set_property_rooted(socket, key, id_value, socket)? {
        return Err(RootedError::host("cannot tag net socket"));
    }
    let retained = context.retain(socket)?;
    shared.borrow_mut().net_sockets.insert(
        id,
        crate::host::node_host::NetSocket {
            root: retained,
            encoding: None,
            parent_server: None,
        },
    );
    Ok(socket)
}

pub(crate) fn write_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Socket.write requires a socket"))?;
    let id = socket_id(context, socket)?;
    let bytes = args
        .get(1)
        .copied()
        .map(|value| context.to_string(value))
        .transpose()?
        .unwrap_or_default();
    context
        .host_mut()
        .net_pending_writes
        .push((id, bytes.into_bytes()));
    Ok(context.boolean(true))
}

pub(crate) fn end_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Socket.end requires a socket"))?;
    let id = socket_id(context, socket)?;
    context.host_mut().net_pending_ends.push(id);
    Ok(socket)
}

pub(crate) fn set_encoding_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Socket.setEncoding requires a socket"))?;
    let id = socket_id(context, socket)?;
    let encoding = args
        .get(1)
        .copied()
        .map(|value| context.to_string(value))
        .transpose()?;
    if let Some(socket) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_sockets
        .get_mut(&id)
    {
        socket.encoding = encoding;
    }
    Ok(context.undefined())
}

pub(crate) fn server_listen_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Server.listen requires a server"))?;
    let port = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|port| port.fract() == 0.0 && (0.0..=65535.0).contains(port))
        .ok_or_else(|| RootedError::host("invalid TCP port"))? as u16;
    let host = match args.get(2).copied() {
        Some(host) => context.string_text(host)?.unwrap_or_else(|| "127.0.0.1".to_owned()),
        None => "127.0.0.1".to_owned(),
    };
    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| RootedError::host(error.to_string()))?
        .next()
        .ok_or_else(|| RootedError::host("net.Server.listen host did not resolve"))?;
    let shared = context.host_mut().shared_state();
    let listener = listen(&mut shared.borrow_mut().tcp, address).map_err(RootedError::host)?;
    let key = context.string_rooted("__quenchNetServerId");
    let value = context.number(listener as f64);
    if !context.set_property_rooted(server, key, value, server)? {
        return Err(RootedError::host("cannot tag net server"));
    }
    let retained = context.retain(server)?;
    shared.borrow_mut().net_servers.insert(
        listener,
        crate::host::node_host::NetServer {
            root: retained,
            listener,
            connections: Default::default(),
            closing: false,
            listening_pending: true,
        },
    );
    Ok(server)
}

pub(crate) fn server_close_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Server.close requires a server"))?;
    let id = server_id(context, server)?;
    let shared = context.host_mut().shared_state();
    {
        let mut state = shared.borrow_mut();
        if let Some(listener) = state.net_servers.get_mut(&id).map(|server| {
            server.closing = true;
            server.listener
        }) {
            close_listener(&mut state.tcp, listener);
        }
    }
    Ok(server)
}

pub(crate) fn server_address_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Server.address requires a server"))?;
    let id = server_id(context, server)?;
    let shared = context.host_mut().shared_state();
    let address = address(&shared.borrow().tcp, id);
    let Some(address) = address else {
        return Ok(context.null());
    };
    let object = context.object_rooted()?;
    let address_value = context.string_rooted(&address.ip().to_string());
    let key = context.string_rooted("address");
    context.set_property_rooted(object, key, address_value, object)?;
    let family = context.string_rooted(if address.ip().is_ipv4() { "IPv4" } else { "IPv6" });
    let key = context.string_rooted("family");
    context.set_property_rooted(object, key, family, object)?;
    let port = context.number(address.port() as f64);
    let key = context.string_rooted("port");
    context.set_property_rooted(object, key, port, object)?;
    Ok(object)
}

fn server_id(context: &mut NativeContext<'_, NodeHost>, server: RootId) -> Result<u64, RootedError> {
    let key = context.string_rooted("__quenchNetServerId");
    let value = context.get_property_rooted(server, key)?;
    context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("net server identifier is invalid"))
}

fn socket_id(context: &mut NativeContext<'_, NodeHost>, socket: RootId) -> Result<u64, RootedError> {
    let key = context.string_rooted("__quenchNetSocketId");
    let value = context.get_property_rooted(socket, key)?;
    context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("net socket identifier is invalid"))
}

pub(crate) fn get_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let timeout = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_auto_select_family_attempt_timeout;
    Ok(context.number(timeout as f64))
}

pub(crate) fn set_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().and_then(|arg| {
        context
            .rooted_value(arg)
            .and_then(|value| value.as_number())
    });
    let Some(value) =
        value.filter(|value| value.is_finite() && value.fract() == 0.0 && *value > 0.0)
    else {
        let error = context.error_rooted("The \"ms\" argument must be a positive integer")?;
        let name = context.string_rooted("name");
        let range_error = context.string_rooted("RangeError");
        if !context.set_property_rooted(error, name, range_error, error)? {
            return Err(RootedError::host("cannot set net timeout error name"));
        }
        let code = context.string_rooted("code");
        let code_value = context.string_rooted("ERR_OUT_OF_RANGE");
        if !context.set_property_rooted(error, code, code_value, error)? {
            return Err(RootedError::host("cannot set net timeout error code"));
        }
        return Err(context.throw(error));
    };
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_auto_select_family_attempt_timeout =
        crate::modules::net_config::normalize_auto_select_family_attempt_timeout(value as u64);
    Ok(context.undefined())
}
