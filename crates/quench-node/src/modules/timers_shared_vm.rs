use crate::host::NodeHost;
use crate::modules::shared_event_loop::SharedCallback;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::time::Duration;

const TIMER_ID: &str = "\0quench:shared-timer-id";
const TIMER_REFED: &str = "refed";
const PROMISE_STATE: &str = "\0quench:timer-promise-state";
const PROMISE_KIND: &str = "\0quench:timer-promise-kind";
const PROMISE_DELAY: &str = "\0quench:timer-promise-delay";
const PROMISE_VALUE: &str = "\0quench:timer-promise-value";
const PROMISE_OPTIONS: &str = "\0quench:timer-promise-options";
const PROMISE_RESOLVE: &str = "\0quench:timer-promise-resolve";
const PROMISE_REJECT: &str = "\0quench:timer-promise-reject";
const PROMISE_SIGNAL: &str = "\0quench:timer-promise-signal";
const PROMISE_LISTENER: &str = "\0quench:timer-promise-listener";
const PROMISE_HANDLE: &str = "\0quench:timer-promise-handle";
const PROMISE_PENDING: &str = "pending";
const PROMISE_FULFILLED: &str = "fulfilled";
const PROMISE_REJECTED: &str = "rejected";
const PROMISE_TIMEOUT: &str = "timeout";
const PROMISE_IMMEDIATE: &str = "immediate";
const DEFAULT_TIMER_DELAY_MS: u64 = 1;
const MAX_TIMER_DELAY_MS: u64 = 2_147_483_647;
const CALLBACK_TIMER_EXPORTS: &[&str] = &[
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "setImmediate",
    "clearImmediate",
];

const PROMISE_INTERVAL_FACTORY: &str = quench_js_check::checked_js!(
    r#"(schedule, cancel) => function setInterval(delay, value, options = {}) {
  const signal = options?.signal;
  let timer;
  let stopped = false;
  let failure;
  let waiter;
  const values = [];
  const cleanup = () => signal?.removeEventListener?.("abort", abort);
  const stop = () => {
    if (stopped) return;
    stopped = true;
    if (timer !== undefined) cancel(timer);
    cleanup();
  };
  const abort = () => {
    failure = new Error("The operation was aborted");
    failure.name = "AbortError";
    failure.code = "ABORT_ERR";
    stop();
    if (waiter) {
      const reject = waiter.reject;
      waiter = undefined;
      reject(failure);
    }
  };
  const iterator = {
    [Symbol.asyncIterator]() { return this; },
    next() {
      if (failure) return Promise.reject(failure);
      if (values.length) return Promise.resolve({ value: values.shift(), done: false });
      if (stopped) return Promise.resolve({ value: undefined, done: true });
      return new Promise((resolve, reject) => { waiter = { resolve, reject }; });
    },
    return() {
      values.length = 0;
      stop();
      if (waiter) {
        const resolve = waiter.resolve;
        waiter = undefined;
        resolve({ value: undefined, done: true });
      }
      return Promise.resolve({ value: undefined, done: true });
    },
    throw(error) { stop(); return Promise.reject(error); },
    ref() { timer?.ref?.(); return this; },
    unref() { timer?.unref?.(); return this; },
    hasRef() { return timer?.hasRef?.() ?? true; },
  };
  if (signal?.aborted) {
    abort();
  } else {
    signal?.addEventListener?.("abort", abort, { once: true });
    timer = schedule(() => {
      if (stopped) return;
      if (waiter) {
        const resolve = waiter.resolve;
        waiter = undefined;
        resolve({ value, done: false });
      } else {
        values.push(value);
      }
    }, delay);
  }
  return iterator;
}"#
);

#[derive(Clone, Copy)]
pub(crate) struct TimerHandleApi {
    pub(crate) refed_key: RootId,
    pub(crate) timeout_prototype: RootId,
    pub(crate) immediate_prototype: RootId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HandleRefState {
    Refed,
    Unrefed,
    Settled,
}

impl HandleRefState {
    fn from_rooted(context: &NativeContext<'_, NodeHost>, value: RootId) -> Self {
        match context
            .rooted_value(value)
            .and_then(|value| value.as_bool())
        {
            Some(true) => Self::Refed,
            Some(false) => Self::Unrefed,
            None => Self::Settled,
        }
    }

    fn transition(self, refed: bool) -> Option<bool> {
        match (self, refed) {
            (Self::Unrefed, true) => Some(true),
            (Self::Refed, false) => Some(false),
            _ => None,
        }
    }
}

pub(crate) fn queue_microtask(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        return invalid_callback(context, "non-function");
    }
    context.queue_microtask_rooted(callback, &[])?;
    Ok(context.undefined())
}

pub(crate) fn promises_module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    for (name, operation) in [
        ("setTimeout", "timersPromiseSetTimeout"),
        ("setImmediate", "timersPromiseSetImmediate"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        install(context, module, name, function)?;
    }
    let factory = context.evaluate_script_rooted(
        PROMISE_INTERVAL_FACTORY,
        "node:timers/promises/interval.js",
    )?;
    let global = context.global_root()?;
    let schedule = property(context, global, "setInterval")?;
    let cancel = property(context, global, "clearInterval")?;
    let undefined = context.undefined();
    let interval = context.call_rooted(factory, undefined, &[schedule, cancel])?;
    install(context, module, "setInterval", interval)?;
    let scheduler = scheduler_object(context)?;
    install(context, module, "scheduler", scheduler)?;
    Ok(module)
}

fn scheduler_object(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let scheduler = context.object_rooted()?;
    for (name, operation) in [
        ("wait", "timersPromiseSchedulerWait"),
        ("yield", "timersPromiseSchedulerYield"),
    ] {
        let function = context
            .host_function_with_data(crate::host::shared_vm::operation(operation), scheduler)?;
        install(context, scheduler, name, function)?;
    }
    let global = context.global_root()?;
    let proxy_key = context.string_rooted("Proxy");
    let proxy = context.get_property_rooted(global, proxy_key)?;
    let object_key = context.string_rooted("Object");
    let object = context.get_property_rooted(global, object_key)?;
    let handler = context.object_rooted()?;
    let illegal_constructor = context.host_function(crate::host::shared_vm::operation(
        "timersPromiseSchedulerConstructor",
    ))?;
    install(context, handler, "apply", illegal_constructor)?;
    install(context, handler, "construct", illegal_constructor)?;
    let constructor = context.construct_rooted(proxy, proxy, &[object, handler])?;
    install(context, scheduler, "constructor", constructor)?;
    Ok(scheduler)
}

pub(crate) fn timers_module(
    context: &mut NativeContext<'_, NodeHost>,
    promises: RootId,
) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let global = context.global_root()?;
    for name in CALLBACK_TIMER_EXPORTS {
        let key = context.string_rooted(name);
        let function = context.get_property_rooted(global, key)?;
        if !context.set_property_rooted(module, key, function, module)? {
            return Err(RootedError::host(format!("cannot install timers.{name}")));
        }
    }
    let promises_key = context.string_rooted("promises");
    if !context.set_property_rooted(module, promises_key, promises, module)? {
        return Err(RootedError::host("cannot install timers.promises"));
    }
    Ok(module)
}

pub(crate) fn promise_set_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let delay = argument_or_undefined(context, args, 0);
    let value = argument_or_undefined(context, args, 1);
    let options = argument_or_undefined(context, args, 2);
    promise_timer(
        context,
        PromiseTimerSpec::Timeout {
            delay,
            value,
            options,
        },
    )
}

pub(crate) fn promise_set_immediate(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = argument_or_undefined(context, args, 0);
    let options = argument_or_undefined(context, args, 1);
    promise_timer(context, PromiseTimerSpec::Immediate { value, options })
}

pub(crate) fn scheduler_wait(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let scheduler = context.host_function_data()?;
    if !context.same_value_rooted(receiver, scheduler)? {
        return invalid_scheduler_receiver(context);
    }
    let delay = argument_or_undefined(context, args, 0);
    let value = context.undefined();
    let options = argument_or_undefined(context, args, 1);
    promise_timer(
        context,
        PromiseTimerSpec::Timeout {
            delay,
            value,
            options,
        },
    )
}

pub(crate) fn scheduler_yield(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let scheduler = context.host_function_data()?;
    if !context.same_value_rooted(receiver, scheduler)? {
        return invalid_scheduler_receiver(context);
    }
    let value = context.undefined();
    let options = argument_or_undefined(context, args, 0);
    promise_timer(context, PromiseTimerSpec::Immediate { value, options })
}

pub(crate) fn scheduler_constructor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted("Illegal constructor")?;
    let code = context.string_rooted("ERR_ILLEGAL_CONSTRUCTOR");
    install(context, error, "code", code)?;
    Err(context.throw(error))
}

enum PromiseTimerSpec {
    Timeout {
        delay: RootId,
        value: RootId,
        options: RootId,
    },
    Immediate {
        value: RootId,
        options: RootId,
    },
}

fn promise_timer(
    context: &mut NativeContext<'_, NodeHost>,
    spec: PromiseTimerSpec,
) -> Result<RootId, RootedError> {
    let operation = context.object_rooted()?;
    let state = context.string_rooted(PROMISE_PENDING);
    install(context, operation, PROMISE_STATE, state)?;
    let (kind, delay, value, options) = match spec {
        PromiseTimerSpec::Timeout {
            delay,
            value,
            options,
        } => (PROMISE_TIMEOUT, delay, value, options),
        PromiseTimerSpec::Immediate { value, options } => {
            let undefined = context.undefined();
            (PROMISE_IMMEDIATE, undefined, value, options)
        }
    };
    let kind_value = context.string_rooted(kind);
    install(context, operation, PROMISE_KIND, kind_value)?;
    install(context, operation, PROMISE_DELAY, delay)?;
    install(context, operation, PROMISE_VALUE, value)?;
    install(context, operation, PROMISE_OPTIONS, options)?;
    let executor = context.host_function_with_data(
        crate::host::shared_vm::operation("timersPromiseSchedule"),
        operation,
    )?;
    let global = context.global_root()?;
    let promise_name = context.string_rooted("Promise");
    let promise_constructor = context.get_property_rooted(global, promise_name)?;
    context.construct_rooted(promise_constructor, promise_constructor, &[executor])
}

fn argument_or_undefined(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    index: usize,
) -> RootId {
    args.get(index)
        .copied()
        .unwrap_or_else(|| context.undefined())
}

pub(crate) fn schedule_promise_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(resolve) = args.first().copied() else {
        return Err(RootedError::host(
            "timers/promises Promise executor omitted resolve",
        ));
    };
    let reject = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("timers/promises Promise executor omitted reject"))?;
    let operation = context.host_function_data()?;
    install(context, operation, PROMISE_RESOLVE, resolve)?;
    install(context, operation, PROMISE_REJECT, reject)?;
    let options = property(context, operation, PROMISE_OPTIONS)?;
    let signal = if is_undefined(context, options) {
        context.undefined()
    } else {
        property(context, options, "signal")?
    };
    install(context, operation, PROMISE_SIGNAL, signal)?;
    if is_undefined(context, signal) {
        return schedule_promise_operation(context, operation);
    }
    let add_listener = property(context, signal, "addEventListener")?;
    if !context.is_callable_rooted(add_listener)? {
        let error = type_error(
            context,
            "The signal option must be an AbortSignal",
            "ERR_INVALID_ARG_TYPE",
        )?;
        return reject_operation(context, operation, error);
    }
    if property_truthy(context, signal, "aborted")? {
        let error = abort_error(context, signal)?;
        return reject_operation(context, operation, error);
    }
    let listener = context.host_function_with_data(
        crate::host::shared_vm::operation("timersPromiseAbort"),
        operation,
    )?;
    install(context, operation, PROMISE_LISTENER, listener)?;
    let event_name = context.string_rooted("abort");
    let listener_options = context.object_rooted()?;
    let once = context.boolean(true);
    install(context, listener_options, "once", once)?;
    context.call_rooted(
        add_listener,
        signal,
        &[event_name, listener, listener_options],
    )?;
    schedule_promise_operation(context, operation)
}

fn schedule_promise_operation(
    context: &mut NativeContext<'_, NodeHost>,
    operation: RootId,
) -> Result<RootId, RootedError> {
    let kind = property_text(context, operation, PROMISE_KIND)?;
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("timersPromiseComplete"),
        operation,
    )?;
    let handle = if kind == PROMISE_IMMEDIATE {
        let receiver = context.undefined();
        set_immediate(context, receiver, &[callback])?
    } else {
        let delay = property(context, operation, PROMISE_DELAY)?;
        schedule_timer(context, &[callback, delay], false)?
    };
    install(context, operation, PROMISE_HANDLE, handle)?;
    Ok(context.undefined())
}

pub(crate) fn complete_promise_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    settle_operation(context)
}

pub(crate) fn abort_promise_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let operation = context.host_function_data()?;
    if operation_pending(context, operation)? {
        let kind = property_text(context, operation, PROMISE_KIND)?;
        let handle = property(context, operation, PROMISE_HANDLE)?;
        if kind == PROMISE_IMMEDIATE {
            let receiver = context.undefined();
            clear_immediate(context, receiver, &[handle])?;
        } else {
            let receiver = context.undefined();
            clear_timer(context, receiver, &[handle])?;
        }
        let signal = property(context, operation, PROMISE_SIGNAL)?;
        let error = abort_error(context, signal)?;
        reject_operation(context, operation, error)?;
    }
    Ok(context.undefined())
}

fn settle_operation(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let operation = context.host_function_data()?;
    if operation_pending(context, operation)? {
        let signal = property(context, operation, PROMISE_SIGNAL)?;
        let listener = property(context, operation, PROMISE_LISTENER)?;
        remove_abort_listener(context, signal, listener)?;
        let state = context.string_rooted(PROMISE_FULFILLED);
        install(context, operation, PROMISE_STATE, state)?;
        let resolve = property(context, operation, PROMISE_RESOLVE)?;
        let value = property(context, operation, PROMISE_VALUE)?;
        let receiver = context.undefined();
        context.call_rooted(resolve, receiver, &[value])?;
    }
    Ok(context.undefined())
}

fn reject_operation(
    context: &mut NativeContext<'_, NodeHost>,
    operation: RootId,
    error: RootId,
) -> Result<RootId, RootedError> {
    if operation_pending(context, operation)? {
        let signal = property(context, operation, PROMISE_SIGNAL)?;
        let listener = property(context, operation, PROMISE_LISTENER)?;
        remove_abort_listener(context, signal, listener)?;
        let state = context.string_rooted(PROMISE_REJECTED);
        install(context, operation, PROMISE_STATE, state)?;
        let reject = property(context, operation, PROMISE_REJECT)?;
        let receiver = context.undefined();
        context.call_rooted(reject, receiver, &[error])?;
    }
    Ok(context.undefined())
}

fn remove_abort_listener(
    context: &mut NativeContext<'_, NodeHost>,
    signal: RootId,
    listener: RootId,
) -> Result<(), RootedError> {
    if is_undefined(context, signal) || is_undefined(context, listener) {
        return Ok(());
    }
    let remove = property(context, signal, "removeEventListener")?;
    if context.is_callable_rooted(remove)? {
        let event_name = context.string_rooted("abort");
        context.call_rooted(remove, signal, &[event_name, listener])?;
    }
    Ok(())
}

fn abort_error(
    context: &mut NativeContext<'_, NodeHost>,
    signal: RootId,
) -> Result<RootId, RootedError> {
    let error = context.error_rooted("The operation was aborted")?;
    let name = context.string_rooted("AbortError");
    install(context, error, "name", name)?;
    let code = context.string_rooted("ABORT_ERR");
    install(context, error, "code", code)?;
    let cause = property(context, signal, "reason")?;
    if !is_undefined(context, cause) {
        install(context, error, "cause", cause)?;
    }
    Ok(error)
}

fn type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
    code: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(message)?;
    let code = context.string_rooted(code);
    install(context, error, "code", code)?;
    Ok(error)
}

fn invalid_scheduler_receiver(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let error = type_error(
        context,
        "Cannot read properties of an invalid Scheduler",
        "ERR_INVALID_THIS",
    )?;
    Err(context.throw(error))
}

pub(crate) fn set_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    schedule_timer(context, args, false)
}

pub(crate) fn set_interval(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    schedule_timer(context, args, true)
}

fn schedule_timer(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    repeating: bool,
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        return invalid_callback(context, "non-function");
    }
    let delay = timer_delay(context, args.get(1).copied())?;
    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .reserve_shared_timer_id()
        .ok_or_else(|| RootedError::host("shared timer identifier space exhausted"))?;
    let api = timer_handle_api(context)?;
    let handle = context.object_rooted()?;
    let id_value = context.string_rooted(&id.to_string());
    let id_key = context.string_rooted(TIMER_ID);
    if !context.set_property_rooted(handle, id_key, id_value, handle)? {
        return Err(RootedError::host("cannot initialize Timeout handle"));
    }
    if !context.set_prototype_rooted(handle, api.timeout_prototype)? {
        return Err(RootedError::host("cannot install Timeout prototype"));
    }
    let refed = context.boolean(true);
    if !context.define_data_property_rooted(handle, api.refed_key, refed, true, false, true)? {
        return Err(RootedError::host(
            "cannot initialize Timeout reference state",
        ));
    }
    crate::modules::async_hooks_shared_vm::emit_init(context, handle, "Timeout")?;
    let callback = context.retain(callback)?;
    let receiver = context.retain(handle)?;
    let callback_args = args
        .get(2..)
        .unwrap_or_default()
        .iter()
        .copied()
        .map(|argument| context.retain(argument))
        .collect::<Result<Vec<_>, _>>()?;
    let callback = SharedCallback {
        callback,
        receiver,
        args: callback_args,
    };
    let interval = repeating.then_some(delay);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_timer(id, delay, interval, callback);
    Ok(handle)
}

fn timer_delay(
    context: &mut NativeContext<'_, NodeHost>,
    delay: Option<RootId>,
) -> Result<Duration, RootedError> {
    let Some(delay) = delay else {
        return Ok(Duration::from_millis(DEFAULT_TIMER_DELAY_MS));
    };
    let value = context
        .rooted_value(delay)
        .ok_or_else(|| RootedError::host("invalid timer delay root"))?;
    let numeric = if let Some(number) = value.as_number() {
        Some(number)
    } else if let Some(boolean) = value.as_bool() {
        Some(if boolean { 1.0 } else { 0.0 })
    } else if value.is_null() {
        Some(0.0)
    } else if let Some(text) = context.string_text(delay)? {
        if text.trim().is_empty() {
            Some(0.0)
        } else {
            text.trim().parse::<f64>().ok()
        }
    } else if value.is_undefined() {
        None
    } else {
        None
    };
    let milliseconds = numeric
        .filter(|value| value.is_finite())
        .map(|value| value.max(0.0).trunc() as u64)
        .unwrap_or(DEFAULT_TIMER_DELAY_MS)
        .clamp(DEFAULT_TIMER_DELAY_MS, MAX_TIMER_DELAY_MS);
    Ok(Duration::from_millis(milliseconds))
}

pub(crate) fn timer_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    set_timer_ref(context, receiver, true, TimerHandleKind::Timeout)
}

pub(crate) fn timer_unref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    set_timer_ref(context, receiver, false, TimerHandleKind::Timeout)
}

pub(crate) fn immediate_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    set_timer_ref(context, receiver, true, TimerHandleKind::Immediate)
}

pub(crate) fn immediate_unref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    set_timer_ref(context, receiver, false, TimerHandleKind::Immediate)
}

#[derive(Clone, Copy)]
enum TimerHandleKind {
    Timeout,
    Immediate,
}

fn set_timer_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    refed: bool,
    kind: TimerHandleKind,
) -> Result<RootId, RootedError> {
    let api = timer_handle_api(context)?;
    let current = context.get_property_rooted(receiver, api.refed_key)?;
    let current = HandleRefState::from_rooted(context, current);
    let Some(next_refed) = current.transition(refed) else {
        return Ok(receiver);
    };

    let timer = match kind {
        TimerHandleKind::Timeout => timer_id(context, receiver)?,
        TimerHandleKind::Immediate => None,
    };
    let value = context.boolean(next_refed);
    if !context.set_property_rooted(receiver, api.refed_key, value, receiver)? {
        let error = context.type_error_rooted("Cannot update timer handle reference state")?;
        return Err(context.throw(error));
    }
    let shared_state = context.host_mut().shared_state();
    let mut state = shared_state.borrow_mut();
    match kind {
        TimerHandleKind::Timeout => {
            if let Some(id) = timer {
                state.scheduler.set_shared_timer_ref(id, next_refed);
            }
        }
        TimerHandleKind::Immediate => {
            state.scheduler.transition_shared_immediate_ref(next_refed);
        }
    }
    Ok(receiver)
}

pub(crate) fn timer_has_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let api = timer_handle_api(context)?;
    let refed = context.get_property_rooted(receiver, api.refed_key)?;
    let refed = context.truthy_rooted(refed)?;
    Ok(context.boolean(refed))
}

pub(crate) fn immediate_has_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let api = timer_handle_api(context)?;
    let refed = context.get_property_rooted(receiver, api.refed_key)?;
    let refed = context.truthy_rooted(refed)?;
    Ok(context.boolean(refed))
}

fn timer_handle_api(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<TimerHandleApi, RootedError> {
    if let Some(api) = context.host_mut().shared_state().borrow().timer_handle_api {
        return Ok(api);
    }
    let refed_key = context.symbol_rooted(Some(TIMER_REFED));
    let timeout_prototype = timer_handle_prototype(context, "Timeout", "timerHasRef")?;
    let immediate_prototype = timer_handle_prototype(context, "Immediate", "immediateHasRef")?;
    let api = TimerHandleApi {
        refed_key: context.retain(refed_key)?,
        timeout_prototype: context.retain(timeout_prototype)?,
        immediate_prototype: context.retain(immediate_prototype)?,
    };
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .timer_handle_api = Some(api);
    Ok(api)
}

fn timer_handle_prototype(
    context: &mut NativeContext<'_, NodeHost>,
    owner: &str,
    has_ref_operation: &str,
) -> Result<RootId, RootedError> {
    let class_source = match owner {
        "Timeout" => {
            "(() => class Timeout { constructor(callback, after, args, repeat, isRefed) { const schedule = repeat ? setInterval : setTimeout; const handle = Reflect.apply(schedule, undefined, [callback, after, ...(args ?? [])]); if (!isRefed) handle.unref(); return handle; } })()"
        }
        "Immediate" => {
            "(() => class Immediate { constructor(callback, args) { return Reflect.apply(setImmediate, undefined, [callback, ...(args ?? [])]); } })()"
        }
        _ => return Err(RootedError::host("unknown timer handle kind")),
    };
    let constructor = context.evaluate_script_rooted(class_source, "node:timers/shared.js")?;
    let prototype_key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(constructor, prototype_key)?;
    let (ref_operation, unref_operation) = match owner {
        "Timeout" => ("timerRef", "timerUnref"),
        "Immediate" => ("immediateRef", "immediateUnref"),
        _ => return Err(RootedError::host("unknown timer handle kind")),
    };
    for (name, operation) in [("ref", ref_operation), ("unref", unref_operation)] {
        let method = context.host_function(crate::host::shared_vm::operation(operation))?;
        let key = context.string_rooted(name);
        if !context.define_data_property_rooted(prototype, key, method, true, false, true)? {
            return Err(RootedError::host(format!(
                "cannot install {owner}.prototype.{name}"
            )));
        }
        let function_name = context.string_rooted("name");
        let function_value = context.string_rooted(name);
        if !context.define_data_property_rooted(
            method,
            function_name,
            function_value,
            false,
            false,
            true,
        )? {
            return Err(RootedError::host(format!(
                "cannot name {owner}.prototype.{name}"
            )));
        }
    }
    let method = context.host_function(crate::host::shared_vm::operation(has_ref_operation))?;
    let key = context.string_rooted("hasRef");
    if !context.define_data_property_rooted(prototype, key, method, true, false, true)? {
        return Err(RootedError::host(format!(
            "cannot install {owner}.prototype.hasRef"
        )));
    }
    let function_name = context.string_rooted("name");
    let method_name = context.string_rooted("hasRef");
    if !context.define_data_property_rooted(
        method,
        function_name,
        method_name,
        false,
        false,
        true,
    )? {
        return Err(RootedError::host(format!(
            "cannot name {owner}.prototype.hasRef"
        )));
    }
    Ok(prototype)
}

fn timer_id(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<Option<u64>, RootedError> {
    let key = context.string_rooted(TIMER_ID);
    let value = context.get_property_rooted(handle, key)?;
    let value = context.string_text(value)?;
    Ok(value.and_then(|value| value.parse::<u64>().ok()))
}

fn queued_guest_immediate_id(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<Option<u64>, RootedError> {
    let candidates = context
        .host_mut()
        .shared_state()
        .borrow()
        .scheduler
        .shared_guest_immediate_handles();
    for (id, candidate) in candidates {
        if context.same_value_rooted(handle, candidate)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub(crate) fn set_immediate(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = context.rooted_value(callback).map_or("undefined", |value| {
            if value.is_undefined() {
                "undefined"
            } else if value.is_null() {
                "null"
            } else if value.as_number().is_some() {
                "a number"
            } else if value.as_bool().is_some() {
                "a boolean"
            } else {
                "an instance of Object"
            }
        });
        return invalid_callback(context, received);
    }

    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("shared Immediate identifier space exhausted"))?;
    let api = timer_handle_api(context)?;
    let handle = context.object_rooted()?;
    if !context.set_prototype_rooted(handle, api.immediate_prototype)? {
        return Err(RootedError::host("cannot install Immediate prototype"));
    }
    let refed = context.boolean(true);
    if !context.define_data_property_rooted(handle, api.refed_key, refed, true, false, true)? {
        return Err(RootedError::host(
            "cannot initialize Immediate reference state",
        ));
    }
    crate::modules::async_hooks_shared_vm::emit_init(context, handle, "Immediate")?;
    let callback = context.retain(callback)?;
    let receiver = context.retain(handle)?;
    let args = args[1..]
        .iter()
        .copied()
        .map(|argument| context.retain(argument))
        .collect::<Result<Vec<_>, _>>()?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_immediate(
            id,
            crate::modules::shared_event_loop::SharedImmediateOwner::Guest,
            SharedCallback {
                callback,
                receiver,
                args,
            },
        );
    Ok(handle)
}

pub(crate) fn clear_immediate(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(handle) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let id = queued_guest_immediate_id(context, handle)?;
    if let Some(id) = id {
        let api = timer_handle_api(context)?;
        let refed = context.get_property_rooted(handle, api.refed_key)?;
        let was_refed = context.truthy_rooted(refed)?;
        let immediate = context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .scheduler
            .cancel_shared_guest_immediate(id);
        if let Some(immediate) = immediate {
            if was_refed {
                context
                    .host_mut()
                    .shared_state()
                    .borrow_mut()
                    .scheduler
                    .transition_shared_immediate_ref(false);
            }
            release_callback(context, immediate.callback);
            let settled = context.null();
            if !context.set_property_rooted(handle, api.refed_key, settled, handle)? {
                let error = context.type_error_rooted("Cannot settle Immediate reference state")?;
                return Err(context.throw(error));
            }
        }
    }
    Ok(context.undefined())
}

pub(crate) fn clear_timer(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(handle) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let id = timer_id(context, handle)?;
    if let Some(id) = id {
        let callback = context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .scheduler
            .cancel_shared_timer(id);
        if let Some(callback) = callback {
            release_callback(context, callback);
        }
    }
    Ok(context.undefined())
}

fn invalid_callback(
    context: &mut NativeContext<'_, NodeHost>,
    received: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"callback\" argument must be of type function. Received {received}"
    ))?;
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let property = context.string_rooted("code");
    if !context.set_property_rooted(error, property, code, error)? {
        return Err(RootedError::host("cannot set setImmediate error code"));
    }
    Err(context.throw(error))
}

fn install(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install {name}")))
    }
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn is_undefined(context: &NativeContext<'_, NodeHost>, root: RootId) -> bool {
    context
        .rooted_value(root)
        .is_some_and(|value| value.is_undefined())
}

fn property_truthy(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<bool, RootedError> {
    let value = property(context, object, name)?;
    context.truthy_rooted(value)
}

fn property_text(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<String, RootedError> {
    let value = property(context, object, name)?;
    context
        .string_text(value)?
        .ok_or_else(|| RootedError::host(format!("{name} is not a string")))
}

fn operation_pending(
    context: &mut NativeContext<'_, NodeHost>,
    operation: RootId,
) -> Result<bool, RootedError> {
    Ok(property_text(context, operation, PROMISE_STATE)? == PROMISE_PENDING)
}

fn release_callback(context: &mut NativeContext<'_, NodeHost>, callback: SharedCallback) {
    context.release_root(callback.callback);
    context.release_root(callback.receiver);
    for argument in callback.args {
        context.release_root(argument);
    }
}
