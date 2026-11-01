//! `ReadableStream`, its default reader and default controller, the read requests readers
//! queue, `tee` for default streams, `ReadableStream.from` and async iteration
//! (`lib/internal/webstreams/readablestream.js`).

use super::{
    key, algorithm, arg, attach, callback, class, custom_inspect, deferred, dictionary, done_result, flag, get,
    install, install_symbol_getter, instance, intrinsic, is_nullish,
    is_undefined, list, literal, mark_handled, num, optional, put, put_flag, put_num, read_result,
    record, rejected, rejected_with, released_error, resolved, resolved_undefined, run_algorithm, settle,
    settle_undefined, size_algorithm, slots, state_of, this_state, throw, upon, Brand,
    ClassMember, Context, Dictionary, Done, Outcome, WebError,
};
use crate::modules::host_members::{captured, current, function, members};
use quench_runtime::{RootId, RootedError};

members!(Member, "webReadable", [
    Construct,
    Locked,
    Cancel,
    GetReader,
    PipeThrough,
    PipeTo,
    Tee,
    Values,
    From,
    Inspect,
    IsDisturbed,
    IsErrored,
    IsReadable,
    ReaderConstruct,
    ReaderRead,
    ReaderReleaseLock,
    ReaderClosed,
    ReaderCancel,
    ReaderInspect,
    ControllerConstruct,
    ControllerDesiredSize,
    ControllerClose,
    ControllerEnqueue,
    ControllerError,
    ControllerInspect,
    /// The stream's `[kControllerErrorFunction]`: its controller's `error`, bound.
    ErrorFunction,
    /// The no-op `[kControllerErrorFunction]` a constructed stream starts with.
    Nop,
    StartFulfilled,
    StartRejected,
    PullFulfilled,
    PullRejected,
    /// A cancel's result, fulfilled with `undefined`.
    Ignore,
    IteratorNext,
    IteratorReturn,
    IteratorSelf,
    IteratorNextSteps,
    IteratorReturnSteps,
    IteratorReturnFulfilled,
    IteratorReturnRejected,
    TeePull,
    TeeCancel1,
    TeeCancel2,
    TeeChunk,
    TeeClose,
    TeeReaderError,
    FromPull,
    FromPulled,
    FromCancel,
    FromCanceled,
    FromSyncNext,
]);

/// A readable stream's state.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Readable,
    Closed,
    Errored,
}

impl State {
    fn from(value: f64) -> Self {
        match value as u8 {
            0 => Self::Readable,
            1 => Self::Closed,
            _ => Self::Errored,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Readable => "readable",
            Self::Closed => "closed",
            Self::Errored => "errored",
        }
    }
}

/// The record fields of streams, readers, controllers and read requests.
pub(crate) mod field {
    pub(crate) const SELF: &str = "self";
    pub(crate) const STATE: &str = "state";
    pub(crate) const DISTURBED: &str = "disturbed";
    pub(crate) const READER: &str = "reader";
    pub(crate) const STORED_ERROR: &str = "storedError";
    pub(crate) const CONTROLLER: &str = "controller";
    pub(crate) const CLOSED_PROMISE: &str = "closedPromise";
    pub(crate) const STREAM: &str = "stream";
    pub(crate) const CLOSED: &str = "closed";
    pub(crate) const REQUESTS: &str = "requests";
    pub(crate) const QUEUE: &str = "queue";
    pub(crate) const QUEUE_SIZE: &str = "queueTotalSize";
    pub(crate) const HIGH_WATER_MARK: &str = "highWaterMark";
    pub(crate) const STARTED: &str = "started";
    pub(crate) const CLOSE_REQUESTED: &str = "closeRequested";
    pub(crate) const PULLING: &str = "pulling";
    pub(crate) const PULL_AGAIN: &str = "pullAgain";
    pub(crate) const PULL: &str = "pull";
    pub(crate) const CANCEL: &str = "cancel";
    pub(crate) const SIZE: &str = "size";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const PROMISE: &str = "promise";
    pub(crate) const VALUE: &str = "value";
}
use field::*;

/// How a read request completes: the reader whose `read` queued it, or an algorithm
/// reading on its own behalf.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadSteps {
    /// `DefaultReadRequest`: settles `promise` with `{ value, done }`.
    Default,
    /// `ReadIntoRequest`: as `Default`, but a close carries the view it filled.
    Into,
    /// An async iterator's read.
    Iterator,
    /// A `pipeTo` read.
    Pipe,
    /// A default tee's read.
    Tee,
    /// A byte tee's default read.
    ByteTee,
    /// A byte tee's BYOB read.
    ByteTeeInto,
    /// A host consumer's read: `chunk`, `close` and `error` are its functions.
    Hook,
}

impl ReadSteps {
    const ALL: [Self; 8] =
        [Self::Default, Self::Into, Self::Iterator, Self::Pipe, Self::Tee, Self::ByteTee, Self::ByteTeeInto, Self::Hook];
}

/// The fields of a read request beyond its kind.
pub(crate) mod request {
    pub(crate) const READER: &str = "reader";
    pub(crate) const ITERATION: &str = "iteration";
    pub(crate) const WRITER: &str = "writer";
    pub(crate) const PIPE: &str = "pipe";
    pub(crate) const TEE: &str = "tee";
    pub(crate) const BRANCH: &str = "branch";
    pub(crate) const CHUNK: &str = "chunk";
    pub(crate) const CLOSE: &str = "close";
    pub(crate) const ERROR: &str = "error";
}

pub(crate) fn new_request(c: &mut Context<'_>, steps: ReadSteps, fields: &[(&str, RootId)]) -> Outcome {
    let request = record(c, fields)?;
    put_num(c, request, KIND, steps as usize as f64)?;
    Ok(request)
}

pub(crate) fn steps_kind(c: &mut Context<'_>, request: RootId) -> Result<ReadSteps, RootedError> {
    let kind = num(c, request, KIND)? as usize;
    Ok(ReadSteps::ALL[kind])
}

/// A read request's chunk steps.
pub(crate) fn chunk_steps(c: &mut Context<'_>, request: RootId, chunk: RootId) -> Done {
    match steps_kind(c, request)? {
        ReadSteps::Default | ReadSteps::Into => {
            let promise = get(c, request, PROMISE)?;
            let result = read_result(c, chunk, false)?;
            settle(c, promise, result)
        }
        ReadSteps::Iterator => {
            let iteration = get(c, request, request::ITERATION)?;
            let undefined = c.undefined();
            put(c, iteration, iteration::CURRENT, undefined)?;
            let promise = get(c, request, PROMISE)?;
            let result = read_result(c, chunk, false)?;
            settle(c, promise, result)
        }
        ReadSteps::Pipe => super::pipe::read_chunk(c, request, chunk),
        ReadSteps::Tee => {
            let tee = get(c, request, request::TEE)?;
            let task = function(c, Member::TeeChunk, "", &[tee, chunk])?;
            c.queue_microtask_rooted(task, &[])
        }
        ReadSteps::ByteTee | ReadSteps::ByteTeeInto => super::byte::tee_chunk(c, request, chunk),
        ReadSteps::Hook => hook(c, request, request::CHUNK, chunk),
    }
}

/// A read request's close steps; `value` is the view a BYOB read filled, if any.
pub(crate) fn close_steps(c: &mut Context<'_>, request: RootId, value: RootId) -> Done {
    match steps_kind(c, request)? {
        ReadSteps::Default => {
            let promise = get(c, request, PROMISE)?;
            let undefined = c.undefined();
            let result = read_result(c, undefined, true)?;
            settle(c, promise, result)
        }
        ReadSteps::Into => {
            let promise = get(c, request, PROMISE)?;
            let result = read_result(c, value, true)?;
            settle(c, promise, result)
        }
        ReadSteps::Iterator => {
            finish_iteration(c, request)?;
            let promise = get(c, request, PROMISE)?;
            let undefined = c.undefined();
            let result = done_result(c, undefined)?;
            settle(c, promise, result)
        }
        ReadSteps::Pipe => {
            let promise = get(c, request, PROMISE)?;
            let done = c.boolean(true);
            settle(c, promise, done)
        }
        ReadSteps::Tee => {
            let tee = get(c, request, request::TEE)?;
            let task = function(c, Member::TeeClose, "", &[tee])?;
            let undefined = c.undefined();
            crate::modules::shared_event_loop::queue_next_tick(c, task, undefined, &[])
        }
        ReadSteps::ByteTee | ReadSteps::ByteTeeInto => super::byte::tee_close(c, request, value),
        ReadSteps::Hook => hook(c, request, request::CLOSE, value),
    }
}

/// A read request's error steps.
pub(crate) fn error_steps(c: &mut Context<'_>, request: RootId, error: RootId) -> Done {
    match steps_kind(c, request)? {
        ReadSteps::Default | ReadSteps::Into | ReadSteps::Pipe => {
            let promise = get(c, request, PROMISE)?;
            super::fail(c, promise, error)
        }
        ReadSteps::Iterator => {
            finish_iteration(c, request)?;
            let promise = get(c, request, PROMISE)?;
            super::fail(c, promise, error)
        }
        ReadSteps::Tee => {
            let tee = get(c, request, request::TEE)?;
            put_flag(c, tee, tee::READING, false)
        }
        ReadSteps::ByteTee | ReadSteps::ByteTeeInto => {
            let tee = get(c, request, request::TEE)?;
            put_flag(c, tee, tee::READING, false)
        }
        ReadSteps::Hook => hook(c, request, request::ERROR, error),
    }
}

fn hook(c: &mut Context<'_>, request: RootId, member: &str, value: RootId) -> Done {
    let function = get(c, request, member)?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, &[value]).map(drop)
}

// ---- The stream

pub(crate) fn state(c: &mut Context<'_>, stream: RootId) -> Result<State, RootedError> {
    Ok(State::from(num(c, stream, STATE)?))
}

fn set_state(c: &mut Context<'_>, stream: RootId, state: State) -> Done {
    put_num(c, stream, STATE, state as usize as f64)
}

/// `createReadableStreamState()` on `object`, which becomes a ReadableStream.
fn initialize_stream(c: &mut Context<'_>, object: RootId) -> Outcome {
    let closed = deferred(c)?;
    let state = record(c, &[(SELF, object), (CLOSED_PROMISE, closed)])?;
    set_state(c, state, State::Readable)?;
    put_flag(c, state, DISTURBED, false)?;
    attach(c, object, Brand::ReadableStream, state)?;
    // `stream[kIsClosedPromise]`, which the stream utilities' `finished` reads.
    let holder = literal(c, &[(PROMISE, closed)])?;
    let key = intrinsic(c, key::IS_CLOSED)?;
    c.define_data_property_rooted(object, key, holder, true, true, true)?;
    Ok(state)
}


fn set_error_function(c: &mut Context<'_>, object: RootId, function: RootId) -> Done {
    let key = intrinsic(c, key::ERROR_FUNCTION)?;
    c.define_data_property_rooted(object, key, function, true, true, true)?;
    Ok(())
}

pub(crate) fn is_locked(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    let reader = get(c, stream, READER)?;
    Ok(!is_undefined(c, reader))
}

/// The reader's internal slots when the stream has a reader of `brand`.
fn reader_of(c: &mut Context<'_>, stream: RootId, brand: Brand) -> Result<Option<RootId>, RootedError> {
    let reader = get(c, stream, READER)?;
    if is_undefined(c, reader) {
        return Ok(None);
    }
    state_of(c, reader, brand)
}

pub(crate) fn has_default_reader(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    Ok(reader_of(c, stream, Brand::DefaultReader)?.is_some())
}

pub(crate) fn has_byob_reader(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    Ok(reader_of(c, stream, Brand::ByobReader)?.is_some())
}

/// The number of requests queued on the stream's reader.
pub(crate) fn queued_requests(c: &mut Context<'_>, stream: RootId) -> Result<usize, RootedError> {
    let reader = get(c, stream, READER)?;
    if is_undefined(c, reader) {
        return Ok(0);
    }
    let reader = slots(c, reader)?;
    let requests = get(c, reader, REQUESTS)?;
    list::len(c, requests)
}

pub(crate) fn add_request(c: &mut Context<'_>, stream: RootId, request: RootId) -> Done {
    let reader = get(c, stream, READER)?;
    let reader = slots(c, reader)?;
    let requests = get(c, reader, REQUESTS)?;
    list::push(c, requests, request)
}

/// `readableStreamFulfillReadRequest` and `readableStreamFulfillReadIntoRequest`.
pub(crate) fn fulfill_request(c: &mut Context<'_>, stream: RootId, chunk: RootId, done: bool) -> Done {
    let reader = get(c, stream, READER)?;
    let reader = slots(c, reader)?;
    let requests = get(c, reader, REQUESTS)?;
    let request = list::shift(c, requests)?;
    if done {
        close_steps(c, request, chunk)
    } else {
        chunk_steps(c, request, chunk)
    }
}

/// `readableStreamCancel(stream, reason)`.
pub(crate) fn cancel(c: &mut Context<'_>, stream: RootId, reason: RootId) -> Outcome {
    put_flag(c, stream, DISTURBED, true)?;
    match state(c, stream)? {
        State::Closed => return resolved_undefined(c),
        State::Errored => {
            let error = get(c, stream, STORED_ERROR)?;
            return rejected(c, error);
        }
        State::Readable => {}
    }
    close(c, stream)?;
    if let Some(reader) = reader_of(c, stream, Brand::ByobReader)? {
        let requests = get(c, reader, REQUESTS)?;
        let undefined = c.undefined();
        for request in list::drain(c, requests)? {
            close_steps(c, request, undefined)?;
        }
    }
    let controller = get(c, stream, CONTROLLER)?;
    let result = if flag(c, controller, super::byte::field::BYTE)? {
        super::byte::cancel_steps(c, controller, reason)?
    } else {
        default_cancel_steps(c, controller, reason)?
    };
    upon(c, result, Some(Member::Ignore), None, &[])
}

/// `readableStreamClose(stream)`.
pub(crate) fn close(c: &mut Context<'_>, stream: RootId) -> Done {
    set_state(c, stream, State::Closed)?;
    let closed = get(c, stream, CLOSED_PROMISE)?;
    settle_undefined(c, closed)?;
    let reader = get(c, stream, READER)?;
    if is_undefined(c, reader) {
        return Ok(());
    }
    let reader_state = slots(c, reader)?;
    let promise = get(c, reader_state, CLOSED)?;
    settle_undefined(c, promise)?;
    if has_default_reader(c, stream)? {
        let requests = get(c, reader_state, REQUESTS)?;
        let undefined = c.undefined();
        for request in list::drain(c, requests)? {
            close_steps(c, request, undefined)?;
        }
    }
    Ok(())
}

/// `readableStreamError(stream, error)`.
pub(crate) fn error(c: &mut Context<'_>, stream: RootId, error: RootId) -> Done {
    set_state(c, stream, State::Errored)?;
    put(c, stream, STORED_ERROR, error)?;
    let closed = get(c, stream, CLOSED_PROMISE)?;
    mark_handled(c, closed)?;
    super::fail(c, closed, error)?;
    let reader = get(c, stream, READER)?;
    if is_undefined(c, reader) {
        return Ok(());
    }
    let reader_state = slots(c, reader)?;
    let promise = get(c, reader_state, CLOSED)?;
    mark_handled(c, promise)?;
    super::fail(c, promise, error)?;
    let requests = get(c, reader_state, REQUESTS)?;
    for request in list::drain(c, requests)? {
        error_steps(c, request, error)?;
    }
    Ok(())
}

// ---- Readers

/// `readableStreamReaderGenericInitialize(reader, stream)` for a new reader of `brand`,
/// after the lock check.
fn initialize_reader(c: &mut Context<'_>, object: RootId, brand: Brand, stream_object: RootId) -> Outcome {
    let stream = this_or_argument(c, stream_object)?;
    if is_locked(c, stream)? {
        return Err(throw(c, WebError::InvalidState("ReadableStream is locked")));
    }
    if brand == Brand::ByobReader {
        let controller = get(c, stream, CONTROLLER)?;
        if !flag(c, controller, super::byte::field::BYTE)? {
            return Err(throw(c, WebError::ArgValueBecause("stream", stream_object, "must be a byte stream")));
        }
    }
    let requests = list::new(c)?;
    let closed = match state(c, stream)? {
        State::Readable => deferred(c)?,
        State::Closed => resolved_undefined(c)?,
        State::Errored => {
            let error = get(c, stream, STORED_ERROR)?;
            let promise = rejected(c, error)?;
            mark_handled(c, promise)?;
            promise
        }
    };
    let state = record(c, &[(SELF, object), (STREAM, stream), (CLOSED, closed), (REQUESTS, requests)])?;
    attach(c, object, brand, state)?;
    put(c, stream, READER, object)?;
    Ok(state)
}

fn this_or_argument(c: &mut Context<'_>, stream: RootId) -> Outcome {
    match state_of(c, stream, Brand::ReadableStream)? {
        Some(state) => Ok(state),
        None => Err(throw(c, WebError::ArgType("stream", "ReadableStream", stream))),
    }
}

/// `new ReadableStreamDefaultReader(stream)` from the module.
pub(crate) fn default_reader(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let prototype_holder = super::classes(c)?;
    let class = get(c, prototype_holder, Brand::DefaultReader.name())?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let stream_object = get(c, stream, SELF)?;
    initialize_reader(c, object, Brand::DefaultReader, stream_object)?;
    Ok(object)
}

/// `new ReadableStreamBYOBReader(stream)` from the module.
pub(crate) fn byob_reader(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let prototype_holder = super::classes(c)?;
    let class = get(c, prototype_holder, Brand::ByobReader.name())?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let stream_object = get(c, stream, SELF)?;
    initialize_reader(c, object, Brand::ByobReader, stream_object)?;
    Ok(object)
}

pub(crate) fn construct_byob_reader(c: &mut Context<'_>, this: RootId, stream: RootId) -> Done {
    initialize_reader(c, this, Brand::ByobReader, stream).map(drop)
}

/// `readableStreamReaderGenericRelease(reader)`.
pub(crate) fn generic_release(c: &mut Context<'_>, reader: RootId) -> Done {
    let stream = get(c, reader, STREAM)?;
    let released = released_error(c, key::READER_RELEASED, "Reader released")?;
    if state(c, stream)? == State::Readable {
        let closed = get(c, reader, CLOSED)?;
        super::fail(c, closed, released)?;
    } else {
        let closed = rejected(c, released)?;
        put(c, reader, CLOSED, closed)?;
    }
    let closed = get(c, reader, CLOSED)?;
    mark_handled(c, closed)?;
    let controller = get(c, stream, CONTROLLER)?;
    if flag(c, controller, super::byte::field::BYTE)? {
        super::byte::release_steps(c, controller)?;
    }
    let undefined = c.undefined();
    put(c, stream, READER, undefined)?;
    put(c, reader, STREAM, undefined)
}


/// `readableStreamDefaultReaderRelease` and `readableStreamBYOBReaderRelease`: release,
/// then fail the queued requests.
pub(crate) fn reader_release(c: &mut Context<'_>, reader: RootId) -> Done {
    generic_release(c, reader)?;
    let releasing = released_error(c, key::READER_RELEASING, "Releasing reader")?;
    let requests = get(c, reader, REQUESTS)?;
    for request in list::drain(c, requests)? {
        error_steps(c, request, releasing)?;
    }
    Ok(())
}

/// `readableStreamDefaultReaderRead(reader, request)`.
pub(crate) fn default_reader_read(c: &mut Context<'_>, reader: RootId, request: RootId) -> Done {
    let stream = get(c, reader, STREAM)?;
    put_flag(c, stream, DISTURBED, true)?;
    match state(c, stream)? {
        State::Closed => {
            let undefined = c.undefined();
            close_steps(c, request, undefined)
        }
        State::Errored => {
            let error = get(c, stream, STORED_ERROR)?;
            error_steps(c, request, error)
        }
        State::Readable => {
            let controller = get(c, stream, CONTROLLER)?;
            if flag(c, controller, super::byte::field::BYTE)? {
                super::byte::pull_steps(c, controller, request)
            } else {
                default_pull_steps(c, controller, request)
            }
        }
    }
}

/// A host consumer's read on `reader` (a default reader's internal slots): `chunk`,
/// `close` and `error` are called with the outcome.
pub(crate) fn read_with(c: &mut Context<'_>, reader: RootId, chunk: RootId, close: RootId, error: RootId) -> Done {
    let request = new_request(c, ReadSteps::Hook, &[(request::CHUNK, chunk), (request::CLOSE, close), (request::ERROR, error)])?;
    default_reader_read(c, reader, request)
}

// ---- The default controller

/// `setupReadableStreamDefaultController`: `start` runs the start algorithm on the
/// controller.
fn setup_default_controller(
    c: &mut Context<'_>,
    stream: RootId,
    algorithms: &[(&str, RootId)],
    internal: bool,
    high_water_mark: f64,
    size: RootId,
    start: impl FnOnce(&mut Context<'_>, RootId) -> Outcome,
) -> Outcome {
    let queue = list::new(c)?;
    let controller = record(c, &[(STREAM, stream), (QUEUE, queue), (SIZE, size)])?;
    for &(name, value) in algorithms {
        put(c, controller, name, value)?;
    }
    put_flag(c, controller, algorithm::INTERNAL, internal)?;
    put_num(c, controller, QUEUE_SIZE, 0.0)?;
    put_num(c, controller, HIGH_WATER_MARK, high_water_mark)?;
    for name in [STARTED, CLOSE_REQUESTED, PULLING, PULL_AGAIN] {
        put_flag(c, controller, name, false)?;
    }
    let object = instance(c, Brand::DefaultController, controller)?;
    put(c, controller, SELF, object)?;
    put(c, stream, CONTROLLER, controller)?;
    let stream_object = get(c, stream, SELF)?;
    let error_function = function(c, Member::ErrorFunction, "bound error", &[controller])?;
    set_error_function(c, stream_object, error_function)?;
    let start_result = start(c, object)?;
    let start_promise = deferred(c)?;
    c.resolve_promise_rooted(start_promise, start_result)?;
    upon(c, start_promise, Some(Member::StartFulfilled), Some(Member::StartRejected), &[controller])?;
    Ok(controller)
}

/// `createReadableStream(start, pull, cancel, highWaterMark, size)` over internal
/// algorithms: `start` is the start algorithm's result, `pull` and `cancel` host functions
/// returning promises. Returns the stream's internal slots.
pub(crate) fn create_readable(
    c: &mut Context<'_>,
    start: RootId,
    pull: RootId,
    cancel: RootId,
    high_water_mark: f64,
    size: RootId,
) -> Outcome {
    let stream = create_readable_shell(c)?;
    setup_default_controller(c, stream, &[(PULL, pull), (CANCEL, cancel)], true, high_water_mark, size, |_, _| Ok(start))?;
    Ok(stream)
}

/// A new ReadableStream without a controller (`InternalReadableStream`'s first steps).
pub(crate) fn create_readable_shell(c: &mut Context<'_>) -> Outcome {
    let classes = super::classes(c)?;
    let class = get(c, classes, Brand::ReadableStream.name())?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    initialize_stream(c, object)
}

pub(crate) fn can_close_or_enqueue(c: &mut Context<'_>, controller: RootId) -> Result<bool, RootedError> {
    if flag(c, controller, CLOSE_REQUESTED)? {
        return Ok(false);
    }
    let stream = get(c, controller, STREAM)?;
    Ok(state(c, stream)? == State::Readable)
}

/// The desired size of a default or byte controller: `None` when errored.
pub(crate) fn desired_size(c: &mut Context<'_>, controller: RootId) -> Result<Option<f64>, RootedError> {
    let stream = get(c, controller, STREAM)?;
    Ok(match state(c, stream)? {
        State::Errored => None,
        State::Closed => Some(0.0),
        State::Readable => Some(num(c, controller, HIGH_WATER_MARK)? - num(c, controller, QUEUE_SIZE)?),
    })
}

pub(crate) fn desired_size_value(c: &mut Context<'_>, controller: RootId) -> Outcome {
    Ok(match desired_size(c, controller)? {
        Some(size) => c.number(size),
        None => c.null(),
    })
}

fn clear_algorithms(c: &mut Context<'_>, controller: RootId) -> Done {
    let undefined = c.undefined();
    for name in [PULL, CANCEL, SIZE] {
        put(c, controller, name, undefined)?;
    }
    Ok(())
}

/// `resetQueue(controller)`.
pub(crate) fn reset_queue(c: &mut Context<'_>, controller: RootId) -> Done {
    let queue = list::new(c)?;
    put(c, controller, QUEUE, queue)?;
    put_num(c, controller, QUEUE_SIZE, 0.0)
}

/// `enqueueValueWithSize(controller, value, size)`.
pub(crate) fn enqueue_with_size(c: &mut Context<'_>, controller: RootId, value: RootId, size: RootId) -> Done {
    let number = c.to_number(size)?;
    if number.is_nan() || number < 0.0 || number == f64::INFINITY {
        let size = c.number(number);
        return Err(throw(c, WebError::ArgValueRange("size", size)));
    }
    let size = c.number(number);
    let entry = record(c, &[(VALUE, value), (SIZE, size)])?;
    let queue = get(c, controller, QUEUE)?;
    list::push(c, queue, entry)?;
    let total = num(c, controller, QUEUE_SIZE)?;
    put_num(c, controller, QUEUE_SIZE, total + number)
}

/// `dequeueValue(controller)`.
pub(crate) fn dequeue(c: &mut Context<'_>, controller: RootId) -> Outcome {
    let queue = get(c, controller, QUEUE)?;
    let entry = list::shift(c, queue)?;
    let size = num(c, entry, SIZE)?;
    let total = num(c, controller, QUEUE_SIZE)?;
    put_num(c, controller, QUEUE_SIZE, (total - size).max(0.0))?;
    get(c, entry, VALUE)
}

pub(crate) fn peek(c: &mut Context<'_>, controller: RootId) -> Outcome {
    let queue = get(c, controller, QUEUE)?;
    let entry = list::peek(c, queue)?;
    get(c, entry, VALUE)
}

pub(crate) fn queue_length(c: &mut Context<'_>, controller: RootId) -> Result<usize, RootedError> {
    let queue = get(c, controller, QUEUE)?;
    list::len(c, queue)
}

/// `readableStreamDefaultControllerClose(controller)`.
pub(crate) fn default_controller_close(c: &mut Context<'_>, controller: RootId) -> Done {
    if !can_close_or_enqueue(c, controller)? {
        return Ok(());
    }
    put_flag(c, controller, CLOSE_REQUESTED, true)?;
    if queue_length(c, controller)? == 0 {
        clear_algorithms(c, controller)?;
        let stream = get(c, controller, STREAM)?;
        close(c, stream)?;
    }
    Ok(())
}

/// `readableStreamDefaultControllerEnqueue(controller, chunk)`.
pub(crate) fn default_controller_enqueue(c: &mut Context<'_>, controller: RootId, chunk: RootId) -> Done {
    if !can_close_or_enqueue(c, controller)? {
        return Ok(());
    }
    let stream = get(c, controller, STREAM)?;
    if is_locked(c, stream)? && queued_requests(c, stream)? > 0 {
        fulfill_request(c, stream, chunk, false)?;
    } else {
        let size = get(c, controller, SIZE)?;
        let sized = if is_undefined(c, size) {
            let one = c.number(1.0);
            Ok(one)
        } else {
            let undefined = c.undefined();
            c.call_rooted(size, undefined, &[chunk])
        };
        let enqueued = sized.and_then(|size| enqueue_with_size(c, controller, chunk, size));
        if let Err(failure) = enqueued {
            if let Some(exception) = failure.exception {
                default_controller_error(c, controller, exception)?;
            }
            return Err(failure);
        }
    }
    call_pull_if_needed(c, controller)
}

/// `readableStreamDefaultControllerError(controller, error)`.
pub(crate) fn default_controller_error(c: &mut Context<'_>, controller: RootId, failure: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    if state(c, stream)? == State::Readable {
        reset_queue(c, controller)?;
        clear_algorithms(c, controller)?;
        error(c, stream, failure)?;
    }
    Ok(())
}

pub(crate) fn has_backpressure(c: &mut Context<'_>, controller: RootId) -> Result<bool, RootedError> {
    Ok(!should_call_pull(c, controller)?)
}

fn should_call_pull(c: &mut Context<'_>, controller: RootId) -> Result<bool, RootedError> {
    if !can_close_or_enqueue(c, controller)? || !flag(c, controller, STARTED)? {
        return Ok(false);
    }
    let stream = get(c, controller, STREAM)?;
    if is_locked(c, stream)? && queued_requests(c, stream)? > 0 {
        return Ok(true);
    }
    Ok(desired_size(c, controller)?.is_some_and(|size| size > 0.0))
}

fn call_pull_if_needed(c: &mut Context<'_>, controller: RootId) -> Done {
    if !should_call_pull(c, controller)? {
        return Ok(());
    }
    if flag(c, controller, PULLING)? {
        return put_flag(c, controller, PULL_AGAIN, true);
    }
    put_flag(c, controller, PULLING, true)?;
    let object = get(c, controller, SELF)?;
    let pulled = run_algorithm(c, controller, PULL, &[object])?;
    upon(c, pulled, Some(Member::PullFulfilled), Some(Member::PullRejected), &[controller]).map(drop)
}

fn default_cancel_steps(c: &mut Context<'_>, controller: RootId, reason: RootId) -> Outcome {
    reset_queue(c, controller)?;
    let result = run_algorithm(c, controller, CANCEL, &[reason])?;
    clear_algorithms(c, controller)?;
    Ok(result)
}

fn default_pull_steps(c: &mut Context<'_>, controller: RootId, request: RootId) -> Done {
    let stream = get(c, controller, STREAM)?;
    if queue_length(c, controller)? > 0 {
        let chunk = dequeue(c, controller)?;
        if flag(c, controller, CLOSE_REQUESTED)? && queue_length(c, controller)? == 0 {
            clear_algorithms(c, controller)?;
            close(c, stream)?;
        } else {
            call_pull_if_needed(c, controller)?;
        }
        return chunk_steps(c, request, chunk);
    }
    add_request(c, stream, request)?;
    call_pull_if_needed(c, controller)
}

// ---- Construction

fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let source = dictionary(c, args, 0, "source", Dictionary::Objects)?;
    let strategy = dictionary(c, args, 1, "strategy", Dictionary::ObjectsAndNull)?;
    let stream = initialize_stream(c, this)?;
    let nop = function(c, Member::Nop, "", &[])?;
    set_error_function(c, this, nop)?;
    let size = optional(c, strategy, SIZE)?;
    let high_water_mark = optional(c, strategy, HIGH_WATER_MARK)?;
    let kind = optional(c, source, "type")?;
    let kind_text = c.to_string(kind)?;
    if kind_text == "bytes" {
        if !is_undefined(c, size) {
            return Err(throw(c, WebError::ArgValueRange("strategy.size", size)));
        }
        let high_water_mark = super::high_water_mark(c, high_water_mark, 0.0)?;
        super::byte::setup_from_source(c, stream, source, high_water_mark)?;
    } else {
        if !is_undefined(c, kind) {
            return Err(throw(c, WebError::ArgValue("source.type", kind)));
        }
        let high_water_mark = super::high_water_mark(c, high_water_mark, 1.0)?;
        let size = size_algorithm(c, size)?;
        let start = optional(c, source, "start")?;
        let pull = if is_nullish(c, source) { c.undefined() } else { callback(c, source, PULL, "source.pull")? };
        let cancel = if is_nullish(c, source) { c.undefined() } else { callback(c, source, CANCEL, "source.cancel")? };
        let algorithms = [(PULL, pull), (CANCEL, cancel), (algorithm::SOURCE, source)];
        setup_default_controller(c, stream, &algorithms, false, high_water_mark, size, |c, controller| {
            if c.truthy_rooted(start)? {
                c.call_rooted(start, source, &[controller])
            } else {
                Ok(c.undefined())
            }
        })?;
    }
    Ok(c.undefined())
}

fn stream_of(c: &mut Context<'_>, this: RootId) -> Outcome {
    this_state(c, this, Brand::ReadableStream)
}

fn get_reader(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let stream = stream_of(c, this)?;
    let options = dictionary(c, args, 0, "options", Dictionary::ObjectsAndNull)?;
    let mode = optional(c, options, "mode")?;
    if is_undefined(c, mode) {
        return default_reader(c, stream);
    }
    if c.to_string(mode)? != "byob" {
        return Err(throw(c, WebError::ArgValue("options.mode", mode)));
    }
    byob_reader(c, stream)
}

fn inspect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let stream = stream_of(c, this)?;
    let locked = is_locked(c, stream)?;
    let locked = c.boolean(locked);
    let state = state(c, stream)?.name();
    let state = c.string_rooted(state);
    let controller = get(c, stream, CONTROLLER)?;
    let byob = !is_undefined(c, controller) && flag(c, controller, super::byte::field::BYTE)?;
    let byob = c.boolean(byob);
    let data = literal(c, &[("locked", locked), ("state", state), ("supportsBYOB", byob)])?;
    custom_inspect(c, this, Brand::ReadableStream.name(), data, args)
}

// ---- Async iteration

/// An async iterator's state: Node's `{ done, current }` and whether a step ran yet.
mod iteration {
    pub(super) const DONE: &str = "done";
    pub(super) const CURRENT: &str = "current";
    pub(super) const STARTED: &str = "started";
    pub(super) const READER: &str = "reader";
    pub(super) const PREVENT_CANCEL: &str = "preventCancel";
}

fn values(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let stream = stream_of(c, this)?;
    let options = dictionary(c, args, 0, "options", Dictionary::ObjectsAndNull)?;
    let prevent_cancel = optional(c, options, "preventCancel")?;
    let prevent_cancel = c.truthy_rooted(prevent_cancel)?;
    let reader = default_reader(c, stream)?;
    let reader = slots(c, reader)?;
    let state = record(c, &[(iteration::READER, reader)])?;
    put_flag(c, state, iteration::DONE, false)?;
    put_flag(c, state, iteration::STARTED, false)?;
    put_flag(c, state, iteration::PREVENT_CANCEL, prevent_cancel)?;
    let iterator = c.object_rooted()?;
    for (name, member, length) in [("next", Member::IteratorNext, 0), ("return", Member::IteratorReturn, 1)] {
        let method = function(c, member, name, &[state])?;
        super::set_length(c, method, length)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(iterator, key, method, true, true, true)?;
    }
    let key = intrinsic(c, key::ASYNC_ITERATOR_KEY)?;
    let method = function(c, Member::IteratorSelf, "[Symbol.asyncIterator]", &[])?;
    c.define_data_property_rooted(iterator, key, method, true, true, true)?;
    let base = intrinsic(c, key::ASYNC_ITERATOR)?;
    c.set_prototype_rooted(iterator, base)?;
    Ok(iterator)
}

/// The iterator's queued steps: `current = current ? then(current, steps, steps) : steps()`.
fn chain_step(c: &mut Context<'_>, state: RootId, step: Member, value: RootId) -> Outcome {
    let current = get(c, state, iteration::CURRENT)?;
    let next = if is_undefined(c, current) {
        run_iteration_step(c, state, step, value)?
    } else {
        upon(c, current, Some(step), Some(step), &[state, value])?
    };
    put(c, state, iteration::CURRENT, next)?;
    Ok(next)
}

fn run_iteration_step(c: &mut Context<'_>, state: RootId, step: Member, value: RootId) -> Outcome {
    match step {
        Member::IteratorNextSteps => next_steps(c, state),
        _ => return_steps(c, state, value),
    }
}

fn next_steps(c: &mut Context<'_>, state: RootId) -> Outcome {
    if flag(c, state, iteration::DONE)? {
        let undefined = c.undefined();
        let result = done_result(c, undefined)?;
        return resolved(c, result);
    }
    let reader = get(c, state, iteration::READER)?;
    let stream = get(c, reader, STREAM)?;
    if is_undefined(c, stream) {
        return rejected_with(c, WebError::InvalidState("The reader is not bound to a ReadableStream"));
    }
    let promise = deferred(c)?;
    let request = new_request(c, ReadSteps::Iterator, &[(PROMISE, promise), (request::READER, reader), (request::ITERATION, state)])?;
    default_reader_read(c, reader, request)?;
    Ok(promise)
}

/// The iterator's `returnSteps(value)`, an async function.
fn return_steps(c: &mut Context<'_>, state: RootId, value: RootId) -> Outcome {
    let promise = deferred(c)?;
    let finished = done_result(c, value)?;
    if flag(c, state, iteration::DONE)? {
        c.resolve_promise_rooted(promise, finished)?;
        return Ok(promise);
    }
    put_flag(c, state, iteration::DONE, true)?;
    let reader = get(c, state, iteration::READER)?;
    let stream = get(c, reader, STREAM)?;
    if is_undefined(c, stream) {
        let error = super::create_error(c, WebError::InvalidState("The reader is not bound to a ReadableStream"))?;
        c.reject_promise_rooted(promise, error)?;
        return Ok(promise);
    }
    if !flag(c, state, iteration::PREVENT_CANCEL)? {
        let result = cancel(c, stream, value)?;
        generic_release(c, reader)?;
        // `await result; return { done: true, value }`.
        let awaited = resolved(c, result)?;
        upon(c, awaited, Some(Member::IteratorReturnFulfilled), Some(Member::IteratorReturnRejected), &[promise, finished])?;
        return Ok(promise);
    }
    generic_release(c, reader)?;
    c.resolve_promise_rooted(promise, finished)?;
    Ok(promise)
}

/// An iterator read's close or error: the iteration is done and the reader released.
fn finish_iteration(c: &mut Context<'_>, request: RootId) -> Done {
    let state = get(c, request, request::ITERATION)?;
    let undefined = c.undefined();
    put(c, state, iteration::CURRENT, undefined)?;
    put_flag(c, state, iteration::DONE, true)?;
    let reader = get(c, request, request::READER)?;
    generic_release(c, reader)
}

// ---- Default tee

/// A default tee's shared state (`readableStreamDefaultTee`'s variables).
pub(crate) mod tee {
    pub(crate) const STREAM: &str = "stream";
    pub(crate) const READER: &str = "reader";
    pub(crate) const READING: &str = "reading";
    pub(crate) const CANCELED1: &str = "canceled1";
    pub(crate) const CANCELED2: &str = "canceled2";
    pub(crate) const REASON1: &str = "reason1";
    pub(crate) const REASON2: &str = "reason2";
    pub(crate) const BRANCH1: &str = "branch1";
    pub(crate) const BRANCH2: &str = "branch2";
    pub(crate) const CANCEL_PROMISE: &str = "cancelPromise";
    /// Whether `cancelPromise` was resolved (a deferred's resolve runs once).
    pub(crate) const CANCEL_RESOLVED: &str = "cancelResolved";
    pub(crate) const READ_AGAIN1: &str = "readAgainForBranch1";
    pub(crate) const READ_AGAIN2: &str = "readAgainForBranch2";
}

/// `cancelPromise.resolve(value)`, once.
pub(crate) fn resolve_cancel(c: &mut Context<'_>, tee: RootId, value: RootId) -> Done {
    if flag(c, tee, tee::CANCEL_RESOLVED)? {
        return Ok(());
    }
    put_flag(c, tee, tee::CANCEL_RESOLVED, true)?;
    let promise = get(c, tee, tee::CANCEL_PROMISE)?;
    c.resolve_promise_rooted(promise, value)
}

/// The branch controllers of a tee.
pub(crate) fn branch_controller(c: &mut Context<'_>, tee: RootId, branch: &str) -> Outcome {
    let stream = get(c, tee, branch)?;
    get(c, stream, CONTROLLER)
}

/// `readableStreamTee(stream, false)`: the two branches, as an array.
pub(crate) fn tee_stream(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let controller = get(c, stream, CONTROLLER)?;
    if flag(c, controller, super::byte::field::BYTE)? {
        super::byte::tee(c, stream)
    } else {
        default_tee(c, stream)
    }
}

/// Whether the stream was read from or canceled.
pub(crate) fn is_disturbed(c: &mut Context<'_>, stream: RootId) -> Result<bool, RootedError> {
    flag(c, stream, DISTURBED)
}

fn default_tee(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let reader = default_reader(c, stream)?;
    let reader = slots(c, reader)?;
    let cancel_promise = deferred(c)?;
    let tee = record(c, &[(tee::STREAM, stream), (tee::READER, reader), (tee::CANCEL_PROMISE, cancel_promise)])?;
    for name in [tee::READING, tee::CANCELED1, tee::CANCELED2, tee::CANCEL_RESOLVED] {
        put_flag(c, tee, name, false)?;
    }
    let undefined = c.undefined();
    let mut branches = [undefined; 2];
    for (index, (name, cancel)) in [(tee::BRANCH1, Member::TeeCancel1), (tee::BRANCH2, Member::TeeCancel2)].into_iter().enumerate() {
        let pull = function(c, Member::TeePull, "", &[tee])?;
        let cancel = function(c, cancel, "", &[tee])?;
        let branch = create_readable(c, undefined, pull, cancel, 1.0, undefined)?;
        put(c, tee, name, branch)?;
        branches[index] = get(c, branch, SELF)?;
    }
    let closed = get(c, reader, CLOSED)?;
    upon(c, closed, None, Some(Member::TeeReaderError), &[tee])?;
    c.array_rooted(&branches)
}

/// `pullAlgorithm`, an async function.
fn tee_pull(c: &mut Context<'_>, tee: RootId) -> Outcome {
    if !flag(c, tee, tee::READING)? {
        put_flag(c, tee, tee::READING, true)?;
        let reader = get(c, tee, tee::READER)?;
        let request = new_request(c, ReadSteps::Tee, &[(request::TEE, tee)])?;
        default_reader_read(c, reader, request)?;
    }
    resolved_undefined(c)
}

fn tee_cancel(c: &mut Context<'_>, tee: RootId, reason: RootId, first: bool) -> Outcome {
    let (canceled, reason_field, other) =
        if first { (tee::CANCELED1, tee::REASON1, tee::CANCELED2) } else { (tee::CANCELED2, tee::REASON2, tee::CANCELED1) };
    put_flag(c, tee, canceled, true)?;
    put(c, tee, reason_field, reason)?;
    if flag(c, tee, other)? {
        let reason1 = get(c, tee, tee::REASON1)?;
        let reason2 = get(c, tee, tee::REASON2)?;
        let composite = c.array_rooted(&[reason1, reason2])?;
        let stream = get(c, tee, tee::STREAM)?;
        let result = cancel(c, stream, composite)?;
        resolve_cancel(c, tee, result)?;
    }
    get(c, tee, tee::CANCEL_PROMISE)
}

fn tee_chunk(c: &mut Context<'_>, tee: RootId, chunk: RootId) -> Done {
    put_flag(c, tee, tee::READING, false)?;
    for (canceled, branch) in [(tee::CANCELED1, tee::BRANCH1), (tee::CANCELED2, tee::BRANCH2)] {
        if !flag(c, tee, canceled)? {
            let controller = branch_controller(c, tee, branch)?;
            default_controller_enqueue(c, controller, chunk)?;
        }
    }
    Ok(())
}

fn tee_close(c: &mut Context<'_>, tee: RootId) -> Done {
    put_flag(c, tee, tee::READING, false)?;
    let canceled1 = flag(c, tee, tee::CANCELED1)?;
    let canceled2 = flag(c, tee, tee::CANCELED2)?;
    for (canceled, branch) in [(canceled1, tee::BRANCH1), (canceled2, tee::BRANCH2)] {
        if !canceled {
            let controller = branch_controller(c, tee, branch)?;
            default_controller_close(c, controller)?;
        }
    }
    if !canceled1 || !canceled2 {
        let undefined = c.undefined();
        resolve_cancel(c, tee, undefined)?;
    }
    Ok(())
}

fn tee_reader_error(c: &mut Context<'_>, tee: RootId, failure: RootId) -> Done {
    for branch in [tee::BRANCH1, tee::BRANCH2] {
        let controller = branch_controller(c, tee, branch)?;
        default_controller_error(c, controller, failure)?;
    }
    if !flag(c, tee, tee::CANCELED1)? || !flag(c, tee, tee::CANCELED2)? {
        let undefined = c.undefined();
        resolve_cancel(c, tee, undefined)?;
    }
    Ok(())
}

// ---- ReadableStream.from

/// An iterator record: `{ iterator, next }`, and whether it wraps a sync iterator.
mod iterator_record {
    pub(super) const ITERATOR: &str = "iterator";
    pub(super) const NEXT: &str = "next";
    pub(super) const SYNC: &str = "sync";
}

fn from(c: &mut Context<'_>, iterable: RootId) -> Outcome {
    let record = get_async_iterator(c, iterable)?;
    let stream_cell = super::record(c, &[])?;
    let pull = function(c, Member::FromPull, "", &[record, stream_cell])?;
    let cancel = function(c, Member::FromCancel, "", &[record])?;
    let undefined = c.undefined();
    let stream = create_readable(c, undefined, pull, cancel, 0.0, undefined)?;
    put(c, stream_cell, STREAM, stream)?;
    get(c, stream, SELF)
}

/// `getIterator(obj, 'async')`.
fn get_async_iterator(c: &mut Context<'_>, iterable: RootId) -> Outcome {
    let not_iterable = |c: &mut Context<'_>| throw(c, WebError::NotIterable(iterable));
    if is_nullish(c, iterable) {
        return Err(not_iterable(c));
    }
    let async_key = intrinsic(c, key::ASYNC_ITERATOR_KEY)?;
    let method = c.get_property_rooted(iterable, async_key)?;
    let (method, sync) = if is_nullish(c, method) {
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let sync_key = get(c, symbol, "iterator")?;
        let method = c.get_property_rooted(iterable, sync_key)?;
        if is_undefined(c, method) {
            return Err(not_iterable(c));
        }
        (method, true)
    } else {
        (method, false)
    };
    let iterator = c.call_rooted(method, iterable, &[])?;
    if !c.is_object_rooted(iterator)? || is_nullish(c, iterator) {
        return Err(throw(c, WebError::InvalidState("The iterator method must return an object")));
    }
    let next = get(c, iterator, iterator_record::NEXT)?;
    let sync = c.boolean(sync);
    record(c, &[(iterator_record::ITERATOR, iterator), (iterator_record::NEXT, next), (iterator_record::SYNC, sync)])
}

/// `iteratorNext(record)`: the next result, or for a sync iterator the promise of it with
/// its value awaited (`CreateAsyncFromSyncIterator`).
fn iterator_next(c: &mut Context<'_>, iterator: RootId) -> Outcome {
    let target = get(c, iterator, iterator_record::ITERATOR)?;
    let next = get(c, iterator, iterator_record::NEXT)?;
    let result = c.call_rooted(next, target, &[])?;
    if !c.is_object_rooted(result)? || is_nullish(c, result) {
        return Err(throw(c, WebError::InvalidState("The iterator.next() method must return an object")));
    }
    if !flag(c, iterator, iterator_record::SYNC)? {
        return Ok(result);
    }
    let done = get(c, result, "done")?;
    let value = get(c, result, VALUE)?;
    let value = resolved(c, value)?;
    upon(c, value, Some(Member::FromSyncNext), None, &[done])
}

fn from_pull(c: &mut Context<'_>, iterator: RootId, stream_cell: RootId) -> Outcome {
    let promise = deferred(c)?;
    match super::attempt(iterator_next(c, iterator))? {
        Ok(result) => {
            let result = resolved(c, result)?;
            let pulled = upon(c, result, Some(Member::FromPulled), None, &[stream_cell])?;
            c.resolve_promise_rooted(promise, pulled)?;
        }
        Err(error) => c.reject_promise_rooted(promise, error)?,
    }
    Ok(promise)
}

fn from_pulled(c: &mut Context<'_>, stream_cell: RootId, result: RootId) -> Outcome {
    if !c.is_object_rooted(result)? || is_nullish(c, result) {
        return Err(throw(c, WebError::InvalidState("The promise returned by the iterator.next() method must fulfill with an object")));
    }
    let stream = get(c, stream_cell, STREAM)?;
    let controller = get(c, stream, CONTROLLER)?;
    let done = get(c, result, "done")?;
    if c.truthy_rooted(done)? {
        default_controller_close(c, controller)?;
    } else {
        let value = get(c, result, VALUE)?;
        default_controller_enqueue(c, controller, value)?;
    }
    Ok(c.undefined())
}

fn from_cancel(c: &mut Context<'_>, iterator: RootId, reason: RootId) -> Outcome {
    let promise = deferred(c)?;
    let target = get(c, iterator, iterator_record::ITERATOR)?;
    let outcome = super::attempt((|| {
        let method = get(c, target, "return")?;
        if is_undefined(c, method) {
            return resolved_undefined(c);
        }
        let result = c.call_rooted(method, target, &[reason])?;
        let result = resolved(c, result)?;
        upon(c, result, Some(Member::FromCanceled), None, &[])
    })())?;
    match outcome {
        Ok(value) => c.resolve_promise_rooted(promise, value)?,
        Err(error) => c.reject_promise_rooted(promise, error)?,
    }
    Ok(promise)
}

// ---- Classes

const STREAM_MEMBERS: [ClassMember<Member>; 7] = [
    ClassMember::Getter("locked", Member::Locked),
    ClassMember::Method("cancel", Member::Cancel, 0),
    ClassMember::Method("getReader", Member::GetReader, 0),
    ClassMember::Method("pipeThrough", Member::PipeThrough, 1),
    ClassMember::Method("pipeTo", Member::PipeTo, 1),
    ClassMember::Method("tee", Member::Tee, 0),
    ClassMember::Method("values", Member::Values, 0),
];

const READER_MEMBERS: [ClassMember<Member>; 4] = [
    ClassMember::Getter("closed", Member::ReaderClosed),
    ClassMember::Method("read", Member::ReaderRead, 0),
    ClassMember::Method("releaseLock", Member::ReaderReleaseLock, 0),
    ClassMember::Method("cancel", Member::ReaderCancel, 0),
];

const CONTROLLER_MEMBERS: [ClassMember<Member>; 4] = [
    ClassMember::Getter("desiredSize", Member::ControllerDesiredSize),
    ClassMember::Method("close", Member::ControllerClose, 0),
    ClassMember::Method("enqueue", Member::ControllerEnqueue, 0),
    ClassMember::Method("error", Member::ControllerError, 0),
];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Done {
    let stream = class(c, Brand::ReadableStream, Member::Construct, 0, Member::Inspect)?;
    let prototype = get(c, stream, "prototype")?;
    install(c, prototype, &STREAM_MEMBERS)?;
    let values = get(c, prototype, "values")?;
    let key = intrinsic(c, key::ASYNC_ITERATOR_KEY)?;
    c.define_data_property_rooted(prototype, key, values, true, false, true)?;
    for (symbol, member, name) in [
        (key::IS_DISTURBED, Member::IsDisturbed, "get [nodejs.stream.disturbed]"),
        (key::IS_ERRORED, Member::IsErrored, "get [nodejs.stream.errored]"),
        (key::IS_READABLE, Member::IsReadable, "get [nodejs.stream.readable]"),
    ] {
        install_symbol_getter(c, prototype, symbol, member, name)?;
    }
    let from = super::method_function(c, Member::From, "from", 1)?;
    let key = c.string_rooted("from");
    c.define_data_property_rooted(stream, key, from, true, true, true)?;
    put(c, table, Brand::ReadableStream.name(), stream)?;

    let reader = class(c, Brand::DefaultReader, Member::ReaderConstruct, 1, Member::ReaderInspect)?;
    let prototype = get(c, reader, "prototype")?;
    install(c, prototype, &READER_MEMBERS)?;
    put(c, table, Brand::DefaultReader.name(), reader)?;

    let controller = class(c, Brand::DefaultController, Member::ControllerConstruct, 0, Member::ControllerInspect)?;
    let prototype = get(c, controller, "prototype")?;
    install(c, prototype, &CONTROLLER_MEMBERS)?;
    put(c, table, Brand::DefaultController.name(), controller)?;
    Ok(())
}

/// The shared `closed`, `cancel` and `releaseLock` members of both readers.
pub(crate) fn reader_closed(c: &mut Context<'_>, this: RootId, brand: Brand) -> Outcome {
    match state_of(c, this, brand)? {
        Some(reader) => get(c, reader, CLOSED),
        None => rejected_with(c, WebError::InvalidThis(brand.name())),
    }
}

pub(crate) fn reader_cancel(c: &mut Context<'_>, this: RootId, brand: Brand, reason: RootId) -> Outcome {
    let Some(reader) = state_of(c, this, brand)? else {
        return rejected_with(c, WebError::InvalidThis(brand.name()));
    };
    let stream = get(c, reader, STREAM)?;
    if is_undefined(c, stream) {
        return rejected_with(c, WebError::InvalidState("The reader is not attached to a stream"));
    }
    cancel(c, stream, reason)
}

pub(crate) fn reader_release_lock(c: &mut Context<'_>, this: RootId, brand: Brand) -> Outcome {
    let reader = this_state(c, this, brand)?;
    let stream = get(c, reader, STREAM)?;
    if !is_undefined(c, stream) {
        reader_release(c, reader)?;
    }
    Ok(c.undefined())
}

pub(crate) fn reader_inspect(c: &mut Context<'_>, this: RootId, brand: Brand, requests_name: &str, args: &[RootId]) -> Outcome {
    let reader = this_state(c, this, brand)?;
    let stream = get(c, reader, STREAM)?;
    let stream = if is_undefined(c, stream) { stream } else { get(c, stream, SELF)? };
    let requests = get(c, reader, REQUESTS)?;
    let count = list::len(c, requests)?;
    let count = c.number(count as f64);
    let closed = get(c, reader, CLOSED)?;
    let data = literal(c, &[("stream", stream), (requests_name, count), ("close", closed)])?;
    custom_inspect(c, this, brand.name(), data, args)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            super::require_new(c, Brand::ReadableStream.name())?;
            construct(c, this, args)
        }
        Member::Locked => {
            let stream = stream_of(c, this)?;
            let locked = is_locked(c, stream)?;
            Ok(c.boolean(locked))
        }
        Member::Cancel => {
            let Some(stream) = state_of(c, this, Brand::ReadableStream)? else {
                return rejected_with(c, WebError::InvalidThis(Brand::ReadableStream.name()));
            };
            if is_locked(c, stream)? {
                return rejected_with(c, WebError::InvalidState("ReadableStream is locked"));
            }
            let reason = arg(c, args, 0);
            cancel(c, stream, reason)
        }
        Member::GetReader => get_reader(c, this, args),
        Member::PipeThrough => super::pipe::pipe_through(c, this, args),
        Member::PipeTo => super::pipe::pipe_to(c, this, args),
        Member::Tee => {
            let stream = stream_of(c, this)?;
            tee_stream(c, stream)
        }
        Member::Values => values(c, this, args),
        Member::From => {
            let iterable = arg(c, args, 0);
            from(c, iterable)
        }
        Member::Inspect => inspect(c, this, args),
        Member::IsDisturbed | Member::IsErrored | Member::IsReadable => {
            let stream = slots(c, this)?;
            let value = match member {
                Member::IsDisturbed => flag(c, stream, DISTURBED)?,
                Member::IsErrored => state(c, stream)? == State::Errored,
                _ => state(c, stream)? == State::Readable,
            };
            Ok(c.boolean(value))
        }
        Member::ReaderConstruct => {
            super::require_new(c, Brand::DefaultReader.name())?;
            let stream = arg(c, args, 0);
            initialize_reader(c, this, Brand::DefaultReader, stream)?;
            Ok(c.undefined())
        }
        Member::ReaderRead => {
            let Some(reader) = state_of(c, this, Brand::DefaultReader)? else {
                return rejected_with(c, WebError::InvalidThis(Brand::DefaultReader.name()));
            };
            let stream = get(c, reader, STREAM)?;
            if is_undefined(c, stream) {
                return rejected_with(c, WebError::InvalidState("The reader is not attached to a stream"));
            }
            let promise = deferred(c)?;
            let request = new_request(c, ReadSteps::Default, &[(PROMISE, promise)])?;
            default_reader_read(c, reader, request)?;
            Ok(promise)
        }
        Member::ReaderReleaseLock => reader_release_lock(c, this, Brand::DefaultReader),
        Member::ReaderClosed => reader_closed(c, this, Brand::DefaultReader),
        Member::ReaderCancel => {
            let reason = arg(c, args, 0);
            reader_cancel(c, this, Brand::DefaultReader, reason)
        }
        Member::ReaderInspect => reader_inspect(c, this, Brand::DefaultReader, "readRequests", args),
        Member::ControllerConstruct => {
            super::require_new(c, Brand::DefaultController.name())?;
            Err(throw(c, WebError::IllegalConstructor))
        }
        Member::ControllerDesiredSize => {
            let controller = this_state(c, this, Brand::DefaultController)?;
            desired_size_value(c, controller)
        }
        Member::ControllerClose => {
            let controller = this_state(c, this, Brand::DefaultController)?;
            if !can_close_or_enqueue(c, controller)? {
                return Err(throw(c, WebError::InvalidState("Controller is already closed")));
            }
            default_controller_close(c, controller)?;
            Ok(c.undefined())
        }
        Member::ControllerEnqueue => {
            let controller = this_state(c, this, Brand::DefaultController)?;
            if !can_close_or_enqueue(c, controller)? {
                return Err(throw(c, WebError::InvalidState("Controller is already closed")));
            }
            let chunk = arg(c, args, 0);
            default_controller_enqueue(c, controller, chunk)?;
            Ok(c.undefined())
        }
        Member::ControllerError => {
            let controller = this_state(c, this, Brand::DefaultController)?;
            let failure = arg(c, args, 0);
            default_controller_error(c, controller, failure)?;
            Ok(c.undefined())
        }
        Member::ControllerInspect => {
            this_state(c, this, Brand::DefaultController)?;
            let data = c.object_rooted()?;
            custom_inspect(c, this, Brand::DefaultController.name(), data, args)
        }
        Member::ErrorFunction => {
            let [controller] = captured(c)?;
            let failure = arg(c, args, 0);
            default_controller_error(c, controller, failure)?;
            Ok(c.undefined())
        }
        Member::Nop | Member::Ignore => Ok(c.undefined()),
        Member::StartFulfilled => {
            let [controller] = captured(c)?;
            put_flag(c, controller, STARTED, true)?;
            call_pull_if_needed(c, controller)?;
            Ok(c.undefined())
        }
        Member::StartRejected | Member::PullRejected => {
            let [controller] = captured(c)?;
            let failure = arg(c, args, 0);
            default_controller_error(c, controller, failure)?;
            Ok(c.undefined())
        }
        Member::PullFulfilled => {
            let [controller] = captured(c)?;
            put_flag(c, controller, PULLING, false)?;
            if flag(c, controller, PULL_AGAIN)? {
                put_flag(c, controller, PULL_AGAIN, false)?;
                call_pull_if_needed(c, controller)?;
            }
            Ok(c.undefined())
        }
        Member::IteratorNext => {
            let [state] = captured(c)?;
            if !flag(c, state, iteration::STARTED)? {
                let start = resolved_undefined(c)?;
                put(c, state, iteration::CURRENT, start)?;
                put_flag(c, state, iteration::STARTED, true)?;
            }
            let undefined = c.undefined();
            chain_step(c, state, Member::IteratorNextSteps, undefined)
        }
        Member::IteratorReturn => {
            let [state] = captured(c)?;
            put_flag(c, state, iteration::STARTED, true)?;
            let value = arg(c, args, 0);
            chain_step(c, state, Member::IteratorReturnSteps, value)
        }
        Member::IteratorSelf => Ok(this),
        Member::IteratorNextSteps => {
            let [state] = captured(c)?;
            next_steps(c, state)
        }
        Member::IteratorReturnSteps => {
            let [state, value] = captured(c)?;
            return_steps(c, state, value)
        }
        Member::IteratorReturnFulfilled => {
            let [promise, finished] = captured(c)?;
            settle(c, promise, finished)?;
            Ok(c.undefined())
        }
        Member::IteratorReturnRejected => {
            let [promise] = captured(c)?;
            let failure = arg(c, args, 0);
            super::fail(c, promise, failure)?;
            Ok(c.undefined())
        }
        Member::TeePull => {
            let [tee] = captured(c)?;
            tee_pull(c, tee)
        }
        Member::TeeCancel1 | Member::TeeCancel2 => {
            let [tee] = captured(c)?;
            let reason = arg(c, args, 0);
            tee_cancel(c, tee, reason, member == Member::TeeCancel1)
        }
        Member::TeeChunk => {
            let [tee, chunk] = captured(c)?;
            tee_chunk(c, tee, chunk)?;
            Ok(c.undefined())
        }
        Member::TeeClose => {
            let [tee] = captured(c)?;
            tee_close(c, tee)?;
            Ok(c.undefined())
        }
        Member::TeeReaderError => {
            let [tee] = captured(c)?;
            let failure = arg(c, args, 0);
            tee_reader_error(c, tee, failure)?;
            Ok(c.undefined())
        }
        Member::FromPull => {
            let [iterator, stream_cell] = captured(c)?;
            from_pull(c, iterator, stream_cell)
        }
        Member::FromPulled => {
            let [stream_cell] = captured(c)?;
            let result = arg(c, args, 0);
            from_pulled(c, stream_cell, result)
        }
        Member::FromCancel => {
            let [iterator] = captured(c)?;
            let reason = arg(c, args, 0);
            from_cancel(c, iterator, reason)
        }
        Member::FromCanceled => {
            let result = arg(c, args, 0);
            if !c.is_object_rooted(result)? || is_nullish(c, result) {
                return Err(throw(c, WebError::InvalidState("The promise returned by the iterator.return() method must fulfill with an object")));
            }
            Ok(c.undefined())
        }
        Member::FromSyncNext => {
            let [done] = captured(c)?;
            let value = arg(c, args, 0);
            let done = c.truthy_rooted(done)?;
            read_result(c, value, done)
        }
    }
}
