//! `internal/streams/utils`: what a value is as a stream, and how far it has come; the
//! readable and writable accessors on the stream prototypes; abort-signal wiring.

use super::errors;
use super::state::{self, bits, change, writable as w};
use super::{arg, call_method, define_accessor, get, intrinsics, is_function, is_nullish, put, Context, Outcome, Slot};
use quench_runtime::{RootId, RootedError};

/// `true`, `false`, or Node's `null` for "not that kind of stream".
pub(crate) type Tri = Option<bool>;

fn has_function(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    is_function(c, value)
}

/// `object?.[name]`.
fn optional(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    if is_nullish(c, object) { Ok(c.undefined()) } else { get(c, object, name) }
}

fn is_false(c: &mut Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).and_then(|value| value.as_bool()) == Some(false)
}

fn as_bool(c: &Context<'_>, value: RootId) -> Option<bool> {
    c.rooted_value(value).and_then(|value| value.as_bool())
}

pub(crate) fn is_readable_node_stream(c: &mut Context<'_>, object: RootId, strict: bool) -> Result<bool, RootedError> {
    if !c.truthy_rooted(object)? || !has_function(c, object, "pipe")? || !has_function(c, object, "on")? {
        return Ok(false);
    }
    if strict && (!has_function(c, object, "pause")? || !has_function(c, object, "resume")?) {
        return Ok(false);
    }
    if super::truthy_member(c, object, "_writableState")? {
        let readable_state = get(c, object, "_readableState")?;
        let readable = optional(c, readable_state, "readable")?;
        if is_false(c, readable) || !c.truthy_rooted(readable_state)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn is_writable_node_stream(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    if !c.truthy_rooted(object)? || !has_function(c, object, "write")? || !has_function(c, object, "on")? {
        return Ok(false);
    }
    if super::truthy_member(c, object, "_readableState")? {
        let writable_state = get(c, object, "_writableState")?;
        let writable = optional(c, writable_state, "writable")?;
        return Ok(!is_false(c, writable));
    }
    Ok(true)
}

pub(crate) fn is_duplex_node_stream(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    Ok(c.truthy_rooted(object)?
        && has_function(c, object, "pipe")?
        && super::truthy_member(c, object, "_readableState")?
        && has_function(c, object, "on")?
        && has_function(c, object, "write")?)
}

pub(crate) fn is_node_stream(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    if !c.truthy_rooted(object)? {
        return Ok(false);
    }
    Ok(super::truthy_member(c, object, "_readableState")?
        || super::truthy_member(c, object, "_writableState")?
        || (has_function(c, object, "write")? && has_function(c, object, "on")?)
        || (has_function(c, object, "pipe")? && has_function(c, object, "on")?))
}

pub(crate) fn is_readable_stream(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    Ok(c.truthy_rooted(object)?
        && !is_node_stream(c, object)?
        && has_function(c, object, "pipeThrough")?
        && has_function(c, object, "getReader")?
        && has_function(c, object, "cancel")?)
}

pub(crate) fn is_writable_stream(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    Ok(c.truthy_rooted(object)?
        && !is_node_stream(c, object)?
        && has_function(c, object, "getWriter")?
        && has_function(c, object, "abort")?)
}

fn is_object_typed(c: &mut Context<'_>, value: RootId) -> bool {
    matches!(
        c.value_type_rooted(value),
        Some(quench_runtime::ValueType::Object | quench_runtime::ValueType::Null)
    )
}

pub(crate) fn is_transform_stream(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    if !c.truthy_rooted(object)? || is_node_stream(c, object)? {
        return Ok(false);
    }
    let readable = get(c, object, "readable")?;
    let writable = get(c, object, "writable")?;
    Ok(is_object_typed(c, readable) && is_object_typed(c, writable))
}

pub(crate) fn is_web_stream(c: &mut Context<'_>, object: RootId) -> Result<bool, RootedError> {
    Ok(is_readable_stream(c, object)? || is_writable_stream(c, object)? || is_transform_stream(c, object)?)
}

/// `isIterable(obj, isAsync)`: `None` accepts either protocol.
pub(crate) fn is_iterable(c: &mut Context<'_>, object: RootId, asynchronous: Option<bool>) -> Result<bool, RootedError> {
    if is_nullish(c, object) {
        return Ok(false);
    }
    let intrinsics = intrinsics(c)?;
    let protocol = |c: &mut Context<'_>, slot: Slot| -> Result<bool, RootedError> {
        let value = c.get_property_rooted(object, intrinsics.slot(slot))?;
        is_function(c, value)
    };
    match asynchronous {
        Some(true) => protocol(c, Slot::AsyncIterator),
        Some(false) => protocol(c, Slot::Iterator),
        None => Ok(protocol(c, Slot::AsyncIterator)? || protocol(c, Slot::Iterator)?),
    }
}

/// `w || r` of a stream.
fn primary_state(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let writable = get(c, stream, "_writableState")?;
    if c.truthy_rooted(writable)? { Ok(writable) } else { get(c, stream, "_readableState") }
}

pub(crate) fn is_destroyed(c: &mut Context<'_>, stream: RootId) -> Result<Tri, RootedError> {
    if !is_node_stream(c, stream)? {
        return Ok(None);
    }
    let state = primary_state(c, stream)?;
    let key = intrinsics(c)?.slot(Slot::IsDestroyed);
    let marked = c.get_property_rooted(stream, key)?;
    let state_destroyed = optional(c, state, "destroyed")?;
    Ok(Some(super::truthy_member(c, stream, "destroyed")? || c.truthy_rooted(marked)? || c.truthy_rooted(state_destroyed)?))
}

pub(crate) fn is_writable_ended(c: &mut Context<'_>, stream: RootId) -> Result<Tri, RootedError> {
    if !is_writable_node_stream(c, stream)? {
        return Ok(None);
    }
    let ended = get(c, stream, "writableEnded")?;
    if as_bool(c, ended) == Some(true) {
        return Ok(Some(true));
    }
    let state = get(c, stream, "_writableState")?;
    let errored = optional(c, state, "errored")?;
    if c.truthy_rooted(errored)? {
        return Ok(Some(false));
    }
    let ended = optional(c, state, "ended")?;
    Ok(as_bool(c, ended))
}

pub(crate) fn is_writable_finished(c: &mut Context<'_>, stream: RootId, strict: Option<bool>) -> Result<Tri, RootedError> {
    if !is_writable_node_stream(c, stream)? {
        return Ok(None);
    }
    let finished = get(c, stream, "writableFinished")?;
    if as_bool(c, finished) == Some(true) {
        return Ok(Some(true));
    }
    let state = get(c, stream, "_writableState")?;
    let errored = optional(c, state, "errored")?;
    if c.truthy_rooted(errored)? {
        return Ok(Some(false));
    }
    let finished = optional(c, state, "finished")?;
    let Some(finished) = as_bool(c, finished) else { return Ok(None) };
    if finished {
        return Ok(Some(true));
    }
    let ended = get(c, state, "ended")?;
    let length = get(c, state, "length")?;
    Ok(Some(
        strict == Some(false) && as_bool(c, ended) == Some(true) && c.rooted_value(length).and_then(|v| v.as_number()) == Some(0.0),
    ))
}

pub(crate) fn is_readable_ended(c: &mut Context<'_>, stream: RootId) -> Result<Tri, RootedError> {
    if !is_readable_node_stream(c, stream, false)? {
        return Ok(None);
    }
    let ended = get(c, stream, "readableEnded")?;
    if as_bool(c, ended) == Some(true) {
        return Ok(Some(true));
    }
    let state = get(c, stream, "_readableState")?;
    if !c.truthy_rooted(state)? || super::truthy_member(c, state, "errored")? {
        return Ok(Some(false));
    }
    let ended = get(c, state, "ended")?;
    Ok(as_bool(c, ended))
}

pub(crate) fn is_readable_finished(c: &mut Context<'_>, stream: RootId, strict: Option<bool>) -> Result<Tri, RootedError> {
    if !is_readable_node_stream(c, stream, false)? {
        return Ok(None);
    }
    let state = get(c, stream, "_readableState")?;
    let errored = optional(c, state, "errored")?;
    if c.truthy_rooted(errored)? {
        return Ok(Some(false));
    }
    let emitted = optional(c, state, "endEmitted")?;
    let Some(emitted) = as_bool(c, emitted) else { return Ok(None) };
    if emitted {
        return Ok(Some(true));
    }
    let ended = get(c, state, "ended")?;
    let length = get(c, state, "length")?;
    Ok(Some(
        strict == Some(false) && as_bool(c, ended) == Some(true) && c.rooted_value(length).and_then(|v| v.as_number()) == Some(0.0),
    ))
}

/// `stream[symbol] != null ? stream[symbol]` for an `isReadable`/`isWritable` override.
fn marked(c: &mut Context<'_>, stream: RootId, slot: Slot) -> Result<Option<RootId>, RootedError> {
    if !c.truthy_rooted(stream)? {
        return Ok(None);
    }
    let key = intrinsics(c)?.slot(slot);
    let value = c.get_property_rooted(stream, key)?;
    Ok((!is_nullish(c, value)).then_some(value))
}

/// `isReadable(stream)`: `Some` of the override's truthiness when it is no boolean.
pub(crate) fn is_readable(c: &mut Context<'_>, stream: RootId) -> Result<Tri, RootedError> {
    if let Some(value) = marked(c, stream, Slot::IsReadable)? {
        return Ok(Some(c.truthy_rooted(value)?));
    }
    let readable = optional(c, stream, "readable")?;
    if as_bool(c, readable).is_none() {
        return Ok(None);
    }
    if is_destroyed(c, stream)? == Some(true) {
        return Ok(Some(false));
    }
    Ok(Some(
        is_readable_node_stream(c, stream, false)?
            && super::truthy_member(c, stream, "readable")?
            && is_readable_finished(c, stream, None)? != Some(true),
    ))
}

pub(crate) fn is_writable(c: &mut Context<'_>, stream: RootId) -> Result<Tri, RootedError> {
    if let Some(value) = marked(c, stream, Slot::IsWritable)? {
        return Ok(Some(c.truthy_rooted(value)?));
    }
    let writable = optional(c, stream, "writable")?;
    if as_bool(c, writable).is_none() {
        return Ok(None);
    }
    if is_destroyed(c, stream)? == Some(true) {
        return Ok(Some(false));
    }
    Ok(Some(
        is_writable_node_stream(c, stream)?
            && super::truthy_member(c, stream, "writable")?
            && is_writable_ended(c, stream)? != Some(true),
    ))
}

/// `isFinished(stream, opts)`; `options` gives `readable`/`writable` `false` to skip a half.
pub(crate) fn is_finished(c: &mut Context<'_>, stream: RootId, options: Option<RootId>) -> Result<Tri, RootedError> {
    if !is_node_stream(c, stream)? {
        return Ok(None);
    }
    if is_destroyed(c, stream)? == Some(true) {
        return Ok(Some(true));
    }
    let skips = |c: &mut Context<'_>, name: &str| -> Result<bool, RootedError> {
        match options {
            Some(options) => {
                let value = optional(c, options, name)?;
                Ok(is_false(c, value))
            }
            None => Ok(false),
        }
    };
    if !skips(c, "readable")? && is_readable(c, stream)? == Some(true) {
        return Ok(Some(false));
    }
    if !skips(c, "writable")? && is_writable(c, stream)? == Some(true) {
        return Ok(Some(false));
    }
    Ok(Some(true))
}

/// `isWritableErrored(stream)`: the error (or `null`).
pub(crate) fn writable_errored(c: &mut Context<'_>, stream: RootId) -> Outcome {
    half_errored(c, stream, "writableErrored", "_writableState")
}

pub(crate) fn readable_errored(c: &mut Context<'_>, stream: RootId) -> Outcome {
    half_errored(c, stream, "readableErrored", "_readableState")
}

fn half_errored(c: &mut Context<'_>, stream: RootId, own: &str, state: &str) -> Outcome {
    if !is_node_stream(c, stream)? {
        return Ok(c.null());
    }
    let errored = get(c, stream, own)?;
    if c.truthy_rooted(errored)? {
        return Ok(errored);
    }
    let state = get(c, stream, state)?;
    let errored = optional(c, state, "errored")?;
    if is_nullish(c, errored) { Ok(c.null()) } else { Ok(errored) }
}

/// `isClosed(stream)`: a boolean, or `null` when the stream does not say.
pub(crate) fn is_closed(c: &mut Context<'_>, stream: RootId) -> Outcome {
    if !is_node_stream(c, stream)? {
        return Ok(c.null());
    }
    let closed = get(c, stream, "closed")?;
    if as_bool(c, closed).is_some() {
        return Ok(closed);
    }
    let writable_state = get(c, stream, "_writableState")?;
    let readable_state = get(c, stream, "_readableState")?;
    let writable_closed = optional(c, writable_state, "closed")?;
    let readable_closed = optional(c, readable_state, "closed")?;
    if as_bool(c, writable_closed).is_some() || as_bool(c, readable_closed).is_some() {
        return Ok(if c.truthy_rooted(writable_closed)? { writable_closed } else { readable_closed });
    }
    let own = get(c, stream, "_closed")?;
    if as_bool(c, own).is_some() && is_outgoing_message(c, stream)? {
        return Ok(own);
    }
    Ok(c.null())
}

fn is_boolean_member(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    Ok(as_bool(c, value).is_some())
}

fn is_outgoing_message(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    for name in ["_closed", "_defaultKeepAlive", "_removedConnection", "_removedContLen"] {
        if !is_boolean_member(c, stream, name)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn is_server_response(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    Ok(is_boolean_member(c, stream, "_sent100")? && is_outgoing_message(c, stream)?)
}

pub(crate) fn is_server_request(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    if !is_boolean_member(c, stream, "_consuming")? || !is_boolean_member(c, stream, "_dumped")? {
        return Ok(false);
    }
    let request = get(c, stream, "req")?;
    let upgrade = optional(c, request, "upgradeOrConnect")?;
    Ok(super::is_undefined(c, upgrade))
}

/// `isRequest(stream)`: an outgoing client request.
pub(crate) fn is_request(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    if is_nullish(c, stream) || !super::truthy_member(c, stream, "setHeader")? {
        return Ok(false);
    }
    has_function(c, stream, "abort")
}

pub(crate) fn will_emit_close(c: &mut Context<'_>, stream: RootId) -> Result<Tri, RootedError> {
    if !is_node_stream(c, stream)? {
        return Ok(None);
    }
    let state = primary_state(c, stream)?;
    if !c.truthy_rooted(state)? {
        return Ok(Some(is_server_response(c, stream)?));
    }
    let closed = get(c, state, "closed")?;
    Ok(Some(super::truthy_member(c, state, "autoDestroy")? && super::truthy_member(c, state, "emitClose")? && is_false(c, closed)))
}

/// The first of `candidates` that is not nullish, as `a ?? b ?? ...` reads them.
fn first_defined(c: &mut Context<'_>, stream: RootId, candidates: &[(Option<&str>, &str)]) -> Outcome {
    for &(state, name) in candidates {
        let holder = match state {
            Some(state) => get(c, stream, state)?,
            None => stream,
        };
        let value = optional(c, holder, name)?;
        if !is_nullish(c, value) {
            return Ok(value);
        }
    }
    Ok(c.undefined())
}

pub(crate) fn is_disturbed(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    if !c.truthy_rooted(stream)? {
        return Ok(false);
    }
    let key = intrinsics(c)?.slot(Slot::IsDisturbed);
    let marked = c.get_property_rooted(stream, key)?;
    if !is_nullish(c, marked) {
        return c.truthy_rooted(marked);
    }
    Ok(super::truthy_member(c, stream, "readableDidRead")? || super::truthy_member(c, stream, "readableAborted")?)
}

pub(crate) fn is_errored(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    if !c.truthy_rooted(stream)? {
        return Ok(false);
    }
    let key = intrinsics(c)?.slot(Slot::IsErrored);
    let marked = c.get_property_rooted(stream, key)?;
    if !is_nullish(c, marked) {
        return c.truthy_rooted(marked);
    }
    let value = first_defined(
        c,
        stream,
        &[
            (None, "readableErrored"),
            (None, "writableErrored"),
            (Some("_readableState"), "errorEmitted"),
            (Some("_writableState"), "errorEmitted"),
            (Some("_readableState"), "errored"),
            (Some("_writableState"), "errored"),
        ],
    )?;
    c.truthy_rooted(value)
}

/// A predicate `internal/streams/utils` exports, called through one dispatching operation.
#[derive(Clone, Copy)]
enum Predicate {
    IsDestroyed,
    IsDisturbed,
    IsErrored,
    IsReadable,
    IsWritable,
    IsFinished,
    IsIterable,
    IsReadableNodeStream,
    IsWritableNodeStream,
    IsDuplexNodeStream,
    IsNodeStream,
    IsReadableEnded,
    IsReadableFinished,
    IsReadableErrored,
    IsWritableEnded,
    IsWritableFinished,
    IsWritableErrored,
    IsClosed,
    WillEmitClose,
    IsServerRequest,
    IsServerResponse,
    IsReadableStream,
    IsWritableStream,
    IsTransformStream,
    IsWebStream,
}

const PREDICATES: &[(&str, Predicate)] = &[
    ("isDestroyed", Predicate::IsDestroyed),
    ("isDisturbed", Predicate::IsDisturbed),
    ("isErrored", Predicate::IsErrored),
    ("isReadable", Predicate::IsReadable),
    ("isWritable", Predicate::IsWritable),
    ("isFinished", Predicate::IsFinished),
    ("isIterable", Predicate::IsIterable),
    ("isReadableNodeStream", Predicate::IsReadableNodeStream),
    ("isWritableNodeStream", Predicate::IsWritableNodeStream),
    ("isDuplexNodeStream", Predicate::IsDuplexNodeStream),
    ("isNodeStream", Predicate::IsNodeStream),
    ("isReadableEnded", Predicate::IsReadableEnded),
    ("isReadableFinished", Predicate::IsReadableFinished),
    ("isReadableErrored", Predicate::IsReadableErrored),
    ("isWritableEnded", Predicate::IsWritableEnded),
    ("isWritableFinished", Predicate::IsWritableFinished),
    ("isWritableErrored", Predicate::IsWritableErrored),
    ("isClosed", Predicate::IsClosed),
    ("willEmitClose", Predicate::WillEmitClose),
    ("isServerRequest", Predicate::IsServerRequest),
    ("isServerResponse", Predicate::IsServerResponse),
    ("isReadableStream", Predicate::IsReadableStream),
    ("isWritableStream", Predicate::IsWritableStream),
    ("isTransformStream", Predicate::IsTransformStream),
    ("isWebStream", Predicate::IsWebStream),
];

/// The predicates `stream` itself exports.
pub(crate) const PUBLIC_PREDICATES: &[&str] = &["isDestroyed", "isDisturbed", "isErrored", "isReadable", "isWritable"];

/// Installs predicates by name, each a function over `streamPredicate`.
pub(crate) fn define_predicates(c: &mut Context<'_>, target: RootId, names: &[&str]) -> Result<(), RootedError> {
    for name in names {
        let index = PREDICATES.iter().position(|(candidate, _)| candidate == name).expect("listed predicate");
        let data = c.number(index as f64);
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation("streamPredicate"), name, data)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(target, key, function, true, true, true)?;
    }
    Ok(())
}

fn tri(c: &mut Context<'_>, value: Tri) -> RootId {
    match value {
        Some(value) => c.boolean(value),
        None => c.null(),
    }
}

fn strictness(c: &mut Context<'_>, value: RootId) -> Option<bool> {
    as_bool(c, value)
}

/// The dispatching operation behind every exported predicate.
pub(crate) fn predicate(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let index = c.to_number(data)? as usize;
    let (subject, second) = (arg(c, args, 0), arg(c, args, 1));
    let boolean = |c: &mut Context<'_>, value: bool| Ok(c.boolean(value));
    match PREDICATES[index].1 {
        Predicate::IsDestroyed => Ok(is_destroyed(c, subject).map(|value| tri(c, value))?),
        Predicate::IsDisturbed => {
            let value = is_disturbed(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsErrored => {
            let value = is_errored(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsReadable => Ok(is_readable(c, subject).map(|value| tri(c, value))?),
        Predicate::IsWritable => Ok(is_writable(c, subject).map(|value| tri(c, value))?),
        Predicate::IsFinished => {
            let options = (!super::is_undefined(c, second)).then_some(second);
            Ok(is_finished(c, subject, options).map(|value| tri(c, value))?)
        }
        Predicate::IsIterable => {
            let asynchronous = as_bool(c, second);
            let value = is_iterable(c, subject, asynchronous)?;
            boolean(c, value)
        }
        Predicate::IsReadableNodeStream => {
            let strict = c.truthy_rooted(second)?;
            let value = is_readable_node_stream(c, subject, strict)?;
            boolean(c, value)
        }
        Predicate::IsWritableNodeStream => {
            let value = is_writable_node_stream(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsDuplexNodeStream => {
            let value = is_duplex_node_stream(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsNodeStream => {
            let value = is_node_stream(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsReadableEnded => Ok(is_readable_ended(c, subject).map(|value| tri(c, value))?),
        Predicate::IsReadableFinished => {
            let strict = strictness(c, second);
            Ok(is_readable_finished(c, subject, strict).map(|value| tri(c, value))?)
        }
        Predicate::IsReadableErrored => readable_errored(c, subject),
        Predicate::IsWritableEnded => Ok(is_writable_ended(c, subject).map(|value| tri(c, value))?),
        Predicate::IsWritableFinished => {
            let strict = strictness(c, second);
            Ok(is_writable_finished(c, subject, strict).map(|value| tri(c, value))?)
        }
        Predicate::IsWritableErrored => writable_errored(c, subject),
        Predicate::IsClosed => is_closed(c, subject),
        Predicate::WillEmitClose => Ok(will_emit_close(c, subject).map(|value| tri(c, value))?),
        Predicate::IsServerRequest => {
            let value = is_server_request(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsServerResponse => {
            let value = is_server_response(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsReadableStream => {
            let value = is_readable_stream(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsWritableStream => {
            let value = is_writable_stream(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsTransformStream => {
            let value = is_transform_stream(c, subject)?;
            boolean(c, value)
        }
        Predicate::IsWebStream => {
            let value = is_web_stream(c, subject)?;
            boolean(c, value)
        }
    }
}

/// `internal/streams/utils`' predicates and constants.
pub(crate) fn define_utils_exports(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let names: Vec<&str> = PREDICATES.iter().map(|(name, _)| *name).collect();
    define_predicates(c, module, &names)?;
    let intrinsics = intrinsics(c)?;
    for (name, slot) in [
        ("kOnConstructed", Slot::OnConstructed),
        ("kIsDestroyed", Slot::IsDestroyed),
        ("kIsDisturbed", Slot::IsDisturbed),
        ("kIsErrored", Slot::IsErrored),
        ("kIsReadable", Slot::IsReadable),
        ("kIsWritable", Slot::IsWritable),
        ("kState", Slot::State),
    ] {
        put(c, module, name, intrinsics.slot(slot))?;
    }
    for (name, bit) in [
        ("kObjectMode", state::OBJECT_MODE),
        ("kErrorEmitted", state::ERROR_EMITTED),
        ("kAutoDestroy", state::AUTO_DESTROY),
        ("kEmitClose", state::EMIT_CLOSE),
        ("kDestroyed", state::DESTROYED),
        ("kClosed", state::CLOSED),
        ("kCloseEmitted", state::CLOSE_EMITTED),
        ("kErrored", state::ERRORED),
        ("kConstructed", state::CONSTRUCTED),
    ] {
        super::put_number(c, module, name, f64::from(bit))?;
    }
    Ok(())
}

pub(crate) fn registered_symbol(c: &mut Context<'_>, key: &str) -> Outcome {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = c.string_rooted(key);
    call_method(c, symbol, "for", &[key])
}

/// `addAbortListener(signal, listener)` from `events`: the disposable that removes it.
pub(crate) fn add_abort_listener(c: &mut Context<'_>, signal: RootId, listener: RootId) -> Outcome {
    let events = crate::host::shared_vm::commonjs::events_module(c)?;
    let add = get(c, events, "addAbortListener")?;
    let undefined = c.undefined();
    c.call_rooted(add, undefined, &[signal, listener])
}

/// `typeof signal === 'object' && 'aborted' in signal`.
fn is_signal_shaped(c: &mut Context<'_>, signal: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(signal)? || is_function(c, signal)? {
        return Ok(false);
    }
    let key = c.string_rooted("aborted");
    c.has_property_rooted(signal, key)
}

/// `addAbortSignal(signal, stream)`, validated.
pub(crate) fn add_abort_signal(c: &mut Context<'_>, signal: RootId, stream: RootId) -> Outcome {
    if !is_signal_shaped(c, signal)? {
        return Err(super::super::util_shared_vm::argument_type_error(c, signal, "signal", "AbortSignal")?);
    }
    if !is_node_stream(c, stream)? && !is_web_stream(c, stream)? {
        let error = errors::invalid_argument_type(c, stream, "stream", "an instance of ReadableStream, WritableStream, or Stream")?;
        return Err(c.throw(error));
    }
    add_abort_signal_unchecked(c, signal, stream)
}

/// `addAbortSignalNoValidate(signal, stream)`: destroy the stream when the signal aborts.
fn add_abort_signal_unchecked(c: &mut Context<'_>, signal: RootId, stream: RootId) -> Outcome {
    if !is_signal_shaped(c, signal)? {
        return Ok(stream);
    }
    let node = is_node_stream(c, stream)?;
    let node = c.boolean(node);
    let on_abort = super::closure(c, "streamOnSignalAbort", "", &[stream, signal, node])?;
    let aborted = get(c, signal, "aborted")?;
    let undefined = c.undefined();
    if c.truthy_rooted(aborted)? {
        c.call_rooted(on_abort, undefined, &[])?;
    } else {
        let disposable = add_abort_listener(c, signal, on_abort)?;
        let key = intrinsics(c)?.slot(Slot::Dispose);
        let dispose = c.get_property_rooted(disposable, key)?;
        super::eos::eos(c, stream, undefined, dispose)?;
    }
    Ok(stream)
}

pub(crate) fn add_abort_signal_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (signal, stream) = (arg(c, args, 0), arg(c, args, 1));
    add_abort_signal(c, signal, stream)
}

pub(crate) fn add_abort_signal_unchecked_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (signal, stream) = (arg(c, args, 0), arg(c, args, 1));
    add_abort_signal_unchecked(c, signal, stream)
}

pub(crate) fn on_signal_abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [stream, signal, node] = super::captures(c)?;
    let error = errors::abort_error_for(c, signal)?;
    if c.truthy_rooted(node)? {
        call_method(c, stream, "destroy", &[error])?;
    } else {
        let key = registered_symbol(c, "nodejs.webstream.controllerErrorFunction")?;
        let fail = c.get_property_rooted(stream, key)?;
        c.call_rooted(fail, stream, &[error])?;
    }
    Ok(c.undefined())
}

/// A readable or writable accessor on a stream prototype.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Accessor {
    Readable,
    ReadableDidRead,
    ReadableAborted,
    ReadableHighWaterMark,
    ReadableBuffer,
    ReadableFlowing,
    ReadableLength,
    ReadableObjectMode,
    ReadableEncoding,
    ReadableErrored,
    ReadableClosed,
    ReadableDestroyed,
    ReadableEnded,
    WritableClosed,
    WritableDestroyed,
    Writable,
    WritableFinished,
    WritableObjectMode,
    WritableBuffer,
    WritableEnded,
    WritableNeedDrain,
    WritableHighWaterMark,
    WritableCorked,
    WritableLength,
    WritableErrored,
    WritableAborted,
    DuplexDestroyed,
}

/// `(name, accessor, whether it has a setter)`.
type AccessorRow = (&'static str, Accessor, bool);

pub(crate) const READABLE_ACCESSORS: &[AccessorRow] = &[
    ("readable", Accessor::Readable, true),
    ("readableDidRead", Accessor::ReadableDidRead, false),
    ("readableAborted", Accessor::ReadableAborted, false),
    ("readableHighWaterMark", Accessor::ReadableHighWaterMark, false),
    ("readableBuffer", Accessor::ReadableBuffer, false),
    ("readableFlowing", Accessor::ReadableFlowing, true),
    ("readableLength", Accessor::ReadableLength, false),
    ("readableObjectMode", Accessor::ReadableObjectMode, false),
    ("readableEncoding", Accessor::ReadableEncoding, false),
    ("errored", Accessor::ReadableErrored, false),
    ("closed", Accessor::ReadableClosed, false),
    ("destroyed", Accessor::ReadableDestroyed, true),
    ("readableEnded", Accessor::ReadableEnded, false),
];

pub(crate) const WRITABLE_ACCESSORS: &[AccessorRow] = &[
    ("closed", Accessor::WritableClosed, false),
    ("destroyed", Accessor::WritableDestroyed, true),
    ("writable", Accessor::Writable, true),
    ("writableFinished", Accessor::WritableFinished, false),
    ("writableObjectMode", Accessor::WritableObjectMode, false),
    ("writableBuffer", Accessor::WritableBuffer, false),
    ("writableEnded", Accessor::WritableEnded, false),
    ("writableNeedDrain", Accessor::WritableNeedDrain, false),
    ("writableHighWaterMark", Accessor::WritableHighWaterMark, false),
    ("writableCorked", Accessor::WritableCorked, false),
    ("writableLength", Accessor::WritableLength, false),
    ("errored", Accessor::WritableErrored, false),
    ("writableAborted", Accessor::WritableAborted, false),
];

pub(crate) const DUPLEX_ACCESSORS: &[AccessorRow] = &[("destroyed", Accessor::DuplexDestroyed, true)];

fn all_accessors() -> impl Iterator<Item = &'static AccessorRow> {
    READABLE_ACCESSORS.iter().chain(WRITABLE_ACCESSORS).chain(DUPLEX_ACCESSORS)
}

pub(crate) fn define_stream_accessors(c: &mut Context<'_>, prototype: RootId, rows: &[AccessorRow]) -> Result<(), RootedError> {
    for &(name, accessor, settable) in rows {
        let index = all_accessors()
            .position(|(_, kind, _)| *kind == accessor)
            .expect("listed accessor");
        let data = c.number(index as f64);
        let getter = c.host_function_named_with_data(crate::host::shared_vm::operation("streamAccessorGet"), "get", data)?;
        let setter = if settable {
            Some(c.host_function_named_with_data(crate::host::shared_vm::operation("streamAccessorSet"), "set", data)?)
        } else {
            None
        };
        define_accessor(c, prototype, name, getter, setter)?;
    }
    Ok(())
}

fn accessor(c: &mut Context<'_>) -> Result<Accessor, RootedError> {
    let data = c.host_function_data()?;
    let index = c.to_number(data)? as usize;
    Ok(all_accessors().nth(index).expect("accessor index").1)
}

/// `state ? state[name] : absent`.
fn state_member(c: &mut Context<'_>, state: RootId, name: &str, absent: RootId) -> Outcome {
    if c.truthy_rooted(state)? { get(c, state, name) } else { Ok(absent) }
}

pub(crate) fn accessor_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let accessor = accessor(c)?;
    let r = get(c, this, "_readableState")?;
    let no = c.boolean(false);
    let null = c.null();
    let w_state = || "_writableState";
    let writable_bits = |c: &mut Context<'_>| -> Result<Option<u32>, RootedError> {
        let state = get(c, this, w_state())?;
        if c.truthy_rooted(state)? { Ok(Some(bits(c, state)?)) } else { Ok(None) }
    };
    let flag = |c: &mut Context<'_>, mask: u32| -> Outcome {
        let value = writable_bits(c)?.is_some_and(|field| field & mask != 0);
        Ok(c.boolean(value))
    };
    match accessor {
        Accessor::Readable => {
            let readable = if c.truthy_rooted(r)? {
                let flag = get(c, r, "readable")?;
                !c.strict_equal_rooted(flag, no)?
                    && !super::truthy_member(c, r, "destroyed")?
                    && !super::truthy_member(c, r, "errorEmitted")?
                    && !super::truthy_member(c, r, "endEmitted")?
            } else {
                false
            };
            Ok(c.boolean(readable))
        }
        Accessor::ReadableDidRead => get(c, r, "dataEmitted"),
        Accessor::ReadableAborted => {
            let flag = get(c, r, "readable")?;
            let aborted = !c.strict_equal_rooted(flag, no)?
                && (super::truthy_member(c, r, "destroyed")? || super::truthy_member(c, r, "errored")?)
                && !super::truthy_member(c, r, "endEmitted")?;
            Ok(c.boolean(aborted))
        }
        Accessor::ReadableHighWaterMark => get(c, r, "highWaterMark"),
        Accessor::ReadableBuffer => optional(c, r, "buffer"),
        Accessor::ReadableFlowing => get(c, r, "flowing"),
        Accessor::ReadableLength => get(c, r, "length"),
        Accessor::ReadableObjectMode => state_member(c, r, "objectMode", no),
        Accessor::ReadableEncoding => state_member(c, r, "encoding", null),
        Accessor::ReadableErrored => state_member(c, r, "errored", null),
        Accessor::ReadableClosed => state_member(c, r, "closed", no),
        Accessor::ReadableDestroyed => state_member(c, r, "destroyed", no),
        Accessor::ReadableEnded => state_member(c, r, "endEmitted", no),
        Accessor::WritableClosed => flag(c, state::CLOSED),
        Accessor::WritableDestroyed => flag(c, state::DESTROYED),
        Accessor::Writable => {
            let state = get(c, this, w_state())?;
            let writable = if c.truthy_rooted(state)? {
                let flag = get(c, state, "writable")?;
                !c.strict_equal_rooted(flag, no)? && bits(c, state)? & (w::ENDING | w::ENDED | state::DESTROYED | state::ERRORED) == 0
            } else {
                false
            };
            Ok(c.boolean(writable))
        }
        Accessor::WritableFinished => flag(c, w::FINISHED),
        Accessor::WritableObjectMode => flag(c, state::OBJECT_MODE),
        Accessor::WritableBuffer => {
            let state = get(c, this, w_state())?;
            if c.truthy_rooted(state)? { call_method(c, state, "getBuffer", &[]) } else { Ok(state) }
        }
        Accessor::WritableEnded => flag(c, w::ENDING),
        Accessor::WritableNeedDrain => {
            let value = writable_bits(c)?.is_some_and(|field| field & (state::DESTROYED | w::ENDING | w::NEED_DRAIN) == w::NEED_DRAIN);
            Ok(c.boolean(value))
        }
        Accessor::WritableHighWaterMark => {
            let state = get(c, this, w_state())?;
            optional(c, state, "highWaterMark")
        }
        Accessor::WritableCorked => {
            let state = get(c, this, w_state())?;
            let zero = c.number(0.0);
            state_member(c, state, "corked", zero)
        }
        Accessor::WritableLength => {
            let state = get(c, this, w_state())?;
            optional(c, state, "length")
        }
        Accessor::WritableErrored => {
            let state = get(c, this, w_state())?;
            state_member(c, state, "errored", null)
        }
        Accessor::WritableAborted => {
            let state = get(c, this, w_state())?;
            let field = bits(c, state)?;
            let aborted = field & (w::HAS_WRITABLE | w::WRITABLE) != w::HAS_WRITABLE
                && field & (state::DESTROYED | state::ERRORED) != 0
                && field & w::FINISHED == 0;
            Ok(c.boolean(aborted))
        }
        Accessor::DuplexDestroyed => {
            let state = get(c, this, w_state())?;
            if super::is_undefined(c, r) || super::is_undefined(c, state) {
                return Ok(no);
            }
            let readable_destroyed = get(c, r, "destroyed")?;
            if c.truthy_rooted(readable_destroyed)? { get(c, state, "destroyed") } else { Ok(readable_destroyed) }
        }
    }
}

pub(crate) fn accessor_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let accessor = accessor(c)?;
    let value = arg(c, args, 0);
    let r = get(c, this, "_readableState")?;
    let state = get(c, this, "_writableState")?;
    match accessor {
        Accessor::Readable if c.truthy_rooted(r)? => {
            let flag = c.truthy_rooted(value)?;
            super::put_bool(c, r, "readable", flag)?;
        }
        Accessor::ReadableFlowing if c.truthy_rooted(r)? => put(c, r, "flowing", value)?,
        Accessor::ReadableDestroyed if c.truthy_rooted(r)? => put(c, r, "destroyed", value)?,
        Accessor::WritableDestroyed if c.truthy_rooted(state)? => {
            if c.truthy_rooted(value)? { change(c, state, state::DESTROYED, 0)? } else { change(c, state, 0, state::DESTROYED)? };
        }
        Accessor::Writable if c.truthy_rooted(state)? => {
            let flag = c.truthy_rooted(value)?;
            super::put_bool(c, state, "writable", flag)?;
        }
        Accessor::DuplexDestroyed if c.truthy_rooted(r)? && c.truthy_rooted(state)? => {
            put(c, r, "destroyed", value)?;
            put(c, state, "destroyed", value)?;
        }
        _ => {}
    }
    Ok(c.undefined())
}
