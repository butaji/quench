//! Machine state for the explicit-stack interpreter.

use std::rc::Rc;
use std::cell::RefCell;

use crate::env::Environment;
use crate::stack_machine::work::Work;
use crate::Value;

/// Stack machine for executing JavaScript with an explicit call stack.
pub struct Machine {
    pub frames: Vec<Frame>,
}

impl Machine {
    /// Create a new machine with a single frame using the given environment.
    pub fn new(env: Rc<RefCell<Environment>>) -> Self {
        Machine {
            frames: vec![Frame {
                env,
                values: Vec::new(),
                work: Vec::new(),
                catches: Vec::new(),
            }],
        }
    }
}

/// A call frame in the explicit stack.
pub struct Frame {
    pub env: Rc<RefCell<Environment>>,
    /// Operand / result stack for this frame.
    pub values: Vec<Value>,
    /// Continuation stack (LIFO).
    pub work: Vec<Work>,
    /// Active try-catch handlers in this frame, innermost last.
    pub catches: Vec<CatchFrame>,
}

/// A catch frame for exception handling.
pub struct CatchFrame {
    pub handler: Rc<crate::ast::Statement>,
    pub param: Option<String>,
    pub env: Rc<RefCell<Environment>>,
    pub is_expr_body: bool,
}
