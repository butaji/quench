use std::{
    collections::HashMap,
    ffi::CStr,
    io::{ErrorKind, Read, Write},
    net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket},
    sync::{Mutex, MutexGuard, OnceLock},
};
enum QuenchTcpResource {
    Listener(TcpListener),
    Stream(TcpStream),
    Udp(UdpSocket),
}
static QUENCH_TCP_RESOURCES: OnceLock<Mutex<HashMap<u32, QuenchTcpResource>>> = OnceLock::new();
static QUENCH_TCP_NEXT_ID: OnceLock<Mutex<u32>> = OnceLock::new();
fn quench_tcp_resources() -> &'static Mutex<HashMap<u32, QuenchTcpResource>> {
    QUENCH_TCP_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}
fn quench_tcp_resources_lock() -> MutexGuard<'static, HashMap<u32, QuenchTcpResource>> {
    quench_tcp_resources()
        .lock()
        .expect("tcp resource mutex poisoned")
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
    quench_tcp_resources_lock().insert(id, resource);
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
    let resources = quench_tcp_resources_lock();
    match resources.get(&id) {
        Some(QuenchTcpResource::Listener(listener)) => listener
            .local_addr()
            .map(|address| address.port())
            .map_err(|_| rquickjs::Error::new_from_js("tcp", "address failed")),
        _ => Err(rquickjs::Error::new_from_js("tcp", "not a listener")),
    }
}
pub(crate) fn quench_tcp_local_port(id: u32) -> rquickjs::Result<u16> {
    let resources = quench_tcp_resources_lock();
    match resources.get(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream
            .local_addr()
            .map(|address| address.port())
            .map_err(|_| rquickjs::Error::new_from_js("tcp", "address failed")),
        _ => Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    }
}
pub(crate) fn quench_tcp_peer_port(id: u32) -> rquickjs::Result<u16> {
    let resources = quench_tcp_resources_lock();
    match resources.get(&id) {
        Some(QuenchTcpResource::Stream(stream)) => stream
            .peer_addr()
            .map(|address| address.port())
            .map_err(|_| rquickjs::Error::new_from_js("tcp", "address failed")),
        _ => Err(rquickjs::Error::new_from_js("tcp", "not a stream")),
    }
}
pub(crate) fn quench_tcp_accept(id: u32) -> rquickjs::Result<u32> {
    let resources = quench_tcp_resources_lock();
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
    let mut resources = quench_tcp_resources_lock();
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
    let resources = quench_tcp_resources_lock();
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
    let mut resources = quench_tcp_resources_lock();
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
    let resources = quench_tcp_resources_lock();
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
pub(crate) fn quench_udp_socket(host: String, port: u16) -> rquickjs::Result<u32> {
    let socket = UdpSocket::bind((host.as_str(), port))
        .map_err(|_| rquickjs::Error::new_from_js("udp", "bind failed"))?;
    socket
        .set_nonblocking(true)
        .map_err(|_| rquickjs::Error::new_from_js("udp", "nonblocking failed"))?;
    Ok(quench_tcp_insert(QuenchTcpResource::Udp(socket)))
}
pub(crate) fn quench_udp_send(
    id: u32,
    host: String,
    port: u16,
    data: Vec<u8>,
) -> rquickjs::Result<u32> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("udp resource mutex poisoned");
    let socket = match resources.get(&id) {
        Some(QuenchTcpResource::Udp(socket)) => socket,
        _ => return Err(rquickjs::Error::new_from_js("udp", "not a socket")),
    };
    socket
        .send_to(
            &data,
            SocketAddr::new(
                host.parse()
                    .map_err(|_| rquickjs::Error::new_from_js("udp", "invalid host"))?,
                port,
            ),
        )
        .map(|length| length as u32)
        .map_err(|_| rquickjs::Error::new_from_js("udp", "send failed"))
}
pub(crate) fn quench_udp_recv(id: u32) -> rquickjs::Result<Vec<u8>> {
    let resources = quench_tcp_resources()
        .lock()
        .expect("udp resource mutex poisoned");
    let socket = match resources.get(&id) {
        Some(QuenchTcpResource::Udp(socket)) => socket,
        _ => return Err(rquickjs::Error::new_from_js("udp", "not a socket")),
    };
    let mut data = vec![0; 65_507];
    match socket.recv(&mut data) {
        Ok(length) => {
            data.truncate(length);
            Ok(data)
        }
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(Vec::new()),
        Err(_) => Err(rquickjs::Error::new_from_js("udp", "receive failed")),
    }
}
pub(crate) fn quench_dns_lookup(host: String, port: u16) -> rquickjs::Result<Vec<String>> {
    (host.as_str(), port)
        .to_socket_addrs()
        .map(|addresses| addresses.map(|address| address.ip().to_string()).collect())
        .map_err(|_| rquickjs::Error::new_from_js("dns", "lookup failed"))
}
#[allow(clippy::too_many_lines)]
pub(crate) fn quench_dns_reverse(address: String) -> rquickjs::Result<String> {
    let ip: IpAddr = address
        .parse()
        .map_err(|_| rquickjs::Error::new_from_js("dns", "invalid address"))?;
    let mut storage = unsafe { std::mem::zeroed::<libc::sockaddr_storage>() };
    let (storage_ptr, storage_len) = match ip {
        IpAddr::V4(value) => {
            let target = unsafe { &mut *(&mut storage as *mut _ as *mut libc::sockaddr_in) };
            target.sin_family = libc::AF_INET as libc::sa_family_t;
            target.sin_addr = libc::in_addr {
                s_addr: u32::from_ne_bytes(value.octets()),
            };
            (
                &storage as *const libc::sockaddr_storage as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        IpAddr::V6(value) => {
            let target = unsafe { &mut *(&mut storage as *mut _ as *mut libc::sockaddr_in6) };
            target.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            target.sin6_addr = libc::in6_addr {
                s6_addr: value.octets(),
            };
            (
                &storage as *const libc::sockaddr_storage as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    };
    let mut hostname = [0 as libc::c_char; libc::NI_MAXHOST as usize];
    let result = unsafe {
        libc::getnameinfo(
            storage_ptr,
            storage_len,
            hostname.as_mut_ptr(),
            hostname.len() as libc::socklen_t,
            std::ptr::null_mut(),
            0,
            libc::NI_NAMEREQD,
        )
    };
    if result != 0 {
        return Err(rquickjs::Error::new_from_js("dns", "reverse lookup failed"));
    }
    unsafe { CStr::from_ptr(hostname.as_ptr()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| rquickjs::Error::new_from_js("dns", "invalid hostname"))
}
include!("host_context_macro.inc");
