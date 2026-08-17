//! Engine-neutral external resources.
//!
//! A resource is deliberately smaller than any Node subsystem. Files,
//! sockets, timers, and processes all share identity and lifecycle semantics;
//! platform implementations decide what the identifier means.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ResourceId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    File,
    Socket,
    Listener,
    Pipe,
    Process,
    Timer,
    Tty,
    Dns,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceState {
    Open,
    Closing,
    Closed,
}

pub trait Resource {
    fn id(&self) -> ResourceId;
    fn kind(&self) -> ResourceKind;
    fn state(&self) -> ResourceState;
}
