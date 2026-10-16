//! A resolver channel's configuration and its blocking query exchange, run off the VM
//! thread. Retries follow c-ares 1.34: each try moves to the next server, the timeout
//! doubles per pass through the server list (capped by `maxTimeout`), a truncated UDP
//! answer is repeated over TCP, and an EDNS FORMERR without OPT is retried without EDNS.

use super::answers::Status;
use super::wire::{self, RecordType, Response};
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

/// The DNS port (IANA).
pub(crate) const DNS_PORT: u16 = 53;
/// c-ares defaults (`ares_private.h`): the per-try timeout before sysconfig, and the
/// loopback server used when no nameserver is configured.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(2000);
const DEFAULT_SERVER: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const RESOLV_CONF: &str = "/etc/resolv.conf";
/// Largest DNS message: TCP frames carry a 16-bit length.
const MAX_MESSAGE_LEN: usize = u16::MAX as usize;
const TCP_LENGTH_PREFIX: usize = 2;

#[derive(Clone, Debug)]
pub(crate) struct Server {
    pub(crate) address: IpAddr,
    pub(crate) port: u16,
}

impl Server {
    fn socket_address(&self) -> SocketAddr {
        SocketAddr::new(self.address, if self.port == 0 { DNS_PORT } else { self.port })
    }
}

/// One `ChannelWrap`: its servers, retry policy and local source addresses.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) servers: Vec<Server>,
    timeout: Duration,
    tries: u32,
    max_timeout: Option<Duration>,
    pub(crate) local_v4: Option<Ipv4Addr>,
    pub(crate) local_v6: Option<Ipv6Addr>,
}

impl Config {
    /// `new ChannelWrap(timeout, tries, maxTimeout)`: a non-positive timeout uses the
    /// system configuration's, then c-ares' default.
    pub(crate) fn new(timeout_ms: i32, tries: i32, max_timeout_ms: i32) -> Self {
        let system = SystemConfig::read();
        let timeout = u64::try_from(timeout_ms)
            .ok()
            .filter(|&ms| ms > 0)
            .map(Duration::from_millis)
            .or(system.timeout)
            .unwrap_or(DEFAULT_TIMEOUT);
        let servers = if system.servers.is_empty() {
            vec![Server { address: DEFAULT_SERVER, port: DNS_PORT }]
        } else {
            system.servers
        };
        Self {
            servers,
            timeout,
            tries: u32::try_from(tries).unwrap_or(1).max(1),
            max_timeout: u64::try_from(max_timeout_ms).ok().filter(|&ms| ms > 0).map(Duration::from_millis),
            local_v4: None,
            local_v6: None,
        }
    }
}

struct SystemConfig {
    servers: Vec<Server>,
    timeout: Option<Duration>,
}

impl SystemConfig {
    fn read() -> Self {
        let mut config = Self { servers: Vec::new(), timeout: None };
        let Ok(text) = std::fs::read_to_string(RESOLV_CONF) else { return config };
        for line in text.lines() {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("nameserver") => {
                    let address = words.next().map(|word| word.split('%').next().unwrap_or(word));
                    if let Some(address) = address.and_then(|text| text.parse::<IpAddr>().ok()) {
                        config.servers.push(Server { address, port: DNS_PORT });
                    }
                }
                Some("options") => {
                    for option in words {
                        if let Some(seconds) = option.strip_prefix("timeout:").and_then(|value| value.parse::<u64>().ok()) {
                            config.timeout = Some(Duration::from_secs(seconds));
                        }
                    }
                }
                _ => {}
            }
        }
        config
    }
}

/// How one exchange with one server ended.
enum Exchange {
    Answer(Response),
    Failed(Status),
}

/// Send `name`'s query through the channel's servers until one answers or tries run out.
pub(crate) fn query(config: &Config, name: &str, kind: RecordType) -> Result<Response, Status> {
    if config.servers.is_empty() {
        return Err(Status::NoServer);
    }
    let attempts = config.tries as usize * config.servers.len();
    let mut last = Status::Timeout;
    for attempt in 0..attempts {
        let server = &config.servers[attempt % config.servers.len()];
        let rounds = (attempt / config.servers.len()) as u32;
        let mut timeout = config.timeout.checked_mul(1 << rounds.min(u32::BITS - 1)).unwrap_or(Duration::MAX);
        if let Some(max) = config.max_timeout {
            // The cap never shortens a try below the base timeout.
            timeout = timeout.min(max).max(config.timeout);
        }
        match exchange(config, server, name, kind, timeout)? {
            Exchange::Answer(response) => return Ok(response),
            Exchange::Failed(status) => last = status,
        }
    }
    Err(last)
}

fn exchange(config: &Config, server: &Server, name: &str, kind: RecordType, timeout: Duration) -> Result<Exchange, Status> {
    let mut edns = true;
    loop {
        let id = query_id();
        let message = wire::encode_query(id, name, kind, edns).map_err(|_| Status::BadName)?;
        let outcome = match udp(config, server, &message, id, timeout) {
            Ok(response) if response.truncated => tcp(config, server, &message, id, timeout),
            other => other,
        };
        return Ok(match outcome {
            Ok(response) if edns && !response.has_opt && response.code == wire::ResponseCode::FormErr => {
                edns = false;
                continue;
            }
            Ok(response) => Exchange::Answer(response),
            Err(status) => Exchange::Failed(status),
        });
    }
}

fn query_id() -> u16 {
    let mut bytes = [0u8; 2];
    if openssl::rand::rand_bytes(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        bytes = (nanos as u16).to_be_bytes();
    }
    u16::from_be_bytes(bytes)
}

fn local_address(config: &Config, server: &SocketAddr) -> SocketAddr {
    match server {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(config.local_v4.unwrap_or(Ipv4Addr::UNSPECIFIED)), 0),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(config.local_v6.unwrap_or(Ipv6Addr::UNSPECIFIED)), 0),
    }
}

fn io_status(error: &std::io::Error) -> Status {
    match error.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => Status::Timeout,
        _ => Status::ConnRefused,
    }
}

/// One UDP exchange; datagrams for other queries are ignored until the deadline.
fn udp(config: &Config, server: &Server, message: &[u8], id: u16, timeout: Duration) -> Result<Response, Status> {
    let target = server.socket_address();
    let socket = UdpSocket::bind(local_address(config, &target)).map_err(|error| io_status(&error))?;
    socket.connect(target).map_err(|error| io_status(&error))?;
    socket.send(message).map_err(|error| io_status(&error))?;
    let deadline = Instant::now() + timeout;
    let mut buffer = vec![0u8; MAX_MESSAGE_LEN];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Status::Timeout);
        }
        socket.set_read_timeout(Some(remaining)).map_err(|error| io_status(&error))?;
        let length = socket.recv(&mut buffer).map_err(|error| io_status(&error))?;
        match wire::decode(&buffer[..length]) {
            Ok(response) if response.id == id => return Ok(response),
            Ok(_) => continue,
            Err(_) => return Err(Status::BadResp),
        }
    }
}

/// One TCP exchange: a length-prefixed query and answer.
fn tcp(config: &Config, server: &Server, message: &[u8], id: u16, timeout: Duration) -> Result<Response, Status> {
    let target = server.socket_address();
    let deadline = Instant::now() + timeout;
    let socket = socket2::Socket::new(socket2::Domain::for_address(target), socket2::Type::STREAM, None)
        .map_err(|error| io_status(&error))?;
    socket.bind(&local_address(config, &target).into()).map_err(|error| io_status(&error))?;
    socket.connect_timeout(&target.into(), timeout).map_err(|error| io_status(&error))?;
    let mut stream: TcpStream = socket.into();
    let remaining = |deadline: Instant| {
        let left = deadline.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left).ok_or(Status::Timeout)
    };
    stream.set_write_timeout(Some(remaining(deadline)?)).map_err(|error| io_status(&error))?;
    let length = u16::try_from(message.len()).map_err(|_| Status::BadQuery)?;
    stream.write_all(&length.to_be_bytes()).and_then(|()| stream.write_all(message)).map_err(|error| io_status(&error))?;
    stream.set_read_timeout(Some(remaining(deadline)?)).map_err(|error| io_status(&error))?;
    let mut prefix = [0u8; TCP_LENGTH_PREFIX];
    stream.read_exact(&mut prefix).map_err(|error| io_status(&error))?;
    let mut answer = vec![0u8; usize::from(u16::from_be_bytes(prefix))];
    stream.set_read_timeout(Some(remaining(deadline)?)).map_err(|error| io_status(&error))?;
    stream.read_exact(&mut answer).map_err(|error| io_status(&error))?;
    match wire::decode(&answer) {
        Ok(response) if response.id == id => Ok(response),
        _ => Err(Status::BadResp),
    }
}
