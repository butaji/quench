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
    Break(Option<String>),
    Continue(Option<String>),
    Suspend(Rc<PromiseData>),
    Yield(Value),
}

impl Completion {
    pub(crate) fn is_suspension(&self) -> bool {
        matches!(self, Self::Suspend(_) | Self::Yield(_))
    }

    pub(crate) fn from_vm_error(
        error: crate::execute::VmError,
    ) -> Result<Self, crate::execute::VmError> {
        use crate::execute::VmError;
        Ok(match error {
            VmError::Thrown(value) => Self::Throw(value),
            VmError::Break(label) => Self::Break(label),
            VmError::Continue(label) => Self::Continue(label),
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

    pub(crate) fn into_vm_error(self) -> Result<Value, crate::execute::VmError> {
        use crate::execute::VmError;
        match self {
            Self::Normal => Err(VmError::MissingReturn),
            Self::Return(value) => Ok(value),
            Self::TailCall(_) => Err(VmError::EvalError(
                "Unconsumed tail-call completion".to_string(),
            )),
            Self::Throw(value) => Err(VmError::Thrown(value)),
            Self::Break(label) => Err(VmError::Break(label)),
            Self::Continue(label) => Err(VmError::Continue(label)),
            Self::Suspend(promise) => Err(VmError::Suspended(promise)),
            Self::Yield(_) => Err(VmError::EvalError(
                "Unconsumed yield completion".to_string(),
            )),
        }
    }
}
