//! Linux glibc translation: symbol mapping owned by vendored libuv getaddrinfo.c.
//! UV numbers come from the existing generated authoritative UV_ERRORS table.
use super::backend::ResolveError;
mod native {
    include!("dns_native_eai.rs");
}
fn uv(name: &str) -> i32 {
    crate::modules::util_system_error_shared_vm::system_error_number(name)
        .expect("libuv generated table contains EAI status")
}
pub fn status(error: ResolveError) -> i32 {
    match error {
        ResolveError::InteriorNul => uv("EINVAL"),
        ResolveError::NoData => uv("EAI_NODATA"),
        ResolveError::Gai { code, system_errno } => {
            if code == libc::EAI_SYSTEM {
                return -system_errno.expect("EAI_SYSTEM preserves errno");
            }
            let name = match code {
                native::EAI_ADDRFAMILY => "EAI_ADDRFAMILY",
                native::EAI_AGAIN => "EAI_AGAIN",
                native::EAI_BADFLAGS => "EAI_BADFLAGS",
                native::EAI_CANCELED => "EAI_CANCELED",
                native::EAI_FAIL => "EAI_FAIL",
                native::EAI_FAMILY => "EAI_FAMILY",
                native::EAI_MEMORY => "EAI_MEMORY",
                native::EAI_NODATA => "EAI_NODATA",
                native::EAI_NONAME => "EAI_NONAME",
                native::EAI_OVERFLOW => "EAI_OVERFLOW",
                native::EAI_SERVICE => "EAI_SERVICE",
                native::EAI_SOCKTYPE => "EAI_SOCKTYPE",
                other => panic!("unknown platform EAI code {other}"),
            };
            uv(name)
        }
    }
}
