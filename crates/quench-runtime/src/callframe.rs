//! Explicit call frames for the HIR/trampoline interpreter.
//!
//! Each frame holds local registers, the current basic block, the program
//! counter (instruction index), the `this` binding, and the return target.
//! Frames are stored on an explicit stack in the interpreter so that deep
//! recursion no longer grows the native Rust stack.

use crate::hir::BlockId;
use crate::value::Value;

/// Opaque handle to a frame slot in the interpreter's frame stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FrameId(pub usize);

/// A single call frame in the explicit interpreter stack.
#[derive(Debug)]
pub struct CallFrame {
    /// Local register storage. Index 0 is reserved for the return value / accumulator.
    pub locals: Vec<Value>,
    /// Index of the currently executing basic block.
    pub block: BlockId,
    /// Index of the next instruction to execute within the block.
    pub pc: usize,
    /// Bound `this` value for the current call.
    pub this: Value,
    /// Frame to return to when this call completes.
    pub caller: Option<FrameId>,
    /// Local index to write the return value into in the caller frame.
    pub return_slot: Option<usize>,
}

impl CallFrame {
    /// Create a new frame with space for `local_count` locals.
    pub fn new(
        local_count: usize,
        this: Value,
        caller: Option<FrameId>,
        return_slot: Option<usize>,
    ) -> Self {
        let mut locals = Vec::with_capacity(local_count);
        for _ in 0..local_count {
            locals.push(Value::Undefined);
        }
        CallFrame {
            locals,
            block: BlockId(0),
            pc: 0,
            this,
            caller,
            return_slot,
        }
    }

    /// Read a local register.
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.locals.get(index)
    }

    /// Write a local register.
    pub fn set(&mut self, index: usize, value: Value) {
        if let Some(slot) = self.locals.get_mut(index) {
            *slot = value;
        }
    }

    /// Set the bound `this` value.
    pub fn set_this(&mut self, this: Value) {
        self.this = this;
    }
}

/// Explicit stack of call frames used by the HIR interpreter.
#[derive(Debug, Default)]
pub struct FrameStack {
    frames: Vec<CallFrame>,
}

impl FrameStack {
    /// Create an empty frame stack.
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Push a frame and return its id.
    pub fn push(&mut self, frame: CallFrame) -> FrameId {
        let id = FrameId(self.frames.len());
        self.frames.push(frame);
        id
    }

    /// Borrow the frame at `id`.
    pub fn get(&self, id: FrameId) -> Option<&CallFrame> {
        self.frames.get(id.0)
    }

    /// Borrow the frame at `id` mutably.
    pub fn get_mut(&mut self, id: FrameId) -> Option<&mut CallFrame> {
        self.frames.get_mut(id.0)
    }

    /// Pop the top frame.
    pub fn pop(&mut self) -> Option<CallFrame> {
        self.frames.pop()
    }

    /// Whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Number of frames on the stack.
    pub fn len(&self) -> usize {
        self.frames.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_locals() {
        let mut frame = CallFrame::new(3, Value::Undefined, None, None);
        frame.set(1, Value::Number(42.0));
        assert_eq!(frame.get(1), Some(&Value::Number(42.0)));
    }

    #[test]
    fn test_frame_stack() {
        let mut stack = FrameStack::new();
        let id = stack.push(CallFrame::new(1, Value::Undefined, None, None));
        assert_eq!(stack.len(), 1);
        assert!(stack.get(id).is_some());
        assert!(stack.pop().is_some());
        assert!(stack.is_empty());
    }
}
