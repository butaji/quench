//! The body mixin (undici's `lib/web/fetch/body.js`): extracting a body from a BodyInit,
//! the stream every body is read through, and consuming it as text, JSON, bytes, a Blob or
//! a FormData.

use super::{form_data, headers, type_error, Context, Outcome};
use crate::modules::blob_shared_vm;
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::webstreams_shared_vm::{
    self as web, arg, byte, deferred, get, is_nullish, put, readable, record, slots,
};
use quench_runtime::{RootId, RootedError, ValueType};

members!(Member, "fetchBody", [
    SourcePull,
    SourceClose,
    Ignore,
    ReadChunk,
    ReadClose,
    ReadError,
]);

/// A body's fields: its stream, the bytes it was made of when known (to send again on a
/// redirect), and their length.
pub(crate) mod field {
    pub(crate) const STREAM: &str = "stream";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const LENGTH: &str = "length";
}
use field::*;

/// A read in progress: the target promise, what it makes, the chunks so far and the
/// reader.
mod read {
    pub(super) const PROMISE: &str = "promise";
    pub(super) const KIND: &str = "kind";
    pub(super) const CHUNKS: &str = "chunks";
    pub(super) const READER: &str = "reader";
    pub(super) const MIME: &str = "mime";
}

/// What a consumption produces.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Consume {
    ArrayBuffer,
    Blob,
    Bytes,
    FormData,
    Json,
    Text,
}

impl Consume {
    const ALL: [Self; 6] = [Self::ArrayBuffer, Self::Blob, Self::Bytes, Self::FormData, Self::Json, Self::Text];
}

const TEXT_PLAIN: &str = "text/plain;charset=UTF-8";
const FORM_URLENCODED: &str = "application/x-www-form-urlencoded;charset=UTF-8";
const BODY_UNUSABLE: &str = "Body is unusable: Body has already been read";
const DISTURBED: &str = "Response body object should not be disturbed or locked";

/// A body over `bytes`: a byte stream that delivers them in one chunk.
pub(crate) fn from_bytes(c: &mut Context<'_>, bytes: Vec<u8>) -> Outcome {
    let length = c.number(bytes.len() as f64);
    let source = c.uint8_array_from_rooted(bytes)?;
    let cell = record(c, &[])?;
    let pull = function(c, Member::SourcePull, "", &[source, cell])?;
    let cancel = function(c, Member::Ignore, "", &[])?;
    let stream = byte::create_byte_readable(c, pull, cancel)?;
    let object = get(c, stream, readable::field::SELF)?;
    record(c, &[(STREAM, object), (SOURCE, source), (LENGTH, length)])
}

/// A body over a ReadableStream, of unknown length.
pub(crate) fn from_stream(c: &mut Context<'_>, stream: RootId) -> Outcome {
    record(c, &[(STREAM, stream)])
}

/// `extractBody(object)`: the body and the content type it implies.
pub(crate) fn extract(c: &mut Context<'_>, object: RootId) -> Result<(RootId, Option<String>), RootedError> {
    if matches!(c.value_type_rooted(object), Some(ValueType::String)) {
        let text = super::usv_string(c, object)?;
        return Ok((from_bytes(c, text.into_bytes())?, Some(TEXT_PLAIN.to_owned())));
    }
    if crate::modules::url_whatwg_shared_vm::is_search_params(c, object)? {
        let text = crate::modules::url_whatwg_shared_vm::params_to_string(c, object, &[])?;
        let text = c.to_string(text)?;
        return Ok((from_bytes(c, text.into_bytes())?, Some(FORM_URLENCODED.to_owned())));
    }
    if c.has_builtin_brand_rooted(object, quench_runtime::BuiltinBrand::AnyArrayBuffer)?
        || c.has_builtin_brand_rooted(object, quench_runtime::BuiltinBrand::ArrayBufferView)?
    {
        let bytes = match web::view_parts(c, object)? {
            Some(parts) => c.with_buffer_bytes_rooted(parts.buffer, |bytes| bytes[parts.offset..parts.offset + parts.length].to_vec()),
            None => c.with_buffer_bytes_rooted(object, <[u8]>::to_vec),
        }
        .unwrap_or_default();
        return Ok((from_bytes(c, bytes)?, None));
    }
    if blob_shared_vm::is_blob(c, object)? {
        let bytes = blob_shared_vm::chunks(c, object)?.concat();
        let kind = blob_shared_vm::type_of(c, object)?;
        return Ok((from_bytes(c, bytes)?, (!kind.is_empty()).then_some(kind)));
    }
    if let Some(form) = super::private_slot::read(c, object, super::Slot::FormData)? {
        let (bytes, kind) = form_data::serialize(c, form)?;
        return Ok((from_bytes(c, bytes)?, Some(kind)));
    }
    if let Some(stream) = web::state_of(c, object, web::Brand::ReadableStream)? {
        if readable::is_disturbed(c, stream)? || readable::is_locked(c, stream)? {
            return Err(type_error(c, DISTURBED));
        }
        return Ok((from_stream(c, object)?, None));
    }
    if is_async_iterable(c, object)? {
        let stream = readable::from(c, object)?;
        return Ok((from_stream(c, stream)?, None));
    }
    let text = super::usv_string(c, object)?;
    Ok((from_bytes(c, text.into_bytes())?, Some(TEXT_PLAIN.to_owned())))
}

/// Whether `object` has an async iterator, which undici reads as a body stream.
fn is_async_iterable(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(false);
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = get(c, symbol, "asyncIterator")?;
    let method = c.get_property_rooted(object, key)?;
    c.is_callable_rooted(method)
}

/// Whether `object` is a ReadableStream (a body that cannot be sent twice).
pub(crate) fn is_stream_init(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    web::is(c, object, web::Brand::ReadableStream)
}

fn stream_slots(c: &mut Context<'_>, body: RootId) -> Outcome {
    let stream = get(c, body, STREAM)?;
    slots(c, stream)
}

/// `bodyUsed`: the body's stream was read from.
pub(crate) fn used(c: &mut Context<'_>, body: RootId) -> Result<bool, RootedError> {
    if is_nullish(c, body) {
        return Ok(false);
    }
    let stream = stream_slots(c, body)?;
    readable::is_disturbed(c, stream)
}

/// Whether the body can no longer be read (`bodyUnusable`).
pub(crate) fn unusable(c: &mut Context<'_>, body: RootId) -> Result<bool, RootedError> {
    if is_nullish(c, body) {
        return Ok(false);
    }
    let stream = stream_slots(c, body)?;
    Ok(readable::is_disturbed(c, stream)? || readable::is_locked(c, stream)?)
}

/// `cloneBody(body)`: tees the stream, keeping one branch and giving the other.
pub(crate) fn clone(c: &mut Context<'_>, body: RootId) -> Outcome {
    let stream = stream_slots(c, body)?;
    let branches = readable::tee_stream(c, stream)?;
    let first = get(c, branches, "0")?;
    let second = get(c, branches, "1")?;
    put(c, body, STREAM, first)?;
    let copy = record(c, &[(STREAM, second)])?;
    for name in [SOURCE, LENGTH] {
        let value = get(c, body, name)?;
        put(c, copy, name, value)?;
    }
    Ok(copy)
}

/// A body reading `body`'s stream through an identity transform, as a Request made from
/// another takes over its body: the source stream ends up locked to the pipe.
pub(crate) fn pipe_through_identity(c: &mut Context<'_>, body: RootId) -> Outcome {
    let classes = web::classes(c)?;
    let class = get(c, classes, web::Brand::TransformStream.name())?;
    let transform = c.construct_rooted(class, class, &[])?;
    let transform_state = slots(c, transform)?;
    let writable = web::transform::side(c, transform_state, false)?;
    let readable = web::transform::side(c, transform_state, true)?;
    let source = stream_slots(c, body)?;
    let destination = slots(c, writable)?;
    let undefined = c.undefined();
    let piped = web::pipe::pipe(c, source, destination, false, false, false, undefined)?;
    web::mark_handled(c, piped)?;
    let copy = record(c, &[(STREAM, readable)])?;
    for name in [SOURCE, LENGTH] {
        let value = get(c, body, name)?;
        put(c, copy, name, value)?;
    }
    Ok(copy)
}

/// The body mixin's consumers: a promise of the body read as `kind`. `headers` are the
/// Request's or Response's, whose content type a Blob or FormData follows.
pub(crate) fn consume(c: &mut Context<'_>, body: RootId, headers_state: RootId, kind: Consume) -> Outcome {
    let promise = deferred(c)?;
    if unusable(c, body)? {
        let error = c.type_error_rooted(BODY_UNUSABLE)?;
        c.reject_promise_rooted(promise, error)?;
        return Ok(promise);
    }
    let mime = headers::get_raw(c, headers_state, "content-type")?.unwrap_or_default();
    let mime = c.string_rooted(&mime);
    let kind_value = c.number(kind as usize as f64);
    let chunks = c.array_rooted(&[])?;
    let state = record(c, &[(read::PROMISE, promise), (read::KIND, kind_value), (read::CHUNKS, chunks), (read::MIME, mime)])?;
    if is_nullish(c, body) {
        finish(c, state)?;
        return Ok(promise);
    }
    let stream = stream_slots(c, body)?;
    let reader = readable::default_reader(c, stream)?;
    let reader = slots(c, reader)?;
    put(c, state, read::READER, reader)?;
    read_next(c, state)?;
    Ok(promise)
}

fn read_next(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let reader = get(c, state, read::READER)?;
    let chunk = function(c, Member::ReadChunk, "", &[state])?;
    let close = function(c, Member::ReadClose, "", &[state])?;
    let error = function(c, Member::ReadError, "", &[state])?;
    readable::read_with(c, reader, chunk, close, error)
}

/// "Fully read body" then "package data" for the consumption's kind.
fn finish(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let promise = get(c, state, read::PROMISE)?;
    let chunks = get(c, state, read::CHUNKS)?;
    let mut bytes = Vec::new();
    for chunk in c.array_like_values_rooted(chunks)? {
        bytes.extend(c.view_bytes_rooted(chunk).unwrap_or_default());
    }
    let kind = Consume::ALL[web::num(c, state, read::KIND)? as usize];
    let mime = get(c, state, read::MIME)?;
    let mime = c.to_string(mime)?;
    match web::attempt(package(c, bytes, kind, &mime))? {
        Ok(value) => c.resolve_promise_rooted(promise, value),
        Err(error) => c.reject_promise_rooted(promise, error),
    }
}

/// UTF-8 decode, as undici's `utf8DecodeBytes`: a leading BOM is dropped.
fn decode_utf8(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()
}

fn package(c: &mut Context<'_>, bytes: Vec<u8>, kind: Consume, mime: &str) -> Outcome {
    match kind {
        Consume::ArrayBuffer => {
            let view = c.uint8_array_from_rooted(bytes)?;
            get(c, view, "buffer")
        }
        Consume::Bytes => c.uint8_array_from_rooted(bytes),
        Consume::Blob => blob_shared_vm::from_bytes(c, bytes, &blob_type(mime)),
        Consume::Text => Ok(c.string_rooted(&decode_utf8(&bytes))),
        Consume::Json => c.parse_json_rooted(&decode_utf8(&bytes)),
        Consume::FormData => {
            let essence = mime.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            if essence == "multipart/form-data" {
                let parsed = match form_data::header_parameter(mime, "boundary") {
                    Some(boundary) => form_data::parse_multipart(c, &bytes, &boundary)?,
                    None => None,
                };
                return match parsed {
                    Some(form) => Ok(form),
                    None => Err(type_error(c, "Failed to parse body as FormData.")),
                };
            }
            if essence == "application/x-www-form-urlencoded" {
                return form_data::parse_urlencoded(c, &bytes);
            }
            Err(type_error(c, "Content-Type was not one of \"multipart/form-data\" or \"application/x-www-form-urlencoded\"."))
        }
    }
}

/// The Blob type of a body's content type: its serialization, or empty when it is not a
/// valid MIME type.
fn blob_type(mime: &str) -> String {
    let essence = mime.split(';').next().unwrap_or_default().trim();
    if essence.split_once('/').is_some_and(|(kind, sub)| !kind.is_empty() && !sub.is_empty()) {
        mime.to_ascii_lowercase()
    } else {
        String::new()
    }
}

pub(crate) fn member(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::SourcePull => {
            let [source, cell] = captured(c)?;
            let controller = arg(c, args, 0);
            let controller_state = slots(c, controller)?;
            if !web::flag(c, cell, "pulled")? {
                web::put_flag(c, cell, "pulled", true)?;
                let bytes = c.view_bytes_rooted(source).unwrap_or_default();
                if !bytes.is_empty() {
                    let copy = c.uint8_array_from_rooted(bytes)?;
                    byte::enqueue(c, controller_state, copy)?;
                }
                let close = function(c, Member::SourceClose, "", &[controller])?;
                c.queue_microtask_rooted(close, &[])?;
            }
            web::resolved_undefined(c)
        }
        Member::SourceClose => {
            let [controller] = captured(c)?;
            let controller_state = slots(c, controller)?;
            if byte::can_close(c, controller_state)? {
                byte::close(c, controller_state)?;
                let request = get(c, controller, "byobRequest")?;
                if !is_nullish(c, request) {
                    let respond = get(c, request, "respond")?;
                    let zero = c.number(0.0);
                    c.call_rooted(respond, request, &[zero])?;
                }
            }
            Ok(c.undefined())
        }
        Member::Ignore => web::resolved_undefined(c),
        Member::ReadChunk => {
            let [state] = captured(c)?;
            let chunk = arg(c, args, 0);
            if !c.has_builtin_brand_rooted(chunk, quench_runtime::BuiltinBrand::Uint8Array)? {
                let promise = get(c, state, read::PROMISE)?;
                let error = c.type_error_rooted("Received non-Uint8Array chunk")?;
                c.reject_promise_rooted(promise, error)?;
                return Ok(c.undefined());
            }
            let chunks = get(c, state, read::CHUNKS)?;
            let mut values = c.array_like_values_rooted(chunks)?;
            values.push(chunk);
            let chunks = c.array_rooted(&values)?;
            put(c, state, read::CHUNKS, chunks)?;
            read_next(c, state)?;
            Ok(c.undefined())
        }
        Member::ReadClose => {
            let [state] = captured(c)?;
            finish(c, state)?;
            Ok(c.undefined())
        }
        Member::ReadError => {
            let [state] = captured(c)?;
            let promise = get(c, state, read::PROMISE)?;
            let error = arg(c, args, 0);
            web::fail(c, promise, error)?;
            Ok(c.undefined())
        }
    }
}
