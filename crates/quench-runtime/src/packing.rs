//! Compact storage primitives — organized so each category can be packed independently.
//!
//! The four categories are:
//!
//! - **Immediate values**: inline, no heap allocation, stored directly in a register.
//!   `Number`, `Boolean`, `Null`, `Undefined`, `Builtin`.
//!
//! - **Heap references**: pointers to shared heap-allocated data.
//!   `String`, `Array`, `Object`, `Function`, `BoundFunction`, `Proxy`, `Promise`, `Map`, `Set`,
//!   `ModuleNamespace`.
//!
//! - **Frames**: the runtime state for one suspended function call.
//!   Contains registers, a program counter, and lexical environment.
//!
//! - **Continuations**: the full call-stack used to suspend and resume execution.
//!   A `Continuation` holds the active frames and the current program counter.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::ops::{Builtin, Op};

// ---------------------------------------------------------------------------
// Immediate values — small, inline, no heap allocation.
// ---------------------------------------------------------------------------

/// Unboxed immediate values. Stored directly in a register slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Immediate {
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
    Builtin(Builtin),
}

// ---------------------------------------------------------------------------
// Heap references — shared pointers to heap-allocated runtime data.
// ---------------------------------------------------------------------------

/// A reference into the managed heap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapRef(u32);

impl HeapRef {
    pub const INVALID: Self = Self(u32::MAX);
    pub fn index(&self) -> u32 { self.0 }
    pub fn is_invalid(&self) -> bool { self.0 == u32::MAX }
}

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
    pub state: PromiseState,
    pub result: Option<Value>,
    pub then_actions: Vec<(Option<Value>, Option<Value>)>,
}

impl Default for PromiseData {
    fn default() -> Self {
        Self { state: PromiseState::Pending, result: None, then_actions: Vec::new() }
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

// ---------------------------------------------------------------------------
// Full runtime value — the union of immediates and heap references.
// ---------------------------------------------------------------------------

/// Machine-sized runtime value for the residual kernel.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    // Immediates — no heap allocation.
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
    Builtin(Builtin),
    // Heap references — shared pointers.
    String(String),
    Array(Rc<Vec<Value>>),
    Object(Rc<Vec<(String, Value)>>),
    Function(Rc<FunctionValue>),
    BoundFunction(Rc<BoundFunctionValue>),
    Proxy(Rc<ProxyValue>),
    Promise(Rc<PromiseData>),
    Map(Rc<MapData>),
    Set(Rc<SetData>),
    ModuleNamespace(Rc<Vec<(String, Value)>>),
}

/// The closure environment associated with a function.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionValue {
    pub body: Vec<Op>,
    pub params: u16,
    pub env_idx: u16,
    pub captures: Rc<RefCell<Vec<Value>>>,
    pub properties: Rc<RefCell<Vec<(String, Value)>>>,
}

/// A bound-function value created by `Function.prototype.bind`.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundFunctionValue {
    pub target: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
}

/// A Proxy value wrapping a target and handler.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyValue {
    pub target: Value,
    pub handler: Value,
}

impl From<&crate::ops::Constant> for Value {
    fn from(value: &crate::ops::Constant) -> Self {
        match value {
            crate::ops::Constant::Number(value) => Self::Number(*value),
            crate::ops::Constant::Boolean(value) => Self::Boolean(*value),
            crate::ops::Constant::String(value) => Self::String(value.clone()),
            crate::ops::Constant::Null => Self::Null,
            crate::ops::Constant::Undefined => Self::Undefined,
        }
    }
}

// ---------------------------------------------------------------------------
// Frames — the runtime state for one suspended function call.
// ---------------------------------------------------------------------------

/// The runtime state for one suspended function call.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub registers: Vec<Value>,
    pub ops: Vec<Op>,
    pub pc: usize,
    pub captures: Rc<RefCell<Vec<Value>>>,
    pub param_count: u16,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            registers: Vec::new(),
            ops: Vec::new(),
            pc: 0,
            captures: Rc::new(RefCell::new(Vec::new())),
            param_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Continuations — the full call-stack used to suspend and resume execution.
// ---------------------------------------------------------------------------

/// A continuation captures the full call-stack needed to resume execution.
#[derive(Debug, Clone, PartialEq)]
pub enum Continuation {
    Done(Value),
    Active { frames: Vec<Frame>, frame_idx: usize },
}

impl Default for Continuation {
    fn default() -> Self { Self::Done(Value::Undefined) }
}

impl Continuation {
    pub fn active_frame(&self) -> Option<&Frame> {
        match self {
            Continuation::Active { frames, frame_idx } => frames.get(*frame_idx),
            Continuation::Done(_) => None,
        }
    }

    pub fn active_frame_mut(&mut self) -> Option<&mut Frame> {
        match self {
            Continuation::Active { frames, frame_idx } => frames.get_mut(*frame_idx),
            Continuation::Done(_) => None,
        }
    }
}
