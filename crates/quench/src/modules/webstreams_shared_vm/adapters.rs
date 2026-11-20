//! Adapters between Node streams and web streams (`lib/internal/webstreams/adapters.js`):
//! `Readable.toWeb`/`fromWeb`, `Writable.toWeb`/`fromWeb`, `Duplex.toWeb`/`fromWeb` and the
//! readable/writable pair a compression stream wraps around a zlib stream.

use super::readable::field::{CLOSED, CONTROLLER, PROMISE};
use super::readable::{self, new_request, ReadSteps};
use super::writable::{self, field::READY};
use super::{
    arg, deferred, fail, flag, get, is_nullish, is_undefined, literal, put, put_flag, record, resolved_undefined,
    settle_undefined, slots, state_of, throw, then, Brand, Context, Done, Outcome, WebError,
};
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::stream_shared_vm::{destroy::destroyer, utils};
use quench_runtime::{RootId, RootedError};

members!(Member, "webAdapters", [
    ReadableToWeb,
    ReadableFromWeb,
    WritableToWeb,
    WritableFromWeb,
    DuplexToWeb,
    DuplexFromWeb,
    // newReadableStreamFromStreamReadable
    SourceStart,
    SourcePull,
    SourceCancel,
    SourceData,
    SourceFinished,
    // newWritableStreamFromStreamWritable
    SinkStart,
    SinkWrite,
    SinkAbort,
    SinkClose,
    SinkDrain,
    SinkFinished,
    SinkBackpressureFulfilled,
    SinkBackpressureRejected,
    // newStreamReadableFromReadableStream and the readable side of a duplex
    NodeRead,
    NodeReadResult,
    NodeReadFailed,
    NodeReaderClosed,
    NodeReaderFailed,
    // newStreamWritableFromWritableStream and the writable side of a duplex
    NodeWrite,
    NodeWritev,
    NodeFinal,
    NodeWriteReady,
    NodeWritevReady,
    NodeWriteDone,
    NodeWritevDone,
    NodeWriterClosed,
    NodeWriterFailed,
    NodeDestroy,
    NodeDestroyDone,
    NodeDestroyLater,
    NodeThrowLater,
    Ignore,
    // pipeline's pumpToWeb
    PumpResult,
    PumpSyncValue,
    PumpReady,
    PumpDrained,
    PumpClosed,
    PumpFailed,
    PumpAborted,
    PumpAbortFailed,
]);

/// The adapters' closure state.
mod field {
    pub(super) const NODE: &str = "node";
    pub(super) const OBJECT_MODE: &str = "objectMode";
    pub(super) const CANCELED: &str = "wasCanceled";
    pub(super) const CLEANUP: &str = "cleanup";
    pub(super) const BACKPRESSURE: &str = "backpressurePromise";
    pub(super) const CLOSE: &str = "closed";
    pub(crate) use crate::modules::keys::READER;
    pub(crate) use crate::modules::keys::WRITER;
    pub(super) const READABLE_CLOSED: &str = "readableClosed";
    pub(super) const WRITABLE_CLOSED: &str = "writableClosed";
    pub(super) const DECODE_STRINGS: &str = "decodeStrings";
    pub(super) const CALLBACK: &str = "callback";
    pub(crate) use crate::modules::keys::ERROR;
}
use field::*;

/// The `code`s of zlib failures (`ZLIB_FAILURES`): the negative zlib and brotli results and
/// `Z_NEED_DICT`.
const ZLIB_FAILURES: [&str; 6] = ["Z_NEED_DICT", "Z_ERRNO", "Z_STREAM_ERROR", "Z_DATA_ERROR", "Z_MEM_ERROR", "Z_BUF_ERROR"];
const ZLIB_VERSION_ERROR: &str = "Z_VERSION_ERROR";
const BROTLI_ERROR_PREFIX: &str = "BROTLI_DECODER_ERROR_";

fn stream_module(c: &mut Context<'_>) -> Outcome {
    crate::host::shared_vm::commonjs::stream_module(c)
}

fn class(c: &mut Context<'_>, name: &str) -> Outcome {
    let stream = stream_module(c)?;
    get(c, stream, name)
}

fn call(c: &mut Context<'_>, object: RootId, method: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}

fn truthy_tri(value: utils::Tri) -> bool {
    value == Some(true)
}

/// `handleKnownInternalErrors(cause)`.
fn known_internal_error(c: &mut Context<'_>, cause: RootId) -> Outcome {
    if is_nullish(c, cause) || !c.is_object_rooted(cause)? {
        return Ok(cause);
    }
    let code = get(c, cause, "code")?;
    let Some(code) = c.string_text(code)? else { return Ok(cause) };
    if code == "ERR_STREAM_PREMATURE_CLOSE" {
        return crate::modules::stream_shared_vm::errors::abort_error(c, Some(cause));
    }
    if ZLIB_FAILURES.contains(&code.as_str()) || code == ZLIB_VERSION_ERROR || code.starts_with(BROTLI_ERROR_PREFIX) {
        let global = c.global_root()?;
        let type_error = get(c, global, "TypeError")?;
        let options = literal(c, &["cause"], &[cause])?;
        let undefined = c.undefined();
        let error = c.construct_rooted(type_error, type_error, &[undefined, options])?;
        let code = c.string_rooted(&code);
        put(c, error, "code", code)?;
        return Ok(error);
    }
    Ok(cause)
}

fn on(c: &mut Context<'_>, emitter: RootId, event: &str, listener: RootId) -> Done {
    let event = c.string_rooted(event);
    call(c, emitter, "on", &[event, listener]).map(drop)
}

fn finished(c: &mut Context<'_>, stream: RootId, callback: RootId) -> Outcome {
    let module = stream_module(c)?;
    let finished = get(c, module, "finished")?;
    let undefined = c.undefined();
    c.call_rooted(finished, undefined, &[stream, callback])
}

fn construct_web(c: &mut Context<'_>, brand: Brand, args: &[RootId]) -> Outcome {
    let classes = super::classes(c)?;
    let class = get(c, classes, brand.name())?;
    c.construct_rooted(class, class, args)
}

fn closed_writable(c: &mut Context<'_>) -> Outcome {
    let writable = construct_web(c, Brand::WritableStream, &[])?;
    let stream = slots(c, writable)?;
    writable::close(c, stream)?;
    Ok(writable)
}

fn canceled_readable(c: &mut Context<'_>) -> Outcome {
    let readable = construct_web(c, Brand::ReadableStream, &[])?;
    let stream = slots(c, readable)?;
    let undefined = c.undefined();
    readable::cancel(c, stream, undefined)?;
    Ok(readable)
}

fn methods(c: &mut Context<'_>, state: RootId, members: &[(&str, Member)]) -> Outcome {
    let object = c.object_rooted()?;
    for &(name, member) in members {
        let method = function(c, member, name, &[state])?;
        put(c, object, name, method)?;
    }
    Ok(object)
}

fn high_water_strategy(c: &mut Context<'_>, brand: Brand, high_water_mark: RootId) -> Outcome {
    let init = literal(c, &["highWaterMark"], &[high_water_mark])?;
    construct_web(c, brand, &[init])
}

/// `newReadableStreamFromStreamReadable(streamReadable, options)`.
pub(crate) fn readable_from_node(c: &mut Context<'_>, node: RootId, options: RootId) -> Outcome {
    let state_object = if is_nullish(c, node) { c.undefined() } else { get(c, node, "_readableState")? };
    if !c.is_object_rooted(state_object)? || is_nullish(c, state_object) {
        let error = crate::modules::util_shared_vm::argument_type_error_with_expected(c, node, "streamReadable", "an stream.Readable")?;
        return Err(error);
    }
    stream_module(c)?;
    if truthy_tri(utils::is_destroyed(c, node)?) || !truthy_tri(utils::is_readable(c, node)?) {
        return canceled_readable(c);
    }
    let object_mode = get(c, node, "readableObjectMode")?;
    let object_mode = c.truthy_rooted(object_mode)?;
    let high_water_mark = get(c, node, "readableHighWaterMark")?;
    let given = super::optional(c, options, "strategy")?;
    let strategy = if c.truthy_rooted(given)? {
        given
    } else if object_mode {
        high_water_strategy(c, Brand::CountStrategy, high_water_mark)?
    } else {
        high_water_strategy(c, Brand::ByteLengthStrategy, high_water_mark)?
    };
    let state = record(c, &[NODE], &[node])?;
    put_flag(c, state, OBJECT_MODE, object_mode)?;
    put_flag(c, state, CANCELED, false)?;
    call(c, node, "pause", &[])?;
    let callback = function(c, Member::SourceFinished, "", &[state])?;
    let cleanup = finished(c, node, callback)?;
    put(c, state, CLEANUP, cleanup)?;
    let data = function(c, Member::SourceData, "onData", &[state])?;
    on(c, node, "data", data)?;
    let source = methods(c, state, &[("start", Member::SourceStart), ("pull", Member::SourcePull), ("cancel", Member::SourceCancel)])?;
    construct_web(c, Brand::ReadableStream, &[source, strategy])
}

/// `newWritableStreamFromStreamWritable(streamWritable)`.
pub(crate) fn writable_from_node(c: &mut Context<'_>, node: RootId) -> Outcome {
    let valid = c.truthy_rooted(node)? && {
        let write = get(c, node, "write")?;
        let listen = get(c, node, "on")?;
        c.is_callable_rooted(write)? && c.is_callable_rooted(listen)?
    };
    if !valid {
        let error = crate::modules::util_shared_vm::argument_type_error_with_expected(c, node, "streamWritable", "an stream.Writable")?;
        return Err(error);
    }
    stream_module(c)?;
    if truthy_tri(utils::is_destroyed(c, node)?) || !truthy_tri(utils::is_writable(c, node)?) {
        return closed_writable(c);
    }
    let high_water_mark = get(c, node, "writableHighWaterMark")?;
    let object_mode = get(c, node, "writableObjectMode")?;
    let strategy = if c.truthy_rooted(object_mode)? {
        high_water_strategy(c, Brand::CountStrategy, high_water_mark)?
    } else {
        literal(c, &["highWaterMark"], &[high_water_mark])?
    };
    let state = record(c, &[NODE], &[node])?;
    let callback = function(c, Member::SinkFinished, "", &[state])?;
    let cleanup = finished(c, node, callback)?;
    put(c, state, CLEANUP, cleanup)?;
    let drain = function(c, Member::SinkDrain, "onDrain", &[state])?;
    on(c, node, "drain", drain)?;
    let sink = methods(
        c,
        state,
        &[("start", Member::SinkStart), ("write", Member::SinkWrite), ("abort", Member::SinkAbort), ("close", Member::SinkClose)],
    )?;
    construct_web(c, Brand::WritableStream, &[sink, strategy])
}

/// `newReadableWritablePairFromDuplex(duplex)`: the `(readable, writable)` web streams.
pub(crate) fn pair_from_duplex(c: &mut Context<'_>, duplex: RootId) -> Result<(RootId, RootId), RootedError> {
    let has_states = !is_nullish(c, duplex) && {
        let writable_state = get(c, duplex, "_writableState")?;
        let readable_state = get(c, duplex, "_readableState")?;
        c.is_object_rooted(writable_state)? && !is_nullish(c, writable_state) && c.is_object_rooted(readable_state)? && !is_nullish(c, readable_state)
    };
    if !has_states {
        let error = crate::modules::util_shared_vm::argument_type_error_with_expected(c, duplex, "duplex", "an stream.Duplex")?;
        return Err(error);
    }
    stream_module(c)?;
    if truthy_tri(utils::is_destroyed(c, duplex)?) {
        let writable = closed_writable(c)?;
        let readable = canceled_readable(c)?;
        return Ok((readable, writable));
    }
    let writable = if truthy_tri(utils::is_writable(c, duplex)?) {
        writable_from_node(c, duplex)?
    } else {
        closed_writable(c)?
    };
    let readable = if truthy_tri(utils::is_readable(c, duplex)?) {
        let undefined = c.undefined();
        readable_from_node(c, duplex, undefined)?
    } else {
        canceled_readable(c)?
    };
    Ok((readable, writable))
}

fn validate_boolean(c: &mut Context<'_>, value: RootId, name: &str) -> Done {
    if c.rooted_value(value).and_then(|value| value.as_bool()).is_none() {
        return Err(throw(c, WebError::ArgType(name, "boolean", value)));
    }
    Ok(())
}

/// An option of `options`, or `default` when it is `undefined`.
fn option_or(c: &mut Context<'_>, options: RootId, name: &str, default: bool) -> Outcome {
    let value = get(c, options, name)?;
    Ok(if is_undefined(c, value) { c.boolean(default) } else { value })
}

fn validate_encoding(c: &mut Context<'_>, encoding: RootId) -> Done {
    if is_undefined(c, encoding) {
        return Ok(());
    }
    let buffer = super::intrinsic(c, super::key::BUFFER)?;
    let valid = call(c, buffer, "isEncoding", &[encoding])?;
    if c.truthy_rooted(valid)? {
        return Ok(());
    }
    // Node passes the arguments the other way around.
    let name = c.to_string(encoding)?;
    let label = c.string_rooted("options.encoding");
    Err(throw(c, WebError::ArgValue(&name, label)))
}

/// The options a Node stream wrapping a web stream is constructed with.
fn node_options(c: &mut Context<'_>, state: RootId, options: RootId, names: &'static [&'static str], values: &[RootId], handlers: &[(&str, Member)]) -> Outcome {
    let object = literal(c, names, values)?;
    for name in ["highWaterMark", "signal"] {
        let value = get(c, options, name)?;
        put(c, object, name, value)?;
    }
    for &(name, member) in handlers {
        let handler = function(c, member, name, &[state])?;
        put(c, object, name, handler)?;
    }
    Ok(object)
}

fn options_object(c: &mut Context<'_>, args: &[RootId], index: usize) -> Outcome {
    let options = arg(c, args, index);
    if is_undefined(c, options) {
        return c.object_rooted();
    }
    super::validate_dictionary(c, options, "options", super::Dictionary::Plain)?;
    Ok(options)
}

/// `newStreamReadableFromReadableStream(readableStream, options)`.
fn node_readable_from_web(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let web = arg(c, args, 0);
    let Some(stream) = state_of(c, web, Brand::ReadableStream)? else {
        return Err(throw(c, WebError::ArgType("readableStream", "ReadableStream", web)));
    };
    let options = options_object(c, args, 1)?;
    let encoding = get(c, options, "encoding")?;
    let object_mode = option_or(c, options, "objectMode", false)?;
    validate_encoding(c, encoding)?;
    validate_boolean(c, object_mode, "options.objectMode")?;
    let reader = readable::default_reader(c, stream)?;
    let reader = slots(c, reader)?;
    let state = record(c, &[READER], &[reader])?;
    put_flag(c, state, READABLE_CLOSED, false)?;
    let node_options = node_options(
        c,
        state,
        options,
        &["objectMode", "encoding"], &[object_mode, encoding],
        &[("read", Member::NodeRead), ("destroy", Member::NodeDestroy)],
    )?;
    let readable_class = class(c, "Readable")?;
    let node = c.construct_rooted(readable_class, readable_class, &[node_options])?;
    put(c, state, NODE, node)?;
    watch_reader(c, state, reader)?;
    Ok(node)
}

fn watch_reader(c: &mut Context<'_>, state: RootId, reader: RootId) -> Done {
    let closed = get(c, reader, CLOSED)?;
    let fulfilled = function(c, Member::NodeReaderClosed, "", &[state])?;
    let rejected = function(c, Member::NodeReaderFailed, "", &[state])?;
    then(c, closed, fulfilled, rejected).map(drop)
}

fn watch_writer(c: &mut Context<'_>, state: RootId, writer: RootId) -> Done {
    let closed = get(c, writer, CLOSED)?;
    let fulfilled = function(c, Member::NodeWriterClosed, "", &[state])?;
    let rejected = function(c, Member::NodeWriterFailed, "", &[state])?;
    then(c, closed, fulfilled, rejected).map(drop)
}

/// `newStreamWritableFromWritableStream(writableStream, options)`.
fn node_writable_from_web(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let web = arg(c, args, 0);
    let Some(stream) = state_of(c, web, Brand::WritableStream)? else {
        return Err(throw(c, WebError::ArgType("writableStream", "WritableStream", web)));
    };
    let options = options_object(c, args, 1)?;
    let decode_strings = option_or(c, options, "decodeStrings", true)?;
    let object_mode = option_or(c, options, "objectMode", false)?;
    validate_boolean(c, object_mode, "options.objectMode")?;
    validate_boolean(c, decode_strings, "options.decodeStrings")?;
    let writer = writable::writer(c, stream)?;
    let state = record(c, &[WRITER], &[writer])?;
    put_flag(c, state, WRITABLE_CLOSED, false)?;
    put_flag(c, state, OBJECT_MODE, c.rooted_value(object_mode).and_then(|v| v.as_bool()) == Some(true))?;
    let decode = c.truthy_rooted(decode_strings)?;
    put_flag(c, state, DECODE_STRINGS, decode)?;
    let node_options = node_options(
        c,
        state,
        options,
        &["objectMode", "decodeStrings"], &[object_mode, decode_strings],
        &[("writev", Member::NodeWritev), ("write", Member::NodeWrite), ("destroy", Member::NodeDestroy), ("final", Member::NodeFinal)],
    )?;
    let writable_class = class(c, "Writable")?;
    let node = c.construct_rooted(writable_class, writable_class, &[node_options])?;
    put(c, state, NODE, node)?;
    watch_writer(c, state, writer)?;
    Ok(node)
}

/// `newStreamDuplexFromReadableWritablePair(pair, options)`.
fn duplex_from_pair(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let pair = arg(c, args, 0);
    let pair = if is_undefined(c, pair) { c.object_rooted()? } else { pair };
    super::validate_dictionary(c, pair, "pair", super::Dictionary::Plain)?;
    let readable_web = get(c, pair, "readable")?;
    let writable_web = get(c, pair, "writable")?;
    let Some(readable_stream) = state_of(c, readable_web, Brand::ReadableStream)? else {
        return Err(throw(c, WebError::ArgType("pair.readable", "ReadableStream", readable_web)));
    };
    let Some(writable_stream) = state_of(c, writable_web, Brand::WritableStream)? else {
        return Err(throw(c, WebError::ArgType("pair.writable", "WritableStream", writable_web)));
    };
    let options = options_object(c, args, 1)?;
    let allow_half_open = option_or(c, options, "allowHalfOpen", false)?;
    let object_mode = option_or(c, options, "objectMode", false)?;
    let encoding = get(c, options, "encoding")?;
    let decode_strings = option_or(c, options, "decodeStrings", true)?;
    validate_boolean(c, object_mode, "options.objectMode")?;
    validate_encoding(c, encoding)?;
    let writer = writable::writer(c, writable_stream)?;
    let reader = readable::default_reader(c, readable_stream)?;
    let reader = slots(c, reader)?;
    let state = record(c, &[WRITER, READER], &[writer, reader])?;
    put_flag(c, state, WRITABLE_CLOSED, false)?;
    put_flag(c, state, READABLE_CLOSED, false)?;
    put_flag(c, state, OBJECT_MODE, c.rooted_value(object_mode).and_then(|v| v.as_bool()) == Some(true))?;
    let decode = c.truthy_rooted(decode_strings)?;
    put_flag(c, state, DECODE_STRINGS, decode)?;
    let node_options = node_options(
        c,
        state,
        options,
        &["allowHalfOpen", "objectMode", "encoding", "decodeStrings"], &[allow_half_open, object_mode, encoding, decode_strings],
        &[
            ("writev", Member::NodeWritev),
            ("write", Member::NodeWrite),
            ("final", Member::NodeFinal),
            ("read", Member::NodeRead),
            ("destroy", Member::NodeDestroy),
        ],
    )?;
    let duplex_class = class(c, "Duplex")?;
    let node = c.construct_rooted(duplex_class, duplex_class, &[node_options])?;
    put(c, state, NODE, node)?;
    watch_writer(c, state, writer)?;
    watch_reader(c, state, reader)?;
    Ok(node)
}

/// Calls a Node stream callback; a throw destroys the stream on the next tick (or rethrows
/// there, for `destroy`'s callback).
fn invoke_callback(c: &mut Context<'_>, state: RootId, callback: RootId, error: RootId, rethrow: bool) -> Done {
    let undefined = c.undefined();
    if let Err(thrown) = super::attempt(c.call_rooted(callback, undefined, &[error]))? {
        let task = function(c, if rethrow { Member::NodeThrowLater } else { Member::NodeDestroyLater }, "", &[state, thrown])?;
        crate::modules::shared_event_loop::queue_next_tick(c, task, undefined, &[])?;
    }
    Ok(())
}

/// A string chunk as the writable side converts it (`encoder.encode` or `Buffer.from`).
fn convert_chunk(c: &mut Context<'_>, state: RootId, chunk: RootId, encoding: RootId) -> Outcome {
    let is_string = matches!(c.value_type_rooted(chunk), Some(quench_runtime::ValueType::String));
    if !is_string || !flag(c, state, DECODE_STRINGS)? || flag(c, state, OBJECT_MODE)? {
        return Ok(chunk);
    }
    let encoding_text = if is_undefined(c, encoding) { "utf8".to_owned() } else { c.to_string(encoding)?.to_ascii_lowercase() };
    if matches!(encoding_text.as_str(), "utf8" | "utf-8") {
        let units = c.string_code_units(chunk)?.unwrap_or_default();
        return c.uint8_array_from_rooted(String::from_utf16_lossy(&units).into_bytes());
    }
    let buffer = super::intrinsic(c, super::key::BUFFER)?;
    let bytes = call(c, buffer, "from", &[chunk, encoding])?;
    let parts = super::view_parts(c, bytes)?.ok_or_else(|| RootedError::host("Buffer.from made no view"))?;
    super::uint8_view(c, parts.buffer, parts.offset, parts.length)
}

/// The writer's `write(chunk)` as a promise.
fn writer_write(c: &mut Context<'_>, state: RootId, chunk: RootId) -> Outcome {
    let writer = get(c, state, WRITER)?;
    let stream = get(c, writer, super::readable::field::STREAM)?;
    if is_undefined(c, stream) {
        return super::rejected_with(c, WebError::InvalidState("Writer is not bound to a WritableStream"));
    }
    writable::writer_write(c, writer, chunk)
}

fn writer_ready(c: &mut Context<'_>, state: RootId) -> Outcome {
    let writer = get(c, state, WRITER)?;
    get(c, writer, READY)
}

fn reader_read(c: &mut Context<'_>, state: RootId) -> Outcome {
    let reader = get(c, state, READER)?;
    let stream = get(c, reader, super::readable::field::STREAM)?;
    if is_undefined(c, stream) {
        return super::rejected_with(c, WebError::InvalidState("The reader is not attached to a stream"));
    }
    let promise = deferred(c)?;
    let request = new_request(c, ReadSteps::Default, &[PROMISE, crate::modules::keys::KIND], &[promise])?;
    readable::default_reader_read(c, reader, request)?;
    Ok(promise)
}

fn writer_close(c: &mut Context<'_>, state: RootId) -> Outcome {
    let writer = get(c, state, WRITER)?;
    let stream = get(c, writer, super::readable::field::STREAM)?;
    if is_undefined(c, stream) {
        return super::rejected_with(c, WebError::InvalidState("Writer is not bound to a WritableStream"));
    }
    if writable::close_queued_or_in_flight(c, stream)? {
        return super::rejected_with(c, WebError::InvalidState("Failure to close WritableStream"));
    }
    writable::writer_close(c, writer)
}

fn react(c: &mut Context<'_>, promise: RootId, fulfilled: Member, rejected: Member, captures: &[RootId]) -> Outcome {
    let fulfilled = function(c, fulfilled, "", captures)?;
    let rejected = function(c, rejected, "", captures)?;
    then(c, promise, fulfilled, rejected)
}

/// `destroy(error, callback)` of a Node stream over a reader and/or writer.
fn node_destroy(c: &mut Context<'_>, state: RootId, error: RootId, callback: RootId) -> Done {
    let writer = get(c, state, WRITER)?;
    let reader = get(c, state, READER)?;
    let has_writer = !is_undefined(c, writer);
    let has_reader = !is_undefined(c, reader);
    let writable_closed = !has_writer || flag(c, state, WRITABLE_CLOSED)?;
    let readable_closed = !has_reader || flag(c, state, READABLE_CLOSED)?;
    if writable_closed && readable_closed {
        return invoke_callback(c, state, callback, error, true);
    }
    let mut pending = Vec::new();
    if !writable_closed {
        let writer = get(c, state, WRITER)?;
        let stream = get(c, writer, super::readable::field::STREAM)?;
        pending.push(if has_reader && has_writer {
            // A duplex's `closeWriter()`: `await writer.abort(error)`.
            writable::abort(c, stream, error)?
        } else if is_nullish(c, error) {
            writer_close(c, state)?
        } else {
            writable::abort(c, stream, error)?
        });
    }
    if !readable_closed {
        let reader = get(c, state, READER)?;
        let stream = get(c, reader, super::readable::field::STREAM)?;
        pending.push(if is_undefined(c, stream) {
            super::rejected_with(c, WebError::InvalidState("The reader is not attached to a stream"))?
        } else {
            readable::cancel(c, stream, error)?
        });
    }
    let captures = record(c, &[CALLBACK, ERROR], &[callback, error])?;
    let remaining = c.number(pending.len() as f64);
    put(c, captures, "remaining", remaining)?;
    for promise in pending {
        react(c, promise, Member::NodeDestroyDone, Member::NodeDestroyDone, &[state, captures])?;
    }
    Ok(())
}

pub(crate) fn member(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::ReadableToWeb => {
            let (node, options) = (arg(c, args, 0), arg(c, args, 1));
            readable_from_node(c, node, options)
        }
        Member::WritableToWeb => {
            let node = arg(c, args, 0);
            writable_from_node(c, node)
        }
        Member::DuplexToWeb => {
            let duplex = arg(c, args, 0);
            let (readable, writable) = pair_from_duplex(c, duplex)?;
            literal(c, &["writable", "readable"], &[writable, readable])
        }
        Member::ReadableFromWeb => node_readable_from_web(c, args),
        Member::WritableFromWeb => node_writable_from_web(c, args),
        Member::DuplexFromWeb => duplex_from_pair(c, args),
        Member::SourceStart | Member::SinkStart => {
            let [state] = captured(c)?;
            let controller = arg(c, args, 0);
            put(c, state, CONTROLLER, controller)?;
            Ok(c.undefined())
        }
        Member::SourcePull => {
            let [state] = captured(c)?;
            let node = get(c, state, NODE)?;
            call(c, node, "resume", &[])?;
            Ok(c.undefined())
        }
        Member::SourceCancel => {
            let [state] = captured(c)?;
            put_flag(c, state, CANCELED, true)?;
            let node = get(c, state, NODE)?;
            let reason = arg(c, args, 0);
            destroyer(c, node, reason)?;
            Ok(c.undefined())
        }
        Member::SourceData => {
            let [state] = captured(c)?;
            let mut chunk = arg(c, args, 0);
            let buffer = super::intrinsic(c, super::key::BUFFER)?;
            let is_buffer = call(c, buffer, "isBuffer", &[chunk])?;
            if c.truthy_rooted(is_buffer)? && !flag(c, state, OBJECT_MODE)? {
                let bytes = c.view_bytes_rooted(chunk).unwrap_or_default();
                chunk = c.uint8_array_from_rooted(bytes)?;
            }
            let controller = get(c, state, CONTROLLER)?;
            call(c, controller, "enqueue", &[chunk])?;
            let desired = get(c, controller, "desiredSize")?;
            if c.to_number(desired)? <= 0.0 {
                let node = get(c, state, NODE)?;
                call(c, node, "pause", &[])?;
            }
            Ok(c.undefined())
        }
        Member::SourceFinished => {
            let [state] = captured(c)?;
            let error = arg(c, args, 0);
            let error = known_internal_error(c, error)?;
            let cleanup = get(c, state, CLEANUP)?;
            let undefined = c.undefined();
            c.call_rooted(cleanup, undefined, &[])?;
            let node = get(c, state, NODE)?;
            let ignore = function(c, Member::Ignore, "", &[])?;
            on(c, node, "error", ignore)?;
            let controller = get(c, state, CONTROLLER)?;
            if c.truthy_rooted(error)? {
                return call(c, controller, "error", &[error]);
            }
            if flag(c, state, CANCELED)? {
                return Ok(c.undefined());
            }
            call(c, controller, "close", &[])
        }
        Member::SinkWrite => {
            let [state] = captured(c)?;
            let chunk = arg(c, args, 0);
            let node = get(c, state, NODE)?;
            let need_drain = get(c, node, "writableNeedDrain")?;
            let accepted = c.truthy_rooted(need_drain)? || {
                let written = call(c, node, "write", &[chunk])?;
                !c.truthy_rooted(written)?
            };
            if accepted {
                let promise = deferred(c)?;
                put(c, state, BACKPRESSURE, promise)?;
                return react(c, promise, Member::SinkBackpressureFulfilled, Member::SinkBackpressureRejected, &[state]);
            }
            Ok(c.undefined())
        }
        Member::SinkBackpressureFulfilled | Member::SinkBackpressureRejected => {
            let [state] = captured(c)?;
            let undefined = c.undefined();
            put(c, state, BACKPRESSURE, undefined)?;
            if member == Member::SinkBackpressureRejected {
                let error = arg(c, args, 0);
                return Err(c.throw(error));
            }
            Ok(undefined)
        }
        Member::SinkAbort => {
            let [state] = captured(c)?;
            let node = get(c, state, NODE)?;
            let reason = arg(c, args, 0);
            destroyer(c, node, reason)?;
            Ok(c.undefined())
        }
        Member::SinkClose => {
            let [state] = captured(c)?;
            let node = get(c, state, NODE)?;
            let closed = get(c, state, CLOSE)?;
            if is_undefined(c, closed) && !truthy_tri(utils::is_writable_ended(c, node)?) {
                let promise = deferred(c)?;
                put(c, state, CLOSE, promise)?;
                call(c, node, "end", &[])?;
                return Ok(promise);
            }
            let undefined = c.undefined();
            put(c, state, CONTROLLER, undefined)?;
            resolved_undefined(c)
        }
        Member::SinkDrain => {
            let [state] = captured(c)?;
            let promise = get(c, state, BACKPRESSURE)?;
            if !is_undefined(c, promise) {
                settle_undefined(c, promise)?;
            }
            Ok(c.undefined())
        }
        Member::SinkFinished => {
            let [state] = captured(c)?;
            let error = arg(c, args, 0);
            let error = known_internal_error(c, error)?;
            let cleanup = get(c, state, CLEANUP)?;
            let undefined = c.undefined();
            c.call_rooted(cleanup, undefined, &[])?;
            let node = get(c, state, NODE)?;
            let ignore = function(c, Member::Ignore, "", &[])?;
            on(c, node, "error", ignore)?;
            let backpressure = get(c, state, BACKPRESSURE)?;
            let closed = get(c, state, CLOSE)?;
            let controller = get(c, state, CONTROLLER)?;
            if !is_nullish(c, error) {
                if !is_undefined(c, backpressure) {
                    fail(c, backpressure, error)?;
                }
                if !is_undefined(c, closed) {
                    fail(c, closed, error)?;
                    put(c, state, CLOSE, undefined)?;
                }
                call(c, controller, "error", &[error])?;
                put(c, state, CONTROLLER, undefined)?;
                return Ok(undefined);
            }
            if !is_undefined(c, closed) {
                settle_undefined(c, closed)?;
                put(c, state, CLOSE, undefined)?;
                return Ok(undefined);
            }
            let abort = crate::modules::stream_shared_vm::errors::abort_error(c, None)?;
            call(c, controller, "error", &[abort])?;
            put(c, state, CONTROLLER, undefined)?;
            Ok(undefined)
        }
        Member::NodeRead => {
            let [state] = captured(c)?;
            let read = reader_read(c, state)?;
            react(c, read, Member::NodeReadResult, Member::NodeReadFailed, &[state])?;
            Ok(c.undefined())
        }
        Member::NodeReadResult => {
            let [state] = captured(c)?;
            let result = arg(c, args, 0);
            let node = get(c, state, NODE)?;
            let done = get(c, result, "done")?;
            let value = if c.truthy_rooted(done)? { c.null() } else { get(c, result, "value")? };
            call(c, node, "push", &[value])?;
            Ok(c.undefined())
        }
        Member::NodeReadFailed => {
            let [state] = captured(c)?;
            let error = arg(c, args, 0);
            let node = get(c, state, NODE)?;
            destroyer(c, node, error)?;
            Ok(c.undefined())
        }
        Member::NodeReaderClosed => {
            let [state] = captured(c)?;
            put_flag(c, state, READABLE_CLOSED, true)?;
            Ok(c.undefined())
        }
        Member::NodeReaderFailed | Member::NodeWriterFailed => {
            let [state] = captured(c)?;
            put_flag(c, state, READABLE_CLOSED, true)?;
            put_flag(c, state, WRITABLE_CLOSED, true)?;
            let error = arg(c, args, 0);
            let node = get(c, state, NODE)?;
            destroyer(c, node, error)?;
            Ok(c.undefined())
        }
        Member::NodeWriterClosed => {
            let [state] = captured(c)?;
            put_flag(c, state, WRITABLE_CLOSED, true)?;
            let node = get(c, state, NODE)?;
            if !truthy_tri(utils::is_writable_ended(c, node)?) {
                let error = crate::modules::stream_shared_vm::errors::create(
                    c,
                    crate::modules::stream_shared_vm::errors::StreamError::PrematureClose,
                )?;
                destroyer(c, node, error)?;
            }
            Ok(c.undefined())
        }
        Member::NodeWrite => {
            let [state] = captured(c)?;
            let (chunk, encoding, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
            let chunk = convert_chunk(c, state, chunk, encoding)?;
            let captures = record(c, &["chunk", CALLBACK], &[chunk, callback])?;
            let ready = writer_ready(c, state)?;
            react(c, ready, Member::NodeWriteReady, Member::NodeWriteDone, &[state, captures])?;
            Ok(c.undefined())
        }
        Member::NodeWriteReady => {
            let [state, captures] = captured(c)?;
            let chunk = get(c, captures, "chunk")?;
            let written = writer_write(c, state, chunk)?;
            react(c, written, Member::NodeWriteDone, Member::NodeWriteDone, &[state, captures])
        }
        Member::NodeWriteDone => {
            let [state, captures] = captured(c)?;
            let callback = get(c, captures, CALLBACK)?;
            let error = arg(c, args, 0);
            let undefined = c.undefined();
            if let Err(thrown) = super::attempt(c.call_rooted(callback, undefined, &[error]))? {
                let node = get(c, state, NODE)?;
                destroyer(c, node, thrown)?;
            }
            Ok(c.undefined())
        }
        Member::NodeWritev => {
            let [state] = captured(c)?;
            let (chunks, callback) = (arg(c, args, 0), arg(c, args, 1));
            let captures = record(c, &["chunks", CALLBACK], &[chunks, callback])?;
            let ready = writer_ready(c, state)?;
            react(c, ready, Member::NodeWritevReady, Member::NodeWritevDone, &[state, captures])?;
            Ok(c.undefined())
        }
        Member::NodeWritevReady => {
            let [state, captures] = captured(c)?;
            let chunks = get(c, captures, "chunks")?;
            let mut writes = Vec::new();
            for entry in c.array_like_values_rooted(chunks)? {
                let chunk = get(c, entry, "chunk")?;
                writes.push(writer_write(c, state, chunk)?);
            }
            let all = super::pipe::all(c, &writes)?;
            react(c, all, Member::NodeWritevDone, Member::NodeWritevDone, &[state, captures])
        }
        Member::NodeWritevDone => {
            let [state, captures] = captured(c)?;
            let callback = get(c, captures, CALLBACK)?;
            let error = arg(c, args, 0);
            let error = if c.is_array_rooted(error)? || is_undefined(c, error) { c.undefined() } else { error };
            invoke_callback(c, state, callback, error, false)?;
            Ok(c.undefined())
        }
        Member::NodeFinal => {
            let [state] = captured(c)?;
            let callback = arg(c, args, 0);
            if !flag(c, state, WRITABLE_CLOSED)? {
                let closed = writer_close(c, state)?;
                let captures = record(c, &[CALLBACK], &[callback])?;
                let fulfilled = function(c, Member::NodeWritevDone, "", &[state, captures])?;
                then(c, closed, fulfilled, fulfilled)?;
            }
            Ok(c.undefined())
        }
        Member::NodeDestroy => {
            let [state] = captured(c)?;
            let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
            node_destroy(c, state, error, callback)?;
            Ok(c.undefined())
        }
        Member::NodeDestroyDone => {
            let [state, captures] = captured(c)?;
            let remaining = super::num(c, captures, "remaining")? - 1.0;
            super::put_num(c, captures, "remaining", remaining)?;
            if remaining <= 0.0 {
                let callback = get(c, captures, CALLBACK)?;
                let error = get(c, captures, ERROR)?;
                invoke_callback(c, state, callback, error, true)?;
            }
            Ok(c.undefined())
        }
        Member::NodeDestroyLater => {
            let [state, error] = captured(c)?;
            let node = get(c, state, NODE)?;
            destroyer(c, node, error)?;
            Ok(c.undefined())
        }
        Member::NodeThrowLater => {
            let [_, error] = captured(c)?;
            Err(c.throw(error))
        }
        Member::Ignore => Ok(c.undefined()),
        Member::PumpResult => {
            let [state] = captured(c)?;
            let result = arg(c, args, 0);
            let done = get(c, result, "done")?;
            if c.truthy_rooted(done)? {
                pump_done(c, state)?;
            } else {
                let value = get(c, result, "value")?;
                pump_chunk(c, state, value)?;
            }
            Ok(c.undefined())
        }
        Member::PumpSyncValue => {
            let [state, done] = captured(c)?;
            if c.truthy_rooted(done)? {
                pump_done(c, state)?;
            } else {
                let value = arg(c, args, 0);
                pump_chunk(c, state, value)?;
            }
            Ok(c.undefined())
        }
        Member::PumpReady => {
            let [state, chunk] = captured(c)?;
            let writer = get(c, state, pump::WRITER)?;
            let written = writable::writer_write(c, writer, chunk)?;
            super::mark_handled(c, written)?;
            if let Err(error) = super::attempt(pump_next(c, state))? {
                pump_abort(c, state, error)?;
            }
            Ok(c.undefined())
        }
        Member::PumpDrained => {
            let [state] = captured(c)?;
            if flag(c, state, pump::END)? {
                let writer = get(c, state, pump::WRITER)?;
                let closed = writable::writer_close(c, writer)?;
                react(c, closed, Member::PumpClosed, Member::PumpFailed, &[state])?;
            } else {
                pump_finish(c, state, None)?;
            }
            Ok(c.undefined())
        }
        Member::PumpClosed => {
            let [state] = captured(c)?;
            pump_finish(c, state, None)?;
            Ok(c.undefined())
        }
        Member::PumpFailed => {
            let [state] = captured(c)?;
            let error = arg(c, args, 0);
            pump_abort(c, state, error)?;
            Ok(c.undefined())
        }
        Member::PumpAborted => {
            let [state, error] = captured(c)?;
            pump_finish(c, state, Some(error))?;
            Ok(c.undefined())
        }
        Member::PumpAbortFailed => {
            let [state] = captured(c)?;
            let error = arg(c, args, 0);
            pump_finish(c, state, Some(error))?;
            Ok(c.undefined())
        }
    }
}

/// A pump's fields (pipeline's `pumpToWeb`).
mod pump {
    pub(crate) use crate::modules::keys::ITERATOR;
    pub(crate) use crate::modules::keys::SYNC;
    pub(crate) use crate::modules::keys::WRITER;
    pub(crate) use crate::modules::keys::STREAM;
    pub(super) const FINISH: &str = "finish";
    pub(super) const END: &str = "end";
}

/// pipeline's `pumpToWeb(readable, writable, finish, { end })`: `for await` over
/// `readable` into the web `writable` (or a TransformStream's writable side) through a
/// writer, honoring its backpressure, then closes it when `end`; `finish` is called with
/// the failure, if any.
pub(crate) fn pump_to_web(c: &mut Context<'_>, readable: RootId, writable: RootId, finish: RootId, end: bool) -> Result<(), RootedError> {
    let writable = if let Some(transform) = state_of(c, writable, Brand::TransformStream)? {
        super::transform::side(c, transform, false)?
    } else {
        writable
    };
    let stream = slots(c, writable)?;
    let writer = writable::writer(c, stream)?;
    let end = c.boolean(end);
    let state = record(c, &[pump::WRITER, pump::STREAM, pump::FINISH, pump::END], &[writer, stream, finish, end])?;
    if let Err(error) = super::attempt(pump_open(c, state, readable))? {
        pump_abort(c, state, error)?;
    }
    Ok(())
}

/// `for await (const chunk of readable)`'s iterator, then its first step.
fn pump_open(c: &mut Context<'_>, state: RootId, readable: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let async_key = get(c, symbol, "asyncIterator")?;
    let method = c.get_property_rooted(readable, async_key)?;
    let (method, sync) = if is_nullish(c, method) {
        let sync_key = get(c, symbol, "iterator")?;
        (c.get_property_rooted(readable, sync_key)?, true)
    } else {
        (method, false)
    };
    let iterator = c.call_rooted(method, readable, &[])?;
    put(c, state, pump::ITERATOR, iterator)?;
    put_flag(c, state, pump::SYNC, sync)?;
    pump_next(c, state)
}

fn pump_next(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let iterator = get(c, state, pump::ITERATOR)?;
    let result = call(c, iterator, "next", &[])?;
    if flag(c, state, pump::SYNC)? {
        let done = get(c, result, "done")?;
        let value = get(c, result, "value")?;
        let value = super::resolved(c, value)?;
        react(c, value, Member::PumpSyncValue, Member::PumpFailed, &[state, done])?;
        return Ok(());
    }
    let result = super::resolved(c, result)?;
    react(c, result, Member::PumpResult, Member::PumpFailed, &[state])?;
    Ok(())
}

/// One chunk: `await writer.ready; writer.write(chunk).catch(() => {})`.
fn pump_chunk(c: &mut Context<'_>, state: RootId, chunk: RootId) -> Result<(), RootedError> {
    let writer = get(c, state, pump::WRITER)?;
    let ready = get(c, writer, READY)?;
    react(c, ready, Member::PumpReady, Member::PumpFailed, &[state, chunk])?;
    Ok(())
}

/// The loop ended: `await writer.ready; if (end) await writer.close(); finish()`.
fn pump_done(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let writer = get(c, state, pump::WRITER)?;
    let ready = get(c, writer, READY)?;
    react(c, ready, Member::PumpDrained, Member::PumpFailed, &[state])?;
    Ok(())
}

fn pump_finish(c: &mut Context<'_>, state: RootId, error: Option<RootId>) -> Result<(), RootedError> {
    let finish = get(c, state, pump::FINISH)?;
    let undefined = c.undefined();
    let args: Vec<RootId> = error.into_iter().collect();
    c.call_rooted(finish, undefined, &args).map(drop)
}

/// The catch: `await writer.abort(err); finish(err)`, or `finish` with the abort's failure.
fn pump_abort(c: &mut Context<'_>, state: RootId, error: RootId) -> Result<(), RootedError> {
    let stream = get(c, state, pump::STREAM)?;
    let aborted = writable::abort(c, stream, error)?;
    react(c, aborted, Member::PumpAborted, Member::PumpAbortFailed, &[state, error])?;
    Ok(())
}

/// Installs `toWeb` and `fromWeb` on the stream classes.
pub(crate) fn install(c: &mut Context<'_>, readable: RootId, writable: RootId, duplex: RootId) -> Done {
    for (class, to_web, from_web) in [
        (readable, Member::ReadableToWeb, Member::ReadableFromWeb),
        (writable, Member::WritableToWeb, Member::WritableFromWeb),
        (duplex, Member::DuplexToWeb, Member::DuplexFromWeb),
    ] {
        for (name, member, length) in [("toWeb", to_web, 2), ("fromWeb", from_web, 2)] {
            let length = if member == Member::WritableToWeb || member == Member::DuplexToWeb { 1 } else { length };
            let method = function(c, member, name, &[])?;
            super::set_length(c, method, length)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(class, key, method, true, true, true)?;
        }
    }
    Ok(())
}
