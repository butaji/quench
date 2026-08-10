use std::rc::Rc;

use crate::value::{PromiseData, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TailCallRequest {
    pub(crate) callee: Value,
    pub(crate) receiver: Value,
    pub(crate) arguments: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Completion {
    Normal,
    Return(Value),
    TailCall(TailCallRequest),
    Throw(Value),
    Break(Option<String>),
    Continue(Option<String>),
    Suspend(Rc<PromiseData>),
    Yield(Value),
}
