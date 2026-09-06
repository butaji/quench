//! Rust-owned `timers/promises` wrappers over the callback timer registry.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef};
use quench_runtime::value::{PromiseData, PromiseState, Value};

use crate::host::HostState;

const MODE_TIMEOUT: u16 = 0;
const MODE_IMMEDIATE: u16 = 1;

fn cap(spec: crate::registry::NodeSpec) -> Value {
    crate::host::capability(spec)
}

pub fn build() -> Result<Value, VmError> {
    let scheduler = host_api::object(vec![
        (
            "wait".into(),
            cap(crate::registry::SPEC_TIMERS_PROMISE_SCHEDULER_WAIT),
        ),
        (
            "yield".into(),
            cap(crate::registry::SPEC_TIMERS_PROMISE_SCHEDULER_YIELD),
        ),
        (
            "constructor".into(),
            cap(crate::registry::SPEC_TIMERS_PROMISE_SCHEDULER_CONSTRUCTOR),
        ),
    ]);
    Ok(host_api::object(vec![
        (
            "setTimeout".into(),
            cap(crate::registry::SPEC_TIMERS_PROMISE_TIMEOUT),
        ),
        (
            "setImmediate".into(),
            cap(crate::registry::SPEC_TIMERS_PROMISE_IMMEDIATE),
        ),
        (
            "setInterval".into(),
            cap(crate::registry::SPEC_TIMERS_PROMISE_INTERVAL),
        ),
        ("scheduler".into(), scheduler),
    ]))
}

pub fn timeout(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    promise_timer(state, args, MODE_TIMEOUT)
}

pub fn immediate(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    promise_timer(state, args, MODE_IMMEDIATE)
}

fn promise_timer(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    mode: u16,
) -> Result<Value, VmError> {
    let delay = args.first().cloned().unwrap_or(Value::Undefined);
    if mode == MODE_TIMEOUT && !matches!(delay, Value::Undefined | Value::Number(_)) {
        return Ok(rejected(type_error_value(
            "The \"delay\" argument must be of type number",
            "ERR_INVALID_ARG_TYPE",
        )));
    }
    let value = if mode == MODE_TIMEOUT {
        args.get(1).cloned().unwrap_or(Value::Undefined)
    } else {
        args.first().cloned().unwrap_or(Value::Undefined)
    };
    let options = if mode == MODE_TIMEOUT {
        args.get(2).cloned().unwrap_or(Value::Undefined)
    } else {
        args.get(1).cloned().unwrap_or(Value::Undefined)
    };
    if let Err(error) = validate_options(&options) {
        return Ok(rejected(error_value(error)));
    }
    let promise = PromiseData::allocate(PromiseState::Pending);
    let result = Value::Promise(promise);
    let context = host_api::object(vec![
        ("\0timer-promise".into(), result.clone()),
        ("\0timer-value".into(), value),
        (
            "\0timer-signal".into(),
            get(&options, "signal").unwrap_or(Value::Undefined),
        ),
        ("\0timer-mode".into(), Value::Number(mode as f64)),
    ]);
    let callback = bound(crate::registry::SPEC_TIMERS_PROMISE_FINISH, context.clone());
    let timer = if mode == MODE_TIMEOUT {
        crate::modules::timers::set_timeout(state, &[callback, delay])?
    } else {
        crate::modules::timers::set_immediate(state, &[callback])?
    };
    set(&context, "\0timer-handle", timer.clone());
    if matches!(get(&options, "ref"), Some(Value::Boolean(false))) {
        crate::modules::timers::method_unref(state, Some(&timer));
    }
    attach_abort(state, &context)?;
    Ok(result)
}

pub fn finish(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(context) = args.first() else {
        return Ok(Value::Undefined);
    };
    if let Some(Value::Promise(promise)) = get(context, "\0timer-promise") {
        let value = get(context, "\0timer-value").unwrap_or(Value::Undefined);
        quench_runtime::resolve_promise(&promise, value.clone());
    }
    remove_timer_signal_listener(state, context);
    Ok(Value::Undefined)
}

pub fn abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(context) = args.first() else {
        return Ok(Value::Undefined);
    };
    if let Some(handle) = get(context, "\0timer-handle") {
        let mode = get(context, "\0timer-mode").and_then(number).unwrap_or(0.0) as u16;
        let clear = if mode == MODE_IMMEDIATE {
            crate::modules::timers::clear_immediate
        } else {
            crate::modules::timers::clear_timeout
        };
        clear(state, &[handle])?;
    }
    let signal = get(context, "\0timer-signal").unwrap_or(Value::Undefined);
    let error = abort_error(&signal);
    remove_timer_signal_listener(state, context);
    if let Some(Value::Promise(promise)) = get(context, "\0timer-promise") {
        promise.mark_rejection_handled();
        reject_later(Rc::clone(&promise), error);
    }
    Ok(Value::Undefined)
}

fn attach_abort(state: &Rc<RefCell<HostState>>, context: &Value) -> Result<(), VmError> {
    let signal = get(context, "\0timer-signal").unwrap_or(Value::Undefined);
    if matches!(signal, Value::Undefined) {
        return Ok(());
    }
    if !quench_runtime::is_callable(&get(&signal, "addEventListener").unwrap_or(Value::Undefined)) {
        return Err(type_error(
            "The signal option must be an AbortSignal",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let callback = bound(crate::registry::SPEC_TIMERS_PROMISE_ABORT, context.clone());
    if truthy(&signal, "aborted") {
        return abort(state, None, &[context.clone()]).map(|_| ());
    }
    crate::modules::event_target::add_event_listener(
        state,
        Some(&signal),
        &[Value::String("abort".into()), callback.clone()],
    )?;
    set(context, "\0timer-listener", callback);
    Ok(())
}

fn remove_timer_signal_listener(state: &Rc<RefCell<HostState>>, context: &Value) {
    let Some(signal) = get(context, "\0timer-signal") else {
        return;
    };
    let Some(listener) = get(context, "\0timer-listener") else {
        return;
    };
    let _ = crate::modules::event_target::remove_event_listener(
        state,
        Some(&signal),
        &[Value::String("abort".into()), listener],
    );
}

pub fn interval(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let delay = args.first().cloned().unwrap_or(Value::Undefined);
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);
    let options = args.get(2).cloned().unwrap_or(Value::Undefined);
    if !matches!(delay, Value::Undefined | Value::Number(_)) {
        return Ok(interval_iterator(
            Value::Undefined,
            Some(type_error_value(
                "The \"delay\" argument must be of type number",
                "ERR_INVALID_ARG_TYPE",
            )),
        ));
    }
    if let Err(error) = validate_options(&options) {
        return Ok(interval_iterator(
            Value::Undefined,
            Some(error_value(error)),
        ));
    }
    let context = host_api::object(vec![
        ("\0interval-values".into(), host_api::array(Vec::new())),
        ("\0interval-waiter".into(), Value::Undefined),
        ("\0interval-value".into(), value),
        ("\0interval-closed".into(), Value::Boolean(false)),
        ("\0interval-failure".into(), Value::Undefined),
    ]);
    let callback = bound(
        crate::registry::SPEC_TIMERS_PROMISE_INTERVAL_TICK,
        context.clone(),
    );
    let timer = crate::modules::timers::set_interval(state, &[callback, delay])?;
    set(&context, "\0interval-handle", timer.clone());
    if matches!(get(&options, "ref"), Some(Value::Boolean(false))) {
        crate::modules::timers::method_unref(state, Some(&timer));
    }
    let signal = get(&options, "signal").unwrap_or(Value::Undefined);
    if !matches!(signal, Value::Undefined) {
        if !quench_runtime::is_callable(
            &get(&signal, "addEventListener").unwrap_or(Value::Undefined),
        ) {
            crate::modules::timers::clear_timeout(state, std::slice::from_ref(&timer))?;
            set(
                &context,
                "\0interval-failure",
                type_error_value(
                    "The signal option must be an AbortSignal",
                    "ERR_INVALID_ARG_TYPE",
                ),
            );
            set(&context, "\0interval-closed", Value::Boolean(true));
        } else {
            set(&context, "\0interval-signal", signal.clone());
            if truthy(&signal, "aborted") {
                interval_abort(state, None, &[context.clone()])?;
            } else {
                let listener = bound(
                    crate::registry::SPEC_TIMERS_PROMISE_INTERVAL_ABORT,
                    context.clone(),
                );
                set(&context, "\0interval-listener", listener.clone());
                crate::modules::event_target::add_event_listener(
                    state,
                    Some(&signal),
                    &[Value::String("abort".into()), listener],
                )?;
            }
        }
    }
    Ok(interval_iterator(context, None))
}

pub fn interval_tick(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(context) = args.first() else {
        return Ok(Value::Undefined);
    };
    if truthy(context, "\0interval-closed") {
        return Ok(Value::Undefined);
    }
    let result = host_api::object(vec![
        (
            "value".into(),
            get(context, "\0interval-value").unwrap_or(Value::Undefined),
        ),
        ("done".into(), Value::Boolean(false)),
    ]);
    if let Some(Value::Promise(promise)) = get(context, "\0interval-waiter") {
        set(context, "\0interval-waiter", Value::Undefined);
        quench_runtime::resolve_promise(&promise, result);
    } else {
        let values = push(
            get(context, "\0interval-values").unwrap_or_else(|| host_api::array(Vec::new())),
            result,
        );
        set(context, "\0interval-values", values);
    }
    Ok(Value::Undefined)
}

pub fn interval_next(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let context = receiver
        .and_then(|value| get(value, "\0interval-context"))
        .unwrap_or(Value::Undefined);
    let values = get(&context, "\0interval-values").unwrap_or_else(|| host_api::array(Vec::new()));
    if let Some(value) = shift(&values) {
        set(&context, "\0interval-values", values);
        return Ok(quench_runtime::promise_resolve(&[value]));
    }
    if let Some(failure) =
        get(&context, "\0interval-failure").filter(|value| !matches!(value, Value::Undefined))
    {
        return Ok(rejected(failure));
    }
    if truthy(&context, "\0interval-closed") {
        return Ok(quench_runtime::promise_resolve(&[done()]));
    }
    let promise = PromiseData::allocate(PromiseState::Pending);
    set(
        &context,
        "\0interval-waiter",
        Value::Promise(promise.clone()),
    );
    Ok(Value::Promise(promise))
}

pub fn interval_return(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let context = receiver
        .and_then(|value| get(value, "\0interval-context"))
        .unwrap_or(Value::Undefined);
    if !truthy(&context, "\0interval-closed") {
        set(&context, "\0interval-closed", Value::Boolean(true));
        if let Some(handle) = get(&context, "\0interval-handle") {
            crate::modules::timers::clear_timeout(state, &[handle])?;
        }
        remove_interval_signal_listener(state, &context);
        if let Some(Value::Promise(promise)) = get(&context, "\0interval-waiter") {
            quench_runtime::resolve_promise(&promise, done());
            set(&context, "\0interval-waiter", Value::Undefined);
        }
    }
    Ok(quench_runtime::promise_resolve(&[done()]))
}

pub fn interval_abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(context) = args.first() else {
        return Ok(Value::Undefined);
    };
    set(
        &context,
        "\0interval-failure",
        abort_error(&get(context, "\0interval-signal").unwrap_or(Value::Undefined)),
    );
    set(&context, "\0interval-closed", Value::Boolean(true));
    if let Some(handle) = get(context, "\0interval-handle") {
        crate::modules::timers::clear_timeout(state, &[handle])?;
    }
    remove_interval_signal_listener(state, context);
    let failure = get(context, "\0interval-failure").unwrap_or(Value::Undefined);
    if let Some(Value::Promise(promise)) = get(context, "\0interval-waiter") {
        promise.mark_rejection_handled();
        reject_later(Rc::clone(&promise), failure);
        set(context, "\0interval-waiter", Value::Undefined);
    }
    Ok(Value::Undefined)
}

fn remove_interval_signal_listener(state: &Rc<RefCell<HostState>>, context: &Value) {
    let Some(signal) = get(context, "\0interval-signal") else {
        return;
    };
    let Some(listener) = get(context, "\0interval-listener") else {
        return;
    };
    let _ = crate::modules::event_target::remove_event_listener(
        state,
        Some(&signal),
        &[Value::String("abort".into()), listener],
    );
}

fn interval_iterator(context: Value, failure: Option<Value>) -> Value {
    let context = if matches!(context, Value::Undefined) {
        host_api::object(vec![
            ("\0interval-values".into(), host_api::array(Vec::new())),
            ("\0interval-waiter".into(), Value::Undefined),
            ("\0interval-closed".into(), Value::Boolean(false)),
            ("\0interval-failure".into(), Value::Undefined),
        ])
    } else {
        context
    };
    if let Some(error) = failure {
        set(&context, "\0interval-failure", error);
        set(&context, "\0interval-closed", Value::Boolean(true));
    }
    let iterator = host_api::object(vec![("\0interval-context".into(), context)]);
    set(
        &iterator,
        "next",
        cap(crate::registry::SPEC_TIMERS_PROMISE_INTERVAL_NEXT),
    );
    set(
        &iterator,
        "return",
        cap(crate::registry::SPEC_TIMERS_PROMISE_INTERVAL_RETURN),
    );
    set(
        &iterator,
        "Symbol.asyncIterator",
        cap(crate::registry::SPEC_TIMERS_PROMISE_INTERVAL_ASYNC_ITERATOR),
    );
    iterator
}

pub fn async_iterator(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}
pub fn scheduler_wait(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    scheduler_check(receiver)?;
    let mut forwarded = args.to_vec();
    forwarded.insert(1, Value::Undefined);
    timeout(state, None, &forwarded)
}
pub fn scheduler_yield(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    scheduler_check(receiver)?;
    immediate(state, None, &[Value::Undefined])
}
pub fn scheduler_constructor(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(type_error("Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn scheduler_check(receiver: Option<&Value>) -> Result<(), VmError> {
    receiver
        .filter(|value| {
            matches!(
                get(value, "wait"),
                Some(Value::BoundFunction(_))
                    | Some(Value::Function(_))
                    | Some(Value::HostCapability(_))
            )
        })
        .map(|_| ())
        .ok_or_else(|| {
            type_error(
                "Cannot read properties of an invalid Scheduler",
                "ERR_INVALID_THIS",
            )
        })
}
fn validate_options(options: &Value) -> Result<(), VmError> {
    if matches!(options, Value::Undefined) {
        return Ok(());
    }
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(type_error(
            "The options argument must be an object",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    if let Some(value) = get(options, "ref") {
        if !matches!(value, Value::Undefined | Value::Boolean(_)) {
            return Err(type_error(
                "The options.ref property must be of type boolean",
                "ERR_INVALID_ARG_TYPE",
            ));
        }
    }
    if let Some(signal) = get(options, "signal") {
        if !matches!(signal, Value::Undefined)
            && !quench_runtime::is_callable(
                &get(&signal, "addEventListener").unwrap_or(Value::Undefined),
            )
        {
            return Err(type_error(
                "The signal option must be an AbortSignal",
                "ERR_INVALID_ARG_TYPE",
            ));
        }
    }
    Ok(())
}
fn abort_error(signal: &Value) -> Value {
    let error = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::Error),
        &Value::Undefined,
        &[Value::String("The operation was aborted".into())],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    set(&error, "name", Value::String("AbortError".into()));
    set(&error, "code", Value::String("ABORT_ERR".into()));
    if let Some(reason) = get(signal, "reason") {
        set(&error, "cause", reason);
    }
    error
}
fn rejected(error: Value) -> Value {
    let promise = PromiseData::allocate(PromiseState::Pending);
    promise.mark_rejection_handled();
    reject_later(Rc::clone(&promise), error);
    Value::Promise(promise)
}
fn reject_later(promise: Rc<PromiseData>, error: Value) {
    let target = Rc::clone(&promise);
    quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
        quench_runtime::reject_promise(&target, error.clone());
    }));
}
fn type_error(message: &str, code: &str) -> VmError {
    VmError::Thrown(type_error_value(message, code))
}
fn type_error_value(message: &str, code: &str) -> Value {
    let error = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::TypeError),
        &Value::Undefined,
        &[Value::String(message.into())],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    set(&error, "code", Value::String(code.into()));
    error
}
fn error_value(error: VmError) -> Value {
    match error {
        VmError::Thrown(value) => value,
        _ => host_api::object(Vec::new()),
    }
}
fn done() -> Value {
    host_api::object(vec![
        ("value".into(), Value::Undefined),
        ("done".into(), Value::Boolean(true)),
    ])
}
fn bound(spec: crate::registry::NodeSpec, context: Value) -> Value {
    host_api::bound_capability_with_arguments(
        HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: HostCapabilityKind::Custom(spec.cap),
        },
        vec![context],
    )
}
fn get(value: &Value, key: &str) -> Option<Value> {
    quench_runtime::execute::get_property_result(value, key).ok()
}
fn set(value: &Value, key: &str, item: Value) {
    quench_runtime::execute::set_property_in_place(value, key, item);
}
fn number(value: Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(number),
        _ => None,
    }
}
fn truthy(value: &Value, key: &str) -> bool {
    get(value, key).is_some_and(|item| quench_runtime::execute::is_truthy(&item))
}
fn push(value: Value, item: Value) -> Value {
    let length = get(&value, "length")
        .and_then(number)
        .unwrap_or(0.0)
        .max(0.0) as usize;
    let value = quench_runtime::execute::set_property(value, &length.to_string(), item);
    quench_runtime::execute::set_property(value, "length", Value::Number((length + 1) as f64))
}
fn shift(value: &Value) -> Option<Value> {
    let length = get(value, "length")
        .and_then(number)
        .unwrap_or(0.0)
        .max(0.0) as usize;
    if length == 0 {
        None
    } else {
        let first = quench_runtime::execute::get_property(value, "0");
        for index in 1..length {
            let item = quench_runtime::execute::get_property(value, &index.to_string());
            let _ = quench_runtime::execute::set_property_in_place(
                value,
                &(index - 1).to_string(),
                item,
            );
        }
        let _ = quench_runtime::execute::set_property_in_place(
            value,
            &(length - 1).to_string(),
            Value::Undefined,
        );
        let _ = quench_runtime::execute::set_property_in_place(
            value,
            "length",
            Value::Number((length - 1) as f64),
        );
        Some(first)
    }
}
