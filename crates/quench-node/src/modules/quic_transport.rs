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

struct PendingDatagram {
    peer: SocketAddr,
    payload: Vec<u8>,
}

struct Endpoint {
    socket: UdpSocket,
    outbound: VecDeque<PendingDatagram>,
}

/// Host-owned UDP endpoints and their transport queues.
pub struct QuicTransportState {
    next_endpoint: u64,
    endpoints: HashMap<u64, Endpoint>,
    received: VecDeque<ReceivedDatagram>,
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
            },
        );
        Ok(id)
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
        let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) else {
            return;
        };
        let mut buffer = vec![0u8; RECEIVE_BUFFER_SIZE];
        loop {
            match endpoint.socket.recv_from(&mut buffer) {
                Ok((size, peer)) => {
                    self.received.push_back(ReceivedDatagram {
                        endpoint: endpoint_id,
                        peer,
                        payload: buffer[..size].to_vec(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.errors.push_back(TransportError::Io {
                        endpoint: endpoint_id,
                        kind: error.kind(),
                    });
                    return;
                }
            }
        }
    }
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
        state.poll();
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
}
