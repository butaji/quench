//! The shared operation algebra used by platform and protocol adapters.

use super::resource::ResourceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    Read,
    Write,
    Open,
    Close,
    Poll,
    Wait,
    Spawn,
    Sleep,
    Resolve,
    Random,
    Control,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationState {
    Pending,
    Ready,
    Error,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Operation {
    pub resource: ResourceId,
    pub kind: OperationKind,
    pub state: OperationState,
}

impl Operation {
    pub const fn pending(resource: ResourceId, kind: OperationKind) -> Self {
        Self {
            resource,
            kind,
            state: OperationState::Pending,
        }
    }

    pub const fn complete(self, state: OperationState) -> Self {
        Self { state, ..self }
    }
}
