//! `WritableStream`, `WritableStreamDefaultWriter` and `WritableStreamDefaultController`
//! (`lib/internal/webstreams/writablestream.js`).

use super::readable::field::{
    CLOSED, CLOSED_PROMISE, CONTROLLER, HIGH_WATER_MARK, QUEUE, QUEUE_SIZE, SELF, SIZE, STARTED, STATE, STORED_ERROR,
    STREAM,
};
use super::readable::{dequeue, enqueue_with_size, peek, queue_length, reset_queue};
use super::{
    algorithm, arg, attach, callback, class, custom_inspect, deferred, dictionary, fail, flag, get,
    install, install_symbol_getter, instance, intrinsic, is_pending, is_undefined, key, list, literal, mark_handled,
    num, optional, put, put_flag, put_num, record, rejected, rejected_with, released_error, resolved_undefined,
    run_algorithm, settle_undefined, size_algorithm, slots, state_of, this_state, throw, upon, Brand, ClassMember,
    Context, Dictionary, Done, Outcome, WebError,
};
use crate::modules::host_members::{captured, current, function, members};
use quench_runtime::{RootId, RootedError};

members!(Member, "webWritable", [
    Construct,
    Locked,
    Abort,
    Close,
    GetWriter,
    Inspect,
    IsErrored,
    IsWritable,
    WriterConstruct,
    WriterClosed,
    WriterDesiredSize,
    WriterReady,
    WriterAbort,
    WriterClose,
    WriterReleaseLock,
    WriterWrite,
    WriterInspect,
    ControllerConstruct,
    ControllerSignal,
    ControllerError,
    ControllerInspect,
    ErrorFunction,
    Nop,
    StartFulfilled,
    StartRejected,
    WriteFulfilled,
    WriteRejected,
    CloseFulfilled,
    CloseRejected,
    AbortFulfilled,
    AbortRejected,
]);

/// A writable stream's state.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Writable,
    Erroring,
    Closed,
    Errored,
}

impl State {
    fn from(value: f64) -> Self {
        match value as u8 {
            0 => Self::Writable,
            1 => Self::Erroring,
            2 => Self::Closed,
            _ => Self::Errored,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Writable => "writable",
            Self::Erroring => "erroring",
            Self::Closed => "closed",
            Self::Errored => "errored",
        }
    }
}

/// A writable stream's, writer's and controller's fields beyond the readable family's.
pub(crate) mod field {
    pub(crate) const WRITER: &str = "writer";
    pub(crate) const BACKPRESSURE: &str = "backpressure";
    pub(crate) const CLOSE_REQUEST: &str = "closeRequest";
    pub(crate) const IN_FLIGHT_WRITE: &str = "inFlightWriteRequest";
    pub(crate) const IN_FLIGHT_CLOSE: &str = "inFlightCloseRequest";
    pub(crate) const PENDING_ABORT: &str = "pendingAbortRequest";
    pub(crate) const WRITE_REQUESTS: &str = "writeRequests";
    pub(crate) const READY: &str = "ready";
    pub(crate) const WRITE: &str = "write";
    pub(crate) const CLOSE: &str = "close";
    pub(crate) const ABORT: &str = "abort";
    pub(crate) const ABORT_CONTROLLER: &str = "abortController";
    /// A pending abort request's fields.
    pub(crate) const PROMISE: &str = "promise";
    pub(crate) const REASON: &str = "reason";
    pub(crate) const WAS_ALREADY_ERRORING: &str = "wasAlreadyErroring";
}
use field::*;

pub(crate) fn state(c: &mut Context<'_>, stream: RootId) -> Result<State, RootedError> {
    Ok(State::from(num(c, stream, STATE)?))
}

fn set_state(c: &mut Context<'_>, stream: RootId, state: State) -> Done {
    put_num(c, stream, STATE, state as usize as f64)
}

fn present(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    Ok(!is_undefined(c, value))
}

fn clear(c: &mut Context<'_>, object: RootId, name: &str) -> Done {
    let undefined = c.undefined();
    put(c, object, name, undefined)
}

/// `createWritableStreamState()` on `object`.
fn initialize_stream(c: &mut Context<'_>, object: RootId) -> Outcome {
    let closed = deferred(c)?;
    let requests = list::new(c)?;
    let stream = record(c, &[(SELF, object), (CLOSED_PROMISE, closed), (WRITE_REQUESTS, requests)])?;
    set_state(c, stream, State::Writable)?;
    put_flag(c, stream, BACKPRESSURE, false)?;
    attach(c, object, Brand::WritableStream, stream)?;
    let holder = literal(c, &[(PROMISE, closed)])?;
    let symbol = intrinsic(c, key::IS_CLOSED)?;
    c.define_data_property_rooted(object, symbol, holder, true, true, true)?;
    Ok(stream)
}

fn set_error_function(c: &mut Context<'_>, object: RootId, function: RootId) -> Done {
    let symbol = intrinsic(c, key::ERROR_FUNCTION)?;
    c.define_data_property_rooted(object, symbol, function, true, true, true)?;
    Ok(())
}

pub(crate) fn is_locked(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    present(c, stream, WRITER)
}

/// `setupWritableStreamDefaultWriter(writer, stream)`, after the brand check.
fn initialize_writer(c: &mut Context<'_>, object: RootId, stream: RootId) -> Outcome {
    if is_locked(c, stream)? {
        return Err(throw(c, WebError::InvalidState("WritableStream is locked")));
    }
    let writer = record(c, &[(SELF, object), (STREAM, stream)])?;
    attach(c, object, Brand::Writer, writer)?;
    put(c, stream, WRITER, object)?;
    let (ready, closed) = match state(c, stream)? {
        State::Writable => {
            let ready = if !close_queued_or_in_flight(c, stream)? && flag(c, stream, BACKPRESSURE)? {
                deferred(c)?
            } else {
                resolved_undefined(c)?
            };
            (ready, deferred(c)?)
        }
        State::Erroring => {
            let error = get(c, stream, STORED_ERROR)?;
            let ready = rejected(c, error)?;
            mark_handled(c, ready)?;
            (ready, deferred(c)?)
        }
        State::Closed => (resolved_undefined(c)?, resolved_undefined(c)?),
        State::Errored => {
            let error = get(c, stream, STORED_ERROR)?;
            let ready = rejected(c, error)?;
            let closed = rejected(c, error)?;
            mark_handled(c, ready)?;
            mark_handled(c, closed)?;
            (ready, closed)
        }
    };
    put(c, writer, READY, ready)?;
    put(c, writer, CLOSED, closed)?;
    Ok(writer)
}

/// `new WritableStreamDefaultWriter(stream)` from the module; returns its slots.
pub(crate) fn writer(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let classes = super::classes(c)?;
    let class = get(c, classes, Brand::Writer.name())?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    initialize_writer(c, object, stream)
}

/// `writableStreamAbort(stream, reason)`.
pub(crate) fn abort(c: &mut Context<'_>, stream: RootId, reason: RootId) -> Outcome {
    let current = state(c, stream)?;
    if matches!(current, State::Closed | State::Errored) {
        return resolved_undefined(c);
    }
    let controller = get(c, stream, CONTROLLER)?;
    let abort_controller = get(c, controller, ABORT_CONTROLLER)?;
    let method = get(c, abort_controller, "abort")?;
    c.call_rooted(method, abort_controller, &[reason])?;
    let pending = get(c, stream, PENDING_ABORT)?;
    if !is_undefined(c, pending) {
        return get(c, pending, PROMISE);
    }
    let already = state(c, stream)? == State::Erroring;
    let reason = if already { c.undefined() } else { reason };
    let promise = deferred(c)?;
    let already_value = c.boolean(already);
    let request = record(c, &[(PROMISE, promise), (REASON, reason), (WAS_ALREADY_ERRORING, already_value)])?;
    put(c, stream, PENDING_ABORT, request)?;
    if !already {
        start_erroring(c, stream, reason)?;
    }
    Ok(promise)
}

/// `writableStreamClose(stream)`.
pub(crate) fn close(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let current = state(c, stream)?;
    if matches!(current, State::Closed | State::Errored) {
        return rejected_with(c, WebError::InvalidState("WritableStream is closed"));
    }
    let promise = deferred(c)?;
    put(c, stream, CLOSE_REQUEST, promise)?;
    let writer = get(c, stream, WRITER)?;
    if !is_undefined(c, writer) && flag(c, stream, BACKPRESSURE)? && current == State::Writable {
        let writer = slots(c, writer)?;
        let ready = get(c, writer, READY)?;
        settle_undefined(c, ready)?;
    }
    let controller = get(c, stream, CONTROLLER)?;
    controller_close(c, controller)?;
    Ok(promise)
}

fn update_backpressure(c: &mut Context<'_>, stream: RootId, backpressure: bool) -> Done {
    let writer = get(c, stream, WRITER)?;
    if !is_undefined(c, writer) && flag(c, stream, BACKPRESSURE)? != backpressure {
        let writer = slots(c, writer)?;
        if backpressure {
            let ready = deferred(c)?;
            put(c, writer, READY, ready)?;
        } else {
            let ready = get(c, writer, READY)?;
            settle_undefined(c, ready)?;
        }
    }
    put_flag(c, stream, BACKPRESSURE, backpressure)
}

fn start_erroring(c: &mut Context<'_>, stream: RootId, reason: RootId) -> Done {
    let controller = get(c, stream, CONTROLLER)?;
    set_state(c, stream, State::Erroring)?;
    put(c, stream, STORED_ERROR, reason)?;
    let writer = get(c, stream, WRITER)?;
    if !is_undefined(c, writer) {
        let writer = slots(c, writer)?;
        ensure_ready_rejected(c, writer, reason)?;
    }
    if !has_operation_in_flight(c, stream)? && flag(c, controller, STARTED)? {
        finish_erroring(c, stream)?;
    }
    Ok(())
}

fn reject_close_and_closed(c: &mut Context<'_>, stream: RootId) -> Done {
    let stored = get(c, stream, STORED_ERROR)?;
    let request = get(c, stream, CLOSE_REQUEST)?;
    if !is_undefined(c, request) {
        fail(c, request, stored)?;
        clear(c, stream, CLOSE_REQUEST)?;
    }
    let closed = get(c, stream, CLOSED_PROMISE)?;
    mark_handled(c, closed)?;
    fail(c, closed, stored)?;
    let writer = get(c, stream, WRITER)?;
    if !is_undefined(c, writer) {
        let writer = slots(c, writer)?;
        let closed = get(c, writer, CLOSED)?;
        mark_handled(c, closed)?;
        fail(c, closed, stored)?;
    }
    Ok(())
}

fn has_operation_in_flight(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    Ok(present(c, stream, IN_FLIGHT_WRITE)? || present(c, stream, IN_FLIGHT_CLOSE)?)
}

fn finish_in_flight_write_with_error(c: &mut Context<'_>, stream: RootId, error: RootId) -> Done {
    let request = get(c, stream, IN_FLIGHT_WRITE)?;
    fail(c, request, error)?;
    clear(c, stream, IN_FLIGHT_WRITE)?;
    deal_with_rejection(c, stream, error)
}

fn finish_in_flight_write(c: &mut Context<'_>, stream: RootId) -> Done {
    let request = get(c, stream, IN_FLIGHT_WRITE)?;
    settle_undefined(c, request)?;
    clear(c, stream, IN_FLIGHT_WRITE)
}

fn finish_in_flight_close_with_error(c: &mut Context<'_>, stream: RootId, error: RootId) -> Done {
    let request = get(c, stream, IN_FLIGHT_CLOSE)?;
    fail(c, request, error)?;
    clear(c, stream, IN_FLIGHT_CLOSE)?;
    let pending = get(c, stream, PENDING_ABORT)?;
    if !is_undefined(c, pending) {
        let promise = get(c, pending, PROMISE)?;
        fail(c, promise, error)?;
        clear(c, stream, PENDING_ABORT)?;
    }
    deal_with_rejection(c, stream, error)
}

fn finish_in_flight_close(c: &mut Context<'_>, stream: RootId) -> Done {
    let request = get(c, stream, IN_FLIGHT_CLOSE)?;
    settle_undefined(c, request)?;
    clear(c, stream, IN_FLIGHT_CLOSE)?;
    if state(c, stream)? == State::Erroring {
        clear(c, stream, STORED_ERROR)?;
        let pending = get(c, stream, PENDING_ABORT)?;
        if !is_undefined(c, pending) {
            let promise = get(c, pending, PROMISE)?;
            settle_undefined(c, promise)?;
            clear(c, stream, PENDING_ABORT)?;
        }
    }
    set_state(c, stream, State::Closed)?;
    let writer = get(c, stream, WRITER)?;
    if !is_undefined(c, writer) {
        let writer = slots(c, writer)?;
        let closed = get(c, writer, CLOSED)?;
        settle_undefined(c, closed)?;
    }
    let closed = get(c, stream, CLOSED_PROMISE)?;
    settle_undefined(c, closed)
}

fn finish_erroring(c: &mut Context<'_>, stream: RootId) -> Done {
    set_state(c, stream, State::Errored)?;
    let controller = get(c, stream, CONTROLLER)?;
    reset_queue(c, controller)?;
    let stored = get(c, stream, STORED_ERROR)?;
    let requests = get(c, stream, WRITE_REQUESTS)?;
    for request in list::drain(c, requests)? {
        fail(c, request, stored)?;
    }
    let pending = get(c, stream, PENDING_ABORT)?;
    if is_undefined(c, pending) {
        return reject_close_and_closed(c, stream);
    }
    clear(c, stream, PENDING_ABORT)?;
    let promise = get(c, pending, PROMISE)?;
    if flag(c, pending, WAS_ALREADY_ERRORING)? {
        fail(c, promise, stored)?;
        return reject_close_and_closed(c, stream);
    }
    let reason = get(c, pending, REASON)?;
    let result = run_algorithm(c, controller, ABORT, &[reason])?;
    clear_algorithms(c, controller)?;
    upon(c, result, Some(Member::AbortFulfilled), Some(Member::AbortRejected), &[stream, promise]).map(drop)
}

fn deal_with_rejection(c: &mut Context<'_>, stream: RootId, error: RootId) -> Done {
    if state(c, stream)? == State::Writable {
        return start_erroring(c, stream, error);
    }
    finish_erroring(c, stream)
}

pub(crate) fn close_queued_or_in_flight(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    Ok(present(c, stream, CLOSE_REQUEST)? || present(c, stream, IN_FLIGHT_CLOSE)?)
}

/// `writableStreamDefaultWriterWrite(writer, chunk)`.
pub(crate) fn writer_write(c: &mut Context<'_>, writer: RootId, chunk: RootId) -> Outcome {
    let stream = get(c, writer, STREAM)?;
    let controller = get(c, stream, CONTROLLER)?;
    let chunk_size = chunk_size(c, controller, chunk)?;
    let current_stream = get(c, writer, STREAM)?;
    if !super::same(c, stream, current_stream)? {
        return rejected_with(c, WebError::InvalidState("Mismatched WritableStreams"));
    }
    let current = state(c, stream)?;
    if current == State::Errored {
        let error = get(c, stream, STORED_ERROR)?;
        return rejected(c, error);
    }
    if close_queued_or_in_flight(c, stream)? || current == State::Closed {
        return rejected_with(c, WebError::InvalidState("WritableStream is closed"));
    }
    if current == State::Erroring {
        let error = get(c, stream, STORED_ERROR)?;
        return rejected(c, error);
    }
    let promise = deferred(c)?;
    let requests = get(c, stream, WRITE_REQUESTS)?;
    list::push(c, requests, promise)?;
    controller_write(c, controller, chunk, chunk_size)?;
    Ok(promise)
}

/// `writableStreamDefaultWriterRelease(writer)`.
pub(crate) fn writer_release(c: &mut Context<'_>, writer: RootId) -> Done {
    let stream = get(c, writer, STREAM)?;
    let released = released_error(c, key::WRITER_RELEASED, "Writer has been released")?;
    ensure_ready_rejected(c, writer, released)?;
    ensure_closed_rejected(c, writer, released)?;
    clear(c, stream, WRITER)?;
    clear(c, writer, STREAM)
}

fn writer_desired_size(c: &mut Context<'_>, writer: RootId) -> Outcome {
    let stream = get(c, writer, STREAM)?;
    match state(c, stream)? {
        State::Errored | State::Erroring => Ok(c.null()),
        State::Closed => Ok(c.number(0.0)),
        State::Writable => {
            let controller = get(c, stream, CONTROLLER)?;
            let size = desired_size(c, controller)?;
            Ok(c.number(size))
        }
    }
}

fn ensure_ready_rejected(c: &mut Context<'_>, writer: RootId, error: RootId) -> Done {
    replace_rejected(c, writer, READY, error)
}

fn ensure_closed_rejected(c: &mut Context<'_>, writer: RootId, error: RootId) -> Done {
    replace_rejected(c, writer, CLOSED, error)
}

/// Rejects the writer's pending promise `name`, or replaces a settled one with a rejection.
fn replace_rejected(c: &mut Context<'_>, writer: RootId, name: &str, error: RootId) -> Done {
    let promise = get(c, writer, name)?;
    if is_pending(c, promise)? {
        fail(c, promise, error)?;
    } else {
        let promise = rejected(c, error)?;
        put(c, writer, name, promise)?;
    }
    let promise = get(c, writer, name)?;
    mark_handled(c, promise)
}

/// `writableStreamDefaultWriterCloseWithErrorPropagation(writer)`.
pub(crate) fn close_with_error_propagation(c: &mut Context<'_>, writer: RootId) -> Outcome {
    let stream = get(c, writer, STREAM)?;
    let current = state(c, stream)?;
    if close_queued_or_in_flight(c, stream)? || current == State::Closed {
        return resolved_undefined(c);
    }
    if current == State::Errored {
        let error = get(c, stream, STORED_ERROR)?;
        return rejected(c, error);
    }
    close(c, stream)
}

/// `writableStreamDefaultWriterClose(writer)`.
pub(crate) fn writer_close(c: &mut Context<'_>, writer: RootId) -> Outcome {
    let stream = get(c, writer, STREAM)?;
    close(c, stream)
}

// ---- The controller

fn controller_write(c: &mut Context<'_>, controller: RootId, chunk: RootId, chunk_size: RootId) -> Done {
    if let Err(failure) = super::attempt(enqueue_with_size(c, controller, chunk, chunk_size))? {
        return error_if_needed(c, controller, failure);
    }
    let stream = get(c, controller, STREAM)?;
    if !close_queued_or_in_flight(c, stream)? && state(c, stream)? == State::Writable {
        let backpressure = backpressure(c, controller)?;
        update_backpressure(c, stream, backpressure)?;
    }
    advance_queue_if_needed(c, controller)
}

fn process_write(c: &mut Context<'_>, controller: RootId, chunk: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    let requests = get(c, stream, WRITE_REQUESTS)?;
    let request = list::shift(c, requests)?;
    put(c, stream, IN_FLIGHT_WRITE, request)?;
    let object = get(c, controller, SELF)?;
    let written = run_algorithm(c, controller, WRITE, &[chunk, object])?;
    upon(c, written, Some(Member::WriteFulfilled), Some(Member::WriteRejected), &[controller]).map(drop)
}

fn process_close(c: &mut Context<'_>, controller: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    let request = get(c, stream, CLOSE_REQUEST)?;
    put(c, stream, IN_FLIGHT_CLOSE, request)?;
    clear(c, stream, CLOSE_REQUEST)?;
    dequeue(c, controller)?;
    let closed = run_algorithm(c, controller, CLOSE, &[])?;
    clear_algorithms(c, controller)?;
    upon(c, closed, Some(Member::CloseFulfilled), Some(Member::CloseRejected), &[stream]).map(drop)
}

fn desired_size(c: &mut Context<'_>, controller: RootId) -> Result<f64, RootedError> {
    Ok(num(c, controller, HIGH_WATER_MARK)? - num(c, controller, QUEUE_SIZE)?)
}

fn chunk_size(c: &mut Context<'_>, controller: RootId, chunk: RootId) -> Outcome {
    let size = get(c, controller, SIZE)?;
    if !c.is_callable_rooted(size)? {
        return Ok(c.number(1.0));
    }
    let undefined = c.undefined();
    match super::attempt(c.call_rooted(size, undefined, &[chunk]))? {
        Ok(value) => Ok(value),
        Err(failure) => {
            error_if_needed(c, controller, failure)?;
            Ok(c.number(1.0))
        }
    }
}

pub(crate) fn error_if_needed(c: &mut Context<'_>, controller: RootId, error: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    if state(c, stream)? == State::Writable {
        controller_error(c, controller, error)?;
    }
    Ok(())
}

fn controller_error(c: &mut Context<'_>, controller: RootId, error: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    clear_algorithms(c, controller)?;
    start_erroring(c, stream, error)
}

fn controller_close(c: &mut Context<'_>, controller: RootId) -> Done {
    let sentinel = close_sentinel(c)?;
    let zero = c.number(0.0);
    enqueue_with_size(c, controller, sentinel, zero)?;
    advance_queue_if_needed(c, controller)
}

/// Node's `kCloseSentinel`, the queue entry a close request is.
fn close_sentinel(c: &mut Context<'_>) -> Outcome {
    let intrinsics = super::intrinsics(c)?;
    let sentinel = get(c, intrinsics, key::CLOSE_SENTINEL)?;
    if !is_undefined(c, sentinel) {
        return Ok(sentinel);
    }
    let sentinel = record(c, &[])?;
    put(c, intrinsics, key::CLOSE_SENTINEL, sentinel)?;
    Ok(sentinel)
}

fn clear_algorithms(c: &mut Context<'_>, controller: RootId) -> Done {
    for name in [WRITE, CLOSE, ABORT, SIZE] {
        clear(c, controller, name)?;
    }
    Ok(())
}

fn backpressure(c: &mut Context<'_>, controller: RootId) -> Result<bool, RootedError> {
    Ok(desired_size(c, controller)? <= 0.0)
}

fn advance_queue_if_needed(c: &mut Context<'_>, controller: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    if !flag(c, controller, STARTED)? || present(c, stream, IN_FLIGHT_WRITE)? {
        return Ok(());
    }
    if state(c, stream)? == State::Erroring {
        return finish_erroring(c, stream);
    }
    if queue_length(c, controller)? == 0 {
        return Ok(());
    }
    let value = peek(c, controller)?;
    let sentinel = close_sentinel(c)?;
    if super::same(c, value, sentinel)? {
        process_close(c, controller)
    } else {
        process_write(c, controller, value)
    }
}

/// `setupWritableStreamDefaultController`; `start` produces the start algorithm's result.
fn setup_controller(
    c: &mut Context<'_>,
    stream: RootId,
    algorithms: &[(&str, RootId)],
    internal: bool,
    high_water_mark: f64,
    size: RootId,
    start: impl FnOnce(&mut Context<'_>, RootId) -> Outcome,
) -> Outcome {
    let queue = list::new(c)?;
    let family = crate::modules::events_shared_vm::event_family(c)?;
    let abort_controller_class = get(c, family, "abortControllerCtor")?;
    let abort_controller = c.construct_rooted(abort_controller_class, abort_controller_class, &[])?;
    let controller = record(c, &[(STREAM, stream), (QUEUE, queue), (SIZE, size), (ABORT_CONTROLLER, abort_controller)])?;
    for &(name, value) in algorithms {
        put(c, controller, name, value)?;
    }
    put_flag(c, controller, algorithm::INTERNAL, internal)?;
    put_num(c, controller, QUEUE_SIZE, 0.0)?;
    put_num(c, controller, HIGH_WATER_MARK, high_water_mark)?;
    put_flag(c, controller, STARTED, false)?;
    let object = instance(c, Brand::WritableController, controller)?;
    put(c, controller, SELF, object)?;
    put(c, stream, CONTROLLER, controller)?;
    let stream_object = get(c, stream, SELF)?;
    let error_function = function(c, Member::ErrorFunction, "bound error", &[controller])?;
    set_error_function(c, stream_object, error_function)?;
    let backpressure = backpressure(c, controller)?;
    update_backpressure(c, stream, backpressure)?;
    let started = start(c, object)?;
    let promise = deferred(c)?;
    c.resolve_promise_rooted(promise, started)?;
    upon(c, promise, Some(Member::StartFulfilled), Some(Member::StartRejected), &[controller, stream])?;
    Ok(controller)
}

/// `createWritableStream(start, write, close, abort, highWaterMark, size)` over internal
/// algorithms (host functions returning promises); `start` is the start algorithm's
/// result. Returns the stream's internal slots.
pub(crate) fn create_writable(
    c: &mut Context<'_>,
    start: RootId,
    write: RootId,
    close_algorithm: RootId,
    abort_algorithm: RootId,
    high_water_mark: f64,
    size: RootId,
) -> Outcome {
    let classes = super::classes(c)?;
    let class = get(c, classes, Brand::WritableStream.name())?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let stream = initialize_stream(c, object)?;
    let algorithms = [(WRITE, write), (CLOSE, close_algorithm), (ABORT, abort_algorithm)];
    setup_controller(c, stream, &algorithms, true, high_water_mark, size, |_, _| Ok(start))?;
    Ok(stream)
}

fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let sink = dictionary(c, args, 0, "sink", Dictionary::Objects)?;
    let strategy = dictionary(c, args, 1, "strategy", Dictionary::ObjectsAndNull)?;
    let kind = optional(c, sink, "type")?;
    if !is_undefined(c, kind) {
        return Err(throw(c, WebError::ArgValueRange("type", kind)));
    }
    let stream = initialize_stream(c, this)?;
    let nop = function(c, Member::Nop, "", &[])?;
    set_error_function(c, this, nop)?;
    let size = optional(c, strategy, SIZE)?;
    let size = size_algorithm(c, size)?;
    let high_water_mark = optional(c, strategy, HIGH_WATER_MARK)?;
    let high_water_mark = super::high_water_mark(c, high_water_mark, 1.0)?;
    let start = optional(c, sink, "start")?;
    let mut algorithms = [(WRITE, sink), (CLOSE, sink), (ABORT, sink), (algorithm::SOURCE, sink)];
    for (index, (member, name)) in [(WRITE, "sink.write"), (CLOSE, "sink.close"), (ABORT, "sink.abort")].into_iter().enumerate() {
        algorithms[index].1 = if super::is_nullish(c, sink) { c.undefined() } else { callback(c, sink, member, name)? };
    }
    setup_controller(c, stream, &algorithms, false, high_water_mark, size, |c, controller| {
        if c.truthy_rooted(start)? {
            c.call_rooted(start, sink, &[controller])
        } else {
            Ok(c.undefined())
        }
    })?;
    Ok(c.undefined())
}

fn writer_of(c: &mut Context<'_>, this: RootId) -> Result<Result<RootId, RootId>, RootedError> {
    match state_of(c, this, Brand::Writer)? {
        Some(writer) => Ok(Ok(writer)),
        None => Ok(Err(rejected_with(c, WebError::InvalidThis(Brand::Writer.name()))?)),
    }
}

/// The writer's stream, or the rejection a detached writer's methods return.
fn bound_writer(c: &mut Context<'_>, this: RootId) -> Result<Result<RootId, RootId>, RootedError> {
    let writer = match writer_of(c, this)? {
        Ok(writer) => writer,
        Err(rejection) => return Ok(Err(rejection)),
    };
    let stream = get(c, writer, STREAM)?;
    if is_undefined(c, stream) {
        return Ok(Err(rejected_with(c, WebError::InvalidState("Writer is not bound to a WritableStream"))?));
    }
    Ok(Ok(writer))
}

const STREAM_MEMBERS: [ClassMember<Member>; 4] = [
    ClassMember::Getter("locked", Member::Locked),
    ClassMember::Method("abort", Member::Abort, 0),
    ClassMember::Method("close", Member::Close, 0),
    ClassMember::Method("getWriter", Member::GetWriter, 0),
];

const WRITER_MEMBERS: [ClassMember<Member>; 7] = [
    ClassMember::Getter("closed", Member::WriterClosed),
    ClassMember::Getter("ready", Member::WriterReady),
    ClassMember::Getter("desiredSize", Member::WriterDesiredSize),
    ClassMember::Method("abort", Member::WriterAbort, 0),
    ClassMember::Method("close", Member::WriterClose, 0),
    ClassMember::Method("releaseLock", Member::WriterReleaseLock, 0),
    ClassMember::Method("write", Member::WriterWrite, 0),
];

const CONTROLLER_MEMBERS: [ClassMember<Member>; 2] =
    [ClassMember::Getter("signal", Member::ControllerSignal), ClassMember::Method("error", Member::ControllerError, 0)];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Done {
    let stream = class(c, Brand::WritableStream, Member::Construct, 0, Member::Inspect)?;
    let prototype = get(c, stream, "prototype")?;
    install(c, prototype, &STREAM_MEMBERS)?;
    install_symbol_getter(c, prototype, key::IS_ERRORED, Member::IsErrored, "get [nodejs.stream.errored]")?;
    install_symbol_getter(c, prototype, key::IS_WRITABLE, Member::IsWritable, "get [nodejs.stream.writable]")?;
    put(c, table, Brand::WritableStream.name(), stream)?;
    let writer = class(c, Brand::Writer, Member::WriterConstruct, 1, Member::WriterInspect)?;
    let prototype = get(c, writer, "prototype")?;
    install(c, prototype, &WRITER_MEMBERS)?;
    put(c, table, Brand::Writer.name(), writer)?;
    let controller = class(c, Brand::WritableController, Member::ControllerConstruct, 0, Member::ControllerInspect)?;
    let prototype = get(c, controller, "prototype")?;
    install(c, prototype, &CONTROLLER_MEMBERS)?;
    put(c, table, Brand::WritableController.name(), controller)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            super::require_new(c, Brand::WritableStream.name())?;
            construct(c, this, args)
        }
        Member::Locked => {
            let stream = this_state(c, this, Brand::WritableStream)?;
            let locked = is_locked(c, stream)?;
            Ok(c.boolean(locked))
        }
        Member::Abort | Member::Close => {
            let Some(stream) = state_of(c, this, Brand::WritableStream)? else {
                return rejected_with(c, WebError::InvalidThis(Brand::WritableStream.name()));
            };
            if is_locked(c, stream)? {
                return rejected_with(c, WebError::InvalidState("WritableStream is locked"));
            }
            if member == Member::Abort {
                let reason = arg(c, args, 0);
                return abort(c, stream, reason);
            }
            if close_queued_or_in_flight(c, stream)? {
                return rejected_with(c, WebError::InvalidState("Failure closing WritableStream"));
            }
            close(c, stream)
        }
        Member::GetWriter => {
            let stream = this_state(c, this, Brand::WritableStream)?;
            let writer = writer(c, stream)?;
            get(c, writer, SELF)
        }
        Member::Inspect => {
            let stream = this_state(c, this, Brand::WritableStream)?;
            let locked = is_locked(c, stream)?;
            let locked = c.boolean(locked);
            let state = state(c, stream)?.name();
            let state = c.string_rooted(state);
            let data = literal(c, &[("locked", locked), ("state", state)])?;
            custom_inspect(c, this, Brand::WritableStream.name(), data, args)
        }
        Member::IsErrored | Member::IsWritable => {
            let stream = slots(c, this)?;
            let wanted = if member == Member::IsErrored { State::Errored } else { State::Writable };
            let value = state(c, stream)? == wanted;
            Ok(c.boolean(value))
        }
        Member::WriterConstruct => {
            super::require_new(c, Brand::Writer.name())?;
            let stream = arg(c, args, 0);
            let Some(stream) = state_of(c, stream, Brand::WritableStream)? else {
                return Err(throw(c, WebError::ArgType("stream", "WritableStream", stream)));
            };
            initialize_writer(c, this, stream)?;
            Ok(c.undefined())
        }
        Member::WriterClosed | Member::WriterReady => match writer_of(c, this)? {
            Ok(writer) => get(c, writer, if member == Member::WriterClosed { CLOSED } else { READY }),
            Err(rejection) => Ok(rejection),
        },
        Member::WriterDesiredSize => {
            let writer = this_state(c, this, Brand::Writer)?;
            let stream = get(c, writer, STREAM)?;
            if is_undefined(c, stream) {
                return Err(throw(c, WebError::InvalidState("Writer is not bound to a WritableStream")));
            }
            writer_desired_size(c, writer)
        }
        Member::WriterAbort => match bound_writer(c, this)? {
            Ok(writer) => {
                let stream = get(c, writer, STREAM)?;
                let reason = arg(c, args, 0);
                abort(c, stream, reason)
            }
            Err(rejection) => Ok(rejection),
        },
        Member::WriterClose => match bound_writer(c, this)? {
            Ok(writer) => {
                let stream = get(c, writer, STREAM)?;
                if close_queued_or_in_flight(c, stream)? {
                    return rejected_with(c, WebError::InvalidState("Failure to close WritableStream"));
                }
                writer_close(c, writer)
            }
            Err(rejection) => Ok(rejection),
        },
        Member::WriterReleaseLock => {
            let writer = this_state(c, this, Brand::Writer)?;
            let stream = get(c, writer, STREAM)?;
            if !is_undefined(c, stream) {
                writer_release(c, writer)?;
            }
            Ok(c.undefined())
        }
        Member::WriterWrite => match bound_writer(c, this)? {
            Ok(writer) => {
                let chunk = arg(c, args, 0);
                writer_write(c, writer, chunk)
            }
            Err(rejection) => Ok(rejection),
        },
        Member::WriterInspect => {
            let writer = this_state(c, this, Brand::Writer)?;
            let stream = get(c, writer, STREAM)?;
            let (stream_object, desired) = if is_undefined(c, stream) {
                return Err(throw(c, WebError::InvalidState("Writer is not bound to a WritableStream")));
            } else {
                (get(c, stream, SELF)?, writer_desired_size(c, writer)?)
            };
            let closed = get(c, writer, CLOSED)?;
            let ready = get(c, writer, READY)?;
            let data = literal(c, &[("stream", stream_object), ("close", closed), ("ready", ready), ("desiredSize", desired)])?;
            custom_inspect(c, this, Brand::Writer.name(), data, args)
        }
        Member::ControllerConstruct => {
            super::require_new(c, Brand::WritableController.name())?;
            Err(throw(c, WebError::IllegalConstructor))
        }
        Member::ControllerSignal => {
            let controller = this_state(c, this, Brand::WritableController)?;
            let abort_controller = get(c, controller, ABORT_CONTROLLER)?;
            get(c, abort_controller, "signal")
        }
        Member::ControllerError => {
            let controller = this_state(c, this, Brand::WritableController)?;
            let stream = get(c, controller, STREAM)?;
            if state(c, stream)? == State::Writable {
                let error = arg(c, args, 0);
                controller_error(c, controller, error)?;
            }
            Ok(c.undefined())
        }
        Member::ControllerInspect => {
            let controller = this_state(c, this, Brand::WritableController)?;
            let stream = get(c, controller, STREAM)?;
            let stream = get(c, stream, SELF)?;
            let data = literal(c, &[("stream", stream)])?;
            custom_inspect(c, this, Brand::WritableController.name(), data, args)
        }
        Member::ErrorFunction => {
            let [controller] = captured(c)?;
            let stream = get(c, controller, STREAM)?;
            if state(c, stream)? == State::Writable {
                let error = arg(c, args, 0);
                controller_error(c, controller, error)?;
            }
            Ok(c.undefined())
        }
        Member::Nop => Ok(c.undefined()),
        Member::StartFulfilled => {
            let [controller] = captured(c)?;
            put_flag(c, controller, STARTED, true)?;
            advance_queue_if_needed(c, controller)?;
            Ok(c.undefined())
        }
        Member::StartRejected => {
            let [controller, stream] = captured(c)?;
            put_flag(c, controller, STARTED, true)?;
            let error = arg(c, args, 0);
            deal_with_rejection(c, stream, error)?;
            Ok(c.undefined())
        }
        Member::WriteFulfilled => {
            let [controller] = captured(c)?;
            let stream = get(c, controller, STREAM)?;
            finish_in_flight_write(c, stream)?;
            dequeue(c, controller)?;
            if !close_queued_or_in_flight(c, stream)? && state(c, stream)? == State::Writable {
                let backpressure = backpressure(c, controller)?;
                update_backpressure(c, stream, backpressure)?;
            }
            advance_queue_if_needed(c, controller)?;
            Ok(c.undefined())
        }
        Member::WriteRejected => {
            let [controller] = captured(c)?;
            let stream = get(c, controller, STREAM)?;
            if state(c, stream)? == State::Writable {
                clear_algorithms(c, controller)?;
            }
            let error = arg(c, args, 0);
            finish_in_flight_write_with_error(c, stream, error)?;
            Ok(c.undefined())
        }
        Member::CloseFulfilled => {
            let [stream] = captured(c)?;
            finish_in_flight_close(c, stream)?;
            Ok(c.undefined())
        }
        Member::CloseRejected => {
            let [stream] = captured(c)?;
            let error = arg(c, args, 0);
            finish_in_flight_close_with_error(c, stream, error)?;
            Ok(c.undefined())
        }
        Member::AbortFulfilled => {
            let [stream, promise] = captured(c)?;
            settle_undefined(c, promise)?;
            reject_close_and_closed(c, stream)?;
            Ok(c.undefined())
        }
        Member::AbortRejected => {
            let [stream, promise] = captured(c)?;
            let error = arg(c, args, 0);
            fail(c, promise, error)?;
            reject_close_and_closed(c, stream)?;
            Ok(c.undefined())
        }
    }
}
