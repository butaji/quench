//! `internal/streams/end-of-stream`: `finished(stream[, options], callback)` and its
//! promise form.

use super::errors::{self, StreamError};
use super::utils::{self, is_readable_node_stream, is_writable_node_stream, Tri};
use super::{
    arg, captures, closure, get, intrinsics, is_function, is_nullish, is_undefined, next_tick, on, put,
    put_bool, remove_listener, Context, Outcome,
};
use quench_runtime::{RootId, RootedError};

/// The listeners `eos` installs, each sharing one record.
const LISTENERS: &[(&str, &str)] = &[
    ("onlegacyfinish", "streamEosOnLegacyFinish"),
    ("onfinish", "streamEosOnFinish"),
    ("onend", "streamEosOnEnd"),
    ("onerror", "streamEosOnError"),
    ("onclose", "streamEosOnClose"),
    ("onclosed", "streamEosOnClosed"),
    ("onrequest", "streamEosOnRequest"),
];

fn tri_root(c: &mut Context<'_>, value: Tri) -> RootId {
    match value {
        Some(value) => c.boolean(value),
        None => c.null(),
    }
}

/// `eos(stream[, options], callback)`, the exported `finished`.
pub(crate) fn finished(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, mut options, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if args.len() == 2 {
        callback = options;
        options = c.undefined();
    }
    eos(c, stream, options, callback)
}

/// `eos(stream, options, callback)`: the cleanup function it returns.
pub(crate) fn eos(c: &mut Context<'_>, stream: RootId, options: RootId, callback: RootId) -> Outcome {
    let options = if is_nullish(c, options) {
        c.object_rooted()?
    } else {
        if !c.is_object_rooted(options)? || c.is_array_rooted(options)? || is_function(c, options)? {
            return Err(super::super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
        }
        options
    };
    if !is_function(c, callback)? {
        return Err(super::super::util_shared_vm::argument_type_error(c, callback, "callback", "function")?);
    }
    let signal = get(c, options, "signal")?;
    super::super::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
    let record = c.null_object_rooted()?;
    put(c, record, "stream", stream)?;
    put(c, record, "callback", callback)?;
    put_bool(c, record, "called", false)?;
    put_bool(c, record, "cleaned", false)?;
    if utils::is_readable_stream(c, stream)? || utils::is_writable_stream(c, stream)? {
        return eos_web(c, record, stream, signal);
    }
    if !utils::is_node_stream(c, stream)? {
        let error = errors::invalid_argument_type(c, stream, "stream", "an instance of ReadableStream, WritableStream, or Stream")?;
        return Err(c.throw(error));
    }
    let mut readable = get(c, options, "readable")?;
    if is_nullish(c, readable) {
        let value = is_readable_node_stream(c, stream, false)?;
        readable = c.boolean(value);
    }
    let mut writable = get(c, options, "writable")?;
    if is_nullish(c, writable) {
        let value = is_writable_node_stream(c, stream)?;
        writable = c.boolean(value);
    }
    put(c, record, "readable", readable)?;
    put(c, record, "writable", writable)?;
    let writable_state = get(c, stream, "_writableState")?;
    let readable_state = get(c, stream, "_readableState")?;
    let will_emit_close = utils::will_emit_close(c, stream)? == Some(true) && {
        let node_readable = is_readable_node_stream(c, stream, false)?;
        let node_readable = c.boolean(node_readable);
        let node_writable = is_writable_node_stream(c, stream)?;
        let node_writable = c.boolean(node_writable);
        c.strict_equal_rooted(node_readable, readable)? && c.strict_equal_rooted(node_writable, writable)?
    };
    put_bool(c, record, "willEmitClose", will_emit_close)?;
    let writable_finished = utils::is_writable_finished(c, stream, Some(false))?;
    let value = tri_root(c, writable_finished);
    put(c, record, "writableFinished", value)?;
    let readable_finished = utils::is_readable_finished(c, stream, Some(false))?;
    let value = tri_root(c, readable_finished);
    put(c, record, "readableFinished", value)?;
    let closed = utils::is_closed(c, stream)?;
    put(c, record, "closed", closed)?;
    for &(name, operation) in LISTENERS {
        let listener = closure(c, operation, name, &[record])?;
        put(c, record, name, listener)?;
    }
    let listener = |c: &mut Context<'_>, name: &str| get(c, record, name);
    if utils::is_request(c, stream)? {
        let onfinish = listener(c, "onfinish")?;
        on(c, stream, "complete", onfinish)?;
        if !will_emit_close {
            let onclose = listener(c, "onclose")?;
            on(c, stream, "abort", onclose)?;
        }
        let request = get(c, stream, "req")?;
        let onrequest = listener(c, "onrequest")?;
        if c.truthy_rooted(request)? {
            let undefined = c.undefined();
            c.call_rooted(onrequest, undefined, &[])?;
        } else {
            on(c, stream, "request", onrequest)?;
        }
    } else if c.truthy_rooted(writable)? && !c.truthy_rooted(writable_state)? {
        let legacy = listener(c, "onlegacyfinish")?;
        on(c, stream, "end", legacy)?;
        on(c, stream, "close", legacy)?;
    }
    let aborted = get(c, stream, "aborted")?;
    if !will_emit_close && c.rooted_value(aborted).is_some_and(|value| value.as_bool().is_some()) {
        let onclose = listener(c, "onclose")?;
        on(c, stream, "aborted", onclose)?;
    }
    let onend = listener(c, "onend")?;
    on(c, stream, "end", onend)?;
    let onfinish = listener(c, "onfinish")?;
    on(c, stream, "finish", onfinish)?;
    let error_option = get(c, options, "error")?;
    let no = c.boolean(false);
    if !c.strict_equal_rooted(error_option, no)? {
        let onerror = listener(c, "onerror")?;
        on(c, stream, "error", onerror)?;
    }
    let onclose = listener(c, "onclose")?;
    on(c, stream, "close", onclose)?;
    let error_emitted = |c: &mut Context<'_>, state: RootId| -> Result<bool, RootedError> {
        if !c.truthy_rooted(state)? {
            return Ok(false);
        }
        let emitted = get(c, state, "errorEmitted")?;
        c.truthy_rooted(emitted)
    };
    let readable_flag = c.truthy_rooted(readable)?;
    let writable_flag = c.truthy_rooted(writable)?;
    let settled = if c.truthy_rooted(closed)? {
        Some("onclose")
    } else if error_emitted(c, writable_state)? || error_emitted(c, readable_state)? {
        (!will_emit_close).then_some("onclosed")
    } else {
        let writable_only = !readable_flag
            && (!will_emit_close || utils::is_readable(c, stream)?.is_some_and(|value| value))
            && (writable_finished == Some(true) || utils::is_writable(c, stream)? == Some(false))
            && no_pending_callbacks(c, writable_state)?;
        let mut readable_only = || -> Result<bool, RootedError> {
            Ok(!writable_flag
                && (!will_emit_close || utils::is_writable(c, stream)?.is_some_and(|value| value))
                && (readable_finished == Some(true) || utils::is_readable(c, stream)? == Some(false)))
        };
        let finished = writable_only || readable_only()? || {
            let request = get(c, stream, "req")?;
            let aborted = get(c, stream, "aborted")?;
            c.truthy_rooted(readable_state)? && c.truthy_rooted(request)? && c.truthy_rooted(aborted)?
        };
        finished.then_some("onclosed")
    };
    if let Some(name) = settled {
        let settle = listener(c, name)?;
        next_tick(c, settle, &[])?;
    }
    let cleanup = closure(c, "streamEosCleanup", "cleanup", &[record])?;
    if c.truthy_rooted(signal)? && !c.truthy_rooted(closed)? {
        let abort = closure(c, "streamEosAbort", "abort", &[record, signal])?;
        let aborted = get(c, signal, "aborted")?;
        if c.truthy_rooted(aborted)? {
            next_tick(c, abort, &[])?;
        } else {
            let disposable = utils::add_abort_listener(c, signal, abort)?;
            put(c, record, "disposable", disposable)?;
        }
    }
    Ok(cleanup)
}

/// `wState == null || wState.pendingcb === undefined || wState.pendingcb === 0`.
fn no_pending_callbacks(c: &mut Context<'_>, state: RootId) -> Result<bool, RootedError> {
    if is_nullish(c, state) {
        return Ok(true);
    }
    let pending = get(c, state, "pendingcb")?;
    Ok(is_undefined(c, pending) || c.rooted_value(pending).and_then(|value| value.as_number()) == Some(0.0))
}

/// `eosWeb(stream, options, callback)`: settle when the web stream's closed promise does.
fn eos_web(c: &mut Context<'_>, record: RootId, stream: RootId, signal: RootId) -> Outcome {
    if c.truthy_rooted(signal)? {
        let abort = closure(c, "streamEosWebAbort", "abort", &[record, signal])?;
        let aborted = get(c, signal, "aborted")?;
        if c.truthy_rooted(aborted)? {
            next_tick(c, abort, &[])?;
        } else {
            let disposable = utils::add_abort_listener(c, signal, abort)?;
            put(c, record, "disposable", disposable)?;
        }
    }
    let key = utils::registered_symbol(c, "nodejs.webstream.isClosedPromise")?;
    let closed = c.get_property_rooted(stream, key)?;
    let promise = get(c, closed, "promise")?;
    let resolver = closure(c, "streamEosWebSettled", "", &[record])?;
    let then = get(c, promise, "then")?;
    c.call_rooted(then, promise, &[resolver, resolver])?;
    let nop = intrinsics(c)?.nop;
    Ok(nop)
}

pub(crate) fn web_settled(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    if !super::truthy_member(c, record, "aborted")? {
        let values = c.array_rooted(args)?;
        let deliver = closure(c, "streamEosWebDeliver", "", &[record, values])?;
        next_tick(c, deliver, &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn web_deliver(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record, values] = captures(c)?;
    let values = c.array_like_values_rooted(values)?;
    invoke(c, record, &values, false)?;
    Ok(c.undefined())
}

pub(crate) fn web_abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record, signal] = captures(c)?;
    put_bool(c, record, "aborted", true)?;
    let error = errors::abort_error_for(c, signal)?;
    invoke(c, record, &[error], false)?;
    Ok(c.undefined())
}

/// The user's callback, called once with `this` the stream: no longer after `cleanup`
/// unless `even_cleaned` (the abort path calls the callback it held before cleaning up).
fn invoke(c: &mut Context<'_>, record: RootId, args: &[RootId], even_cleaned: bool) -> Result<(), RootedError> {
    if super::truthy_member(c, record, "called")? || (!even_cleaned && super::truthy_member(c, record, "cleaned")?) {
        return Ok(());
    }
    put_bool(c, record, "called", true)?;
    let disposable = get(c, record, "disposable")?;
    if !is_nullish(c, disposable) {
        let key = intrinsics(c)?.slot(super::Slot::Dispose);
        let dispose = c.get_property_rooted(disposable, key)?;
        c.call_rooted(dispose, disposable, &[])?;
    }
    let stream = get(c, record, "stream")?;
    let callback = get(c, record, "callback")?;
    c.call_rooted(callback, stream, args)?;
    Ok(())
}

/// `readableFinished`/`writableFinished` set, `willEmitClose` dropped for a destroyed
/// stream; whether the callback still waits for the close event.
fn finished_half(c: &mut Context<'_>, record: RootId, own: &str, other: &str, other_finished: &str) -> Result<(), RootedError> {
    put_bool(c, record, own, true)?;
    let stream = get(c, record, "stream")?;
    let destroyed = get(c, stream, "destroyed")?;
    if c.truthy_rooted(destroyed)? {
        put_bool(c, record, "willEmitClose", false)?;
    }
    let live = get(c, stream, other)?;
    if super::truthy_member(c, record, "willEmitClose")? && (!c.truthy_rooted(live)? || super::truthy_member(c, record, other)?) {
        return Ok(());
    }
    if !super::truthy_member(c, record, other)? || super::truthy_member(c, record, other_finished)? {
        invoke(c, record, &[], false)?;
    }
    Ok(())
}

pub(crate) fn on_legacy_finish(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let stream = get(c, record, "stream")?;
    let writable = get(c, stream, "writable")?;
    if !c.truthy_rooted(writable)? {
        finished_half(c, record, "writableFinished", "readable", "readableFinished")?;
    }
    Ok(c.undefined())
}

pub(crate) fn on_finish(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    finished_half(c, record, "writableFinished", "readable", "readableFinished")?;
    Ok(c.undefined())
}

pub(crate) fn on_end(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    finished_half(c, record, "readableFinished", "writable", "writableFinished")?;
    Ok(c.undefined())
}

pub(crate) fn on_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    invoke(c, record, &[error], false)?;
    Ok(c.undefined())
}

/// The stream's error, when either half holds a non-boolean one.
fn held_error(c: &mut Context<'_>, stream: RootId) -> Result<Option<RootId>, RootedError> {
    let mut errored = utils::writable_errored(c, stream)?;
    if !c.truthy_rooted(errored)? {
        errored = utils::readable_errored(c, stream)?;
    }
    let boolean = c.rooted_value(errored).is_some_and(|value| value.as_bool().is_some());
    Ok((c.truthy_rooted(errored)? && !boolean).then_some(errored))
}

pub(crate) fn on_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    put_bool(c, record, "closed", true)?;
    let stream = get(c, record, "stream")?;
    if let Some(error) = held_error(c, stream)? {
        invoke(c, record, &[error], false)?;
        return Ok(c.undefined());
    }
    if super::truthy_member(c, record, "readable")?
        && !super::truthy_member(c, record, "readableFinished")?
        && is_readable_node_stream(c, stream, true)?
        && utils::is_readable_finished(c, stream, Some(false))? != Some(true)
    {
        let error = errors::create(c, StreamError::PrematureClose)?;
        invoke(c, record, &[error], false)?;
        return Ok(c.undefined());
    }
    if super::truthy_member(c, record, "writable")?
        && !super::truthy_member(c, record, "writableFinished")?
        && utils::is_writable_finished(c, stream, Some(false))? != Some(true)
    {
        let error = errors::create(c, StreamError::PrematureClose)?;
        invoke(c, record, &[error], false)?;
        return Ok(c.undefined());
    }
    invoke(c, record, &[], false)?;
    Ok(c.undefined())
}

pub(crate) fn on_closed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    put_bool(c, record, "closed", true)?;
    let stream = get(c, record, "stream")?;
    match held_error(c, stream)? {
        Some(error) => invoke(c, record, &[error], false)?,
        None => invoke(c, record, &[], false)?,
    }
    Ok(c.undefined())
}

pub(crate) fn on_request(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let stream = get(c, record, "stream")?;
    let request = get(c, stream, "req")?;
    let onfinish = get(c, record, "onfinish")?;
    on(c, request, "finish", onfinish)?;
    Ok(c.undefined())
}

pub(crate) fn cleanup(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    cleanup_now(c, record)?;
    Ok(c.undefined())
}

fn cleanup_now(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    put_bool(c, record, "cleaned", true)?;
    let stream = get(c, record, "stream")?;
    for (event, name) in [("aborted", "onclose"), ("complete", "onfinish"), ("abort", "onclose"), ("request", "onrequest")] {
        let listener = get(c, record, name)?;
        remove_listener(c, stream, event, listener)?;
    }
    let request = get(c, stream, "req")?;
    if c.truthy_rooted(request)? {
        let onfinish = get(c, record, "onfinish")?;
        remove_listener(c, request, "finish", onfinish)?;
    }
    for (event, name) in [
        ("end", "onlegacyfinish"),
        ("close", "onlegacyfinish"),
        ("finish", "onfinish"),
        ("end", "onend"),
        ("error", "onerror"),
        ("close", "onclose"),
    ] {
        let listener = get(c, record, name)?;
        remove_listener(c, stream, event, listener)?;
    }
    Ok(())
}

pub(crate) fn abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record, signal] = captures(c)?;
    cleanup_now(c, record)?;
    let error = errors::abort_error_for(c, signal)?;
    invoke(c, record, &[error], true)?;
    Ok(c.undefined())
}

/// `finished(stream, opts)` from `stream/promises`.
pub(crate) fn promises_finished(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, mut options) = (arg(c, args, 0), arg(c, args, 1));
    if super::is_null(c, options) {
        options = c.object_rooted()?;
    }
    let mut auto_cleanup = false;
    if !is_nullish(c, options) {
        let cleanup = get(c, options, "cleanup")?;
        if c.truthy_rooted(cleanup)? {
            if c.rooted_value(cleanup).and_then(|value| value.as_bool()).is_none() {
                return Err(super::super::util_shared_vm::argument_type_error(c, cleanup, "cleanup", "boolean")?);
            }
            auto_cleanup = true;
        }
    }
    let promise = c.pending_promise_rooted()?;
    let holder = c.null_object_rooted()?;
    let auto = c.boolean(auto_cleanup);
    let callback = closure(c, "streamPromisesFinishedSettle", "", &[promise, holder, auto])?;
    match super::attempt(eos(c, stream, options, callback))? {
        Ok(cleanup) => put(c, holder, "cleanup", cleanup)?,
        Err(thrown) => c.reject_promise_rooted(promise, thrown)?,
    }
    Ok(promise)
}

pub(crate) fn promises_finished_settle(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [promise, holder, auto] = captures(c)?;
    let error = arg(c, args, 0);
    if c.truthy_rooted(auto)? {
        let cleanup = get(c, holder, "cleanup")?;
        let undefined = c.undefined();
        c.call_rooted(cleanup, undefined, &[])?;
    }
    if c.truthy_rooted(error)? {
        c.reject_promise_rooted(promise, error)?;
    } else {
        let undefined = c.undefined();
        c.resolve_promise_rooted(promise, undefined)?;
    }
    Ok(c.undefined())
}

/// Which completion errors an async disposal tolerates.
#[derive(Clone, Copy)]
pub(crate) enum Tolerated {
    /// Only the error the disposal destroyed the stream with.
    Same,
    /// Any `AbortError`.
    Aborts,
}

/// The promise an async disposal returns: settled when the stream finishes.
pub(crate) fn settle_when_finished(c: &mut Context<'_>, stream: RootId, error: RootId, tolerated: Tolerated) -> Outcome {
    let promise = c.pending_promise_rooted()?;
    let aborts = c.boolean(matches!(tolerated, Tolerated::Aborts));
    let callback = closure(c, "streamDisposeSettle", "", &[promise, error, aborts])?;
    let undefined = c.undefined();
    match super::attempt(eos(c, stream, undefined, callback))? {
        Ok(_) => {}
        Err(thrown) => c.reject_promise_rooted(promise, thrown)?,
    }
    Ok(promise)
}

pub(crate) fn dispose_settle(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [promise, expected, aborts] = captures(c)?;
    let error = arg(c, args, 0);
    let tolerated = if !c.truthy_rooted(error)? {
        true
    } else if c.truthy_rooted(aborts)? {
        let name = get(c, error, "name")?;
        c.string_text(name)?.is_some_and(|name| name == "AbortError")
    } else {
        c.strict_equal_rooted(error, expected)?
    };
    if tolerated {
        let undefined = c.undefined();
        c.resolve_promise_rooted(promise, undefined)?;
    } else {
        c.reject_promise_rooted(promise, error)?;
    }
    Ok(c.undefined())
}

/// `eos(stream, options, callback)` from Rust with an options record.
pub(crate) fn eos_with(c: &mut Context<'_>, stream: RootId, readable: Option<bool>, writable: Option<bool>, callback: RootId) -> Outcome {
    let options = c.object_rooted()?;
    for (name, value) in [("readable", readable), ("writable", writable)] {
        let value = match value {
            Some(value) => c.boolean(value),
            None => c.undefined(),
        };
        put(c, options, name, value)?;
    }
    eos(c, stream, options, callback)
}

/// Calls `cleanup` (an `eos` cleanup function).
pub(crate) fn run_cleanup(c: &mut Context<'_>, cleanup: RootId) -> Result<(), RootedError> {
    let undefined = c.undefined();
    c.call_rooted(cleanup, undefined, &[]).map(drop)
}
