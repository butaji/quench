//! Message channel endpoints. An endpoint is process-wide: a port transferred to another
//! agent moves its endpoint (and the messages already queued for it) to that agent.

use crate::modules::napi::WakeSignal;
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
    /// Wake the owning VM's event loop when this endpoint receives work.
    wake: Option<Arc<WakeSignal>>,
}

impl Endpoint {
    fn lock(&self) -> MutexGuard<'_, EndpointState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Two entangled endpoints.
    pub(crate) fn channel() -> (Arc<Self>, Arc<Self>) {
        let first = Arc::new(Self {
            state: Mutex::default(),
        });
        let second = Arc::new(Self {
            state: Mutex::default(),
        });
        first.lock().peer = Arc::downgrade(&second);
        second.lock().peer = Arc::downgrade(&first);
        (first, second)
    }

    /// Associate this endpoint with the event loop that currently owns its port.
    /// A message may have arrived before adoption, so signal after releasing the lock.
    pub(crate) fn attach_wake(&self, wake: Arc<WakeSignal>) {
        let has_pending = {
            let mut state = self.lock();
            state.wake = Some(Arc::clone(&wake));
            !state.inbox.is_empty() || state.peer_closed
        };
        if has_pending {
            wake.notify();
        }
    }

    /// Queue `message` for the other end; it is dropped when that end is gone or closed.
    pub(crate) fn send(&self, message: Message) {
        let peer = self.lock().peer.upgrade();
        if let Some(peer) = peer {
            let wake = {
                let mut state = peer.lock();
                if state.closed {
                    None
                } else {
                    state.inbox.push_back(message);
                    state.wake.clone()
                }
            };
            if let Some(wake) = wake {
                wake.notify();
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
            let wake = {
                let mut state = peer.lock();
                state.peer_closed = true;
                state.wake.clone()
            };
            if let Some(wake) = wake {
                wake.notify();
            }
        }
    }
}

impl Drop for Endpoint {
    /// An endpoint no agent holds any longer closes its channel.
    fn drop(&mut self) {
        let peer = self.lock().peer.upgrade();
        if let Some(peer) = peer {
            let wake = {
                let mut state = peer.lock();
                state.peer_closed = true;
                state.wake.clone()
            };
            if let Some(wake) = wake {
                wake.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Endpoint;
    use crate::modules::napi;
    use std::sync::Arc;

    fn assert_wakes(_signal: &napi::WakeSignal) {
        let mut descriptor = libc::pollfd {
            fd: napi::wake_fd().unwrap(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll only reads and writes this one valid pollfd for the live wake signal.
        let ready = unsafe { libc::poll(&mut descriptor, 1, 1000) };
        assert_eq!(
            ready, 1,
            "endpoint activity must wake the owning event loop"
        );
        napi::drain_wake();
    }

    #[test]
    fn peer_close_wakes_the_attached_event_loop() {
        let (sender, receiver) = Endpoint::channel();
        let signal = napi::wake_signal().unwrap();
        receiver.attach_wake(Arc::clone(&signal));
        sender.close();
        assert_wakes(&signal);
        assert!(receiver.peer_closed());
    }

    #[test]
    fn adoption_wakes_if_peer_closed_before_attachment() {
        let (sender, receiver) = Endpoint::channel();
        sender.close();
        let signal = napi::wake_signal().unwrap();
        receiver.attach_wake(Arc::clone(&signal));
        assert_wakes(&signal);
        assert!(receiver.peer_closed());
    }
}
