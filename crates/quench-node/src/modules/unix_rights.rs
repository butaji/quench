//! Unix IPC ancillary transport. No guest-visible descriptor is fabricated.
//! A queued send owns a duplicate solely to pin kernel identity until sendmsg.
//! Received descriptors are new kernel-owned references; each has one owner.
#![cfg(unix)]
use std::io;
use std::mem::{size_of, zeroed};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};

// Node's write-with-handle protocol carries one handle per write.
const HANDLES_PER_WRITE: usize = 1;
const CONTROL_BYTES: usize =
    unsafe { libc::CMSG_SPACE((HANDLES_PER_WRITE * size_of::<RawFd>()) as u32) as usize };
const CONTROL_WORDS: usize = CONTROL_BYTES.div_ceil(size_of::<usize>());

pub(crate) struct PendingHandle {
    descriptor: Option<OwnedFd>,
}
impl PendingHandle {
    pub(crate) fn from_optional(descriptor: Option<OwnedFd>) -> Self {
        Self { descriptor }
    }
    pub(crate) fn send(&mut self, socket: BorrowedFd<'_>, payload: &[u8]) -> io::Result<usize> {
        if payload.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "SCM_RIGHTS requires a payload byte",
            ));
        }
        let mut iovec = libc::iovec {
            iov_base: payload.as_ptr().cast_mut().cast(),
            iov_len: payload.len(),
        };
        let mut control = [0usize; CONTROL_WORDS];
        let mut message: libc::msghdr = unsafe { zeroed() };
        message.msg_iov = &mut iovec;
        message.msg_iovlen = 1;
        if let Some(fd) = self.descriptor.as_ref() {
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = CONTROL_BYTES as _;
            unsafe {
                let header = libc::CMSG_FIRSTHDR(&message);
                (*header).cmsg_level = libc::SOL_SOCKET;
                (*header).cmsg_type = libc::SCM_RIGHTS;
                (*header).cmsg_len = libc::CMSG_LEN(size_of::<RawFd>() as u32) as _;
                std::ptr::write_unaligned(libc::CMSG_DATA(header).cast::<RawFd>(), fd.as_raw_fd());
            }
        }
        let sent = unsafe {
            libc::sendmsg(
                socket.as_raw_fd(),
                &message,
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            )
        };
        if sent < 0 {
            return Err(io::Error::last_os_error());
        }
        // Ancillary ownership crosses the kernel boundary with the first byte.
        // Partial payload completion must NEVER resend the descriptor.
        if sent > 0 {
            self.descriptor.take();
        }
        Ok(sent as usize)
    }
}

pub(crate) struct Received {
    pub(crate) bytes: usize,
    pub(crate) descriptor: Option<OwnedFd>,
}

pub(crate) fn receive(socket: BorrowedFd<'_>, payload: &mut [u8]) -> io::Result<Received> {
    if payload.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty IPC receive buffer",
        ));
    }
    let mut iovec = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut control = [0usize; CONTROL_WORDS];
    let mut message: libc::msghdr = unsafe { zeroed() };
    message.msg_iov = &mut iovec;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = CONTROL_BYTES as _;
    #[cfg(target_os = "linux")]
    let cloexec = libc::MSG_CMSG_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let cloexec = 0;
    let received = unsafe {
        libc::recvmsg(
            socket.as_raw_fd(),
            &mut message,
            libc::MSG_DONTWAIT | cloexec,
        )
    };
    if received < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut owned = Vec::<OwnedFd>::new();
    let mut malformed = false;
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            let length = (*header).cmsg_len as usize;
            let base = libc::CMSG_LEN(0) as usize;
            if length < base {
                malformed = true;
                break;
            }
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let bytes = length - base;
                malformed |= bytes % size_of::<RawFd>() != 0;
                for index in 0..bytes / size_of::<RawFd>() {
                    let fd = std::ptr::read_unaligned(
                        libc::CMSG_DATA(header).cast::<RawFd>().add(index),
                    );
                    // Take ownership immediately, including on malformed/truncated messages.
                    owned.push(OwnedFd::from_raw_fd(fd));
                }
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
    }
    if malformed || message.msg_flags & libc::MSG_CTRUNC != 0 || owned.len() > HANDLES_PER_WRITE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated or invalid IPC descriptor control message",
        ));
    }
    #[cfg(not(target_os = "linux"))]
    for fd in &owned {
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(Received {
        bytes: received as usize,
        descriptor: owned.pop(),
    })
}

pub(crate) fn socket_type(fd: &std::os::fd::OwnedFd) -> std::io::Result<libc::c_int> {
    use std::os::fd::AsRawFd;
    let mut kind: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut length,
        )
    } < 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(kind)
    }
}
