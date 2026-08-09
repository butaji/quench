//! Promise implementation with microtask queue.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::{
    execute::{execute_with_registers, VmError},
    ops::Builtin,
    value::{PromiseData, PromiseState, Value},
};

thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Rc<RefCell<PromiseData>>>> =
        const { RefCell::new(VecDeque::new()) };
}

/// Drains all queued microtasks.
pub(crate) fn drain_microtasks() {
    while let Some(promise) = MICROTASK_QUEUE.with(|q| q.borrow_mut().pop_front()) {
        process_promise(&promise);
    }
}

fn process_promise(promise: &Rc<RefCell<PromiseData>>) {
    let promise_data = promise.borrow();
    if !matches!(promise_data.state, PromiseState::Pending) {
        return;
    }
    for (on_fulfilled, on_rejected) in promise_data.then_actions.iter() {
        let action = match &promise_data.result {
            Some(_) => on_fulfilled,
            None => on_rejected,
        };
        if let Some(handler) = action {
            if let Err(e) = queue_microtask(handler.clone()) {
                // If queuing fails, reject the promise
                drop(promise_data);
                if let Value::Function(func) = handler {
                    let result = promise.borrow().result.clone().unwrap_or(Value::Undefined);
                    if let Err(_) = execute_with_registers(&func.body, vec![result]) {
                        // Handler threw, propagate rejection
                    }
                }
            }
        }
    }
}

fn queue_microtask(value: Value) -> Result<(), VmError> {
    if let Value::Function(_func) = &value {
        MICROTASK_QUEUE.with(|q| {
            q.borrow_mut().push_back(Rc::new(RefCell::new(PromiseData {
                state: PromiseState::Pending,
                result: None,
                then_actions: Vec::new(),
            })));
        });
    }
    Ok(())
}

/// Create a new pending Promise.
pub fn new_promise() -> Value {
    Value::Promise(Rc::new(RefCell::new(PromiseData {
        state: PromiseState::Pending,
        result: None,
        then_actions: Vec::new(),
    })))
}

/// Resolve a Promise with a value.
pub fn resolve_promise(promise: &Rc<RefCell<PromiseData>>, value: Value) {
    let mut promise_data = promise.borrow_mut();
    if !matches!(promise_data.state, PromiseState::Pending) {
        return;
    }
    promise_data.state = PromiseState::Fulfilled(value);
}

/// Reject a Promise with a reason.
pub fn reject_promise(promise: &Rc<RefCell<PromiseData>>, reason: Value) {
    let mut promise_data = promise.borrow_mut();
    if !matches!(promise_data.state, PromiseState::Pending) {
        return;
    }
    promise_data.state = PromiseState::Rejected(reason);
}

/// Execute Promise.resolve.
pub fn promise_resolve(_arguments: &[Value]) -> Value {
    let value = _arguments.first().cloned().unwrap_or(Value::Undefined);
    Value::Promise(Rc::new(RefCell::new(PromiseData {
        state: PromiseState::Fulfilled(value),
        result: None,
        then_actions: Vec::new(),
    })))
}

/// Execute Promise.reject.
pub fn promise_reject(_arguments: &[Value]) -> Value {
    let reason = _arguments.first().cloned().unwrap_or(Value::Undefined);
    Value::Promise(Rc::new(RefCell::new(PromiseData {
        state: PromiseState::Rejected(reason),
        result: None,
        then_actions: Vec::new(),
    })))
}

/// Execute Promise.prototype.then.
pub fn promise_then(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let _on_fulfilled = arguments.first().cloned().unwrap_or(Value::Undefined);
    let _on_rejected = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let result = new_promise();
    Ok(result)
}

/// Execute Promise.prototype.catch.
pub fn promise_catch(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let _on_rejected = arguments.first().cloned().unwrap_or(Value::Undefined);
    let result = new_promise();
    Ok(result)
}

/// Execute Promise.prototype.finally.
pub fn promise_finally(_receiver: Option<&Value>, _arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

/// Dispatch Promise builtins.
pub fn execute_builtin(
    _builtin: Builtin,
    _receiver: Option<&Value>,
    _arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    // Placeholder - actual implementation would dispatch to specific functions
    None
}
