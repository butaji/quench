//! Machine-sized runtime values for the residual kernel.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::ops::{Builtin, Constant, Op};

/// Promise state: pending, fulfilled, or rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

/// Heap-allocated Promise data.
#[derive(Debug, Clone, PartialEq)]
pub struct PromiseData {
    pub state: RefCell<PromiseState>,
    pub result: RefCell<Option<Value>>,
    pub then_actions: RefCell<Vec<(Option<Value>, Option<Value>)>>,
}

impl PromiseData {
    pub fn new(state: PromiseState) -> Self {
        let result = match &state {
            PromiseState::Pending => None,
            PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
        };
        Self {
            state: RefCell::new(state),
            result: RefCell::new(result),
            then_actions: RefCell::new(Vec::new()),
        }
    }
}

impl Default for PromiseData {
    fn default() -> Self {
        Self::new(PromiseState::Pending)
    }
}

/// Map key-value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct MapData {
    pub keys: VecDeque<Value>,
    pub values: Vec<Value>,
}

/// Set value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct SetData {
    pub values: VecDeque<Value>,
}

/// A Proxy value wrapping a target and handler.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyValue {
    pub target: Value,
    pub handler: Value,
    pub revoked: Rc<RefCell<bool>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    BigInt(String),
    Array(Rc<Vec<Value>>),
    Object(Rc<Vec<(String, Value)>>),
    Builtin(Builtin),
    Function(Rc<FunctionValue>),
    BoundFunction(Rc<BoundFunctionValue>),
    Proxy(Rc<ProxyValue>),
    Promise(Rc<PromiseData>),
    Map(Rc<MapData>),
    Set(Rc<SetData>),
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

impl From<&Constant> for Value {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(*value),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::String(value) => Self::String(value.clone()),
            Constant::BigInt(value) => Self::BigInt(value.clone()),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}
