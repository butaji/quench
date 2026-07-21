//! Generator function state management
//!
//! This module provides the infrastructure for JavaScript generator functions.
//! When a generator function is called, it returns a generator object that can
//! be resumed with .next(), .throw(), or .return().

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::env::Environment;
use crate::value::Value;

/// Generator state - stores the execution state of a suspended generator.
#[derive(Debug)]
#[allow(dead_code)]
pub struct GeneratorState {
    /// Saved environment for resume
    pub env: Rc<RefCell<Environment>>,
    /// Saved program counter (index into body)
    pub pc: usize,
    /// Saved local variables
    pub locals: HashMap<String, Value>,
    /// Whether generator has started
    pub started: bool,
    /// Whether generator is closed
    pub closed: bool,
    /// Value to return from the yield expression (set by .next(value))
    pub yield_value: Value,
}

impl GeneratorState {
    /// Create a new generator state for a generator function body
    pub fn new(env: Rc<RefCell<Environment>>) -> Self {
        Self {
            env,
            pc: 0,
            locals: HashMap::new(),
            started: false,
            closed: false,
            yield_value: Value::Undefined,
        }
    }
}

thread_local! {
    static CURRENT_GENERATOR: RefCell<Option<Rc<RefCell<GeneratorState>>>> = const { RefCell::new(None) };
}

/// Set the current generator state (called when entering generator evaluation)
pub fn set_current_generator(gen: Option<Rc<RefCell<GeneratorState>>>) {
    CURRENT_GENERATOR.with(|cell| *cell.borrow_mut() = gen);
}

/// Get the current generator state (called when yielding)
pub fn get_current_generator() -> Option<Rc<RefCell<GeneratorState>>> {
    CURRENT_GENERATOR.with(|cell| cell.borrow().clone())
}

/// Take the current generator state (used when closing the generator)
pub fn take_current_generator() -> Option<Rc<RefCell<GeneratorState>>> {
    CURRENT_GENERATOR.with(|cell| cell.take())
}

/// Yield from the current generator, returning the yield value.
/// This suspends the generator and returns ControlFlow::Yield(value).
#[allow(dead_code, clippy::result_large_err)]
pub(crate) fn yield_value(value: Value) -> Result<Value, crate::interpreter::ControlFlow> {
    let gen = get_current_generator();
    if let Some(gen_rc) = gen {
        // Store the yield value so .next() can return it
        gen_rc.borrow_mut().yield_value = value.clone();
        // Signal yield to the call loop
        Err(crate::interpreter::ControlFlow::Yield(value))
    } else {
        #[allow(clippy::match_same_arms)]
        Err(crate::interpreter::ControlFlow::Return(Value::Undefined))
    }
}

/// Yield* delegation - yields all values from an iterator.
/// Returns ControlFlow::YieldDelegate(iter) to signal delegation.
#[allow(dead_code, clippy::result_large_err)]
pub(crate) fn yield_delegate(iter: Value) -> Result<Value, crate::interpreter::ControlFlow> {
    Err(crate::interpreter::ControlFlow::YieldDelegate(iter))
}
