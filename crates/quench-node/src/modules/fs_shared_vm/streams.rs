//! `fs.ReadStream` and `fs.WriteStream` (internal/fs/streams.js): a `Readable` and a
//! `Writable` over a file the stream opens, or a descriptor it is given.
//!
//! A stream does its I/O through `options.fs` when one is given, as Node's does through
//! `this[kFs]`; otherwise through the host's descriptor table, completing each operation
//! from the immediate queue as a threadpool completion arrives after the current turn.

use crate::modules::shared_event_loop::{SharedCallback, SharedImmediateOwner};
use crate::modules::stream_shared_vm::{
    arg, attempt, call_method, captures, closure, emit, eos, get, is_nullish, is_undefined, next_tick, number, once,
    put, put_bool, put_number, truthy_member, Context, Outcome,
};
use quench_runtime::{RootId, RootedError, ValueType};

/// The default `highWaterMark` of a read stream: 64 KiB.
const READ_HIGH_WATER_MARK: f64 = 64.0 * 1024.0;
/// The default creation mode, `0o666`.
const DEFAULT_MODE: f64 = 0o666 as f64;
/// The writes in a row that may make no progress before a write fails.
const MAX_WRITE_RETRIES: f64 = 5.0;
/// The largest integer a position may be, `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// The private symbols a stream keys its state with.
#[derive(Clone, Copy)]
enum Slot {
    /// `kFs`: the operations the stream uses, or `null` for the host's own.
    Fs,
    /// `kIsPerformingIO`.
    IsPerformingIo,
    /// `kIoDone`: the event an operation finishing on a destroyed stream emits.
    IoDone,
}

impl Slot {
    fn description(self) -> &'static str {
        match self {
            Self::Fs => "kFs",
            Self::IsPerformingIo => "kIsPerformingIO",
            Self::IoDone => "kIoDone",
        }
    }
}

fn slot(c: &mut Context<'_>, which: Slot) -> Outcome {
    let key = format!("\0fs:{}", which.description());
    let shared = c.host_mut().shared_state();
    if let Some(symbol) = shared.borrow().builtin_cache.get(&key).copied() {
        return Ok(symbol);
    }
    let symbol = c.symbol_rooted(Some(which.description()));
    let retained = c.retain(symbol)?;
    shared.borrow_mut().builtin_cache.insert(key, retained);
    Ok(symbol)
}

fn get_slot(c: &mut Context<'_>, object: RootId, which: Slot) -> Outcome {
    let key = slot(c, which)?;
    c.get_property_rooted(object, key)
}

fn put_slot(c: &mut Context<'_>, object: RootId, which: Slot, value: RootId) -> Result<(), RootedError> {
    let key = slot(c, which)?;
    c.set_property_rooted(object, key, value, object).map(drop)
}

fn performing_io(c: &mut Context<'_>, stream: RootId, value: bool) -> Result<(), RootedError> {
    let value = c.boolean(value);
    put_slot(c, stream, Slot::IsPerformingIo, value)
}

fn is_function(c: &Context<'_>, value: RootId) -> bool {
    matches!(c.value_type_rooted(value), Some(ValueType::Function))
}

fn is_number(c: &Context<'_>, value: RootId) -> bool {
    matches!(c.value_type_rooted(value), Some(ValueType::Number))
}

fn thrown(c: &mut Context<'_>, built: Outcome) -> RootedError {
    match built {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

fn invalid_type(c: &mut Context<'_>, value: RootId, name: &str, expected: &str) -> RootedError {
    match crate::modules::util_shared_vm::argument_type_error_with_expected(c, value, name, expected) {
        Ok(error) | Err(error) => error,
    }
}

fn require_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if is_function(c, value) { Ok(()) } else { Err(invalid_type(c, value, name, "of type function")) }
}

/// `validateInteger(value, name, 0)`.
fn non_negative_integer(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    let maximum = c.number(MAX_SAFE_INTEGER);
    crate::modules::util_shared_vm::validate_integer(c, value, name, 0.0, maximum)
}

// ---------------------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------------------

/// `copyObject(getOptions(options, kEmptyObject))`.
fn options_copy(c: &mut Context<'_>, options: RootId) -> Outcome {
    let copy = c.object_rooted()?;
    if is_nullish(c, options) || is_function(c, options) {
        return Ok(copy);
    }
    if crate::modules::stream_shared_vm::is_string(c, options) {
        put(c, copy, "encoding", options)?;
    } else if c.is_object_rooted(options)? {
        for key in c.enumerable_own_keys_rooted(options, false, true)? {
            let value = c.get_property_rooted(options, key)?;
            c.set_property_rooted(copy, key, value, copy)?;
        }
    } else {
        return Err(invalid_type(c, options, "options", "one of type string or object"));
    }
    let encoding = get(c, copy, "encoding")?;
    let is_buffer = c.string_text(encoding)?.as_deref() == Some("buffer");
    if !is_buffer && c.truthy_rooted(encoding)? {
        let undefined = c.undefined();
        let known = crate::modules::buffer_module::is_encoding(c, undefined, &[encoding])?;
        if !c.truthy_rooted(known)? {
            let error = crate::modules::util_shared_vm::argument_value_error(c, encoding, "encoding", "is invalid encoding");
            return Err(match error {
                Ok(error) | Err(error) => error,
            });
        }
    }
    let signal = get(c, copy, "signal")?;
    if !is_undefined(c, signal) {
        crate::modules::util_shared_vm::validate_abort_signal(c, signal, "options.signal")?;
    }
    Ok(copy)
}

/// `importFd(stream, options)` then `getValidatedFd`: a numeric descriptor and its `fs`.
fn import_fd(c: &mut Context<'_>, stream: RootId, options: RootId) -> Result<RootId, RootedError> {
    let fd = get(c, options, "fd")?;
    if super::file_handle::descriptor_of(c, fd)?.is_some() {
        let operations = super::file_handle::stream_operations(c, fd)?;
        put_slot(c, stream, Slot::Fs, operations)?;
        return get(c, fd, "fd");
    }
    if !is_number(c, fd) {
        return Err(invalid_type(c, fd, "options.fd", "of type number or an instance of FileHandle"));
    }
    let operations = get(c, options, "fs")?;
    let operations = if c.truthy_rooted(operations)? { operations } else { c.null() };
    put_slot(c, stream, Slot::Fs, operations)?;
    let maximum = c.number(f64::from(i32::MAX));
    crate::modules::util_shared_vm::validate_integer(c, fd, "fd", 0.0, maximum)?;
    Ok(fd)
}

/// The file the stream opens: `path`, `flags` and `mode`, or the descriptor it was given.
fn initialize_source(c: &mut Context<'_>, stream: RootId, path: RootId, options: RootId, default_flags: &str) -> Result<(), RootedError> {
    let fd = get(c, options, "fd")?;
    if is_nullish(c, fd) {
        let null = c.null();
        put(c, stream, "fd", null)?;
        let operations = get(c, options, "fs")?;
        let operations = if c.truthy_rooted(operations)? { operations } else { null };
        put_slot(c, stream, Slot::Fs, operations)?;
        if !super::super::stream_shared_vm::is_null(c, operations) {
            let open = get(c, operations, "open")?;
            require_function(c, open, "options.fs.open")?;
        }
        let path = super::super::fs_path::validate(c, path, "path")?;
        put(c, stream, "path", path)?;
        let flags = get(c, options, "flags")?;
        let flags = if is_undefined(c, flags) { c.string_rooted(default_flags) } else { flags };
        put(c, stream, "flags", flags)?;
        let mode = get(c, options, "mode")?;
        let mode = if is_undefined(c, mode) { c.number(DEFAULT_MODE) } else { mode };
        put(c, stream, "mode", mode)
    } else {
        let fd = import_fd(c, stream, options)?;
        put(c, stream, "fd", fd)
    }
}

/// `options.autoDestroy = options.autoClose === undefined ? true : options.autoClose`.
fn auto_close(c: &mut Context<'_>, options: RootId) -> Result<bool, RootedError> {
    let value = get(c, options, "autoClose")?;
    let value = if is_undefined(c, value) { c.boolean(true) } else { value };
    put(c, options, "autoDestroy", value)?;
    c.truthy_rooted(value)
}

/// The function `options.fs[name]`, checked as the constructor checks it.
fn custom_operation(c: &mut Context<'_>, stream: RootId, name: &str) -> Result<Option<RootId>, RootedError> {
    let operations = get_slot(c, stream, Slot::Fs)?;
    if super::super::stream_shared_vm::is_null(c, operations) {
        return Ok(None);
    }
    get(c, operations, name).map(Some)
}

fn require_operation(c: &mut Context<'_>, stream: RootId, name: &str) -> Result<(), RootedError> {
    if let Some(operation) = custom_operation(c, stream, name)? {
        require_function(c, operation, &format!("options.fs.{name}"))?;
    }
    Ok(())
}

/// The position option `start`, validated, as `pos`.
fn initialize_position(c: &mut Context<'_>, stream: RootId, options: RootId) -> Result<Option<f64>, RootedError> {
    let start = get(c, options, "start")?;
    put(c, stream, "start", start)?;
    let undefined = c.undefined();
    put(c, stream, "pos", undefined)?;
    if is_undefined(c, start) {
        return Ok(None);
    }
    let start = non_negative_integer(c, start, "start")?;
    put_number(c, stream, "pos", start)?;
    Ok(Some(start))
}

// ---------------------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------------------

/// Run `callback(...args)` from the immediate queue.
pub(crate) fn complete(c: &mut Context<'_>, callback: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let mut values = vec![callback];
    values.extend_from_slice(args);
    let invoke = closure(c, "fsStreamComplete", "", &values)?;
    let shared = c.host_mut().shared_state();
    let id = shared
        .borrow_mut()
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("fs stream completion identifiers exhausted"))?;
    let invoke = c.retain(invoke)?;
    let undefined = c.undefined();
    let receiver = c.retain(undefined)?;
    shared.borrow_mut().scheduler.queue_shared_immediate(
        id,
        SharedImmediateOwner::Host,
        SharedCallback { execution: None, callback: invoke, receiver, args: Vec::new() },
    );
    Ok(())
}

/// The completion `complete` queued: `callback(...args)`.
pub(crate) fn complete_operation(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let environment = c.host_function_data()?;
    let values = c.array_like_values_rooted(environment)?;
    let undefined = c.undefined();
    c.call_rooted(values[0], undefined, &values[1..])
}

/// A host operation's outcome as a callback's arguments: `(null, ...values)` or `(error)`.
fn settle(c: &mut Context<'_>, result: Result<Vec<RootId>, RootId>, callback: RootId) -> Result<(), RootedError> {
    match result {
        Ok(values) => {
            let mut args = vec![c.null()];
            args.extend(values);
            complete(c, callback, &args)
        }
        Err(error) => complete(c, callback, &[error]),
    }
}

/// Run one of `fs`'s sync operations; a thrown error becomes the operation's error.
fn host_operation(c: &mut Context<'_>, op: super::ops::Op, args: &[RootId]) -> Result<Result<RootId, RootId>, RootedError> {
    attempt(super::api::run(c, op, super::api::Flavor::Sync, args))
}

/// `kFs.open(path, flags, mode, cb)`.
fn open(c: &mut Context<'_>, stream: RootId, callback: RootId) -> Result<(), RootedError> {
    let path = get(c, stream, "path")?;
    let flags = get(c, stream, "flags")?;
    let mode = get(c, stream, "mode")?;
    if let Some(open) = custom_operation(c, stream, "open")? {
        let operations = get_slot(c, stream, Slot::Fs)?;
        c.call_rooted(open, operations, &[path, flags, mode, callback])?;
        return Ok(());
    }
    let result = host_operation(c, super::ops::Op::Open, &[path, flags, mode])?;
    settle(c, result.map(|fd| vec![fd]), callback)
}

/// `kFs.close(fd, cb)`.
fn close_descriptor(c: &mut Context<'_>, stream: RootId, fd: RootId, callback: RootId) -> Result<(), RootedError> {
    if let Some(close) = custom_operation(c, stream, "close")? {
        let operations = get_slot(c, stream, Slot::Fs)?;
        c.call_rooted(close, operations, &[fd, callback])?;
        return Ok(());
    }
    let result = host_operation(c, super::ops::Op::Close, &[fd])?;
    settle(c, result.map(|_| Vec::new()), callback)
}

/// `kFs.fsync(fd, cb)`.
fn sync_descriptor(c: &mut Context<'_>, stream: RootId, fd: RootId, callback: RootId) -> Result<(), RootedError> {
    if let Some(fsync) = custom_operation(c, stream, "fsync")? {
        let operations = get_slot(c, stream, Slot::Fs)?;
        c.call_rooted(fsync, operations, &[fd, callback])?;
        return Ok(());
    }
    let result = host_operation(c, super::ops::Op::Fsync, &[fd])?;
    settle(c, result.map(|_| Vec::new()), callback)
}

/// `close(stream, err, cb)`: sync first when `flush` asks, then close the descriptor.
fn close_stream(c: &mut Context<'_>, stream: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    let fd = get(c, stream, "fd")?;
    if !c.truthy_rooted(fd)? {
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[error])?;
        return Ok(());
    }
    if truthy_member(c, stream, "flush")? {
        let after = closure(c, "fsStreamFlushed", "", &[stream, error, callback])?;
        return sync_descriptor(c, stream, fd, after);
    }
    finish_close(c, stream, error, callback)
}

/// `_close(stream, err, cb)`.
fn finish_close(c: &mut Context<'_>, stream: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    let fd = get(c, stream, "fd")?;
    let after = closure(c, "fsStreamClosed", "", &[error, callback])?;
    close_descriptor(c, stream, fd, after)?;
    let null = c.null();
    put(c, stream, "fd", null)
}

pub(crate) fn flushed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, error, callback] = captures(c)?;
    let flush_error = arg(c, args, 0);
    let error = if c.truthy_rooted(error)? { error } else { flush_error };
    finish_close(c, stream, error, callback)?;
    Ok(c.undefined())
}

/// `(er) => cb(er || err)`.
pub(crate) fn closed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [error, callback] = captures(c)?;
    let close_error = arg(c, args, 0);
    let error = if c.truthy_rooted(close_error)? { close_error } else { error };
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])
}

/// `_destroy(err, cb)`: close once the operation in progress finishes.
pub(crate) fn destroy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    let busy = get_slot(c, this, Slot::IsPerformingIo)?;
    if c.truthy_rooted(busy)? {
        let after = closure(c, "fsStreamIoDone", "", &[this, error, callback])?;
        let event = slot(c, Slot::IoDone)?;
        call_method(c, this, "once", &[event, after])?;
    } else {
        close_stream(c, this, error, callback)?;
    }
    Ok(c.undefined())
}

pub(crate) fn io_done(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, error, callback] = captures(c)?;
    let io_error = arg(c, args, 0);
    let error = if c.truthy_rooted(error)? { error } else { io_error };
    close_stream(c, stream, error, callback)?;
    Ok(c.undefined())
}

fn emit_io_done(c: &mut Context<'_>, stream: RootId, error: RootId) -> Result<(), RootedError> {
    let event = slot(c, Slot::IoDone)?;
    crate::modules::stream_shared_vm::emit_key(c, stream, event, &[error]).map(drop)
}

/// `_construct(callback)`: open the file unless the stream has a descriptor.
fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId], writing: bool) -> Outcome {
    let callback = arg(c, args, 0);
    let fd = get(c, this, "fd")?;
    if is_number(c, fd) {
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[])?;
        return Ok(undefined);
    }
    let open_method = get(c, this, "open")?;
    let prototype_open = default_open(c, writing)?;
    if !c.strict_equal_rooted(open_method, prototype_open)? {
        let opened = closure(c, "fsStreamCustomOpened", "", &[callback])?;
        let failed = closure(c, "fsStreamCustomFailed", "", &[callback])?;
        once(c, this, "open", opened)?;
        once(c, this, "error", failed)?;
        call_method(c, this, "open", &[])?;
        return Ok(c.undefined());
    }
    let opened = closure(c, "fsStreamOpened", "", &[this, callback])?;
    open(c, this, opened)?;
    Ok(c.undefined())
}

pub(crate) fn read_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    construct(c, this, args, false)
}

pub(crate) fn write_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    construct(c, this, args, true)
}

/// The open callback: record the descriptor, finish construction, then `open` and `ready`.
pub(crate) fn opened(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, callback] = captures(c)?;
    let (error, fd) = (arg(c, args, 0), arg(c, args, 1));
    let undefined = c.undefined();
    if c.truthy_rooted(error)? {
        c.call_rooted(callback, undefined, &[error])?;
        return Ok(undefined);
    }
    put(c, stream, "fd", fd)?;
    c.call_rooted(callback, undefined, &[])?;
    let fd = get(c, stream, "fd")?;
    emit(c, stream, "open", &[fd])?;
    emit(c, stream, "ready", &[])?;
    Ok(undefined)
}

pub(crate) fn custom_opened(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [callback] = captures(c)?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])
}

pub(crate) fn custom_failed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [callback] = captures(c)?;
    let error = arg(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])
}

/// The deprecated `open()` the prototype carries, which construction compares against.
fn default_open(c: &mut Context<'_>, writing: bool) -> Outcome {
    let key = if writing { "\0fs:openWriteFs" } else { "\0fs:openReadFs" };
    let shared = c.host_mut().shared_state();
    let found = shared.borrow().builtin_cache.get(key).copied();
    found.ok_or_else(|| RootedError::host("fs stream classes are not initialized"))
}

/// The deprecated `open()`: does nothing.
pub(crate) fn deprecated_open(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// ReadStream
// ---------------------------------------------------------------------------------------

fn needs_new(c: &mut Context<'_>, this: RootId) -> Result<Option<RootId>, RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(None);
    }
    let [class] = captures(c)?;
    if c.is_object_rooted(this)? && c.instance_of_rooted(this, class)? {
        return Ok(None);
    }
    Ok(Some(class))
}

fn stream_class(c: &mut Context<'_>, name: &str) -> Outcome {
    let stream = crate::modules::stream_shared_vm::module(c)?;
    get(c, stream, name)
}

/// `new ReadStream(path[, options])`.
pub(crate) fn read_stream(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (path, options) = (arg(c, args, 0), arg(c, args, 1));
    if let Some(class) = needs_new(c, this)? {
        return c.construct_rooted(class, class, &[path, options]);
    }
    let options = options_copy(c, options)?;
    let mark = get(c, options, "highWaterMark")?;
    if is_undefined(c, mark) {
        put_number(c, options, "highWaterMark", READ_HIGH_WATER_MARK)?;
    }
    let destroy = get(c, options, "autoDestroy")?;
    if is_undefined(c, destroy) {
        put_bool(c, options, "autoDestroy", false)?;
    }
    initialize_source(c, this, path, options, "r")?;
    let closes = auto_close(c, options)?;
    require_operation(c, this, "read")?;
    if closes {
        require_operation(c, this, "close")?;
    }
    let start = initialize_position(c, this, options)?;
    let end = get(c, options, "end")?;
    put(c, this, "end", end)?;
    put_number(c, this, "bytesRead", 0.0)?;
    performing_io(c, this, false)?;
    if is_undefined(c, end) {
        put_number(c, this, "end", f64::INFINITY)?;
    } else if c.rooted_value(end).and_then(|value| value.as_number()) != Some(f64::INFINITY) {
        let end_value = non_negative_integer(c, end, "end")?;
        if let Some(start) = start.filter(|start| *start > end_value) {
            let start = c.number(start);
            let received = crate::modules::util_shared_vm::range_error_received(c, start)?;
            let end_text = c.to_string(end)?;
            let message =
                format!("The value of \"start\" is out of range. It must be <= \"end\" (here: {end_text}). Received {received}");
            let built = c.range_error_rooted(&message).and_then(|error| {
                crate::modules::node_errors_shared_vm::decorate_range_error(c, error, "ERR_OUT_OF_RANGE")?;
                Ok(error)
            });
            return Err(thrown(c, built));
        }
    }
    let readable = stream_class(c, "Readable")?;
    c.call_rooted(readable, this, &[options])?;
    Ok(c.undefined())
}

/// `ReadStream.prototype._read(n)`: read at most up to `end`.
pub(crate) fn read(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let requested = arg(c, args, 0);
    let requested = c.to_number(requested)?;
    let end = number(c, this, "end")?;
    let position = get(c, this, "pos")?;
    let size = match c.rooted_value(position).and_then(|value| value.as_number()) {
        Some(position) => (end - position + 1.0).min(requested),
        None => (end - number(c, this, "bytesRead")? + 1.0).min(requested),
    };
    if size <= 0.0 {
        let null = c.null();
        call_method(c, this, "push", &[null])?;
        return Ok(c.undefined());
    }
    performing_io(c, this, true)?;
    let done = closure(c, "fsStreamsReadChunk", "", &[this])?;
    let fd = get(c, this, "fd")?;
    let size = c.number(size);
    if let Some(read) = custom_operation(c, this, "read")? {
        let operations = get_slot(c, this, Slot::Fs)?;
        let length = c.to_number(size)? as usize;
        let buffer = crate::modules::buffer_shared_vm::from_vec(c, vec![0; length])?;
        let zero = c.number(0.0);
        c.call_rooted(read, operations, &[fd, buffer, zero, size, position, done])?;
        return Ok(c.undefined());
    }
    let undefined = c.undefined();
    let result = attempt(super::stream_read(c, undefined, &[fd, size, position]))?;
    match result {
        Ok(chunk) => {
            let null = c.null();
            let length = if super::super::stream_shared_vm::is_null(c, chunk) { 0.0 } else { number(c, chunk, "length")? };
            let length = c.number(length);
            complete(c, done, &[null, length, chunk])?;
        }
        Err(error) => complete(c, done, &[error])?,
    }
    Ok(c.undefined())
}

/// The read callback `(er, bytesRead, buf)`.
pub(crate) fn read_chunk(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream] = captures(c)?;
    let (error, read, buffer) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    performing_io(c, stream, false)?;
    if truthy_member(c, stream, "destroyed")? {
        emit_io_done(c, stream, error)?;
        return Ok(c.undefined());
    }
    if c.truthy_rooted(error)? {
        crate::modules::stream_shared_vm::destroy::error_or_destroy(c, stream, error, false)?;
        return Ok(c.undefined());
    }
    let read = c.to_number(read)?;
    if read > 0.0 {
        let position = get(c, stream, "pos")?;
        if let Some(position) = c.rooted_value(position).and_then(|value| value.as_number()) {
            put_number(c, stream, "pos", position + read)?;
        }
        let total = number(c, stream, "bytesRead")?;
        put_number(c, stream, "bytesRead", total + read)?;
        let chunk = if number(c, buffer, "length")? == read {
            buffer
        } else {
            let bytes = c.view_bytes_rooted(buffer).unwrap_or_default();
            crate::modules::buffer_shared_vm::from_vec(c, bytes[..read as usize].to_vec())?
        };
        call_method(c, stream, "push", &[chunk])?;
    } else {
        let null = c.null();
        call_method(c, stream, "push", &[null])?;
    }
    Ok(c.undefined())
}

/// `ReadStream.prototype.close([cb])`.
pub(crate) fn read_close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    if is_function(c, callback) {
        let undefined = c.undefined();
        eos::finished(c, undefined, &[this, callback])?;
    }
    call_method(c, this, "destroy", &[])?;
    Ok(c.undefined())
}

/// The `pending` getter: whether the file is still opening.
pub(crate) fn pending(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let fd = get(c, this, "fd")?;
    let pending = super::super::stream_shared_vm::is_null(c, fd);
    Ok(c.boolean(pending))
}

/// `validateThisInternalField(this, kFs, name)`.
fn require_stream(c: &mut Context<'_>, this: RootId, class: &str) -> Result<(), RootedError> {
    let key = slot(c, Slot::Fs)?;
    if c.is_object_rooted(this)? && c.has_property_rooted(this, key)? {
        return Ok(());
    }
    Err(invalid_type(c, this, "this", &format!("an instance of {class}")))
}

fn auto_close_state(c: &mut Context<'_>, this: RootId, writing: bool) -> Outcome {
    let (class, state) = if writing { ("WriteStream", "_writableState") } else { ("ReadStream", "_readableState") };
    require_stream(c, this, class)?;
    get(c, this, state)
}

/// The `autoClose` getter: the state's `autoDestroy`.
pub(crate) fn read_auto_close(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = auto_close_state(c, this, false)?;
    get(c, state, "autoDestroy")
}

pub(crate) fn set_read_auto_close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = auto_close_state(c, this, false)?;
    let value = arg(c, args, 0);
    put(c, state, "autoDestroy", value)?;
    Ok(c.undefined())
}

pub(crate) fn write_auto_close(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = auto_close_state(c, this, true)?;
    get(c, state, "autoDestroy")
}

pub(crate) fn set_write_auto_close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = auto_close_state(c, this, true)?;
    let value = arg(c, args, 0);
    put(c, state, "autoDestroy", value)?;
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// WriteStream
// ---------------------------------------------------------------------------------------

/// `new WriteStream(path[, options])`.
pub(crate) fn write_stream(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (path, options) = (arg(c, args, 0), arg(c, args, 1));
    if let Some(class) = needs_new(c, this)? {
        return c.construct_rooted(class, class, &[path, options]);
    }
    let options = options_copy(c, options)?;
    put_bool(c, options, "decodeStrings", true)?;
    initialize_source(c, this, path, options, "w")?;
    let closes = auto_close(c, options)?;
    if let Some(write) = custom_operation(c, this, "write")? {
        let writev = custom_operation(c, this, "writev")?.unwrap_or_else(|| c.undefined());
        if !c.truthy_rooted(write)? && !c.truthy_rooted(writev)? {
            return Err(invalid_type(c, write, "options.fs.write", "of type function"));
        }
        for (name, value) in [("write", write), ("writev", writev)] {
            if c.truthy_rooted(value)? {
                require_function(c, value, &format!("options.fs.{name}"))?;
            }
        }
    }
    if closes {
        require_operation(c, this, "close")?;
    }
    let flush = get(c, options, "flush")?;
    if is_nullish(c, flush) {
        put_bool(c, this, "flush", false)?;
    } else {
        if !matches!(c.value_type_rooted(flush), Some(ValueType::Boolean)) {
            return Err(invalid_type(c, flush, "options.flush", "of type boolean"));
        }
        put(c, this, "flush", flush)?;
        require_operation(c, this, "fsync")?;
    }
    for (operation, method) in [("write", "_write"), ("writev", "_writev")] {
        if let Some(value) = custom_operation(c, this, operation)? {
            if !c.truthy_rooted(value)? {
                let null = c.null();
                put(c, this, method, null)?;
            }
        }
    }
    initialize_position(c, this, options)?;
    put_number(c, this, "bytesWritten", 0.0)?;
    performing_io(c, this, false)?;
    let writable = stream_class(c, "Writable")?;
    c.call_rooted(writable, this, &[options])?;
    let encoding = get(c, options, "encoding")?;
    if c.truthy_rooted(encoding)? {
        call_method(c, this, "setDefaultEncoding", &[encoding])?;
    }
    Ok(c.undefined())
}

/// `writeAll(data, size, pos, cb)`: write until `size` bytes are out.
fn write_all(c: &mut Context<'_>, stream: RootId, data: RootId, position: RootId, callback: RootId, retries: f64) -> Result<(), RootedError> {
    let retries = c.number(retries);
    let done = closure(c, "fsStreamsWriteWritten", "", &[stream, data, position, callback, retries])?;
    let fd = get(c, stream, "fd")?;
    if let Some(write) = custom_operation(c, stream, "write")? {
        let operations = get_slot(c, stream, Slot::Fs)?;
        let zero = c.number(0.0);
        let size = get(c, data, "length")?;
        c.call_rooted(write, operations, &[fd, data, zero, size, position, done])?;
        return Ok(());
    }
    let undefined = c.undefined();
    let result = host_operation(c, super::ops::Op::Write, &[fd, data, undefined, undefined, position])?;
    settle(c, result.map(|written| vec![written, data]), done)
}

/// The write callback `(er, bytesWritten, buffer)`.
pub(crate) fn written(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, data, position, callback, retries] = captures(c)?;
    let (mut error, written) = (arg(c, args, 0), arg(c, args, 1));
    let undefined = c.undefined();
    let mut written = if is_undefined(c, written) { 0.0 } else { c.to_number(written)? };
    if crate::modules::stream_shared_vm::errors::has_code(c, error, "EAGAIN")? {
        error = c.null();
        written = 0.0;
    }
    if truthy_member(c, stream, "destroyed")? || c.truthy_rooted(error)? {
        let error = if c.truthy_rooted(error)? {
            error
        } else {
            crate::modules::stream_shared_vm::errors::create(c, crate::modules::stream_shared_vm::errors::StreamError::Destroyed("write"))?
        };
        c.call_rooted(callback, undefined, &[error])?;
        return Ok(undefined);
    }
    let total = number(c, stream, "bytesWritten")?;
    put_number(c, stream, "bytesWritten", total + written)?;
    let retries = if written > 0.0 { 0.0 } else { c.to_number(retries)? + 1.0 };
    let remaining = number(c, data, "length")? - written;
    if retries > MAX_WRITE_RETRIES {
        let error = c.error_rooted("A system error occurred: write failed")?;
        crate::modules::node_errors_shared_vm::decorate_error(c, error, "ERR_SYSTEM_ERROR")?;
        c.call_rooted(callback, undefined, &[error])?;
    } else if remaining > 0.0 {
        let offset = c.number(written);
        let rest = call_method(c, data, "slice", &[offset])?;
        let position = match c.rooted_value(position).and_then(|value| value.as_number()) {
            Some(position) => c.number(position + written),
            None => position,
        };
        write_all(c, stream, rest, position, callback, retries)?;
    } else {
        c.call_rooted(callback, undefined, &[])?;
    }
    Ok(undefined)
}

/// The callback a stream write finishes with: report, or close a destroyed stream.
fn write_finished(c: &mut Context<'_>, stream: RootId, callback: RootId) -> Outcome {
    closure(c, "fsStreamsWriteDone", "", &[stream, callback])
}

pub(crate) fn write_done(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, callback] = captures(c)?;
    let error = arg(c, args, 0);
    performing_io(c, stream, false)?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])?;
    if truthy_member(c, stream, "destroyed")? {
        emit_io_done(c, stream, error)?;
    }
    Ok(undefined)
}

fn advance(c: &mut Context<'_>, stream: RootId, size: f64) -> Result<(), RootedError> {
    let position = get(c, stream, "pos")?;
    if let Some(position) = c.rooted_value(position).and_then(|value| value.as_number()) {
        put_number(c, stream, "pos", position + size)?;
    }
    Ok(())
}

/// `WriteStream.prototype._write(data, encoding, cb)`.
pub(crate) fn write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (data, callback) = (arg(c, args, 0), arg(c, args, 2));
    performing_io(c, this, true)?;
    let done = write_finished(c, this, callback)?;
    let position = get(c, this, "pos")?;
    write_all(c, this, data, position, done, 0.0)?;
    let size = number(c, data, "length")?;
    advance(c, this, size)?;
    Ok(c.undefined())
}

/// `WriteStream.prototype._writev(data, cb)`: the chunks as one write.
pub(crate) fn writev(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (entries, callback) = (arg(c, args, 0), arg(c, args, 1));
    let mut chunks = Vec::new();
    for entry in c.array_like_values_rooted(entries)? {
        chunks.push(get(c, entry, "chunk")?);
    }
    let list = c.array_rooted(&chunks)?;
    let buffer = crate::modules::buffer_shared_vm::fast_buffer(c)?;
    let joined = call_method(c, buffer, "concat", &[list])?;
    performing_io(c, this, true)?;
    let done = write_finished(c, this, callback)?;
    let position = get(c, this, "pos")?;
    write_all(c, this, joined, position, done, 0.0)?;
    let size = number(c, joined, "length")?;
    advance(c, this, size)?;
    Ok(c.undefined())
}

/// `WriteStream.prototype.close([cb])`: end, then close.
pub(crate) fn write_close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    if c.truthy_rooted(callback)? {
        if truthy_member(c, this, "closed")? {
            next_tick(c, callback, &[])?;
            return Ok(c.undefined());
        }
        crate::modules::stream_shared_vm::on(c, this, "close", callback)?;
    }
    if !truthy_member(c, this, "autoClose")? {
        let destroy = get(c, this, "destroy")?;
        crate::modules::stream_shared_vm::on(c, this, "finish", destroy)?;
    }
    call_method(c, this, "end", &[])?;
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// The classes
// ---------------------------------------------------------------------------------------

fn method(c: &mut Context<'_>, target: RootId, name: &str, operation: &str, enumerable: bool) -> Result<RootId, RootedError> {
    let function = c.host_function_named(crate::host::shared_vm::operation(operation), "")?;
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, function, true, enumerable, true)?;
    Ok(function)
}

/// An accessor as `ObjectDefineProperty` installs it: `autoClose` without `configurable`,
/// `pending` with it.
fn accessor(c: &mut Context<'_>, target: RootId, name: &str, getter: &str, setter: Option<&str>, configurable: bool) -> Result<(), RootedError> {
    let getter = c.host_function_named(crate::host::shared_vm::operation(getter), "get")?;
    let setter = match setter {
        Some(operation) => c.host_function_named(crate::host::shared_vm::operation(operation), "set")?,
        None => c.undefined(),
    };
    let key = c.string_rooted(name);
    let fields = quench_runtime::DescriptorFields {
        get: Some(getter),
        set: Some(setter),
        enumerable: Some(false),
        configurable: Some(configurable),
        ..quench_runtime::DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields)?;
    Ok(())
}

fn class(c: &mut Context<'_>, name: &str, operation: &str, parent: &str) -> Outcome {
    let parent = stream_class(c, parent)?;
    let data = c.array_rooted(&[])?;
    let constructor = c.host_constructor_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
    put(c, data, "0", constructor)?;
    let prototype = get(c, constructor, "prototype")?;
    let parent_prototype = get(c, parent, "prototype")?;
    c.set_prototype_rooted(prototype, parent_prototype)?;
    c.set_prototype_rooted(constructor, parent)?;
    Ok(constructor)
}

fn remember(c: &mut Context<'_>, key: &str, value: RootId) -> Result<(), RootedError> {
    let retained = c.retain(value)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(key.to_owned(), retained);
    Ok(())
}

/// `fs.ReadStream`.
pub(crate) fn read_stream_class(c: &mut Context<'_>) -> Outcome {
    let constructor = class(c, "ReadStream", "fsStreamsRead", "Readable")?;
    let prototype = get(c, constructor, "prototype")?;
    accessor(c, prototype, "autoClose", "fsStreamsReadAutoClose", Some("fsStreamsReadSetAutoClose"), false)?;
    let open = method(c, prototype, "open", "fsStreamDeprecatedOpen", true)?;
    remember(c, "\0fs:openReadFs", open)?;
    method(c, prototype, "_construct", "fsStreamsReadConstruct", true)?;
    method(c, prototype, "_read", "fsStreamsReadRead", true)?;
    method(c, prototype, "_destroy", "fsStreamsDestroy", true)?;
    method(c, prototype, "close", "fsStreamsReadClose", true)?;
    accessor(c, prototype, "pending", "fsStreamPending", None, true)?;
    Ok(constructor)
}

/// `fs.WriteStream`.
pub(crate) fn write_stream_class(c: &mut Context<'_>) -> Outcome {
    let constructor = class(c, "WriteStream", "fsStreamsWrite", "Writable")?;
    let prototype = get(c, constructor, "prototype")?;
    accessor(c, prototype, "autoClose", "fsStreamsWriteAutoClose", Some("fsStreamsWriteSetAutoClose"), false)?;
    let open = method(c, prototype, "open", "fsStreamDeprecatedOpen", true)?;
    remember(c, "\0fs:openWriteFs", open)?;
    method(c, prototype, "_construct", "fsStreamsWriteConstruct", true)?;
    method(c, prototype, "_write", "fsStreamsWriteWrite", true)?;
    method(c, prototype, "_writev", "fsStreamsWriteWritev", true)?;
    method(c, prototype, "_destroy", "fsStreamsDestroy", true)?;
    method(c, prototype, "close", "fsStreamsWriteClose", true)?;
    let end = get(c, prototype, "end")?;
    let key = c.string_rooted("destroySoon");
    c.define_data_property_rooted(prototype, key, end, true, true, true)?;
    accessor(c, prototype, "pending", "fsStreamPending", None, true)?;
    Ok(constructor)
}
