//! Machine-sized runtime values for the residual kernel.

use std::{cell::RefCell, rc::Rc};

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
    BoundFunction(Rc<BoundFunctionValue>),
    Null,
    Undefined,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionValue {
    pub body: Vec<Op>,
    pub params: u16,
    pub captures: Rc<RefCell<Vec<Value>>>,
    pub properties: Rc<RefCell<Vec<(String, Value)>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundFunctionValue {
    pub target: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
}
