//! `internal/streams/pipeline`: `pipeline(...streams, callback)` and its promise form,
//! with Node's `pumpToNode` as a promise-driven state machine.

use super::errors::{self, StreamError};
use super::utils::{self, is_iterable, is_node_stream, is_readable_node_stream};
use super::{
    arg, await_then, call_method, captures, closure, get, intrinsics, is_nullish, next_tick,
    number, on, once, put, put_bool, put_number, remove_listener, Class, Context, Outcome, Slot,
};
use quench_runtime::{RootId, RootedError};

/// `pipeline(...streams, callback)`.
pub(crate) fn pipeline(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, args.len().saturating_sub(1));
    if !c.is_callable_rooted(callback)? {
        return Err(super::super::util_shared_vm::argument_type_error(c, callback, "streams[stream.length - 1]", "function")?);
    }
    let streams = c.array_rooted(&args[..args.len() - 1])?;
    let undefined = c.undefined();
    run(c, streams, callback, undefined)
}

/// `pipelineImpl(streams, callback, opts)`, as the pipeline module exports it.
pub(crate) fn pipeline_impl(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (streams, callback, options) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    run(c, streams, callback, options)
}

fn run(c: &mut Context<'_>, streams: RootId, callback: RootId, options: RootId) -> Outcome {
    let mut stages = c.array_like_values_rooted(streams)?;
    if stages.len() == 1 && c.is_array_rooted(stages[0])? {
        stages = c.array_like_values_rooted(stages[0])?;
    }
    if stages.len() < 2 {
        return Err(errors::throw(c, StreamError::MissingArgs("streams")));
    }
    let outer_signal = super::state::option(c, options, "signal")?;
    super::super::util_shared_vm::validate_abort_signal(c, outer_signal, "options.signal")?;
    let record = c.null_object_rooted()?;
    put(c, record, "callback", callback)?;
    put_bool(c, record, "called", false)?;
    let undefined = c.undefined();
    put(c, record, "error", undefined)?;
    put(c, record, "value", undefined)?;
    let destroys = c.array_rooted(&[])?;
    put(c, record, "destroys", destroys)?;
    let last_cleanup = c.array_rooted(&[])?;
    put(c, record, "lastStreamCleanup", last_cleanup)?;
    put_number(c, record, "finishCount", 0.0)?;
    put(c, record, "outerSignal", outer_signal)?;
    let finish = closure(c, "streamPipelineFinish", "finish", &[record])?;
    put(c, record, "finish", finish)?;
    let finish_only = closure(c, "streamPipelineFinishOnlyHandleError", "finishOnlyHandleError", &[record])?;
    put(c, record, "finishOnly", finish_only)?;
    if c.truthy_rooted(outer_signal)? {
        let abort = closure(c, "streamPipelineAbort", "abort", &[record])?;
        let disposable = utils::add_abort_listener(c, outer_signal, abort)?;
        put(c, record, "disposable", disposable)?;
    }
    let end_option = super::state::option(c, options, "end")?;
    let no = c.boolean(false);
    let ends_last = !c.strict_equal_rooted(end_option, no)?;
    let count = stages.len();
    let mut current = undefined;
    for (index, stream) in stages.into_iter().enumerate() {
        let reading = index < count - 1;
        let writing = index > 0;
        let end = reading || ends_last;
        let last = index == count - 1;
        if is_node_stream(c, stream)? {
            if end {
                let cleanup = destroyer(c, record, stream, reading, writing)?;
                if last && utils::is_readable(c, stream)? == Some(true) {
                    push_cleanup(c, record, cleanup)?;
                }
            }
            let on_error = closure(c, "streamPipelineOnError", "onError", &[record])?;
            on(c, stream, "error", on_error)?;
            if last && utils::is_readable(c, stream)? == Some(true) {
                let remove = closure(c, "streamPipelineRemoveErrorListener", "", &[stream, on_error])?;
                push_cleanup(c, record, remove)?;
            }
        }
        current = if index == 0 {
            source(c, record, stream)?
        } else if c.is_callable_rooted(stream)? {
            function_stage(c, record, current, stream, index, reading, end, last)?
        } else if is_node_stream(c, stream)? {
            node_stage(c, record, current, stream, end, last)?;
            stream
        } else if utils::is_web_stream(c, stream)? {
            web_stage(c, record, current, stream, end)?;
            stream
        } else {
            return Err(unsupported_body(c, stream)?);
        };
    }
    let signal = get(c, record, "signal")?;
    let aborted = |c: &mut Context<'_>, signal: RootId| -> Result<bool, RootedError> {
        if is_nullish(c, signal) {
            return Ok(false);
        }
        let aborted = get(c, signal, "aborted")?;
        c.truthy_rooted(aborted)
    };
    if aborted(c, signal)? || aborted(c, outer_signal)? {
        let abort = closure(c, "streamPipelineAbort", "abort", &[record])?;
        next_tick(c, abort, &[])?;
    }
    Ok(current)
}

/// `Duplex.from(body)` for a value no stage kind accepts.
fn unsupported_body(c: &mut Context<'_>, body: RootId) -> Result<RootedError, RootedError> {
    let expected = "of type function or an instance of Blob, ReadableStream, WritableStream, Stream, Iterable, AsyncIterable, or Promise or { readable, writable } pair";
    let error = errors::invalid_argument_type(c, body, "body", expected)?;
    Ok(c.throw(error))
}

fn push_cleanup(c: &mut Context<'_>, record: RootId, cleanup: RootId) -> Result<(), RootedError> {
    let list = get(c, record, "lastStreamCleanup")?;
    call_method(c, list, "push", &[cleanup]).map(drop)
}

fn add_finishes(c: &mut Context<'_>, record: RootId, count: f64) -> Result<(), RootedError> {
    let current = number(c, record, "finishCount")?;
    put_number(c, record, "finishCount", current + count)
}

/// The `{ signal }` a function stage receives, from the pipeline's own controller.
fn stage_options(c: &mut Context<'_>, record: RootId) -> Outcome {
    let mut signal = get(c, record, "signal")?;
    if is_nullish(c, signal) {
        let global = c.global_root()?;
        let class = get(c, global, "AbortController")?;
        let controller = c.construct_rooted(class, class, &[])?;
        put(c, record, "controller", controller)?;
        signal = get(c, controller, "signal")?;
        put(c, record, "signal", signal)?;
    }
    let options = c.object_rooted()?;
    put(c, options, "signal", signal)?;
    Ok(options)
}

/// The first stage: a source function's iterable, or the source itself.
fn source(c: &mut Context<'_>, record: RootId, stream: RootId) -> Outcome {
    if c.is_callable_rooted(stream)? {
        let options = stage_options(c, record)?;
        let undefined = c.undefined();
        let produced = c.call_rooted(stream, undefined, &[options])?;
        if !is_iterable(c, produced, None)? {
            return Err(errors::throw(
                c,
                StreamError::InvalidReturnValue("Iterable, AsyncIterable or Stream", "source".to_owned(), produced),
            ));
        }
        return Ok(produced);
    }
    if is_iterable(c, stream, None)? || is_readable_node_stream(c, stream, false)? || utils::is_transform_stream(c, stream)? {
        return Ok(stream);
    }
    Err(unsupported_body(c, stream)?)
}

/// `makeAsyncIterable(val)`.
fn async_iterable(c: &mut Context<'_>, value: RootId) -> Outcome {
    if is_iterable(c, value, None)? {
        return Ok(value);
    }
    if is_readable_node_stream(c, value, false)? {
        let readable = intrinsics(c)?.class(Class::Readable);
        let prototype = get(c, readable, "prototype")?;
        let key = intrinsics(c)?.slot(Slot::AsyncIterator);
        let method = c.get_property_rooted(prototype, key)?;
        return c.call_rooted(method, value, &[]);
    }
    let error = errors::invalid_argument_type(c, value, "val", "an instance of Readable, Iterable, or AsyncIterable")?;
    Err(c.throw(error))
}

#[allow(clippy::too_many_arguments)]
fn function_stage(
    c: &mut Context<'_>,
    record: RootId,
    previous: RootId,
    stage: RootId,
    index: usize,
    reading: bool,
    end: bool,
    last: bool,
) -> Outcome {
    let input = if utils::is_transform_stream(c, previous)? {
        let readable = get(c, previous, "readable")?;
        async_iterable(c, readable)?
    } else {
        async_iterable(c, previous)?
    };
    let options = stage_options(c, record)?;
    let undefined = c.undefined();
    let produced = c.call_rooted(stage, undefined, &[input, options])?;
    if reading {
        if !is_iterable(c, produced, Some(true))? {
            return Err(errors::throw(c, StreamError::InvalidReturnValue("AsyncIterable", format!("transform[{}]", index - 1), produced)));
        }
        return Ok(produced);
    }
    let pass_through = intrinsics(c)?.class(Class::PassThrough);
    let options = c.object_rooted()?;
    put_bool(c, options, "objectMode", true)?;
    let destination = c.construct_rooted(pass_through, pass_through, &[options])?;
    let then = if is_nullish(c, produced) { c.undefined() } else { get(c, produced, "then")? };
    if c.is_callable_rooted(then)? {
        add_finishes(c, record, 1.0)?;
        let end_flag = c.boolean(end);
        let fulfilled = closure(c, "streamPipelineValue", "", &[record, destination, end_flag])?;
        let rejected = closure(c, "streamPipelineValueFailed", "", &[record, destination])?;
        c.call_rooted(then, produced, &[fulfilled, rejected])?;
    } else if is_iterable(c, produced, Some(true))? {
        add_finishes(c, record, 1.0)?;
        let finish = get(c, record, "finish")?;
        pump(c, produced, destination, finish, end)?;
    } else {
        return Err(errors::throw(c, StreamError::InvalidReturnValue("AsyncIterable or Promise", "destination".to_owned(), produced)));
    }
    let cleanup = destroyer(c, record, destination, false, true)?;
    if last {
        push_cleanup(c, record, cleanup)?;
    }
    Ok(destination)
}

/// A web stream stage: the previous stage pumped into it (`pumpToWeb`).
fn web_stage(c: &mut Context<'_>, record: RootId, previous: RootId, stream: RootId, end: bool) -> Result<(), RootedError> {
    let input = if is_readable_node_stream(c, previous, false)? {
        async_iterable(c, previous)?
    } else if utils::is_readable_stream(c, previous)? || is_iterable(c, previous, None)? {
        previous
    } else if utils::is_transform_stream(c, previous)? {
        get(c, previous, "readable")?
    } else {
        let error = errors::invalid_argument_type(
            c,
            previous,
            "val",
            "an instance of Readable, Iterable, AsyncIterable, ReadableStream, or TransformStream",
        )?;
        return Err(c.throw(error));
    };
    add_finishes(c, record, 1.0)?;
    let finish = get(c, record, "finish")?;
    super::super::webstreams_shared_vm::adapters::pump_to_web(c, input, stream, finish, end)
}

fn node_stage(c: &mut Context<'_>, record: RootId, previous: RootId, stream: RootId, end: bool, last: bool) -> Result<(), RootedError> {
    let finish = get(c, record, "finish")?;
    let previous = if utils::is_transform_stream(c, previous)? && !is_node_stream(c, previous)? { get(c, previous, "readable")? } else { previous };
    if is_readable_node_stream(c, previous, false)? {
        add_finishes(c, record, 2.0)?;
        let cleanup = pipe(c, record, previous, stream, end)?;
        if last && utils::is_readable(c, stream)? == Some(true) {
            push_cleanup(c, record, cleanup)?;
        }
        return Ok(());
    }
    if is_iterable(c, previous, None)? {
        add_finishes(c, record, 1.0)?;
        return pump(c, previous, stream, finish, end);
    }
    let error = errors::invalid_argument_type(
        c,
        previous,
        "val",
        "an instance of Readable, Iterable, AsyncIterable, ReadableStream, or TransformStream",
    )?;
    Err(c.throw(error))
}

/// `destroyer(stream, reading, writing)`: registers the stage's `destroy` and returns its
/// end-of-stream cleanup.
fn destroyer(c: &mut Context<'_>, record: RootId, stream: RootId, reading: bool, writing: bool) -> Outcome {
    let stage = c.null_object_rooted()?;
    put(c, stage, "stream", stream)?;
    put_bool(c, stage, "finished", false)?;
    let closed = closure(c, "streamPipelineStageClosed", "", &[stage])?;
    on(c, stream, "close", closed)?;
    let settled = closure(c, "streamPipelineStageSettled", "", &[stage])?;
    let cleanup = super::eos::eos_with(c, stream, Some(reading), Some(writing), settled)?;
    let destroy = closure(c, "streamPipelineStageDestroy", "destroy", &[stage])?;
    let destroys = get(c, record, "destroys")?;
    call_method(c, destroys, "push", &[destroy])?;
    Ok(cleanup)
}

pub(crate) fn stage_closed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [stage] = captures(c)?;
    put_bool(c, stage, "finished", true)?;
    Ok(c.undefined())
}

pub(crate) fn stage_settled(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stage] = captures(c)?;
    let error = arg(c, args, 0);
    let clean = !c.truthy_rooted(error)?;
    put_bool(c, stage, "finished", clean)?;
    Ok(c.undefined())
}

pub(crate) fn stage_destroy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stage] = captures(c)?;
    let finished = get(c, stage, "finished")?;
    if c.truthy_rooted(finished)? {
        return Ok(c.undefined());
    }
    put_bool(c, stage, "finished", true)?;
    let stream = get(c, stage, "stream")?;
    let error = arg(c, args, 0);
    let error = if c.truthy_rooted(error)? { error } else { errors::create(c, StreamError::Destroyed("pipe"))? };
    super::destroy::destroyer(c, stream, error)?;
    Ok(c.undefined())
}

pub(crate) fn on_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    if c.truthy_rooted(error)? {
        let name = get(c, error, "name")?;
        let aborted = c.string_text(name)?.is_some_and(|name| name == "AbortError");
        if !aborted && !errors::has_code(c, error, "ERR_STREAM_PREMATURE_CLOSE")? {
            finish_impl(c, record, error, false)?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn remove_error_listener(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [stream, listener] = captures(c)?;
    remove_listener(c, stream, "error", listener)?;
    Ok(c.undefined())
}

pub(crate) fn finish(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    let count = number(c, record, "finishCount")? - 1.0;
    put_number(c, record, "finishCount", count)?;
    finish_impl(c, record, error, count == 0.0)?;
    Ok(c.undefined())
}

pub(crate) fn finish_only_handle_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    finish_impl(c, record, error, false)?;
    Ok(c.undefined())
}

pub(crate) fn abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let signal = get(c, record, "outerSignal")?;
    let reason = if is_nullish(c, signal) { c.undefined() } else { get(c, signal, "reason")? };
    let error = errors::abort_error(c, Some(reason))?;
    finish_impl(c, record, error, false)?;
    Ok(c.undefined())
}

/// `finishImpl(err, final)`: keep the first error (a premature close yields to a real
/// one), destroy every stage on failure, and call back once the last stage finished.
fn finish_impl(c: &mut Context<'_>, record: RootId, error: RootId, last: bool) -> Result<(), RootedError> {
    let held = get(c, record, "error")?;
    if c.truthy_rooted(error)? && (!c.truthy_rooted(held)? || errors::has_code(c, held, "ERR_STREAM_PREMATURE_CLOSE")?) {
        put(c, record, "error", error)?;
    }
    let held = get(c, record, "error")?;
    if !c.truthy_rooted(held)? && !last {
        return Ok(());
    }
    let destroys = get(c, record, "destroys")?;
    let undefined = c.undefined();
    while number(c, destroys, "length")? > 0.0 {
        let destroy = call_method(c, destroys, "shift", &[])?;
        let held = get(c, record, "error")?;
        c.call_rooted(destroy, undefined, &[held])?;
    }
    let disposable = get(c, record, "disposable")?;
    if !is_nullish(c, disposable) {
        let key = intrinsics(c)?.slot(Slot::Dispose);
        let dispose = c.get_property_rooted(disposable, key)?;
        c.call_rooted(dispose, disposable, &[])?;
    }
    let controller = get(c, record, "controller")?;
    if !is_nullish(c, controller) {
        call_method(c, controller, "abort", &[])?;
    }
    if last {
        let held = get(c, record, "error")?;
        if !c.truthy_rooted(held)? {
            let cleanups = get(c, record, "lastStreamCleanup")?;
            for cleanup in c.array_like_values_rooted(cleanups)? {
                c.call_rooted(cleanup, undefined, &[])?;
            }
        }
        let called = get(c, record, "called")?;
        if !c.truthy_rooted(called)? {
            put_bool(c, record, "called", true)?;
            let callback = get(c, record, "callback")?;
            let value = get(c, record, "value")?;
            next_tick(c, callback, &[held, value])?;
        }
    }
    Ok(())
}

/// A final function stage's promise fulfilled: write its value and end.
pub(crate) fn value(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record, destination, end] = captures(c)?;
    let value = arg(c, args, 0);
    put(c, record, "value", value)?;
    if !is_nullish(c, value) {
        call_method(c, destination, "write", &[value])?;
    }
    if c.truthy_rooted(end)? {
        call_method(c, destination, "end", &[])?;
    }
    let finish = get(c, record, "finish")?;
    next_tick(c, finish, &[])?;
    Ok(c.undefined())
}

pub(crate) fn value_failed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record, destination] = captures(c)?;
    let error = arg(c, args, 0);
    call_method(c, destination, "destroy", &[error])?;
    let finish = get(c, record, "finish")?;
    next_tick(c, finish, &[error])?;
    Ok(c.undefined())
}

/// `pipe(src, dst, finish, finishOnlyHandleError, { end })`: the destination's cleanup.
fn pipe(c: &mut Context<'_>, record: RootId, source: RootId, destination: RootId, end: bool) -> Outcome {
    let link = c.null_object_rooted()?;
    put(c, link, "record", record)?;
    put(c, link, "source", source)?;
    put(c, link, "destination", destination)?;
    put_bool(c, link, "ended", false)?;
    let closed = closure(c, "streamPipelineDestinationClosed", "", &[link])?;
    on(c, destination, "close", closed)?;
    let options = c.object_rooted()?;
    put_bool(c, options, "end", false)?;
    call_method(c, source, "pipe", &[destination, options])?;
    let finish = get(c, record, "finish")?;
    if end {
        let end_destination = closure(c, "streamPipelineEndDestination", "endFn", &[link])?;
        if utils::is_readable_finished(c, source, None)? == Some(true) {
            next_tick(c, end_destination, &[])?;
        } else {
            once(c, source, "end", end_destination)?;
        }
    } else {
        let undefined = c.undefined();
        c.call_rooted(finish, undefined, &[])?;
    }
    let source_done = closure(c, "streamPipelineSourceDone", "", &[link])?;
    super::eos::eos_with(c, source, Some(true), Some(false), source_done)?;
    super::eos::eos_with(c, destination, Some(false), Some(true), finish)
}

pub(crate) fn destination_closed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [link] = captures(c)?;
    let ended = get(c, link, "ended")?;
    if !c.truthy_rooted(ended)? {
        let record = get(c, link, "record")?;
        let error = errors::create(c, StreamError::PrematureClose)?;
        finish_impl(c, record, error, false)?;
    }
    Ok(c.undefined())
}

pub(crate) fn end_destination(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [link] = captures(c)?;
    put_bool(c, link, "ended", true)?;
    let destination = get(c, link, "destination")?;
    call_method(c, destination, "end", &[])?;
    Ok(c.undefined())
}

/// The source's end of stream: a premature close of a source that ended cleanly waits for
/// its `end`.
pub(crate) fn source_done(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [link] = captures(c)?;
    let error = arg(c, args, 0);
    let record = get(c, link, "record")?;
    let source = get(c, link, "source")?;
    let finish = get(c, record, "finish")?;
    let state = get(c, source, "_readableState")?;
    let wait_for_end = c.truthy_rooted(error)?
        && errors::has_code(c, error, "ERR_STREAM_PREMATURE_CLOSE")?
        && c.truthy_rooted(state)?
        && {
            let ended = get(c, state, "ended")?;
            let errored = get(c, state, "errored")?;
            let emitted = get(c, state, "errorEmitted")?;
            c.truthy_rooted(ended)? && !c.truthy_rooted(errored)? && !c.truthy_rooted(emitted)?
        };
    if wait_for_end {
        once(c, source, "end", finish)?;
        once(c, source, "error", finish)?;
    } else {
        let undefined = c.undefined();
        c.call_rooted(finish, undefined, &[error])?;
    }
    Ok(c.undefined())
}

/// `pumpToNode(iterable, writable, finish, { end })`.
fn pump(c: &mut Context<'_>, iterable: RootId, writable: RootId, finish: RootId, end: bool) -> Result<(), RootedError> {
    let pump = c.null_object_rooted()?;
    put(c, pump, "writable", writable)?;
    put(c, pump, "finish", finish)?;
    put_bool(c, pump, "end", end)?;
    let undefined = c.undefined();
    put(c, pump, "error", undefined)?;
    let resume = closure(c, "streamPumpResume", "resume", &[pump])?;
    put(c, pump, "resume", resume)?;
    on(c, writable, "drain", resume)?;
    let cleanup = super::eos::eos_with(c, writable, Some(false), None, resume)?;
    put(c, pump, "cleanup", cleanup)?;
    let intrinsics = intrinsics(c)?;
    let opened = super::attempt(open_iterator(c, pump, iterable, intrinsics.slot(Slot::AsyncIterator), intrinsics.slot(Slot::Iterator)))?;
    if let Err(thrown) = opened {
        return fail(c, pump, thrown);
    }
    let need_drain = get(c, writable, "writableNeedDrain")?;
    if c.truthy_rooted(need_drain)? {
        return wait(c, pump, "streamPumpNext");
    }
    next(c, pump)
}

/// `for await (const chunk of iterable)`'s iterator: async, or a sync one whose values are
/// awaited.
fn open_iterator(c: &mut Context<'_>, pump: RootId, iterable: RootId, async_key: RootId, sync_key: RootId) -> Result<(), RootedError> {
    let method = c.get_property_rooted(iterable, async_key)?;
    let (method, synchronous) = if is_nullish(c, method) { (c.get_property_rooted(iterable, sync_key)?, true) } else { (method, false) };
    let iterator = c.call_rooted(method, iterable, &[])?;
    put(c, pump, "iterator", iterator)?;
    put_bool(c, pump, "sync", synchronous)
}

/// `await wait()`, then the continuation `operation`; a failure goes to the pump's catch.
fn wait(c: &mut Context<'_>, pump: RootId, operation: &str) -> Result<(), RootedError> {
    let promise = c.pending_promise_rooted()?;
    let error = get(c, pump, "error")?;
    if c.truthy_rooted(error)? {
        c.reject_promise_rooted(promise, error)?;
    } else {
        put(c, pump, "waiting", promise)?;
    }
    let fulfilled = closure(c, operation, "", &[pump])?;
    let rejected = closure(c, "streamPumpBodyFailed", "", &[pump])?;
    await_then(c, promise, fulfilled, rejected)
}

/// `resume(err)`: record the error, release a waiting `wait()`.
pub(crate) fn pump_resume(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [pump] = captures(c)?;
    let error = arg(c, args, 0);
    if c.truthy_rooted(error)? {
        put(c, pump, "error", error)?;
    }
    let waiting = get(c, pump, "waiting")?;
    if !is_nullish(c, waiting) {
        let undefined = c.undefined();
        put(c, pump, "waiting", undefined)?;
        let error = get(c, pump, "error")?;
        if c.truthy_rooted(error)? {
            c.reject_promise_rooted(waiting, error)?;
        } else {
            c.resolve_promise_rooted(waiting, undefined)?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn pump_next(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [pump] = captures(c)?;
    next(c, pump)?;
    Ok(c.undefined())
}

/// The loop's `iterator.next()`.
fn next(c: &mut Context<'_>, pump: RootId) -> Result<(), RootedError> {
    let iterator = get(c, pump, "iterator")?;
    let result = match super::attempt(call_method(c, iterator, "next", &[]))? {
        Ok(result) => result,
        Err(thrown) => return fail(c, pump, thrown),
    };
    let synchronous = get(c, pump, "sync")?;
    if c.truthy_rooted(synchronous)? {
        let step = super::attempt((|| {
            let done = get(c, result, "done")?;
            let done = c.truthy_rooted(done)?;
            let value = get(c, result, "value")?;
            Ok((done, value))
        })())?;
        return match step {
            Ok((done, value)) => {
                let done = c.boolean(done);
                let fulfilled = closure(c, "streamPumpSyncValue", "", &[pump, done])?;
                let rejected = closure(c, "streamPumpFailed", "", &[pump])?;
                await_then(c, value, fulfilled, rejected)
            }
            Err(thrown) => fail(c, pump, thrown),
        };
    }
    let fulfilled = closure(c, "streamPumpResult", "", &[pump])?;
    let rejected = closure(c, "streamPumpFailed", "", &[pump])?;
    await_then(c, result, fulfilled, rejected)
}

pub(crate) fn pump_result(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [pump] = captures(c)?;
    let result = arg(c, args, 0);
    let step = super::attempt((|| {
        if !c.is_object_rooted(result)? {
            let error = c.type_error_rooted("Iterator result is not an object")?;
            return Err(c.throw(error));
        }
        let done = get(c, result, "done")?;
        let done = c.truthy_rooted(done)?;
        let value = get(c, result, "value")?;
        Ok((done, value))
    })())?;
    match step {
        Ok((true, _)) => after_loop(c, pump)?,
        Ok((false, value)) => body(c, pump, value)?,
        Err(thrown) => fail(c, pump, thrown)?,
    }
    Ok(c.undefined())
}

/// A sync iterator's awaited value: `CreateAsyncFromSyncIterator` settles its own promise
/// with the result, which the loop awaits in turn.
pub(crate) fn pump_sync_value(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [pump, done] = captures(c)?;
    let value = arg(c, args, 0);
    let done = c.truthy_rooted(done)?;
    let result = super::iterator_result(c, value, done)?;
    let fulfilled = closure(c, "streamPumpResult", "", &[pump])?;
    let rejected = closure(c, "streamPumpFailed", "", &[pump])?;
    await_then(c, result, fulfilled, rejected)?;
    Ok(c.undefined())
}

/// The loop body: write the chunk, waiting for drain when the writable asks.
fn body(c: &mut Context<'_>, pump: RootId, chunk: RootId) -> Result<(), RootedError> {
    let writable = get(c, pump, "writable")?;
    match super::attempt(call_method(c, writable, "write", &[chunk]))? {
        Ok(more) if c.truthy_rooted(more)? => next(c, pump),
        Ok(_) => wait(c, pump, "streamPumpNext"),
        Err(thrown) => body_failed_now(c, pump, thrown),
    }
}

/// A throw from the loop body: close the iterator (awaiting its `return()`), then catch.
pub(crate) fn pump_body_failed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [pump] = captures(c)?;
    let error = arg(c, args, 0);
    body_failed_now(c, pump, error)?;
    Ok(c.undefined())
}

fn body_failed_now(c: &mut Context<'_>, pump: RootId, error: RootId) -> Result<(), RootedError> {
    let iterator = get(c, pump, "iterator")?;
    let opened = !is_nullish(c, iterator) && {
        let finished = get(c, pump, "loopDone")?;
        !c.truthy_rooted(finished)?
    };
    if opened {
        let method = get(c, iterator, "return")?;
        if c.is_callable_rooted(method)? {
            if let Ok(result) = super::attempt(c.call_rooted(method, iterator, &[]))? {
                let after = closure(c, "streamPumpClosed", "", &[pump, error])?;
                return await_then(c, result, after, after);
            }
        }
    }
    fail(c, pump, error)
}

pub(crate) fn pump_closed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [pump, error] = captures(c)?;
    fail(c, pump, error)?;
    Ok(c.undefined())
}

pub(crate) fn pump_failed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [pump] = captures(c)?;
    let error = arg(c, args, 0);
    fail(c, pump, error)?;
    Ok(c.undefined())
}

/// After the loop: end the writable and wait for it to finish.
fn after_loop(c: &mut Context<'_>, pump: RootId) -> Result<(), RootedError> {
    put_bool(c, pump, "loopDone", true)?;
    let end = get(c, pump, "end")?;
    if !c.truthy_rooted(end)? {
        return succeed(c, pump);
    }
    let writable = get(c, pump, "writable")?;
    if let Err(thrown) = super::attempt(call_method(c, writable, "end", &[]))? {
        return fail(c, pump, thrown);
    }
    wait(c, pump, "streamPumpEnded")
}

pub(crate) fn pump_ended(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [pump] = captures(c)?;
    succeed(c, pump)?;
    Ok(c.undefined())
}

fn succeed(c: &mut Context<'_>, pump: RootId) -> Result<(), RootedError> {
    let finish = get(c, pump, "finish")?;
    let undefined = c.undefined();
    c.call_rooted(finish, undefined, &[])?;
    conclude(c, pump)
}

/// The pump's catch: `finish(error !== err ? aggregateTwoErrors(error, err) : err)`.
fn fail(c: &mut Context<'_>, pump: RootId, thrown: RootId) -> Result<(), RootedError> {
    let held = get(c, pump, "error")?;
    let error = if c.strict_equal_rooted(held, thrown)? { thrown } else { errors::aggregate_two(c, held, thrown)? };
    let finish = get(c, pump, "finish")?;
    let undefined = c.undefined();
    c.call_rooted(finish, undefined, &[error])?;
    conclude(c, pump)
}

/// The pump's finally.
fn conclude(c: &mut Context<'_>, pump: RootId) -> Result<(), RootedError> {
    let cleanup = get(c, pump, "cleanup")?;
    super::eos::run_cleanup(c, cleanup)?;
    let writable = get(c, pump, "writable")?;
    let resume = get(c, pump, "resume")?;
    let drain = c.string_rooted("drain");
    call_method(c, writable, "off", &[drain, resume]).map(drop)
}

/// `pipeline(...streams[, options])` from `stream/promises`.
pub(crate) fn promises_pipeline(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let promise = c.pending_promise_rooted()?;
    let mut streams = args.to_vec();
    let mut options = c.undefined();
    if let Some(&last) = streams.last() {
        let options_like = c.truthy_rooted(last)?
            && matches!(c.value_type_rooted(last), Some(quench_runtime::ValueType::Object))
            && !is_node_stream(c, last)?
            && !is_iterable(c, last, None)?
            && !utils::is_web_stream(c, last)?;
        if options_like {
            streams.pop();
            let signal = get(c, last, "signal")?;
            let end = get(c, last, "end")?;
            options = c.object_rooted()?;
            put(c, options, "signal", signal)?;
            put(c, options, "end", end)?;
        }
    }
    let streams = c.array_rooted(&streams)?;
    let callback = closure(c, "streamPromisesPipelineSettle", "", &[promise])?;
    if let Err(thrown) = super::attempt(run(c, streams, callback, options))? {
        c.reject_promise_rooted(promise, thrown)?;
    }
    Ok(promise)
}

pub(crate) fn promises_pipeline_settle(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [promise] = captures(c)?;
    let (error, value) = (arg(c, args, 0), arg(c, args, 1));
    if c.truthy_rooted(error)? {
        c.reject_promise_rooted(promise, error)?;
    } else {
        c.resolve_promise_rooted(promise, value)?;
    }
    Ok(c.undefined())
}
