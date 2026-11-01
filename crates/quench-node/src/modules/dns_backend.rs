//! Integration draft: OS getaddrinfo backend for cares_wrap.getaddrinfo.
//! Uses the existing quench-node libc dependency; no guest JS or Node subprocess.
use std::{
    ffi::CString,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ptr,
};
#[derive(Clone, Copy, Debug)]
pub enum Family {
    Any,
    V4,
    V6,
}
#[derive(Clone, Copy, Debug)]
pub enum Order {
    Verbatim,
    V4First,
    V6First,
}
#[derive(Debug)]
pub enum ResolveError {
    InteriorNul,
    Gai {
        code: i32,
        system_errno: Option<i32>,
    },
    NoData,
}
struct AddressList(*mut libc::addrinfo);
impl Drop for AddressList {
    fn drop(&mut self) {
        unsafe { libc::freeaddrinfo(self.0) }
    }
}
/// `ascii_hostname` is the IDNA ASCII output produced by the host boundary.
/// `hints` must be validated against this platform's supported AI_* flags there.
pub fn resolve(
    ascii_hostname: &str,
    family: Family,
    hints: i32,
    order: Order,
) -> Result<Vec<IpAddr>, ResolveError> {
    let hostname = CString::new(ascii_hostname).map_err(|_| ResolveError::InteriorNul)?;
    let mut options: libc::addrinfo = unsafe { std::mem::zeroed() };
    options.ai_family = match family {
        Family::Any => libc::AF_UNSPEC,
        Family::V4 => libc::AF_INET,
        Family::V6 => libc::AF_INET6,
    };
    options.ai_socktype = libc::SOCK_STREAM;
    options.ai_flags = hints;
    let mut head = ptr::null_mut();
    let status = unsafe { libc::getaddrinfo(hostname.as_ptr(), ptr::null(), &options, &mut head) };
    if status != 0 {
        return Err(gai_error(status));
    }
    let list = AddressList(head);
    let mut cursor = list.0;
    let mut addresses = Vec::new();
    while !cursor.is_null() {
        let item = unsafe { &*cursor };
        if !item.ai_addr.is_null() {
            let address = match item.ai_family {
                libc::AF_INET
                    if item.ai_addrlen as usize >= std::mem::size_of::<libc::sockaddr_in>() =>
                {
                    let socket = unsafe { &*(item.ai_addr as *const libc::sockaddr_in) };
                    Some(IpAddr::V4(Ipv4Addr::from(
                        socket.sin_addr.s_addr.to_ne_bytes(),
                    )))
                }
                libc::AF_INET6
                    if item.ai_addrlen as usize >= std::mem::size_of::<libc::sockaddr_in6>() =>
                {
                    let socket = unsafe { &*(item.ai_addr as *const libc::sockaddr_in6) };
                    Some(IpAddr::V6(Ipv6Addr::from(socket.sin6_addr.s6_addr)))
                }
                _ => None,
            };
            if let Some(address) = address {
                addresses.push(address);
            }
        }
        cursor = item.ai_next;
    }
    // Stable partition matches Node's two passes, preserving duplicates and OS order.
    match order {
        Order::Verbatim => {}
        Order::V4First => addresses.sort_by_key(IpAddr::is_ipv6),
        Order::V6First => addresses.sort_by_key(IpAddr::is_ipv4),
    }
    if addresses.is_empty() {
        Err(ResolveError::NoData)
    } else {
        Ok(addresses)
    }
}

pub fn gai_error(code: i32) -> ResolveError {
    // Preserve errno before another system call can overwrite it.
    let system_errno = (code == libc::EAI_SYSTEM)
        .then(|| io::Error::last_os_error().raw_os_error())
        .flatten();
    ResolveError::Gai { code, system_errno }
}
