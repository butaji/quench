//! Explicit interpreter activation records.
//!
//! Ordinary calls are being migrated to this representation incrementally.  Keeping
//! the record independent from the execution driver lets cold call paths continue
//! using their existing continuations while the synchronous path moves to a loop.
use std::rc::Rc;

use crate::environment::Environment;
use crate::execute::VmError;
use crate::value::{FunctionValue, Value};

/// Hard bound for interpreter activations.  The check is performed before a new
/// record is allocated, so exhaustion is a regular catchable JS RangeError.
pub(crate) const MAX_FRAMES: usize = 40_000;

#[derive(Debug)]
pub(crate) struct CallRequest {
    pub(crate) function: Rc<FunctionValue>,
    pub(crate) receiver: Value,
    pub(crate) arguments: Vec<Value>,
    pub(crate) return_destination: u16,
    pub(crate) caller_resume_pc: usize,
    pub(crate) caller_registers: Vec<Value>,
}

#[derive(Debug)]
pub(crate) struct Activation {
    pub(crate) function: Option<Rc<FunctionValue>>,
    pub(crate) pc: usize,
    pub(crate) registers: Vec<Value>,
    pub(crate) environment: Rc<Environment>,
    pub(crate) receiver: Value,
    pub(crate) return_destination: Option<u16>,
    pub(crate) caller_continuation: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct VmCallStack {
    frames: Vec<Activation>,
}

impl VmCallStack {
    #[inline]
    pub(crate) fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.frames.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub(crate) fn push(&mut self, activation: Activation) -> Result<(), VmError> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(crate::value::error::throw_range_error(
                "Maximum call stack size exceeded",
            ));
        }
        self.frames.push(activation);
        Ok(())
    }
    pub(crate) fn pop(&mut self) -> Option<Activation> {
        self.frames.pop()
    }
    pub(crate) fn current(&self) -> Option<&Activation> {
        self.frames.last()
    }
    pub(crate) fn current_mut(&mut self) -> Option<&mut Activation> {
        self.frames.last_mut()
    }
}

/// Result of one interpreter operation. `Continue` advances the current
/// activation, `Push` transfers control to an ordinary interpreted callee, and
/// `Return` transfers control to its caller.
#[derive(Debug)]
pub(crate) enum Transition {
    Continue,
    Push(CallRequest),
    Return(crate::completion::Completion),
}
