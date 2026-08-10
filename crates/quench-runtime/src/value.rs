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

/// Shared byte storage for ArrayBuffer and typed-array views.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayBufferData {
    pub bytes: Rc<RefCell<Vec<u8>>>,
    pub detached: Rc<RefCell<bool>>,
}

impl ArrayBufferData {
    pub fn new(byte_length: usize) -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![0; byte_length])),
            detached: Rc::new(RefCell::new(false)),
        }
    }

    pub fn byte_length(&self) -> usize {
        if *self.detached.borrow() {
            0
        } else {
            self.bytes.borrow().len()
        }
    }

    pub fn detach(&self) {
        *self.detached.borrow_mut() = true;
        self.bytes.borrow_mut().clear();
    }
}

/// A Float64Array view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Float64ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

impl Float64ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<f64>();

    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            length,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.length * Self::BYTES_PER_ELEMENT
    }

    pub fn get(&self, index: usize) -> Option<f64> {
        if index >= self.length || self.buffer.byte_length() < self.byte_offset + self.byte_length()
        {
            return None;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let bytes = self.buffer.bytes.borrow();
        let data: [u8; Self::BYTES_PER_ELEMENT] = bytes.get(start..end)?.try_into().ok()?;
        Some(f64::from_ne_bytes(data))
    }

    pub fn set(&self, index: usize, value: f64) -> bool {
        if index >= self.length || self.buffer.byte_length() < self.byte_offset + self.byte_length()
        {
            return false;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(destination) = bytes.get_mut(start..end) else {
            return false;
        };
        destination.copy_from_slice(&value.to_ne_bytes());
        true
    }
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
