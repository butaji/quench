//! Pre-listen ownership; a descriptor is never duplicated across lifecycle maps.
use std::io;
use std::net::{SocketAddr, TcpListener};

pub(crate) const IPV6_ONLY: u32 = 1;
pub(crate) const REUSE_PORT: u32 = 2;

pub(crate) struct TcpOwned {
    socket: socket2::Socket,
    phase: Phase,
    pub(crate) referenced: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Unbound,
    Bound,
}
impl TcpOwned {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(Self {
            socket: Self::socket(false)?,
            phase: Phase::Unbound,
            referenced: true,
        })
    }
    #[cfg(unix)]
    pub(crate) fn from_owned_descriptor(fd: std::os::fd::OwnedFd) -> io::Result<Self> {
        let socket = socket2::Socket::from(fd);
        socket.set_nonblocking(true)?;
        let address = socket
            .local_addr()?
            .as_socket()
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
        let phase = if address.port() == 0 {
            Phase::Unbound
        } else {
            Phase::Bound
        };
        Ok(Self {
            socket,
            phase,
            referenced: true,
        })
    }
    fn socket(ipv6: bool) -> io::Result<socket2::Socket> {
        let socket = socket2::Socket::new(
            if ipv6 {
                socket2::Domain::IPV6
            } else {
                socket2::Domain::IPV4
            },
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )?;
        socket.set_nonblocking(true)?;
        Ok(socket)
    }
    pub(crate) fn bind(&mut self, address: SocketAddr, flags: u32) -> io::Result<()> {
        if self.phase != Phase::Unbound {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        if flags & !(IPV6_ONLY | REUSE_PORT) != 0 {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        // Match libuv's lazy family selection while retaining genuine RAII
        // ownership even before bind. Failed creation leaves prior owner intact.
        if self.socket.local_addr()?.is_ipv6() != address.is_ipv6() {
            self.socket = Self::socket(address.is_ipv6())?;
        }
        if address.is_ipv6() {
            self.socket.set_only_v6(flags & IPV6_ONLY != 0)?;
        }
        #[cfg(unix)]
        self.socket.set_reuse_address(true)?;
        if flags & REUSE_PORT != 0 {
            #[cfg(any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "aix"
            ))]
            self.socket.set_reuse_port(true)?;
            #[cfg(not(any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "aix"
            )))]
            return Err(io::Error::from_raw_os_error(libc::ENOTSUP));
        }
        self.socket.bind(&address.into())?;
        self.phase = Phase::Bound;
        Ok(())
    }
    #[cfg(unix)]
    pub(crate) fn fd(&self) -> i32 {
        use std::os::fd::AsRawFd;
        if self.phase == Phase::Unbound {
            -libc::EBADF
        } else {
            self.socket.as_raw_fd()
        }
    }
    pub(crate) fn address(&self) -> io::Result<SocketAddr> {
        if self.phase == Phase::Unbound {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        self.socket
            .local_addr()?
            .as_socket()
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))
    }
    // Failed listen leaves descriptor ownership unchanged and retryable.
    pub(crate) fn start_listening(&self, backlog: i32) -> io::Result<()> {
        self.socket.listen(backlog)
    }
    pub(crate) fn into_listener(self) -> TcpListener {
        self.socket.into()
    }
    pub(crate) fn start_connect(&mut self, address: SocketAddr) -> io::Result<()> {
        if self.socket.local_addr()?.is_ipv6() != address.is_ipv6() {
            if self.phase == Phase::Bound {
                return Err(io::Error::from_raw_os_error(libc::EINVAL));
            }
            self.socket = Self::socket(address.is_ipv6())?;
        }
        match self.socket.connect(&address.into()) {
            Ok(()) => {}
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || matches!(
                        error.raw_os_error(),
                        Some(libc::EINPROGRESS) | Some(libc::EALREADY)
                    ) => {}
            Err(error) => return Err(error),
        }
        // A successful connect submission implicitly binds the actual OS socket.
        self.phase = Phase::Bound;
        Ok(())
    }
    pub(crate) fn connect_ready(&self) -> io::Result<bool> {
        if let Some(error) = self.socket.take_error()? {
            return Err(error);
        }
        match self.socket.peer_addr() {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotConnected => Ok(false),
            Err(error) => Err(error),
        }
    }
    pub(crate) fn into_stream(self) -> std::net::TcpStream {
        self.socket.into()
    }
}
