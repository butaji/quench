//! Machine-sized runtime values for the residual kernel.

use std::rc::Rc;

use crate::ops::{Builtin, Op};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    Array(Rc<Vec<Value>>),
    Object(Rc<Vec<(String, Value)>>),
    Builtin(Builtin),
    Function(Rc<FunctionValue>),
    Null,
    Undefined,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionValue {
    pub body: Vec<Op>,
    pub params: u16,
    pub properties: Rc<Vec<(String, Value)>>,
}
