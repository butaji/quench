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

impl OperationState {
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
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

    /// Complete a pending operation exactly once. A pending target or a
    /// second completion is invalid and leaves the canonical operation
    /// unchanged.
    pub const fn try_complete(self, state: OperationState) -> Option<Self> {
        if !matches!(self.state, OperationState::Pending) || !state.is_terminal() {
            return None;
        }
        Some(Self { state, ..self })
    }

    pub const fn complete(self, state: OperationState) -> Self {
        match self.try_complete(state) {
            Some(operation) => operation,
            None => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Operation, OperationKind, OperationState};
    use crate::resource::ResourceId;

    #[test]
    fn operation_completion_preserves_identity_and_is_terminal() {
        let pending = Operation::pending(ResourceId(7), OperationKind::Read);
        let ready = pending.try_complete(OperationState::Ready).unwrap();
        assert_eq!(ready.resource, ResourceId(7));
        assert_eq!(ready.kind, OperationKind::Read);
        assert!(ready.state.is_terminal());
    }

    #[test]
    fn operation_rejects_invalid_or_duplicate_completion() {
        let pending = Operation::pending(ResourceId(1), OperationKind::Write);
        assert!(pending.try_complete(OperationState::Pending).is_none());
        let ready = pending.complete(OperationState::Ready);
        assert!(ready.try_complete(OperationState::Error).is_none());
    }
}
