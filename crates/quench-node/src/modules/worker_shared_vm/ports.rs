//! Message channel endpoints. An endpoint is process-wide: a port transferred to another
//! agent moves its endpoint (and the messages already queued for it) to that agent.

use quench_runtime::SerializedValue;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

/// One serialized message and the endpoints of the ports it transfers, in transfer order.
pub(crate) struct Message {
    pub(crate) value: SerializedValue,
    pub(crate) ports: Vec<Arc<Endpoint>>,
}

/// One end of a channel: the messages its port has yet to receive.
pub(crate) struct Endpoint {
    state: Mutex<EndpointState>,
}

#[derive(Default)]
struct EndpointState {
    inbox: VecDeque<Message>,
    peer: Weak<Endpoint>,
    /// This end was closed; it accepts no further messages.
    closed: bool,
    /// The other end was closed or dropped; this port's 'close' is due.
    peer_closed: bool,
}

impl Endpoint {
    fn lock(&self) -> MutexGuard<'_, EndpointState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Two entangled endpoints.
    pub(crate) fn channel() -> (Arc<Self>, Arc<Self>) {
        let first = Arc::new(Self { state: Mutex::default() });
        let second = Arc::new(Self { state: Mutex::default() });
        first.lock().peer = Arc::downgrade(&second);
        second.lock().peer = Arc::downgrade(&first);
        (first, second)
    }

    /// Queue `message` for the other end; it is dropped when that end is gone or closed.
    pub(crate) fn send(&self, message: Message) {
        let peer = self.lock().peer.upgrade();
        if let Some(peer) = peer {
            let mut state = peer.lock();
            if !state.closed {
                state.inbox.push_back(message);
            }
        }
    }

    pub(crate) fn receive(&self) -> Option<Message> {
        self.lock().inbox.pop_front()
    }

    pub(crate) fn has_messages(&self) -> bool {
        !self.lock().inbox.is_empty()
    }

    pub(crate) fn peer_closed(&self) -> bool {
        self.lock().peer_closed
    }

    /// Close this end: undelivered messages are discarded and the other end's 'close' is due.
    pub(crate) fn close(&self) {
        let peer = {
            let mut state = self.lock();
            state.closed = true;
            state.inbox.clear();
            state.peer.upgrade()
        };
        if let Some(peer) = peer {
            peer.lock().peer_closed = true;
        }
    }
}

impl Drop for Endpoint {
    /// An endpoint no agent holds any longer closes its channel.
    fn drop(&mut self) {
        let peer = self.lock().peer.upgrade();
        if let Some(peer) = peer {
            peer.lock().peer_closed = true;
        }
    }
}
