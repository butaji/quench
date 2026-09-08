//! Rust-owned UDP edge for the future QUIC endpoint.
//!
//! QUIC packet protection and connection state belong above this layer.  This
//! module owns only the operating-system datagram edge: non-blocking sockets,
//! bounded transmit queues, and incremental receive events.  Keeping those
//! facts in the host envelope gives the eventual QUIC state machine one
//! transport instead of creating a second socket implementation in a JS
//! bridge or in each session object.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Instant;

use bytes::BytesMut;
use quinn_proto::{DatagramEvent, Endpoint as ProtocolEndpoint, EndpointConfig};

/// Maximum UDP payload accepted by the transport edge.  QUIC's packet layer
/// validates the packet-specific limits above this boundary; this cap keeps a
/// malformed or abandoned endpoint from retaining unbounded input.
pub const MAX_DATAGRAM_SIZE: usize = 64 * 1024;
const MAX_PENDING_DATAGRAMS: usize = 1024;
const RECEIVE_BUFFER_SIZE: usize = MAX_DATAGRAM_SIZE;

#[derive(Debug, PartialEq, Eq)]
pub enum TransportError {
    UnknownEndpoint(u64),
    DatagramTooLarge(usize),
    QueueFull(u64),
    Io { endpoint: u64, kind: io::ErrorKind },
}

/// One datagram delivered by an endpoint's non-blocking receive edge.
#[derive(Debug, PartialEq, Eq)]
pub struct ReceivedDatagram {
    pub endpoint: u64,
    pub peer: SocketAddr,
    pub payload: Vec<u8>,
}

/// Result of passing one UDP payload through the QUIC packet state machine.
///
/// This is intentionally transport-neutral: the future `node:quic` adapter
/// can map these facts to endpoint/session events without exposing quinn's
/// internal connection handles to the VM.
#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolEvent {
    Ignored {
        endpoint: u64,
        peer: SocketAddr,
        payload_len: usize,
    },
    Response {
        endpoint: u64,
        peer: SocketAddr,
        payload_len: usize,
    },
}

struct PendingDatagram {
    peer: SocketAddr,
    payload: Vec<u8>,
}

struct Endpoint {
    socket: UdpSocket,
    outbound: VecDeque<PendingDatagram>,
    /// QUIC's deterministic protocol state is kept beside the UDP edge.  The
    /// endpoint is deliberately not exposed to JavaScript: public session and
    /// stream facts must be derived from protocol events, not from a second
    /// JS-owned packet implementation.
    protocol: ProtocolEndpoint,
    /// Whether an application-installed server configuration allows Initial
    /// packets to enter quinn-proto's incoming-connection state machine.
    server_configured: bool,
}

/// Host-owned UDP endpoints and their transport queues.
pub struct QuicTransportState {
    next_endpoint: u64,
    endpoints: HashMap<u64, Endpoint>,
    received: VecDeque<ReceivedDatagram>,
    protocol_events: VecDeque<ProtocolEvent>,
    errors: VecDeque<TransportError>,
}

impl Default for QuicTransportState {
    fn default() -> Self {
        Self::new()
    }
}

impl QuicTransportState {
    pub fn new() -> Self {
        Self {
            next_endpoint: 1,
            endpoints: HashMap::new(),
            received: VecDeque::new(),
            protocol_events: VecDeque::new(),
            errors: VecDeque::new(),
        }
    }

    /// Bind a non-blocking UDP endpoint and return its host-local identity.
    pub fn bind(&mut self, address: SocketAddr) -> io::Result<u64> {
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        let id = self.next_endpoint;
        self.next_endpoint = self.next_endpoint.saturating_add(1);
        self.endpoints.insert(
            id,
            Endpoint {
                socket,
                outbound: VecDeque::new(),
                protocol: ProtocolEndpoint::new(
                    Arc::new(EndpointConfig::default()),
                    None,
                    false,
                    None,
                ),
                server_configured: false,
            },
        );
        Ok(id)
    }

    /// Install or remove the server TLS/protocol configuration for an
    /// endpoint. No JavaScript surface calls this yet; it is the single
    /// host-side transition a future `node:quic` adapter will use once
    /// certificate and ALPN validation are mapped into `ServerConfig`.
    pub fn set_server_config(
        &mut self,
        endpoint: u64,
        config: Option<Arc<quinn_proto::ServerConfig>>,
    ) -> Result<(), TransportError> {
        let endpoint_state = self
            .endpoints
            .get_mut(&endpoint)
            .ok_or(TransportError::UnknownEndpoint(endpoint))?;
        endpoint_state.server_configured = config.is_some();
        endpoint_state.protocol.set_server_config(config);
        Ok(())
    }

    pub fn local_addr(&self, endpoint: u64) -> Result<SocketAddr, TransportError> {
        self.endpoints
            .get(&endpoint)
            .ok_or(TransportError::UnknownEndpoint(endpoint))?
            .socket
            .local_addr()
            .map_err(|error| TransportError::Io {
                endpoint,
                kind: error.kind(),
            })
    }

    /// Queue one datagram for non-blocking transmission on the next pump.
    /// The queue is deliberately bounded; backpressure belongs to the public
    /// QUIC writer layer and cannot be represented by an unbounded host queue.
    pub fn send(
        &mut self,
        endpoint: u64,
        peer: SocketAddr,
        payload: Vec<u8>,
    ) -> Result<(), TransportError> {
        if payload.len() > MAX_DATAGRAM_SIZE {
            return Err(TransportError::DatagramTooLarge(payload.len()));
        }
        let socket = self
            .endpoints
            .get_mut(&endpoint)
            .ok_or(TransportError::UnknownEndpoint(endpoint))?;
        if socket.outbound.len() >= MAX_PENDING_DATAGRAMS {
            return Err(TransportError::QueueFull(endpoint));
        }
        socket.outbound.push_back(PendingDatagram { peer, payload });
        Ok(())
    }

    /// Poll all endpoints once.  Readiness is represented by the resulting
    /// queues, so the VM-facing pump never blocks on an OS socket.
    pub fn poll(&mut self) {
        let ids = self.endpoints.keys().copied().collect::<Vec<_>>();
        for endpoint_id in ids {
            self.flush(endpoint_id);
            self.receive(endpoint_id);
        }
    }

    pub fn take_received(&mut self) -> Option<ReceivedDatagram> {
        self.received.pop_front()
    }

    /// Take the next packet-level protocol result for the eventual endpoint
    /// and session adapter.
    pub fn take_protocol_event(&mut self) -> Option<ProtocolEvent> {
        self.protocol_events.pop_front()
    }

    pub fn take_error(&mut self) -> Option<TransportError> {
        self.errors.pop_front()
    }

    pub fn pending_received(&self) -> usize {
        self.received.len()
    }

    pub fn close(&mut self, endpoint: u64) -> bool {
        self.endpoints.remove(&endpoint).is_some()
    }

    fn flush(&mut self, endpoint_id: u64) {
        let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) else {
            return;
        };
        loop {
            let Some(datagram) = endpoint.outbound.front() else {
                return;
            };
            match endpoint.socket.send_to(&datagram.payload, datagram.peer) {
                Ok(written) if written == datagram.payload.len() => {
                    endpoint.outbound.pop_front();
                }
                Ok(_) => {
                    // UDP writes are atomic. A short write is not a valid
                    // transport outcome, so report it as an I/O failure and
                    // discard only this datagram.
                    endpoint.outbound.pop_front();
                    self.errors.push_back(TransportError::Io {
                        endpoint: endpoint_id,
                        kind: io::ErrorKind::WriteZero,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) => {
                    endpoint.outbound.pop_front();
                    self.errors.push_back(TransportError::Io {
                        endpoint: endpoint_id,
                        kind: error.kind(),
                    });
                }
            }
        }
    }

    fn receive(&mut self, endpoint_id: u64) {
        let mut datagrams = Vec::new();
        {
            let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) else {
                return;
            };
            let mut buffer = vec![0u8; RECEIVE_BUFFER_SIZE];
            loop {
                match endpoint.socket.recv_from(&mut buffer) {
                    Ok((size, peer)) => {
                        let payload = buffer[..size].to_vec();
                        self.received.push_back(ReceivedDatagram {
                            endpoint: endpoint_id,
                            peer,
                            payload: payload.clone(),
                        });
                        datagrams.push((peer, payload));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        self.errors.push_back(TransportError::Io {
                            endpoint: endpoint_id,
                            kind: error.kind(),
                        });
                        break;
                    }
                }
            }
        }

        // Feed every received datagram through the protocol state in the same
        // pump tick.  `received` remains available to the eventual session
        // adapter; protocol processing has its own copy so consuming one queue
        // cannot make the other silently lose bytes.
        for (peer, payload) in datagrams {
            self.handle_protocol_datagram(endpoint_id, peer, payload);
        }
    }

    fn handle_protocol_datagram(&mut self, endpoint_id: u64, peer: SocketAddr, payload: Vec<u8>) {
        // quinn-proto's endpoint parser assumes a server configuration exists
        // once a packet is identified as Initial. This host endpoint is
        // currently transport-only, so reject that transition explicitly
        // rather than allowing a peer-controlled datagram to panic the event
        // loop. The original datagram remains available through `received`.
        let server_configured = self
            .endpoints
            .get(&endpoint_id)
            .is_some_and(|endpoint| endpoint.server_configured);
        if !server_configured && is_initial_packet(&payload) {
            self.protocol_events.push_back(ProtocolEvent::Ignored {
                endpoint: endpoint_id,
                peer,
                payload_len: payload.len(),
            });
            return;
        }
        let Some(local_ip) = self
            .endpoints
            .get(&endpoint_id)
            .and_then(|endpoint| endpoint.socket.local_addr().ok())
            .map(|address| address.ip())
        else {
            return;
        };
        let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) else {
            return;
        };
        let mut response = Vec::new();
        let event = endpoint.protocol.handle(
            Instant::now(),
            peer,
            Some(local_ip),
            None,
            BytesMut::from(payload.as_slice()),
            &mut response,
        );
        let Some(DatagramEvent::Response(transmit)) = event else {
            self.protocol_events.push_back(ProtocolEvent::Ignored {
                endpoint: endpoint_id,
                peer,
                payload_len: payload.len(),
            });
            return;
        };
        // `quinn-proto` may describe a GSO transmit.  The current UDP edge is
        // intentionally one datagram per queue item, so do not accidentally
        // truncate or reinterpret a segmented response.
        if transmit.segment_size.is_some() || transmit.size > response.len() {
            self.errors.push_back(TransportError::Io {
                endpoint: endpoint_id,
                kind: io::ErrorKind::InvalidData,
            });
            return;
        }
        let payload = response[..transmit.size].to_vec();
        self.protocol_events.push_back(ProtocolEvent::Response {
            endpoint: endpoint_id,
            peer,
            payload_len: payload.len(),
        });
        if let Err(error) = self.send(endpoint_id, transmit.destination, payload) {
            self.errors.push_back(error);
        }
    }
}

/// Recognize the QUIC long-header Initial packet type without decoding it.
/// This is only a capability guard; all packet validation remains delegated to
/// quinn-proto after a server configuration is installed.
fn is_initial_packet(payload: &[u8]) -> bool {
    payload
        .first()
        .is_some_and(|first| first & 0x80 != 0 && first & 0x30 == 0)
}

/// Event-loop integration point.  QUIC remains capability-disabled until a
/// protocol/session layer consumes `take_received`; polling now nevertheless
/// gives that layer the same host-owned lifecycle as TCP endpoints.
pub fn poll(state: &std::rc::Rc<std::cell::RefCell<crate::host::HostState>>) {
    state.borrow_mut().quic.poll();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn loopback() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 0))
    }

    #[test]
    fn endpoint_receives_and_sends_loopback_datagrams() {
        let mut state = QuicTransportState::new();
        let endpoint = state.bind(loopback()).unwrap();
        let address = state.local_addr(endpoint).unwrap();
        let peer = UdpSocket::bind(loopback()).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();

        peer.send_to(b"inbound", address).unwrap();
        for _ in 0..100 {
            state.poll();
            if state.pending_received() != 0 {
                break;
            }
            std::thread::yield_now();
        }
        let received = state.take_received().unwrap();
        assert_eq!(received.endpoint, endpoint);
        assert_eq!(received.peer, peer.local_addr().unwrap());
        assert_eq!(received.payload, b"inbound");

        state
            .send(endpoint, peer.local_addr().unwrap(), b"outbound".to_vec())
            .unwrap();
        state.poll();
        let mut output = [0u8; 32];
        let (size, source) = peer.recv_from(&mut output).unwrap();
        assert_eq!(source, address);
        assert_eq!(&output[..size], b"outbound");
    }

    #[test]
    fn transport_rejects_oversize_and_unknown_datagrams() {
        let mut state = QuicTransportState::new();
        let payload = vec![0u8; MAX_DATAGRAM_SIZE + 1];
        assert_eq!(
            state.send(9, loopback(), payload.clone()),
            Err(TransportError::DatagramTooLarge(payload.len()))
        );
        assert_eq!(
            state.send(9, loopback(), Vec::new()),
            Err(TransportError::UnknownEndpoint(9))
        );
    }

    #[test]
    fn protocol_state_consumes_malformed_packets_without_losing_transport_bytes() {
        let mut state = QuicTransportState::new();
        let endpoint = state.bind(loopback()).unwrap();
        let address = state.local_addr(endpoint).unwrap();
        let peer = UdpSocket::bind(loopback()).unwrap();
        let packet = vec![0u8, 1, 2];

        peer.send_to(&packet, address).unwrap();
        for _ in 0..100 {
            state.poll();
            if state.pending_received() != 0 {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(
            state.take_protocol_event(),
            Some(ProtocolEvent::Ignored {
                endpoint,
                peer: peer.local_addr().unwrap(),
                payload_len: packet.len(),
            })
        );
        // The protocol layer observes a copy; the transport contract still
        // exposes the original datagram to the eventual endpoint adapter.
        assert_eq!(state.take_received().unwrap().payload, packet);
    }

    #[test]
    fn unconfigured_endpoint_ignores_initial_without_panicking() {
        let mut state = QuicTransportState::new();
        let endpoint = state.bind(loopback()).unwrap();
        let address = state.local_addr(endpoint).unwrap();
        let peer = UdpSocket::bind(loopback()).unwrap();
        // Long-header, fixed-bit Initial prefix and a version. The remainder
        // is intentionally incomplete; this exercises the configuration guard
        // before quinn-proto's Initial decoder.
        let packet = vec![0xc0, 0, 0, 0, 1, 0, 0, 0];

        peer.send_to(&packet, address).unwrap();
        for _ in 0..100 {
            state.poll();
            if state.pending_received() != 0 {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(
            state.take_protocol_event(),
            Some(ProtocolEvent::Ignored {
                endpoint,
                peer: peer.local_addr().unwrap(),
                payload_len: packet.len(),
            })
        );
        assert_eq!(state.take_received().unwrap().payload, packet);
    }

    #[test]
    fn server_configuration_transition_is_endpoint_scoped() {
        let mut state = QuicTransportState::new();
        let first = state.bind(loopback()).unwrap();
        let second = state.bind(loopback()).unwrap();

        assert_eq!(state.set_server_config(first, None), Ok(()));
        assert_eq!(
            state.set_server_config(99, None),
            Err(TransportError::UnknownEndpoint(99))
        );
        assert!(!state.endpoints.get(&first).unwrap().server_configured);
        assert!(!state.endpoints.get(&second).unwrap().server_configured);
    }
}
