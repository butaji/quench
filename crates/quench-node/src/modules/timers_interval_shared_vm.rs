//! `timers/promises.setInterval(delay, value, options)`: an async iterator over an interval
//! timer's ticks, the state machine of Node's async generator.
//!
//! The generator's body runs on the first `next()`: it validates the options, rejects at
//! once for an aborted signal, and starts the interval. Every tick either settles the oldest
//! waiting `next()` or is counted; counted ticks are yielded before an abort is observed. An
//! abort clears the interval and rejects the waiting `next()` with an `AbortError`; `return`
//! and `throw` finish the generator.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The generator's lifecycle: not yet run, waiting for ticks, or finished.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Fresh,
    Running,
    Done,
}

impl Phase {
    const ALL: [Phase; 3] = [Phase::Fresh, Phase::Running, Phase::Done];

    fn code(self) -> f64 {
        Self::ALL.iter().position(|phase| *phase == self).unwrap_or_default() as f64
    }

    fn of(code: f64) -> Phase {
        Self::ALL.get(code as usize).copied().unwrap_or(Phase::Done)
    }
}

const PHASE: &str = "phase";
const DELAY: &str = "delay";
const VALUE: &str = "value";
const OPTIONS: &str = "options";
const TICKS: &str = "ticks";
const WAITERS: &str = "waiters";
const HANDLE: &str = "handle";
const SIGNAL: &str = "signal";
const LISTENER: &str = "listener";
const ABORT_EVENT: &str = "abort";

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn phase(c: &mut Context<'_>, state: RootId) -> Result<Phase, RootedError> {
    let code = get(c, state, PHASE)?;
    Ok(Phase::of(c.to_number(code)?))
}

fn set_phase(c: &mut Context<'_>, state: RootId, phase: Phase) -> Result<(), RootedError> {
    let code = c.number(phase.code());
    put(c, state, PHASE, code)
}

/// `{ value, done }`.
fn iteration_result(c: &mut Context<'_>, value: RootId, done: bool) -> Result<RootId, RootedError> {
    let result = c.object_rooted()?;
    put(c, result, "value", value)?;
    let done = c.boolean(done);
    put(c, result, "done", done)?;
    Ok(result)
}

fn settle(c: &mut Context<'_>, promise: RootId, value: RootId, done: bool) -> Result<(), RootedError> {
    let result = iteration_result(c, value, done)?;
    c.resolve_promise_rooted(promise, result)
}

/// `setInterval(delay, value, options)`.
pub(crate) fn set_interval(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.null_object_rooted()?;
    for (index, name) in [DELAY, VALUE, OPTIONS].into_iter().enumerate() {
        let value = args.get(index).copied().unwrap_or_else(|| c.undefined());
        put(c, state, name, value)?;
    }
    set_phase(c, state, Phase::Fresh)?;
    let zero = c.number(0.0);
    put(c, state, TICKS, zero)?;
    let waiters = c.array_rooted(&[])?;
    put(c, state, WAITERS, waiters)?;
    let iterator = c.object_rooted()?;
    for (name, operation) in [
        ("next", "timersIntervalNext"),
        ("return", "timersIntervalReturn"),
        ("throw", "timersIntervalThrow"),
    ] {
        let method = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, state)?;
        c.define_named_data_property_rooted(iterator, name, method, true, false, true)?;
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = get(c, symbol, "asyncIterator")?;
    let identity =
        c.host_function_named(crate::host::shared_vm::operation("timersIntervalIterator"), "[Symbol.asyncIterator]")?;
    c.define_data_property_rooted(iterator, key, identity, true, false, true)?;
    Ok(iterator)
}

/// `[Symbol.asyncIterator]()`: the iterator itself.
pub(crate) fn iterator(_: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(receiver)
}

/// The generator body's prologue: `validateObject(options)`, the signal and `ref` checks,
/// then the interval and its abort listener. Returns the rejection reason when the body
/// throws before its first `await`.
fn start(c: &mut Context<'_>, state: RootId) -> Result<Option<RootId>, RootedError> {
    let options = get(c, state, OPTIONS)?;
    let (signal, referenced) = if is_undefined(c, options) {
        (c.undefined(), true)
    } else {
        let valid = c.is_object_rooted(options)?
            && !c.is_callable_rooted(options)?
            && !c.is_array_rooted(options)?
            && !c.rooted_value(options).is_some_and(|value| value.is_null());
        if !valid {
            return thrown(super::util_shared_vm::argument_type_error(c, options, "options", "Object"));
        }
        let signal = get(c, options, SIGNAL)?;
        if let Err(error) = super::util_shared_vm::validate_abort_signal(c, signal, "options.signal") {
            return thrown(Ok(error));
        }
        let referenced = get(c, options, "ref")?;
        let referenced = if is_undefined(c, referenced) {
            true
        } else if let Some(flag) = c.rooted_value(referenced).and_then(|value| value.as_bool()) {
            flag
        } else {
            return thrown(super::util_shared_vm::argument_type_error(c, referenced, "options.ref", "boolean"));
        };
        (signal, referenced)
    };
    put(c, state, SIGNAL, signal)?;
    if !is_undefined(c, signal) && aborted(c, signal)? {
        return super::timers_shared_vm::abort_error(c, signal).map(Some);
    }
    let tick = c.host_function_with_data(crate::host::shared_vm::operation("timersIntervalTick"), state)?;
    let delay = get(c, state, DELAY)?;
    let handle = super::timers_shared_vm::schedule_timer(c, &[tick, delay], true, referenced)?;
    put(c, state, HANDLE, handle)?;
    if !is_undefined(c, signal) {
        let listener = c.host_function_with_data(crate::host::shared_vm::operation("timersIntervalAbort"), state)?;
        put(c, state, LISTENER, listener)?;
        let add = get(c, signal, "addEventListener")?;
        let event = c.string_rooted(ABORT_EVENT);
        let options = c.object_rooted()?;
        let yes = c.boolean(true);
        put(c, options, "once", yes)?;
        c.call_rooted(add, signal, &[event, listener, options])?;
    }
    Ok(None)
}

/// A throw of the generator's prologue as its rejection reason.
fn thrown(error: Result<RootedError, RootedError>) -> Result<Option<RootId>, RootedError> {
    let error = error?;
    match error.exception {
        Some(exception) => Ok(Some(exception)),
        None => Err(error),
    }
}

fn aborted(c: &mut Context<'_>, signal: RootId) -> Result<bool, RootedError> {
    let aborted = get(c, signal, "aborted")?;
    c.truthy_rooted(aborted)
}

/// The generator's `finally`: the interval is cleared and the abort listener removed.
fn finish(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    set_phase(c, state, Phase::Done)?;
    let handle = get(c, state, HANDLE)?;
    if !is_undefined(c, handle) {
        let undefined = c.undefined();
        super::timers_shared_vm::clear_timer(c, undefined, &[handle])?;
    }
    let signal = get(c, state, SIGNAL)?;
    let listener = get(c, state, LISTENER)?;
    if !is_undefined(c, signal) && !is_undefined(c, listener) {
        let remove = get(c, signal, "removeEventListener")?;
        let event = c.string_rooted(ABORT_EVENT);
        c.call_rooted(remove, signal, &[event, listener])?;
    }
    Ok(())
}

/// The waiting `next()` promises, oldest first, taken from the state.
fn take_waiters(c: &mut Context<'_>, state: RootId) -> Result<Vec<RootId>, RootedError> {
    let waiters = get(c, state, WAITERS)?;
    let taken = c.array_like_values_rooted(waiters)?;
    let empty = c.array_rooted(&[])?;
    put(c, state, WAITERS, empty)?;
    Ok(taken)
}

/// `next()`.
pub(crate) fn next(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let promise = c.pending_promise_rooted()?;
    if phase(c, state)? == Phase::Fresh {
        set_phase(c, state, Phase::Running)?;
        if let Some(reason) = start(c, state)? {
            finish(c, state)?;
            c.reject_promise_rooted(promise, reason)?;
            return Ok(promise);
        }
    }
    let undefined = c.undefined();
    if phase(c, state)? == Phase::Done {
        settle(c, promise, undefined, true)?;
        return Ok(promise);
    }
    let ticks = get(c, state, TICKS)?;
    let ticks = c.to_number(ticks)?;
    if ticks > 0.0 {
        let remaining = c.number(ticks - 1.0);
        put(c, state, TICKS, remaining)?;
        let value = get(c, state, VALUE)?;
        settle(c, promise, value, false)?;
        return Ok(promise);
    }
    let signal = get(c, state, SIGNAL)?;
    if !is_undefined(c, signal) && aborted(c, signal)? {
        finish(c, state)?;
        let error = super::timers_shared_vm::abort_error(c, signal)?;
        c.reject_promise_rooted(promise, error)?;
        return Ok(promise);
    }
    let waiters = get(c, state, WAITERS)?;
    let push = get(c, waiters, "push")?;
    c.call_rooted(push, waiters, &[promise])?;
    Ok(promise)
}

/// The interval's callback: the oldest waiting `next()` receives the value, or the tick is
/// counted.
pub(crate) fn tick(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let waiters = get(c, state, WAITERS)?;
    let shift = get(c, waiters, "shift")?;
    let waiter = c.call_rooted(shift, waiters, &[])?;
    if is_undefined(c, waiter) {
        let ticks = get(c, state, TICKS)?;
        let ticks = c.to_number(ticks)?;
        let counted = c.number(ticks + 1.0);
        put(c, state, TICKS, counted)?;
    } else {
        let value = get(c, state, VALUE)?;
        settle(c, waiter, value, false)?;
    }
    Ok(c.undefined())
}

/// The signal's `abort` listener: the interval stops; a waiting `next()` rejects with an
/// `AbortError` and later ones complete.
pub(crate) fn abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let handle = get(c, state, HANDLE)?;
    let undefined = c.undefined();
    super::timers_shared_vm::clear_timer(c, undefined, &[handle])?;
    let waiters = take_waiters(c, state)?;
    if let Some((&first, rest)) = waiters.split_first() {
        finish(c, state)?;
        let signal = get(c, state, SIGNAL)?;
        let error = super::timers_shared_vm::abort_error(c, signal)?;
        c.reject_promise_rooted(first, error)?;
        for &waiter in rest {
            settle(c, waiter, undefined, true)?;
        }
    }
    Ok(undefined)
}

/// `return(value)`: the generator finishes with `value`.
pub(crate) fn finish_with_return(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    let promise = c.pending_promise_rooted()?;
    if phase(c, state)? != Phase::Done {
        finish(c, state)?;
    }
    let undefined = c.undefined();
    for waiter in take_waiters(c, state)? {
        settle(c, waiter, undefined, true)?;
    }
    settle(c, promise, value, true)?;
    Ok(promise)
}

/// `throw(error)`: the generator finishes by throwing `error`.
pub(crate) fn finish_with_throw(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let error = args.first().copied().unwrap_or_else(|| c.undefined());
    let promise = c.pending_promise_rooted()?;
    if phase(c, state)? != Phase::Done {
        finish(c, state)?;
    }
    let undefined = c.undefined();
    for waiter in take_waiters(c, state)? {
        settle(c, waiter, undefined, true)?;
    }
    c.reject_promise_rooted(promise, error)?;
    Ok(promise)
}
