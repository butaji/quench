//! Connect failures retain their OS cause and destination until guest projection.
use std::net::SocketAddr;

#[derive(Debug)]
pub(crate) enum TcpConnectFailure {
    Connect {
        error: std::io::Error,
        address: SocketAddr,
    },
    Resolve {
        error: std::io::Error,
        hostname: String,
    },
    Setup(std::io::Error),
    WorkerStopped,
    EmptyDestinations,
}

impl TcpConnectFailure {
    pub(crate) fn errno(&self) -> Option<f64> {
        match self {
            Self::Connect { error, .. } | Self::Setup(error) => {
                error.raw_os_error().map(|errno| -f64::from(errno))
            }
            // std's resolver error does not preserve a portable getaddrinfo code.
            Self::Resolve { .. } | Self::WorkerStopped | Self::EmptyDestinations => None,
        }
    }
    pub(crate) fn code(&self) -> String {
        if let Some(errno) = self.errno() {
            return super::util_system_error_shared_vm::system_error_name(errno);
        }
        match self {
            Self::Resolve { .. } => "ENOTFOUND",
            Self::Connect { error, .. } | Self::Setup(error) => match error.kind() {
                std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
                std::io::ErrorKind::TimedOut => "ETIMEDOUT",
                std::io::ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL",
                std::io::ErrorKind::AddrInUse => "EADDRINUSE",
                std::io::ErrorKind::PermissionDenied => "EACCES",
                std::io::ErrorKind::NetworkUnreachable => "ENETUNREACH",
                std::io::ErrorKind::HostUnreachable => "EHOSTUNREACH",
                _ => "UNKNOWN",
            },
            Self::WorkerStopped | Self::EmptyDestinations => "UNKNOWN",
        }
        .to_owned()
    }
    pub(crate) fn syscall(&self) -> &'static str {
        match self {
            Self::Resolve { .. } => "getaddrinfo",
            _ => "connect",
        }
    }
    pub(crate) fn address(&self) -> Option<SocketAddr> {
        match self {
            Self::Connect { address, .. } => Some(*address),
            _ => None,
        }
    }
    pub(crate) fn hostname(&self) -> Option<&str> {
        match self {
            Self::Resolve { hostname, .. } => Some(hostname),
            _ => None,
        }
    }
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Connect { address, .. } => format!(
                "connect {} {}:{}",
                self.code(),
                address.ip(),
                address.port()
            ),
            Self::Resolve { hostname, .. } => format!("getaddrinfo ENOTFOUND {hostname}"),
            Self::Setup(_) => format!("connect {}", self.code()),
            Self::WorkerStopped => "socket connect worker stopped".to_owned(),
            Self::EmptyDestinations => "resolved socket has no destinations".to_owned(),
        }
    }
}

impl std::fmt::Display for TcpConnectFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}
impl std::error::Error for TcpConnectFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect { error, .. } | Self::Resolve { error, .. } | Self::Setup(error) => {
                Some(error)
            }
            Self::WorkerStopped | Self::EmptyDestinations => None,
        }
    }
}
