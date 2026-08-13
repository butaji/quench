//! Promise implementation with microtask queue.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
};

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{
        PromiseAggregate, PromiseAggregateKind, PromiseContinuation, PromiseData, PromiseState,
        Value,
    },
};

#[path = "promise_constructor.rs"]
mod promise_constructor;

include!("promise_combinators.rs");
include!("promise_finally.rs");
include!("promise_settlement.rs");
include!("promise_with_resolvers.rs");
include!("promise_try.rs");

thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Rc<PromiseData>>> =
        const { RefCell::new(VecDeque::new()) };
    static THEN_RESULTS: RefCell<HashMap<usize, VecDeque<Rc<PromiseData>>>> = RefCell::new(HashMap::new());
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
    let promise_key = Rc::as_ptr(promise) as usize;
    for (on_fulfilled, on_rejected) in then_actions {
        let result_promise = THEN_RESULTS.with(|results| {
            let mut results = results.borrow_mut();
            let queue = results.get_mut(&promise_key)?;
            let result = queue.pop_front();
            if queue.is_empty() {
                results.remove(&promise_key);
            }
            result
        });
        let Some(result_promise) = result_promise else {
            continue;
        };
        let action = match &state {
            PromiseState::Fulfilled(_) => on_fulfilled,
            PromiseState::Rejected(_) => on_rejected,
            PromiseState::Pending => continue,
        };
        let value = match &state {
            PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => value.clone(),
            PromiseState::Pending => continue,
        };
        let Some(handler) = action.filter(crate::conversion::is_callable) else {
            propagate_default(&result_promise, &state, value);
            continue;
        };
        match crate::functions::execute_target(&handler, &Value::Undefined, &[value]) {
            Ok(Value::Promise(next)) => adopt_promise(&result_promise, &next),
            Ok(value) => resolve_promise(&result_promise, value),
            Err(VmError::Thrown(reason)) => reject_promise(&result_promise, reason),
            Err(_) => reject_promise(&result_promise, Value::Undefined),
        }
    }
    let continuations = std::mem::take(&mut *promise.continuations.borrow_mut());
    for continuation in continuations {
        process_continuation(continuation, &state);
    }
}

fn process_continuation(continuation: PromiseContinuation, state: &PromiseState) {
    match continuation {
        PromiseContinuation::Thenable {
            target,
            thenable,
            then,
        } => process_thenable(target, thenable, then),
        PromiseContinuation::Aggregate { aggregate, index } => {
            aggregate_settle(&aggregate, index, state);
        }
        PromiseContinuation::AsyncGenerator { generator, result } => {
            process_async_continuation(generator, result, false, state)
        }
        PromiseContinuation::AsyncGeneratorYield { generator, result } => {
            process_async_continuation(generator, result, true, state)
        }
    }
}

fn process_async_continuation(
    generator: Rc<crate::value::GeneratorData>,
    result: Rc<PromiseData>,
    yielding: bool,
    state: &PromiseState,
) {
    let value = match state {
        PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => value.clone(),
        PromiseState::Pending => return,
    };
    if yielding {
        *generator.pending_yield.borrow_mut() = false;
        finish_async_yield(&generator, &result, state, value);
        return;
    }
    let resume = if matches!(state, PromiseState::Rejected(_)) {
        crate::generator::resume_async_after_await(&generator, true, value)
    } else {
        crate::generator::resume_async_after_await(&generator, false, value)
    };
    match resume {
        Ok(value) => resolve_promise(&result, value),
        Err(VmError::Suspended(awaited)) => {
            register_async_generator(&awaited, generator, result);
        }
        Err(VmError::Thrown(reason)) => reject_promise(&result, reason),
        Err(_) => reject_promise(&result, Value::Undefined),
    }
}

fn process_thenable(target: Rc<PromiseData>, thenable: Value, then: Value) {
    let resolve = bound_settler(Builtin::PromiseResolve, &target);
    let reject = bound_settler(Builtin::PromiseReject, &target);
    match crate::functions::execute_target(&then, &thenable, &[resolve, reject]) {
        Ok(_) => {}
        Err(VmError::Thrown(reason)) => reject_promise(&target, reason),
        Err(_) => reject_promise(&target, Value::Undefined),
    }
}

fn finish_async_yield(
    generator: &Rc<crate::value::GeneratorData>,
    result: &Rc<PromiseData>,
    state: &PromiseState,
    value: Value,
) {
    if matches!(state, PromiseState::Rejected(_)) {
        *generator.done.borrow_mut() = true;
        reject_promise(result, value);
    } else {
        resolve_promise(result, crate::generator::iterator_result(value, false));
    }
}

fn propagate_default(result: &Rc<PromiseData>, state: &PromiseState, value: Value) {
    match state {
        PromiseState::Fulfilled(_) => resolve_promise(result, value),
        PromiseState::Rejected(_) => reject_promise(result, value),
        PromiseState::Pending => {}
    }
}

fn adopt_promise(result: &Rc<PromiseData>, next: &Rc<PromiseData>) {
    if Rc::ptr_eq(result, next) {
        reject_promise(
            result,
            crate::builtins::error(
                Builtin::TypeError,
                &[Value::String(
                    "promise cannot resolve to itself".to_string(),
                )],
            ),
        );
        return;
    }
    match next.state.borrow().clone() {
        PromiseState::Fulfilled(value) => resolve_promise(result, value),
        PromiseState::Rejected(reason) => reject_promise(result, reason),
        PromiseState::Pending => {
            let next_value = Value::Promise(Rc::clone(next));
            let resolve = bound_settler(Builtin::PromiseResolve, result);
            let reject = bound_settler(Builtin::PromiseReject, result);
            let _ = promise_then(Some(&next_value), &[resolve, reject]);
        }
    }
}

/// Create a new pending Promise.
pub fn new_promise() -> Value {
    Value::Promise(Rc::new(PromiseData::default()))
}

/// Convert an async function's completion into its result Promise.
pub(crate) fn from_async_completion(completion: Result<Value, VmError>) -> Value {
    let promise = Rc::new(PromiseData::default());
    match completion {
        Ok(value) => resolve_promise(&promise, value),
        Err(VmError::Thrown(reason)) => reject_promise(&promise, reason),
        Err(VmError::Suspended(_)) => {}
        Err(_) => reject_promise(&promise, Value::Undefined),
    }
    Value::Promise(promise)
}

pub(crate) fn from_async_generator_completion(
    completion: Result<Value, VmError>,
    generator: Rc<crate::value::GeneratorData>,
) -> Value {
    let promise = Rc::new(PromiseData::default());
    match completion {
        Ok(value) => resolve_promise(&promise, value),
        Err(VmError::Suspended(awaited)) => {
            register_async_generator(&awaited, generator, Rc::clone(&promise))
        }
        Err(VmError::Thrown(reason)) => reject_promise(&promise, reason),
        Err(_) => reject_promise(&promise, Value::Undefined),
    }
    Value::Promise(promise)
}

pub(crate) fn register_async_generator(
    awaited: &Rc<PromiseData>,
    generator: Rc<crate::value::GeneratorData>,
    result: Rc<PromiseData>,
) {
    awaited
        .continuations
        .borrow_mut()
        .push(if *generator.pending_yield.borrow() {
            PromiseContinuation::AsyncGeneratorYield { generator, result }
        } else {
            PromiseContinuation::AsyncGenerator { generator, result }
        });
    if !matches!(*awaited.state.borrow(), PromiseState::Pending) {
        queue_promise(awaited);
    }
}

fn queue_promise(promise: &Rc<PromiseData>) {
    MICROTASK_QUEUE.with(|queue| queue.borrow_mut().push_back(Rc::clone(promise)));
}

/// Resolve a Promise with a value.
pub fn resolve_promise(promise: &Rc<PromiseData>, value: Value) {
    if !claim_promise(promise) || !matches!(*promise.state.borrow(), PromiseState::Pending) {
        return;
    }
    set_promise_state(promise, PromiseState::Fulfilled(value));
    queue_promise(promise);
}

/// Reject a Promise with a reason.
pub fn reject_promise(promise: &Rc<PromiseData>, reason: Value) {
    if !claim_promise(promise) || !matches!(*promise.state.borrow(), PromiseState::Pending) {
        return;
    }
    set_promise_state(promise, PromiseState::Rejected(reason));
    queue_promise(promise);
}

/// Execute Promise.resolve using the single canonical promise-resolution path.
pub fn promise_resolve(arguments: &[Value]) -> Value {
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    resolve_value(value)
}

fn resolve_value(value: Value) -> Value {
    if matches!(value, Value::Promise(_)) {
        return value;
    }
    let promise = Rc::new(PromiseData::default());
    let then = match crate::execute::get_property_result(&value, "then") {
        Ok(then) => then,
        Err(VmError::Thrown(reason)) => {
            reject_promise(&promise, reason);
            return Value::Promise(promise);
        }
        Err(_) => {
            reject_promise(&promise, Value::Undefined);
            return Value::Promise(promise);
        }
    };
    if !crate::conversion::is_callable(&then) {
        resolve_promise(&promise, value);
        return Value::Promise(promise);
    }
    promise
        .continuations
        .borrow_mut()
        .push(PromiseContinuation::Thenable {
            target: Rc::clone(&promise),
            thenable: value,
            then,
        });
    queue_promise(&promise);
    Value::Promise(promise)
}

fn bound_settler(target: Builtin, promise: &Rc<PromiseData>) -> Value {
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(target),
        receiver: Value::Promise(Rc::clone(promise)),
        arguments: Vec::new(),
        properties: RefCell::new(Vec::new()),
    }))
}

fn resolve_receiver(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    match receiver {
        Some(Value::Builtin(Builtin::Promise)) => Ok(promise_resolve(arguments)),
        Some(Value::Promise(promise)) => {
            let value = arguments.first().cloned().unwrap_or(Value::Undefined);
            if !claim_promise(promise) {
                return Ok(Value::Undefined);
            }
            if let Value::Promise(other) = &value {
                if Rc::ptr_eq(promise, other) {
                    settle_rejected(
                        promise,
                        crate::builtins::error(
                            Builtin::TypeError,
                            &[Value::String(
                                "promise cannot resolve to itself".to_string(),
                            )],
                        ),
                    );
                    return Ok(Value::Undefined);
                }
            }
            let resolved = resolve_value(value);
            let resolve = bound_settler(Builtin::PromiseAdoptResolve, promise);
            let reject = bound_settler(Builtin::PromiseAdoptReject, promise);
            let _ = promise_then(Some(&resolved), &[resolve, reject])?;
            Ok(Value::Undefined)
        }
        Some(constructor) if crate::conversion::is_callable(constructor) => {
            promise_constructor::resolve(constructor, arguments)
        }
        Some(_) => Err(VmError::NotCallable),
        None => Ok(promise_resolve(arguments)),
    }
}

/// Execute Promise.reject.
pub fn promise_reject(_arguments: &[Value]) -> Value {
    let reason = _arguments.first().cloned().unwrap_or(Value::Undefined);
    Value::Promise(Rc::new(PromiseData::new(PromiseState::Rejected(reason))))
}

fn reject_receiver(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    match receiver {
        Some(Value::Builtin(Builtin::Promise)) => Ok(promise_reject(arguments)),
        Some(Value::Promise(promise)) => {
            if claim_promise(promise) {
                settle_rejected(
                    promise,
                    arguments.first().cloned().unwrap_or(Value::Undefined),
                );
            }
            Ok(Value::Undefined)
        }
        Some(_) => Err(VmError::NotCallable),
        None => Ok(promise_reject(arguments)),
    }
}

pub(crate) fn construct_promise(executor: &Value) -> Result<Value, VmError> {
    let promise = new_promise();
    let resolve = Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(Builtin::PromiseResolve),
        receiver: promise.clone(),
        arguments: Vec::new(),
        properties: RefCell::new(Vec::new()),
    }));
    let reject = Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(Builtin::PromiseReject),
        receiver: promise.clone(),
        arguments: Vec::new(),
        properties: RefCell::new(Vec::new()),
    }));
    crate::functions::execute_target(executor, &Value::Undefined, &[resolve, reject])?;
    Ok(promise)
}

fn maybe_handler(arguments: &[Value], index: usize) -> Option<Value> {
    arguments
        .get(index)
        .cloned()
        .filter(|value| *value != Value::Undefined)
}

/// Execute Promise.prototype.then.
pub fn promise_then(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Promise(promise)) = receiver else {
        return Err(VmError::NotCallable);
    };
    let result = new_promise();
    let result_promise = match &result {
        Value::Promise(promise) => Rc::clone(promise),
        _ => return Err(VmError::NotCallable),
    };
    promise
        .then_actions
        .borrow_mut()
        .push((maybe_handler(arguments, 0), maybe_handler(arguments, 1)));
    let promise_key = Rc::as_ptr(promise) as usize;
    THEN_RESULTS.with(|results| {
        results
            .borrow_mut()
            .entry(promise_key)
            .or_default()
            .push_back(result_promise);
    });
    if !matches!(*promise.state.borrow(), PromiseState::Pending) {
        queue_promise(promise);
    }
    Ok(result)
}

/// Execute Promise.prototype.catch.
pub fn promise_catch(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(VmError::NotCallable);
    };
    let then = crate::execute::get_property_result(receiver, "then")?;
    if !crate::conversion::is_callable(&then) {
        return Err(crate::vm::not_callable());
    }
    crate::functions::execute_target(
        &then,
        receiver,
        &[
            Value::Undefined,
            arguments.first().cloned().unwrap_or(Value::Undefined),
        ],
    )
}

/// Dispatch Promise builtins.
pub fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::Promise => Ok(new_promise()),
        Builtin::PromiseResolve => resolve_receiver(receiver, arguments),
        Builtin::PromiseReject => reject_receiver(receiver, arguments),
        Builtin::PromiseAll => promise_combinator(PromiseAggregateKind::All, arguments),
        Builtin::PromiseAllSettled => {
            promise_combinator(PromiseAggregateKind::AllSettled, arguments)
        }
        Builtin::PromiseAny => promise_combinator(PromiseAggregateKind::Any, arguments),
        Builtin::PromiseRace => promise_combinator(PromiseAggregateKind::Race, arguments),
        Builtin::PromiseWithResolvers => with_resolvers(receiver),
        Builtin::PromiseTry => promise_try(receiver, arguments),
        Builtin::PromiseThen => promise_then(receiver, arguments),
        Builtin::PromiseCatch => promise_catch(receiver, arguments),
        Builtin::PromiseFinally => promise_finally(receiver, arguments),
        Builtin::PromiseFinallyFulfilled => settle_finally_value(true, receiver),
        Builtin::PromiseFinallyRejected => settle_finally_value(false, receiver),
        Builtin::PromiseFinallyOnFulfilled => execute_finally_handler(
            true,
            receiver,
            arguments.first().cloned().unwrap_or(Value::Undefined),
        ),
        Builtin::PromiseFinallyOnRejected => execute_finally_handler(
            false,
            receiver,
            arguments.first().cloned().unwrap_or(Value::Undefined),
        ),
        Builtin::PromiseAdoptResolve => adopt_resolve(receiver, arguments),
        Builtin::PromiseAdoptReject => adopt_reject(receiver, arguments),
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
