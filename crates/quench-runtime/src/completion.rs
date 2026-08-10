use std::rc::Rc;

use crate::value::{PromiseData, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Completion {
    Normal,
    Return(Value),
    Throw(Value),
    Break(Option<String>),
    Continue(Option<String>),
    Suspend(Rc<PromiseData>),
}
