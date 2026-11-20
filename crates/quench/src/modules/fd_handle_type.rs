//! OS descriptor classification: Node node_util.cc GetUVHandleTypeCode and
//! libuv unix/tty.c uv_guess_handle. Numeric values are the native binding ABI.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Kind {
    Tcp,
    Tty,
    Udp,
    File,
    Pipe,
    Unknown,
}
#[cfg(unix)]
pub(crate) fn guess(fd: i32) -> Kind {
    if fd < 0 {
        return Kind::Unknown;
    }
    if unsafe { libc::isatty(fd) } != 0 {
        return Kind::Tty;
    }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Kind::Unknown;
    }
    let stat = unsafe { stat.assume_init() };
    match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG | libc::S_IFCHR => return Kind::File,
        libc::S_IFIFO => return Kind::Pipe,
        libc::S_IFSOCK => {}
        _ => return Kind::Unknown,
    }
    let mut address = std::mem::MaybeUninit::<libc::sockaddr_storage>::zeroed();
    let mut length = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    if unsafe { libc::getsockname(fd, address.as_mut_ptr().cast(), &mut length) } != 0 {
        return Kind::Unknown;
    }
    let address = unsafe { address.assume_init() };
    let mut kind = 0i32;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut i32).cast(),
            &mut length,
        )
    } != 0
    {
        return Kind::Unknown;
    }
    match (kind, address.ss_family as i32) {
        (libc::SOCK_DGRAM, libc::AF_INET | libc::AF_INET6) => Kind::Udp,
        (libc::SOCK_STREAM, libc::AF_INET | libc::AF_INET6) => Kind::Tcp,
        (libc::SOCK_STREAM, libc::AF_UNIX) => Kind::Pipe,
        _ => Kind::Unknown,
    }
}
#[cfg(not(unix))]
pub(crate) fn guess(_: i32) -> Kind {
    Kind::Unknown
}
