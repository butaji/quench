//! The WHATWG Streams standard as Node exposes it (`stream/web` and the stream globals):
//! readable, writable and transform streams with their readers, writers and controllers,
//! the queuing strategies, and the encoding and compression transforms.
//!
//! Every instance keeps its internal slots in one null-prototype record under a private
//! symbol; the record's `brand` names the interface it implements, which is the brand check.
//! Algorithms follow Node's `lib/internal/webstreams`, including where its promise and
//! microtask steps differ from the standard's prose, so reactions run in Node's order.

pub(crate) mod adapters;
pub(crate) mod byte;
pub(crate) mod codecs;
pub(crate) mod pipe;
pub(crate) mod readable;
mod states;
pub(crate) mod strategies;
pub(crate) mod transform;
pub(crate) mod writable;

use crate::host::NodeHost;
use crate::modules::host_members::{self, Members};
use quench_runtime::{DescriptorFields, NativeContext, PromiseSnapshot, RootId, RootedError};

pub(crate) type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) type Outcome = Result<RootId, RootedError>;
type Done = Result<(), RootedError>;

/// The `builtin_cache` key of the realm's streams intrinsics.
const INTRINSICS: &str = "\0webstreams:intrinsics";

/// The interfaces the module implements, by the brand their records carry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Brand {
    ReadableStream,
    DefaultReader,
    ByobReader,
    ByobRequest,
    DefaultController,
    ByteController,
    WritableStream,
    Writer,
    WritableController,
    TransformStream,
    TransformController,
    ByteLengthStrategy,
    CountStrategy,
    TextEncoderStream,
    TextDecoderStream,
    CompressionStream,
    DecompressionStream,
}

impl Brand {
    const ALL: [Self; 17] = [
        Self::ReadableStream,
        Self::DefaultReader,
        Self::ByobReader,
        Self::ByobRequest,
        Self::DefaultController,
        Self::ByteController,
        Self::WritableStream,
        Self::Writer,
        Self::WritableController,
        Self::TransformStream,
        Self::TransformController,
        Self::ByteLengthStrategy,
        Self::CountStrategy,
        Self::TextEncoderStream,
        Self::TextDecoderStream,
        Self::CompressionStream,
        Self::DecompressionStream,
    ];

    /// The interface's name: its class's, its `Symbol.toStringTag`'s and its export's.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::ReadableStream => "ReadableStream",
            Self::DefaultReader => "ReadableStreamDefaultReader",
            Self::ByobReader => "ReadableStreamBYOBReader",
            Self::ByobRequest => "ReadableStreamBYOBRequest",
            Self::DefaultController => "ReadableStreamDefaultController",
            Self::ByteController => "ReadableByteStreamController",
            Self::WritableStream => "WritableStream",
            Self::Writer => "WritableStreamDefaultWriter",
            Self::WritableController => "WritableStreamDefaultController",
            Self::TransformStream => "TransformStream",
            Self::TransformController => "TransformStreamDefaultController",
            Self::ByteLengthStrategy => "ByteLengthQueuingStrategy",
            Self::CountStrategy => "CountQueuingStrategy",
            Self::TextEncoderStream => "TextEncoderStream",
            Self::TextDecoderStream => "TextDecoderStream",
            Self::CompressionStream => "CompressionStream",
            Self::DecompressionStream => "DecompressionStream",
        }
    }

    fn index(self) -> f64 {
        self as usize as f64
    }
}

/// `stream/web`'s exports, in Node's order.
const EXPORTS: [Brand; 17] = [
    Brand::ReadableStream,
    Brand::DefaultReader,
    Brand::ByobReader,
    Brand::ByobRequest,
    Brand::ByteController,
    Brand::DefaultController,
    Brand::TransformStream,
    Brand::TransformController,
    Brand::WritableStream,
    Brand::Writer,
    Brand::WritableController,
    Brand::ByteLengthStrategy,
    Brand::CountStrategy,
    Brand::TextEncoderStream,
    Brand::TextDecoderStream,
    Brand::CompressionStream,
    Brand::DecompressionStream,
];

/// The intrinsics record's members.
pub(crate) mod key {
    pub(crate) use crate::modules::keys::STATE;
    pub(crate) const THEN: &str = "then";
    pub(crate) const IS_CLOSED: &str = "isClosedPromise";
    pub(crate) const ERROR_FUNCTION: &str = "controllerErrorFunction";
    pub(crate) use crate::modules::keys::INSPECT;
    pub(crate) const ASYNC_ITERATOR: &str = "AsyncIterator";
    pub(crate) const ASYNC_ITERATOR_KEY: &str = "asyncIterator";
    pub(crate) const TO_STRING_TAG: &str = "toStringTag";
    pub(crate) const TRANSFER: &str = "transfer";
    pub(crate) const SLICE: &str = "slice";
    pub(crate) const UINT8_ARRAY: &str = "Uint8Array";
    pub(crate) const DATA_VIEW: &str = "DataView";
    pub(crate) const ARRAY_BUFFER: &str = "ArrayBuffer";
    pub(crate) const BUFFER: &str = "Buffer";
    pub(crate) const MODULE: &str = "module";
    pub(crate) const TEXT_DECODER: &str = "TextDecoder";
    /// Node's `kCloseSentinel`, made on first use.
    pub(crate) const CLOSE_SENTINEL: &str = "closeSentinel";
    /// The shared errors a released reader or writer reports, made on first use.
    pub(crate) const READER_RELEASED: &str = "readerReleased";
    pub(crate) const READER_RELEASING: &str = "readerReleasing";
    pub(crate) const WRITER_RELEASED: &str = "writerReleased";
    /// `nodejs.stream.*` protocol symbols the stream utilities read.
    pub(crate) const IS_DISTURBED: &str = "isDisturbed";
    pub(crate) const IS_ERRORED: &str = "isErrored";
    pub(crate) const IS_READABLE: &str = "isReadable";
    pub(crate) const IS_WRITABLE: &str = "isWritable";
}

/// Registered symbols the module keys protocol members with.
const REGISTERED_SYMBOLS: [(&str, &str); 7] = [
    (key::IS_CLOSED, "nodejs.webstream.isClosedPromise"),
    (key::ERROR_FUNCTION, "nodejs.webstream.controllerErrorFunction"),
    (key::INSPECT, "nodejs.util.inspect.custom"),
    (key::IS_DISTURBED, "nodejs.stream.disturbed"),
    (key::IS_ERRORED, "nodejs.stream.errored"),
    (key::IS_READABLE, "nodejs.stream.readable"),
    (key::IS_WRITABLE, "nodejs.stream.writable"),
];

/// Every record's brand field.
pub(crate) const BRAND: &str = "brand";

// ---- Values and records

pub(crate) fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

pub(crate) fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    c.get_named_property_rooted(object, name)
}

pub(crate) fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Done {
    c.assign_named_property_rooted(object, name, value)
}

pub(crate) fn flag(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    c.truthy_rooted(value)
}

pub(crate) fn put_flag(c: &mut Context<'_>, object: RootId, name: &str, value: bool) -> Done {
    let value = c.boolean(value);
    put(c, object, name, value)
}

pub(crate) fn num(c: &mut Context<'_>, object: RootId, name: &str) -> Result<f64, RootedError> {
    let value = get(c, object, name)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).unwrap_or(f64::NAN))
}

pub(crate) fn put_num(c: &mut Context<'_>, object: RootId, name: &str, value: f64) -> Done {
    let value = c.number(value);
    put(c, object, name, value)
}

pub(crate) fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

pub(crate) fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}

pub(crate) fn same(c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
    c.strict_equal_rooted(left, right)
}

/// A fresh null-prototype record of `names` holding `values`, made at its final shape.
pub(crate) fn record(c: &mut Context<'_>, names: &'static [&'static str], values: &[RootId]) -> Outcome {
    c.record_rooted(true, names, values)
}

/// A plain object of `names` holding `values` in order, as an object literal makes it.
pub(crate) fn literal(c: &mut Context<'_>, names: &'static [&'static str], values: &[RootId]) -> Outcome {
    c.record_rooted(false, names, values)
}

/// `{ value, done }`, in the order Node's read requests write it.
pub(crate) fn read_result(c: &mut Context<'_>, value: RootId, done: bool) -> Outcome {
    let done = c.boolean(done);
    literal(c, &["value", "done"], &[value, done])
}

/// `{ done, value }`, in the order Node's async iterator writes a finished step.
pub(crate) fn done_result(c: &mut Context<'_>, value: RootId) -> Outcome {
    let done = c.boolean(true);
    literal(c, &["done", "value"], &[done, value])
}

/// A queue of values: a record heading a singly linked list of `{ value, next }` cells.
pub(crate) mod list {
    use super::{get, is_undefined, num, put, put_num, record, Context, Done, Outcome};
    use quench_runtime::{RootId, RootedError};

    const HEAD: &str = "head";
    const TAIL: &str = "tail";
    use quench_runtime::property_names::LENGTH;
    use crate::modules::keys::VALUE;
    use crate::modules::keys::NEXT;

    pub(crate) fn new(c: &mut Context<'_>) -> Outcome {
        let empty = c.number(0.0);
        record(c, &[LENGTH], &[empty])
    }

    pub(crate) fn len(c: &mut Context<'_>, list: RootId) -> Result<usize, RootedError> {
        Ok(num(c, list, LENGTH)? as usize)
    }

    pub(crate) fn push(c: &mut Context<'_>, list: RootId, value: RootId) -> Done {
        let cell = record(c, &[VALUE], &[value])?;
        let tail = get(c, list, TAIL)?;
        if is_undefined(c, tail) {
            put(c, list, HEAD, cell)?;
        } else {
            put(c, tail, NEXT, cell)?;
        }
        put(c, list, TAIL, cell)?;
        let length = len(c, list)?;
        put_num(c, list, LENGTH, (length + 1) as f64)
    }

    /// The first value; the list must not be empty.
    pub(crate) fn peek(c: &mut Context<'_>, list: RootId) -> Outcome {
        let head = get(c, list, HEAD)?;
        get(c, head, VALUE)
    }

    /// Removes and returns the first value; the list must not be empty.
    pub(crate) fn shift(c: &mut Context<'_>, list: RootId) -> Outcome {
        let head = get(c, list, HEAD)?;
        let value = get(c, head, VALUE)?;
        let next = get(c, head, NEXT)?;
        put(c, list, HEAD, next)?;
        if is_undefined(c, next) {
            let undefined = c.undefined();
            put(c, list, TAIL, undefined)?;
        }
        let length = len(c, list)?;
        put_num(c, list, LENGTH, (length - 1) as f64)?;
        Ok(value)
    }

    /// Every value, in order, leaving the list empty.
    pub(crate) fn drain(c: &mut Context<'_>, list: RootId) -> Result<Vec<RootId>, RootedError> {
        let mut values = Vec::with_capacity(len(c, list)?);
        let mut cell = get(c, list, HEAD)?;
        while !is_undefined(c, cell) {
            values.push(get(c, cell, VALUE)?);
            cell = get(c, cell, NEXT)?;
        }
        let undefined = c.undefined();
        put(c, list, HEAD, undefined)?;
        put(c, list, TAIL, undefined)?;
        put_num(c, list, LENGTH, 0.0)?;
        Ok(values)
    }
}

// ---- Intrinsics

/// The realm's intrinsics record, built on first use from the startup intrinsics.
pub(crate) fn intrinsics(c: &mut Context<'_>) -> Outcome {
    if let Some(root) = c.host_mut().shared_state().borrow().builtin_cache.get(INTRINSICS).copied() {
        return Ok(root);
    }
    let built = c.with_startup_intrinsics(build_intrinsics)?;
    let retained = c.retain(built)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(INTRINSICS.to_owned(), retained);
    Ok(built)
}

pub(crate) fn intrinsic(c: &mut Context<'_>, name: &str) -> Outcome {
    let intrinsics = intrinsics(c)?;
    get(c, intrinsics, name)
}

fn build_intrinsics(c: &mut Context<'_>) -> Outcome {
    let state = c.private_symbol_rooted(Some("kState"));
    let table = record(c, &[key::STATE], &[state])?;
    let global = c.global_root()?;
    let promise = get(c, global, "Promise")?;
    let promise_prototype = get(c, promise, "prototype")?;
    let then = get(c, promise_prototype, "then")?;
    put(c, table, key::THEN, then)?;
    let symbol = get(c, global, "Symbol")?;
    let registry = get(c, symbol, "for")?;
    for (member, key) in REGISTERED_SYMBOLS {
        let key = c.string_rooted(key);
        let value = c.call_rooted(registry, symbol, &[key])?;
        put(c, table, member, value)?;
    }
    for member in [key::ASYNC_ITERATOR_KEY, key::TO_STRING_TAG] {
        let value = get(c, symbol, member)?;
        put(c, table, member, value)?;
    }
    for name in [key::UINT8_ARRAY, key::DATA_VIEW, key::ARRAY_BUFFER, key::TEXT_DECODER] {
        let value = get(c, global, name)?;
        put(c, table, name, value)?;
    }
    let array_buffer = get(c, global, key::ARRAY_BUFFER)?;
    let array_buffer_prototype = get(c, array_buffer, "prototype")?;
    for name in [key::TRANSFER, key::SLICE] {
        let value = get(c, array_buffer_prototype, name)?;
        put(c, table, name, value)?;
    }
    let buffer = get(c, global, key::BUFFER)?;
    put(c, table, key::BUFFER, buffer)?;
    // Node's `AsyncIterator`: `{ __proto__: AsyncIteratorPrototype, next, return }`.
    let async_iterator = c.object_rooted()?;
    let prototype = c.async_iterator_prototype_rooted();
    c.set_prototype_rooted(async_iterator, prototype)?;
    let undefined = c.undefined();
    for name in ["next", "return"] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(async_iterator, key, undefined, true, true, true)?;
    }
    put(c, table, key::ASYNC_ITERATOR, async_iterator)?;
    Ok(table)
}

// ---- Branded instances

/// A new instance of `brand`'s class carrying `state`, whose `brand` this sets.
pub(crate) fn instance(c: &mut Context<'_>, brand: Brand, state: RootId) -> Outcome {
    let prototype = prototype(c, brand)?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    attach(c, object, brand, state)?;
    Ok(object)
}

/// Gives `object` the internal slots `state` as an instance of `brand`.
pub(crate) fn attach(c: &mut Context<'_>, object: RootId, brand: Brand, state: RootId) -> Done {
    let brand = c.number(brand.index());
    put(c, state, BRAND, brand)?;
    let key = intrinsic(c, key::STATE)?;
    c.define_data_property_rooted(object, key, state, true, false, true)?;
    Ok(())
}

/// The internal slots of `value` when it is an instance of `brand`.
pub(crate) fn state_of(c: &mut Context<'_>, value: RootId, brand: Brand) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(None);
    }
    let key = intrinsic(c, key::STATE)?;
    if c.own_property_fields_rooted(value, key)?.is_none() {
        return Ok(None);
    }
    let state = c.get_property_rooted(value, key)?;
    Ok((num(c, state, BRAND)? == brand.index()).then_some(state))
}

pub(crate) fn is(c: &mut Context<'_>, value: RootId, brand: Brand) -> Result<bool, RootedError> {
    Ok(state_of(c, value, brand)?.is_some())
}

/// The internal slots of a receiver that must be an instance of `brand`.
pub(crate) fn this_state(c: &mut Context<'_>, this: RootId, brand: Brand) -> Outcome {
    match state_of(c, this, brand)? {
        Some(state) => Ok(state),
        None => Err(throw(c, WebError::InvalidThis(brand.name()))),
    }
}

/// A class constructor's `new` check: `TypeError` when called without `new`.
pub(crate) fn require_new(c: &mut Context<'_>, name: &str) -> Done {
    if c.new_target_rooted().is_some() {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!("Class constructor {name} cannot be invoked without 'new'"))?;
    Err(c.throw(error))
}

/// The internal slots of an object the module made as an instance of `brand`.
pub(crate) fn slots(c: &mut Context<'_>, object: RootId) -> Outcome {
    let key = intrinsic(c, key::STATE)?;
    c.get_property_rooted(object, key)
}

fn prototype(c: &mut Context<'_>, brand: Brand) -> Outcome {
    let classes = classes(c)?;
    let class = get(c, classes, brand.name())?;
    get(c, class, "prototype")
}

// ---- Promises

/// A new pending promise.
pub(crate) fn deferred(c: &mut Context<'_>) -> Outcome {
    c.pending_promise_rooted()
}

/// `PromiseResolve(%Promise%, value)`.
pub(crate) fn resolved(c: &mut Context<'_>, value: RootId) -> Outcome {
    if c.is_promise_rooted(value)? {
        let constructor = get(c, value, "constructor")?;
        let global = c.global_root()?;
        let promise = get(c, global, "Promise")?;
        if same(c, constructor, promise)? {
            return Ok(value);
        }
    }
    let promise = deferred(c)?;
    c.resolve_promise_rooted(promise, value)?;
    Ok(promise)
}

pub(crate) fn resolved_undefined(c: &mut Context<'_>) -> Outcome {
    let promise = deferred(c)?;
    let undefined = c.undefined();
    c.resolve_promise_rooted(promise, undefined)?;
    Ok(promise)
}

/// `PromiseReject(reason)`.
pub(crate) fn rejected(c: &mut Context<'_>, reason: RootId) -> Outcome {
    let promise = deferred(c)?;
    c.reject_promise_rooted(promise, reason)?;
    Ok(promise)
}

/// A promise rejected with `error`, as a method reports a failed precondition.
pub(crate) fn rejected_with(c: &mut Context<'_>, error: WebError<'_>) -> Outcome {
    let error = create_error(c, error)?;
    rejected(c, error)
}

pub(crate) fn is_pending(c: &mut Context<'_>, promise: RootId) -> Result<bool, RootedError> {
    Ok(matches!(c.promise_snapshot_rooted(promise)?, Some(PromiseSnapshot::Pending)))
}

/// Resolves `promise` if it is still pending (a deferred's `resolve?.()`).
pub(crate) fn settle(c: &mut Context<'_>, promise: RootId, value: RootId) -> Done {
    if is_pending(c, promise)? {
        c.resolve_promise_rooted(promise, value)?;
    }
    Ok(())
}

pub(crate) fn settle_undefined(c: &mut Context<'_>, promise: RootId) -> Done {
    let undefined = c.undefined();
    settle(c, promise, undefined)
}

/// Rejects `promise` if it is still pending (a deferred's `reject?.()`).
pub(crate) fn fail(c: &mut Context<'_>, promise: RootId, reason: RootId) -> Done {
    if is_pending(c, promise)? {
        c.reject_promise_rooted(promise, reason)?;
    }
    Ok(())
}

pub(crate) fn mark_handled(c: &mut Context<'_>, promise: RootId) -> Done {
    c.mark_promise_handled_rooted(promise)
}

/// `PromisePrototypeThen(promise, fulfilled, rejected)` with the original `then`.
pub(crate) fn then(c: &mut Context<'_>, promise: RootId, fulfilled: RootId, rejected: RootId) -> Outcome {
    let then = intrinsic(c, key::THEN)?;
    c.call_rooted(then, promise, &[fulfilled, rejected])
}

/// `then` with reactions of the family `M`, each closing over `captures`.
pub(crate) fn upon<M: Members>(
    c: &mut Context<'_>,
    promise: RootId,
    fulfilled: Option<M>,
    rejected: Option<M>,
    captures: &[RootId],
) -> Outcome {
    let fulfilled = reaction(c, fulfilled, captures)?;
    let rejected = reaction(c, rejected, captures)?;
    then(c, promise, fulfilled, rejected)
}

fn reaction<M: Members>(c: &mut Context<'_>, member: Option<M>, captures: &[RootId]) -> Outcome {
    match member {
        Some(member) => host_members::function(c, member, "", captures),
        None => Ok(c.undefined()),
    }
}

/// Webidl's "invoke a callback function" for promise-returning callbacks: Node's
/// `async function invokePromiseCallback(fn, thisArg, ...args)`.
pub(crate) fn promise_call(c: &mut Context<'_>, function: RootId, this: RootId, args: &[RootId]) -> Outcome {
    let promise = deferred(c)?;
    match c.call_rooted(function, this, args) {
        Ok(value) => c.resolve_promise_rooted(promise, value)?,
        Err(RootedError { exception: Some(error), .. }) => c.reject_promise_rooted(promise, error)?,
        Err(error) => return Err(error),
    }
    Ok(promise)
}

/// An internal algorithm slot: the user callback `function` with `this`, called as
/// `promise_call` does, or an internal one (`this` absent) that returns its promise.
/// An absent callback is Node's `nonOp*`, an already fulfilled promise.
pub(crate) fn run_algorithm(c: &mut Context<'_>, controller: RootId, name: &str, args: &[RootId]) -> Outcome {
    let function = get(c, controller, name)?;
    if !c.is_callable_rooted(function)? {
        return resolved_undefined(c);
    }
    let user = get(c, controller, algorithm::SOURCE)?;
    if flag(c, controller, algorithm::INTERNAL)? {
        let undefined = c.undefined();
        return c.call_rooted(function, undefined, args);
    }
    promise_call(c, function, user, args)
}

/// The controller fields that say how its algorithms are called.
pub(crate) mod algorithm {
    pub(crate) use crate::modules::keys::SOURCE;
    /// Whether the algorithms are the module's own (they return promises directly).
    pub(crate) const INTERNAL: &str = "internal";
}

/// A callback member of `source` as `createPromiseCallback(name, fn)` validates it:
/// `undefined` when absent, a function otherwise.
pub(crate) fn callback(c: &mut Context<'_>, source: RootId, member: &str, name: &str) -> Outcome {
    let value = get(c, source, member)?;
    if is_undefined(c, value) || c.is_callable_rooted(value)? {
        return Ok(value);
    }
    Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "function")?)
}

/// The member of `object`, or `undefined` for a nullish object (`object?.member`).
pub(crate) fn optional(c: &mut Context<'_>, object: RootId, member: &str) -> Outcome {
    if is_nullish(c, object) {
        return Ok(c.undefined());
    }
    get(c, object, member)
}

// ---- Validation and errors

/// The objects Node's `validateObject` accepts for a dictionary argument.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dictionary {
    /// Objects, arrays and functions (`kValidateObjectAllowObjects`).
    Objects,
    /// Those, `null` too (`kValidateObjectAllowObjectsAndNull`).
    ObjectsAndNull,
    /// Plain objects only (`validateObject`'s default).
    Plain,
}

/// `validateObject(value, name, kind)`; `undefined` arguments are first given Node's
/// `kEmptyObject` default by the caller.
pub(crate) fn validate_dictionary(c: &mut Context<'_>, value: RootId, name: &str, kind: Dictionary) -> Done {
    let is_null = c.rooted_value(value).is_some_and(|value| value.is_null());
    let callable = c.is_callable_rooted(value)?;
    let valid = match kind {
        Dictionary::Objects => !is_null && (c.is_object_rooted(value)? || callable),
        Dictionary::ObjectsAndNull => is_null || c.is_object_rooted(value)? || callable,
        Dictionary::Plain => !is_null && c.is_object_rooted(value)? && !c.is_array_rooted(value)?,
    };
    if valid {
        return Ok(());
    }
    Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "Object")?)
}

/// An optional dictionary argument: Node's `kEmptyObject` default for `undefined`.
pub(crate) fn dictionary(c: &mut Context<'_>, args: &[RootId], index: usize, name: &str, kind: Dictionary) -> Outcome {
    let value = arg(c, args, index);
    if is_undefined(c, value) {
        return Ok(c.undefined());
    }
    validate_dictionary(c, value, name, kind)?;
    Ok(value)
}

/// `extractHighWaterMark(value, default)`.
pub(crate) fn high_water_mark(c: &mut Context<'_>, value: RootId, default: f64) -> Result<f64, RootedError> {
    if is_undefined(c, value) {
        return Ok(default);
    }
    let number = c.to_number(value)?;
    if number.is_nan() || number < 0.0 {
        let number = c.number(number);
        return Err(throw(c, WebError::ArgValueRange("strategy.highWaterMark", number)));
    }
    Ok(number)
}

/// `extractSizeAlgorithm(size)`: `undefined` stands for Node's `() => 1`.
pub(crate) fn size_algorithm(c: &mut Context<'_>, size: RootId) -> Outcome {
    if is_undefined(c, size) || c.is_callable_rooted(size)? {
        return Ok(size);
    }
    Err(crate::modules::util_shared_vm::argument_type_error(c, size, "strategy.size", "function")?)
}

/// The coded errors the module raises.
pub(crate) enum WebError<'a> {
    /// `ERR_INVALID_STATE.TypeError`.
    InvalidState(&'a str),
    /// `ERR_INVALID_STATE.RangeError`.
    InvalidStateRange(&'a str),
    /// `ERR_INVALID_THIS`, naming the interface.
    InvalidThis(&'a str),
    IllegalConstructor,
    /// `ERR_MISSING_OPTION`.
    MissingOption(&'a str),
    /// `ERR_INVALID_ARG_VALUE(name, value)`.
    ArgValue(&'a str, RootId),
    /// `ERR_INVALID_ARG_VALUE(name, value, reason)`.
    ArgValueBecause(&'a str, RootId, &'a str),
    /// `ERR_INVALID_ARG_VALUE.RangeError(name, value)`.
    ArgValueRange(&'a str, RootId),
    /// `ERR_INVALID_ARG_TYPE(name, expected, value)`, `expected` as Node's validators name it.
    ArgType(&'a str, &'a str, RootId),
    /// `ERR_OUT_OF_RANGE(name, range, value)`.
    OutOfRange(&'a str, &'a str, RootId),
    /// `ERR_ARG_NOT_ITERABLE(value)`.
    NotIterable(RootId),
}

/// The error value, as `new ERR_*()` makes it.
pub(crate) fn create_error(c: &mut Context<'_>, error: WebError<'_>) -> Outcome {
    use crate::modules::node_errors_shared_vm::{decorate_range_error, decorate_type_error};
    use crate::modules::util_shared_vm::{argument_type_error, argument_value_error};
    let coded = |c: &mut Context<'_>, message: &str, code: &'static str, range: bool| -> Outcome {
        if range {
            let error = c.range_error_rooted(message)?;
            decorate_range_error(c, error, code)?;
            Ok(error)
        } else {
            let error = c.type_error_rooted(message)?;
            decorate_type_error(c, error, code)?;
            Ok(error)
        }
    };
    match error {
        WebError::InvalidState(message) => coded(c, &format!("Invalid state: {message}"), "ERR_INVALID_STATE", false),
        WebError::InvalidStateRange(message) => coded(c, &format!("Invalid state: {message}"), "ERR_INVALID_STATE", true),
        WebError::InvalidThis(name) => coded(c, &format!("Value of \"this\" must be of type {name}"), "ERR_INVALID_THIS", false),
        WebError::IllegalConstructor => coded(c, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR", false),
        WebError::MissingOption(name) => coded(c, &format!("{name} is required"), "ERR_MISSING_OPTION", false),
        WebError::ArgValue(name, value) => caught(argument_value_error(c, value, name, "is invalid")?),
        WebError::ArgValueBecause(name, value, reason) => caught(argument_value_error(c, value, name, reason)?),
        WebError::ArgValueRange(name, value) => {
            let thrown = caught(argument_value_error(c, value, name, "is invalid")?)?;
            let message = get(c, thrown, "message")?;
            let message = c.to_string(message)?;
            coded(c, &message, "ERR_INVALID_ARG_VALUE", true)
        }
        WebError::ArgType(name, expected, value) => caught(argument_type_error(c, value, name, expected)?),
        WebError::OutOfRange(name, range, value) => {
            let received = crate::modules::util_shared_vm::range_error_received(c, value)?;
            let message = format!("The value of \"{name}\" is out of range. It must be {range}. Received {received}");
            coded(c, &message, "ERR_OUT_OF_RANGE", true)
        }
        WebError::NotIterable(value) => {
            let text = crate::modules::console_format_shared_vm::inspect(c, value, Default::default())?;
            coded(c, &format!("{text} must be iterable"), "ERR_ARG_NOT_ITERABLE", false)
        }
    }
}

pub(crate) fn throw(c: &mut Context<'_>, error: WebError<'_>) -> RootedError {
    match create_error(c, error) {
        Ok(value) => c.throw(value),
        Err(error) => error,
    }
}

/// The value a thrown error carries.
pub(crate) fn caught(error: RootedError) -> Outcome {
    match error.exception {
        Some(value) => Ok(value),
        None => Err(error),
    }
}

pub(crate) use crate::modules::stream_shared_vm::attempt;

/// One of the shared release errors, made once (Node's `lazyReadableReleasedError` and
/// friends).
pub(crate) fn released_error(c: &mut Context<'_>, member: &str, message: &str) -> Outcome {
    let intrinsics = intrinsics(c)?;
    let cached = get(c, intrinsics, member)?;
    if !is_undefined(c, cached) {
        return Ok(cached);
    }
    let error = create_error(c, WebError::InvalidState(message))?;
    put(c, intrinsics, member, error)?;
    Ok(error)
}

// ---- Buffers

/// The buffer, byte offset and byte length of an ArrayBuffer view, read without
/// observable property access.
pub(crate) struct ViewParts {
    pub(crate) buffer: RootId,
    pub(crate) offset: usize,
    pub(crate) length: usize,
    /// Whether the view is a DataView rather than a typed array.
    pub(crate) data_view: bool,
}

pub(crate) fn view_parts(c: &mut Context<'_>, view: RootId) -> Result<Option<ViewParts>, RootedError> {
    if !c.has_builtin_brand_rooted(view, quench_runtime::BuiltinBrand::ArrayBufferView)? {
        return Ok(None);
    }
    let data_view = c.has_builtin_brand_rooted(view, quench_runtime::BuiltinBrand::DataView)?;
    let Some(storage) = c.buffer_storage_rooted(view)? else {
        let buffer = get(c, view, "buffer")?;
        return Ok(Some(ViewParts { buffer, offset: 0, length: 0, data_view }));
    };
    Ok(Some(ViewParts { buffer: storage.buffer, offset: storage.byte_offset, length: storage.byte_length, data_view }))
}

pub(crate) fn buffer_length(c: &mut Context<'_>, buffer: RootId) -> usize {
    c.with_buffer_bytes_rooted(buffer, <[u8]>::len).unwrap_or(0)
}

pub(crate) fn is_detached(c: &mut Context<'_>, buffer: RootId) -> Result<bool, RootedError> {
    let detached = get(c, buffer, "detached")?;
    c.truthy_rooted(detached)
}

/// `ArrayBufferPrototypeTransfer(buffer)`.
pub(crate) fn transfer(c: &mut Context<'_>, buffer: RootId) -> Outcome {
    let transfer = intrinsic(c, key::TRANSFER)?;
    c.call_rooted(transfer, buffer, &[])
}

/// `ArrayBufferPrototypeSlice(buffer, start, end)`.
pub(crate) fn slice(c: &mut Context<'_>, buffer: RootId, start: usize, end: usize) -> Outcome {
    let function = intrinsic(c, key::SLICE)?;
    let start = c.number(start as f64);
    let end = c.number(end as f64);
    c.call_rooted(function, buffer, &[start, end])
}

/// `new constructor(buffer, offset, length)`.
pub(crate) fn construct_view(c: &mut Context<'_>, constructor: RootId, buffer: RootId, offset: usize, length: usize) -> Outcome {
    let offset = c.number(offset as f64);
    let length = c.number(length as f64);
    c.construct_rooted(constructor, constructor, &[buffer, offset, length])
}

pub(crate) fn uint8_view(c: &mut Context<'_>, buffer: RootId, offset: usize, length: usize) -> Outcome {
    let constructor = intrinsic(c, key::UINT8_ARRAY)?;
    construct_view(c, constructor, buffer, offset, length)
}

/// Copies `count` bytes between two distinct buffers.
pub(crate) fn copy_bytes(c: &mut Context<'_>, to: RootId, to_index: usize, from: RootId, from_index: usize, count: usize) -> Done {
    let bytes = c.with_buffer_bytes_rooted(from, |bytes| bytes[from_index..from_index + count].to_vec()).unwrap_or_default();
    c.with_buffer_bytes_mut_rooted(to, |target| target[to_index..to_index + bytes.len()].copy_from_slice(&bytes));
    Ok(())
}

// ---- Classes

/// A class member: a method, or an accessor with an optional setter, by family member.
pub(crate) enum ClassMember<M> {
    Method(&'static str, M, u32),
    Getter(&'static str, M),
}

/// Installs `members` enumerable on `holder`, as `ObjectDefineProperties` with
/// `kEnumerableProperty` leaves Node's class members.
pub(crate) fn install<M: Members>(c: &mut Context<'_>, holder: RootId, members: &[ClassMember<M>]) -> Done {
    for member in members {
        match *member {
            ClassMember::Method(name, member, length) => {
                let function = method_function(c, member, name, length)?;
                let key = c.string_rooted(name);
                c.define_data_property_rooted(holder, key, function, true, true, true)?;
            }
            ClassMember::Getter(name, member) => {
                let getter = method_function(c, member, &format!("get {name}"), 0)?;
                let key = c.string_rooted(name);
                let undefined = c.undefined();
                let fields = DescriptorFields {
                    get: Some(getter),
                    set: Some(undefined),
                    enumerable: Some(true),
                    configurable: Some(true),
                    ..Default::default()
                };
                c.define_property_fields_rooted(holder, key, fields)?;
            }
        }
    }
    Ok(())
}

/// A host function of `member` named `name` whose `length` is `length`.
pub(crate) fn method_function<M: Members>(c: &mut Context<'_>, member: M, name: &str, length: u32) -> Outcome {
    let function = host_members::function(c, member, name, &[])?;
    set_length(c, function, length)?;
    Ok(function)
}

pub(crate) fn set_length(c: &mut Context<'_>, function: RootId, length: u32) -> Done {
    let key = c.string_rooted("length");
    let value = c.number(f64::from(length));
    let fields = DescriptorFields { value: Some(value), writable: Some(false), enumerable: Some(false), configurable: Some(true), ..Default::default() };
    c.define_property_fields_rooted(function, key, fields)?;
    Ok(())
}

/// A symbol-keyed non-enumerable method or getter on `holder`.
pub(crate) fn install_symbol_method<M: Members>(c: &mut Context<'_>, holder: RootId, symbol: &str, member: M, name: &str) -> Done {
    let key = intrinsic(c, symbol)?;
    let function = method_function(c, member, name, 0)?;
    c.define_data_property_rooted(holder, key, function, true, false, true)?;
    Ok(())
}

pub(crate) fn install_symbol_getter<M: Members>(c: &mut Context<'_>, holder: RootId, symbol: &str, member: M, name: &str) -> Done {
    let key = intrinsic(c, symbol)?;
    let getter = method_function(c, member, name, 0)?;
    let undefined = c.undefined();
    let fields = DescriptorFields { get: Some(getter), set: Some(undefined), enumerable: Some(false), configurable: Some(true), ..Default::default() };
    c.define_property_fields_rooted(holder, key, fields)?;
    Ok(())
}

/// The class of `brand`, constructed by `member` with `length` parameters: its prototype
/// gets `Symbol.toStringTag` and the custom inspection `inspect`.
pub(crate) fn class<M: Members>(c: &mut Context<'_>, brand: Brand, member: M, length: u32, inspect: M) -> Outcome {
    class_with(c, brand, member, length, inspect, true)
}

/// `class`, with `Symbol.toStringTag` only when `tagged` (Node's encoding streams lack one).
pub(crate) fn class_with<M: Members>(c: &mut Context<'_>, brand: Brand, member: M, length: u32, inspect: M, tagged: bool) -> Outcome {
    let constructor = host_members::constructor(c, member, brand.name())?;
    set_length(c, constructor, length)?;
    let prototype = get(c, constructor, "prototype")?;
    c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
    if tagged {
        let tag = intrinsic(c, key::TO_STRING_TAG)?;
        let name = c.string_rooted(brand.name());
        c.define_data_property_rooted(prototype, tag, name, false, false, true)?;
    }
    install_symbol_method(c, prototype, key::INSPECT, inspect, "[nodejs.util.inspect.custom]")?;
    Ok(constructor)
}

/// Node's `customInspect(depth, options, name, data)`; `args` are `(depth, options)`.
pub(crate) fn custom_inspect(c: &mut Context<'_>, this: RootId, name: &str, data: RootId, args: &[RootId]) -> Outcome {
    let depth = arg(c, args, 0);
    if c.rooted_value(depth).and_then(|value| value.as_number()).is_some_and(|depth| depth < 0.0) {
        return Ok(this);
    }
    let options = arg(c, args, 1);
    let copy = c.object_rooted()?;
    if c.is_object_rooted(options)? {
        for (key, value) in c.plain_enumerable_entries_rooted(options).unwrap_or_default() {
            put(c, copy, &key, value)?;
        }
        let depth = get(c, options, "depth")?;
        let next = if is_nullish(c, depth) {
            c.null()
        } else {
            let depth = c.to_number(depth)?;
            c.number(depth - 1.0)
        };
        put(c, copy, "depth", next)?;
    }
    let inspect = crate::modules::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let text = c.call_rooted(inspect, undefined, &[data, copy])?;
    let text = c.to_string(text)?;
    Ok(c.string_rooted(&format!("{name} {text}")))
}

/// The module's classes, by interface name, built once.
pub(crate) fn classes(c: &mut Context<'_>) -> Outcome {
    let intrinsics = intrinsics(c)?;
    let built = get(c, intrinsics, key::MODULE)?;
    if !is_undefined(c, built) {
        return Ok(built);
    }
    let table = c.null_object_rooted()?;
    put(c, intrinsics, key::MODULE, table)?;
    let built = c.with_startup_intrinsics(|c| {
        readable::build(c, table)?;
        byte::build(c, table)?;
        writable::build(c, table)?;
        transform::build(c, table)?;
        strategies::build(c, table)?;
        codecs::build(c, table)
    });
    if built.is_err() {
        let undefined = c.undefined();
        put(c, intrinsics, key::MODULE, undefined)?;
    }
    built?;
    Ok(table)
}

/// The `stream/web` module.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let classes = classes(c)?;
    let module = c.object_rooted()?;
    for brand in EXPORTS {
        let class = get(c, classes, brand.name())?;
        let key = c.string_rooted(brand.name());
        c.define_data_property_rooted(module, key, class, true, true, true)?;
    }
    Ok(module)
}

/// The stream globals' lazy initializer: each global is its class.
pub(crate) fn initialize_globals(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let classes = classes(c)?;
    let global = c.global_root()?;
    for brand in Brand::ALL {
        let class = get(c, classes, brand.name())?;
        let key = c.string_rooted(brand.name());
        c.define_data_property_rooted(global, key, class, true, false, true)?;
    }
    Ok(c.undefined())
}

/// The globals `initialize_globals` defines.
pub(crate) const GLOBALS: [&str; 17] = {
    let mut names = [""; Brand::ALL.len()];
    let mut index = 0;
    while index < names.len() {
        names[index] = EXPORTS[index].name();
        index += 1;
    }
    names
};

// The brand records index `Brand::ALL` by discriminant.
const _: () = {
    let mut index = 0;
    while index < Brand::ALL.len() {
        assert!(Brand::ALL[index] as usize == index);
        index += 1;
    }
};
