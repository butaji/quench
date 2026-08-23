use std::rc::Rc;

use crate::value::{PromiseData, Value};

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
