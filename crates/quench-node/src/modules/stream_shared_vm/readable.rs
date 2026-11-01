//! `Readable`: buffering, flowing and paused modes, piping, and the readable accessors.

use super::destroy::{self, error_or_destroy};
use super::errors::{self, StreamError};
use super::state::{self, bits, change, readable::*};
use super::{
    arg, call_method, captures, closure, define_methods, emit, get, get_slot, intrinsics, is_null,
    is_nullish, is_string, is_undefined, listener_count, next_tick, number, on, once, put, put_bool, put_number,
    remove_listener, tick, Class, Context, Key, Outcome, Slot, Tick,
};
use quench_runtime::{BuiltinBrand, RootId, RootedError};

/// The largest `read(n)` a stream raises its high water mark to (1 GiB).
const MAX_HIGH_WATER_MARK: f64 = (1u64 << 30) as f64;
/// Consumed entries a buffer keeps before compacting its front.
const BUFFER_COMPACTION_INDEX: f64 = 1024.0;

/// The events a new stream's listener store names (Node's shape hint).
pub(crate) const READABLE_EVENTS: &[&str] = &["close", "error", "data", "end", "readable"];

pub(crate) fn state_of(c: &mut Context<'_>, stream: RootId) -> Outcome {
    get(c, stream, "_readableState")
}

/// `this._events ??= { ...events: undefined }`.
pub(crate) fn shape_events(c: &mut Context<'_>, stream: RootId, events: &[&str]) -> Result<(), RootedError> {
    let current = get(c, stream, "_events")?;
    if !is_nullish(c, current) {
        return Ok(());
    }
    let store = c.object_rooted()?;
    let undefined = c.undefined();
    for event in events {
        put(c, store, event, undefined)?;
    }
    put(c, stream, "_events", store)
}

/// Assigns each function-valued option to the stream member it implements.
pub(crate) fn adopt_options(c: &mut Context<'_>, stream: RootId, options: RootId, members: &[(&str, &str)]) -> Result<(), RootedError> {
    for &(option, member) in members {
        let value = get(c, options, option)?;
        if c.is_callable_rooted(value)? {
            put(c, stream, member, value)?;
        }
    }
    Ok(())
}

/// Whether the constructor was reached without `new` on a receiver that is no instance of
/// `class`: Node then constructs one.
pub(crate) fn needs_new(c: &mut Context<'_>, this: RootId, class: Class) -> Result<Option<RootId>, RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(None);
    }
    let constructor = intrinsics(c)?.class(class);
    if c.is_object_rooted(this)? && c.instance_of_rooted(this, constructor)? {
        return Ok(None);
    }
    Ok(Some(constructor))
}

/// `new Readable(options)`.
pub(crate) fn constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    if let Some(class) = needs_new(c, this, Class::Readable)? {
        return c.construct_rooted(class, class, &[options]);
    }
    shape_events(c, this, READABLE_EVENTS)?;
    let state = state::create(c, Class::ReadableState, options, this, false)?;
    put(c, this, "_readableState", state)?;
    if c.truthy_rooted(options)? {
        adopt_options(c, this, options, &[("read", "_read"), ("destroy", "_destroy"), ("construct", "_construct")])?;
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

/// `ReadableState.prototype[kOnConstructed](stream)`.
pub(crate) fn on_constructed(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    if state::has(c, this, NEED_READABLE)? {
        maybe_read_more(c, stream, this)?;
    }
    Ok(c.undefined())
}

pub(crate) fn default_destroy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])?;
    Ok(undefined)
}

pub(crate) fn push(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    add(c, this, args, false)
}

pub(crate) fn unshift(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    add(c, this, args, true)
}

fn add(c: &mut Context<'_>, stream: RootId, args: &[RootId], front: bool) -> Outcome {
    let (chunk, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let state = state_of(c, stream)?;
    let more = if state::has(c, state, state::OBJECT_MODE)? {
        add_object(c, stream, state, chunk, encoding, front)?
    } else {
        add_bytes(c, stream, state, chunk, encoding, front)?
    };
    Ok(c.boolean(more))
}

/// `chunk.length`.
pub(crate) fn length_of(c: &mut Context<'_>, chunk: RootId) -> Result<f64, RootedError> {
    number(c, chunk, "length")
}

pub(crate) fn is_buffer(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    let buffer = intrinsics(c)?.class(Class::Buffer);
    c.instance_of_rooted(value, buffer)
}

pub(crate) fn is_view(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)
}

/// `Stream._uint8ArrayToBuffer(chunk)`: a Buffer over the view's bytes.
pub(crate) fn view_to_buffer(c: &mut Context<'_>, view: RootId) -> Outcome {
    let buffer = get(c, view, "buffer")?;
    let offset = get(c, view, "byteOffset")?;
    let length = get(c, view, "byteLength")?;
    let class = intrinsics(c)?.class(Class::Buffer);
    c.construct_rooted(class, class, &[buffer, offset, length])
}

/// A Buffer over `length` bytes of `chunk` from `start`.
fn buffer_window(c: &mut Context<'_>, chunk: RootId, start: f64, length: f64) -> Outcome {
    let buffer = get(c, chunk, "buffer")?;
    let offset = number(c, chunk, "byteOffset")?;
    let offset = c.number(offset + start);
    let length = c.number(length);
    let class = intrinsics(c)?.class(Class::Buffer);
    c.construct_rooted(class, class, &[buffer, offset, length])
}

/// `Buffer.from(string, encoding)`.
pub(crate) fn encode(c: &mut Context<'_>, string: RootId, encoding: RootId) -> Outcome {
    let bytes = super::super::buffer_shared_vm::encode_bytes(c, string, Some(encoding))?;
    super::super::buffer_shared_vm::from_vec(c, bytes)
}

/// `Buffer.from(string, from).toString(to)`.
fn reencode(c: &mut Context<'_>, string: RootId, from: RootId, to: RootId) -> Outcome {
    let bytes = super::super::buffer_shared_vm::encode_bytes(c, string, Some(from))?;
    let name = c.to_string(to)?;
    let canonical = super::super::buffer_codec::canonical_encoding(&name).unwrap_or("utf8");
    let units = super::super::buffer_codec::decode_units(&bytes, canonical);
    Ok(c.string_units_rooted(&units))
}

fn default_encoding(c: &mut Context<'_>, encoding: RootId, state: RootId) -> Outcome {
    if c.truthy_rooted(encoding)? { Ok(encoding) } else { get(c, state, "defaultEncoding") }
}

fn invalid_chunk(c: &mut Context<'_>, stream: RootId, chunk: RootId) -> Result<bool, RootedError> {
    let error = errors::invalid_argument_type(c, chunk, "chunk", errors::CHUNK_TYPES)?;
    error_or_destroy(c, stream, error, false)?;
    Ok(false)
}

fn end_chunk(c: &mut Context<'_>, stream: RootId, state: RootId) -> Result<bool, RootedError> {
    change(c, state, 0, READING)?;
    on_eof_chunk(c, stream, state)?;
    Ok(false)
}

/// The byte-mode `push`/`unshift` (`readableAddChunk{Push,Unshift}ByteMode`).
fn add_bytes(c: &mut Context<'_>, stream: RootId, state: RootId, chunk: RootId, encoding: RootId, front: bool) -> Result<bool, RootedError> {
    if is_null(c, chunk) {
        return end_chunk(c, stream, state);
    }
    let mut chunk = chunk;
    let mut encoding = encoding;
    if is_string(c, chunk) {
        encoding = default_encoding(c, encoding, state)?;
        let current = get(c, state, "encoding")?;
        if !c.strict_equal_rooted(current, encoding)? {
            if front && c.truthy_rooted(current)? {
                chunk = reencode(c, chunk, encoding, current)?;
            } else {
                chunk = encode(c, chunk, encoding)?;
                encoding = c.string_rooted("");
            }
        }
    } else if !front && is_buffer(c, chunk)? {
        encoding = c.string_rooted("");
    } else if is_view(c, chunk)? {
        chunk = view_to_buffer(c, chunk)?;
        encoding = c.string_rooted("");
    } else if !is_undefined(c, chunk) && !is_buffer(c, chunk)? {
        return invalid_chunk(c, stream, chunk);
    }
    let empty = !c.truthy_rooted(chunk)? || length_of(c, chunk)? <= 0.0;
    if front {
        if empty {
            return can_push_more(c, state);
        }
        return add_front(c, stream, state, chunk);
    }
    if empty {
        change(c, state, 0, READING)?;
        maybe_read_more(c, stream, state)?;
        return can_push_more(c, state);
    }
    let field = bits(c, state)?;
    if field & ENDED != 0 {
        let error = errors::create(c, StreamError::PushAfterEof)?;
        error_or_destroy(c, stream, error, false)?;
        return Ok(false);
    }
    if field & (state::DESTROYED | state::ERRORED) != 0 {
        return Ok(false);
    }
    let field = change(c, state, 0, READING)?;
    if field & DECODER != 0 && !c.truthy_rooted(encoding)? {
        let decoder = get_slot(c, state, Slot::DecoderValue)?;
        chunk = call_method(c, decoder, "write", &[chunk])?;
        if length_of(c, chunk)? == 0.0 {
            maybe_read_more(c, stream, state)?;
            return can_push_more(c, state);
        }
    }
    add_chunk(c, stream, state, chunk, false)?;
    can_push_more(c, state)
}

/// The object-mode `push`/`unshift`.
fn add_object(c: &mut Context<'_>, stream: RootId, state: RootId, chunk: RootId, encoding: RootId, front: bool) -> Result<bool, RootedError> {
    if is_null(c, chunk) {
        return end_chunk(c, stream, state);
    }
    if front {
        return add_front(c, stream, state, chunk);
    }
    let field = bits(c, state)?;
    if field & ENDED != 0 {
        let error = errors::create(c, StreamError::PushAfterEof)?;
        error_or_destroy(c, stream, error, false)?;
        return Ok(false);
    }
    if field & (state::DESTROYED | state::ERRORED) != 0 {
        return Ok(false);
    }
    let field = change(c, state, 0, READING)?;
    let mut chunk = chunk;
    if field & DECODER != 0 && !c.truthy_rooted(encoding)? {
        let decoder = get_slot(c, state, Slot::DecoderValue)?;
        chunk = call_method(c, decoder, "write", &[chunk])?;
    }
    add_chunk(c, stream, state, chunk, false)?;
    can_push_more(c, state)
}

/// `readableAddChunkUnshiftValue`.
fn add_front(c: &mut Context<'_>, stream: RootId, state: RootId, chunk: RootId) -> Result<bool, RootedError> {
    let field = bits(c, state)?;
    if field & END_EMITTED != 0 {
        let error = errors::create(c, StreamError::UnshiftAfterEndEvent)?;
        error_or_destroy(c, stream, error, false)?;
    } else if field & (state::DESTROYED | state::ERRORED) != 0 {
        return Ok(false);
    } else {
        add_chunk(c, stream, state, chunk, true)?;
    }
    can_push_more(c, state)
}

fn can_push_more(c: &mut Context<'_>, state: RootId) -> Result<bool, RootedError> {
    if state::has(c, state, ENDED)? {
        return Ok(false);
    }
    let length = number(c, state, "length")?;
    let mark = number(c, state, "highWaterMark")?;
    Ok(length < mark || length == 0.0)
}

/// `state.awaitDrainWriters` emptied: the set cleared, or the single writer forgotten.
fn clear_await_drain(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    if state::has(c, state, MULTI_AWAIT_DRAIN)? {
        let writers = get(c, state, "awaitDrainWriters")?;
        call_method(c, writers, "clear", &[])?;
    } else {
        let null = c.null();
        put(c, state, "awaitDrainWriters", null)?;
    }
    Ok(())
}

/// `addChunk(stream, state, chunk, addToFront)`.
fn add_chunk(c: &mut Context<'_>, stream: RootId, state: RootId, chunk: RootId, front: bool) -> Result<(), RootedError> {
    let field = bits(c, state)?;
    let direct = field & (FLOWING | SYNC | DATA_LISTENING) == FLOWING | DATA_LISTENING && number(c, state, "length")? == 0.0;
    if direct {
        clear_await_drain(c, state)?;
        change(c, state, DATA_EMITTED, 0)?;
        emit(c, stream, "data", &[chunk])?;
    } else {
        let size = state::chunk_size(c, state, chunk)?;
        let length = number(c, state, "length")?;
        put_number(c, state, "length", length + size)?;
        let buffer = get(c, state, "buffer")?;
        if front {
            let index = number(c, state, "bufferIndex")?;
            if index > 0.0 {
                put_number(c, state, "bufferIndex", index - 1.0)?;
                put(c, buffer, &((index - 1.0) as u64).to_string(), chunk)?;
            } else {
                call_method(c, buffer, "unshift", &[chunk])?;
            }
        } else {
            call_method(c, buffer, "push", &[chunk])?;
        }
        if state::has(c, state, NEED_READABLE)? {
            emit_readable(c, stream)?;
        }
    }
    maybe_read_more(c, stream, state)
}

pub(crate) fn is_paused(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state_of(c, this)?;
    let field = bits(c, state)?;
    Ok(c.boolean(field & PAUSED != 0 || field & (HAS_FLOWING | FLOWING) == HAS_FLOWING))
}

pub(crate) fn set_encoding(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    let state = state_of(c, this)?;
    let decoder = state::string_decoder(c, encoding)?;
    put(c, state, "decoder", decoder)?;
    let decoder = get(c, state, "decoder")?;
    let name = get(c, decoder, "encoding")?;
    put(c, state, "encoding", name)?;
    let buffer = get(c, state, "buffer")?;
    let index = get(c, state, "bufferIndex")?;
    let pending = call_method(c, buffer, "slice", &[index])?;
    let mut content = Vec::new();
    for data in c.array_like_values_rooted(pending)? {
        let text = call_method(c, decoder, "write", &[data])?;
        content.extend(c.to_string_code_units(text)?);
    }
    put_number(c, buffer, "length", 0.0)?;
    put_number(c, state, "bufferIndex", 0.0)?;
    if !content.is_empty() {
        let text = c.string_units_rooted(&content);
        call_method(c, buffer, "push", &[text])?;
    }
    put_number(c, state, "length", content.len() as f64)?;
    Ok(this)
}

/// `computeNewHighWaterMark(n)`: the next power of two, at most 1 GiB.
fn new_high_water_mark(c: &mut Context<'_>, n: f64) -> Result<f64, RootedError> {
    if n > MAX_HIGH_WATER_MARK {
        let value = c.number(n);
        let received = super::super::util_shared_vm::range_error_received(c, value)?;
        let error = c.range_error_rooted(&format!("The value of \"size\" is out of range. It must be <= 1GiB. Received {received}"))?;
        super::super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_OUT_OF_RANGE")?;
        return Err(c.throw(error));
    }
    Ok(f64::from((n as u32).next_power_of_two()))
}

/// `howMuchToRead(n, state)`.
fn how_much_to_read(c: &mut Context<'_>, n: f64, state: RootId) -> Result<f64, RootedError> {
    let field = bits(c, state)?;
    let length = number(c, state, "length")?;
    if n <= 0.0 || (length == 0.0 && field & ENDED != 0) {
        return Ok(0.0);
    }
    if field & state::OBJECT_MODE != 0 {
        return Ok(1.0);
    }
    if n.is_nan() {
        if field & FLOWING != 0 && length != 0.0 {
            let buffer = get(c, state, "buffer")?;
            let index = number(c, state, "bufferIndex")?;
            let first = get(c, buffer, &(index as u64).to_string())?;
            return length_of(c, first);
        }
        return Ok(length);
    }
    if n <= length {
        return Ok(n);
    }
    Ok(if field & ENDED != 0 { length } else { 0.0 })
}

/// `NumberParseInt(String(value), 10)`.
fn parse_int(text: &str) -> f64 {
    let text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<f64>() {
        Ok(value) if negative => -value,
        Ok(value) => value,
        Err(_) => f64::NAN,
    }
}

pub(crate) fn read(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let requested = arg(c, args, 0);
    let n = if is_undefined(c, requested) {
        f64::NAN
    } else {
        match c.rooted_value(requested).and_then(|value| value.as_number()) {
            Some(number) if number.is_finite() && number.fract() == 0.0 => number,
            _ => {
                let text = c.to_string(requested)?;
                parse_int(&text)
            }
        }
    };
    read_count(c, this, n)
}

/// `Readable.prototype.read(n)` for a numeric `n` (`NaN`: as much as suits the mode).
pub(crate) fn read_count(c: &mut Context<'_>, stream: RootId, n: f64) -> Outcome {
    let state = state_of(c, stream)?;
    let original = n;
    let mut n = n;
    if n > number(c, state, "highWaterMark")? {
        let mark = new_high_water_mark(c, n)?;
        put_number(c, state, "highWaterMark", mark)?;
    }
    if n != 0.0 {
        change(c, state, 0, EMITTED_READABLE)?;
    }
    let field = bits(c, state)?;
    let length = number(c, state, "length")?;
    let mark = number(c, state, "highWaterMark")?;
    if n == 0.0
        && field & NEED_READABLE != 0
        && ((if mark != 0.0 { length >= mark } else { length > 0.0 }) || field & ENDED != 0)
    {
        if length == 0.0 && field & ENDED != 0 {
            end_readable(c, stream)?;
        } else {
            emit_readable(c, stream)?;
        }
        return Ok(c.null());
    }
    n = how_much_to_read(c, n, state)?;
    if n == 0.0 && state::has(c, state, ENDED)? {
        if number(c, state, "length")? == 0.0 {
            end_readable(c, stream)?;
        }
        return Ok(c.null());
    }
    let field = bits(c, state)?;
    let length = number(c, state, "length")?;
    let mark = number(c, state, "highWaterMark")?;
    let mut do_read = field & NEED_READABLE != 0 || length == 0.0 || length - n < mark;
    if field & (READING | ENDED | state::DESTROYED | state::ERRORED | state::CONSTRUCTED) != state::CONSTRUCTED {
        do_read = false;
    }
    if do_read {
        change(c, state, READING | SYNC | if length == 0.0 { NEED_READABLE } else { 0 }, 0)?;
        let size = c.number(mark);
        let result = call_method(c, stream, "_read", &[size]);
        if let Err(thrown) = super::attempt(result)? {
            error_or_destroy(c, stream, thrown, false)?;
        }
        let field = change(c, state, 0, SYNC)?;
        if field & READING == 0 {
            n = how_much_to_read(c, original, state)?;
        }
    }
    let chunk = if n > 0.0 { from_list(c, n, state)? } else { c.null() };
    if is_null(c, chunk) {
        if number(c, state, "length")? <= number(c, state, "highWaterMark")? {
            change(c, state, NEED_READABLE, 0)?;
        }
        n = 0.0;
    } else {
        let length = number(c, state, "length")?;
        put_number(c, state, "length", length - n)?;
        clear_await_drain(c, state)?;
    }
    if number(c, state, "length")? == 0.0 {
        let field = bits(c, state)?;
        if field & ENDED == 0 {
            change(c, state, NEED_READABLE, 0)?;
        }
        // `nOrig !== n`: a NaN request never matches.
        if original != n && field & ENDED != 0 {
            end_readable(c, stream)?;
        }
    }
    if !is_null(c, chunk) && bits(c, state)? & (state::ERROR_EMITTED | state::CLOSE_EMITTED) == 0 {
        change(c, state, DATA_EMITTED, 0)?;
        emit(c, stream, "data", &[chunk])?;
    }
    Ok(chunk)
}

fn buffer_entry(c: &mut Context<'_>, buffer: RootId, index: f64) -> Outcome {
    get(c, buffer, &(index as u64).to_string())
}

fn set_buffer_entry(c: &mut Context<'_>, buffer: RootId, index: f64, value: RootId) -> Result<(), RootedError> {
    put(c, buffer, &(index as u64).to_string(), value)
}

fn view_bytes(c: &mut Context<'_>, view: RootId) -> Vec<u8> {
    c.view_bytes_rooted(view).unwrap_or_default()
}

/// `fromList(n, state)`: take `n` bytes or characters (one object in object mode) from
/// the front of the buffer.
fn from_list(c: &mut Context<'_>, n: f64, state: RootId) -> Outcome {
    if number(c, state, "length")? == 0.0 {
        return Ok(c.null());
    }
    let field = bits(c, state)?;
    let decoding = field & DECODER != 0;
    let buffer = get(c, state, "buffer")?;
    let mut index = number(c, state, "bufferIndex")?;
    let length = number(c, buffer, "length")?;
    let null = c.null();
    let total = number(c, state, "length")?;
    let result;
    if field & state::OBJECT_MODE != 0 {
        result = buffer_entry(c, buffer, index)?;
        set_buffer_entry(c, buffer, index, null)?;
        index += 1.0;
    } else if n == 0.0 || n.is_nan() || n >= total {
        if decoding {
            let mut units = Vec::new();
            while index < length {
                let entry = buffer_entry(c, buffer, index)?;
                units.extend(c.to_string_code_units(entry)?);
                set_buffer_entry(c, buffer, index, null)?;
                index += 1.0;
            }
            result = c.string_units_rooted(&units);
        } else if length - index == 0.0 {
            result = super::super::buffer_shared_vm::from_vec(c, Vec::new())?;
        } else if length - index == 1.0 {
            result = buffer_entry(c, buffer, index)?;
            set_buffer_entry(c, buffer, index, null)?;
            index += 1.0;
        } else {
            let mut bytes = Vec::with_capacity(total as usize);
            while index < length {
                let entry = buffer_entry(c, buffer, index)?;
                bytes.extend(view_bytes(c, entry));
                set_buffer_entry(c, buffer, index, null)?;
                index += 1.0;
            }
            bytes.resize(total as usize, 0);
            result = super::super::buffer_shared_vm::from_vec(c, bytes)?;
        }
    } else {
        let first = buffer_entry(c, buffer, index)?;
        let first_length = length_of(c, first)?;
        if n < first_length {
            let zero = c.number(0.0);
            let count = c.number(n);
            result = call_method(c, first, "slice", &[zero, count])?;
            let rest = call_method(c, first, "slice", &[count])?;
            set_buffer_entry(c, buffer, index, rest)?;
        } else if n == first_length {
            result = first;
            set_buffer_entry(c, buffer, index, null)?;
            index += 1.0;
        } else if decoding {
            let mut units = Vec::new();
            let mut remaining = n;
            while index < length {
                let text = buffer_entry(c, buffer, index)?;
                let size = length_of(c, text)?;
                if remaining > size {
                    units.extend(c.to_string_code_units(text)?);
                    remaining -= size;
                    set_buffer_entry(c, buffer, index, null)?;
                    index += 1.0;
                } else {
                    if remaining == size {
                        units.extend(c.to_string_code_units(text)?);
                        set_buffer_entry(c, buffer, index, null)?;
                        index += 1.0;
                    } else {
                        let zero = c.number(0.0);
                        let count = c.number(remaining);
                        let head = call_method(c, text, "slice", &[zero, count])?;
                        units.extend(c.to_string_code_units(head)?);
                        let rest = call_method(c, text, "slice", &[count])?;
                        set_buffer_entry(c, buffer, index, rest)?;
                    }
                    break;
                }
            }
            result = c.string_units_rooted(&units);
        } else {
            let mut bytes = Vec::with_capacity(n as usize);
            let mut remaining = n;
            while index < length {
                let data = buffer_entry(c, buffer, index)?;
                let size = length_of(c, data)?;
                if remaining > size {
                    bytes.extend(view_bytes(c, data));
                    remaining -= size;
                    set_buffer_entry(c, buffer, index, null)?;
                    index += 1.0;
                } else {
                    let data_bytes = view_bytes(c, data);
                    bytes.extend_from_slice(&data_bytes[..remaining as usize]);
                    if remaining == size {
                        set_buffer_entry(c, buffer, index, null)?;
                        index += 1.0;
                    } else {
                        let rest = buffer_window(c, data, remaining, size - remaining)?;
                        set_buffer_entry(c, buffer, index, rest)?;
                    }
                    break;
                }
            }
            bytes.resize(n as usize, 0);
            result = super::super::buffer_shared_vm::from_vec(c, bytes)?;
        }
    }
    if index == length {
        put_number(c, buffer, "length", 0.0)?;
        put_number(c, state, "bufferIndex", 0.0)?;
    } else if index > BUFFER_COMPACTION_INDEX {
        let zero = c.number(0.0);
        let count = c.number(index);
        call_method(c, buffer, "splice", &[zero, count])?;
        put_number(c, state, "bufferIndex", 0.0)?;
    } else {
        put_number(c, state, "bufferIndex", index)?;
    }
    Ok(result)
}

/// `onEofChunk(stream, state)`.
fn on_eof_chunk(c: &mut Context<'_>, stream: RootId, state: RootId) -> Result<(), RootedError> {
    let field = bits(c, state)?;
    if field & ENDED != 0 {
        return Ok(());
    }
    if field & DECODER != 0 {
        let decoder = get_slot(c, state, Slot::DecoderValue)?;
        let chunk = call_method(c, decoder, "end", &[])?;
        if c.truthy_rooted(chunk)? && length_of(c, chunk)? != 0.0 {
            let buffer = get(c, state, "buffer")?;
            call_method(c, buffer, "push", &[chunk])?;
            let size = state::chunk_size(c, state, chunk)?;
            let length = number(c, state, "length")?;
            put_number(c, state, "length", length + size)?;
        }
    }
    let field = change(c, state, ENDED, 0)?;
    if field & SYNC != 0 {
        emit_readable(c, stream)
    } else {
        change(c, state, EMITTED_READABLE, NEED_READABLE)?;
        emit_readable_now(c, stream)
    }
}

/// `emitReadable(stream)`: one `readable` event per tick.
fn emit_readable(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let state = state_of(c, stream)?;
    let field = change(c, state, 0, NEED_READABLE)?;
    if field & EMITTED_READABLE == 0 {
        change(c, state, EMITTED_READABLE, 0)?;
        tick(c, Tick::EmitReadable, &[stream])?;
    }
    Ok(())
}

pub(crate) fn emit_readable_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    emit_readable_now(c, stream)?;
    Ok(c.undefined())
}

/// `emitReadable_(stream)`.
fn emit_readable_now(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let state = state_of(c, stream)?;
    let field = bits(c, state)?;
    if field & (state::DESTROYED | state::ERRORED) == 0 && (number(c, state, "length")? != 0.0 || field & ENDED != 0) {
        emit(c, stream, "readable", &[])?;
        change(c, state, 0, EMITTED_READABLE)?;
    }
    let field = bits(c, state)?;
    if field & (FLOWING | ENDED) == 0 && number(c, state, "length")? <= number(c, state, "highWaterMark")? {
        change(c, state, NEED_READABLE, 0)?;
    }
    flow(c, stream)
}

/// `maybeReadMore(stream, state)`: top the buffer up on the next tick.
pub(crate) fn maybe_read_more(c: &mut Context<'_>, stream: RootId, state: RootId) -> Result<(), RootedError> {
    if bits(c, state)? & (READING_MORE | READING | state::CONSTRUCTED) == state::CONSTRUCTED {
        change(c, state, READING_MORE, 0)?;
        tick(c, Tick::MaybeReadMore, &[stream, state])?;
    }
    Ok(())
}

pub(crate) fn maybe_read_more_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, state) = (arg(c, args, 0), arg(c, args, 1));
    loop {
        let mark = c.root_scope_mark();
        let field = bits(c, state)?;
        let length = number(c, state, "length")?;
        let wanted = length < number(c, state, "highWaterMark")? || (field & FLOWING != 0 && length == 0.0);
        if field & (READING | ENDED) != 0 || !wanted {
            break;
        }
        read_count(c, stream, 0.0)?;
        let unchanged = number(c, state, "length")? == length;
        c.release_roots_since(mark, &[]);
        if unchanged {
            break;
        }
    }
    change(c, state, 0, READING_MORE)?;
    Ok(c.undefined())
}

pub(crate) fn default_read(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(errors::throw(c, StreamError::MethodNotImplemented("_read()")))
}

const PIPE_LISTENERS: &[(&str, &str)] = &[
    ("onunpipe", "streamPipeOnUnpipe"),
    ("onend", "streamPipeOnEnd"),
    ("cleanup", "streamPipeCleanup"),
    ("ondata", "streamPipeOnData"),
    ("onerror", "streamPipeOnError"),
    ("onclose", "streamPipeOnClose"),
    ("onfinish", "streamPipeOnFinish"),
    ("unpipe", "streamPipeUnpipe"),
];

pub(crate) fn pipe(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (dest, options) = (arg(c, args, 0), arg(c, args, 1));
    let state = state_of(c, this)?;
    let pipes = get(c, state, "pipes")?;
    if number(c, pipes, "length")? == 1.0 && !state::has(c, state, MULTI_AWAIT_DRAIN)? {
        change(c, state, MULTI_AWAIT_DRAIN, 0)?;
        let writer = get(c, state, "awaitDrainWriters")?;
        let initial = if c.truthy_rooted(writer)? { c.array_rooted(&[writer])? } else { c.array_rooted(&[])? };
        let set = new_set(c, initial)?;
        put(c, state, "awaitDrainWriters", set)?;
    }
    call_method(c, pipes, "push", &[dest])?;
    let ends = if c.truthy_rooted(options)? {
        let end = get(c, options, "end")?;
        let no = c.boolean(false);
        !c.strict_equal_rooted(end, no)?
    } else {
        true
    };
    let do_end = ends && !is_standard_output(c, dest)?;
    let record = c.null_object_rooted()?;
    put(c, record, "src", this)?;
    put(c, record, "dest", dest)?;
    put(c, record, "state", state)?;
    put_bool(c, record, "cleanedUp", false)?;
    for &(name, operation) in PIPE_LISTENERS {
        let listener = closure(c, operation, name, &[record])?;
        put(c, record, name, listener)?;
    }
    let end_listener = get(c, record, if do_end { "onend" } else { "unpipe" })?;
    if state::has(c, state, END_EMITTED)? {
        next_tick(c, end_listener, &[])?;
    } else {
        once(c, this, "end", end_listener)?;
    }
    let listener = get(c, record, "onunpipe")?;
    on(c, dest, "unpipe", listener)?;
    let listener = get(c, record, "ondata")?;
    on(c, this, "data", listener)?;
    let listener = get(c, record, "onerror")?;
    let error = c.string_rooted("error");
    super::legacy::prepend_listener(c, dest, error, listener)?;
    let listener = get(c, record, "onclose")?;
    once(c, dest, "close", listener)?;
    let listener = get(c, record, "onfinish")?;
    once(c, dest, "finish", listener)?;
    emit(c, dest, "pipe", &[this])?;
    let need_drain = get(c, dest, "writableNeedDrain")?;
    let yes = c.boolean(true);
    if c.strict_equal_rooted(need_drain, yes)? {
        pipe_pause(c, record)?;
    } else if !state::has(c, state, FLOWING)? {
        call_method(c, this, "resume", &[])?;
    }
    Ok(dest)
}

/// `dest === process.stdout || dest === process.stderr`.
fn is_standard_output(c: &mut Context<'_>, dest: RootId) -> Result<bool, RootedError> {
    let process = c.host_mut().shared_state().borrow().process_module;
    let Some(process) = process else { return Ok(false) };
    for name in ["stdout", "stderr"] {
        let output = get(c, process, name)?;
        if c.strict_equal_rooted(dest, output)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn new_set(c: &mut Context<'_>, values: RootId) -> Outcome {
    let global = c.global_root()?;
    let set = get(c, global, "Set")?;
    c.construct_rooted(set, set, &[values])
}

pub(crate) fn pipe_on_unpipe(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let (readable, info) = (arg(c, args, 0), arg(c, args, 1));
    let src = get(c, record, "src")?;
    if c.strict_equal_rooted(readable, src)? && c.truthy_rooted(info)? {
        let unpiped = get(c, info, "hasUnpiped")?;
        let no = c.boolean(false);
        if c.strict_equal_rooted(unpiped, no)? {
            put_bool(c, info, "hasUnpiped", true)?;
            pipe_cleanup_now(c, record)?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn pipe_on_end(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let dest = get(c, record, "dest")?;
    call_method(c, dest, "end", &[])?;
    Ok(c.undefined())
}

pub(crate) fn pipe_cleanup(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    pipe_cleanup_now(c, record)?;
    Ok(c.undefined())
}

fn pipe_cleanup_now(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let src = get(c, record, "src")?;
    let dest = get(c, record, "dest")?;
    let drain = get(c, record, "ondrain")?;
    for (event, name) in [("close", "onclose"), ("finish", "onfinish")] {
        let listener = get(c, record, name)?;
        remove_listener(c, dest, event, listener)?;
    }
    if c.truthy_rooted(drain)? {
        remove_listener(c, dest, "drain", drain)?;
    }
    for (event, name) in [("error", "onerror"), ("unpipe", "onunpipe")] {
        let listener = get(c, record, name)?;
        remove_listener(c, dest, event, listener)?;
    }
    for (event, name) in [("end", "onend"), ("end", "unpipe"), ("data", "ondata")] {
        let listener = get(c, record, name)?;
        remove_listener(c, src, event, listener)?;
    }
    put_bool(c, record, "cleanedUp", true)?;
    let state = get(c, record, "state")?;
    let writers = get(c, state, "awaitDrainWriters")?;
    if c.truthy_rooted(drain)? && c.truthy_rooted(writers)? {
        let writable_state = get(c, dest, "_writableState")?;
        let waiting = if c.truthy_rooted(writable_state)? {
            let need = get(c, writable_state, "needDrain")?;
            c.truthy_rooted(need)?
        } else {
            true
        };
        if waiting {
            let undefined = c.undefined();
            c.call_rooted(drain, undefined, &[])?;
        }
    }
    Ok(())
}

/// The pipe's `pause()`: wait for this destination's drain.
fn pipe_pause(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let src = get(c, record, "src")?;
    let dest = get(c, record, "dest")?;
    let state = get(c, record, "state")?;
    let cleaned = get(c, record, "cleanedUp")?;
    if !c.truthy_rooted(cleaned)? {
        let pipes = get(c, state, "pipes")?;
        let count = number(c, pipes, "length")?;
        let first = get(c, pipes, "0")?;
        if count == 1.0 && c.strict_equal_rooted(first, dest)? {
            put(c, state, "awaitDrainWriters", dest)?;
            change(c, state, 0, MULTI_AWAIT_DRAIN)?;
        } else if count > 1.0 {
            let includes = call_method(c, pipes, "includes", &[dest])?;
            if c.truthy_rooted(includes)? {
                let writers = get(c, state, "awaitDrainWriters")?;
                call_method(c, writers, "add", &[dest])?;
            }
        }
        call_method(c, src, "pause", &[])?;
    }
    let drain = get(c, record, "ondrain")?;
    if !c.truthy_rooted(drain)? {
        let drain = closure(c, "streamPipeOnDrain", "pipeOnDrainFunctionResult", &[src, dest])?;
        put(c, record, "ondrain", drain)?;
        on(c, dest, "drain", drain)?;
    }
    Ok(())
}

pub(crate) fn pipe_on_data(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let chunk = arg(c, args, 0);
    let dest = get(c, record, "dest")?;
    let written = call_method(c, dest, "write", &[chunk])?;
    let no = c.boolean(false);
    if c.strict_equal_rooted(written, no)? {
        pipe_pause(c, record)?;
    }
    Ok(c.undefined())
}

pub(crate) fn pipe_on_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    pipe_unpipe_now(c, record)?;
    let dest = get(c, record, "dest")?;
    let listener = get(c, record, "onerror")?;
    remove_listener(c, dest, "error", listener)?;
    if listener_count(c, dest, "error")? == 0.0 {
        let writable_state = get(c, dest, "_writableState")?;
        let half = if c.truthy_rooted(writable_state)? { writable_state } else { get(c, dest, "_readableState")? };
        let emitted = if c.truthy_rooted(half)? {
            let emitted = get(c, half, "errorEmitted")?;
            Some(c.truthy_rooted(emitted)?)
        } else {
            None
        };
        if emitted == Some(false) {
            error_or_destroy(c, dest, error, false)?;
        } else {
            emit(c, dest, "error", &[error])?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn pipe_on_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let dest = get(c, record, "dest")?;
    let listener = get(c, record, "onfinish")?;
    remove_listener(c, dest, "finish", listener)?;
    pipe_unpipe_now(c, record)?;
    Ok(c.undefined())
}

pub(crate) fn pipe_on_finish(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let dest = get(c, record, "dest")?;
    let listener = get(c, record, "onclose")?;
    remove_listener(c, dest, "close", listener)?;
    pipe_unpipe_now(c, record)?;
    Ok(c.undefined())
}

pub(crate) fn pipe_unpipe(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    pipe_unpipe_now(c, record)?;
    Ok(c.undefined())
}

fn pipe_unpipe_now(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let src = get(c, record, "src")?;
    let dest = get(c, record, "dest")?;
    call_method(c, src, "unpipe", &[dest]).map(drop)
}

/// `pipeOnDrain(src, dest)`'s listener: resume once every waiting destination drained.
pub(crate) fn pipe_on_drain(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [src, dest] = captures(c)?;
    let state = state_of(c, src)?;
    let writers = get(c, state, "awaitDrainWriters")?;
    if c.strict_equal_rooted(writers, dest)? {
        let null = c.null();
        put(c, state, "awaitDrainWriters", null)?;
    } else if state::has(c, state, MULTI_AWAIT_DRAIN)? {
        call_method(c, writers, "delete", &[dest])?;
    }
    let writers = get(c, state, "awaitDrainWriters")?;
    let drained = !c.truthy_rooted(writers)? || number(c, writers, "size")? == 0.0;
    if drained && state::has(c, state, DATA_LISTENING)? {
        call_method(c, src, "resume", &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn unpipe(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let dest = arg(c, args, 0);
    let state = state_of(c, this)?;
    let info = c.object_rooted()?;
    put_bool(c, info, "hasUnpiped", false)?;
    let pipes = get(c, state, "pipes")?;
    if number(c, pipes, "length")? == 0.0 {
        return Ok(this);
    }
    if !c.truthy_rooted(dest)? {
        let fresh = c.array_rooted(&[])?;
        put(c, state, "pipes", fresh)?;
        call_method(c, this, "pause", &[])?;
        for destination in c.array_like_values_rooted(pipes)? {
            let info = c.object_rooted()?;
            put_bool(c, info, "hasUnpiped", false)?;
            emit(c, destination, "unpipe", &[this, info])?;
        }
        return Ok(this);
    }
    let index = call_method(c, pipes, "indexOf", &[dest])?;
    let index_value = c.to_number(index)?;
    if index_value < 0.0 {
        return Ok(this);
    }
    let one = c.number(1.0);
    call_method(c, pipes, "splice", &[index, one])?;
    if number(c, pipes, "length")? == 0.0 {
        call_method(c, this, "pause", &[])?;
    }
    emit(c, dest, "unpipe", &[this, info])?;
    Ok(this)
}

fn event_is(c: &mut Context<'_>, event: RootId, name: &str) -> Result<bool, RootedError> {
    Ok(is_string(c, event) && c.string_text(event)?.is_some_and(|text| text == name))
}

/// `Stream.prototype[method]`, the EventEmitter member a readable override extends.
fn emitter_member(c: &mut Context<'_>, method: &str) -> Outcome {
    let stream = intrinsics(c)?.class(Class::Stream);
    let prototype = get(c, stream, "prototype")?;
    get(c, prototype, method)
}

pub(crate) fn on_listener(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (event, listener) = (arg(c, args, 0), arg(c, args, 1));
    let add = emitter_member(c, "on")?;
    let result = c.call_rooted(add, this, &[event, listener])?;
    let state = state_of(c, this)?;
    if event_is(c, event, "data")? {
        change(c, state, DATA_LISTENING, 0)?;
        if listener_count(c, this, "readable")? > 0.0 {
            change(c, state, READABLE_LISTENING, 0)?;
        }
        if bits(c, state)? & (HAS_FLOWING | FLOWING) != HAS_FLOWING {
            call_method(c, this, "resume", &[])?;
        }
    } else if event_is(c, event, "readable")? && bits(c, state)? & (END_EMITTED | READABLE_LISTENING) == 0 {
        let field = change(c, state, READABLE_LISTENING | NEED_READABLE | HAS_FLOWING, FLOWING | EMITTED_READABLE)?;
        if number(c, state, "length")? != 0.0 {
            emit_readable(c, this)?;
        } else if field & READING == 0 {
            tick(c, Tick::ReadZero, &[this])?;
        }
    }
    Ok(result)
}

pub(crate) fn remove_listener_method(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (event, listener) = (arg(c, args, 0), arg(c, args, 1));
    let state = state_of(c, this)?;
    let remove = emitter_member(c, "removeListener")?;
    let result = c.call_rooted(remove, this, &[event, listener])?;
    if event_is(c, event, "readable")? {
        tick(c, Tick::UpdateReadableListening, &[this])?;
    } else if event_is(c, event, "data")? && listener_count(c, this, "data")? == 0.0 {
        change(c, state, 0, DATA_LISTENING)?;
    }
    Ok(result)
}

pub(crate) fn remove_all_listeners(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let remove = emitter_member(c, "removeAllListeners")?;
    let result = c.call_rooted(remove, this, args)?;
    let event = arg(c, args, 0);
    if is_undefined(c, event) || event_is(c, event, "readable")? {
        tick(c, Tick::UpdateReadableListening, &[this])?;
    }
    Ok(result)
}

pub(crate) fn update_readable_listening_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    let state = state_of(c, stream)?;
    if listener_count(c, stream, "readable")? > 0.0 {
        change(c, state, READABLE_LISTENING, 0)?;
    } else {
        change(c, state, 0, READABLE_LISTENING)?;
    }
    let field = bits(c, state)?;
    if field & (HAS_PAUSED | PAUSED | RESUME_SCHEDULED) == HAS_PAUSED | RESUME_SCHEDULED {
        change(c, state, HAS_FLOWING | FLOWING, 0)?;
    } else if field & DATA_LISTENING != 0 {
        call_method(c, stream, "resume", &[])?;
    } else if field & READABLE_LISTENING == 0 {
        change(c, state, 0, HAS_FLOWING | FLOWING)?;
    }
    Ok(c.undefined())
}

pub(crate) fn read_zero_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    let zero = c.number(0.0);
    call_method(c, stream, "read", &[zero])?;
    Ok(c.undefined())
}

pub(crate) fn resume(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state_of(c, this)?;
    let field = bits(c, state)?;
    if field & FLOWING == 0 {
        if field & READABLE_LISTENING == 0 {
            change(c, state, HAS_FLOWING | FLOWING, 0)?;
        } else {
            change(c, state, HAS_FLOWING, FLOWING)?;
        }
        if !state::has(c, state, RESUME_SCHEDULED)? {
            change(c, state, RESUME_SCHEDULED, 0)?;
            tick(c, Tick::Resume, &[this, state])?;
        }
    }
    change(c, state, HAS_PAUSED, PAUSED)?;
    Ok(this)
}

pub(crate) fn resume_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, state) = (arg(c, args, 0), arg(c, args, 1));
    let zero = c.number(0.0);
    if !state::has(c, state, READING)? {
        call_method(c, stream, "read", &[zero])?;
    }
    change(c, state, 0, RESUME_SCHEDULED)?;
    emit(c, stream, "resume", &[])?;
    flow(c, stream)?;
    if bits(c, state)? & (FLOWING | READING) == FLOWING {
        call_method(c, stream, "read", &[zero])?;
    }
    Ok(c.undefined())
}

pub(crate) fn pause(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state_of(c, this)?;
    if bits(c, state)? & (HAS_FLOWING | FLOWING) != HAS_FLOWING {
        change(c, state, HAS_FLOWING, FLOWING)?;
        emit(c, this, "pause", &[])?;
    }
    change(c, state, HAS_PAUSED | PAUSED, 0)?;
    Ok(this)
}

/// `flow(stream)`: read while flowing.
fn flow(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let state = state_of(c, stream)?;
    loop {
        let mark = c.root_scope_mark();
        if !state::has(c, state, FLOWING)? {
            break;
        }
        let chunk = call_method(c, stream, "read", &[])?;
        let ended = is_null(c, chunk);
        c.release_roots_since(mark, &[]);
        if ended {
            break;
        }
    }
    Ok(())
}

/// `endReadable(stream)`.
fn end_readable(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let state = state_of(c, stream)?;
    if !state::has(c, state, END_EMITTED)? {
        change(c, state, ENDED, 0)?;
        tick(c, Tick::EndReadable, &[state, stream])?;
    }
    Ok(())
}

pub(crate) fn end_readable_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (state, stream) = (arg(c, args, 0), arg(c, args, 1));
    let field = bits(c, state)?;
    if field & (state::ERRORED | state::CLOSE_EMITTED | END_EMITTED) != 0 || number(c, state, "length")? != 0.0 {
        return Ok(c.undefined());
    }
    change(c, state, END_EMITTED, 0)?;
    emit(c, stream, "end", &[])?;
    let writable = get(c, stream, "writable")?;
    let half_open = get(c, stream, "allowHalfOpen")?;
    let no = c.boolean(false);
    if c.truthy_rooted(writable)? && c.strict_equal_rooted(half_open, no)? {
        tick(c, Tick::EndWritable, &[stream])?;
    } else if state::has(c, state, state::AUTO_DESTROY)? {
        let writable_state = get(c, stream, "_writableState")?;
        let destroy = if c.truthy_rooted(writable_state)? {
            let auto = get(c, writable_state, "autoDestroy")?;
            let finished = get(c, writable_state, "finished")?;
            let writable = get(c, writable_state, "writable")?;
            c.truthy_rooted(auto)? && (c.truthy_rooted(finished)? || c.strict_equal_rooted(writable, no)?)
        } else {
            true
        };
        if destroy {
            call_method(c, stream, "destroy", &[])?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn end_writable_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    let writable = get(c, stream, "writable")?;
    let ended = get(c, stream, "writableEnded")?;
    let destroyed = get(c, stream, "destroyed")?;
    if c.truthy_rooted(writable)? && !c.truthy_rooted(ended)? && !c.truthy_rooted(destroyed)? {
        call_method(c, stream, "end", &[])?;
    }
    Ok(c.undefined())
}

/// `Readable.prototype.wrap(stream)`: adopt an old-style stream as this readable's source.
pub(crate) fn wrap(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let source = arg(c, args, 0);
    let paused = c.boolean(false);
    let record = c.null_object_rooted()?;
    put(c, record, "readable", this)?;
    put(c, record, "source", source)?;
    put(c, record, "paused", paused)?;
    for (event, operation) in [
        ("data", "streamWrapOnData"),
        ("end", "streamWrapOnEnd"),
        ("error", "streamWrapOnError"),
        ("close", "streamWrapOnClose"),
        ("destroy", "streamWrapOnClose"),
    ] {
        let listener = closure(c, operation, "", &[record])?;
        on(c, source, event, listener)?;
    }
    let read = closure(c, "streamWrapRead", "", &[record])?;
    put(c, this, "_read", read)?;
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let keys = call_method(c, object, "keys", &[source])?;
    for key in c.array_like_values_rooted(keys)?.into_iter().skip(1) {
        let own = c.get_property_rooted(this, key)?;
        let member = c.get_property_rooted(source, key)?;
        if is_undefined(c, own) && c.is_callable_rooted(member)? {
            let bound = call_method(c, member, "bind", &[source])?;
            c.set_property_rooted(this, key, bound, this)?;
        }
    }
    Ok(this)
}

pub(crate) fn wrap_on_data(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let chunk = arg(c, args, 0);
    let readable = get(c, record, "readable")?;
    let source = get(c, record, "source")?;
    let pushed = call_method(c, readable, "push", &[chunk])?;
    let pause = get(c, source, "pause")?;
    if !c.truthy_rooted(pushed)? && c.truthy_rooted(pause)? {
        put_bool(c, record, "paused", true)?;
        c.call_rooted(pause, source, &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn wrap_on_end(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let readable = get(c, record, "readable")?;
    let null = c.null();
    call_method(c, readable, "push", &[null])?;
    Ok(c.undefined())
}

pub(crate) fn wrap_on_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    let readable = get(c, record, "readable")?;
    error_or_destroy(c, readable, error, false)?;
    Ok(c.undefined())
}

pub(crate) fn wrap_on_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let readable = get(c, record, "readable")?;
    call_method(c, readable, "destroy", &[])?;
    Ok(c.undefined())
}

pub(crate) fn wrap_read(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let source = get(c, record, "source")?;
    let paused = get(c, record, "paused")?;
    let resume = get(c, source, "resume")?;
    if c.truthy_rooted(paused)? && c.truthy_rooted(resume)? {
        put_bool(c, record, "paused", false)?;
        c.call_rooted(resume, source, &[])?;
    }
    Ok(c.undefined())
}

/// `Readable._fromList(n, state)`.
pub(crate) fn from_list_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (n, state) = (arg(c, args, 0), arg(c, args, 1));
    let n = c.to_number(n)?;
    from_list(c, n, state)
}

/// `Readable.wrap(src, options)`: a readable over an old-style stream, destroying it along.
pub(crate) fn wrap_static(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (source, options) = (arg(c, args, 0), arg(c, args, 1));
    let settings = c.object_rooted()?;
    let mut object_mode = get(c, source, "readableObjectMode")?;
    if is_nullish(c, object_mode) {
        object_mode = get(c, source, "objectMode")?;
    }
    if is_nullish(c, object_mode) {
        object_mode = c.boolean(true);
    }
    put(c, settings, "objectMode", object_mode)?;
    if !is_nullish(c, options) {
        let global = c.global_root()?;
        let object = get(c, global, "Object")?;
        call_method(c, object, "assign", &[settings, options])?;
    }
    let destroy = closure(c, "streamWrapDestroy", "destroy", &[source])?;
    put(c, settings, "destroy", destroy)?;
    let readable = intrinsics(c)?.class(Class::Readable);
    let wrapper = c.construct_rooted(readable, readable, &[settings])?;
    call_method(c, wrapper, "wrap", &[source])
}

/// The `destroy` a `Readable.wrap` wrapper takes: destroy the source, then call back.
pub(crate) fn wrap_destroy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [source] = captures(c)?;
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    destroy::destroyer(c, source, error)?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])?;
    Ok(undefined)
}

/// `Stream._isArrayBufferView(value)`.
pub(crate) fn is_view_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let view = is_view(c, value)?;
    Ok(c.boolean(view))
}

/// `Stream._isUint8Array(value)`.
pub(crate) fn is_uint8_array_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let bytes = c.has_builtin_brand_rooted(value, BuiltinBrand::Uint8Array)?;
    Ok(c.boolean(bytes))
}

/// `Stream._uint8ArrayToBuffer(chunk)`.
pub(crate) fn view_to_buffer_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    view_to_buffer(c, value)
}

/// `Readable.prototype[Symbol.asyncDispose]()`.
pub(crate) fn async_dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let destroyed = get(c, this, "destroyed")?;
    let mut error = c.undefined();
    if !c.truthy_rooted(destroyed)? {
        let ended = get(c, this, "readableEnded")?;
        error = if c.truthy_rooted(ended)? { c.null() } else { errors::abort_error(c, None)? };
        call_method(c, this, "destroy", &[error])?;
    }
    super::eos::settle_when_finished(c, this, error, super::eos::Tolerated::Same)
}

pub(crate) fn build(c: &mut Context<'_>, stream: RootId, state_class: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamReadable"), "Readable")?;
    let prototype = get(c, constructor, "prototype")?;
    let stream_prototype = get(c, stream, "prototype")?;
    c.set_prototype_rooted(prototype, stream_prototype)?;
    c.set_prototype_rooted(constructor, stream)?;
    let key = c.string_rooted("ReadableState");
    c.define_data_property_rooted(constructor, key, state_class, true, true, true)?;
    define_methods(
        c,
        constructor,
        &[
            (Key::Name("_fromList"), "streamReadableFromList", "fromList"),
            (Key::Name("from"), "streamReadableFrom", ""),
            (Key::Name("wrap"), "streamReadableWrapStatic", ""),
        ],
    )?;
    define_methods(
        c,
        prototype,
        &[
            (Key::Name("destroy"), "streamDestroy", "destroy"),
            (Key::Name("_undestroy"), "streamUndestroy", "undestroy"),
            (Key::Name("_destroy"), "streamDefaultDestroy", ""),
            (Key::Name("push"), "streamReadablePush", ""),
            (Key::Name("unshift"), "streamReadableUnshift", ""),
            (Key::Name("isPaused"), "streamReadableIsPaused", ""),
            (Key::Name("setEncoding"), "streamReadableSetEncoding", ""),
            (Key::Name("read"), "streamReadableRead", ""),
            (Key::Name("_read"), "streamReadableDefaultRead", ""),
            (Key::Name("pipe"), "streamReadablePipe", ""),
            (Key::Name("unpipe"), "streamReadableUnpipe", ""),
            (Key::Name("on"), "streamReadableOn", ""),
        ],
    )?;
    let on = get(c, prototype, "on")?;
    let key = c.string_rooted("addListener");
    c.define_data_property_rooted(prototype, key, on, true, true, true)?;
    define_methods(c, prototype, &[(Key::Name("removeListener"), "streamReadableRemoveListener", "")])?;
    let remove = get(c, prototype, "removeListener")?;
    let key = c.string_rooted("off");
    c.define_data_property_rooted(prototype, key, remove, true, true, true)?;
    define_methods(
        c,
        prototype,
        &[
            (Key::Name("removeAllListeners"), "streamReadableRemoveAllListeners", ""),
            (Key::Name("resume"), "streamReadableResume", ""),
            (Key::Name("pause"), "streamReadablePause", ""),
            (Key::Name("wrap"), "streamReadableWrap", ""),
            (Key::Name("iterator"), "streamReadableIterator", ""),
        ],
    )?;
    super::utils::define_stream_accessors(c, prototype, super::utils::READABLE_ACCESSORS)?;
    define_methods(
        c,
        prototype,
        &[
            (Key::Symbol(Slot::Rejection), "streamCaptureRejection", ""),
            (Key::Symbol(Slot::AsyncDispose), "streamReadableAsyncDispose", ""),
            (Key::Symbol(Slot::AsyncIterator), "streamReadableAsyncIterator", ""),
        ],
    )?;
    Ok(constructor)
}
