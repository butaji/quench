//! `stream/consumers` (lib/stream/consumers.js): collect an async iterable (a web or Node
//! stream, an async generator or any iterable) into a Blob, an ArrayBuffer, a Buffer,
//! bytes, a string or parsed JSON. Each consumer is Node's `for await` loop as a state
//! machine: one step per `next()` result, the iterator closed when a chunk fails.

use crate::host::NodeHost;
use crate::modules::host_members::{captured, current, members};
use crate::modules::webstreams_shared_vm::{arg, deferred, get, is_nullish, literal, put, record, resolved, upon};
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Member, "streamConsumers", [
    ArrayBuffer,
    Blob,
    Buffer,
    Bytes,
    Json,
    Text,
    Stepped,
    SyncValue,
    Failed,
    Packaged,
]);

/// What a consumer makes, in the module's export order.
const CONSUMERS: [(&str, Member); 6] = [
    ("arrayBuffer", Member::ArrayBuffer),
    ("blob", Member::Blob),
    ("buffer", Member::Buffer),
    ("bytes", Member::Bytes),
    ("text", Member::Text),
    ("json", Member::Json),
];

const NOT_ASYNC_ITERABLE: &str = "stream is not async iterable";

/// A consumption's state.
mod field {
    pub(crate) use crate::modules::keys::PROMISE;
    pub(crate) use crate::modules::keys::ITERATOR;
    pub(crate) use crate::modules::keys::NEXT;
    pub(crate) use crate::modules::keys::SYNC;
    pub(crate) use crate::modules::keys::KIND;
    pub(crate) use crate::modules::keys::CHUNKS;
    pub(super) const TEXT: &str = "text";
    pub(super) const DECODER: &str = "decoder";
}
use field::*;

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let module = c.object_rooted()?;
    for (name, member) in CONSUMERS {
        let function = crate::modules::webstreams_shared_vm::method_function(c, member, name, 1)?;
        put(c, module, name, function)?;
    }
    Ok(module)
}

fn kind_index(member: Member) -> f64 {
    CONSUMERS.iter().position(|(_, candidate)| *candidate == member).unwrap_or(0) as f64
}

fn kind_of(c: &mut Context<'_>, state: RootId) -> Result<Member, RootedError> {
    let index = crate::modules::webstreams_shared_vm::num(c, state, KIND)? as usize;
    Ok(CONSUMERS[index].1)
}

fn symbol(c: &mut Context<'_>, name: &str) -> Outcome {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    get(c, symbol, name)
}

/// Starts the `for await` over `stream` for the consumer `member`.
fn consume(c: &mut Context<'_>, member: Member, stream: RootId) -> Outcome {
    let promise = deferred(c)?;
    match crate::modules::webstreams_shared_vm::attempt(begin(c, member, stream, promise))? {
        Ok(()) => {}
        Err(error) => c.reject_promise_rooted(promise, error)?,
    }
    Ok(promise)
}

fn begin(c: &mut Context<'_>, member: Member, stream: RootId, promise: RootId) -> Result<(), RootedError> {
    // V8 names the operand by its source text, Node's parameter `stream`.
    let not_iterable = |c: &mut Context<'_>| -> RootedError {
        match c.type_error_rooted(NOT_ASYNC_ITERABLE) {
            Ok(error) => c.throw(error),
            Err(error) => error,
        }
    };
    if is_nullish(c, stream) {
        return Err(not_iterable(c));
    }
    let async_key = symbol(c, "asyncIterator")?;
    let method = c.get_property_rooted(stream, async_key)?;
    let (method, sync) = if is_nullish(c, method) {
        let sync_key = symbol(c, "iterator")?;
        let method = c.get_property_rooted(stream, sync_key)?;
        if !c.is_callable_rooted(method)? {
            return Err(not_iterable(c));
        }
        (method, true)
    } else {
        (method, false)
    };
    let iterator = c.call_rooted(method, stream, &[])?;
    if !c.is_object_rooted(iterator)? {
        let error = c.type_error_rooted("Result of the Symbol.asyncIterator method is not an object")?;
        return Err(c.throw(error));
    }
    let next = get(c, iterator, "next")?;
    let sync = c.boolean(sync);
    let kind = c.number(kind_index(member));
    let chunks = c.array_rooted(&[])?;
    let text = c.string_rooted("");
    let state = record(c, &[PROMISE, ITERATOR, NEXT, SYNC, KIND, CHUNKS, TEXT], &[promise, iterator, next, sync, kind, chunks, text])?;
    if matches!(member, Member::Text | Member::Json) {
        let global = c.global_root()?;
        let decoder = get(c, global, "TextDecoder")?;
        let decoder = c.construct_rooted(decoder, decoder, &[])?;
        put(c, state, DECODER, decoder)?;
    }
    step(c, state)
}

/// One `await iterator.next()`.
fn step(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let iterator = get(c, state, ITERATOR)?;
    let next = get(c, state, NEXT)?;
    let result = c.call_rooted(next, iterator, &[])?;
    if crate::modules::webstreams_shared_vm::flag(c, state, SYNC)? {
        if !c.is_object_rooted(result)? {
            let error = c.type_error_rooted("Iterator result is not an object")?;
            return Err(c.throw(error));
        }
        let done = get(c, result, "done")?;
        let value = get(c, result, "value")?;
        let value = resolved(c, value)?;
        upon(c, value, Some(Member::SyncValue), Some(Member::Failed), &[state, done])?;
        return Ok(());
    }
    let result = resolved(c, result)?;
    upon(c, result, Some(Member::Stepped), Some(Member::Failed), &[state])?;
    Ok(())
}

fn fail(c: &mut Context<'_>, state: RootId, error: RootId) -> Result<(), RootedError> {
    let promise = get(c, state, PROMISE)?;
    crate::modules::webstreams_shared_vm::fail(c, promise, error)
}

/// The loop body for one chunk; a failure closes the iterator and rejects.
fn chunk(c: &mut Context<'_>, state: RootId, value: RootId) -> Result<(), RootedError> {
    let accepted = crate::modules::webstreams_shared_vm::attempt((|| {
        if kind_of(c, state)? == Member::Text || kind_of(c, state)? == Member::Json {
            let text = get(c, state, TEXT)?;
            let mut text = c.to_string_code_units(text)?;
            if matches!(c.value_type_rooted(value), Some(ValueType::String)) {
                text.extend(c.to_string_code_units(value)?);
            } else {
                let decoder = get(c, state, DECODER)?;
                let decode = get(c, decoder, "decode")?;
                let options = literal(c, &[], &[])?;
                let yes = c.boolean(true);
                put(c, options, "stream", yes)?;
                let decoded = c.call_rooted(decode, decoder, &[value, options])?;
                text.extend(c.to_string_code_units(decoded)?);
            }
            let text = c.string_units_rooted(&text);
            put(c, state, TEXT, text)
        } else {
            let chunks = get(c, state, CHUNKS)?;
            let mut values = c.array_like_values_rooted(chunks)?;
            values.push(value);
            let chunks = c.array_rooted(&values)?;
            put(c, state, CHUNKS, chunks)
        }
    })())?;
    match accepted {
        Ok(()) => step(c, state),
        Err(error) => {
            let iterator = get(c, state, ITERATOR)?;
            let close = get(c, iterator, "return")?;
            if c.is_callable_rooted(close)? {
                let _ = crate::modules::webstreams_shared_vm::attempt(c.call_rooted(close, iterator, &[]))?;
            }
            fail(c, state, error)
        }
    }
}

/// The loop ended: package what it gathered.
fn done(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let promise = get(c, state, PROMISE)?;
    let kind = kind_of(c, state)?;
    match kind {
        Member::Text | Member::Json => {
            let text = get(c, state, TEXT)?;
            let mut units = c.to_string_code_units(text)?;
            let decoder = get(c, state, DECODER)?;
            let decode = get(c, decoder, "decode")?;
            let tail = c.call_rooted(decode, decoder, &[])?;
            units.extend(c.to_string_code_units(tail)?);
            let text = c.string_units_rooted(&units);
            if kind == Member::Text {
                return c.resolve_promise_rooted(promise, text);
            }
            let global = c.global_root()?;
            let json = get(c, global, "JSON")?;
            let parse = get(c, json, "parse")?;
            match crate::modules::webstreams_shared_vm::attempt(c.call_rooted(parse, json, &[text]))? {
                Ok(value) => c.resolve_promise_rooted(promise, value),
                Err(error) => c.reject_promise_rooted(promise, error),
            }
        }
        _ => {
            let chunks = get(c, state, CHUNKS)?;
            let (blob_class, _) = crate::modules::blob_shared_vm::classes(c)?;
            let blob = match crate::modules::webstreams_shared_vm::attempt(c.construct_rooted(blob_class, blob_class, &[chunks]))? {
                Ok(blob) => blob,
                Err(error) => return c.reject_promise_rooted(promise, error),
            };
            if kind == Member::Blob {
                return c.resolve_promise_rooted(promise, blob);
            }
            let array_buffer = get(c, blob, "arrayBuffer")?;
            let buffer = c.call_rooted(array_buffer, blob, &[])?;
            if kind == Member::ArrayBuffer {
                return c.resolve_promise_rooted(promise, buffer);
            }
            let packaged = upon(c, buffer, Some(Member::Packaged), None, &[state])?;
            c.resolve_promise_rooted(promise, packaged)
        }
    }
}

pub(crate) fn member(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    let value = arg(c, args, 0);
    match member {
        Member::ArrayBuffer | Member::Blob | Member::Buffer | Member::Bytes | Member::Json | Member::Text => consume(c, member, value),
        Member::Stepped => {
            let [state] = captured(c)?;
            if !c.is_object_rooted(value)? {
                let shown = c.to_string(value)?;
                let error = c.type_error_rooted(&format!("Iterator result {shown} is not an object"))?;
                fail(c, state, error)?;
                return Ok(c.undefined());
            }
            let finished = get(c, value, "done")?;
            if c.truthy_rooted(finished)? {
                done(c, state)?;
            } else {
                let chunk_value = get(c, value, "value")?;
                chunk(c, state, chunk_value)?;
            }
            Ok(c.undefined())
        }
        Member::SyncValue => {
            let [state, finished] = captured(c)?;
            if c.truthy_rooted(finished)? {
                done(c, state)?;
            } else {
                chunk(c, state, value)?;
            }
            Ok(c.undefined())
        }
        Member::Failed => {
            let [state] = captured(c)?;
            fail(c, state, value)?;
            Ok(c.undefined())
        }
        Member::Packaged => {
            let [state] = captured(c)?;
            if kind_of(c, state)? == Member::Buffer {
                let length = crate::modules::webstreams_shared_vm::buffer_length(c, value);
                let view = crate::modules::webstreams_shared_vm::uint8_view(c, value, 0, length)?;
                let fast = crate::modules::buffer_shared_vm::fast_buffer(c)?;
                let prototype = get(c, fast, "prototype")?;
                c.set_prototype_rooted(view, prototype)?;
                return Ok(view);
            }
            let length = crate::modules::webstreams_shared_vm::buffer_length(c, value);
            crate::modules::webstreams_shared_vm::uint8_view(c, value, 0, length)
        }
    }
}
