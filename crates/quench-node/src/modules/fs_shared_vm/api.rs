//! The three faces of each fs operation: the sync API returns its reply or throws, the
//! callback API calls back with it, and the promise API settles with it. Each public
//! function is one host function whose data names its operation and face.

use super::ops::Op;
use crate::modules::stream_shared_vm::{arg, attempt, Context, Outcome};
use quench_runtime::{RootId, RootedError};

/// How a public function delivers an operation's reply.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flavor {
    /// `fs.statSync(...)`: returned, or thrown.
    Sync,
    /// `fs.stat(..., callback)`: `callback(error, ...values)` once the operation completes.
    Callback,
    /// `fs.promises.stat(...)`: a promise settled once it completes.
    Promise,
    /// `filehandle.stat(...)`: a promise, over the handle's descriptor.
    Handle,
}

impl Flavor {
    const ALL: [Self; 4] = [Self::Sync, Self::Callback, Self::Promise, Self::Handle];
}

/// The data a read or write moves, and the names its promise result gives it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transfer {
    Read,
    Write,
    Readv,
    Writev,
}

impl Transfer {
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Read => ("bytesRead", "buffer"),
            Self::Write => ("bytesWritten", "buffer"),
            Self::Readv => ("bytesRead", "buffers"),
            Self::Writev => ("bytesWritten", "buffers"),
        }
    }
}

/// What an operation produced.
pub(crate) enum Reply {
    Value(RootId),
    /// `exists`: its callback takes the answer alone.
    Exists(bool),
    /// `open`: a number, or a `FileHandle` for the promise API.
    Descriptor(i32),
    /// A read or write: the bytes moved, and the buffer (or buffers, or string) involved.
    Transfer { count: usize, data: RootId, kind: Transfer },
    /// A value an `AbortSignal` still cancels until it is delivered: Node's reads and writes
    /// check the signal between their asynchronous steps.
    Abortable { value: RootId, signal: RootId },
}

/// An operation's call: `Err` is an argument error, which the sync and callback APIs throw;
/// `Ok` is the operation's own outcome, its reply or the error it reports.
pub(crate) type Call = Result<Result<Reply, RootedError>, RootedError>;

fn encode(op: Op, flavor: Flavor) -> f64 {
    let op = Op::ALL.iter().position(|candidate| *candidate == op).expect("every operation is listed");
    let flavor = Flavor::ALL.iter().position(|candidate| *candidate == flavor).expect("every flavor is listed");
    (op * Flavor::ALL.len() + flavor) as f64
}

pub(crate) fn decode(c: &mut Context<'_>) -> Result<(Op, Flavor), RootedError> {
    let data = c.host_function_data()?;
    let index = c.to_number(data)? as usize;
    Ok((Op::ALL[index / Flavor::ALL.len()], Flavor::ALL[index % Flavor::ALL.len()]))
}

/// The public function `name` (of `length` parameters) running `op` as `flavor` delivers it.
pub(crate) fn function(c: &mut Context<'_>, op: Op, flavor: Flavor, name: &str, length: usize) -> Outcome {
    let operation = if flavor == Flavor::Handle { "fsHandleCall" } else { "fsCall" };
    let data = c.number(encode(op, flavor));
    let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
    let key = c.string_rooted("length");
    let length = c.number(length as f64);
    c.define_data_property_rooted(function, key, length, false, false, true)?;
    Ok(function)
}

/// A public `fs` function: the data names its operation and flavor.
pub(crate) fn call(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (op, flavor) = decode(c)?;
    run(c, op, flavor, args)
}

pub(crate) fn run(c: &mut Context<'_>, op: Op, flavor: Flavor, args: &[RootId]) -> Outcome {
    match flavor {
        Flavor::Sync => {
            let reply = op.call(c, args, flavor)??;
            project_sync(c, reply)
        }
        Flavor::Callback => {
            let (args, callback) = split_callback(c, op, args)?;
            let outcome = op.call(c, args, flavor)?;
            let (values, signal) = match attempt(outcome)? {
                Ok(reply) => callback_values(c, reply),
                Err(error) => (vec![error], None),
            };
            if let Some(callback) = callback {
                let callback = guarded(c, callback, signal, false)?;
                super::streams::complete(c, callback, &values)?;
            }
            Ok(c.undefined())
        }
        Flavor::Promise | Flavor::Handle => {
            let promise = c.pending_promise_rooted()?;
            let outcome = match op.call(c, args, flavor) {
                Ok(outcome) => outcome,
                Err(error) => Err(error),
            };
            let (settled, signal) = match attempt(outcome)? {
                Ok(Reply::Abortable { value, signal }) => (Ok(value), Some(signal)),
                Ok(reply) => (attempt(project_promise(c, reply))?, None),
                Err(error) => (Err(error), None),
            };
            settle_guarded(c, promise, settled, signal)?;
            Ok(promise)
        }
    }
}

/// Settle `promise` from the immediate queue, as a completion arriving after this turn.
pub(crate) fn settle_later(c: &mut Context<'_>, promise: RootId, outcome: Result<RootId, RootId>) -> Result<(), RootedError> {
    settle_guarded(c, promise, outcome, None)
}

fn settle_guarded(c: &mut Context<'_>, promise: RootId, outcome: Result<RootId, RootId>, signal: Option<RootId>) -> Result<(), RootedError> {
    let settle = c.host_function_with_data(crate::host::shared_vm::operation("fsSettle"), promise)?;
    let settle = guarded(c, settle, signal, true)?;
    let (rejected, value) = match outcome {
        Ok(value) => (false, value),
        Err(error) => (true, error),
    };
    let rejected = c.boolean(rejected);
    super::streams::complete(c, settle, &[rejected, value])
}

/// `deliver`, or a delivery that reports an `AbortError` instead once `signal` aborted:
/// as `(error)` to a callback, as `(true, error)` to a promise's settlement.
fn guarded(c: &mut Context<'_>, deliver: RootId, signal: Option<RootId>, settles: bool) -> Outcome {
    let Some(signal) = signal else {
        return Ok(deliver);
    };
    let settles = c.boolean(settles);
    crate::modules::stream_shared_vm::closure(c, "fsAbortGuard", "", &[deliver, signal, settles])
}

/// The delivery `guarded` wraps.
pub(crate) fn abort_guard(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [deliver, signal, settles] = crate::modules::stream_shared_vm::captures::<3>(c)?;
    let aborted = crate::modules::stream_shared_vm::get(c, signal, "aborted")?;
    let undefined = c.undefined();
    if !c.truthy_rooted(aborted)? {
        return c.call_rooted(deliver, undefined, args);
    }
    let failure = super::failure::aborted(c, signal);
    let Some(error) = failure.exception else {
        return Err(failure);
    };
    if c.truthy_rooted(settles)? {
        let rejected = c.boolean(true);
        return c.call_rooted(deliver, undefined, &[rejected, error]);
    }
    c.call_rooted(deliver, undefined, &[error])
}

/// The completion `settle_later` queued: `(rejected, value)`.
pub(crate) fn settle(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let promise = c.host_function_data()?;
    let rejected = arg(c, args, 0);
    let value = arg(c, args, 1);
    if c.truthy_rooted(rejected)? {
        c.reject_promise_rooted(promise, value)?;
    } else {
        c.resolve_promise_rooted(promise, value)?;
    }
    Ok(c.undefined())
}

/// The callback of a callback-API call and the arguments before it: the earliest function
/// at or after the operation's required parameters (`typeof options === 'function'`), else
/// the parameter after the last.
fn split_callback<'a>(c: &mut Context<'_>, op: Op, args: &'a [RootId]) -> Result<(&'a [RootId], Option<RootId>), RootedError> {
    let (required, declared) = op.parameters();
    for index in required..=declared.min(args.len().saturating_sub(1)) {
        if c.is_callable_rooted(args[index])? {
            return Ok((&args[..index], Some(args[index])));
        }
    }
    let callback = arg(c, args, declared);
    // `fs.close(fd)` closes without a callback.
    if op == Op::Close && crate::modules::stream_shared_vm::is_undefined(c, callback) {
        return Ok((&args[..args.len().min(declared)], None));
    }
    let callback = super::args::function(c, callback, "cb")?;
    Ok((&args[..args.len().min(declared)], Some(callback)))
}

fn project_sync(c: &mut Context<'_>, reply: Reply) -> Outcome {
    Ok(match reply {
        Reply::Value(value) => value,
        Reply::Exists(exists) => c.boolean(exists),
        Reply::Descriptor(fd) => c.number(f64::from(fd)),
        Reply::Transfer { count, .. } => c.number(count as f64),
        Reply::Abortable { value, .. } => value,
    })
}

/// A reply as a callback's arguments: `(null)` for nothing, `(null, value)`, `(exists)` or
/// `(null, count, data)`, with the signal that may still cancel it.
fn callback_values(c: &mut Context<'_>, reply: Reply) -> (Vec<RootId>, Option<RootId>) {
    let null = c.null();
    let values = match reply {
        Reply::Abortable { value, signal } => {
            let (values, _) = callback_values(c, Reply::Value(value));
            return (values, Some(signal));
        }
        Reply::Value(value) if c.rooted_value(value).is_some_and(|value| value.is_undefined()) => vec![null],
        Reply::Value(value) => vec![null, value],
        Reply::Exists(exists) => vec![c.boolean(exists)],
        Reply::Descriptor(fd) => vec![null, c.number(f64::from(fd))],
        Reply::Transfer { count, data, .. } => vec![null, c.number(count as f64), data],
    };
    (values, None)
}

/// A reply as a promise's value: a descriptor becomes a `FileHandle`, a transfer an object
/// `{ __proto__: null, bytesRead, buffer }`.
fn project_promise(c: &mut Context<'_>, reply: Reply) -> Outcome {
    match reply {
        Reply::Value(value) | Reply::Abortable { value, .. } => Ok(value),
        Reply::Exists(exists) => Ok(c.boolean(exists)),
        Reply::Descriptor(fd) => super::file_handle::create(c, fd),
        Reply::Transfer { count, data, kind } => {
            let (count_name, data_name) = kind.names();
            let result = c.null_object_rooted()?;
            let count = c.number(count as f64);
            crate::modules::stream_shared_vm::put(c, result, count_name, count)?;
            crate::modules::stream_shared_vm::put(c, result, data_name, data)?;
            Ok(result)
        }
    }
}
