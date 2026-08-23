use std::rc::Rc;

use crate::value::{PromiseData, Value};

/// State required to resume a caller after a non-tail call completes.
///
/// The payload deliberately owns the caller register window and immutable
/// instruction slice.  This makes the transition independent of Rust's call
/// stack and keeps all state needed by the dispatch loop in one value.
#[derive(Debug, Clone, PartialEq)]
pub struct CallContinuation {
    pub callee: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
    pub caller_ops: Rc<[crate::ops::Op]>,
    pub caller_pc: u32,
    pub caller_registers: Vec<Value>,
    pub caller_environment: crate::identity::EnvironmentRef,
    pub destination: u16,
    pub guards: ContinuationGuards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContinuationGuards {
    /// Guard state captured at the call boundary (strict-eval, with-scope,
    /// private-environment, and host-context bits are intentionally opaque to
    /// the completion transport).
    pub flags: u32,
}

impl ContinuationGuards {
    pub const fn new(flags: u32) -> Self {
        Self { flags }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TailCallRequest {
    pub callee: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Completion {
    Normal,
    Return(Value),
    /// A call that must be executed before the current caller can continue.
    Call(CallContinuation),
    TailCall(TailCallRequest),
    Throw(Value),
    Break {
        label: Option<String>,
        value: Option<Value>,
    },
    Continue {
        label: Option<String>,
        value: Option<Value>,
    },
    Suspend(Rc<PromiseData>),
    Yield(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoopTransition {
    Continue(Option<Value>),
    Break(Option<Value>),
    Propagate(Completion),
}

impl Completion {
    pub(crate) fn into_loop_transition(self, label: &Option<String>) -> LoopTransition {
        match self {
            Self::Normal => LoopTransition::Continue(None),
            Self::Continue {
                label: target,
                value,
            } if target == *label || target.is_none() => LoopTransition::Continue(value),
            Self::Break {
                label: target,
                value,
            } if target == *label || target.is_none() => LoopTransition::Break(value),
            completion => LoopTransition::Propagate(completion),
        }
    }

    pub(crate) fn update_empty(self, value: Value) -> Self {
        match self {
            Self::Continue { label, value: None } => Self::Continue {
                label,
                value: Some(value),
            },
            Self::Break { label, value: None } => Self::Break {
                label,
                value: Some(value),
            },
            other => other,
        }
    }

    pub(crate) fn is_suspension(&self) -> bool {
        matches!(self, Self::Suspend(_) | Self::Yield(_))
    }

    pub(crate) fn from_vm_error(
        error: crate::execute::VmError,
    ) -> Result<Self, crate::execute::VmError> {
        use crate::execute::VmError;
        Ok(match error {
            VmError::Thrown(value) => Self::Throw(value),
            VmError::Break(label) => Self::Break { label, value: None },
            VmError::Continue(label) => Self::Continue { label, value: None },
            VmError::Suspended(promise) => Self::Suspend(promise),
            VmError::NotCallable => {
                let VmError::Thrown(value) = crate::execute::not_callable() else {
                    return Err(VmError::NotCallable);
                };
                Self::Throw(value)
            }
            error => return Err(error),
        })
    }

    pub fn into_vm_error(self) -> Result<Value, crate::execute::VmError> {
        use crate::execute::VmError;
        match self {
            Self::Normal => Err(VmError::MissingReturn),
            Self::Return(value) => Ok(value),
            Self::Call(_) => Err(VmError::EvalError("Unconsumed call completion".to_string())),
            Self::TailCall(_) => Err(VmError::EvalError(
                "Unconsumed tail-call completion".to_string(),
            )),
            Self::Throw(value) => Err(VmError::Thrown(value)),
            Self::Break { label, .. } => Err(VmError::Break(label)),
            Self::Continue { label, .. } => Err(VmError::Continue(label)),
            Self::Suspend(promise) => Err(VmError::Suspended(promise)),
            Self::Yield(_) => Err(VmError::EvalError(
                "Unconsumed yield completion".to_string(),
            )),
        }
    }
}
