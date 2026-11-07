//! `Readable.from(iterable, opts)`: a readable pulling from a sync or async iterator, as
//! Node's `internal/streams/from` with its async functions as promise continuations.

use super::errors::{self, StreamError};
use super::readable::is_buffer;
use super::{
    arg, await_then, call_method, captures, closure, get, intrinsic, intrinsics, is_null, is_nullish, is_string,
    next_tick, put, put_bool, Class, Context, Outcome, Slot,
};
use quench_runtime::{RootId, RootedError};

/// `{ objectMode: true, ...defaults, ...opts }` with the members `extra` adds.
fn options(c: &mut Context<'_>, high_water_mark: bool, opts: RootId) -> Outcome {
    let options = c.object_rooted()?;
    put_bool(c, options, "objectMode", true)?;
    if high_water_mark {
        super::put_number(c, options, "highWaterMark", 1.0)?;
    }
    if !is_nullish(c, opts) {
        let global = c.global_root()?;
        let object = get(c, global, "Object")?;
        call_method(c, object, "assign", &[options, opts])?;
    }
    Ok(options)
}

fn new_readable(c: &mut Context<'_>, options: RootId) -> Outcome {
    let readable = intrinsic(c, |i| i.class(Class::Readable))?;
    c.construct_rooted(readable, readable, &[options])
}

pub(crate) fn from(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (iterable, opts) = (arg(c, args, 0), arg(c, args, 1));
    if is_string(c, iterable) || is_buffer(c, iterable)? {
        let options = options(c, false, opts)?;
        let read = closure(c, "streamFromWholeRead", "read", &[iterable])?;
        put(c, options, "read", read)?;
        return new_readable(c, options);
    }
    let intrinsics = intrinsics(c)?;
    let (method, asynchronous) = if is_nullish(c, iterable) {
        (None, false)
    } else {
        let method = c.get_property_rooted(iterable, intrinsics.slot(Slot::AsyncIterator))?;
        if c.truthy_rooted(method)? {
            (Some(method), true)
        } else {
            let method = c.get_property_rooted(iterable, intrinsics.slot(Slot::Iterator))?;
            (c.truthy_rooted(method)?.then_some(method), false)
        }
    };
    let Some(method) = method else {
        let error = errors::invalid_argument_type(c, iterable, "iterable", "an instance of Iterable")?;
        return Err(c.throw(error));
    };
    let iterator = c.call_rooted(method, iterable, &[])?;
    let options = options(c, true, opts)?;
    let readable = new_readable(c, options)?;
    let record = c.null_object_rooted()?;
    put(c, record, "iterator", iterator)?;
    put(c, record, "readable", readable)?;
    put_bool(c, record, "reading", false)?;
    put_bool(c, record, "isAsync", asynchronous)?;
    put_bool(c, record, "isAsyncValues", false)?;
    let read = closure(c, "streamFromRead", "", &[record])?;
    put(c, readable, "_read", read)?;
    let destroy = closure(c, "streamFromDestroy", "", &[record])?;
    put(c, readable, "_destroy", destroy)?;
    Ok(readable)
}

/// The `read()` of a readable over one string or Buffer.
pub(crate) fn whole_read(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let [value] = captures(c)?;
    call_method(c, this, "push", &[value])?;
    let null = c.null();
    call_method(c, this, "push", &[null])?;
    Ok(c.undefined())
}

pub(crate) fn read(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    if !super::truthy_member(c, record, "reading")? {
        put_bool(c, record, "reading", true)?;
        if super::truthy_member(c, record, "isAsync")? {
            next_async(c, record)?;
        } else if super::truthy_member(c, record, "isAsyncValues")? {
            next_sync_with_async_values(c, record)?;
        } else {
            next_sync(c, record)?;
        }
    }
    Ok(c.undefined())
}

fn readable(c: &mut Context<'_>, record: RootId) -> Outcome {
    get(c, record, "readable")
}

fn destroy_with(c: &mut Context<'_>, record: RootId, error: RootId) -> Result<(), RootedError> {
    let readable = readable(c, record)?;
    call_method(c, readable, "destroy", &[error]).map(drop)
}

/// A `null` value from the iterator: stop reading and fail.
fn null_value(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    put_bool(c, record, "reading", false)?;
    let error = errors::create(c, StreamError::NullValues)?;
    destroy_with(c, record, error)
}

/// What one iterator value did to the loop.
enum Pushed {
    More,
    Stop,
}

/// `if (readable.push(value)) continue; reading = false;`.
fn push_value(c: &mut Context<'_>, record: RootId, value: RootId) -> Result<Result<Pushed, RootId>, RootedError> {
    if is_null(c, value) {
        null_value(c, record)?;
        return Ok(Ok(Pushed::Stop));
    }
    let readable = readable(c, record)?;
    match super::attempt(call_method(c, readable, "push", &[value]))? {
        Ok(more) if c.truthy_rooted(more)? => Ok(Ok(Pushed::More)),
        Ok(_) => {
            put_bool(c, record, "reading", false)?;
            Ok(Ok(Pushed::Stop))
        }
        Err(thrown) => Ok(Err(thrown)),
    }
}

/// `const { value, done } = result`.
fn unpack(c: &mut Context<'_>, result: RootId) -> Result<(RootId, bool), RootedError> {
    let value = get(c, result, "value")?;
    let done = get(c, result, "done")?;
    Ok((value, c.truthy_rooted(done)?))
}

fn end(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let readable = readable(c, record)?;
    let null = c.null();
    call_method(c, readable, "push", &[null]).map(drop)
}

fn is_thenable(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !c.truthy_rooted(value)? {
        return Ok(false);
    }
    let then = get(c, value, "then")?;
    c.is_callable_rooted(then)
}

/// `nextSyncWithSyncValues()`.
fn next_sync(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let iterator = get(c, record, "iterator")?;
    loop {
        let step = super::attempt(next_result(c, iterator))?;
        let (value, done) = match step {
            Ok(step) => step,
            Err(thrown) => return destroy_with(c, record, thrown),
        };
        if done {
            let ended = end(c, record);
            return attempted(c, record, ended);
        }
        match super::attempt(is_thenable(c, value))? {
            Ok(true) => return change_to_async_values(c, record, value),
            Ok(false) => {}
            Err(thrown) => return destroy_with(c, record, thrown),
        }
        match push_value(c, record, value)? {
            Ok(Pushed::More) => continue,
            Ok(Pushed::Stop) => return Ok(()),
            Err(thrown) => return destroy_with(c, record, thrown),
        }
    }
}

/// A step's completion, its thrown value destroying the stream.
fn attempted(c: &mut Context<'_>, record: RootId, result: Result<(), RootedError>) -> Result<(), RootedError> {
    match result {
        Ok(()) => Ok(()),
        Err(RootedError { exception: Some(thrown), .. }) => destroy_with(c, record, thrown),
        Err(error) => Err(error),
    }
}

fn continuation(c: &mut Context<'_>, record: RootId, operation: &str) -> Outcome {
    closure(c, operation, "", &[record])
}

/// `changeToAsyncValues(value)`.
fn change_to_async_values(c: &mut Context<'_>, record: RootId, value: RootId) -> Result<(), RootedError> {
    put_bool(c, record, "isAsyncValues", true)?;
    let fulfilled = continuation(c, record, "streamFromAsyncValue")?;
    let rejected = continuation(c, record, "streamFromRejected")?;
    await_then(c, value, fulfilled, rejected)
}

pub(crate) fn rejected(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    destroy_with(c, record, error)?;
    Ok(c.undefined())
}

/// `nextSyncWithAsyncValues()`: sync steps, awaiting each thenable value.
fn next_sync_with_async_values(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let iterator = get(c, record, "iterator")?;
    loop {
        let step = super::attempt(next_result(c, iterator))?;
        let (value, done) = match step {
            Ok(step) => step,
            Err(thrown) => return destroy_with(c, record, thrown),
        };
        if done {
            let ended = end(c, record);
            return attempted(c, record, ended);
        }
        if is_thenable(c, value)? {
            let fulfilled = continuation(c, record, "streamFromAsyncValue")?;
            let rejected = continuation(c, record, "streamFromRejected")?;
            return await_then(c, value, fulfilled, rejected);
        }
        match push_value(c, record, value)? {
            Ok(Pushed::More) => continue,
            Ok(Pushed::Stop) => return Ok(()),
            Err(thrown) => return destroy_with(c, record, thrown),
        }
    }
}

pub(crate) fn async_value(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let value = arg(c, args, 0);
    match push_value(c, record, value)? {
        Ok(Pushed::More) => next_sync_with_async_values(c, record)?,
        Ok(Pushed::Stop) => {}
        Err(thrown) => destroy_with(c, record, thrown)?,
    }
    Ok(c.undefined())
}

/// `nextAsync()`: one awaited `iterator.next()` per value.
fn next_async(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let iterator = get(c, record, "iterator")?;
    let result = match super::attempt(call_method(c, iterator, "next", &[]))? {
        Ok(result) => result,
        Err(thrown) => return destroy_with(c, record, thrown),
    };
    let fulfilled = continuation(c, record, "streamFromAsyncResult")?;
    let rejected = continuation(c, record, "streamFromRejected")?;
    await_then(c, result, fulfilled, rejected)
}

pub(crate) fn async_result(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let result = arg(c, args, 0);
    let (value, done) = match super::attempt(unpack(c, result))? {
        Ok(pair) => pair,
        Err(thrown) => {
            destroy_with(c, record, thrown)?;
            return Ok(c.undefined());
        }
    };
    if done {
        let ended = end(c, record);
        attempted(c, record, ended)?;
    } else {
        match push_value(c, record, value)? {
            Ok(Pushed::More) => next_async(c, record)?,
            Ok(Pushed::Stop) => {}
            Err(thrown) => destroy_with(c, record, thrown)?,
        }
    }
    Ok(c.undefined())
}

/// `iterator.next()`, unpacked.
fn next_result(c: &mut Context<'_>, iterator: RootId) -> Result<(RootId, bool), RootedError> {
    let result = call_method(c, iterator, "next", &[])?;
    unpack(c, result)
}

/// `readable._destroy(error, cb)`: close the iterator, then call back on the next tick.
pub(crate) fn destroy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    let promise = c.pending_promise_rooted()?;
    let settle = c.null_object_rooted()?;
    put(c, settle, "record", record)?;
    put(c, settle, "promise", promise)?;
    put(c, settle, "error", error)?;
    close(c, settle)?;
    let fulfilled = closure(c, "streamFromClosed", "", &[callback, error])?;
    let rejected = closure(c, "streamFromCloseFailed", "", &[callback, error])?;
    await_then(c, promise, fulfilled, rejected)?;
    Ok(c.undefined())
}

pub(crate) fn closed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [callback, error] = captures(c)?;
    next_tick(c, callback, &[error])?;
    Ok(c.undefined())
}

pub(crate) fn close_failed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [callback, error] = captures(c)?;
    let failure = arg(c, args, 0);
    let delivered = if c.truthy_rooted(failure)? { failure } else { error };
    next_tick(c, callback, &[delivered])?;
    Ok(c.undefined())
}

/// `close(error)`: `iterator.throw(error)` when the stream failed, then `iterator.return()`,
/// awaiting each result's value; settles the record's promise.
fn close(c: &mut Context<'_>, settle: RootId) -> Result<(), RootedError> {
    let record = get(c, settle, "record")?;
    let iterator = get(c, record, "iterator")?;
    let error = get(c, settle, "error")?;
    let promise = get(c, settle, "promise")?;
    let throw = get(c, iterator, "throw")?;
    if !is_nullish(c, error) && c.is_callable_rooted(throw)? {
        match super::attempt(c.call_rooted(throw, iterator, &[error]))? {
            Ok(result) => {
                let fulfilled = closure(c, "streamFromThrown", "", &[settle])?;
                let rejected = closure(c, "streamFromCloseRejected", "", &[settle])?;
                await_then(c, result, fulfilled, rejected)?;
            }
            Err(thrown) => c.reject_promise_rooted(promise, thrown)?,
        }
        return Ok(());
    }
    close_return(c, settle)
}

fn close_return(c: &mut Context<'_>, settle: RootId) -> Result<(), RootedError> {
    let record = get(c, settle, "record")?;
    let iterator = get(c, record, "iterator")?;
    let promise = get(c, settle, "promise")?;
    let method = get(c, iterator, "return")?;
    if !c.is_callable_rooted(method)? {
        let undefined = c.undefined();
        return c.resolve_promise_rooted(promise, undefined);
    }
    match super::attempt(c.call_rooted(method, iterator, &[]))? {
        Ok(result) => {
            let fulfilled = closure(c, "streamFromReturned", "", &[settle])?;
            let rejected = closure(c, "streamFromCloseRejected", "", &[settle])?;
            await_then(c, result, fulfilled, rejected)
        }
        Err(thrown) => c.reject_promise_rooted(promise, thrown),
    }
}

pub(crate) fn close_rejected(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [settle] = captures(c)?;
    let error = arg(c, args, 0);
    let promise = get(c, settle, "promise")?;
    c.reject_promise_rooted(promise, error)?;
    Ok(c.undefined())
}

/// After `await iterator.throw(error)`: await its value, then return unless it finished.
pub(crate) fn thrown(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [settle] = captures(c)?;
    let result = arg(c, args, 0);
    let promise = get(c, settle, "promise")?;
    let (value, done) = match super::attempt(unpack(c, result))? {
        Ok((value, done)) => (value, c.boolean(done)),
        Err(failure) => {
            c.reject_promise_rooted(promise, failure)?;
            return Ok(c.undefined());
        }
    };
    let fulfilled = closure(c, "streamFromThrownValue", "", &[settle, done])?;
    let rejected = closure(c, "streamFromCloseRejected", "", &[settle])?;
    await_then(c, value, fulfilled, rejected)?;
    Ok(c.undefined())
}

pub(crate) fn thrown_value(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [settle, done] = captures(c)?;
    if c.truthy_rooted(done)? {
        let promise = get(c, settle, "promise")?;
        let undefined = c.undefined();
        c.resolve_promise_rooted(promise, undefined)?;
    } else {
        close_return(c, settle)?;
    }
    Ok(c.undefined())
}

/// After `await iterator.return()`: await its value, then settle.
pub(crate) fn returned(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [settle] = captures(c)?;
    let result = arg(c, args, 0);
    let promise = get(c, settle, "promise")?;
    let value = match super::attempt(get(c, result, "value"))? {
        Ok(value) => value,
        Err(failure) => {
            c.reject_promise_rooted(promise, failure)?;
            return Ok(c.undefined());
        }
    };
    let fulfilled = closure(c, "streamFromSettle", "", &[promise])?;
    let rejected = closure(c, "streamFromCloseRejected", "", &[settle])?;
    await_then(c, value, fulfilled, rejected)?;
    Ok(c.undefined())
}

pub(crate) fn settle(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [promise] = captures(c)?;
    let undefined = c.undefined();
    c.resolve_promise_rooted(promise, undefined)?;
    Ok(undefined)
}
