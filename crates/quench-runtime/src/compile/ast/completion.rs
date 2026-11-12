//! What a statement leaves in the completion value register.
use super::*;

#[derive(Clone, Copy)]
pub(in crate::compile) enum StatementCompletion {
    Ignored,
    Track(Register),
    Suppress(Register),
    Finally { current: Register, target: Register },
}

impl StatementCompletion {
    pub(in crate::compile) fn register(self) -> Option<Register> {
        match self {
            Self::Ignored => None,
            Self::Track(register) | Self::Suppress(register) => Some(register),
            Self::Finally { current, .. } => Some(current),
        }
    }
}
