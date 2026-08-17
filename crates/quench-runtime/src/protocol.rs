//! Composable protocols over resources and operations.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolKind {
    Bytes,
    Stream,
    Events,
    Completion,
    Backpressure,
    Ipc,
    Custom,
}

pub trait Protocol {
    fn kind(&self) -> ProtocolKind;
}
