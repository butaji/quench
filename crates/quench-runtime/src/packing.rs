//! Compact storage primitives — organized so each category can be packed independently.
//!
//! The four categories are:
//!
//! - **Immediate values**: inline, no heap allocation, stored directly in a register.

#![allow(dead_code)]
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

use std::{cell::RefCell, rc::Rc};

use crate::{ops::Op, value::Value};

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
    Builtin(crate::ops::Builtin),
}

// ---------------------------------------------------------------------------
// Heap references — shared pointers to heap-allocated runtime data.
// ---------------------------------------------------------------------------

/// A reference into the managed heap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapRef(u32);

impl HeapRef {
    pub const INVALID: Self = Self(u32::MAX);
    pub fn index(&self) -> u32 {
        self.0
    }
    pub fn is_invalid(&self) -> bool {
        self.0 == u32::MAX
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
    Active {
        frames: Vec<Frame>,
        frame_idx: usize,
    },
}

impl Default for Continuation {
    fn default() -> Self {
        Self::Done(Value::Undefined)
    }
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
