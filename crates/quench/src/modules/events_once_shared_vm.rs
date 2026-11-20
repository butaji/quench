//! `events.once` and `events.on`: an emitter's events as a promise and as an async iterator,
//! over EventEmitters and EventTargets alike (`eventTargetAgnosticAddListener`).
use super::event_target_shared_vm::{api, argument, flag, get, is_nullish, is_undefined, put, put_flag};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

use crate::modules::keys::ERROR_EVENT;
use crate::modules::keys::ABORT_EVENT;

/// The fields of a pending `once` and of an `on` iterator.
mod field {
    pub(super) const EMITTER: &str = "emitter";
    pub(crate) use crate::modules::keys::NAME;
    pub(crate) use crate::modules::keys::SIGNAL;
    pub(crate) use crate::modules::keys::PROMISE;
    pub(super) const SETTLED: &str = "settled";
    pub(super) const RESOLVER: &str = "resolver";
    pub(super) const ERROR_LISTENER: &str = "errorListener";
    pub(super) const ABORT_LISTENER: &str = "abortListener";
    pub(super) const EVENTS: &str = "events";
    pub(crate) use crate::modules::keys::WAITERS;
    pub(crate) use crate::modules::keys::ERROR;
    pub(super) const HAS_ERROR: &str = "hasError";
    pub(super) const FINISHED: &str = "finished";
    pub(super) const PAUSED: &str = "paused";
    pub(super) const HIGH: &str = "highWaterMark";
    pub(super) const LOW: &str = "lowWaterMark";
    pub(super) const LISTENERS: &str = "listeners";
    pub(super) const DISPOSABLE: &str = "disposable";
}

/// `on`'s default water marks.
const DEFAULT_LOW_WATER_MARK: f64 = 1.0;
const MIN_WATER_MARK: f64 = 1.0;

fn callable(c: &mut Context<'_>, object: RootId, name: &str) -> Result<Option<RootId>, RootedError> {
    let method = get(c, object, name)?;
    Ok(c.is_callable_rooted(method)?.then_some(method))
}

fn invalid_emitter(c: &mut Context<'_>, emitter: RootId) -> RootedError {
    match super::util_shared_vm::argument_type_error(c, emitter, "emitter", "EventEmitter") {
        Ok(error) | Err(error) => error,
    }
}

/// `eventTargetAgnosticAddListener`: `on`/`once` of an EventEmitter, else `addEventListener`
/// with `{ once, [kResistStopPropagation] }` when `once`.
fn add_listener(c: &mut Context<'_>, emitter: RootId, name: RootId, listener: RootId, once: bool) -> Result<(), RootedError> {
    if callable(c, emitter, "on")?.is_some() {
        let method = get(c, emitter, if once { "once" } else { "on" })?;
        c.call_rooted(method, emitter, &[name, listener])?;
    } else if let Some(add) = callable(c, emitter, "addEventListener")? {
        let options = if once {
            let data = api(c)?;
            get(c, data, "abortListenerOptions")?
        } else {
            c.undefined()
        };
        c.call_rooted(add, emitter, &[name, listener, options])?;
    } else {
        return Err(invalid_emitter(c, emitter));
    }
    Ok(())
}

/// `eventTargetAgnosticRemoveListener`.
fn remove_listener(c: &mut Context<'_>, emitter: RootId, name: RootId, listener: RootId) -> Result<(), RootedError> {
    if let Some(remove) = callable(c, emitter, "removeListener")? {
        c.call_rooted(remove, emitter, &[name, listener])?;
    } else if let Some(remove) = callable(c, emitter, "removeEventListener")? {
        c.call_rooted(remove, emitter, &[name, listener])?;
    } else {
        return Err(invalid_emitter(c, emitter));
    }
    Ok(())
}

/// `validateObject(options, 'options')` and the `signal` option's validation; the signal.
fn options_signal(c: &mut Context<'_>, options: RootId) -> Result<RootId, RootedError> {
    if is_undefined(c, options) {
        return Ok(options);
    }
    let valid = c.is_object_rooted(options)? && !c.is_callable_rooted(options)? && !c.is_array_rooted(options)?;
    if !valid || is_nullish(c, options) {
        return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let signal = get(c, options, field::SIGNAL)?;
    super::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
    Ok(signal)
}

fn signal_aborted(c: &mut Context<'_>, signal: RootId) -> Result<bool, RootedError> {
    Ok(!is_nullish(c, signal) && super::abort_controller_shared_vm::aborted(c, signal)?)
}

fn abort_error(c: &mut Context<'_>, signal: RootId) -> Result<RootId, RootedError> {
    super::timers_shared_vm::abort_error(c, signal)
}

fn string(c: &mut Context<'_>, text: &str) -> RootId {
    c.string_rooted(text)
}

/// `once(emitter, name, options)`: a promise of the next `name` event's arguments, rejected
/// by an `error` event of an EventEmitter or by the signal's abort.
pub(crate) fn once(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let promise = c.pending_promise_rooted()?;
    if let Err(error) = start_once(c, promise, args) {
        let Some(reason) = error.exception else { return Err(error) };
        c.reject_promise_rooted(promise, reason)?;
    }
    Ok(promise)
}

fn start_once(c: &mut Context<'_>, promise: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let emitter = argument(c, args, 0);
    let name = argument(c, args, 1);
    let options = argument(c, args, 2);
    let signal = options_signal(c, options)?;
    if signal_aborted(c, signal)? {
        let error = abort_error(c, signal)?;
        return Err(c.throw(error));
    }
    let state = c.null_object_rooted()?;
    for (key, value) in [(field::EMITTER, emitter), (field::NAME, name), (field::SIGNAL, signal), (field::PROMISE, promise)] {
        put(c, state, key, value)?;
    }
    put_flag(c, state, field::SETTLED, false)?;
    let resolver = c.host_function_with_data(crate::host::shared_vm::operation!("eventsOnceResolve"), state)?;
    let error_listener = c.host_function_with_data(crate::host::shared_vm::operation!("eventsOnceError"), state)?;
    let abort_listener = c.host_function_named_with_data(crate::host::shared_vm::operation!("eventsOnceAbort"), "abortListener", state)?;
    put(c, state, field::RESOLVER, resolver)?;
    put(c, state, field::ERROR_LISTENER, error_listener)?;
    put(c, state, field::ABORT_LISTENER, abort_listener)?;
    add_listener(c, emitter, name, resolver, true)?;
    let error_name = c.string_text(name)?.is_some_and(|text| text == ERROR_EVENT);
    if !error_name {
        if let Some(once) = callable(c, emitter, "once")? {
            let event = string(c, ERROR_EVENT);
            c.call_rooted(once, emitter, &[event, error_listener])?;
        }
    }
    if !is_nullish(c, signal) {
        let event = string(c, ABORT_EVENT);
        add_listener(c, signal, event, abort_listener, true)?;
    }
    Ok(())
}

/// Settles a pending `once` exactly once.
fn settle_once(c: &mut Context<'_>, state: RootId, outcome: Result<RootId, RootId>) -> Result<RootId, RootedError> {
    if !flag(c, state, field::SETTLED)? {
        put_flag(c, state, field::SETTLED, true)?;
        let promise = get(c, state, field::PROMISE)?;
        match outcome {
            Ok(value) => c.resolve_promise_rooted(promise, value)?,
            Err(reason) => c.reject_promise_rooted(promise, reason)?,
        }
    }
    Ok(c.undefined())
}

fn remove_abort(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let signal = get(c, state, field::SIGNAL)?;
    if !is_nullish(c, signal) {
        let listener = get(c, state, field::ABORT_LISTENER)?;
        let event = string(c, ABORT_EVENT);
        remove_listener(c, signal, event, listener)?;
    }
    Ok(())
}

/// The event's listener: resolves with its arguments.
pub(crate) fn once_resolve(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let emitter = get(c, state, field::EMITTER)?;
    if let Some(remove) = callable(c, emitter, "removeListener")? {
        let event = string(c, ERROR_EVENT);
        let listener = get(c, state, field::ERROR_LISTENER)?;
        c.call_rooted(remove, emitter, &[event, listener])?;
    }
    remove_abort(c, state)?;
    let values = c.array_rooted(args)?;
    settle_once(c, state, Ok(values))
}

/// The EventEmitter's `error` listener: rejects with the error.
pub(crate) fn once_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let emitter = get(c, state, field::EMITTER)?;
    let name = get(c, state, field::NAME)?;
    let resolver = get(c, state, field::RESOLVER)?;
    let remove = get(c, emitter, "removeListener")?;
    c.call_rooted(remove, emitter, &[name, resolver])?;
    remove_abort(c, state)?;
    let error = argument(c, args, 0);
    settle_once(c, state, Err(error))
}

/// The signal's listener: rejects with an `AbortError`.
pub(crate) fn once_abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let emitter = get(c, state, field::EMITTER)?;
    let name = get(c, state, field::NAME)?;
    let resolver = get(c, state, field::RESOLVER)?;
    remove_listener(c, emitter, name, resolver)?;
    let event = string(c, ERROR_EVENT);
    let error_listener = get(c, state, field::ERROR_LISTENER)?;
    remove_listener(c, emitter, event, error_listener)?;
    let signal = get(c, state, field::SIGNAL)?;
    let error = abort_error(c, signal)?;
    settle_once(c, state, Err(error))
}

/// `options[name] ?? options[legacy] ?? fallback`, validated as an integer of at least one.
fn water_mark(c: &mut Context<'_>, options: RootId, name: &str, legacy: &str, fallback: f64) -> Result<f64, RootedError> {
    let mut value = c.undefined();
    if !is_undefined(c, options) {
        for key in [name, legacy] {
            value = get(c, options, key)?;
            if !is_nullish(c, value) {
                break;
            }
        }
    }
    if is_nullish(c, value) {
        value = c.number(fallback);
    }
    let maximum = c.number(super::util_shared_vm::MAX_SAFE_INTEGER);
    super::util_shared_vm::validate_integer(c, value, &format!("options.{name}"), MIN_WATER_MARK, maximum)
}

/// `{ value, done }`.
fn iteration_result(c: &mut Context<'_>, value: RootId, done: bool) -> Result<RootId, RootedError> {
    let result = c.object_rooted()?;
    put(c, result, "value", value)?;
    put_flag(c, result, "done", done)?;
    Ok(result)
}

fn resolved(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let promise = c.pending_promise_rooted()?;
    c.resolve_promise_rooted(promise, value)?;
    Ok(promise)
}

/// `on(emitter, event, options)`: an async iterator of the event's argument lists. Events
/// queue until read, pausing the emitter past the high water mark; an EventEmitter's
/// `error`, the signal's abort, or one of `options.close`'s events ends it.
pub(crate) fn on(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let emitter = argument(c, args, 0);
    let name = argument(c, args, 1);
    let options = argument(c, args, 2);
    let signal = options_signal(c, options)?;
    if signal_aborted(c, signal)? {
        let error = abort_error(c, signal)?;
        return Err(c.throw(error));
    }
    let high = water_mark(c, options, "highWaterMark", "highWatermark", super::util_shared_vm::MAX_SAFE_INTEGER)?;
    let low = water_mark(c, options, "lowWaterMark", "lowWatermark", DEFAULT_LOW_WATER_MARK)?;
    let state = c.null_object_rooted()?;
    put(c, state, field::EMITTER, emitter)?;
    for key in [field::EVENTS, field::WAITERS, field::LISTENERS] {
        let list = c.array_rooted(&[])?;
        put(c, state, key, list)?;
    }
    for key in [field::HAS_ERROR, field::FINISHED, field::PAUSED] {
        put_flag(c, state, key, false)?;
    }
    for (key, value) in [(field::HIGH, high), (field::LOW, low)] {
        let value = c.number(value);
        put(c, state, key, value)?;
    }
    let iterator = c.object_rooted()?;
    let operation = crate::host::shared_vm::operation;
    for (method, op) in [("next", "eventsOnNext"), ("return", "eventsOnReturn"), ("throw", "eventsOnThrow")] {
        let function = c.host_function_named_with_data(operation(op), method, state)?;
        c.define_named_data_property_rooted(iterator, method, function, true, true, true)?;
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = get(c, symbol, "asyncIterator")?;
    let identity = c.host_function_named(crate::host::shared_vm::operation!("timersIntervalIterator"), "[Symbol.asyncIterator]")?;
    c.define_data_property_rooted(iterator, key, identity, true, true, true)?;
    let handler = c.host_function_with_data(crate::host::shared_vm::operation!("eventsOnEvent"), state)?;
    track_listener(c, state, emitter, name, handler)?;
    let error_name = c.string_text(name)?.is_some_and(|text| text == ERROR_EVENT);
    if !error_name && callable(c, emitter, "on")?.is_some() {
        let error_handler = c.host_function_with_data(crate::host::shared_vm::operation!("eventsOnError"), state)?;
        let event = string(c, ERROR_EVENT);
        track_listener(c, state, emitter, event, error_handler)?;
    }
    let close_events = if is_undefined(c, options) { options } else { get(c, options, "close")? };
    if !is_nullish(c, close_events) && flag(c, close_events, "length")? {
        let close = c.host_function_with_data(crate::host::shared_vm::operation!("eventsOnClose"), state)?;
        for event in c.array_like_values_rooted(close_events)? {
            track_listener(c, state, emitter, event, close)?;
        }
    }
    if !is_nullish(c, signal) {
        let abort = c.host_function_with_data(crate::host::shared_vm::operation!("eventsOnAbort"), state)?;
        let undefined = c.undefined();
        let disposable = super::events_shared_vm::add_abort_listener(c, undefined, &[signal, abort])?;
        put(c, state, field::DISPOSABLE, disposable)?;
        put(c, state, field::SIGNAL, signal)?;
    }
    Ok(iterator)
}

/// `listenersController().addEventListener`: registers and remembers for `removeAll`.
fn track_listener(c: &mut Context<'_>, state: RootId, emitter: RootId, name: RootId, handler: RootId) -> Result<(), RootedError> {
    add_listener(c, emitter, name, handler, false)?;
    let entry = c.array_rooted(&[emitter, name, handler])?;
    let listeners = get(c, state, field::LISTENERS)?;
    let push = get(c, listeners, "push")?;
    c.call_rooted(push, listeners, &[entry])?;
    Ok(())
}

fn shift(c: &mut Context<'_>, state: RootId, list: &str) -> Result<Option<RootId>, RootedError> {
    let list = get(c, state, list)?;
    let length = get(c, list, "length")?;
    if c.to_number(length)? == 0.0 {
        return Ok(None);
    }
    let shift = get(c, list, "shift")?;
    c.call_rooted(shift, list, &[]).map(Some)
}

fn push(c: &mut Context<'_>, state: RootId, list: &str, value: RootId) -> Result<f64, RootedError> {
    let list = get(c, state, list)?;
    let push = get(c, list, "push")?;
    let length = c.call_rooted(push, list, &[value])?;
    c.to_number(length)
}

fn length(c: &mut Context<'_>, state: RootId, list: &str) -> Result<f64, RootedError> {
    let list = get(c, state, list)?;
    let length = get(c, list, "length")?;
    c.to_number(length)
}

fn number(c: &mut Context<'_>, state: RootId, name: &str) -> Result<f64, RootedError> {
    let value = get(c, state, name)?;
    c.to_number(value)
}

/// `next()`: a queued event, then a pending error, then completion, else a wait.
pub(crate) fn on_next(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    if let Some(value) = shift(c, state, field::EVENTS)? {
        if flag(c, state, field::PAUSED)? && length(c, state, field::EVENTS)? < number(c, state, field::LOW)? {
            let emitter = get(c, state, field::EMITTER)?;
            let resume = get(c, emitter, "resume")?;
            c.call_rooted(resume, emitter, &[])?;
            put_flag(c, state, field::PAUSED, false)?;
        }
        let result = iteration_result(c, value, false)?;
        return resolved(c, result);
    }
    if flag(c, state, field::HAS_ERROR)? {
        put_flag(c, state, field::HAS_ERROR, false)?;
        let error = get(c, state, field::ERROR)?;
        let promise = c.pending_promise_rooted()?;
        c.reject_promise_rooted(promise, error)?;
        return Ok(promise);
    }
    if flag(c, state, field::FINISHED)? {
        return close(c, state);
    }
    let promise = c.pending_promise_rooted()?;
    push(c, state, field::WAITERS, promise)?;
    Ok(promise)
}

pub(crate) fn on_return(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    close(c, state)
}

/// `throw(error)`: ends the iterator with `error`, which must be an Error.
pub(crate) fn on_throw(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let error = argument(c, args, 0);
    let global = c.global_root()?;
    let error_class = get(c, global, "Error")?;
    if !c.truthy_rooted(error)? || !c.instance_of_rooted(error, error_class)? {
        return Err(super::util_shared_vm::argument_type_error(c, error, "EventEmitter.AsyncIterator", "Error")?);
    }
    fail(c, state, error)?;
    Ok(c.undefined())
}

/// The event's listener: its arguments go to a waiting `next()` or the queue.
pub(crate) fn on_event(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let value = c.array_rooted(args)?;
    match shift(c, state, field::WAITERS)? {
        Some(waiter) => {
            let result = iteration_result(c, value, false)?;
            c.resolve_promise_rooted(waiter, result)?;
        }
        None => {
            let size = push(c, state, field::EVENTS, value)?;
            if !flag(c, state, field::PAUSED)? && size > number(c, state, field::HIGH)? {
                put_flag(c, state, field::PAUSED, true)?;
                let emitter = get(c, state, field::EMITTER)?;
                let pause = get(c, emitter, "pause")?;
                c.call_rooted(pause, emitter, &[])?;
            }
        }
    }
    Ok(c.undefined())
}

pub(crate) fn on_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let error = argument(c, args, 0);
    fail(c, state, error)?;
    Ok(c.undefined())
}

pub(crate) fn on_abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let signal = get(c, state, field::SIGNAL)?;
    let error = abort_error(c, signal)?;
    fail(c, state, error)?;
    Ok(c.undefined())
}

pub(crate) fn on_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    close(c, state)
}

/// `errorHandler`: a waiting `next()` rejects, else the error waits for one; then `close`.
fn fail(c: &mut Context<'_>, state: RootId, error: RootId) -> Result<(), RootedError> {
    match shift(c, state, field::WAITERS)? {
        Some(waiter) => c.reject_promise_rooted(waiter, error)?,
        None => {
            put(c, state, field::ERROR, error)?;
            put_flag(c, state, field::HAS_ERROR, true)?;
        }
    }
    close(c, state)?;
    Ok(())
}

/// `closeHandler`: the abort listener and every listener go, waiting `next()`s complete.
fn close(c: &mut Context<'_>, state: RootId) -> Result<RootId, RootedError> {
    let disposable = get(c, state, field::DISPOSABLE)?;
    if !is_undefined(c, disposable) {
        let data = api(c)?;
        let key = get(c, data, "disposeKey")?;
        let dispose = c.get_property_rooted(disposable, key)?;
        c.call_rooted(dispose, disposable, &[])?;
        let undefined = c.undefined();
        put(c, state, field::DISPOSABLE, undefined)?;
    }
    let listeners = get(c, state, field::LISTENERS)?;
    for entry in c.array_like_values_rooted(listeners)? {
        let parts = c.array_like_values_rooted(entry)?;
        if let [emitter, name, handler] = parts[..] {
            remove_listener(c, emitter, name, handler)?;
        }
    }
    let empty = c.array_rooted(&[])?;
    put(c, state, field::LISTENERS, empty)?;
    put_flag(c, state, field::FINISHED, true)?;
    let undefined = c.undefined();
    while let Some(waiter) = shift(c, state, field::WAITERS)? {
        let done = iteration_result(c, undefined, true)?;
        c.resolve_promise_rooted(waiter, done)?;
    }
    let done = iteration_result(c, undefined, true)?;
    resolved(c, done)
}
