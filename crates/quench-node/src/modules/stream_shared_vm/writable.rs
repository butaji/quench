//! `Writable`: buffered writes, corking, the end/finish protocol and the writable accessors.

use super::destroy::{self, error_or_destroy};
use super::errors::{self, StreamError};
use super::readable::{adopt_options, encode, is_buffer, is_view, length_of, needs_new, shape_events, view_to_buffer};
use super::state::{self, bits, change, reset_buffer, writable::*};
use super::{
    arg, call_method, captures, closure, define_methods, emit, get, get_slot, intrinsics, is_function, is_null,
    is_nullish, is_string, next_tick, number, put, put_number, put_slot, tick, Class, Context, Key, Outcome, Slot,
    Tick,
};
use quench_runtime::{RootId, RootedError};

/// Consumed buffered writes a state keeps before compacting its front.
const BUFFER_COMPACTION_INDEX: f64 = 256.0;

pub(crate) const WRITABLE_EVENTS: &[&str] = &["close", "error", "prefinish", "finish", "drain"];

pub(crate) fn state_of(c: &mut Context<'_>, stream: RootId) -> Outcome {
    get(c, stream, "_writableState")
}

/// The options a writable adopts as its implementation.
pub(crate) const WRITABLE_OPTIONS: &[(&str, &str)] = &[
    ("write", "_write"),
    ("writev", "_writev"),
    ("destroy", "_destroy"),
    ("final", "_final"),
    ("construct", "_construct"),
];

/// `new Writable(options)`.
pub(crate) fn constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    if let Some(class) = needs_new(c, this, Class::Writable)? {
        return c.construct_rooted(class, class, &[options]);
    }
    shape_events(c, this, WRITABLE_EVENTS)?;
    let state = state::create(c, Class::WritableState, options, this, false)?;
    put(c, this, "_writableState", state)?;
    if c.truthy_rooted(options)? {
        adopt_options(c, this, options, WRITABLE_OPTIONS)?;
        let signal = get(c, options, "signal")?;
        if c.truthy_rooted(signal)? {
            super::utils::add_abort_signal(c, signal, this)?;
        }
    }
    let stream = intrinsics(c)?.class(Class::Stream);
    c.call_rooted(stream, this, &[options])?;
    destroy::construct_if_defined(c, this)?;
    Ok(c.undefined())
}

/// `Writable[Symbol.hasInstance](object)`: a Duplex's writable half counts as a Writable.
pub(crate) fn has_instance(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let object = arg(c, args, 0);
    let global = c.global_root()?;
    let function = get(c, global, "Function")?;
    let prototype = get(c, function, "prototype")?;
    let intrinsics = intrinsics(c)?;
    let ordinary = c.get_property_rooted(prototype, intrinsics.slot(Slot::HasInstance))?;
    let result = c.call_rooted(ordinary, this, &[object])?;
    if c.truthy_rooted(result)? {
        return Ok(c.boolean(true));
    }
    let writable = intrinsics.class(Class::Writable);
    if !c.strict_equal_rooted(this, writable)? {
        return Ok(c.boolean(false));
    }
    if !c.truthy_rooted(object)? {
        return Ok(object);
    }
    let state = get(c, object, "_writableState")?;
    if !c.is_object_rooted(state)? {
        return Ok(c.boolean(false));
    }
    let class = intrinsics.class(Class::WritableState);
    let instance = c.instance_of_rooted(state, class)?;
    Ok(c.boolean(instance))
}

/// `WritableState.prototype[kOnConstructed](stream)`.
pub(crate) fn on_constructed(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    if !state::has(c, this, WRITING)? {
        clear_buffer(c, stream, this)?;
    }
    if state::has(c, this, ENDING)? {
        finish_maybe(c, stream, this, false)?;
    }
    Ok(c.undefined())
}

pub(crate) fn pipe(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let error = errors::create(c, StreamError::CannotPipe)?;
    error_or_destroy(c, this, error, false)?;
    Ok(c.undefined())
}

/// What `_write` returns: whether more may be written, or the error the write failed with.
enum Written {
    More(bool),
    Failed(RootId),
}

/// `_write(stream, chunk, encoding, cb)`.
fn write_chunk(c: &mut Context<'_>, stream: RootId, chunk: RootId, encoding: RootId, callback: RootId) -> Result<Written, RootedError> {
    let state = state_of(c, stream)?;
    let callback = if is_function(c, callback)? { callback } else { intrinsics(c)?.nop };
    if is_null(c, chunk) {
        return Err(errors::throw(c, StreamError::NullValues));
    }
    let mut chunk = chunk;
    let mut encoding = encoding;
    let field = bits(c, state)?;
    if field & state::OBJECT_MODE == 0 {
        if !c.truthy_rooted(encoding)? {
            encoding = if field & DEFAULT_UTF8_ENCODING != 0 { c.string_rooted("utf8") } else { get(c, state, "defaultEncoding")? };
        } else {
            let name = c.string_text(encoding)?;
            if !name.as_deref().is_some_and(|name| name == "buffer" || state::is_encoding(name)) {
                let shown = c.to_string(encoding)?;
                return Err(errors::throw(c, StreamError::UnknownEncoding(shown)));
            }
        }
        if is_string(c, chunk) {
            if field & DECODE_STRINGS != 0 {
                chunk = encode(c, chunk, encoding)?;
                encoding = c.string_rooted("buffer");
            }
        } else if is_buffer(c, chunk)? {
            encoding = c.string_rooted("buffer");
        } else if is_view(c, chunk)? {
            chunk = view_to_buffer(c, chunk)?;
            encoding = c.string_rooted("buffer");
        } else {
            let error = errors::invalid_argument_type(c, chunk, "chunk", errors::CHUNK_TYPES)?;
            return Err(c.throw(error));
        }
    }
    let field = bits(c, state)?;
    let failure = if field & ENDING != 0 {
        Some(StreamError::WriteAfterEnd)
    } else if field & state::DESTROYED != 0 {
        Some(StreamError::Destroyed("write"))
    } else {
        None
    };
    if let Some(failure) = failure {
        let error = errors::create(c, failure)?;
        next_tick(c, callback, &[error])?;
        error_or_destroy(c, stream, error, true)?;
        return Ok(Written::Failed(error));
    }
    let pending = number(c, state, "pendingcb")?;
    put_number(c, state, "pendingcb", pending + 1.0)?;
    write_or_buffer(c, stream, state, chunk, encoding, callback).map(Written::More)
}

pub(crate) fn write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, mut encoding, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if !is_nullish(c, encoding) && is_function(c, encoding)? {
        callback = encoding;
        encoding = c.null();
    }
    let more = matches!(write_chunk(c, this, chunk, encoding, callback)?, Written::More(true));
    Ok(c.boolean(more))
}

pub(crate) fn cork(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state_of(c, this)?;
    change(c, state, CORKED, 0)?;
    let corked = number(c, state, "corked")?;
    put_number(c, state, "corked", corked + 1.0)?;
    Ok(c.undefined())
}

pub(crate) fn uncork(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state_of(c, this)?;
    let corked = number(c, state, "corked")?;
    if corked != 0.0 && !corked.is_nan() {
        put_number(c, state, "corked", corked - 1.0)?;
        if corked - 1.0 == 0.0 {
            change(c, state, 0, CORKED)?;
        }
        if !state::has(c, state, WRITING)? {
            clear_buffer(c, this, state)?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn set_default_encoding(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let mut encoding = arg(c, args, 0);
    if is_string(c, encoding) {
        encoding = call_method(c, encoding, "toLowerCase", &[])?;
    }
    let name = c.string_text(encoding)?;
    if !name.as_deref().is_some_and(state::is_encoding) {
        let shown = c.to_string(encoding)?;
        return Err(errors::throw(c, StreamError::UnknownEncoding(shown)));
    }
    let state = state_of(c, this)?;
    put(c, state, "defaultEncoding", encoding)?;
    Ok(this)
}

/// `state.writecb = cb` unless it is `nop`.
fn hold_callback(c: &mut Context<'_>, state: RootId, callback: RootId) -> Result<(), RootedError> {
    let nop = intrinsics(c)?.nop;
    if !c.strict_equal_rooted(callback, nop)? {
        put_slot(c, state, Slot::WriteCbValue, callback)?;
        change(c, state, WRITE_CB, 0)?;
    }
    Ok(())
}

/// `writeOrBuffer(stream, state, chunk, encoding, callback)`.
fn write_or_buffer(c: &mut Context<'_>, stream: RootId, state: RootId, chunk: RootId, encoding: RootId, callback: RootId) -> Result<bool, RootedError> {
    let size = state::chunk_size(c, state, chunk)?;
    let length = number(c, state, "length")? + size;
    put_number(c, state, "length", length)?;
    let field = bits(c, state)?;
    if field & (WRITING | state::ERRORED | CORKED | state::CONSTRUCTED) != state::CONSTRUCTED {
        if field & BUFFERED == 0 {
            change(c, state, BUFFERED, 0)?;
            let list = c.array_rooted(&[])?;
            put_slot(c, state, Slot::BufferedValue, list)?;
        }
        let request = c.object_rooted()?;
        put(c, request, "chunk", chunk)?;
        put(c, request, "encoding", encoding)?;
        put(c, request, "callback", callback)?;
        let list = get_slot(c, state, Slot::BufferedValue)?;
        call_method(c, list, "push", &[request])?;
        let all_buffers = c.string_text(encoding)?.is_some_and(|name| name == "buffer");
        if bits(c, state)? & ALL_BUFFERS != 0 && !all_buffers {
            change(c, state, 0, ALL_BUFFERS)?;
        }
        let nop = intrinsics(c)?.nop;
        if bits(c, state)? & ALL_NOOP != 0 && !c.strict_equal_rooted(callback, nop)? {
            change(c, state, 0, ALL_NOOP)?;
        }
    } else {
        put_number(c, state, "writelen", size)?;
        hold_callback(c, state, callback)?;
        change(c, state, WRITING | SYNC | EXPECT_WRITE_CB, 0)?;
        let onwrite = get(c, state, "onwrite")?;
        call_method(c, stream, "_write", &[chunk, encoding, onwrite])?;
        change(c, state, 0, SYNC)?;
    }
    let length = number(c, state, "length")?;
    let more = length < number(c, state, "highWaterMark")? || length == 0.0;
    if !more {
        change(c, state, NEED_DRAIN, 0)?;
    }
    Ok(more && bits(c, state)? & (state::DESTROYED | state::ERRORED) == 0)
}

/// `doWrite(stream, state, writev, len, chunk, encoding, cb)`.
fn do_write(c: &mut Context<'_>, stream: RootId, state: RootId, vectored: bool, size: f64, chunk: RootId, encoding: RootId, callback: RootId) -> Result<(), RootedError> {
    put_number(c, state, "writelen", size)?;
    hold_callback(c, state, callback)?;
    let field = change(c, state, WRITING | SYNC | EXPECT_WRITE_CB, 0)?;
    let onwrite = get(c, state, "onwrite")?;
    if field & state::DESTROYED != 0 {
        let error = errors::create(c, StreamError::Destroyed("write"))?;
        let undefined = c.undefined();
        c.call_rooted(onwrite, undefined, &[error])?;
    } else if vectored {
        call_method(c, stream, "_writev", &[chunk, onwrite])?;
    } else {
        call_method(c, stream, "_write", &[chunk, encoding, onwrite])?;
    }
    change(c, state, 0, SYNC)?;
    Ok(())
}

pub(crate) fn on_write_error_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, state, error, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    on_write_error(c, stream, state, error, callback)?;
    Ok(c.undefined())
}

/// `onwriteError(stream, state, er, cb)`.
fn on_write_error(c: &mut Context<'_>, stream: RootId, state: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    let pending = number(c, state, "pendingcb")?;
    put_number(c, state, "pendingcb", pending - 1.0)?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])?;
    error_buffer(c, state)?;
    error_or_destroy(c, stream, error, false)
}

/// `onwrite(stream, er)`: the callback `_write` receives as `state.onwrite`.
pub(crate) fn on_write(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream] = captures(c)?;
    let error = arg(c, args, 0);
    let state = state_of(c, stream)?;
    let field = bits(c, state)?;
    if field & EXPECT_WRITE_CB == 0 {
        let error = errors::create(c, StreamError::MultipleCallback)?;
        error_or_destroy(c, stream, error, false)?;
        return Ok(c.undefined());
    }
    let sync = field & SYNC != 0;
    let intrinsics = intrinsics(c)?;
    let callback = if field & WRITE_CB != 0 { get_slot(c, state, Slot::WriteCbValue)? } else { intrinsics.nop };
    let null = c.null();
    put_slot(c, state, Slot::WriteCbValue, null)?;
    change(c, state, 0, WRITE_CB | WRITING | EXPECT_WRITE_CB)?;
    let length = number(c, state, "length")?;
    let written = number(c, state, "writelen")?;
    put_number(c, state, "length", length - written)?;
    put_number(c, state, "writelen", 0.0)?;
    if c.truthy_rooted(error)? {
        get(c, error, "stack")?;
        if !state::has(c, state, state::ERRORED)? {
            put_slot(c, state, Slot::ErroredValue, error)?;
            change(c, state, state::ERRORED, 0)?;
        }
        let readable = get(c, stream, "_readableState")?;
        if c.truthy_rooted(readable)? {
            let errored = get(c, readable, "errored")?;
            if !c.truthy_rooted(errored)? {
                put(c, readable, "errored", error)?;
            }
        }
        if sync {
            tick(c, Tick::OnWriteError, &[stream, state, error, callback])?;
        } else {
            on_write_error(c, stream, state, error, callback)?;
        }
        return Ok(c.undefined());
    }
    if state::has(c, state, BUFFERED)? {
        clear_buffer(c, stream, state)?;
    }
    if !sync {
        after_write(c, stream, state, 1.0, callback)?;
        return Ok(c.undefined());
    }
    let field = bits(c, state)?;
    let need_drain = field & NEED_DRAIN != 0 && number(c, state, "length")? == 0.0;
    let is_nop = c.strict_equal_rooted(callback, intrinsics.nop)?;
    // Node's `(state[kState] & kDestroyed !== 0)` binds as `state[kState] & 1`: the
    // object-mode bit.
    let need_tick = need_drain || field & state::OBJECT_MODE != 0 || !is_nop;
    if is_nop {
        if field & AFTER_WRITE_PENDING == 0 && need_tick {
            let count = c.number(1.0);
            tick(c, Tick::AfterWrite, &[stream, state, count, callback])?;
            change(c, state, AFTER_WRITE_PENDING, 0)?;
        } else {
            settle_sync_write(c, stream, state)?;
        }
        return Ok(c.undefined());
    }
    if field & AFTER_WRITE_TICK_INFO != 0 {
        let info = get_slot(c, state, Slot::AfterWriteTickInfoValue)?;
        let held = get(c, info, "cb")?;
        if c.strict_equal_rooted(held, callback)? {
            let count = number(c, info, "count")?;
            put_number(c, info, "count", count + 1.0)?;
            return Ok(c.undefined());
        }
    }
    if need_tick {
        let info = c.object_rooted()?;
        put_number(c, info, "count", 1.0)?;
        put(c, info, "cb", callback)?;
        put(c, info, "stream", stream)?;
        put(c, info, "state", state)?;
        put_slot(c, state, Slot::AfterWriteTickInfoValue, info)?;
        tick(c, Tick::AfterWriteBatch, &[info])?;
        change(c, state, AFTER_WRITE_PENDING | AFTER_WRITE_TICK_INFO, 0)?;
    } else {
        settle_sync_write(c, stream, state)?;
    }
    Ok(c.undefined())
}

/// A synchronous write that needs no tick: one fewer pending callback.
fn settle_sync_write(c: &mut Context<'_>, stream: RootId, state: RootId) -> Result<(), RootedError> {
    let pending = number(c, state, "pendingcb")?;
    put_number(c, state, "pendingcb", pending - 1.0)?;
    if state::has(c, state, ENDING)? {
        finish_maybe(c, stream, state, true)?;
    }
    Ok(())
}

pub(crate) fn after_write_tick_info(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let info = arg(c, args, 0);
    let stream = get(c, info, "stream")?;
    let state = get(c, info, "state")?;
    let count = number(c, info, "count")?;
    let callback = get(c, info, "cb")?;
    change(c, state, 0, AFTER_WRITE_TICK_INFO)?;
    let null = c.null();
    put_slot(c, state, Slot::AfterWriteTickInfoValue, null)?;
    after_write(c, stream, state, count, callback)?;
    Ok(c.undefined())
}

pub(crate) fn after_write_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, state, count, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let count = c.to_number(count)?;
    after_write(c, stream, state, count, callback)?;
    Ok(c.undefined())
}

/// `afterWrite(stream, state, count, cb)`.
fn after_write(c: &mut Context<'_>, stream: RootId, state: RootId, count: f64, callback: RootId) -> Result<(), RootedError> {
    let field = change(c, state, 0, AFTER_WRITE_PENDING)?;
    if field & (ENDING | NEED_DRAIN | state::DESTROYED) == NEED_DRAIN && number(c, state, "length")? == 0.0 {
        change(c, state, 0, NEED_DRAIN)?;
        emit(c, stream, "drain", &[])?;
    }
    let null = c.null();
    let undefined = c.undefined();
    let mut remaining = count;
    while remaining > 0.0 {
        remaining -= 1.0;
        let pending = number(c, state, "pendingcb")?;
        put_number(c, state, "pendingcb", pending - 1.0)?;
        c.call_rooted(callback, undefined, &[null])?;
    }
    if state::has(c, state, state::DESTROYED)? {
        error_buffer(c, state)?;
    }
    if state::has(c, state, ENDING)? {
        finish_maybe(c, stream, state, true)?;
    }
    Ok(())
}

pub(crate) fn error_buffer_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let state = arg(c, args, 0);
    error_buffer(c, state)?;
    Ok(c.undefined())
}

/// The error a destroyed stream's pending callbacks receive.
fn destroyed_error(c: &mut Context<'_>, state: RootId, operation: &str) -> Outcome {
    let errored = get(c, state, "errored")?;
    if is_nullish(c, errored) { errors::create(c, StreamError::Destroyed(operation)) } else { Ok(errored) }
}

/// `errorBuffer(state)`: fail every buffered write and pending `end` callback.
fn error_buffer(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    if state::has(c, state, WRITING)? {
        return Ok(());
    }
    if state::has(c, state, BUFFERED)? {
        let mut index = number(c, state, "bufferedIndex")?;
        loop {
            let buffered = get(c, state, "buffered")?;
            if index >= number(c, buffered, "length")? {
                break;
            }
            let list = get_slot(c, state, Slot::BufferedValue)?;
            let request = get(c, list, &(index as u64).to_string())?;
            let chunk = get(c, request, "chunk")?;
            let callback = get(c, request, "callback")?;
            let size = state::chunk_size(c, state, chunk)?;
            let length = number(c, state, "length")?;
            put_number(c, state, "length", length - size)?;
            let error = destroyed_error(c, state, "write")?;
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[error])?;
            index += 1.0;
        }
    }
    let error = destroyed_error(c, state, "end")?;
    call_finished_callbacks(c, state, error)?;
    reset_buffer(c, state)
}

/// `clearBuffer(stream, state)`: write out buffered requests, vectored when `_writev` exists.
fn clear_buffer(c: &mut Context<'_>, stream: RootId, state: RootId) -> Result<(), RootedError> {
    let field = bits(c, state)?;
    if field & (state::DESTROYED | BUFFER_PROCESSING | CORKED | BUFFERED | state::CONSTRUCTED) != BUFFERED | state::CONSTRUCTED {
        return Ok(());
    }
    let object_mode = field & state::OBJECT_MODE != 0;
    let buffered = get_slot(c, state, Slot::BufferedValue)?;
    let first = number(c, state, "bufferedIndex")?;
    let count = number(c, buffered, "length")? - first;
    if count == 0.0 {
        return Ok(());
    }
    change(c, state, BUFFER_PROCESSING, 0)?;
    let writev = get(c, stream, "_writev")?;
    if count > 1.0 && c.truthy_rooted(writev)? {
        let pending = number(c, state, "pendingcb")?;
        put_number(c, state, "pendingcb", pending - (count - 1.0))?;
        let all_noop = bits(c, state)? & ALL_NOOP != 0;
        let callback = if all_noop {
            intrinsics(c)?.nop
        } else {
            let first_root = c.number(first);
            closure(c, "streamWritevCallback", "", &[buffered, first_root])?
        };
        let chunks = if all_noop && first == 0.0 {
            buffered
        } else {
            let start = c.number(first);
            call_method(c, buffered, "slice", &[start])?
        };
        let all_buffers = state::has(c, state, ALL_BUFFERS)?;
        let flag = c.boolean(all_buffers);
        put(c, chunks, "allBuffers", flag)?;
        let length = number(c, state, "length")?;
        let empty = c.string_rooted("");
        do_write(c, stream, state, true, length, chunks, empty, callback)?;
        reset_buffer(c, state)?;
    } else {
        let null = c.null();
        let mut index = first;
        loop {
            let request = get(c, buffered, &(index as u64).to_string())?;
            let chunk = get(c, request, "chunk")?;
            let encoding = get(c, request, "encoding")?;
            let callback = get(c, request, "callback")?;
            put(c, buffered, &(index as u64).to_string(), null)?;
            index += 1.0;
            let size = if object_mode { 1.0 } else { length_of(c, chunk)? };
            do_write(c, stream, state, false, size, chunk, encoding, callback)?;
            if index >= number(c, buffered, "length")? || state::has(c, state, WRITING)? {
                break;
            }
        }
        if index == number(c, buffered, "length")? {
            reset_buffer(c, state)?;
        } else if index > BUFFER_COMPACTION_INDEX {
            let zero = c.number(0.0);
            let consumed = c.number(index);
            call_method(c, buffered, "splice", &[zero, consumed])?;
            put_number(c, state, "bufferedIndex", 0.0)?;
        } else {
            put_number(c, state, "bufferedIndex", index)?;
        }
    }
    change(c, state, 0, BUFFER_PROCESSING)?;
    Ok(())
}

/// The callback a vectored write of distinct callbacks receives: each request's.
pub(crate) fn writev_callback(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [buffered, first] = captures(c)?;
    let error = arg(c, args, 0);
    let mut index = c.to_number(first)?;
    let undefined = c.undefined();
    while index < number(c, buffered, "length")? {
        let request = get(c, buffered, &(index as u64).to_string())?;
        let callback = get(c, request, "callback")?;
        c.call_rooted(callback, undefined, &[error])?;
        index += 1.0;
    }
    Ok(undefined)
}

pub(crate) fn default_write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, encoding, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let writev = get(c, this, "_writev")?;
    if !c.truthy_rooted(writev)? {
        return Err(errors::throw(c, StreamError::MethodNotImplemented("_write()")));
    }
    let request = c.object_rooted()?;
    put(c, request, "chunk", chunk)?;
    put(c, request, "encoding", encoding)?;
    let list = c.array_rooted(&[request])?;
    c.call_rooted(writev, this, &[list, callback])?;
    Ok(c.undefined())
}

pub(crate) fn end(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = state_of(c, this)?;
    let (mut chunk, mut encoding, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if is_function(c, chunk)? {
        callback = chunk;
        chunk = c.null();
        encoding = c.null();
    } else if is_function(c, encoding)? {
        callback = encoding;
        encoding = c.null();
    }
    let mut error = None;
    if !is_nullish(c, chunk) {
        let undefined = c.undefined();
        if let Written::Failed(failure) = write_chunk(c, this, chunk, encoding, undefined)? {
            error = Some(failure);
        }
    }
    if state::has(c, state, CORKED)? {
        put_number(c, state, "corked", 1.0)?;
        call_method(c, this, "uncork", &[])?;
    }
    if error.is_none() {
        let field = bits(c, state)?;
        if field & (ENDING | state::ERRORED) == 0 {
            change(c, state, ENDING, 0)?;
            finish_maybe(c, this, state, true)?;
            change(c, state, ENDED, 0)?;
        } else if field & FINISHED != 0 {
            error = Some(errors::create(c, StreamError::AlreadyFinished("end"))?);
        } else if field & state::DESTROYED != 0 {
            error = Some(errors::create(c, StreamError::Destroyed("end"))?);
        }
    }
    if is_function(c, callback)? {
        let field = bits(c, state)?;
        if let Some(error) = error {
            next_tick(c, callback, &[error])?;
        } else if field & state::ERRORED != 0 {
            let errored = get_slot(c, state, Slot::ErroredValue)?;
            next_tick(c, callback, &[errored])?;
        } else if field & FINISHED != 0 {
            let null = c.null();
            next_tick(c, callback, &[null])?;
        } else {
            change(c, state, ON_FINISHED, 0)?;
            let mut list = get_slot(c, state, Slot::OnFinishedValue)?;
            if is_nullish(c, list) {
                list = c.array_rooted(&[])?;
                put_slot(c, state, Slot::OnFinishedValue, list)?;
            }
            call_method(c, list, "push", &[callback])?;
        }
    }
    Ok(this)
}

/// `needFinish(state)`.
fn need_finish(c: &mut Context<'_>, state: RootId) -> Result<bool, RootedError> {
    let mask = ENDING
        | state::DESTROYED
        | state::CONSTRUCTED
        | FINISHED
        | WRITING
        | state::ERROR_EMITTED
        | state::CLOSE_EMITTED
        | state::ERRORED
        | BUFFERED;
    Ok(bits(c, state)? & mask == ENDING | state::CONSTRUCTED && number(c, state, "length")? == 0.0)
}

/// `onFinish(err)`: the callback `_final` receives.
pub(crate) fn on_final(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, state] = captures(c)?;
    let error = arg(c, args, 0);
    on_finish(c, stream, state, error)?;
    Ok(c.undefined())
}

fn on_finish(c: &mut Context<'_>, stream: RootId, state: RootId, error: RootId) -> Result<(), RootedError> {
    if state::has(c, state, PREFINISHED)? {
        let error = if is_nullish(c, error) { errors::create(c, StreamError::MultipleCallback)? } else { error };
        return error_or_destroy(c, stream, error, false);
    }
    let pending = number(c, state, "pendingcb")?;
    put_number(c, state, "pendingcb", pending - 1.0)?;
    if c.truthy_rooted(error)? {
        call_finished_callbacks(c, state, error)?;
        let sync = state::has(c, state, SYNC)?;
        return error_or_destroy(c, stream, error, sync);
    }
    if need_finish(c, state)? {
        change(c, state, PREFINISHED, 0)?;
        emit(c, stream, "prefinish", &[])?;
        let pending = number(c, state, "pendingcb")?;
        put_number(c, state, "pendingcb", pending + 1.0)?;
        tick(c, Tick::Finish, &[stream, state])?;
    }
    Ok(())
}

/// `prefinish(stream, state)`: run `_final`, or emit `prefinish` directly.
fn prefinish(c: &mut Context<'_>, stream: RootId, state: RootId) -> Result<(), RootedError> {
    let field = bits(c, state)?;
    if field & (PREFINISHED | FINAL_CALLED) != 0 {
        return Ok(());
    }
    let final_method = get(c, stream, "_final")?;
    if is_function(c, final_method)? && field & state::DESTROYED == 0 {
        change(c, state, FINAL_CALLED | SYNC, 0)?;
        let pending = number(c, state, "pendingcb")?;
        put_number(c, state, "pendingcb", pending + 1.0)?;
        let callback = closure(c, "streamOnFinal", "", &[stream, state])?;
        let result = c.call_rooted(final_method, stream, &[callback]);
        if let Err(thrown) = super::attempt(result)? {
            on_finish(c, stream, state, thrown)?;
        }
        change(c, state, 0, SYNC)?;
    } else {
        change(c, state, FINAL_CALLED | PREFINISHED, 0)?;
        emit(c, stream, "prefinish", &[])?;
    }
    Ok(())
}

/// `finishMaybe(stream, state, sync)`.
fn finish_maybe(c: &mut Context<'_>, stream: RootId, state: RootId, sync: bool) -> Result<(), RootedError> {
    if !need_finish(c, state)? {
        return Ok(());
    }
    prefinish(c, stream, state)?;
    if number(c, state, "pendingcb")? != 0.0 {
        return Ok(());
    }
    if sync {
        let pending = number(c, state, "pendingcb")?;
        put_number(c, state, "pendingcb", pending + 1.0)?;
        tick(c, Tick::FinishIfNeeded, &[stream, state])
    } else if need_finish(c, state)? {
        let pending = number(c, state, "pendingcb")?;
        put_number(c, state, "pendingcb", pending + 1.0)?;
        finish(c, stream, state)
    } else {
        Ok(())
    }
}

pub(crate) fn finish_if_needed_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, state) = (arg(c, args, 0), arg(c, args, 1));
    if need_finish(c, state)? {
        finish(c, stream, state)?;
    } else {
        let pending = number(c, state, "pendingcb")?;
        put_number(c, state, "pendingcb", pending - 1.0)?;
    }
    Ok(c.undefined())
}

pub(crate) fn finish_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, state) = (arg(c, args, 0), arg(c, args, 1));
    finish(c, stream, state)?;
    Ok(c.undefined())
}

/// `finish(stream, state)`.
fn finish(c: &mut Context<'_>, stream: RootId, state: RootId) -> Result<(), RootedError> {
    let pending = number(c, state, "pendingcb")?;
    put_number(c, state, "pendingcb", pending - 1.0)?;
    change(c, state, FINISHED, 0)?;
    let null = c.null();
    call_finished_callbacks(c, state, null)?;
    emit(c, stream, "finish", &[])?;
    if !state::has(c, state, state::AUTO_DESTROY)? {
        return Ok(());
    }
    let readable = get(c, stream, "_readableState")?;
    let destroy = if c.truthy_rooted(readable)? {
        let auto = get(c, readable, "autoDestroy")?;
        let ended = get(c, readable, "endEmitted")?;
        let flag = get(c, readable, "readable")?;
        let no = c.boolean(false);
        c.truthy_rooted(auto)? && (c.truthy_rooted(ended)? || c.strict_equal_rooted(flag, no)?)
    } else {
        true
    };
    if destroy {
        call_method(c, stream, "destroy", &[])?;
    }
    Ok(())
}

/// `callFinishedCallbacks(state, err)`.
fn call_finished_callbacks(c: &mut Context<'_>, state: RootId, error: RootId) -> Result<(), RootedError> {
    if !state::has(c, state, ON_FINISHED)? {
        return Ok(());
    }
    let callbacks = get_slot(c, state, Slot::OnFinishedValue)?;
    let null = c.null();
    put_slot(c, state, Slot::OnFinishedValue, null)?;
    change(c, state, 0, ON_FINISHED)?;
    let undefined = c.undefined();
    for callback in c.array_like_values_rooted(callbacks)? {
        c.call_rooted(callback, undefined, &[error])?;
    }
    Ok(())
}

/// `Writable.prototype.destroy(err, cb)`: fail buffered writes on the next tick.
pub(crate) fn destroy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = state_of(c, this)?;
    let field = bits(c, state)?;
    if field & (BUFFERED | ON_FINISHED) != 0 && field & state::DESTROYED == 0 {
        tick(c, Tick::ErrorBuffer, &[state])?;
    }
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    destroy::destroy_with(c, this, error, callback)?;
    Ok(this)
}

/// `Writable.prototype[Symbol.asyncDispose]()`.
pub(crate) fn async_dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let destroyed = get(c, this, "destroyed")?;
    let mut error = c.undefined();
    if !c.truthy_rooted(destroyed)? {
        let finished = get(c, this, "writableFinished")?;
        error = if c.truthy_rooted(finished)? { c.null() } else { errors::abort_error(c, None)? };
        call_method(c, this, "destroy", &[error])?;
    }
    super::eos::settle_when_finished(c, this, error, super::eos::Tolerated::Aborts)
}

/// `Writable.prototype`'s methods before its accessors, as Node defines them.
pub(crate) const PROTOTYPE_METHODS: &[(Key, &str, &str)] = &[
    (Key::Name("pipe"), "streamWritablePipe", ""),
    (Key::Name("write"), "streamWritableWrite", ""),
    (Key::Name("cork"), "streamWritableCork", ""),
    (Key::Name("uncork"), "streamWritableUncork", ""),
    (Key::Name("setDefaultEncoding"), "streamWritableSetDefaultEncoding", "setDefaultEncoding"),
    (Key::Name("_write"), "streamWritableDefaultWrite", ""),
];

pub(crate) fn build(c: &mut Context<'_>, stream: RootId, state_class: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamWritable"), "Writable")?;
    let prototype = get(c, constructor, "prototype")?;
    let stream_prototype = get(c, stream, "prototype")?;
    c.set_prototype_rooted(prototype, stream_prototype)?;
    c.set_prototype_rooted(constructor, stream)?;
    let key = c.string_rooted("WritableState");
    c.define_data_property_rooted(constructor, key, state_class, true, true, true)?;
    let has_instance = c.host_function_named(crate::host::shared_vm::operation("streamWritableHasInstance"), "value")?;
    let key = intrinsics(c)?.slot(Slot::HasInstance);
    c.define_data_property_rooted(constructor, key, has_instance, false, false, false)?;
    define_methods(c, prototype, PROTOTYPE_METHODS)?;
    let null = c.null();
    let key = c.string_rooted("_writev");
    c.define_data_property_rooted(prototype, key, null, true, true, true)?;
    define_methods(c, prototype, &[(Key::Name("end"), "streamWritableEnd", "")])?;
    super::utils::define_stream_accessors(c, prototype, super::utils::WRITABLE_ACCESSORS)?;
    define_methods(
        c,
        prototype,
        &[
            (Key::Name("destroy"), "streamWritableDestroy", ""),
            (Key::Name("_undestroy"), "streamUndestroy", "undestroy"),
            (Key::Name("_destroy"), "streamDefaultDestroy", ""),
            (Key::Symbol(Slot::Rejection), "streamCaptureRejection", ""),
            (Key::Symbol(Slot::AsyncDispose), "streamWritableAsyncDispose", ""),
        ],
    )?;
    Ok(constructor)
}
