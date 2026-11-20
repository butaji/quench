//! Actual OS reverse-name/service lookup, as uv_getnameinfo with NI_NAMEREQD.
use super::backend::ResolveError;
use std::{ffi::CStr, net::IpAddr};
#[derive(Debug, PartialEq, Eq)]
pub struct NameInfo {
    pub hostname: String,
    pub service: String,
}
// Owned by Node deps/uv/include/uv/unix.h: NI_MAXSERV fallback. libc omits it on glibc.
const UV_NI_MAXSERV: usize = 32;
pub fn lookup(address: IpAddr, port: u16) -> Result<NameInfo, ResolveError> {
    let mut host = [0 as libc::c_char; libc::NI_MAXHOST as usize];
    let mut service = [0 as libc::c_char; UV_NI_MAXSERV];
    let mut v4: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    let mut v6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
    let (address, length) = match address {
        IpAddr::V4(address) => {
            v4.sin_family = libc::AF_INET as libc::sa_family_t;
            v4.sin_port = port.to_be();
            v4.sin_addr.s_addr = u32::from_ne_bytes(address.octets());
            (
                &v4 as *const libc::sockaddr_in as *const libc::sockaddr,
                std::mem::size_of_val(&v4),
            )
        }
        IpAddr::V6(address) => {
            v6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            v6.sin6_port = port.to_be();
            v6.sin6_addr.s6_addr = address.octets();
            (
                &v6 as *const libc::sockaddr_in6 as *const libc::sockaddr,
                std::mem::size_of_val(&v6),
            )
        }
    };
    let status = unsafe {
        libc::getnameinfo(
            address,
            length as libc::socklen_t,
            host.as_mut_ptr(),
            host.len() as libc::socklen_t,
            service.as_mut_ptr(),
            service.len() as libc::socklen_t,
            libc::NI_NAMEREQD,
        )
    };
    if status != 0 {
        return Err(super::backend::gai_error(status));
    }
    // cares_wrap uses OneByteString, not UTF-8 decoding. Retain byte-to-codepoint semantics.
    let one_byte = |buffer: &[libc::c_char]| {
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_bytes()
            .iter()
            .map(|byte| char::from(*byte))
            .collect()
    };
    Ok(NameInfo {
        hostname: one_byte(&host),
        service: one_byte(&service),
    })
}
