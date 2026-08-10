//! Promise implementation with microtask queue.

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
    let state = promise.state.borrow().clone();
    let then_actions = std::mem::take(&mut *promise.then_actions.borrow_mut());
    for (on_fulfilled, on_rejected) in &then_actions {
        let action = match &state {
            PromiseState::Fulfilled(_) => on_fulfilled,
            PromiseState::Rejected(_) => on_rejected,
            PromiseState::Pending => continue,
        };
        if let Some(_handler) = action {
            // Handler would be queued here for later execution
        }
    }
}

/// Create a new pending Promise.
pub fn new_promise() -> Value {
    Value::Promise(Rc::new(PromiseData::default()))
}

fn queue_promise(promise: &Rc<PromiseData>) {
    MICROTASK_QUEUE.with(|queue| queue.borrow_mut().push_back(Rc::clone(promise)));
}

fn set_promise_state(promise: &Rc<PromiseData>, state: PromiseState) {
    let result = match &state {
        PromiseState::Pending => None,
        PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
    };
    *promise.state.borrow_mut() = state;
    *promise.result.borrow_mut() = result;
}

/// Resolve a Promise with a value.
pub fn resolve_promise(promise: &Rc<PromiseData>, value: Value) {
    if !matches!(*promise.state.borrow(), PromiseState::Pending) {
        return;
    }
    set_promise_state(promise, PromiseState::Fulfilled(value));
    queue_promise(promise);
}

/// Reject a Promise with a reason.
pub fn reject_promise(promise: &Rc<PromiseData>, reason: Value) {
    if !matches!(*promise.state.borrow(), PromiseState::Pending) {
        return;
    }
    set_promise_state(promise, PromiseState::Rejected(reason));
    queue_promise(promise);
}

/// Execute Promise.resolve.
pub fn promise_resolve(_arguments: &[Value]) -> Value {
    let value = _arguments.first().cloned().unwrap_or(Value::Undefined);
    Value::Promise(Rc::new(PromiseData::new(PromiseState::Fulfilled(value))))
}

/// Execute Promise.reject.
pub fn promise_reject(_arguments: &[Value]) -> Value {
    let reason = _arguments.first().cloned().unwrap_or(Value::Undefined);
    Value::Promise(Rc::new(PromiseData::new(PromiseState::Rejected(reason))))
}

fn maybe_handler(arguments: &[Value], index: usize) -> Option<Value> {
    arguments
        .get(index)
        .cloned()
        .filter(|value| *value != Value::Undefined)
}

/// Execute Promise.prototype.then.
pub fn promise_then(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Promise(promise)) = receiver {
        promise
            .then_actions
            .borrow_mut()
            .push((maybe_handler(arguments, 0), maybe_handler(arguments, 1)));
        if !matches!(*promise.state.borrow(), PromiseState::Pending) {
            queue_promise(promise);
        }
    }
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
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::Promise => Ok(new_promise()),
        Builtin::PromiseResolve => Ok(promise_resolve(arguments)),
        Builtin::PromiseReject => Ok(promise_reject(arguments)),
        Builtin::PromiseThen => promise_then(receiver, arguments),
        Builtin::PromiseCatch => promise_catch(receiver, arguments),
        Builtin::PromiseFinally => promise_finally(receiver, arguments),
        _ => return None,
    };
    Some(result)
}

const _: () = {
    let _ = drain_microtasks as fn();
    let _ = new_promise as fn() -> Value;
    let _ = resolve_promise as fn(&Rc<PromiseData>, Value);
    let _ = reject_promise as fn(&Rc<PromiseData>, Value);
    let _ = promise_resolve as fn(&[Value]) -> Value;
    let _ = promise_reject as fn(&[Value]) -> Value;
    let _ = promise_then as fn(Option<&Value>, &[Value]) -> Result<Value, VmError>;
    let _ = promise_catch as fn(Option<&Value>, &[Value]) -> Result<Value, VmError>;
    let _ = promise_finally as fn(Option<&Value>, &[Value]) -> Result<Value, VmError>;
    let _ =
        execute_builtin as fn(Builtin, Option<&Value>, &[Value]) -> Option<Result<Value, VmError>>;
};

#[cfg(test)]
mod tests {
    use super::{drain_microtasks, new_promise, promise_then, reject_promise, resolve_promise};
    use crate::value::{PromiseState, Value};

    fn promise_data(value: &Value) -> &std::rc::Rc<crate::value::PromiseData> {
        match value {
            Value::Promise(promise) => promise,
            _ => panic!("expected promise"),
        }
    }

    #[test]
    fn resolve_sets_result_once() {
        let promise = new_promise();
        let data = promise_data(&promise);

        resolve_promise(data, Value::Number(1.0));
        reject_promise(data, Value::Number(2.0));

        assert_eq!(
            data.state.borrow().clone(),
            PromiseState::Fulfilled(Value::Number(1.0))
        );
        assert_eq!(*data.result.borrow(), Some(Value::Number(1.0)));
    }

    #[test]
    fn then_actions_are_consumed_after_settlement() {
        let promise = new_promise();
        let data = promise_data(&promise).clone();

        promise_then(Some(&promise), &[Value::String(String::from("ok"))]).unwrap();
        assert_eq!(data.then_actions.borrow().len(), 1);

        resolve_promise(&data, Value::Boolean(true));
        drain_microtasks();

        assert!(data.then_actions.borrow().is_empty());
        assert_eq!(
            data.state.borrow().clone(),
            PromiseState::Fulfilled(Value::Boolean(true))
        );
    }
}
