//! `TransformStream` and `TransformStreamDefaultController`
//! (`lib/internal/webstreams/transformstream.js`).

use super::readable::field::{CONTROLLER, HIGH_WATER_MARK, SELF, SIZE, STORED_ERROR, STREAM};
use super::readable::{self, default_can_close_or_enqueue, default_controller_close, default_controller_enqueue, default_controller_error};
use super::writable::{self, controller_error_if_needed};
use super::{
    algorithm, arg, attach, callback, class, custom_inspect, deferred, dictionary, flag, get, install, instance,
    is_undefined, literal, optional, put, put_flag, record, run_algorithm, settle_undefined,
    size_algorithm, slots, this_state, throw, upon, Brand, ClassMember, Context, Dictionary, Done, Outcome, WebError,
};
use crate::modules::host_members::{captured, current, function, members};
use quench_runtime::RootId;

members!(Member, "webTransform", [
    Construct,
    Readable,
    Writable,
    Inspect,
    ControllerConstruct,
    DesiredSize,
    Enqueue,
    Error,
    Terminate,
    ControllerInspect,
    SinkWrite,
    SinkClose,
    SinkAbort,
    SourcePull,
    SourceCancel,
    BackpressureChanged,
    TransformFulfilled,
    TransformRejected,
    DefaultTransform,
    FinishFulfilled,
    FinishRejected,
    CancelFulfilled,
    CancelRejected,
]);

/// A transform stream's and its controller's fields.
mod field {
    pub(super) const READABLE: &str = "readable";
    pub(super) const WRITABLE: &str = "writable";
    pub(super) const BACKPRESSURE: &str = "backpressure";
    pub(super) const BACKPRESSURE_CHANGE: &str = "backpressureChange";
    pub(super) const TRANSFORM: &str = "transform";
    pub(super) const FLUSH: &str = "flush";
    pub(super) const CANCEL: &str = "cancel";
    pub(super) const FINISH: &str = "finishPromise";
    /// Whether the stream's transform is the identity (`defaultTransformAlgorithm`).
    pub(super) const IDENTITY: &str = "identity";
}
use field::*;

/// What a finish reaction completes: the sink's close or abort, or the source's cancel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Finish {
    Close,
    Abort,
}

fn readable_controller(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let readable = get(c, stream, READABLE)?;
    get(c, readable, CONTROLLER)
}

/// `initializeTransformStream(stream, startPromise, ...)`; returns the stream's slots.
fn initialize(
    c: &mut Context<'_>,
    object: RootId,
    start: RootId,
    writable_high_water_mark: f64,
    writable_size: RootId,
    readable_high_water_mark: f64,
    readable_size: RootId,
) -> Outcome {
    let stream = record(c, &[(SELF, object)])?;
    attach(c, object, Brand::TransformStream, stream)?;
    let write = function(c, Member::SinkWrite, "", &[stream])?;
    let close = function(c, Member::SinkClose, "", &[stream])?;
    let abort = function(c, Member::SinkAbort, "", &[stream])?;
    let writable = writable::create_writable(c, start, write, close, abort, writable_high_water_mark, writable_size)?;
    let pull = function(c, Member::SourcePull, "", &[stream])?;
    let cancel = function(c, Member::SourceCancel, "", &[stream])?;
    let readable = readable::create_readable(c, start, pull, cancel, readable_high_water_mark, readable_size)?;
    put(c, stream, READABLE, readable)?;
    put(c, stream, WRITABLE, writable)?;
    set_backpressure(c, stream, true)?;
    Ok(stream)
}

fn set_backpressure(c: &mut Context<'_>, stream: RootId, backpressure: bool) -> Done {
    let change = get(c, stream, BACKPRESSURE_CHANGE)?;
    if !is_undefined(c, change) {
        settle_undefined(c, change)?;
    }
    let change = deferred(c)?;
    put(c, stream, BACKPRESSURE_CHANGE, change)?;
    put_flag(c, stream, BACKPRESSURE, backpressure)
}

fn error(c: &mut Context<'_>, stream: RootId, failure: RootId) -> Done {
    let controller = readable_controller(c, stream)?;
    default_controller_error(c, controller, failure)?;
    error_writable_and_unblock_write(c, stream, failure)
}

fn error_writable_and_unblock_write(c: &mut Context<'_>, stream: RootId, failure: RootId) -> Done {
    let controller = get(c, stream, CONTROLLER)?;
    clear_algorithms(c, controller)?;
    let writable = get(c, stream, WRITABLE)?;
    let writable_controller = get(c, writable, CONTROLLER)?;
    controller_error_if_needed(c, writable_controller, failure)?;
    unblock_write(c, stream)
}

fn unblock_write(c: &mut Context<'_>, stream: RootId) -> Done {
    if flag(c, stream, BACKPRESSURE)? {
        set_backpressure(c, stream, false)?;
    }
    Ok(())
}

fn clear_algorithms(c: &mut Context<'_>, controller: RootId) -> Done {
    let undefined = c.undefined();
    for name in [TRANSFORM, FLUSH, CANCEL] {
        put(c, controller, name, undefined)?;
    }
    put_flag(c, controller, IDENTITY, false)
}

/// `transformStreamDefaultControllerEnqueue(controller, chunk)`.
fn enqueue(c: &mut Context<'_>, controller: RootId, chunk: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    let readable_controller = readable_controller(c, stream)?;
    if !default_can_close_or_enqueue(c, readable_controller)? {
        return Err(throw(c, WebError::InvalidState("Unable to enqueue")));
    }
    if super::attempt(default_controller_enqueue(c, readable_controller, chunk))?.is_err() {
        let readable = get(c, stream, READABLE)?;
        let stored = get(c, readable, STORED_ERROR)?;
        error_writable_and_unblock_write(c, stream, stored)?;
        let stored = get(c, readable, STORED_ERROR)?;
        return Err(c.throw(stored));
    }
    let backpressure = readable::has_backpressure(c, readable_controller)?;
    if backpressure != flag(c, stream, BACKPRESSURE)? {
        set_backpressure(c, stream, true)?;
    }
    Ok(())
}

/// The controller's enqueue on behalf of the module's own transforms.
pub(crate) fn controller_enqueue(c: &mut Context<'_>, controller: RootId, chunk: RootId) -> Done {
    enqueue(c, controller, chunk)
}

/// `transformStreamDefaultControllerTerminate(controller)`.
fn terminate(c: &mut Context<'_>, controller: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    let readable_controller = readable_controller(c, stream)?;
    default_controller_close(c, readable_controller)?;
    let failure = super::create_error(c, WebError::InvalidState("TransformStream has been terminated"))?;
    error_writable_and_unblock_write(c, stream, failure)
}

/// `transformStreamDefaultControllerPerformTransform(controller, chunk)`, an async function.
fn perform_transform(c: &mut Context<'_>, controller: RootId, chunk: RootId) -> Outcome {
    let promise = deferred(c)?;
    let object = get(c, controller, SELF)?;
    let transformed = if flag(c, controller, IDENTITY)? {
        let identity = function(c, Member::DefaultTransform, "", &[controller])?;
        super::promise_call(c, identity, object, &[chunk])?
    } else {
        run_algorithm(c, controller, TRANSFORM, &[chunk, object])?
    };
    // `return await transformed`, failing the stream when it rejects.
    let awaited = super::resolved(c, transformed)?;
    upon(c, awaited, Some(Member::TransformFulfilled), Some(Member::TransformRejected), &[promise, controller])?;
    Ok(promise)
}

fn sink_write(c: &mut Context<'_>, stream: RootId, chunk: RootId) -> Outcome {
    let controller = get(c, stream, CONTROLLER)?;
    if flag(c, stream, BACKPRESSURE)? {
        let change = get(c, stream, BACKPRESSURE_CHANGE)?;
        return upon(c, change, Some(Member::BackpressureChanged), None, &[stream, controller, chunk]);
    }
    perform_transform(c, controller, chunk)
}

/// The sink's close and abort and the source's cancel share one finish promise.
fn finish(c: &mut Context<'_>, stream: RootId, kind: Finish, reason: RootId) -> Outcome {
    let controller = get(c, stream, CONTROLLER)?;
    let existing = get(c, controller, FINISH)?;
    if !is_undefined(c, existing) {
        return Ok(existing);
    }
    let promise = deferred(c)?;
    put(c, controller, FINISH, promise)?;
    let started = match kind {
        Finish::Close => {
            let object = get(c, controller, SELF)?;
            run_algorithm(c, controller, FLUSH, &[object])?
        }
        Finish::Abort => run_algorithm(c, controller, CANCEL, &[reason])?,
    };
    clear_algorithms(c, controller)?;
    let close = c.boolean(kind == Finish::Close);
    upon(c, started, Some(Member::FinishFulfilled), Some(Member::FinishRejected), &[stream, promise, reason, close])?;
    Ok(promise)
}

fn sink_abort(c: &mut Context<'_>, stream: RootId, reason: RootId) -> Outcome {
    // An async function returning the finish promise.
    let promise = deferred(c)?;
    let finished = finish(c, stream, Finish::Abort, reason)?;
    c.resolve_promise_rooted(promise, finished)?;
    Ok(promise)
}

fn source_pull(c: &mut Context<'_>, stream: RootId) -> Outcome {
    set_backpressure(c, stream, false)?;
    get(c, stream, BACKPRESSURE_CHANGE)
}

fn source_cancel(c: &mut Context<'_>, stream: RootId, reason: RootId) -> Outcome {
    let controller = get(c, stream, CONTROLLER)?;
    let existing = get(c, controller, FINISH)?;
    if !is_undefined(c, existing) {
        return Ok(existing);
    }
    let promise = deferred(c)?;
    put(c, controller, FINISH, promise)?;
    let cancelled = run_algorithm(c, controller, CANCEL, &[reason])?;
    clear_algorithms(c, controller)?;
    upon(c, cancelled, Some(Member::CancelFulfilled), Some(Member::CancelRejected), &[stream, promise, reason])?;
    Ok(promise)
}

/// A transform stream over host callbacks, invoked as a transformer's are:
/// `transform(chunk, controller)`, `flush(controller)` and `cancel(reason)` (absent ones are
/// Node's defaults). Returns the stream object.
pub(crate) fn create_transform(c: &mut Context<'_>, object: RootId, transform: RootId, flush: RootId, cancel: RootId) -> Outcome {
    let start = deferred(c)?;
    let undefined = c.undefined();
    let stream = initialize(c, object, start, 1.0, undefined, 0.0, undefined)?;
    let identity = is_undefined(c, transform);
    setup_controller(c, stream, &[(TRANSFORM, transform), (FLUSH, flush), (CANCEL, cancel)], false, identity)?;
    settle_undefined(c, start)?;
    Ok(object)
}

fn setup_controller(c: &mut Context<'_>, stream: RootId, algorithms: &[(&str, RootId)], internal: bool, identity: bool) -> Outcome {
    let controller = record(c, &[(STREAM, stream)])?;
    for &(name, value) in algorithms {
        put(c, controller, name, value)?;
    }
    put_flag(c, controller, algorithm::INTERNAL, internal)?;
    put_flag(c, controller, IDENTITY, identity)?;
    let object = instance(c, Brand::TransformController, controller)?;
    put(c, controller, SELF, object)?;
    put(c, stream, CONTROLLER, controller)?;
    Ok(controller)
}

fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let transformer = dictionary(c, args, 0, "transformer", Dictionary::Objects)?;
    let writable_strategy = dictionary(c, args, 1, "writableStrategy", Dictionary::ObjectsAndNull)?;
    let readable_strategy = dictionary(c, args, 2, "readableStrategy", Dictionary::ObjectsAndNull)?;
    for (member, name) in [("readableType", "transformer.readableType"), ("writableType", "transformer.writableType")] {
        let value = optional(c, transformer, member)?;
        if !is_undefined(c, value) {
            return Err(throw(c, WebError::ArgValueRange(name, value)));
        }
    }
    let start = optional(c, transformer, "start")?;
    let readable_high_water_mark = optional(c, readable_strategy, HIGH_WATER_MARK)?;
    let readable_size = optional(c, readable_strategy, SIZE)?;
    let writable_high_water_mark = optional(c, writable_strategy, HIGH_WATER_MARK)?;
    let writable_size = optional(c, writable_strategy, SIZE)?;
    let readable_high_water_mark = super::high_water_mark(c, readable_high_water_mark, 0.0)?;
    let readable_size = size_algorithm(c, readable_size)?;
    let writable_high_water_mark = super::high_water_mark(c, writable_high_water_mark, 1.0)?;
    let writable_size = size_algorithm(c, writable_size)?;
    let start_promise = deferred(c)?;
    let stream = initialize(c, this, start_promise, writable_high_water_mark, writable_size, readable_high_water_mark, readable_size)?;
    let mut algorithms = [(TRANSFORM, transformer), (FLUSH, transformer), (CANCEL, transformer), (algorithm::SOURCE, transformer)];
    for (index, (member, name)) in
        [(TRANSFORM, "transformer.transform"), (FLUSH, "transformer.flush"), (CANCEL, "transformer.cancel")].into_iter().enumerate()
    {
        algorithms[index].1 = if super::is_nullish(c, transformer) { c.undefined() } else { callback(c, transformer, member, name)? };
    }
    let identity = is_undefined(c, algorithms[0].1);
    let controller = setup_controller(c, stream, &algorithms, false, identity)?;
    if is_undefined(c, start) {
        settle_undefined(c, start_promise)?;
    } else {
        let object = get(c, controller, SELF)?;
        let started = c.call_rooted(start, transformer, &[object])?;
        c.resolve_promise_rooted(start_promise, started)?;
    }
    Ok(c.undefined())
}

const STREAM_MEMBERS: [ClassMember<Member>; 2] =
    [ClassMember::Getter("readable", Member::Readable), ClassMember::Getter("writable", Member::Writable)];

const CONTROLLER_MEMBERS: [ClassMember<Member>; 4] = [
    ClassMember::Getter("desiredSize", Member::DesiredSize),
    ClassMember::Method("enqueue", Member::Enqueue, 0),
    ClassMember::Method("error", Member::Error, 0),
    ClassMember::Method("terminate", Member::Terminate, 0),
];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Done {
    let stream = class(c, Brand::TransformStream, Member::Construct, 0, Member::Inspect)?;
    let prototype = get(c, stream, "prototype")?;
    install(c, prototype, &STREAM_MEMBERS)?;
    put(c, table, Brand::TransformStream.name(), stream)?;
    let controller = class(c, Brand::TransformController, Member::ControllerConstruct, 0, Member::ControllerInspect)?;
    let prototype = get(c, controller, "prototype")?;
    install(c, prototype, &CONTROLLER_MEMBERS)?;
    put(c, table, Brand::TransformController.name(), controller)
}

/// The readable or writable side of a transform stream's slots, as an object.
pub(crate) fn side(c: &mut Context<'_>, stream: RootId, readable: bool) -> Outcome {
    let side = get(c, stream, if readable { READABLE } else { WRITABLE })?;
    get(c, side, SELF)
}

/// The slots of a transform stream's object, for the encoding and compression classes.
pub(crate) fn stream_slots(c: &mut Context<'_>, object: RootId) -> Outcome {
    slots(c, object)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => construct(c, this, args),
        Member::Readable | Member::Writable => {
            let stream = this_state(c, this, Brand::TransformStream)?;
            side(c, stream, member == Member::Readable)
        }
        Member::Inspect => {
            let stream = this_state(c, this, Brand::TransformStream)?;
            let readable = side(c, stream, true)?;
            let writable = side(c, stream, false)?;
            let backpressure = get(c, stream, BACKPRESSURE)?;
            let data = literal(c, &[("readable", readable), ("writable", writable), ("backpressure", backpressure)])?;
            custom_inspect(c, this, Brand::TransformStream.name(), data, args)
        }
        Member::ControllerConstruct => Err(throw(c, WebError::IllegalConstructor)),
        Member::DesiredSize => {
            let controller = this_state(c, this, Brand::TransformController)?;
            let stream = get(c, controller, STREAM)?;
            let readable_controller = readable_controller(c, stream)?;
            readable::desired_size_value(c, readable_controller)
        }
        Member::Enqueue => {
            let controller = this_state(c, this, Brand::TransformController)?;
            let chunk = arg(c, args, 0);
            enqueue(c, controller, chunk)?;
            Ok(c.undefined())
        }
        Member::Error => {
            let controller = this_state(c, this, Brand::TransformController)?;
            let stream = get(c, controller, STREAM)?;
            let reason = arg(c, args, 0);
            error(c, stream, reason)?;
            Ok(c.undefined())
        }
        Member::Terminate => {
            let controller = this_state(c, this, Brand::TransformController)?;
            terminate(c, controller)?;
            Ok(c.undefined())
        }
        Member::ControllerInspect => {
            let controller = this_state(c, this, Brand::TransformController)?;
            let stream = get(c, controller, STREAM)?;
            let stream = get(c, stream, SELF)?;
            let data = literal(c, &[("stream", stream)])?;
            custom_inspect(c, this, Brand::TransformController.name(), data, args)
        }
        Member::SinkWrite => {
            let [stream] = captured(c)?;
            let chunk = arg(c, args, 0);
            sink_write(c, stream, chunk)
        }
        Member::SinkClose => {
            let [stream] = captured(c)?;
            let undefined = c.undefined();
            finish(c, stream, Finish::Close, undefined)
        }
        Member::SinkAbort => {
            let [stream] = captured(c)?;
            let reason = arg(c, args, 0);
            sink_abort(c, stream, reason)
        }
        Member::SourcePull => {
            let [stream] = captured(c)?;
            source_pull(c, stream)
        }
        Member::SourceCancel => {
            let [stream] = captured(c)?;
            let reason = arg(c, args, 0);
            source_cancel(c, stream, reason)
        }
        Member::BackpressureChanged => {
            let [stream, controller, chunk] = captured(c)?;
            let writable = get(c, stream, WRITABLE)?;
            if writable::state(c, writable)? == writable::State::Erroring {
                let stored = get(c, writable, STORED_ERROR)?;
                return Err(c.throw(stored));
            }
            perform_transform(c, controller, chunk)
        }
        Member::TransformFulfilled => {
            let [promise] = captured(c)?;
            let value = arg(c, args, 0);
            super::settle(c, promise, value)?;
            Ok(c.undefined())
        }
        Member::TransformRejected => {
            let [promise, controller] = captured(c)?;
            let failure = arg(c, args, 0);
            let stream = get(c, controller, STREAM)?;
            error(c, stream, failure)?;
            super::fail(c, promise, failure)?;
            Ok(c.undefined())
        }
        Member::DefaultTransform => {
            let [controller] = captured(c)?;
            let chunk = arg(c, args, 0);
            enqueue(c, controller, chunk)?;
            Ok(c.undefined())
        }
        Member::FinishFulfilled => {
            let [stream, promise, reason, close] = captured(c)?;
            let readable = get(c, stream, READABLE)?;
            if readable::state(c, readable)? == readable::State::Errored {
                let stored = get(c, readable, STORED_ERROR)?;
                super::fail(c, promise, stored)?;
            } else {
                let controller = get(c, readable, CONTROLLER)?;
                if c.truthy_rooted(close)? {
                    default_controller_close(c, controller)?;
                } else {
                    default_controller_error(c, controller, reason)?;
                }
                settle_undefined(c, promise)?;
            }
            Ok(c.undefined())
        }
        Member::FinishRejected => {
            let [stream, promise] = captured(c)?;
            let failure = arg(c, args, 0);
            let readable = get(c, stream, READABLE)?;
            let controller = get(c, readable, CONTROLLER)?;
            default_controller_error(c, controller, failure)?;
            super::fail(c, promise, failure)?;
            Ok(c.undefined())
        }
        Member::CancelFulfilled => {
            let [stream, promise, reason] = captured(c)?;
            let writable = get(c, stream, WRITABLE)?;
            if writable::state(c, writable)? == writable::State::Errored {
                let stored = get(c, writable, STORED_ERROR)?;
                super::fail(c, promise, stored)?;
            } else {
                let controller = get(c, writable, CONTROLLER)?;
                controller_error_if_needed(c, controller, reason)?;
                unblock_write(c, stream)?;
                settle_undefined(c, promise)?;
            }
            Ok(c.undefined())
        }
        Member::CancelRejected => {
            let [stream, promise] = captured(c)?;
            let failure = arg(c, args, 0);
            let writable = get(c, stream, WRITABLE)?;
            let controller = get(c, writable, CONTROLLER)?;
            controller_error_if_needed(c, controller, failure)?;
            unblock_write(c, stream)?;
            super::fail(c, promise, failure)?;
            Ok(c.undefined())
        }
    }
}
