use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const FIRST_TRANSPORT_ID: u64 = 1;
const TCP_READ_CHUNK: usize = 16 * 1024;
const TCP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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
}

pub(crate) enum TransportEvent {
    Accepted { listener: u64, socket: u64 },
    Connected { socket: u64 },
    ConnectError { socket: u64, message: String },
    Data { socket: u64, bytes: Vec<u8> },
    End { socket: u64 },
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
                    Ok((stream, _)) => accepted.push((*listener_id, stream)),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
            accepted
        })
        .collect::<Vec<_>>();
    let mut events = Vec::with_capacity(listeners.len());
    for (listener, stream) in listeners {
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
            },
        );
        events.push(TransportEvent::Accepted { listener, socket });
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
        let mut received = Vec::new();
        let mut ended = false;
        let mut failure = None;
        loop {
            match socket.stream.read(&mut buffer) {
                Ok(0) => {
                    ended = true;
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
