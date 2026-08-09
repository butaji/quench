use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{Mutex, OnceLock},
};

enum QuenchTcpResource {
    Listener(TcpListener),
    Stream(TcpStream),
}

static QUENCH_TCP_RESOURCES: OnceLock<Mutex<HashMap<u32, QuenchTcpResource>>> = OnceLock::new();
static QUENCH_TCP_NEXT_ID: OnceLock<Mutex<u32>> = OnceLock::new();

fn quench_tcp_resources() -> &'static Mutex<HashMap<u32, QuenchTcpResource>> {
    QUENCH_TCP_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn quench_tcp_id() -> u32 {
    let mut next = QUENCH_TCP_NEXT_ID
        .get_or_init(|| Mutex::new(1))
        .lock()
        .expect("tcp id mutex poisoned");
    let id = *next;
    *next = next.wrapping_add(1).max(1);
    id
}

fn quench_tcp_insert(resource: QuenchTcpResource) -> u32 {
    let id = quench_tcp_id();
    quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned")
        .insert(id, resource);
    id
}

pub(crate) fn quench_tcp_bind(host: String, port: u16) -> rquickjs::Result<u32> {
    let listener = TcpListener::bind((host.as_str(), port))
        .map_err(|_| rquickjs::Error::new_from_js("tcp", "bind failed"))?;
    listener
        .set_nonblocking(true)
        .map_err(|_| rquickjs::Error::new_from_js("tcp", "nonblocking failed"))?;
    Ok(quench_tcp_insert(QuenchTcpResource::Listener(listener)))
}

pub(crate) fn quench_tcp_bound_port(id: u32) -> rquickjs::Result<u16> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    match resources.get(&id) {
        Some(QuenchTcpResource::Listener(listener)) => listener
            .local_addr()
            .map(|address| address.port())
            .map_err(|_| rquickjs::Error::new_from_js("tcp", "address failed")),
        _ => Err(rquickjs::Error::new_from_js("tcp", "not a listener")),
    }
}

pub(crate) fn quench_tcp_local_port(id: u32) -> rquickjs::Result<u16> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    match resources.get(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream
            .local_addr()
            .map(|address| address.port())
            .map_err(|_| rquickjs::Error::new_from_js("tcp", "address failed")),
        _ => Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    }
}

pub(crate) fn quench_tcp_peer_port(id: u32) -> rquickjs::Result<u16> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    match resources.get(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream
            .peer_addr()
            .map(|address| address.port())
            .map_err(|_| rquickjs::Error::new_from_js("tcp", "address failed")),
        _ => Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    }
}

pub(crate) fn quench_tcp_accept(id: u32) -> rquickjs::Result<u32> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    let listener = match resources.get(&id) {
        Some(QuenchTcpResource::Listener(listener)) => listener,
        _ => return Err(rquickjs::Error::new_from_js("tcp", "not a listener")),
    };
    match listener.accept() {
        Ok((stream, _)) => {
            stream
                .set_nonblocking(true)
                .map_err(|_| rquickjs::Error::new_from_js("tcp", "nonblocking failed"))?;
            drop(resources);
            Ok(quench_tcp_insert(QuenchTcpResource::Stream(stream)))
        }
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(0),
        Err(_) => Err(rquickjs::Error::new_from_js("tcp", "accept failed")),
    }
}

pub(crate) fn quench_tcp_connect(host: String, port: u16) -> rquickjs::Result<u32> {
    let stream = TcpStream::connect((host.as_str(), port))
        .map_err(|_| rquickjs::Error::new_from_js("tcp", "connect failed"))?;
    stream
        .set_nonblocking(true)
        .map_err(|_| rquickjs::Error::new_from_js("tcp", "nonblocking failed"))?;
    Ok(quench_tcp_insert(QuenchTcpResource::Stream(stream)))
}

pub(crate) fn quench_tcp_read(id: u32) -> rquickjs::Result<Vec<u8>> {
    let mut resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    let stream = match resources.get_mut(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream,
        _ => return Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    };
    let mut buffer = vec![0; 64 * 1024];
    match stream.read(&mut buffer) {
        Ok(length) => {
            buffer.truncate(length);
            Ok(buffer)
        }
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(Vec::new()),
        Err(_) => Err(rquickjs::Error::new_from_js("tcp", "read failed")),
    }
}

pub(crate) fn quench_tcp_readable(id: u32) -> rquickjs::Result<i32> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    let stream = match resources.get(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream,
        _ => return Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    };
    let mut byte = [0; 1];
    match stream.peek(&mut byte) {
        Ok(0) => Ok(2),
        Ok(_) => Ok(1),
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(0),
        Err(_) => Err(rquickjs::Error::new_from_js("tcp", "peek failed")),
    }
}

pub(crate) fn quench_tcp_write(id: u32, data: Vec<u8>) -> rquickjs::Result<u32> {
    let mut resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    let stream = match resources.get_mut(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream,
        _ => return Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    };
    stream
        .write(&data)
        .map(|length| length as u32)
        .map_err(|_| rquickjs::Error::new_from_js("tcp", "write failed"))
}

pub(crate) fn quench_tcp_shutdown(id: u32) -> rquickjs::Result<()> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned");
    let stream = match resources.get(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream,
        _ => return Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    };
    stream
        .shutdown(Shutdown::Write)
        .map_err(|_| rquickjs::Error::new_from_js("tcp", "shutdown failed"))
}

pub(crate) fn quench_tcp_close(id: u32) {
    quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned")
        .remove(&id);
}

include!("host_context_macro.inc");
