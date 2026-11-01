//! Node's `stream` family over the shared EventEmitter: the legacy `Stream`, `Readable`,
//! `Writable`, `Duplex`, `Transform` and `PassThrough`, and the end-of-stream, destroy,
//! pipeline and abort-signal utilities.
//!
//! Each stream keeps its state where Node keeps it and libraries inspect it: a
//! `_readableState`/`_writableState` object whose flags are one bit field under the
//! private `kState` symbol, projected as accessors on the state prototypes. The host keeps
//! only the fixed intrinsics (symbols, classes, scheduled operations) the module owns.

pub(crate) mod destroy;
pub(crate) mod duplex;
pub(crate) mod eos;
pub(crate) mod errors;
pub(crate) mod from;
pub(crate) mod iterator;
pub(crate) mod legacy;
pub(crate) mod pipeline;
pub(crate) mod readable;
pub(crate) mod state;
pub(crate) mod utils;
pub(crate) mod writable;

use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

pub(crate) type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) type Outcome = Result<RootId, RootedError>;

/// The symbols the stream classes key their private and protocol slots with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    State,
    ErroredValue,
    DefaultEncodingValue,
    DecoderValue,
    EncodingValue,
    OnFinishedValue,
    WriteCbValue,
    AfterWriteTickInfoValue,
    BufferedValue,
    OnConstructed,
    Destroy,
    Construct,
    Callback,
    PairCallback,
    OtherSide,
    IsDestroyed,
    IsErrored,
    IsReadable,
    IsWritable,
    IsDisturbed,
    Rejection,
    AsyncDispose,
    AsyncIterator,
    Iterator,
    HasInstance,
    Dispose,
}

/// Where a slot's symbol comes from.
enum SymbolSource {
    /// A fresh `Symbol(description)`, private to the module.
    Private(&'static str),
    /// A private name no reflection exposes (a class's `#field`).
    PrivateName(&'static str),
    /// `Symbol.for(key)`, shared with every library that names it.
    Registered(&'static str),
    /// A well-known `Symbol[name]`.
    WellKnown(&'static str),
    /// A well-known `Symbol[name]`, or `Symbol.for(key)` where the engine lacks it.
    WellKnownOr(&'static str, &'static str),
}

impl Slot {
    const ALL: [Self; 26] = [
        Self::State,
        Self::ErroredValue,
        Self::DefaultEncodingValue,
        Self::DecoderValue,
        Self::EncodingValue,
        Self::OnFinishedValue,
        Self::WriteCbValue,
        Self::AfterWriteTickInfoValue,
        Self::BufferedValue,
        Self::OnConstructed,
        Self::Destroy,
        Self::Construct,
        Self::Callback,
        Self::PairCallback,
        Self::OtherSide,
        Self::IsDestroyed,
        Self::IsErrored,
        Self::IsReadable,
        Self::IsWritable,
        Self::IsDisturbed,
        Self::Rejection,
        Self::AsyncDispose,
        Self::AsyncIterator,
        Self::Iterator,
        Self::HasInstance,
        Self::Dispose,
    ];

    fn source(self) -> SymbolSource {
        use SymbolSource::*;
        match self {
            Self::State => Private("kState"),
            Self::ErroredValue => Private("kErroredValue"),
            Self::DefaultEncodingValue => Private("kDefaultEncodingValue"),
            Self::DecoderValue => Private("kDecoderValue"),
            Self::EncodingValue => Private("kEncodingValue"),
            Self::OnFinishedValue => Private("kOnFinishedValue"),
            Self::WriteCbValue => Private("kWriteCbValue"),
            Self::AfterWriteTickInfoValue => Private("kAfterWriteTickInfoValue"),
            Self::BufferedValue => Private("kBufferedValue"),
            Self::OnConstructed => Private("kOnConstructed"),
            Self::Destroy => Private("kDestroy"),
            Self::Construct => Private("kConstruct"),
            Self::Callback => Private("kCallback"),
            Self::PairCallback => Private("Callback"),
            Self::OtherSide => PrivateName("#otherSide"),
            Self::IsDestroyed => Registered("nodejs.stream.destroyed"),
            Self::IsErrored => Registered("nodejs.stream.errored"),
            Self::IsReadable => Registered("nodejs.stream.readable"),
            Self::IsWritable => Registered("nodejs.stream.writable"),
            Self::IsDisturbed => Registered("nodejs.stream.disturbed"),
            Self::Rejection => Registered("nodejs.rejection"),
            Self::AsyncDispose => WellKnownOr("asyncDispose", "nodejs.asyncDispose"),
            Self::AsyncIterator => WellKnown("asyncIterator"),
            Self::Iterator => WellKnown("iterator"),
            Self::HasInstance => WellKnown("hasInstance"),
            Self::Dispose => WellKnownOr("dispose", "nodejs.dispose"),
        }
    }
}

/// The module's constructors.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Emitter,
    Stream,
    Readable,
    Writable,
    Duplex,
    Transform,
    PassThrough,
    ReadableState,
    WritableState,
    Buffer,
    DuplexSide,
}

impl Class {
    const COUNT: usize = 11;
}

/// Operations a stream schedules with `process.nextTick`, each a host function made once.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tick {
    EmitReadable,
    MaybeReadMore,
    ReadZero,
    UpdateReadableListening,
    Resume,
    EndReadable,
    EndWritable,
    OnWriteError,
    AfterWrite,
    AfterWriteBatch,
    FinishIfNeeded,
    Finish,
    ErrorBuffer,
    EmitErrorClose,
    EmitClose,
    EmitError,
    Construct,
    EmitCloseLegacy,
    EmitErrorCloseLegacy,
}

impl Tick {
    const ALL: [Self; 19] = [
        Self::EmitReadable,
        Self::MaybeReadMore,
        Self::ReadZero,
        Self::UpdateReadableListening,
        Self::Resume,
        Self::EndReadable,
        Self::EndWritable,
        Self::OnWriteError,
        Self::AfterWrite,
        Self::AfterWriteBatch,
        Self::FinishIfNeeded,
        Self::Finish,
        Self::ErrorBuffer,
        Self::EmitErrorClose,
        Self::EmitClose,
        Self::EmitError,
        Self::Construct,
        Self::EmitCloseLegacy,
        Self::EmitErrorCloseLegacy,
    ];

    /// The host operation, and the name of the Node function it is.
    fn operation(self) -> (&'static str, &'static str) {
        match self {
            Self::EmitReadable => ("streamTickEmitReadable", "emitReadable_"),
            Self::MaybeReadMore => ("streamTickMaybeReadMore", "maybeReadMore_"),
            Self::ReadZero => ("streamTickReadZero", "nReadingNextTick"),
            Self::UpdateReadableListening => ("streamTickUpdateReadableListening", "updateReadableListening"),
            Self::Resume => ("streamTickResume", "resume_"),
            Self::EndReadable => ("streamTickEndReadable", "endReadableNT"),
            Self::EndWritable => ("streamTickEndWritable", "endWritableNT"),
            Self::OnWriteError => ("streamTickOnWriteError", "onwriteError"),
            Self::AfterWrite => ("streamTickAfterWrite", "afterWrite"),
            Self::AfterWriteBatch => ("streamTickAfterWriteTick", "afterWriteTick"),
            Self::FinishIfNeeded => ("streamTickFinishIfNeeded", ""),
            Self::Finish => ("streamTickFinish", "finish"),
            Self::ErrorBuffer => ("streamTickErrorBuffer", "errorBuffer"),
            Self::EmitErrorClose => ("streamTickEmitErrorClose", "emitErrorCloseNT"),
            Self::EmitClose => ("streamTickEmitClose", "emitCloseNT"),
            Self::EmitError => ("streamTickEmitError", "emitErrorNT"),
            Self::Construct => ("streamTickConstruct", "constructNT"),
            Self::EmitCloseLegacy => ("streamTickEmitCloseLegacy", "emitCloseLegacy"),
            Self::EmitErrorCloseLegacy => ("streamTickEmitErrorCloseLegacy", "emitErrorCloseLegacy"),
        }
    }
}

/// The module's fixed roots, retained for the VM's lifetime.
#[derive(Clone, Copy)]
pub(crate) struct Intrinsics {
    slots: [RootId; Slot::ALL.len()],
    classes: [RootId; Class::COUNT],
    ticks: [RootId; Tick::ALL.len()],
    /// The `nop` callback Node's writable compares callbacks with.
    pub(crate) nop: RootId,
    /// `Promise` and `Promise.prototype.then` as the module found them: `await` looks
    /// neither up.
    promise: RootId,
    promise_then: RootId,
    pub(crate) module: RootId,
}

crate::host::startup_snapshot::host_snap_struct!(Intrinsics { slots, classes, ticks, nop, promise, promise_then, module });

impl Intrinsics {
    pub(crate) fn slot(&self, slot: Slot) -> RootId {
        self.slots[Slot::ALL.iter().position(|candidate| *candidate == slot).expect("listed slot")]
    }
    pub(crate) fn class(&self, class: Class) -> RootId {
        self.classes[class as usize]
    }
    fn tick(&self, tick: Tick) -> RootId {
        self.ticks[Tick::ALL.iter().position(|candidate| *candidate == tick).expect("listed tick")]
    }
}

pub(crate) fn intrinsics(c: &mut Context<'_>) -> Result<Intrinsics, RootedError> {
    match c.host_mut().shared_state().borrow().stream_intrinsics {
        Some(intrinsics) => Ok(intrinsics),
        None => Err(RootedError::host("stream module is not initialized")),
    }
}

pub(crate) fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

pub(crate) fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    c.get_named_property_rooted(object, name)
}

pub(crate) fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

pub(crate) fn get_slot(c: &mut Context<'_>, object: RootId, slot: Slot) -> Outcome {
    let key = intrinsics(c)?.slot(slot);
    c.get_property_rooted(object, key)
}

pub(crate) fn put_slot(c: &mut Context<'_>, object: RootId, slot: Slot, value: RootId) -> Result<(), RootedError> {
    let key = intrinsics(c)?.slot(slot);
    c.set_property_rooted(object, key, value, object).map(drop)
}

pub(crate) fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

pub(crate) fn is_null(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null())
}

pub(crate) fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}

pub(crate) fn is_string(c: &Context<'_>, value: RootId) -> bool {
    matches!(c.value_type_rooted(value), Some(quench_runtime::ValueType::String))
}

pub(crate) fn is_function(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    c.is_callable_rooted(value)
}

/// Whether `object[name]` is truthy.
pub(crate) fn truthy_member(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    c.truthy_rooted(value)
}

/// The number a numeric field holds, read as JavaScript arithmetic reads it.
pub(crate) fn number(c: &mut Context<'_>, object: RootId, name: &str) -> Result<f64, RootedError> {
    let value = get(c, object, name)?;
    match c.rooted_value(value).and_then(|value| value.as_number()) {
        Some(number) => Ok(number),
        None => c.to_number(value),
    }
}

pub(crate) fn put_number(c: &mut Context<'_>, object: RootId, name: &str, value: f64) -> Result<(), RootedError> {
    let value = c.number(value);
    put(c, object, name, value)
}

pub(crate) fn put_bool(c: &mut Context<'_>, object: RootId, name: &str, value: bool) -> Result<(), RootedError> {
    let value = c.boolean(value);
    put(c, object, name, value)
}

/// `object[name](...args)`.
pub(crate) fn call_method(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Outcome {
    let method = get(c, object, name)?;
    c.call_rooted(method, object, args)
}

/// `stream.emit(event, ...args)`, through the stream's own `emit`.
pub(crate) fn emit(c: &mut Context<'_>, stream: RootId, event: &str, args: &[RootId]) -> Result<bool, RootedError> {
    let event = c.string_rooted(event);
    emit_key(c, stream, event, args)
}

pub(crate) fn emit_key(c: &mut Context<'_>, stream: RootId, event: RootId, args: &[RootId]) -> Result<bool, RootedError> {
    let mut values = Vec::with_capacity(args.len() + 1);
    values.push(event);
    values.extend_from_slice(args);
    let result = call_method(c, stream, "emit", &values)?;
    c.truthy_rooted(result)
}

pub(crate) fn listener_count(c: &mut Context<'_>, emitter: RootId, event: &str) -> Result<f64, RootedError> {
    let event = c.string_rooted(event);
    let count = call_method(c, emitter, "listenerCount", &[event])?;
    c.to_number(count)
}

pub(crate) fn on(c: &mut Context<'_>, emitter: RootId, event: &str, listener: RootId) -> Result<(), RootedError> {
    let event = c.string_rooted(event);
    call_method(c, emitter, "on", &[event, listener]).map(drop)
}

pub(crate) fn once(c: &mut Context<'_>, emitter: RootId, event: &str, listener: RootId) -> Result<(), RootedError> {
    let event = c.string_rooted(event);
    call_method(c, emitter, "once", &[event, listener]).map(drop)
}

pub(crate) fn remove_listener(c: &mut Context<'_>, emitter: RootId, event: &str, listener: RootId) -> Result<(), RootedError> {
    let event = c.string_rooted(event);
    call_method(c, emitter, "removeListener", &[event, listener]).map(drop)
}

/// A host closure: `operation` called with `captures` as its environment, an array the
/// closure may also update (`set_capture`).
pub(crate) fn closure(c: &mut Context<'_>, operation: &str, name: &str, captures: &[RootId]) -> Outcome {
    let environment = c.array_rooted(captures)?;
    c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, environment)
}

pub(crate) fn captures<const N: usize>(c: &mut Context<'_>) -> Result<[RootId; N], RootedError> {
    let environment = c.host_function_data()?;
    let values = c.array_like_values_rooted(environment)?;
    let missing = c.undefined();
    Ok(std::array::from_fn(|index| values.get(index).copied().unwrap_or(missing)))
}

pub(crate) fn set_capture(c: &mut Context<'_>, index: usize, value: RootId) -> Result<(), RootedError> {
    let environment = c.host_function_data()?;
    put(c, environment, &index.to_string(), value)
}

/// `process.nextTick(operation, ...args)` for one of the module's scheduled operations.
pub(crate) fn tick(c: &mut Context<'_>, operation: Tick, args: &[RootId]) -> Result<(), RootedError> {
    let callback = intrinsics(c)?.tick(operation);
    next_tick(c, callback, args)
}

/// `process.nextTick(callback, ...args)`.
pub(crate) fn next_tick(c: &mut Context<'_>, callback: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let receiver = c.undefined();
    super::shared_event_loop::queue_next_tick(c, callback, receiver, args)
}

/// A call's completion as JavaScript's `try`/`catch` sees it: its value, or the value it
/// threw. A host failure that threw no value still propagates.
pub(crate) fn attempt<T>(result: Result<T, RootedError>) -> Result<Result<T, RootId>, RootedError> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(RootedError { exception: Some(exception), .. }) => Ok(Err(exception)),
        Err(error) => Err(error),
    }
}

/// `await value`, continuing with `fulfilled(result)` or `rejected(reason)` as reactions.
pub(crate) fn await_then(c: &mut Context<'_>, value: RootId, fulfilled: RootId, rejected: RootId) -> Result<(), RootedError> {
    let intrinsics = intrinsics(c)?;
    let promise = if promise_resolves_to_itself(c, value, intrinsics.promise)? {
        value
    } else {
        let promise = c.pending_promise_rooted()?;
        c.resolve_promise_rooted(promise, value)?;
        promise
    };
    c.call_rooted(intrinsics.promise_then, promise, &[fulfilled, rejected])?;
    Ok(())
}

/// `PromiseResolve(%Promise%, value)` returns `value` itself: a promise of `%Promise%`.
fn promise_resolves_to_itself(c: &mut Context<'_>, value: RootId, constructor: RootId) -> Result<bool, RootedError> {
    if !c.is_promise_rooted(value)? {
        return Ok(false);
    }
    let own = get(c, value, "constructor")?;
    c.strict_equal_rooted(own, constructor)
}

/// An iterator result `{ value, done }`.
pub(crate) fn iterator_result(c: &mut Context<'_>, value: RootId, done: bool) -> Outcome {
    let result = c.object_rooted()?;
    put(c, result, "value", value)?;
    put_bool(c, result, "done", done)?;
    Ok(result)
}

/// A key a class member is installed under.
#[derive(Clone, Copy)]
pub(crate) enum Key {
    Name(&'static str),
    Symbol(Slot),
}

fn key_root(c: &mut Context<'_>, key: Key) -> Outcome {
    Ok(match key {
        Key::Name(name) => c.string_rooted(name),
        Key::Symbol(slot) => intrinsics(c)?.slot(slot),
    })
}

/// Methods as Node assigns them: enumerable, writable and configurable.
/// Each is `(key, operation, function name)`.
pub(crate) fn define_methods(c: &mut Context<'_>, target: RootId, methods: &[(Key, &str, &str)]) -> Result<(), RootedError> {
    for &(key, operation, name) in methods {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = key_root(c, key)?;
        c.define_data_property_rooted(target, key, function, true, true, true)?;
    }
    Ok(())
}

/// An accessor pair as `Object.defineProperties` installs it without `enumerable`.
pub(crate) fn define_accessor(
    c: &mut Context<'_>,
    target: RootId,
    name: &str,
    getter: RootId,
    setter: Option<RootId>,
) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(setter.unwrap_or_else(|| c.undefined())),
        enumerable: Some(false),
        configurable: Some(false),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields)?;
    Ok(())
}

fn make_symbol(c: &mut Context<'_>, source: SymbolSource) -> Outcome {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let registered = |c: &mut Context<'_>, key: &str| {
        let description = c.string_rooted(key);
        call_method(c, symbol, "for", &[description])
    };
    match source {
        SymbolSource::Private(description) => Ok(c.symbol_rooted(Some(description))),
        SymbolSource::PrivateName(description) => Ok(c.private_symbol_rooted(Some(description))),
        SymbolSource::Registered(key) => registered(c, key),
        SymbolSource::WellKnown(name) => get(c, symbol, name),
        SymbolSource::WellKnownOr(name, key) => {
            let known = get(c, symbol, name)?;
            if is_undefined(c, known) { registered(c, key) } else { Ok(known) }
        }
    }
}

/// The `stream` module, built on first use with the classes and utilities it exports.
/// The symbols and scheduled operations are published first: the classes' members are keyed
/// by them.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    if let Some(intrinsics) = c.host_mut().shared_state().borrow().stream_intrinsics {
        if !is_undefined(c, intrinsics.module) {
            return Ok(intrinsics.module);
        }
    }
    let built = build(c);
    if built.is_err() {
        c.host_mut().shared_state().borrow_mut().stream_intrinsics = None;
    }
    built
}

fn build(c: &mut Context<'_>) -> Outcome {
    let undefined = c.undefined();
    let mut slots = [undefined; Slot::ALL.len()];
    for (index, slot) in Slot::ALL.iter().enumerate() {
        let symbol = make_symbol(c, slot.source())?;
        slots[index] = c.retain(symbol)?;
    }
    let mut ticks = [undefined; Tick::ALL.len()];
    for (index, tick) in Tick::ALL.iter().enumerate() {
        let (operation, name) = tick.operation();
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        ticks[index] = c.retain(function)?;
    }
    let nop = c.host_function_named(crate::host::shared_vm::operation("streamNop"), "nop")?;
    let nop = c.retain(nop)?;
    let global = c.global_root()?;
    let promise = get(c, global, "Promise")?;
    let promise_prototype = get(c, promise, "prototype")?;
    let promise_then = get(c, promise_prototype, "then")?;
    let promise_then = c.retain(promise_then)?;
    let promise = c.retain(promise)?;
    let mut intrinsics = Intrinsics { slots, classes: [undefined; Class::COUNT], ticks, nop, promise, promise_then, module: undefined };
    c.host_mut().shared_state().borrow_mut().stream_intrinsics = Some(intrinsics);
    let emitter = get(c, global, "__nodeEventEmitter")?;
    let buffer = super::buffer_shared_vm::fast_buffer(c)?;
    let stream = legacy::build(c, emitter)?;
    let readable_state = state::build_readable_state(c)?;
    let writable_state = state::build_writable_state(c)?;
    let readable = readable::build(c, stream, readable_state)?;
    let writable = writable::build(c, stream, writable_state)?;
    let duplex = duplex::build_duplex(c, readable, writable)?;
    let transform = duplex::build_transform(c, duplex)?;
    let pass_through = duplex::build_pass_through(c, transform)?;
    let duplex_side = duplex::build_duplex_side(c, duplex)?;
    for (class, value) in [
        (Class::Emitter, emitter),
        (Class::Stream, stream),
        (Class::Readable, readable),
        (Class::Writable, writable),
        (Class::Duplex, duplex),
        (Class::Transform, transform),
        (Class::PassThrough, pass_through),
        (Class::ReadableState, readable_state),
        (Class::WritableState, writable_state),
        (Class::Buffer, buffer),
        (Class::DuplexSide, duplex_side),
    ] {
        intrinsics.classes[class as usize] = c.retain(value)?;
    }
    c.host_mut().shared_state().borrow_mut().stream_intrinsics = Some(intrinsics);
    exports(c, stream, readable, writable, duplex, transform, pass_through)?;
    intrinsics.module = c.retain(stream)?;
    c.host_mut().shared_state().borrow_mut().stream_intrinsics = Some(intrinsics);
    Ok(stream)
}

fn exports(
    c: &mut Context<'_>,
    stream: RootId,
    readable: RootId,
    writable: RootId,
    duplex: RootId,
    transform: RootId,
    pass_through: RootId,
) -> Result<(), RootedError> {
    utils::define_predicates(c, stream, utils::PUBLIC_PREDICATES)?;
    for (name, value) in [
        ("Readable", readable),
        ("Writable", writable),
        ("Duplex", duplex),
        ("Transform", transform),
        ("PassThrough", pass_through),
    ] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(stream, key, value, true, true, true)?;
    }
    define_methods(
        c,
        stream,
        &[
            (Key::Name("duplexPair"), "streamDuplexPair", "duplexPair"),
            (Key::Name("pipeline"), "streamPipeline", "pipeline"),
            (Key::Name("addAbortSignal"), "streamAddAbortSignal", "addAbortSignal"),
            (Key::Name("finished"), "streamFinished", "eos"),
            (Key::Name("destroy"), "streamDestroyer", "destroyer"),
            (Key::Name("setDefaultHighWaterMark"), "streamSetDefaultHighWaterMark", "setDefaultHighWaterMark"),
            (Key::Name("getDefaultHighWaterMark"), "streamGetDefaultHighWaterMark", "getDefaultHighWaterMark"),
        ],
    )?;
    let promises = promises_module(c)?;
    let getter = c.host_function_named_with_data(crate::host::shared_vm::operation("streamConstant"), "get", promises)?;
    let key = c.string_rooted("promises");
    let undefined = c.undefined();
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(undefined),
        enumerable: Some(true),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(stream, key, fields)?;
    for (name, member) in [("pipeline", "pipeline"), ("finished", "finished")] {
        let target = get(c, stream, name)?;
        let value = get(c, promises, member)?;
        custom_promisify(c, target, value)?;
    }
    let key = c.string_rooted("Stream");
    c.define_data_property_rooted(stream, key, stream, true, true, true)?;
    define_methods(
        c,
        stream,
        &[
            (Key::Name("_isArrayBufferView"), "streamIsArrayBufferView", "isView"),
            (Key::Name("_isUint8Array"), "streamIsUint8Array", "isUint8Array"),
            (Key::Name("_uint8ArrayToBuffer"), "streamUint8ArrayToBuffer", "_uint8ArrayToBuffer"),
        ],
    )?;
    Ok(())
}

/// `util.promisify.custom` on `target`: an enumerable getter of its promise form.
fn custom_promisify(c: &mut Context<'_>, target: RootId, value: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let description = c.string_rooted("nodejs.util.promisify.custom");
    let key = call_method(c, symbol, "for", &[description])?;
    let getter = c.host_function_named_with_data(crate::host::shared_vm::operation("streamConstant"), "get", value)?;
    let undefined = c.undefined();
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(undefined),
        enumerable: Some(true),
        configurable: Some(false),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields)?;
    Ok(())
}

/// `stream/promises`: `finished` and `pipeline` returning promises.
fn promises_module(c: &mut Context<'_>) -> Outcome {
    let module = c.object_rooted()?;
    define_methods(
        c,
        module,
        &[
            (Key::Name("finished"), "streamPromisesFinished", "finished"),
            (Key::Name("pipeline"), "streamPromisesPipeline", "pipeline"),
        ],
    )?;
    Ok(module)
}

/// `stream/promises`, the object `stream.promises` returns.
pub(crate) fn promises(c: &mut Context<'_>) -> Outcome {
    let stream = module(c)?;
    get(c, stream, "promises")
}

/// A getter returning the value it closes over.
pub(crate) fn constant(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    c.host_function_data()
}

pub(crate) fn nop(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Ok(c.undefined())
}

/// The library modules Node's other built-ins load from its stream implementation, as the
/// exports those modules have.
pub(crate) fn library_module(c: &mut Context<'_>, relative: &str) -> Result<Option<RootId>, RootedError> {
    let stream = module(c)?;
    let class = |c: &mut Context<'_>, name: &str| get(c, stream, name).map(Some);
    match relative {
        "stream.js" => Ok(Some(stream)),
        "stream/promises.js" => promises(c).map(Some),
        "internal/streams/readable.js" => class(c, "Readable"),
        "internal/streams/writable.js" => class(c, "Writable"),
        "internal/streams/duplex.js" => class(c, "Duplex"),
        "internal/streams/transform.js" => class(c, "Transform"),
        "internal/streams/passthrough.js" => class(c, "PassThrough"),
        "internal/streams/end-of-stream.js" => class(c, "finished"),
        "internal/streams/legacy.js"
        | "internal/streams/state.js"
        | "internal/streams/utils.js"
        | "internal/streams/destroy.js"
        | "internal/streams/pipeline.js"
        | "internal/streams/add-abort-signal.js" => internal_module(c, relative).map(Some),
        _ => Ok(None),
    }
}

/// An internal module's export object, made once and kept in the builtin cache.
fn internal_module(c: &mut Context<'_>, relative: &str) -> Outcome {
    let key = format!("\0stream:{relative}");
    let shared = c.host_mut().shared_state();
    if let Some(module) = shared.borrow().builtin_cache.get(&key).copied() {
        return Ok(module);
    }
    let stream = module(c)?;
    let module = c.object_rooted()?;
    let members: &[(&str, &str, &str)] = match relative {
        "internal/streams/legacy.js" => &[("prependListener", "streamPrependListener", "prependListener")],
        "internal/streams/state.js" => &[
            ("getHighWaterMark", "streamGetHighWaterMark", "getHighWaterMark"),
            ("getDefaultHighWaterMark", "streamGetDefaultHighWaterMark", "getDefaultHighWaterMark"),
            ("setDefaultHighWaterMark", "streamSetDefaultHighWaterMark", "setDefaultHighWaterMark"),
        ],
        "internal/streams/destroy.js" => &[
            ("construct", "streamConstructImpl", "construct"),
            ("destroyer", "streamDestroyer", "destroyer"),
            ("destroy", "streamDestroy", "destroy"),
            ("undestroy", "streamUndestroy", "undestroy"),
            ("errorOrDestroy", "streamErrorOrDestroy", "errorOrDestroy"),
        ],
        "internal/streams/pipeline.js" => &[
            ("pipelineImpl", "streamPipelineImpl", "pipelineImpl"),
            ("pipeline", "streamPipeline", "pipeline"),
        ],
        "internal/streams/add-abort-signal.js" => &[
            ("addAbortSignal", "streamAddAbortSignal", "addAbortSignal"),
            ("addAbortSignalNoValidate", "streamAddAbortSignalNoValidate", ""),
        ],
        _ => &[],
    };
    for &(name, operation, function_name) in members {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), function_name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(module, key, function, true, true, true)?;
    }
    if relative == "internal/streams/legacy.js" {
        let key = c.string_rooted("Stream");
        c.define_data_property_rooted(module, key, stream, true, true, true)?;
    }
    if relative == "internal/streams/utils.js" {
        utils::define_utils_exports(c, module)?;
    }
    let retained = c.retain(module)?;
    shared.borrow_mut().builtin_cache.insert(key, retained);
    Ok(module)
}
