//! `Readable.prototype[Symbol.asyncIterator]` and `iterator(options)`: Node's
//! `createAsyncIterator` generator as a state machine over one record per iterator.
//!
//! The record's `error` follows the generator's variable: `undefined` while the stream
//! runs, `null` once it finished cleanly, the error once it failed.

use super::errors;
use super::{
    arg, call_method, captures, closure, define_methods, get, intrinsics, is_null, is_undefined, iterator_result, on,
    put, remove_listener, Context, Key, Outcome, Slot,
};
use quench_runtime::{RootId, RootedError};

/// How the generator stands between requests.
const CREATED: f64 = 0.0;
const SUSPENDED: f64 = 1.0;
const AWAITING: f64 = 2.0;
const COMPLETED: f64 = 3.0;

/// A request queued on the generator.
const NEXT: f64 = 0.0;
const RETURN: f64 = 1.0;
const THROW: f64 = 2.0;

pub(crate) fn async_iterator(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    create(c, this, undefined)
}

pub(crate) fn iterator(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    if !is_undefined(c, options) && (!c.is_object_rooted(options)? || c.is_array_rooted(options)? || c.is_callable_rooted(options)?) {
        return Err(super::super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    create(c, this, options)
}

/// `streamToAsyncIterator(stream, options)`.
fn create(c: &mut Context<'_>, stream: RootId, options: RootId) -> Outcome {
    let record = c.null_object_rooted()?;
    put(c, record, "stream", stream)?;
    put(c, record, "options", options)?;
    super::put_number(c, record, "status", CREATED)?;
    let queue = c.array_rooted(&[])?;
    put(c, record, "queue", queue)?;
    let nop = intrinsics(c)?.nop;
    put(c, record, "callback", nop)?;
    let iterator = c.object_rooted()?;
    define_methods(c, iterator, &[(Key::Symbol(Slot::AsyncIterator), "streamIteratorSelf", "[Symbol.asyncIterator]")])?;
    for (name, operation) in [("next", "streamIteratorNext"), ("return", "streamIteratorReturn"), ("throw", "streamIteratorThrow")] {
        let method = closure(c, operation, name, &[record])?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(iterator, key, method, true, false, true)?;
    }
    put(c, iterator, "stream", stream)?;
    Ok(iterator)
}

pub(crate) fn iterator_self(_: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    Ok(this)
}

fn request(c: &mut Context<'_>, kind: f64, value: RootId) -> Outcome {
    let [record] = captures(c)?;
    let promise = c.pending_promise_rooted()?;
    let entry = c.null_object_rooted()?;
    super::put_number(c, entry, "kind", kind)?;
    put(c, entry, "value", value)?;
    put(c, entry, "promise", promise)?;
    let queue = get(c, record, "queue")?;
    call_method(c, queue, "push", &[entry])?;
    drain(c, record)?;
    Ok(promise)
}

pub(crate) fn next(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    request(c, NEXT, value)
}

pub(crate) fn return_(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    request(c, RETURN, value)
}

pub(crate) fn throw(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    request(c, THROW, value)
}

fn status(c: &mut Context<'_>, record: RootId) -> Result<f64, RootedError> {
    super::number(c, record, "status")
}

fn set_status(c: &mut Context<'_>, record: RootId, status: f64) -> Result<(), RootedError> {
    super::put_number(c, record, "status", status)
}

/// Serve queued requests until one waits for the stream.
fn drain(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let queue = get(c, record, "queue")?;
    while status(c, record)? != AWAITING && super::number(c, queue, "length")? > 0.0 {
        let entry = get(c, queue, "0")?;
        let kind = super::number(c, entry, "kind")?;
        let value = get(c, entry, "value")?;
        let promise = get(c, entry, "promise")?;
        let state = status(c, record)?;
        if state == COMPLETED || (state == CREATED && kind != NEXT) {
            call_method(c, queue, "shift", &[])?;
            set_status(c, record, COMPLETED)?;
            if kind == THROW {
                c.reject_promise_rooted(promise, value)?;
            } else {
                let value = if kind == RETURN { value } else { c.undefined() };
                let done = iterator_result(c, value, true)?;
                c.resolve_promise_rooted(promise, done)?;
            }
            continue;
        }
        if kind == RETURN {
            call_method(c, queue, "shift", &[])?;
            match super::attempt(conclude(c, record))? {
                Ok(_) => {
                    let done = iterator_result(c, value, true)?;
                    c.resolve_promise_rooted(promise, done)?;
                }
                Err(thrown) => c.reject_promise_rooted(promise, thrown)?,
            }
            continue;
        }
        if kind == THROW {
            call_method(c, queue, "shift", &[])?;
            fail(c, record, promise, value)?;
            continue;
        }
        if state == CREATED {
            start(c, record)?;
        }
        call_method(c, queue, "shift", &[])?;
        put(c, record, "current", promise)?;
        step(c, record)?;
    }
    Ok(())
}

/// The generator's start: listen for `readable` and for the end of the stream.
fn start(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    set_status(c, record, SUSPENDED)?;
    let stream = get(c, record, "stream")?;
    let listener = closure(c, "streamIteratorOnReadable", "next", &[record])?;
    put(c, record, "listener", listener)?;
    on(c, stream, "readable", listener)?;
    let finished = closure(c, "streamIteratorOnFinished", "", &[record])?;
    let cleanup = super::eos::eos_with(c, stream, None, Some(false), finished)?;
    put(c, record, "cleanup", cleanup)
}

/// The `readable` listener: wake a waiting step.
pub(crate) fn on_readable(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    wake(c, record)?;
    Ok(c.undefined())
}

/// The end-of-stream callback: record how the stream ended, then wake a waiting step.
pub(crate) fn on_finished(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    let value = if c.truthy_rooted(error)? {
        let held = get(c, record, "error")?;
        errors::aggregate_two(c, held, error)?
    } else {
        c.null()
    };
    put(c, record, "error", value)?;
    wake(c, record)?;
    Ok(c.undefined())
}

/// `callback(); callback = nop`.
fn wake(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let callback = get(c, record, "callback")?;
    let nop = intrinsics(c)?.nop;
    put(c, record, "callback", nop)?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[]).map(drop)
}

/// The step an awaited wake resumes, one microtask after the wake (as `await` resumes).
pub(crate) fn resume(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let continue_step = closure(c, "streamIteratorContinue", "", &[record])?;
    c.queue_microtask_rooted(continue_step, &[])?;
    Ok(c.undefined())
}

pub(crate) fn continue_step(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    set_status(c, record, SUSPENDED)?;
    step(c, record)?;
    drain(c, record)?;
    Ok(c.undefined())
}

/// The generator's loop body for the current `next()`.
fn step(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let stream = get(c, record, "stream")?;
    let promise = get(c, record, "current")?;
    let destroyed = get(c, stream, "destroyed")?;
    let chunk = if c.truthy_rooted(destroyed)? {
        c.null()
    } else {
        match super::attempt(call_method(c, stream, "read", &[]))? {
            Ok(chunk) => chunk,
            Err(thrown) => return fail(c, record, promise, thrown),
        }
    };
    if !is_null(c, chunk) {
        let result = iterator_result(c, chunk, false)?;
        return c.resolve_promise_rooted(promise, result);
    }
    let error = get(c, record, "error")?;
    if c.truthy_rooted(error)? {
        return fail(c, record, promise, error);
    }
    if is_null(c, error) {
        match super::attempt(conclude(c, record))? {
            Ok(_) => {
                let undefined = c.undefined();
                let done = iterator_result(c, undefined, true)?;
                c.resolve_promise_rooted(promise, done)?;
            }
            Err(thrown) => c.reject_promise_rooted(promise, thrown)?,
        }
        return Ok(());
    }
    set_status(c, record, AWAITING)?;
    let wake = closure(c, "streamIteratorResume", "", &[record])?;
    put(c, record, "callback", wake)
}

/// The generator's `catch`: aggregate the error, run `finally`, reject.
fn fail(c: &mut Context<'_>, record: RootId, promise: RootId, thrown: RootId) -> Result<(), RootedError> {
    let held = get(c, record, "error")?;
    let error = errors::aggregate_two(c, held, thrown)?;
    put(c, record, "error", error)?;
    match super::attempt(conclude(c, record))? {
        Ok(_) => c.reject_promise_rooted(promise, error),
        Err(thrown) => c.reject_promise_rooted(promise, thrown),
    }
}

/// The generator's `finally`: destroy the stream, or detach from it when the caller keeps it.
fn conclude(c: &mut Context<'_>, record: RootId) -> Outcome {
    set_status(c, record, COMPLETED)?;
    let stream = get(c, record, "stream")?;
    let error = get(c, record, "error")?;
    let options = get(c, record, "options")?;
    let keep = if is_undefined(c, options) {
        false
    } else {
        let destroy = get(c, options, "destroyOnReturn")?;
        c.rooted_value(destroy).and_then(|value| value.as_bool()) == Some(false)
    };
    let auto_destroy = if is_undefined(c, error) {
        true
    } else {
        let state = get(c, stream, "_readableState")?;
        let auto = get(c, state, "autoDestroy")?;
        c.truthy_rooted(auto)?
    };
    if (c.truthy_rooted(error)? || !keep) && auto_destroy {
        let null = c.null();
        super::destroy::destroyer(c, stream, null)?;
    } else {
        let listener = get(c, record, "listener")?;
        remove_listener(c, stream, "readable", listener)?;
        let cleanup = get(c, record, "cleanup")?;
        super::eos::run_cleanup(c, cleanup)?;
    }
    Ok(c.undefined())
}
