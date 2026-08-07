// Promise microtask queue implementation

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::eval::call_value_with_this;
use crate::value::Value;
use crate::JsError;

thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Value>> = const { RefCell::new(VecDeque::new()) };
}

/// Get all pending microtasks and drain the queue
pub fn get_pending_microtasks() -> Vec<Value> {
    MICROTASK_QUEUE.with(|queue| queue.borrow_mut().drain(..).collect())
}

/// Execute all pending microtasks
pub fn execute_pending_microtasks() -> Result<(), JsError> {
    // Process all microtasks, including those added during callback execution.
    // Keep looping as long as the queue has tasks.
    while !MICROTASK_QUEUE.with(|q| q.borrow().is_empty()) {
        let tasks = get_pending_microtasks();
        for task in tasks {
            if matches!(task, Value::Function(_) | Value::NativeFunction(_)) {
                call_value_with_this(task, vec![], Value::Undefined)?;
            }
        }
    }
    Ok(())
}

/// Queue a microtask callback
pub fn queue_microtask_impl(callback: Value) -> Value {
    if matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        MICROTASK_QUEUE.with(|q| q.borrow_mut().push_back(callback));
    }
    Value::Undefined
}
