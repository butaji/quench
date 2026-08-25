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
include!("promise_drain.rs");
include!("promise_settlement.rs");
include!("promise_with_resolvers.rs");
include!("promise_try.rs");

thread_local! {
    static OBJECT_PROMISE_BACKINGS: RefCell<HashMap<u64, Rc<PromiseData>>> = RefCell::new(HashMap::new());
}

pub(super) fn attach_promise_data(value: Value, capability: Rc<PromiseData>) -> Value {
    if let Value::Object(object) = &value {
        OBJECT_PROMISE_BACKINGS.with(|backings| {
            backings.borrow_mut().insert(object.identity(), capability);
        });
    }
    value
}

fn object_promise_backing(value: &Value) -> Option<Rc<PromiseData>> {
    let Value::Object(object) = value else {
        return None;
    };
    OBJECT_PROMISE_BACKINGS.with(|backings| backings.borrow().get(&object.identity()).cloned())
}

fn process_promise(promise: &Rc<PromiseData>) {
    let state = promise.state.borrow().clone();
    let then_actions = std::mem::take(&mut *promise.then_actions.borrow_mut());
    let promise_key = Rc::as_ptr(promise) as usize;
    process_then_actions(then_actions, &state, promise_key);
    let continuations = std::mem::take(&mut *promise.continuations.borrow_mut());
    for continuation in continuations {
        if matches!(state, PromiseState::Pending)
            && matches!(&continuation, PromiseContinuation::Aggregate { .. })
        {
            promise.continuations.borrow_mut().push(continuation);
            continue;
        }
        process_continuation(continuation, &state);
    }
}

fn process_then_actions(
    then_actions: Vec<(Option<Value>, Option<Value>)>,
    state: &PromiseState,
    promise_key: usize,
) {
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
        let Some(handler) = action
            .map(peel_binding_cell)
            .filter(crate::conversion::is_callable)
        else {
            propagate_default(&result_promise, state, value);
            continue;
        };
        match crate::functions::execute_target(&handler, &Value::Undefined, &[value]) {
            Ok(Value::Promise(next)) => adopt_promise(&result_promise, &next),
            Ok(value) => resolve_promise(&result_promise, value),
            Err(VmError::Thrown(reason)) => reject_promise(&result_promise, reason),
            Err(_) => reject_promise(&result_promise, Value::Undefined),
        }
    }
}

fn peel_binding_cell(mut value: Value) -> Value {
    let mut seen = std::collections::HashSet::new();
    loop {
        let Value::BindingCell(cell) = value else {
            return value;
        };
        if !seen.insert(std::rc::Rc::as_ptr(&cell)) {
            return Value::BindingCell(cell);
        }
        value = cell.load();
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
        PromiseContinuation::AsyncGenerator {
            generator,
            result,
            async_function,
        } => process_async_continuation(generator, result, false, async_function, state),
        PromiseContinuation::AsyncGeneratorYield { generator, result } => {
            process_async_continuation(generator, result, true, false, state)
        }
    }
}

fn process_async_continuation(
    generator: Rc<crate::value::GeneratorData>,
    result: Rc<PromiseData>,
    yielding: bool,
    async_function: bool,
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
        Ok(value) => resolve_promise(&result, async_result_value(value, async_function)),
        Err(VmError::Suspended(awaited)) => {
            register_async_generator(&awaited, generator, result, async_function);
        }
        Err(VmError::Thrown(reason)) => reject_promise(&result, reason),
        Err(_) => reject_promise(&result, Value::Undefined),
    }
}

fn process_thenable(target: Rc<PromiseData>, thenable: Value, then: Value) {
    let resolve = bound_settler(Builtin::PromiseResolve, &target, 1.0);
    let reject = bound_settler(Builtin::PromiseReject, &target, 1.0);
    let then = peel_binding_cell(then);
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
            let resolve = bound_settler(Builtin::PromiseResolve, result, 1.0);
            let reject = bound_settler(Builtin::PromiseReject, result, 1.0);
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
    settle_async_generator_completion(completion, generator, Rc::clone(&promise), false);
    Value::Promise(promise)
}

pub(crate) fn from_async_function_completion(
    completion: Result<Value, VmError>,
    generator: Rc<crate::value::GeneratorData>,
) -> Value {
    let promise = Rc::new(PromiseData::default());
    settle_async_generator_completion(completion, generator, Rc::clone(&promise), true);
    Value::Promise(promise)
}

pub(crate) fn settle_async_generator_completion(
    completion: Result<Value, VmError>,
    generator: Rc<crate::value::GeneratorData>,
    promise: Rc<PromiseData>,
    async_function: bool,
) {
    match completion {
        Ok(value) => resolve_promise(&promise, async_result_value(value, async_function)),
        Err(VmError::Suspended(awaited)) => {
            register_async_generator(&awaited, generator, promise, async_function)
        }
        Err(VmError::Thrown(reason)) => reject_promise(&promise, reason),
        Err(_) => reject_promise(&promise, Value::Undefined),
    }
}

pub(crate) fn register_async_generator(
    awaited: &Rc<PromiseData>,
    generator: Rc<crate::value::GeneratorData>,
    result: Rc<PromiseData>,
    async_function: bool,
) {
    awaited
        .continuations
        .borrow_mut()
        .push(if *generator.pending_yield.borrow() {
            PromiseContinuation::AsyncGeneratorYield { generator, result }
        } else {
            PromiseContinuation::AsyncGenerator {
                generator,
                result,
                async_function,
            }
        });
    if !matches!(*awaited.state.borrow(), PromiseState::Pending) {
        queue_promise(awaited);
    }
}

fn async_result_value(value: Value, async_function: bool) -> Value {
    if async_function {
        crate::execute::get_property_result(&value, "value").unwrap_or(Value::Undefined)
    } else {
        value
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
    if let Value::Promise(promise) = &value {
        let constructor = crate::execute::get_property_result(&value, "constructor").ok();
        if constructor.as_ref().is_some_and(|constructor| {
            crate::builtins::same_value(Some(constructor), Some(&Value::Builtin(Builtin::Promise)))
        }) {
            return Value::Promise(Rc::clone(promise));
        }
        return resolve_object_value(Rc::new(PromiseData::default()), value);
    }
    resolve_value(value)
}

fn resolve_value(value: Value) -> Value {
    if matches!(value, Value::Promise(_)) {
        return value;
    }
    let promise = Rc::new(PromiseData::default());
    if !crate::value::is_object(&value) {
        resolve_promise(&promise, value);
        return Value::Promise(promise);
    }
    resolve_object_value(promise, value)
}

fn resolve_object_value(promise: Rc<PromiseData>, value: Value) -> Value {
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

fn bound_settler(target: Builtin, promise: &Rc<PromiseData>, length: f64) -> Value {
    let length = Value::Number(length);
    let name = String::new();
    let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), length.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let name_descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), Value::String(name.clone())),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: crate::vm::current_context_or_default().realm(),
        target: Value::Builtin(target),
        receiver: Value::Promise(Rc::clone(promise)),
        arguments: Vec::new(),
        properties: RefCell::new(vec![
            ("length".to_string(), length),
            (crate::builtins::descriptor_key("length"), descriptor),
            ("name".to_string(), Value::String(name)),
            (crate::builtins::descriptor_key("name"), name_descriptor),
            (
                "\0realm".to_string(),
                crate::vm::realm_token(crate::vm::current_context_or_default().realm())
                    .unwrap_or(Value::Undefined),
            ),
        ]),
    }))
}

pub(crate) fn construct_promise(executor: &Value) -> Result<Value, VmError> {
    if !crate::conversion::is_callable(executor) {
        return Err(VmError::NotCallable);
    }
    let promise = new_promise();
    let Value::Promise(promise_data) = &promise else {
        return Err(VmError::NotCallable);
    };
    let resolve = bound_settler(Builtin::PromiseResolve, promise_data, 1.0);
    let reject = bound_settler(Builtin::PromiseReject, promise_data, 1.0);
    let receiver = match executor {
        Value::Function(function)
            if matches!(function.strictness, crate::ops::FunctionStrictness::Sloppy) =>
        {
            crate::vm::current_global_object()
        }
        _ => Value::Undefined,
    };
    if let Err(VmError::Thrown(reason)) =
        crate::functions::execute_target(executor, &receiver, &[resolve, reject])
    {
        reject_promise(promise_data, reason);
    }
    Ok(promise)
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
            let resolve = bound_settler(Builtin::PromiseAdoptResolve, promise, 1.0);
            let reject = bound_settler(Builtin::PromiseAdoptReject, promise, 1.0);
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
        Some(constructor) if crate::conversion::is_callable(constructor) => {
            promise_constructor::reject(constructor, arguments)
        }
        Some(_) => Err(VmError::NotCallable),
        None => Ok(promise_reject(arguments)),
    }
}

fn maybe_handler(arguments: &[Value], index: usize) -> Option<Value> {
    arguments
        .get(index)
        .cloned()
        .filter(|value| *value != Value::Undefined)
}

/// Execute Promise.prototype.then.
pub fn promise_then(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let backing = receiver.and_then(|value| {
        object_promise_backing(value).or_else(|| {
            crate::execute::get_property_result(value, "\0promise_data")
                .ok()
                .and_then(|value| match value {
                    Value::Promise(promise) => Some(promise),
                    _ => None,
                })
        })
    });
    let fallback = receiver
        .filter(|value| is_promise_prototype_chain(value))
        .map(|_| {
            let promise = Rc::new(PromiseData::new(PromiseState::Fulfilled(Value::Undefined)));
            promise
        });
    let Some(promise) = receiver
        .and_then(|value| match value {
            Value::Promise(promise) => Some(promise),
            _ => None,
        })
        .or(backing.as_ref())
        .or(fallback.as_ref())
    else {
        return Err(VmError::NotCallable);
    };
    let promise_value = Value::Promise(Rc::clone(promise));
    let species_receiver = receiver.unwrap_or(&promise_value);
    let constructor = then_species_constructor(species_receiver)?;
    let (result, result_promise) = match construct_then_result(&constructor) {
        Ok(result) => result,
        Err(error) => return Err(error),
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

fn is_promise_prototype_chain(value: &Value) -> bool {
    let mut current = crate::builtins::object::get_prototype_of(Some(value)).ok();
    while let Some(prototype) = current {
        if matches!(prototype, Value::Builtin(Builtin::PromisePrototype)) {
            return true;
        }
        current = crate::builtins::object::get_prototype_of(Some(&prototype)).ok();
    }
    false
}

include!("promise_methods.rs");
/// Dispatch Promise builtins.
pub fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::Promise => Err(crate::value::error::throw_type_error(
            "Promise constructor cannot be called without new",
        )),
        Builtin::PromiseResolve => resolve_receiver(receiver, arguments),
        Builtin::PromiseReject => reject_receiver(receiver, arguments),
        Builtin::PromiseAll => promise_combinator(PromiseAggregateKind::All, receiver, arguments),
        Builtin::PromiseAllKeyed => {
            promise_keyed_combinator(PromiseAggregateKind::All, receiver, arguments)
        }
        Builtin::PromiseAllSettled => {
            promise_combinator(PromiseAggregateKind::AllSettled, receiver, arguments)
        }
        Builtin::PromiseAllSettledKeyed => {
            promise_keyed_combinator(PromiseAggregateKind::AllSettled, receiver, arguments)
        }
        Builtin::PromiseAny => promise_combinator(PromiseAggregateKind::Any, receiver, arguments),
        Builtin::PromiseRace => promise_combinator(PromiseAggregateKind::Race, receiver, arguments),
        Builtin::PromiseWithResolvers => with_resolvers(receiver),
        Builtin::PromiseTry => promise_try(receiver, arguments),
        Builtin::PromiseAggregateResolve => aggregate_callback(receiver, arguments, true),
        Builtin::PromiseAggregateReject => aggregate_callback(receiver, arguments, false),
        Builtin::PromiseCapabilityExecutor => capability_executor(receiver, arguments),
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
include!("promise_exports.rs");
include!("promise_tests.rs");
