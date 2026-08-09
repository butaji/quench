//! Promise implementation with microtask queue.

#![allow(dead_code)]

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{PromiseData, PromiseState, Value},
};

thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Rc<PromiseData>>> =
        const { RefCell::new(VecDeque::new()) };
}

/// Drains all queued microtasks.
pub(crate) fn drain_microtasks() {
    while let Some(promise) = MICROTASK_QUEUE.with(|q| q.borrow_mut().pop_front()) {
        process_promise(&promise);
    }
}

fn process_promise(promise: &Rc<PromiseData>) {
    let state = promise.state.clone();
    let result = promise.result.clone();
    let then_actions = promise.then_actions.clone();
    if !matches!(state, PromiseState::Pending) {
        return;
    }
    for (on_fulfilled, on_rejected) in then_actions.iter() {
        let action = match &result {
            Some(_) => on_fulfilled,
            None => on_rejected,
        };
        if let Some(_handler) = action {
            // Handler would be queued here for later execution
        }
    }
}

/// Create a new pending Promise.
pub fn new_promise() -> Value {
    Value::Promise(Rc::new(PromiseData {
        state: PromiseState::Pending,
        result: None,
        then_actions: Vec::new(),
    }))
}

/// Resolve a Promise with a value.
pub fn resolve_promise(promise: &Rc<PromiseData>, value: Value) {
    // Since PromiseData doesn't use RefCell, we need interior mutability
    // or a different approach. For now, this is a stub.
    let _ = (promise, value);
}

/// Reject a Promise with a reason.
pub fn reject_promise(promise: &Rc<PromiseData>, reason: Value) {
    let _ = (promise, reason);
}

/// Execute Promise.resolve.
pub fn promise_resolve(_arguments: &[Value]) -> Value {
    let value = _arguments.first().cloned().unwrap_or(Value::Undefined);
    Value::Promise(Rc::new(PromiseData {
        state: PromiseState::Fulfilled(value),
        result: None,
        then_actions: Vec::new(),
    }))
}

/// Execute Promise.reject.
pub fn promise_reject(_arguments: &[Value]) -> Value {
    let reason = _arguments.first().cloned().unwrap_or(Value::Undefined);
    Value::Promise(Rc::new(PromiseData {
        state: PromiseState::Rejected(reason),
        result: None,
        then_actions: Vec::new(),
    }))
}

/// Execute Promise.prototype.then.
pub fn promise_then(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let _on_fulfilled = arguments.first().cloned().unwrap_or(Value::Undefined);
    let _on_rejected = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    Ok(new_promise())
}

/// Execute Promise.prototype.catch.
pub fn promise_catch(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let _on_rejected = arguments.first().cloned().unwrap_or(Value::Undefined);
    Ok(new_promise())
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
    None
}
