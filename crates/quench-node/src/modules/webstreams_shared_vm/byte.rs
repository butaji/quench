//! Readable byte streams: `ReadableByteStreamController`, `ReadableStreamBYOBReader`,
//! `ReadableStreamBYOBRequest`, their pull-into descriptors and the byte-stream tee.

use super::readable::{
    self, add_request, cancel, desired_size_value, fulfill_request, has_byob_reader, has_default_reader,
    new_request, queued_requests, reset_queue, state, ReadSteps, State,
};
use super::readable::field::{
    CANCEL, CLOSE_REQUESTED, CONTROLLER, HIGH_WATER_MARK, PROMISE, PULL, PULLING, PULL_AGAIN, QUEUE, QUEUE_SIZE,
    REQUESTS, SELF, STARTED, STREAM,
};
use super::readable::{request, tee};
use super::{
    algorithm, arg, buffer_length, callback, class, construct_view, copy_bytes, create_error, custom_inspect,
    deferred, dictionary, flag, get, install, instance, is_detached, is_undefined, key, list, literal, num,
    optional, put, put_flag, put_num, record, resolved_undefined, run_algorithm, slice, slots,
    this_state, throw, transfer, uint8_view, upon, view_parts, Brand, ClassMember, Context, Dictionary,
    Done, Outcome, WebError,
};
use crate::modules::host_members::{captured, current, function, members};
use quench_runtime::{RootId, RootedError};

members!(Member, "webByteStream", [
    ControllerConstruct,
    ByobRequestGetter,
    DesiredSize,
    Close,
    Enqueue,
    Error,
    ControllerInspect,
    RequestConstruct,
    RequestView,
    Respond,
    RespondWithNewView,
    RequestInspect,
    ReaderConstruct,
    ReaderRead,
    ReaderReleaseLock,
    ReaderClosed,
    ReaderCancel,
    ReaderInspect,
    StartFulfilled,
    StartRejected,
    PullFulfilled,
    PullRejected,
    TeePull1,
    TeePull2,
    TeeCancel1,
    TeeCancel2,
    TeeChunk,
    TeeReaderError,
]);

/// A byte controller's fields beyond the default controller's.
pub(crate) mod field {
    /// Set on byte controllers only.
    pub(crate) const BYTE: &str = "byte";
    pub(crate) const BYOB_REQUEST: &str = "byobRequest";
    pub(crate) const PENDING: &str = "pendingPullIntos";
    pub(crate) const AUTO_ALLOCATE: &str = "autoAllocateChunkSize";
    pub(crate) const VIEW: &str = "view";
}
use field::*;

/// A pull-into descriptor's fields.
mod desc {
    pub(super) const BUFFER: &str = "buffer";
    pub(super) const BUFFER_BYTE_LENGTH: &str = "bufferByteLength";
    pub(super) const BYTE_OFFSET: &str = "byteOffset";
    pub(super) const BYTE_LENGTH: &str = "byteLength";
    pub(super) const BYTES_FILLED: &str = "bytesFilled";
    pub(super) const MINIMUM_FILL: &str = "minimumFill";
    pub(super) const ELEMENT_SIZE: &str = "elementSize";
    pub(super) const CTOR: &str = "ctor";
    pub(crate) use crate::modules::keys::TYPE;
}

/// What a pull-into descriptor fills.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PullInto {
    /// A default reader's read, into an auto-allocated buffer.
    Default,
    /// A BYOB reader's read.
    Byob,
    /// A BYOB read whose reader was released.
    None,
}

impl PullInto {
    fn from(value: f64) -> Self {
        match value as u8 {
            0 => Self::Default,
            1 => Self::Byob,
            _ => Self::None,
        }
    }
}

fn kind(c: &mut Context<'_>, descriptor: RootId) -> Result<PullInto, RootedError> {
    Ok(PullInto::from(num(c, descriptor, desc::TYPE)?))
}

fn set_kind(c: &mut Context<'_>, descriptor: RootId, kind: PullInto) -> Done {
    put_num(c, descriptor, desc::TYPE, kind as usize as f64)
}

fn size(c: &mut Context<'_>, object: RootId, name: &str) -> Result<usize, RootedError> {
    Ok(num(c, object, name)? as usize)
}

fn put_size(c: &mut Context<'_>, object: RootId, name: &str, value: usize) -> Done {
    put_num(c, object, name, value as f64)
}

/// The elements of the queue: `{ buffer, byteOffset, byteLength }`.
fn chunk_record(c: &mut Context<'_>, buffer: RootId, offset: usize, length: usize) -> Outcome {
    let entry = record(c, &[(desc::BUFFER, buffer)])?;
    put_size(c, entry, desc::BYTE_OFFSET, offset)?;
    put_size(c, entry, desc::BYTE_LENGTH, length)?;
    Ok(entry)
}

// ---- Setup

/// `setupReadableByteStreamController`: `start` produces the start algorithm's result.
fn setup(
    c: &mut Context<'_>,
    stream: RootId,
    algorithms: &[(&str, RootId)],
    internal: bool,
    high_water_mark: f64,
    auto_allocate: RootId,
    start: impl FnOnce(&mut Context<'_>, RootId) -> Outcome,
) -> Outcome {
    let queue = list::new(c)?;
    let pending = list::new(c)?;
    let null = c.null();
    let controller = record(c, &[(STREAM, stream), (QUEUE, queue), (PENDING, pending), (BYOB_REQUEST, null), (AUTO_ALLOCATE, auto_allocate)])?;
    for &(name, value) in algorithms {
        put(c, controller, name, value)?;
    }
    put_flag(c, controller, BYTE, true)?;
    put_flag(c, controller, algorithm::INTERNAL, internal)?;
    for name in [CLOSE_REQUESTED, PULL_AGAIN, PULLING, STARTED] {
        put_flag(c, controller, name, false)?;
    }
    put_num(c, controller, QUEUE_SIZE, 0.0)?;
    put_num(c, controller, HIGH_WATER_MARK, high_water_mark)?;
    let object = instance(c, Brand::ByteController, controller)?;
    put(c, controller, SELF, object)?;
    put(c, stream, CONTROLLER, controller)?;
    let started = start(c, object)?;
    let promise = deferred(c)?;
    c.resolve_promise_rooted(promise, started)?;
    upon(c, promise, Some(Member::StartFulfilled), Some(Member::StartRejected), &[controller])?;
    Ok(controller)
}

/// `setupReadableByteStreamControllerFromSource(stream, source, highWaterMark)`.
pub(crate) fn setup_from_source(c: &mut Context<'_>, stream: RootId, source: RootId, high_water_mark: f64) -> Done {
    let start = optional(c, source, "start")?;
    let (pull, cancel_algorithm) = if super::is_nullish(c, source) {
        (c.undefined(), c.undefined())
    } else {
        (callback(c, source, PULL, "source.pull")?, callback(c, source, CANCEL, "source.cancel")?)
    };
    let auto_allocate = optional(c, source, AUTO_ALLOCATE)?;
    if c.rooted_value(auto_allocate).and_then(|value| value.as_number()) == Some(0.0) {
        return Err(throw(c, WebError::ArgValue("source.autoAllocateChunkSize", auto_allocate)));
    }
    let algorithms = [(PULL, pull), (CANCEL, cancel_algorithm), (algorithm::SOURCE, source)];
    setup(c, stream, &algorithms, false, high_water_mark, auto_allocate, |c, controller| {
        if c.truthy_rooted(start)? {
            c.call_rooted(start, source, &[controller])
        } else {
            Ok(c.undefined())
        }
    })?;
    Ok(())
}

/// `createReadableByteStream(start, pull, cancel)` over internal algorithms, started.
pub(crate) fn create_byte_readable(c: &mut Context<'_>, pull: RootId, cancel_algorithm: RootId) -> Outcome {
    let undefined = c.undefined();
    let stream = readable::create_readable_shell(c)?;
    setup(c, stream, &[(PULL, pull), (CANCEL, cancel_algorithm)], true, 0.0, undefined, |_, _| Ok(undefined))?;
    Ok(stream)
}

// ---- Controller algorithms

fn stream_of(c: &mut Context<'_>, controller: RootId) -> Outcome {
    get(c, controller, STREAM)
}

fn pending(c: &mut Context<'_>, controller: RootId) -> Outcome {
    get(c, controller, PENDING)
}

fn pending_count(c: &mut Context<'_>, controller: RootId) -> Result<usize, RootedError> {
    let pending = pending(c, controller)?;
    list::len(c, pending)
}

fn first_pending(c: &mut Context<'_>, controller: RootId) -> Outcome {
    let pending = pending(c, controller)?;
    list::peek(c, pending)
}

/// Whether a close or enqueue may still happen: no close requested and the stream readable.
pub(crate) fn can_close(c: &mut Context<'_>, controller: RootId) -> Result<bool, RootedError> {
    let stream = stream_of(c, controller)?;
    Ok(!flag(c, controller, CLOSE_REQUESTED)? && state(c, stream)? == State::Readable)
}

/// `readableByteStreamControllerClose(controller)`.
pub(crate) fn close(c: &mut Context<'_>, controller: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    if flag(c, controller, CLOSE_REQUESTED)? || state(c, stream)? != State::Readable {
        return Ok(());
    }
    if num(c, controller, QUEUE_SIZE)? > 0.0 {
        return put_flag(c, controller, CLOSE_REQUESTED, true);
    }
    if pending_count(c, controller)? > 0 {
        let first = first_pending(c, controller)?;
        if size(c, first, desc::BYTES_FILLED)? % size(c, first, desc::ELEMENT_SIZE)? != 0 {
            let failure = create_error(c, WebError::InvalidState("Partial read"))?;
            error(c, controller, failure)?;
            return Err(c.throw(failure));
        }
    }
    clear_algorithms(c, controller)?;
    readable::close(c, stream)
}

fn commit(c: &mut Context<'_>, stream: RootId, descriptor: RootId) -> Done {
    let done = state(c, stream)? == State::Closed;
    let view = convert(c, descriptor)?;
    fulfill_request(c, stream, view, done)
}

fn commit_all(c: &mut Context<'_>, stream: RootId, descriptors: Vec<RootId>) -> Done {
    for descriptor in descriptors {
        commit(c, stream, descriptor)?;
    }
    Ok(())
}

/// `readableByteStreamControllerConvertPullIntoDescriptor(desc)`.
fn convert(c: &mut Context<'_>, descriptor: RootId) -> Outcome {
    let filled = size(c, descriptor, desc::BYTES_FILLED)?;
    let length = size(c, descriptor, desc::BYTE_LENGTH)?;
    if filled > length {
        return Err(throw(c, WebError::InvalidStateRange("The buffer size is invalid")));
    }
    let element = size(c, descriptor, desc::ELEMENT_SIZE)?;
    let buffer = get(c, descriptor, desc::BUFFER)?;
    let buffer = transfer(c, buffer)?;
    let offset = size(c, descriptor, desc::BYTE_OFFSET)?;
    let constructor = get(c, descriptor, desc::CTOR)?;
    let node_buffer = super::intrinsic(c, key::BUFFER)?;
    if super::same(c, constructor, node_buffer)? {
        let view = uint8_view(c, buffer, offset, filled / element)?;
        let fast = crate::modules::buffer_shared_vm::fast_buffer(c)?;
        let prototype = get(c, fast, "prototype")?;
        c.set_prototype_rooted(view, prototype)?;
        return Ok(view);
    }
    construct_view(c, constructor, buffer, offset, filled / element)
}

fn invalidate_byob_request(c: &mut Context<'_>, controller: RootId) -> Done {
    let current = get(c, controller, BYOB_REQUEST)?;
    if super::is_nullish(c, current) {
        return Ok(());
    }
    let request = slots(c, current)?;
    let undefined = c.undefined();
    let null = c.null();
    put(c, request, CONTROLLER, undefined)?;
    put(c, request, VIEW, null)?;
    put(c, controller, BYOB_REQUEST, null)
}

fn clear_algorithms(c: &mut Context<'_>, controller: RootId) -> Done {
    let undefined = c.undefined();
    put(c, controller, PULL, undefined)?;
    put(c, controller, CANCEL, undefined)
}

fn clear_pending(c: &mut Context<'_>, controller: RootId) -> Done {
    invalidate_byob_request(c, controller)?;
    let pending = list::new(c)?;
    put(c, controller, PENDING, pending)
}

fn should_call_pull(c: &mut Context<'_>, controller: RootId) -> Result<bool, RootedError> {
    let stream = stream_of(c, controller)?;
    if state(c, stream)? != State::Readable || flag(c, controller, CLOSE_REQUESTED)? || !flag(c, controller, STARTED)? {
        return Ok(false);
    }
    if (has_default_reader(c, stream)? || has_byob_reader(c, stream)?) && queued_requests(c, stream)? > 0 {
        return Ok(true);
    }
    Ok(readable::desired_size(c, controller)?.is_some_and(|size| size > 0.0))
}

fn handle_queue_drain(c: &mut Context<'_>, controller: RootId) -> Done {
    if num(c, controller, QUEUE_SIZE)? == 0.0 && flag(c, controller, CLOSE_REQUESTED)? {
        clear_algorithms(c, controller)?;
        let stream = stream_of(c, controller)?;
        return readable::close(c, stream);
    }
    call_pull_if_needed(c, controller)
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

/// The element size and constructor of the view a BYOB read fills.
fn view_type(c: &mut Context<'_>, view: RootId, data_view: bool) -> Result<(usize, RootId), RootedError> {
    if data_view {
        let constructor = super::intrinsic(c, key::DATA_VIEW)?;
        return Ok((1, constructor));
    }
    let constructor = get(c, view, "constructor")?;
    let element = get(c, constructor, "BYTES_PER_ELEMENT")?;
    let element = c.to_number(element)? as usize;
    Ok((element, constructor))
}

/// `readableByteStreamControllerPullInto(controller, view, min, readIntoRequest)`.
fn pull_into(c: &mut Context<'_>, controller: RootId, view: RootId, minimum: usize, request: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    let parts = view_parts(c, view)?.ok_or_else(|| RootedError::host("BYOB read of a non-view"))?;
    let (element, constructor) = view_type(c, view, parts.data_view)?;
    let buffer_byte_length = buffer_length(c, parts.buffer);
    let transferred = match super::attempt(transfer(c, parts.buffer))? {
        Ok(buffer) => buffer,
        Err(failure) => return readable::error_steps(c, request, failure),
    };
    let descriptor = record(c, &[(desc::BUFFER, transferred), (desc::CTOR, constructor)])?;
    for (name, value) in [
        (desc::BUFFER_BYTE_LENGTH, buffer_byte_length),
        (desc::BYTE_OFFSET, parts.offset),
        (desc::BYTE_LENGTH, parts.length),
        (desc::BYTES_FILLED, 0),
        (desc::MINIMUM_FILL, minimum * element),
        (desc::ELEMENT_SIZE, element),
    ] {
        put_size(c, descriptor, name, value)?;
    }
    set_kind(c, descriptor, PullInto::Byob)?;
    let pending = pending(c, controller)?;
    if list::len(c, pending)? > 0 {
        list::push(c, pending, descriptor)?;
        return add_request(c, stream, request);
    }
    if state(c, stream)? == State::Closed {
        let empty = construct_view(c, constructor, transferred, parts.offset, 0)?;
        return readable::close_steps(c, request, empty);
    }
    if num(c, controller, QUEUE_SIZE)? > 0.0 {
        if fill_from_queue(c, controller, descriptor)? {
            let filled = convert(c, descriptor)?;
            handle_queue_drain(c, controller)?;
            return readable::chunk_steps(c, request, filled);
        }
        if flag(c, controller, CLOSE_REQUESTED)? {
            let failure = create_error(c, WebError::InvalidState("ReadableStream closed"))?;
            error(c, controller, failure)?;
            return readable::error_steps(c, request, failure);
        }
    }
    list::push(c, pending, descriptor)?;
    add_request(c, stream, request)?;
    call_pull_if_needed(c, controller)
}

fn respond_internal(c: &mut Context<'_>, controller: RootId, written: usize) -> Done {
    let stream = stream_of(c, controller)?;
    let descriptor = first_pending(c, controller)?;
    invalidate_byob_request(c, controller)?;
    if state(c, stream)? == State::Closed {
        if written != 0 {
            return Err(throw(c, WebError::InvalidState("Controller is closed but view is not zero-length")));
        }
        respond_in_closed_state(c, controller, descriptor)?;
    } else {
        if written == 0 {
            return Err(throw(c, WebError::InvalidState("View cannot be zero-length")));
        }
        respond_in_readable_state(c, controller, written, descriptor)?;
    }
    call_pull_if_needed(c, controller)
}

/// `readableByteStreamControllerRespond(controller, bytesWritten)`.
pub(crate) fn respond(c: &mut Context<'_>, controller: RootId, written: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    let descriptor = first_pending(c, controller)?;
    let count = c.to_number(written)?;
    if state(c, stream)? == State::Closed {
        if count != 0.0 {
            return Err(throw(c, WebError::ArgValue("bytesWritten", written)));
        }
    } else {
        if count == 0.0 {
            return Err(throw(c, WebError::ArgValue("bytesWritten", written)));
        }
        let filled = num(c, descriptor, desc::BYTES_FILLED)?;
        let length = num(c, descriptor, desc::BYTE_LENGTH)?;
        if filled + count > length {
            return Err(throw(c, WebError::ArgValueRange("bytesWritten", written)));
        }
    }
    let buffer = get(c, descriptor, desc::BUFFER)?;
    let buffer = transfer(c, buffer)?;
    put(c, descriptor, desc::BUFFER, buffer)?;
    respond_internal(c, controller, count as usize)
}

fn respond_in_closed_state(c: &mut Context<'_>, controller: RootId, descriptor: RootId) -> Done {
    if kind(c, descriptor)? == PullInto::None {
        shift_pending(c, controller)?;
    }
    let stream = stream_of(c, controller)?;
    if has_byob_reader(c, stream)? {
        let mut filled = Vec::new();
        for _ in 0..queued_requests(c, stream)? {
            filled.push(shift_pending(c, controller)?);
        }
        commit_all(c, stream, filled)?;
    }
    Ok(())
}

fn fill_head(c: &mut Context<'_>, descriptor: RootId, count: usize) -> Done {
    let filled = size(c, descriptor, desc::BYTES_FILLED)?;
    put_size(c, descriptor, desc::BYTES_FILLED, filled + count)
}

/// `readableByteStreamControllerEnqueue(controller, chunk)`.
pub(crate) fn enqueue(c: &mut Context<'_>, controller: RootId, chunk: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    let parts = view_parts(c, chunk)?.ok_or_else(|| RootedError::host("byte enqueue of a non-view"))?;
    if flag(c, controller, CLOSE_REQUESTED)? || state(c, stream)? != State::Readable {
        return Ok(());
    }
    let transferred = transfer(c, parts.buffer)?;
    if pending_count(c, controller)? > 0 {
        let first = first_pending(c, controller)?;
        let buffer = get(c, first, desc::BUFFER)?;
        if is_detached(c, buffer)? {
            return Err(throw(c, WebError::InvalidState("Destination ArrayBuffer is detached")));
        }
        invalidate_byob_request(c, controller)?;
        let buffer = transfer(c, buffer)?;
        put(c, first, desc::BUFFER, buffer)?;
        if kind(c, first)? == PullInto::None {
            enqueue_detached_pull_into(c, controller, first)?;
        }
    }
    if has_default_reader(c, stream)? {
        process_read_requests(c, controller)?;
        if queued_requests(c, stream)? == 0 {
            enqueue_chunk(c, controller, transferred, parts.offset, parts.length)?;
        } else {
            if pending_count(c, controller)? > 0 {
                shift_pending(c, controller)?;
            }
            let view = uint8_view(c, transferred, parts.offset, parts.length)?;
            fulfill_request(c, stream, view, false)?;
        }
    } else if has_byob_reader(c, stream)? {
        enqueue_chunk(c, controller, transferred, parts.offset, parts.length)?;
        let filled = process_pull_intos(c, controller)?;
        commit_all(c, stream, filled)?;
    } else {
        enqueue_chunk(c, controller, transferred, parts.offset, parts.length)?;
    }
    call_pull_if_needed(c, controller)
}

fn enqueue_cloned_chunk(c: &mut Context<'_>, controller: RootId, buffer: RootId, offset: usize, length: usize) -> Done {
    match super::attempt(slice(c, buffer, offset, offset + length))? {
        Ok(clone) => enqueue_chunk(c, controller, clone, 0, length),
        Err(failure) => {
            error(c, controller, failure)?;
            Err(c.throw(failure))
        }
    }
}

fn enqueue_chunk(c: &mut Context<'_>, controller: RootId, buffer: RootId, offset: usize, length: usize) -> Done {
    let entry = chunk_record(c, buffer, offset, length)?;
    let queue = get(c, controller, QUEUE)?;
    list::push(c, queue, entry)?;
    let total = num(c, controller, QUEUE_SIZE)?;
    put_num(c, controller, QUEUE_SIZE, total + length as f64)
}

fn enqueue_detached_pull_into(c: &mut Context<'_>, controller: RootId, descriptor: RootId) -> Done {
    let filled = size(c, descriptor, desc::BYTES_FILLED)?;
    if filled > 0 {
        let buffer = get(c, descriptor, desc::BUFFER)?;
        let offset = size(c, descriptor, desc::BYTE_OFFSET)?;
        enqueue_cloned_chunk(c, controller, buffer, offset, filled)?;
    }
    shift_pending(c, controller).map(drop)
}

/// `readableByteStreamControllerFillPullIntoDescriptorFromQueue(controller, desc)`.
fn fill_from_queue(c: &mut Context<'_>, controller: RootId, descriptor: RootId) -> Result<bool, RootedError> {
    let buffer = get(c, descriptor, desc::BUFFER)?;
    let length = size(c, descriptor, desc::BYTE_LENGTH)?;
    let offset = size(c, descriptor, desc::BYTE_OFFSET)?;
    let filled = size(c, descriptor, desc::BYTES_FILLED)?;
    let minimum = size(c, descriptor, desc::MINIMUM_FILL)?;
    let element = size(c, descriptor, desc::ELEMENT_SIZE)?;
    let total = size(c, controller, QUEUE_SIZE)?;
    let max_copy = total.min(length - filled);
    let max_filled = filled + max_copy;
    let max_aligned = max_filled - max_filled % element;
    let mut remaining = max_copy;
    let mut ready = false;
    if max_aligned >= minimum {
        remaining = max_aligned - filled;
        ready = true;
    }
    let queue = get(c, controller, QUEUE)?;
    while remaining > 0 {
        let head = list::peek(c, queue)?;
        let head_length = size(c, head, desc::BYTE_LENGTH)?;
        let head_offset = size(c, head, desc::BYTE_OFFSET)?;
        let count = remaining.min(head_length);
        let destination = offset + size(c, descriptor, desc::BYTES_FILLED)?;
        let source = get(c, head, desc::BUFFER)?;
        copy_bytes(c, buffer, destination, source, head_offset, count)?;
        if head_length == count {
            list::shift(c, queue)?;
        } else {
            put_size(c, head, desc::BYTE_OFFSET, head_offset + count)?;
            put_size(c, head, desc::BYTE_LENGTH, head_length - count)?;
        }
        let total = size(c, controller, QUEUE_SIZE)?;
        put_size(c, controller, QUEUE_SIZE, total - count)?;
        fill_head(c, descriptor, count)?;
        remaining -= count;
    }
    Ok(ready)
}

fn process_pull_intos(c: &mut Context<'_>, controller: RootId) -> Result<Vec<RootId>, RootedError> {
    let mut filled = Vec::new();
    while pending_count(c, controller)? > 0 {
        if num(c, controller, QUEUE_SIZE)? == 0.0 {
            break;
        }
        let descriptor = first_pending(c, controller)?;
        if fill_from_queue(c, controller, descriptor)? {
            shift_pending(c, controller)?;
            filled.push(descriptor);
        }
    }
    Ok(filled)
}

fn respond_in_readable_state(c: &mut Context<'_>, controller: RootId, written: usize, descriptor: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    let filled = size(c, descriptor, desc::BYTES_FILLED)?;
    let length = size(c, descriptor, desc::BYTE_LENGTH)?;
    if filled + written > length {
        return Err(throw(c, WebError::InvalidStateRange("The buffer size is invalid")));
    }
    fill_head(c, descriptor, written)?;
    if kind(c, descriptor)? == PullInto::None {
        enqueue_detached_pull_into(c, controller, descriptor)?;
        let filled = process_pull_intos(c, controller)?;
        return commit_all(c, stream, filled);
    }
    let filled = size(c, descriptor, desc::BYTES_FILLED)?;
    if filled < size(c, descriptor, desc::MINIMUM_FILL)? {
        return Ok(());
    }
    shift_pending(c, controller)?;
    let remainder = filled % size(c, descriptor, desc::ELEMENT_SIZE)?;
    if remainder > 0 {
        let end = size(c, descriptor, desc::BYTE_OFFSET)? + filled;
        let buffer = get(c, descriptor, desc::BUFFER)?;
        let rest = slice(c, buffer, end - remainder, end)?;
        let rest_length = buffer_length(c, rest);
        enqueue_chunk(c, controller, rest, 0, rest_length)?;
    }
    put_size(c, descriptor, desc::BYTES_FILLED, filled - remainder)?;
    let others = process_pull_intos(c, controller)?;
    commit(c, stream, descriptor)?;
    commit_all(c, stream, others)
}

/// `readableByteStreamControllerRespondWithNewView(controller, view)`.
fn respond_with_new_view(c: &mut Context<'_>, controller: RootId, view: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    let descriptor = first_pending(c, controller)?;
    let parts = view_parts(c, view)?.ok_or_else(|| RootedError::host("respond with a non-view"))?;
    let view_buffer_length = buffer_length(c, parts.buffer);
    if state(c, stream)? == State::Closed {
        if parts.length != 0 {
            return Err(throw(c, WebError::InvalidState("View is not zero-length")));
        }
    } else if parts.length == 0 {
        return Err(throw(c, WebError::InvalidState("View is zero-length")));
    }
    let offset = size(c, descriptor, desc::BYTE_OFFSET)?;
    let filled = size(c, descriptor, desc::BYTES_FILLED)?;
    let length = size(c, descriptor, desc::BYTE_LENGTH)?;
    let buffer_byte_length = size(c, descriptor, desc::BUFFER_BYTE_LENGTH)?;
    if offset + filled != parts.offset || filled + parts.length > length || buffer_byte_length != view_buffer_length {
        return Err(throw(c, WebError::ArgValueRange("view", view)));
    }
    let buffer = transfer(c, parts.buffer)?;
    put(c, descriptor, desc::BUFFER, buffer)?;
    respond_internal(c, controller, parts.length)
}

fn shift_pending(c: &mut Context<'_>, controller: RootId) -> Outcome {
    let pending = pending(c, controller)?;
    list::shift(c, pending)
}

/// `readableByteStreamControllerError(controller, error)`.
pub(crate) fn error(c: &mut Context<'_>, controller: RootId, failure: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    if state(c, stream)? != State::Readable {
        return Ok(());
    }
    clear_pending(c, controller)?;
    reset_queue(c, controller)?;
    clear_algorithms(c, controller)?;
    readable::error(c, stream, failure)
}

pub(crate) fn cancel_steps(c: &mut Context<'_>, controller: RootId, reason: RootId) -> Outcome {
    clear_pending(c, controller)?;
    reset_queue(c, controller)?;
    let result = run_algorithm(c, controller, CANCEL, &[reason])?;
    clear_algorithms(c, controller)?;
    Ok(result)
}

fn fill_read_request(c: &mut Context<'_>, controller: RootId, request: RootId) -> Done {
    let queue = get(c, controller, QUEUE)?;
    let entry = list::shift(c, queue)?;
    let length = size(c, entry, desc::BYTE_LENGTH)?;
    let total = size(c, controller, QUEUE_SIZE)?;
    put_size(c, controller, QUEUE_SIZE, total - length)?;
    handle_queue_drain(c, controller)?;
    let buffer = get(c, entry, desc::BUFFER)?;
    let offset = size(c, entry, desc::BYTE_OFFSET)?;
    let view = uint8_view(c, buffer, offset, length)?;
    readable::chunk_steps(c, request, view)
}

fn process_read_requests(c: &mut Context<'_>, controller: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    let reader = get(c, stream, super::readable::field::READER)?;
    let reader = slots(c, reader)?;
    let requests = get(c, reader, REQUESTS)?;
    while list::len(c, requests)? > 0 {
        if num(c, controller, QUEUE_SIZE)? == 0.0 {
            return Ok(());
        }
        let request = list::shift(c, requests)?;
        fill_read_request(c, controller, request)?;
    }
    Ok(())
}

/// `readableByteStreamControllerPullSteps(controller, readRequest)`.
pub(crate) fn pull_steps(c: &mut Context<'_>, controller: RootId, request: RootId) -> Done {
    let stream = stream_of(c, controller)?;
    if num(c, controller, QUEUE_SIZE)? > 0.0 {
        return fill_read_request(c, controller, request);
    }
    let auto_allocate = get(c, controller, AUTO_ALLOCATE)?;
    if !is_undefined(c, auto_allocate) {
        let array_buffer = super::intrinsic(c, key::ARRAY_BUFFER)?;
        match super::attempt(c.construct_rooted(array_buffer, array_buffer, &[auto_allocate]))? {
            Ok(buffer) => {
                let chunk_size = c.to_number(auto_allocate)? as usize;
                let constructor = super::intrinsic(c, key::UINT8_ARRAY)?;
                let descriptor = record(c, &[(desc::BUFFER, buffer), (desc::CTOR, constructor)])?;
                for (name, value) in [
                    (desc::BUFFER_BYTE_LENGTH, chunk_size),
                    (desc::BYTE_OFFSET, 0),
                    (desc::BYTE_LENGTH, chunk_size),
                    (desc::BYTES_FILLED, 0),
                    (desc::MINIMUM_FILL, 1),
                    (desc::ELEMENT_SIZE, 1),
                ] {
                    put_size(c, descriptor, name, value)?;
                }
                set_kind(c, descriptor, PullInto::Default)?;
                let pending = pending(c, controller)?;
                list::push(c, pending, descriptor)?;
            }
            Err(failure) => return readable::error_steps(c, request, failure),
        }
    }
    add_request(c, stream, request)?;
    call_pull_if_needed(c, controller)
}

/// The controller's `[kRelease]()`: only the first pending pull-into remains, detached.
pub(crate) fn release_steps(c: &mut Context<'_>, controller: RootId) -> Done {
    if pending_count(c, controller)? == 0 {
        return Ok(());
    }
    let first = first_pending(c, controller)?;
    set_kind(c, first, PullInto::None)?;
    let pending = list::new(c)?;
    list::push(c, pending, first)?;
    put(c, controller, PENDING, pending)
}

/// `validateBuffer(value, name)`.
fn validate_view(c: &mut Context<'_>, value: RootId, name: &str) -> Result<super::ViewParts, RootedError> {
    match view_parts(c, value)? {
        Some(parts) => Ok(parts),
        None => Err(throw(c, WebError::ArgType(name, "Buffer, TypedArray, or DataView", value))),
    }
}

// ---- The BYOB reader

/// `ReadableStreamBYOBReader.prototype.read(view, options)`, an async method.
fn reader_read(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let promise = deferred(c)?;
    match super::attempt(reader_read_steps(c, this, args))? {
        Ok(result) => c.resolve_promise_rooted(promise, result)?,
        Err(failure) => c.reject_promise_rooted(promise, failure)?,
    }
    Ok(promise)
}

fn reader_read_steps(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let reader = this_state(c, this, Brand::ByobReader)?;
    let view = arg(c, args, 0);
    let Some(parts) = view_parts(c, view)? else {
        return Err(throw(c, WebError::ArgType("view", "Buffer, TypedArray, or DataView", view)));
    };
    let options = dictionary(c, args, 1, "options", Dictionary::ObjectsAndNull)?;
    let buffer_byte_length = buffer_length(c, parts.buffer);
    if parts.length == 0 || buffer_byte_length == 0 {
        return Err(throw(c, WebError::InvalidState("View or Viewed ArrayBuffer is zero-length or detached")));
    }
    let min = optional(c, options, "min")?;
    let min = if super::is_nullish(c, min) { c.number(1.0) } else { min };
    let Some(minimum) = c.rooted_value(min).and_then(|value| value.as_number()) else {
        return Err(throw(c, WebError::ArgType("options.min", "number", min)));
    };
    if minimum.fract() != 0.0 || !minimum.is_finite() {
        return Err(throw(c, WebError::ArgValueBecause("options.min", min, "must be an integer")));
    }
    if minimum <= 0.0 {
        return Err(throw(c, WebError::ArgValueBecause("options.min", min, "must be greater than 0")));
    }
    if parts.data_view {
        if minimum > parts.length as f64 {
            return Err(throw(c, WebError::OutOfRange("options.min", "<= view.byteLength", min)));
        }
    } else {
        let length = get(c, view, "length")?;
        if minimum > c.to_number(length)? {
            return Err(throw(c, WebError::OutOfRange("options.min", "<= view.length", min)));
        }
    }
    let stream = get(c, reader, STREAM)?;
    if is_undefined(c, stream) {
        return Err(throw(c, WebError::InvalidState("The reader is not attached to a stream")));
    }
    let promise = deferred(c)?;
    let request = new_request(c, ReadSteps::Into, &[(PROMISE, promise)])?;
    byob_read(c, reader, view, minimum as usize, request)?;
    Ok(promise)
}

/// `readableStreamBYOBReaderRead(reader, view, min, readIntoRequest)`.
fn byob_read(c: &mut Context<'_>, reader: RootId, view: RootId, minimum: usize, request: RootId) -> Done {
    let stream = get(c, reader, STREAM)?;
    put_flag(c, stream, super::readable::field::DISTURBED, true)?;
    if state(c, stream)? == State::Errored {
        let failure = get(c, stream, super::readable::field::STORED_ERROR)?;
        return readable::error_steps(c, request, failure);
    }
    let controller = get(c, stream, CONTROLLER)?;
    pull_into(c, controller, view, minimum, request)
}

// ---- The byte tee

/// `readableByteStreamTee(stream)`.
pub(crate) fn tee(c: &mut Context<'_>, stream: RootId) -> Outcome {
    let reader = readable::default_reader(c, stream)?;
    let reader = slots(c, reader)?;
    let cancel_promise = deferred(c)?;
    let tee = record(c, &[(tee::STREAM, stream), (tee::READER, reader), (tee::CANCEL_PROMISE, cancel_promise)])?;
    for name in [tee::READING, tee::READ_AGAIN1, tee::READ_AGAIN2, tee::CANCELED1, tee::CANCELED2, tee::CANCEL_RESOLVED] {
        put_flag(c, tee, name, false)?;
    }
    let undefined = c.undefined();
    let mut branches = [undefined; 2];
    for (index, (name, pull, cancel_member)) in
        [(tee::BRANCH1, Member::TeePull1, Member::TeeCancel1), (tee::BRANCH2, Member::TeePull2, Member::TeeCancel2)]
            .into_iter()
            .enumerate()
    {
        let pull = function(c, pull, "", &[tee])?;
        let cancel_algorithm = function(c, cancel_member, "", &[tee])?;
        let branch = create_byte_readable(c, pull, cancel_algorithm)?;
        put(c, tee, name, branch)?;
        branches[index] = get(c, branch, SELF)?;
    }
    forward_reader_error(c, tee, reader)?;
    c.array_rooted(&branches)
}

fn forward_reader_error(c: &mut Context<'_>, tee: RootId, reader: RootId) -> Done {
    let closed = get(c, reader, super::readable::field::CLOSED)?;
    upon(c, closed, None, Some(Member::TeeReaderError), &[tee, reader]).map(drop)
}

fn branch_controller(c: &mut Context<'_>, tee: RootId, second: bool) -> Outcome {
    readable::branch_controller(c, tee, if second { tee::BRANCH2 } else { tee::BRANCH1 })
}

fn canceled(c: &mut Context<'_>, tee: RootId, second: bool) -> Result<bool, RootedError> {
    flag(c, tee, if second { tee::CANCELED2 } else { tee::CANCELED1 })
}

fn pull_with_default_reader(c: &mut Context<'_>, tee: RootId) -> Done {
    let reader = get(c, tee, tee::READER)?;
    if num(c, reader, super::BRAND)? == Brand::ByobReader as usize as f64 {
        readable::reader_release(c, reader)?;
        let stream = get(c, tee, tee::STREAM)?;
        let fresh = readable::default_reader(c, stream)?;
        let fresh = slots(c, fresh)?;
        put(c, tee, tee::READER, fresh)?;
        forward_reader_error(c, tee, fresh)?;
    }
    let reader = get(c, tee, tee::READER)?;
    let request = new_request(c, ReadSteps::ByteTee, &[(request::TEE, tee)])?;
    readable::default_reader_read(c, reader, request)
}

fn pull_with_byob_reader(c: &mut Context<'_>, tee: RootId, view: RootId, second: bool) -> Done {
    let reader = get(c, tee, tee::READER)?;
    if num(c, reader, super::BRAND)? == Brand::DefaultReader as usize as f64 {
        readable::reader_release(c, reader)?;
        let stream = get(c, tee, tee::STREAM)?;
        let fresh = readable::byob_reader(c, stream)?;
        let fresh = slots(c, fresh)?;
        put(c, tee, tee::READER, fresh)?;
        forward_reader_error(c, tee, fresh)?;
    }
    let reader = get(c, tee, tee::READER)?;
    let branch = c.boolean(second);
    let request = new_request(c, ReadSteps::ByteTeeInto, &[(request::TEE, tee), (request::BRANCH, branch)])?;
    byob_read(c, reader, view, 1, request)
}

fn tee_pull(c: &mut Context<'_>, tee: RootId, second: bool) -> Outcome {
    if flag(c, tee, tee::READING)? {
        put_flag(c, tee, if second { tee::READ_AGAIN2 } else { tee::READ_AGAIN1 }, true)?;
        return resolved_undefined(c);
    }
    put_flag(c, tee, tee::READING, true)?;
    let controller = branch_controller(c, tee, second)?;
    let request = byob_request(c, controller)?;
    if super::is_nullish(c, request) {
        pull_with_default_reader(c, tee)?;
    } else {
        let request = slots(c, request)?;
        let view = get(c, request, VIEW)?;
        pull_with_byob_reader(c, tee, view, second)?;
    }
    resolved_undefined(c)
}

fn tee_cancel(c: &mut Context<'_>, tee: RootId, reason: RootId, second: bool) -> Outcome {
    let (mine, reason_field, other) =
        if second { (tee::CANCELED2, tee::REASON2, tee::CANCELED1) } else { (tee::CANCELED1, tee::REASON1, tee::CANCELED2) };
    put_flag(c, tee, mine, true)?;
    put(c, tee, reason_field, reason)?;
    if flag(c, tee, other)? {
        let reason1 = get(c, tee, tee::REASON1)?;
        let reason2 = get(c, tee, tee::REASON2)?;
        let composite = c.array_rooted(&[reason1, reason2])?;
        let stream = get(c, tee, tee::STREAM)?;
        let result = cancel(c, stream, composite)?;
        readable::resolve_cancel(c, tee, result)?;
    }
    get(c, tee, tee::CANCEL_PROMISE)
}

/// A byte tee read's chunk steps: the work runs in a microtask, as in Node.
pub(crate) fn tee_chunk(c: &mut Context<'_>, request: RootId, chunk: RootId) -> Done {
    let task = function(c, Member::TeeChunk, "", &[request, chunk])?;
    c.queue_microtask_rooted(task, &[])
}

/// `cloneAsUint8Array(view)`.
fn clone_as_uint8(c: &mut Context<'_>, view: RootId) -> Outcome {
    let parts = view_parts(c, view)?.ok_or_else(|| RootedError::host("clone of a non-view"))?;
    let copy = slice(c, parts.buffer, parts.offset, parts.offset + parts.length)?;
    let length = buffer_length(c, copy);
    uint8_view(c, copy, 0, length)
}

fn tee_chunk_task(c: &mut Context<'_>, request: RootId, chunk: RootId) -> Done {
    let tee = get(c, request, request::TEE)?;
    put_flag(c, tee, tee::READ_AGAIN1, false)?;
    put_flag(c, tee, tee::READ_AGAIN2, false)?;
    let into = readable::steps_kind(c, request)? == ReadSteps::ByteTeeInto;
    let second = into && flag(c, request, request::BRANCH)?;
    // The branch a BYOB read fills, and the other; a default read fills both alike.
    let (own, other) = (second, !second);
    let own_canceled = canceled(c, tee, own)?;
    let other_canceled = canceled(c, tee, other)?;
    let fail_both = |c: &mut Context<'_>, failure: RootId| -> Done {
        for branch in [own, other] {
            let controller = branch_controller(c, tee, branch)?;
            error(c, controller, failure)?;
        }
        let stream = get(c, tee, tee::STREAM)?;
        let result = cancel(c, stream, failure)?;
        readable::resolve_cancel(c, tee, result)
    };
    if into {
        if !other_canceled {
            let cloned = match super::attempt(clone_as_uint8(c, chunk))? {
                Ok(cloned) => cloned,
                Err(failure) => return fail_both(c, failure),
            };
            if !own_canceled {
                let controller = branch_controller(c, tee, own)?;
                respond_with_new_view(c, controller, chunk)?;
            }
            let controller = branch_controller(c, tee, other)?;
            enqueue(c, controller, cloned)?;
        } else if !own_canceled {
            let controller = branch_controller(c, tee, own)?;
            respond_with_new_view(c, controller, chunk)?;
        }
    } else {
        let canceled1 = canceled(c, tee, false)?;
        let canceled2 = canceled(c, tee, true)?;
        let mut second_chunk = chunk;
        if !canceled1 && !canceled2 {
            match super::attempt(clone_as_uint8(c, chunk))? {
                Ok(cloned) => second_chunk = cloned,
                Err(failure) => return fail_both(c, failure),
            }
        }
        for (branch, canceled, value) in [(false, canceled1, chunk), (true, canceled2, second_chunk)] {
            if !canceled {
                let controller = branch_controller(c, tee, branch)?;
                enqueue(c, controller, value)?;
            }
        }
    }
    put_flag(c, tee, tee::READING, false)?;
    if flag(c, tee, tee::READ_AGAIN1)? {
        tee_pull(c, tee, false)?;
    } else if flag(c, tee, tee::READ_AGAIN2)? {
        tee_pull(c, tee, true)?;
    }
    Ok(())
}

/// A byte tee read's close steps; `chunk` is the view a BYOB read filled.
pub(crate) fn tee_close(c: &mut Context<'_>, request: RootId, chunk: RootId) -> Done {
    let tee = get(c, request, request::TEE)?;
    put_flag(c, tee, tee::READING, false)?;
    let into = readable::steps_kind(c, request)? == ReadSteps::ByteTeeInto;
    let second = into && flag(c, request, request::BRANCH)?;
    let (own, other) = (second, !second);
    let own_canceled = canceled(c, tee, own)?;
    let other_canceled = canceled(c, tee, other)?;
    for (branch, canceled) in [(own, own_canceled), (other, other_canceled)] {
        if !canceled {
            let controller = branch_controller(c, tee, branch)?;
            close(c, controller)?;
        }
    }
    if into {
        if !is_undefined(c, chunk) {
            if !own_canceled {
                let controller = branch_controller(c, tee, own)?;
                respond_with_new_view(c, controller, chunk)?;
            }
            let controller = branch_controller(c, tee, other)?;
            if !other_canceled && pending_count(c, controller)? > 0 {
                let zero = c.number(0.0);
                respond(c, controller, zero)?;
            }
        }
    } else {
        for branch in [false, true] {
            let controller = branch_controller(c, tee, branch)?;
            if pending_count(c, controller)? > 0 {
                let zero = c.number(0.0);
                respond(c, controller, zero)?;
            }
        }
    }
    if !own_canceled || !other_canceled {
        let undefined = c.undefined();
        readable::resolve_cancel(c, tee, undefined)?;
    }
    Ok(())
}

/// The controller's `byobRequest`, made on demand from the first pending pull-into.
fn byob_request(c: &mut Context<'_>, controller: RootId) -> Outcome {
    let current = get(c, controller, BYOB_REQUEST)?;
    if super::is_nullish(c, current) && pending_count(c, controller)? > 0 {
        let first = first_pending(c, controller)?;
        let buffer = get(c, first, desc::BUFFER)?;
        let offset = size(c, first, desc::BYTE_OFFSET)?;
        let filled = size(c, first, desc::BYTES_FILLED)?;
        let length = size(c, first, desc::BYTE_LENGTH)?;
        let view = uint8_view(c, buffer, offset + filled, length - filled)?;
        let request = record(c, &[(CONTROLLER, controller), (VIEW, view)])?;
        let request = instance(c, Brand::ByobRequest, request)?;
        put(c, controller, BYOB_REQUEST, request)?;
        return Ok(request);
    }
    Ok(current)
}

// ---- Classes

const CONTROLLER_MEMBERS: [ClassMember<Member>; 5] = [
    ClassMember::Getter("byobRequest", Member::ByobRequestGetter),
    ClassMember::Getter("desiredSize", Member::DesiredSize),
    ClassMember::Method("close", Member::Close, 0),
    ClassMember::Method("enqueue", Member::Enqueue, 1),
    ClassMember::Method("error", Member::Error, 0),
];

const REQUEST_MEMBERS: [ClassMember<Member>; 3] = [
    ClassMember::Getter("view", Member::RequestView),
    ClassMember::Method("respond", Member::Respond, 1),
    ClassMember::Method("respondWithNewView", Member::RespondWithNewView, 1),
];

const READER_MEMBERS: [ClassMember<Member>; 4] = [
    ClassMember::Getter("closed", Member::ReaderClosed),
    ClassMember::Method("read", Member::ReaderRead, 1),
    ClassMember::Method("releaseLock", Member::ReaderReleaseLock, 0),
    ClassMember::Method("cancel", Member::ReaderCancel, 0),
];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Done {
    for (brand, construct, inspect, length, members) in [
        (Brand::ByteController, Member::ControllerConstruct, Member::ControllerInspect, 0, &CONTROLLER_MEMBERS[..]),
        (Brand::ByobRequest, Member::RequestConstruct, Member::RequestInspect, 0, &REQUEST_MEMBERS[..]),
        (Brand::ByobReader, Member::ReaderConstruct, Member::ReaderInspect, 1, &READER_MEMBERS[..]),
    ] {
        let constructor = class(c, brand, construct, length, inspect)?;
        let prototype = get(c, constructor, "prototype")?;
        install(c, prototype, members)?;
        put(c, table, brand.name(), constructor)?;
    }
    Ok(())
}

fn controller_of(c: &mut Context<'_>, this: RootId) -> Outcome {
    this_state(c, this, Brand::ByteController)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::ControllerConstruct | Member::RequestConstruct => {
            super::require_new(c, if member == Member::ControllerConstruct { Brand::ByteController.name() } else { Brand::ByobRequest.name() })?;
            Err(throw(c, WebError::IllegalConstructor))
        }
        Member::ByobRequestGetter => {
            let controller = controller_of(c, this)?;
            byob_request(c, controller)
        }
        Member::DesiredSize => {
            let controller = controller_of(c, this)?;
            desired_size_value(c, controller)
        }
        Member::Close => {
            let controller = controller_of(c, this)?;
            if flag(c, controller, CLOSE_REQUESTED)? {
                return Err(throw(c, WebError::InvalidState("Controller is already closed")));
            }
            let stream = stream_of(c, controller)?;
            if state(c, stream)? != State::Readable {
                return Err(throw(c, WebError::InvalidState("ReadableStream is already closed")));
            }
            close(c, controller)?;
            Ok(c.undefined())
        }
        Member::Enqueue => {
            let controller = controller_of(c, this)?;
            let chunk = arg(c, args, 0);
            let parts = validate_view(c, chunk, "buffer")?;
            let buffer_byte_length = buffer_length(c, parts.buffer);
            if parts.length == 0 || buffer_byte_length == 0 {
                return Err(throw(c, WebError::InvalidState("chunk ArrayBuffer is zero-length or detached")));
            }
            if flag(c, controller, CLOSE_REQUESTED)? {
                return Err(throw(c, WebError::InvalidState("Controller is already closed")));
            }
            let stream = stream_of(c, controller)?;
            if state(c, stream)? != State::Readable {
                return Err(throw(c, WebError::InvalidState("ReadableStream is already closed")));
            }
            enqueue(c, controller, chunk)?;
            Ok(c.undefined())
        }
        Member::Error => {
            let controller = controller_of(c, this)?;
            let failure = arg(c, args, 0);
            error(c, controller, failure)?;
            Ok(c.undefined())
        }
        Member::ControllerInspect => {
            controller_of(c, this)?;
            let data = c.object_rooted()?;
            custom_inspect(c, this, Brand::ByteController.name(), data, args)
        }
        Member::RequestView => {
            let request = this_state(c, this, Brand::ByobRequest)?;
            get(c, request, VIEW)
        }
        Member::Respond => {
            let request = this_state(c, this, Brand::ByobRequest)?;
            let controller = get(c, request, CONTROLLER)?;
            if is_undefined(c, controller) {
                return Err(throw(c, WebError::InvalidState("This BYOB request has been invalidated")));
            }
            let view = get(c, request, VIEW)?;
            if let Some(parts) = view_parts(c, view)? {
                if is_detached(c, parts.buffer)? {
                    return Err(throw(c, WebError::InvalidState("Viewed ArrayBuffer is detached")));
                }
            }
            let written = arg(c, args, 0);
            respond(c, controller, written)?;
            Ok(c.undefined())
        }
        Member::RespondWithNewView => {
            let request = this_state(c, this, Brand::ByobRequest)?;
            let controller = get(c, request, CONTROLLER)?;
            if is_undefined(c, controller) {
                return Err(throw(c, WebError::InvalidState("This BYOB request has been invalidated")));
            }
            let view = arg(c, args, 0);
            let parts = validate_view(c, view, "view")?;
            if is_detached(c, parts.buffer)? {
                return Err(throw(c, WebError::InvalidState("Viewed ArrayBuffer is detached")));
            }
            respond_with_new_view(c, controller, view)?;
            Ok(c.undefined())
        }
        Member::RequestInspect => {
            let request = this_state(c, this, Brand::ByobRequest)?;
            let view = get(c, request, VIEW)?;
            let controller = get(c, request, CONTROLLER)?;
            let controller = if is_undefined(c, controller) { controller } else { get(c, controller, SELF)? };
            let data = literal(c, &[("view", view), ("controller", controller)])?;
            custom_inspect(c, this, Brand::ByobRequest.name(), data, args)
        }
        Member::ReaderConstruct => {
            super::require_new(c, Brand::ByobReader.name())?;
            let stream = arg(c, args, 0);
            readable::construct_byob_reader(c, this, stream)?;
            Ok(c.undefined())
        }
        Member::ReaderRead => reader_read(c, this, args),
        Member::ReaderReleaseLock => readable::reader_release_lock(c, this, Brand::ByobReader),
        Member::ReaderClosed => readable::reader_closed(c, this, Brand::ByobReader),
        Member::ReaderCancel => {
            let reason = arg(c, args, 0);
            readable::reader_cancel(c, this, Brand::ByobReader, reason)
        }
        Member::ReaderInspect => readable::reader_inspect(c, this, Brand::ByobReader, "readIntoRequests", args),
        Member::StartFulfilled => {
            let [controller] = captured(c)?;
            put_flag(c, controller, STARTED, true)?;
            call_pull_if_needed(c, controller)?;
            Ok(c.undefined())
        }
        Member::StartRejected | Member::PullRejected => {
            let [controller] = captured(c)?;
            let failure = arg(c, args, 0);
            error(c, controller, failure)?;
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
        Member::TeePull1 | Member::TeePull2 => {
            let [tee] = captured(c)?;
            tee_pull(c, tee, member == Member::TeePull2)
        }
        Member::TeeCancel1 | Member::TeeCancel2 => {
            let [tee] = captured(c)?;
            let reason = arg(c, args, 0);
            tee_cancel(c, tee, reason, member == Member::TeeCancel2)
        }
        Member::TeeChunk => {
            let [request, chunk] = captured(c)?;
            tee_chunk_task(c, request, chunk)?;
            Ok(c.undefined())
        }
        Member::TeeReaderError => {
            let [tee, reader] = captured(c)?;
            let current_reader = get(c, tee, tee::READER)?;
            if !super::same(c, current_reader, reader)? {
                return Ok(c.undefined());
            }
            let failure = arg(c, args, 0);
            for branch in [false, true] {
                let controller = branch_controller(c, tee, branch)?;
                error(c, controller, failure)?;
            }
            if !canceled(c, tee, false)? || !canceled(c, tee, true)? {
                let undefined = c.undefined();
                readable::resolve_cancel(c, tee, undefined)?;
            }
            Ok(c.undefined())
        }
    }
}
