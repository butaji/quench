//! Promise implementation with microtask queue.

use std::{
    cell::{Cell, RefCell},
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

struct CapabilityState {
    resolve: Option<Value>,
    reject: Option<Value>,
}

pub(crate) struct ThenResult {
    pub(crate) target: Value,
    pub(crate) resolve: Value,
    pub(crate) reject: Value,
}

thread_local! {
    static NEXT_CAPABILITY_ID: Cell<u64> = const { Cell::new(1) };
    static CAPABILITIES: RefCell<HashMap<u64, CapabilityState>> = RefCell::new(HashMap::new());
}

fn capability_id(receiver: Option<&Value>) -> Option<u64> {
    let Value::Number(value) = receiver? else {
        return None;
    };
    (*value >= 1.0 && value.is_finite() && value.fract() == 0.0).then_some(*value as u64)
}

fn capability_executor(id: u64, arguments: &[Value]) -> Result<Value, VmError> {
    CAPABILITIES.with(|capabilities| {
        let mut capabilities = capabilities.borrow_mut();
        let Some(state) = capabilities.get_mut(&id) else {
            return Err(crate::vm::not_callable());
        };
        if state.resolve.is_some() || state.reject.is_some() {
            return Err(crate::value::error::throw_type_error(
                "Promise capability executor already called",
            ));
        }
        let resolve = arguments.first().cloned().unwrap_or(Value::Undefined);
        let reject = arguments.get(1).cloned().unwrap_or(Value::Undefined);
        if !matches!(resolve, Value::Undefined) {
            state.resolve = Some(resolve);
        }
        if !matches!(reject, Value::Undefined) {
            state.reject = Some(reject);
        }
        Ok(Value::Undefined)
    })
}

fn capability_executor_function(id: u64, target: Builtin) -> Value {
    let length = Value::Number(2.0);
    let name = Value::String(String::new());
    let descriptor = |value: Value| {
        Value::Object(Rc::new(crate::value::ObjectData::new(vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ])))
    };
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: crate::vm::current_context_or_default().realm(),
        target: Value::Builtin(target),
        receiver: Value::Number(id as f64),
        arguments: Vec::new(),
        properties: RefCell::new(vec![
            ("length".to_string(), length.clone()),
            (
                crate::builtins::descriptor_key("length"),
                descriptor(length),
            ),
            ("name".to_string(), name.clone()),
            (crate::builtins::descriptor_key("name"), descriptor(name)),
        ]),
    }))
}

pub(crate) fn new_promise_capability(
    constructor: &Value,
) -> Result<(Value, Value, Value), VmError> {
    let id = NEXT_CAPABILITY_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1).max(1));
        id
    });
    CAPABILITIES.with(|capabilities| {
        capabilities.borrow_mut().insert(
            id,
            CapabilityState {
                resolve: None,
                reject: None,
            },
        );
    });
    let executor = capability_executor_function(id, Builtin::PromiseResolve);
    let result = crate::construct::construct_value(constructor, &[executor]);
    let state = CAPABILITIES.with(|capabilities| capabilities.borrow_mut().remove(&id));
    let result = result?;
    let Some(CapabilityState {
        resolve: Some(resolve),
        reject: Some(reject),
    }) = state
    else {
        return Err(crate::value::error::throw_type_error(
            "Promise capability executor did not provide callable functions",
        ));
    };
    if !crate::conversion::is_callable(&resolve) || !crate::conversion::is_callable(&reject) {
        return Err(crate::value::error::throw_type_error(
            "Promise capability functions are not callable",
        ));
    }
    Ok((result, resolve, reject))
}

include!("promise_combinators.rs");
include!("promise_finally.rs");
include!("promise_drain.rs");
include!("promise_settlement.rs");
include!("promise_with_resolvers.rs");
include!("promise_try.rs");

fn process_promise(promise: &Rc<PromiseData>) {
    let context = Rc::clone(&promise.context.0);
    crate::vm::with_current_context(&context, || process_promise_in_context(promise));
}

fn process_promise_in_context(promise: &Rc<PromiseData>) {
    let state = promise.state.borrow().clone();
    let then_actions = std::mem::take(&mut *promise.then_actions.borrow_mut());
    let promise_key = Rc::as_ptr(promise) as usize;
    if matches!(state, PromiseState::Pending) {
        // Thenable assimilation runs as a continuation while the target
        // promise is still pending. Preserve reactions registered on that
        // target; they must run after the thenable settles.
        promise.then_actions.borrow_mut().extend(then_actions);
    } else {
        process_then_actions(then_actions, &state, promise_key);
    }
    let continuations = std::mem::take(&mut *promise.continuations.borrow_mut());
    for continuation in continuations {
        if matches!(state, PromiseState::Pending)
            && !matches!(&continuation, PromiseContinuation::Thenable { .. })
        {
            promise.continuations.borrow_mut().push(continuation);
        } else {
            let async_continuation = matches!(
                &continuation,
                PromiseContinuation::AsyncGenerator { .. }
                    | PromiseContinuation::AsyncGeneratorYield { .. }
            );
            if async_continuation {
                // Await resumes in its own Promise reaction resource. Using
                // the awaited promise here leaks enterWith() mutations from
                // the producer into the consumer's async context.
                let reaction = match &continuation {
                    PromiseContinuation::AsyncGenerator { reaction, .. }
                    | PromiseContinuation::AsyncGeneratorYield { reaction, .. } => {
                        Rc::clone(reaction)
                    }
                    _ => unreachable!(),
                };
                promise_phase(&reaction, "before");
                process_continuation(continuation, &state);
                promise_phase(&reaction, "after");
            } else {
                process_continuation(continuation, &state);
            }
        }
    }
}

fn with_promise_trigger<T>(trigger: &Rc<PromiseData>, f: impl FnOnce() -> T) -> T {
    let previous = PROMISE_TRIGGER.with(|slot| slot.replace(Some(Rc::clone(trigger))));
    let result = f();
    PROMISE_TRIGGER.with(|slot| slot.replace(previous));
    result
}

fn process_then_actions(
    then_actions: Vec<(Option<Value>, Option<Value>)>,
    state: &PromiseState,
    promise_key: usize,
) {
    for (on_fulfilled, on_rejected) in then_actions {
        let result = THEN_RESULTS.with(|results| {
            let mut results = results.borrow_mut();
            let queue = results.get_mut(&promise_key)?;
            let result = queue.pop_front();
            if queue.is_empty() {
                results.remove(&promise_key);
            }
            result
        });
        let Some(result) = result else {
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
            let settle = if matches!(state, PromiseState::Rejected(_)) {
                &result.reject
            } else {
                &result.resolve
            };
            let _ = crate::functions::execute_target(settle, &Value::Undefined, &[value]);
            continue;
        };
        if let Value::Promise(promise) = &result.target {
            promise_phase(promise, "before");
        }
        let completion =
            match crate::functions::execute_target(&handler, &Value::Undefined, &[value]) {
                Ok(value) => {
                    crate::functions::execute_target(&result.resolve, &Value::Undefined, &[value])
                }
                Err(VmError::Thrown(reason)) => {
                    crate::functions::execute_target(&result.reject, &Value::Undefined, &[reason])
                }
                Err(_) => crate::functions::execute_target(
                    &result.reject,
                    &Value::Undefined,
                    &[Value::Undefined],
                ),
            };
        let _ = completion;
        if let Value::Promise(promise) = &result.target {
            promise_phase(promise, "after");
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
        PromiseContinuation::AsyncGenerator {
            generator,
            result,
            async_function,
            ..
        } => process_async_continuation(generator, result, false, async_function, state),
        PromiseContinuation::AsyncGeneratorYield { generator, result, .. } => {
            process_async_continuation(generator, result, true, false, state)
        }
        PromiseContinuation::ArrayFromAsync {
            result,
            iterator,
            receiver,
            mapper,
            this_arg,
            values,
            index,
            array_like,
            pending,
            target,
        } => crate::arrays::process_async_continuation(
            result, iterator, receiver, mapper, this_arg, values, index, array_like, pending,
            target, state,
        ),
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
        if is_yield_star_suspension(&generator) && delegated_result_done(&value) {
            continue_after_delegation(
                &generator,
                &result,
            );
            return;
        }
        finish_async_yield(&generator, &result, state, value);
        return;
    }
    let resume = if matches!(state, PromiseState::Rejected(_)) {
        crate::generator::resume_async_after_await(&generator, true, value)
    } else {
        crate::generator::resume_async_after_await(&generator, false, value)
    };
    match resume {
        Ok(value) => settle_async_result(
            &result,
            async_result_value(value, async_function),
            async_function,
        ),
        Err(VmError::Suspended(awaited)) => {
            register_async_generator(&awaited, generator, result, async_function);
        }
        Err(VmError::Thrown(reason)) => reject_promise(&result, reason),
        Err(_) => reject_promise(&result, Value::Undefined),
    }
}

fn delegated_result_done(value: &Value) -> bool {
    crate::execute::get_property_result(value, "done")
        .map(|done| crate::execute::is_truthy(&done))
        .unwrap_or(false)
}

fn continue_after_delegation(
    generator: &Rc<crate::value::GeneratorData>,
    result: &Rc<PromiseData>,
) {
    match crate::generator::resume_async_after_await(generator, false, Value::Undefined) {
        Ok(value) => settle_async_result(result, value, false),
        Err(VmError::Suspended(awaited)) => {
            register_async_generator(&awaited, Rc::clone(generator), Rc::clone(result), false)
        }
        Err(VmError::Thrown(reason)) => reject_promise(result, reason),
        Err(_) => reject_promise(result, Value::Undefined),
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
        let value = if is_yield_star_suspension(generator) {
            crate::execute::get_property_result(&value, "value").unwrap_or(Value::Undefined)
        } else {
            value
        };
        resolve_promise(result, crate::generator::iterator_result(value, false));
    }
}

fn is_yield_star_suspension(generator: &crate::value::GeneratorData) -> bool {
    matches!(
        generator.state.borrow().as_ref().and_then(|state| state.suspension.as_ref()),
        Some(crate::continuation::SuspensionPoint::YieldStar { .. })
    )
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
    let next_value = Value::Promise(Rc::clone(next));
    let resolve = bound_settler(Builtin::PromiseResolve, result, 1.0);
    let reject = bound_settler(Builtin::PromiseReject, result, 1.0);
    let _ = promise_then(Some(&next_value), &[resolve, reject]);
}

/// Create a new pending Promise.
pub fn new_promise() -> Value {
    Value::Promise(PromiseData::allocate(PromiseState::Pending))
}

/// Convert an async function's completion into its result Promise.
pub(crate) fn from_async_completion(completion: Result<Value, VmError>) -> Value {
    let promise = PromiseData::allocate(PromiseState::Pending);
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
    let promise = PromiseData::allocate(PromiseState::Pending);
    settle_async_generator_completion(completion, generator, Rc::clone(&promise), false);
    Value::Promise(promise)
}

pub(crate) fn from_async_function_completion(
    completion: Result<Value, VmError>,
    generator: Rc<crate::value::GeneratorData>,
) -> Value {
    let promise = PromiseData::allocate(PromiseState::Pending);
    settle_async_generator_completion(completion, generator, Rc::clone(&promise), true);
    Value::Promise(promise)
}

/// Start an async function with its result promise established before the
/// body runs. Promise allocations performed by `await` then inherit this
/// result as their trigger, matching Node's async-resource ordering.
pub(crate) fn start_async_function(generator: Rc<crate::value::GeneratorData>) -> Value {
    let promise = PromiseData::allocate(PromiseState::Pending);
    promise_phase(&promise, "before");
    let completion = with_promise_trigger(&promise, || {
        crate::generator::resume(
            &generator,
            crate::generator::Resume::Next(crate::value::Value::Undefined),
        )
    });
    promise_phase(&promise, "after");
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
        Ok(value) => settle_async_result(
            &promise,
            async_result_value(value, async_function),
            async_function,
        ),
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
    // Every suspension has a distinct reaction resource. It inherits the
    // producer's context at allocation, while the continuation itself runs
    // under this resource when the awaited promise settles.
    let reaction =
        with_promise_trigger(awaited, || PromiseData::allocate(PromiseState::Pending));
    awaited.continuations.borrow_mut().push(if *generator.pending_yield.borrow() {
        PromiseContinuation::AsyncGeneratorYield { generator, result, reaction }
    } else {
        PromiseContinuation::AsyncGenerator {
            generator,
            result,
            async_function,
            reaction,
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

fn settle_async_result(result: &Rc<PromiseData>, value: Value, async_function: bool) {
    if async_function {
        if let Value::Promise(promise) = value {
            adopt_promise(result, &promise);
            return;
        }
    }
    resolve_promise(result, value);
}

fn queue_promise(promise: &Rc<PromiseData>) {
    MICROTASK_QUEUE.with(|queue| queue.borrow_mut().push_back(Rc::clone(promise)));
}

pub(crate) fn promise_created(promise: &Rc<PromiseData>) {
    let context = crate::vm::current_context();
    let Some(host) = context.host_handle() else {
        return;
    };
    let descriptor = crate::ops::HostCapabilityRef {
        realm: context.realm(),
        kind: crate::ops::HostCapabilityKind::PromiseHook,
    };
    let trigger = PROMISE_TRIGGER.with(|slot| slot.borrow().clone());
    let mut args = vec![
        Value::String("init".into()),
        Value::Promise(Rc::clone(promise)),
    ];
    if let Some(trigger) = trigger {
        args.push(Value::Promise(trigger));
    }
    let _ = host.call(descriptor, None, &args);
}

pub(crate) fn promise_resolved(promise: &Rc<PromiseData>) {
    let context = crate::vm::current_context();
    let Some(host) = context.host_handle() else {
        return;
    };
    let descriptor = crate::ops::HostCapabilityRef {
        realm: context.realm(),
        kind: crate::ops::HostCapabilityKind::PromiseHook,
    };
    let _ = host.call(
        descriptor,
        None,
        &[
            Value::String("resolve".into()),
            Value::Promise(Rc::clone(promise)),
        ],
    );
}

pub(crate) fn promise_phase(promise: &Rc<PromiseData>, event: &str) {
    let context = crate::vm::current_context();
    let Some(host) = context.host_handle() else {
        return;
    };
    let descriptor = crate::ops::HostCapabilityRef {
        realm: context.realm(),
        kind: crate::ops::HostCapabilityKind::PromiseHook,
    };
    let _ = host.call(
        descriptor,
        None,
        &[
            Value::String(event.to_owned()),
            Value::Promise(Rc::clone(promise)),
        ],
    );
}

/// Resolve a Promise with a value.
pub fn resolve_promise(promise: &Rc<PromiseData>, value: Value) {
    if !claim_promise(promise) || !matches!(*promise.state.borrow(), PromiseState::Pending) {
        return;
    }
    set_promise_state(promise, PromiseState::Fulfilled(value));
    let hooks = std::mem::take(&mut *promise.aggregate_hooks.borrow_mut());
    let state = promise.state.borrow().clone();
    for (aggregate, index) in hooks {
        aggregate_settle(&aggregate, index, &state);
    }
    promise_resolved(promise);
    queue_promise(promise);
}

/// Reject a Promise with a reason.
pub fn reject_promise(promise: &Rc<PromiseData>, reason: Value) {
    if !claim_promise(promise) || !matches!(*promise.state.borrow(), PromiseState::Pending) {
        return;
    }
    set_promise_state(promise, PromiseState::Rejected(reason));
    let hooks = std::mem::take(&mut *promise.aggregate_hooks.borrow_mut());
    let state = promise.state.borrow().clone();
    for (aggregate, index) in hooks {
        aggregate_settle(&aggregate, index, &state);
    }
    promise_resolved(promise);
    queue_promise(promise);
    if !promise.rejection_handled.get() && !promise.unhandled_queued.replace(true) {
        crate::promise::queue_unhandled_rejection(
            Rc::clone(promise),
            promise.result.borrow().clone().unwrap_or(Value::Undefined),
        );
    }
}

/// Execute Promise.resolve using the single canonical promise-resolution path.
pub fn promise_resolve(arguments: &[Value]) -> Value {
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    resolve_value(value)
}

pub(crate) fn promise_resolve_with_then(value: Value, then: Value) -> Value {
    let promise = PromiseData::allocate(PromiseState::Pending);
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

fn resolve_value(value: Value) -> Value {
    if matches!(value, Value::Promise(_)) {
        return value;
    }
    let promise = PromiseData::allocate(PromiseState::Pending);
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
    if let Some(id) = capability_id(receiver) {
        return capability_executor(id, arguments);
    }
    match receiver {
        Some(Value::Builtin(Builtin::Promise)) => {
            let value = arguments.first().cloned().unwrap_or(Value::Undefined);
            if let Value::Promise(promise) = &value {
                let constructor = crate::execute::get_property_result(&value, "constructor")?;
                if matches!(constructor, Value::Builtin(Builtin::Promise)) {
                    return Ok(value);
                }
                let target = PromiseData::allocate(PromiseState::Pending);
                let resolve = bound_settler(Builtin::PromiseAdoptResolve, &target, 1.0);
                let reject = bound_settler(Builtin::PromiseAdoptReject, &target, 1.0);
                // Promise.resolve assimilates this source immediately; the
                // paired rejection adopter is already an observable handler.
                promise.rejection_handled.set(true);
                promise
                    .then_actions
                    .borrow_mut()
                    .push((Some(resolve.clone()), Some(reject.clone())));
                THEN_RESULTS.with(|results| {
                    results
                        .borrow_mut()
                        .entry(Rc::as_ptr(promise) as usize)
                        .or_default()
                        .push_back(ThenResult {
                            target: Value::Promise(Rc::clone(&target)),
                            resolve: resolve.clone(),
                            reject: reject.clone(),
                        });
                });
                if !matches!(*promise.state.borrow(), PromiseState::Pending) {
                    queue_promise(promise);
                }
                return Ok(Value::Promise(target));
            }
            Ok(promise_resolve(&[value]))
        }
        Some(Value::Promise(promise)) => {
            let value = arguments.first().cloned().unwrap_or(Value::Undefined);
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
            if !crate::value::is_object(&value) {
                resolve_promise(promise, value);
                return Ok(Value::Undefined);
            }
            if !claim_promise(promise) {
                return Ok(Value::Undefined);
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
    let promise = PromiseData::allocate(PromiseState::Pending);
    reject_promise(&promise, reason);
    Value::Promise(promise)
}

fn reject_receiver(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(id) = capability_id(receiver) {
        return capability_executor(id, arguments);
    }
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
    let Some(Value::Promise(promise)) = receiver else {
        return Err(VmError::NotCallable);
    };
    let promise_value = Value::Promise(Rc::clone(promise));
    let constructor = then_species_constructor(&promise_value)?;
    let (result, resolve, reject) =
        with_promise_trigger(promise, || construct_then_result(&constructor))?;
    promise
        .then_actions
        .borrow_mut()
        .push((maybe_handler(arguments, 0), maybe_handler(arguments, 1)));
    // A rejection reaction transfers failures to the promise returned by
    // `then`, even when the caller omits its explicit reject callback.  The
    // descendant is then responsible for unhandled-rejection reporting; the
    // source promise itself has a reaction and is considered handled.
    promise.rejection_handled.set(true);
    let promise_key = Rc::as_ptr(promise) as usize;
    THEN_RESULTS.with(|results| {
        results
            .borrow_mut()
            .entry(promise_key)
            .or_default()
            .push_back(ThenResult {
                target: result.clone(),
                resolve,
                reject,
            });
    });
    if !matches!(*promise.state.borrow(), PromiseState::Pending) {
        queue_promise(promise);
    }
    Ok(result)
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
