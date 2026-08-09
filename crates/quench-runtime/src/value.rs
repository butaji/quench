//! Machine-sized runtime values for the residual kernel.

use std::rc::Rc;

use crate::ops::Op;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    Function(Rc<Vec<Op>>),
    Null,
    Undefined,
}
