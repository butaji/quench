//! `ReadableStream.prototype.pipeTo` and `pipeThrough`: Node's `readableStreamPipeTo`, its
//! `run`/`step` loop and its shutdown actions.

use super::readable::field::{CLOSED, PROMISE, STORED_ERROR};
use super::readable::{self, new_request, request, ReadSteps};
use super::writable::{self, field::READY};
use super::{
    arg, create_error, deferred, dictionary, fail, flag, get, is_undefined, mark_handled, optional, put, put_flag,
    record, rejected, resolved, resolved_undefined, settle, settle_undefined, slots, state_of, this_state, throw, upon,
    Brand, Context, Dictionary, Done, Outcome, WebError,
};
use crate::modules::host_members::{captured, current, function, members};
use quench_runtime::{RootId, RootedError};

members!(Member, "webPipe", [
    StepReady,
    StepFailed,
    Stepped,
    RunFailed,
    WaitWritten,
    WaitSettled,
    WaitFailed,
    ShutdownWritten,
    ShutdownFailed,
    ActionDone,
    ActionFailed,
    SourceErrored,
    DestErrored,
    SourceClosed,
    Nop,
    Abort,
    AllFulfilled,
    AllRejected,
]);

/// A pipe's state: Node's `readableStreamPipeTo` closure variables.
mod field {
    pub(crate) use crate::modules::keys::SOURCE;
    pub(super) const DEST: &str = "dest";
    pub(crate) use crate::modules::keys::READER;
    pub(crate) use crate::modules::keys::WRITER;
    pub(super) const PREVENT_CLOSE: &str = "preventClose";
    pub(super) const PREVENT_ABORT: &str = "preventAbort";
    pub(crate) use crate::modules::keys::PREVENT_CANCEL;
    pub(crate) use crate::modules::keys::SIGNAL;
    pub(crate) use crate::modules::keys::LISTENER;
    pub(super) const SHUTTING_DOWN: &str = "shuttingDown";
    pub(super) const CURRENT_WRITE: &str = "currentWrite";
    /// An `Promise.all` aggregate's fields.
    pub(super) const REMAINING: &str = "remaining";
}
use field::*;

/// A shutdown action (`shutdownWithAnAction`'s `action`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    AbortDest,
    CancelSource,
    CloseDest,
    /// The abort signal's: abort the destination and cancel the source, as not prevented.
    Signal,
}

impl Action {
    const ALL: [Self; 4] = [Self::AbortDest, Self::CancelSource, Self::CloseDest, Self::Signal];
}

fn validate_signal(c: &mut Context<'_>, signal: RootId) -> Done {
    crate::modules::util_shared_vm::validate_abort_signal(c, signal, "options.signal")
}

/// The validated `(preventClose, preventAbort, preventCancel, signal)` of pipe options.
fn pipe_options(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<(bool, bool, bool, RootId), RootedError> {
    let options = dictionary(c, args, index, "options", Dictionary::ObjectsAndNull)?;
    let prevent_abort = optional(c, options, PREVENT_ABORT)?;
    let prevent_cancel = optional(c, options, PREVENT_CANCEL)?;
    let prevent_close = optional(c, options, PREVENT_CLOSE)?;
    let signal = optional(c, options, SIGNAL)?;
    validate_signal(c, signal)?;
    let prevent_abort = c.truthy_rooted(prevent_abort)?;
    let prevent_cancel = c.truthy_rooted(prevent_cancel)?;
    let prevent_close = c.truthy_rooted(prevent_close)?;
    Ok((prevent_close, prevent_abort, prevent_cancel, signal))
}

pub(crate) fn pipe_through(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let source = this_state(c, this, Brand::ReadableStream)?;
    let transform = arg(c, args, 0);
    let readable = optional(c, transform, "readable")?;
    if !super::is(c, readable, Brand::ReadableStream)? {
        return Err(throw(c, WebError::ArgType("transform.readable", "ReadableStream", readable)));
    }
    let writable = optional(c, transform, "writable")?;
    let Some(dest) = state_of(c, writable, Brand::WritableStream)? else {
        return Err(throw(c, WebError::ArgType("transform.writable", "WritableStream", writable)));
    };
    let (prevent_close, prevent_abort, prevent_cancel, signal) = pipe_options(c, args, 1)?;
    if readable::is_locked(c, source)? {
        return Err(throw(c, WebError::InvalidState("The ReadableStream is locked")));
    }
    if writable::is_locked(c, dest)? {
        return Err(throw(c, WebError::InvalidState("The WritableStream is locked")));
    }
    let promise = pipe(c, source, dest, prevent_close, prevent_abort, prevent_cancel, signal)?;
    mark_handled(c, promise)?;
    Ok(readable)
}

pub(crate) fn pipe_to(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    match super::attempt(pipe_to_steps(c, this, args))? {
        Ok(promise) => Ok(promise),
        Err(error) => rejected(c, error),
    }
}

fn pipe_to_steps(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let source = this_state(c, this, Brand::ReadableStream)?;
    let destination = arg(c, args, 0);
    let Some(dest) = state_of(c, destination, Brand::WritableStream)? else {
        return Err(throw(c, WebError::ArgType("transform.writable", "WritableStream", destination)));
    };
    let (prevent_close, prevent_abort, prevent_cancel, signal) = pipe_options(c, args, 1)?;
    if readable::is_locked(c, source)? {
        return Err(throw(c, WebError::InvalidState("The ReadableStream is locked")));
    }
    if writable::is_locked(c, dest)? {
        return Err(throw(c, WebError::InvalidState("The WritableStream is locked")));
    }
    pipe(c, source, dest, prevent_close, prevent_abort, prevent_cancel, signal)
}

/// `readableStreamPipeTo(source, dest, preventClose, preventAbort, preventCancel, signal)`.
pub(crate) fn pipe(
    c: &mut Context<'_>,
    source: RootId,
    dest: RootId,
    prevent_close: bool,
    prevent_abort: bool,
    prevent_cancel: bool,
    signal: RootId,
) -> Outcome {
    let reader = match super::attempt(readable::default_reader(c, source))? {
        Ok(reader) => slots(c, reader)?,
        Err(error) => return rejected(c, error),
    };
    let writer = match super::attempt(writable::writer(c, dest))? {
        Ok(writer) => writer,
        Err(error) => return rejected(c, error),
    };
    put_flag(c, source, readable::field::DISTURBED, true)?;
    let promise = deferred(c)?;
    let current_write = resolved_undefined(c)?;
    let pipe = record(c, &[SOURCE, DEST, READER, WRITER, SIGNAL, PROMISE, CURRENT_WRITE], &[source, dest, reader, writer, signal, promise, current_write])?;
    for (name, value) in [
        (PREVENT_CLOSE, prevent_close),
        (PREVENT_ABORT, prevent_abort),
        (PREVENT_CANCEL, prevent_cancel),
        (SHUTTING_DOWN, false),
    ] {
        put_flag(c, pipe, name, value)?;
    }
    if !is_undefined(c, signal) {
        if crate::modules::abort_controller_shared_vm::aborted(c, signal)? {
            abort_algorithm(c, pipe)?;
            return Ok(promise);
        }
        let listener = function(c, Member::Abort, "", &[pipe])?;
        let options = super::literal(c, &[], &[])?;
        let once = c.boolean(true);
        put(c, options, "once", once)?;
        let kind = c.string_rooted("abort");
        let add = get(c, signal, "addEventListener")?;
        c.call_rooted(add, signal, &[kind, listener, options])?;
        put(c, pipe, LISTENER, listener)?;
    }
    let run = run(c, pipe)?;
    mark_handled(c, run)?;
    let reader_closed = get(c, reader, CLOSED)?;
    let source_errored = readable::state(c, source)? == readable::State::Errored;
    watch_errored(c, source, reader_closed, Member::SourceErrored, pipe, source_errored)?;
    let writer_closed = get(c, writer, CLOSED)?;
    let dest_errored = writable::state(c, dest)? == writable::State::Errored;
    watch_errored(c, dest, writer_closed, Member::DestErrored, pipe, dest_errored)?;
    if readable::state(c, source)? == readable::State::Closed {
        source_closed(c, pipe)?;
    } else {
        upon(c, reader_closed, Some(Member::SourceClosed), Some(Member::Nop), &[pipe])?;
    }
    if writable::close_queued_or_in_flight(c, dest)? || writable::state(c, dest)? == writable::State::Closed {
        let error = create_error(c, WebError::InvalidState("Destination WritableStream is closed"))?;
        if !flag(c, pipe, PREVENT_CANCEL)? {
            shutdown_with_action(c, pipe, Action::CancelSource, error, true, error)?;
        } else {
            shutdown(c, pipe, true, error)?;
        }
    }
    Ok(promise)
}

fn watch_errored(c: &mut Context<'_>, stream: RootId, promise: RootId, action: Member, pipe: RootId, errored: bool) -> Done {
    if errored {
        let error = get(c, stream, STORED_ERROR)?;
        return errored_action(c, pipe, action, error);
    }
    upon(c, promise, None, Some(action), &[pipe]).map(drop)
}

fn errored_action(c: &mut Context<'_>, pipe: RootId, action: Member, error: RootId) -> Done {
    let (prevented, shutdown_action) = if action == Member::SourceErrored {
        (flag(c, pipe, PREVENT_ABORT)?, Action::AbortDest)
    } else {
        (flag(c, pipe, PREVENT_CANCEL)?, Action::CancelSource)
    };
    if !prevented {
        return shutdown_with_action(c, pipe, shutdown_action, error, true, error);
    }
    shutdown(c, pipe, true, error)
}

fn source_closed(c: &mut Context<'_>, pipe: RootId) -> Done {
    if !flag(c, pipe, PREVENT_CLOSE)? {
        let undefined = c.undefined();
        return shutdown_with_action(c, pipe, Action::CloseDest, undefined, false, undefined);
    }
    let undefined = c.undefined();
    shutdown(c, pipe, false, undefined)
}

fn abort_algorithm(c: &mut Context<'_>, pipe: RootId) -> Done {
    let signal = get(c, pipe, SIGNAL)?;
    let error = get(c, signal, "reason")?;
    shutdown_with_action(c, pipe, Action::Signal, error, true, error)
}

/// `run()`: `while (!await step());`.
fn run(c: &mut Context<'_>, pipe: RootId) -> Outcome {
    let promise = deferred(c)?;
    run_step(c, pipe, promise)?;
    Ok(promise)
}

fn run_step(c: &mut Context<'_>, pipe: RootId, run: RootId) -> Done {
    let step = step(c, pipe)?;
    upon(c, step, Some(Member::Stepped), Some(Member::RunFailed), &[pipe, run]).map(drop)
}

/// `step()`, an async function.
fn step(c: &mut Context<'_>, pipe: RootId) -> Outcome {
    let promise = deferred(c)?;
    if flag(c, pipe, SHUTTING_DOWN)? {
        let done = c.boolean(true);
        c.resolve_promise_rooted(promise, done)?;
        return Ok(promise);
    }
    let writer = get(c, pipe, WRITER)?;
    let ready = get(c, writer, READY)?;
    let ready = resolved(c, ready)?;
    upon(c, ready, Some(Member::StepReady), Some(Member::StepFailed), &[pipe, promise])?;
    Ok(promise)
}

/// The pipe's read request's chunk steps.
pub(crate) fn read_chunk(c: &mut Context<'_>, request: RootId, chunk: RootId) -> Done {
    let pipe = get(c, request, request::PIPE)?;
    let writer = get(c, request, request::WRITER)?;
    let write = writable::writer_write(c, writer, chunk)?;
    put(c, pipe, CURRENT_WRITE, write)?;
    mark_handled(c, write)?;
    let promise = get(c, request, PROMISE)?;
    let done = c.boolean(false);
    settle(c, promise, done)
}

/// `waitForCurrentWrite()`, an async function.
fn wait_for_current_write(c: &mut Context<'_>, pipe: RootId) -> Outcome {
    let promise = deferred(c)?;
    let write = get(c, pipe, CURRENT_WRITE)?;
    let awaited = resolved(c, write)?;
    upon(c, awaited, Some(Member::WaitWritten), Some(Member::WaitFailed), &[pipe, promise, write])?;
    Ok(promise)
}

fn shutdown_with_action(c: &mut Context<'_>, pipe: RootId, action: Action, action_error: RootId, rejected: bool, original: RootId) -> Done {
    if flag(c, pipe, SHUTTING_DOWN)? {
        return Ok(());
    }
    put_flag(c, pipe, SHUTTING_DOWN, true)?;
    let action = c.number(action as usize as f64);
    let rejected = c.boolean(rejected);
    let dest = get(c, pipe, DEST)?;
    if writable::state(c, dest)? == writable::State::Writable && !writable::close_queued_or_in_flight(c, dest)? {
        let waited = wait_for_current_write(c, pipe)?;
        upon(c, waited, Some(Member::ShutdownWritten), Some(Member::ShutdownFailed), &[pipe, action, action_error, rejected, original])?;
        return Ok(());
    }
    complete(c, pipe, action, action_error, rejected, original)
}

/// `complete()`: run the action, then finalize.
fn complete(c: &mut Context<'_>, pipe: RootId, action: RootId, action_error: RootId, rejected: RootId, original: RootId) -> Done {
    let action = Action::ALL[c.to_number(action)? as usize];
    let result = match super::attempt(perform(c, pipe, action, action_error))? {
        Ok(result) => result,
        Err(error) => super::rejected(c, error)?,
    };
    upon(c, result, Some(Member::ActionDone), Some(Member::ActionFailed), &[pipe, rejected, original]).map(drop)
}

fn perform(c: &mut Context<'_>, pipe: RootId, action: Action, error: RootId) -> Outcome {
    let source = get(c, pipe, SOURCE)?;
    let dest = get(c, pipe, DEST)?;
    match action {
        Action::AbortDest => writable::abort(c, dest, error),
        Action::CancelSource => readable::cancel(c, source, error),
        Action::CloseDest => {
            let writer = get(c, pipe, WRITER)?;
            writable::close_with_error_propagation(c, writer)
        }
        Action::Signal => {
            let mut actions = Vec::new();
            if !flag(c, pipe, PREVENT_ABORT)? {
                actions.push(if writable::state(c, dest)? == writable::State::Writable {
                    writable::abort(c, dest, error)?
                } else {
                    resolved_undefined(c)?
                });
            }
            if !flag(c, pipe, PREVENT_CANCEL)? {
                actions.push(if readable::state(c, source)? == readable::State::Readable {
                    readable::cancel(c, source, error)?
                } else {
                    resolved_undefined(c)?
                });
            }
            all(c, &actions)
        }
    }
}

/// `Promise.all(promises)` over native promises, fulfilled with `undefined`.
pub(crate) fn all(c: &mut Context<'_>, promises: &[RootId]) -> Outcome {
    let promise = deferred(c)?;
    if promises.is_empty() {
        settle_undefined(c, promise)?;
        return Ok(promise);
    }
    let remaining = c.number(promises.len() as f64);
    let aggregate = record(c, &[PROMISE, REMAINING], &[promise, remaining])?;
    for &each in promises {
        upon(c, each, Some(Member::AllFulfilled), Some(Member::AllRejected), &[aggregate])?;
    }
    Ok(promise)
}

fn shutdown(c: &mut Context<'_>, pipe: RootId, rejected: bool, error: RootId) -> Done {
    if flag(c, pipe, SHUTTING_DOWN)? {
        return Ok(());
    }
    put_flag(c, pipe, SHUTTING_DOWN, true)?;
    let dest = get(c, pipe, DEST)?;
    if writable::state(c, dest)? == writable::State::Writable && !writable::close_queued_or_in_flight(c, dest)? {
        let waited = wait_for_current_write(c, pipe)?;
        let rejected = c.boolean(rejected);
        let action = c.undefined();
        upon(c, waited, Some(Member::ShutdownWritten), Some(Member::ShutdownFailed), &[pipe, action, action, rejected, error])?;
        return Ok(());
    }
    finalize(c, pipe, rejected, error)
}

fn finalize(c: &mut Context<'_>, pipe: RootId, rejected: bool, error: RootId) -> Done {
    let writer = get(c, pipe, WRITER)?;
    writable::writer_release(c, writer)?;
    let reader = get(c, pipe, READER)?;
    readable::generic_release(c, reader)?;
    let listener = get(c, pipe, LISTENER)?;
    if !is_undefined(c, listener) {
        let signal = get(c, pipe, SIGNAL)?;
        let kind = c.string_rooted("abort");
        let remove = get(c, signal, "removeEventListener")?;
        c.call_rooted(remove, signal, &[kind, listener])?;
    }
    let promise = get(c, pipe, PROMISE)?;
    if rejected {
        fail(c, promise, error)
    } else {
        settle_undefined(c, promise)
    }
}

pub(crate) fn member(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    let value = arg(c, args, 0);
    match member {
        Member::StepReady => {
            let [pipe, promise] = captured(c)?;
            let read = deferred(c)?;
            let reader = get(c, pipe, READER)?;
            let writer = get(c, pipe, WRITER)?;
            let request = new_request(c, ReadSteps::Pipe, &[PROMISE, request::WRITER, request::PIPE, crate::modules::keys::KIND], &[read, writer, pipe])?;
            readable::default_reader_read(c, reader, request)?;
            c.resolve_promise_rooted(promise, read)?;
        }
        Member::StepFailed | Member::WaitFailed | Member::RunFailed => {
            let [_, promise] = captured(c)?;
            fail(c, promise, value)?;
        }
        Member::Stepped => {
            let [pipe, run] = captured(c)?;
            if c.truthy_rooted(value)? {
                settle_undefined(c, run)?;
            } else {
                run_step(c, pipe, run)?;
            }
        }
        Member::WaitWritten => {
            let [pipe, promise, write] = captured(c)?;
            let current = get(c, pipe, CURRENT_WRITE)?;
            if super::same(c, write, current)? {
                settle_undefined(c, promise)?;
            } else {
                let again = wait_for_current_write(c, pipe)?;
                upon(c, again, Some(Member::WaitSettled), Some(Member::WaitFailed), &[pipe, promise])?;
            }
        }
        Member::WaitSettled => {
            let [_, promise] = captured(c)?;
            settle_undefined(c, promise)?;
        }
        Member::ShutdownWritten => {
            let [pipe, action, action_error, rejected, original] = captured(c)?;
            if is_undefined(c, action) {
                let rejected = c.truthy_rooted(rejected)?;
                finalize(c, pipe, rejected, original)?;
            } else {
                complete(c, pipe, action, action_error, rejected, original)?;
            }
        }
        Member::ShutdownFailed | Member::ActionFailed => {
            let [pipe] = captured(c)?;
            finalize(c, pipe, true, value)?;
        }
        Member::ActionDone => {
            let [pipe, rejected, original] = captured(c)?;
            let rejected = c.truthy_rooted(rejected)?;
            finalize(c, pipe, rejected, original)?;
        }
        Member::SourceErrored | Member::DestErrored => {
            let [pipe] = captured(c)?;
            errored_action(c, pipe, member, value)?;
        }
        Member::SourceClosed => {
            let [pipe] = captured(c)?;
            source_closed(c, pipe)?;
        }
        Member::Nop => {}
        Member::Abort => {
            let [pipe] = captured(c)?;
            abort_algorithm(c, pipe)?;
        }
        Member::AllFulfilled => {
            let [aggregate] = captured(c)?;
            let remaining = super::num(c, aggregate, REMAINING)? - 1.0;
            super::put_num(c, aggregate, REMAINING, remaining)?;
            if remaining == 0.0 {
                let promise = get(c, aggregate, PROMISE)?;
                settle_undefined(c, promise)?;
            }
        }
        Member::AllRejected => {
            let [aggregate] = captured(c)?;
            let promise = get(c, aggregate, PROMISE)?;
            fail(c, promise, value)?;
        }
    }
    Ok(c.undefined())
}
