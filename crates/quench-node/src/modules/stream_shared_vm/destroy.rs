//! `internal/streams/destroy`: destruction, construction and the error-or-destroy policy a
//! stream's readable and writable halves share.

use super::errors::{self, StreamError};
use super::state::{self, bits, change, raw_bits};
use super::{
    arg, attempt, call_method, captures, closure, emit, emit_key, get, get_slot, intrinsics, is_nullish,
    put, set_capture, tick, utils, Context, Outcome, Slot, Tick,
};
use quench_runtime::{RootId, RootedError};

/// A stream's `_readableState` and `_writableState`, each when present (truthy).
pub(crate) struct Halves {
    pub(crate) readable: Option<RootId>,
    pub(crate) writable: Option<RootId>,
}

impl Halves {
    pub(crate) fn of(c: &mut Context<'_>, stream: RootId) -> Result<Self, RootedError> {
        let readable = get(c, stream, "_readableState")?;
        let writable = get(c, stream, "_writableState")?;
        Ok(Self {
            readable: c.truthy_rooted(readable)?.then_some(readable),
            writable: c.truthy_rooted(writable)?.then_some(writable),
        })
    }

    fn each(&self) -> impl Iterator<Item = RootId> {
        self.writable.into_iter().chain(self.readable)
    }

    /// Whether either present half has any of `mask`.
    pub(crate) fn any(&self, c: &mut Context<'_>, mask: u32) -> Result<bool, RootedError> {
        for half in self.each() {
            if bits(c, half)? & mask != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn set(&self, c: &mut Context<'_>, mask: u32) -> Result<(), RootedError> {
        for half in self.each() {
            change(c, half, mask, 0)?;
        }
        Ok(())
    }

    /// `w || r`.
    fn primary(&self) -> Option<RootId> {
        self.writable.or(self.readable)
    }
}

/// `checkError(err, w, r)`: record `error` on each half not yet errored.
fn check_error(c: &mut Context<'_>, error: RootId, halves: &Halves) -> Result<(), RootedError> {
    if !c.truthy_rooted(error)? {
        return Ok(());
    }
    get(c, error, "stack")?;
    for half in halves.each() {
        let errored = get(c, half, "errored")?;
        if !c.truthy_rooted(errored)? {
            put(c, half, "errored", error)?;
        }
    }
    Ok(())
}

/// `destroy(err, cb)`, the prototype method.
pub(crate) fn destroy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    destroy_with(c, this, error, callback)?;
    Ok(this)
}

pub(crate) fn destroy_with(c: &mut Context<'_>, stream: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    let halves = Halves::of(c, stream)?;
    if halves.any(c, state::DESTROYED)? {
        if c.is_callable_rooted(callback)? {
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[])?;
        }
        return Ok(());
    }
    check_error(c, error, &halves)?;
    halves.set(c, state::DESTROYED)?;
    let constructed = match halves.primary() {
        Some(primary) => bits(c, primary)? & state::CONSTRUCTED != 0,
        None => true,
    };
    if constructed {
        return run_destroy(c, stream, error, callback);
    }
    let after = closure(c, "streamDestroyAfterConstruct", "", &[error, callback])?;
    let key = intrinsics(c)?.slot(Slot::Destroy);
    call_method(c, stream, "once", &[key, after])?;
    Ok(())
}

/// The `kDestroy` listener a stream destroyed while constructing waits with.
pub(crate) fn destroy_after_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let [error, callback] = captures(c)?;
    let construct_error = arg(c, args, 0);
    let error = errors::aggregate_two(c, construct_error, error)?;
    run_destroy(c, this, error, callback)?;
    Ok(c.undefined())
}

/// `_destroy(self, err, cb)`.
fn run_destroy(c: &mut Context<'_>, stream: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    let no = c.boolean(false);
    let on_destroy = closure(c, "streamOnDestroy", "onDestroy", &[stream, callback, no])?;
    let error = if c.truthy_rooted(error)? { error } else { c.null() };
    let result = call_method(c, stream, "_destroy", &[error, on_destroy]);
    if let Err(thrown) = attempt(result)? {
        let undefined = c.undefined();
        c.call_rooted(on_destroy, undefined, &[thrown])?;
    }
    Ok(())
}

/// `onDestroy(err)`: close both halves once, call back, then emit `error` and `close`.
pub(crate) fn on_destroy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, callback, called] = captures(c)?;
    if c.truthy_rooted(called)? {
        return Ok(c.undefined());
    }
    let yes = c.boolean(true);
    set_capture(c, 2, yes)?;
    let error = arg(c, args, 0);
    let halves = Halves::of(c, stream)?;
    check_error(c, error, &halves)?;
    halves.set(c, state::CLOSED)?;
    if c.is_callable_rooted(callback)? {
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[error])?;
    }
    if c.truthy_rooted(error)? {
        tick(c, Tick::EmitErrorClose, &[stream, error])?;
    } else {
        tick(c, Tick::EmitClose, &[stream])?;
    }
    Ok(c.undefined())
}

pub(crate) fn emit_error_close_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, error) = (arg(c, args, 0), arg(c, args, 1));
    emit_error_now(c, stream, error)?;
    emit_close_now(c, stream)?;
    Ok(c.undefined())
}

pub(crate) fn emit_close_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    emit_close_now(c, stream)?;
    Ok(c.undefined())
}

pub(crate) fn emit_error_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, error) = (arg(c, args, 0), arg(c, args, 1));
    emit_error_now(c, stream, error)?;
    Ok(c.undefined())
}

/// `emitCloseNT(self)`.
fn emit_close_now(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let halves = Halves::of(c, stream)?;
    halves.set(c, state::CLOSE_EMITTED)?;
    if halves.any(c, state::EMIT_CLOSE)? {
        emit(c, stream, "close", &[])?;
    }
    Ok(())
}

/// `emitErrorNT(self, err)`: once per stream.
fn emit_error_now(c: &mut Context<'_>, stream: RootId, error: RootId) -> Result<(), RootedError> {
    let halves = Halves::of(c, stream)?;
    if halves.any(c, state::ERROR_EMITTED)? {
        return Ok(());
    }
    halves.set(c, state::ERROR_EMITTED)?;
    emit(c, stream, "error", &[error])?;
    Ok(())
}

/// `undestroy()`: a stream's states as a freshly constructed stream's.
pub(crate) fn undestroy(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let halves = Halves::of(c, this)?;
    let no = c.boolean(false);
    let null = c.null();
    if let Some(r) = halves.readable {
        let readable = get(c, r, "readable")?;
        let closed_half = c.strict_equal_rooted(readable, no)?;
        change(c, r, state::CONSTRUCTED, state::CLOSED | state::CLOSE_EMITTED | state::DESTROYED | state::ERROR_EMITTED)?;
        put(c, r, "errored", null)?;
        let ended = state::readable::ENDED | state::readable::END_EMITTED;
        if closed_half { change(c, r, ended, state::readable::READING)? } else { change(c, r, 0, ended | state::readable::READING)? };
    }
    if let Some(w) = halves.writable {
        let writable = get(c, w, "writable")?;
        let closed_half = c.strict_equal_rooted(writable, no)?;
        let w_bits = state::writable::FINAL_CALLED | state::writable::PREFINISHED;
        change(c, w, state::CONSTRUCTED, state::DESTROYED | state::CLOSED | state::CLOSE_EMITTED | state::ERROR_EMITTED | w_bits)?;
        put(c, w, "errored", null)?;
        let ended = state::writable::ENDED | state::writable::ENDING | state::writable::FINISHED;
        if closed_half { change(c, w, ended, 0)? } else { change(c, w, 0, ended)? };
    }
    Ok(c.undefined())
}

/// Whether a state is destroyed: its bit, or `destroyed` for a state without `kState`.
fn half_destroyed(c: &mut Context<'_>, half: RootId) -> Result<bool, RootedError> {
    match raw_bits(c, half)? {
        Some(field) if field != 0 => Ok(field & state::DESTROYED != 0),
        _ => {
            let destroyed = get(c, half, "destroyed")?;
            c.truthy_rooted(destroyed)
        }
    }
}

/// `errorOrDestroy(stream, err, sync)`.
pub(crate) fn error_or_destroy(c: &mut Context<'_>, stream: RootId, error: RootId, sync: bool) -> Result<(), RootedError> {
    let halves = Halves::of(c, stream)?;
    for half in halves.each() {
        if half_destroyed(c, half)? {
            return Ok(());
        }
    }
    if halves.any(c, state::AUTO_DESTROY)? {
        call_method(c, stream, "destroy", &[error])?;
        return Ok(());
    }
    if !c.truthy_rooted(error)? {
        return Ok(());
    }
    get(c, error, "stack")?;
    for half in halves.each() {
        if bits(c, half)? & state::ERRORED == 0 {
            put(c, half, "errored", error)?;
        }
    }
    if sync {
        tick(c, Tick::EmitError, &[stream, error])
    } else {
        emit_error_now(c, stream, error)
    }
}

/// `errorOrDestroy(stream, err, sync)`, as the destroy module exports it.
pub(crate) fn error_or_destroy_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, error, sync) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let sync = c.truthy_rooted(sync)?;
    error_or_destroy(c, stream, error, sync)?;
    Ok(c.undefined())
}

/// `construct(stream, cb)`: run `_construct` on the next tick, holding both halves
/// unconstructed until it calls back.
pub(crate) fn construct(c: &mut Context<'_>, stream: RootId, callback: RootId) -> Result<(), RootedError> {
    let method = get(c, stream, "_construct")?;
    if !c.is_callable_rooted(method)? {
        return Ok(());
    }
    let halves = Halves::of(c, stream)?;
    for half in halves.each() {
        change(c, half, 0, state::CONSTRUCTED)?;
    }
    let key = intrinsics(c)?.slot(Slot::Construct);
    call_method(c, stream, "once", &[key, callback])?;
    let count = call_method(c, stream, "listenerCount", &[key])?;
    if c.to_number(count)? > 1.0 {
        return Ok(());
    }
    tick(c, Tick::Construct, &[stream])
}

/// `construct(stream, cb)`, as the destroy module exports it.
pub(crate) fn construct_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, callback) = (arg(c, args, 0), arg(c, args, 1));
    construct(c, stream, callback)?;
    Ok(c.undefined())
}

/// `constructNT(stream)`.
pub(crate) fn construct_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    let no = c.boolean(false);
    let on_construct = closure(c, "streamOnConstruct", "onConstruct", &[stream, no])?;
    let done = closure(c, "streamConstructDone", "", &[on_construct])?;
    let result = call_method(c, stream, "_construct", &[done]);
    if let Err(thrown) = attempt(result)? {
        super::next_tick(c, on_construct, &[thrown])?;
    }
    Ok(c.undefined())
}

/// The callback `_construct` receives: settles on the next tick.
pub(crate) fn construct_done(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [on_construct] = captures(c)?;
    let error = arg(c, args, 0);
    super::next_tick(c, on_construct, &[error])?;
    Ok(c.undefined())
}

/// `onConstruct(err)`.
pub(crate) fn on_construct(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, called] = captures(c)?;
    let error = arg(c, args, 0);
    if c.truthy_rooted(called)? {
        let error = if is_nullish(c, error) { errors::create(c, StreamError::MultipleCallback)? } else { error };
        error_or_destroy(c, stream, error, false)?;
        return Ok(c.undefined());
    }
    let yes = c.boolean(true);
    set_capture(c, 1, yes)?;
    let halves = Halves::of(c, stream)?;
    halves.set(c, state::CONSTRUCTED)?;
    let destroyed = match halves.primary() {
        Some(primary) => {
            let value = get(c, primary, "destroyed")?;
            c.truthy_rooted(value)?
        }
        None => false,
    };
    let intrinsics = intrinsics(c)?;
    if destroyed {
        emit_key(c, stream, intrinsics.slot(Slot::Destroy), &[error])?;
    } else if c.truthy_rooted(error)? {
        error_or_destroy(c, stream, error, true)?;
    } else {
        emit_key(c, stream, intrinsics.slot(Slot::Construct), &[])?;
    }
    Ok(c.undefined())
}

/// The construction callback a stream's constructor passes: each state's `kOnConstructed`.
pub(crate) fn on_constructed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [stream] = captures(c)?;
    let halves = Halves::of(c, stream)?;
    for half in [halves.readable, halves.writable].into_iter().flatten() {
        let method = get_slot(c, half, Slot::OnConstructed)?;
        c.call_rooted(method, half, &[stream])?;
    }
    Ok(c.undefined())
}

/// Runs `construct` with the `kOnConstructed` callback when the stream has `_construct`.
pub(crate) fn construct_if_defined(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let method = get(c, stream, "_construct")?;
    if is_nullish(c, method) {
        return Ok(());
    }
    let callback = closure(c, "streamOnConstructed", "", &[stream])?;
    construct(c, stream, callback)
}

/// How `destroyer` ends a stream-like value, in Node's order of preference.
enum Destruction {
    /// An incoming request: detach its socket, then destroy it.
    ServerRequest,
    /// An outgoing request (the stream or its `req`): abort it.
    Abort(RootId),
    Destroy,
    Close,
    /// A bare emitter: emit `error` (when failing) and `close` itself.
    Emit,
}

fn destruction(c: &mut Context<'_>, stream: RootId) -> Result<Destruction, RootedError> {
    if utils::is_server_request(c, stream)? {
        return Ok(Destruction::ServerRequest);
    }
    if utils::is_request(c, stream)? {
        return Ok(Destruction::Abort(stream));
    }
    let request = get(c, stream, "req")?;
    if utils::is_request(c, request)? {
        return Ok(Destruction::Abort(request));
    }
    let destroy = get(c, stream, "destroy")?;
    if c.is_callable_rooted(destroy)? {
        return Ok(Destruction::Destroy);
    }
    let close = get(c, stream, "close")?;
    Ok(if c.is_callable_rooted(close)? { Destruction::Close } else { Destruction::Emit })
}

/// `destroyer(stream, err)`: destroy any stream-like value the way it supports.
pub(crate) fn destroyer(c: &mut Context<'_>, stream: RootId, error: RootId) -> Result<(), RootedError> {
    if !c.truthy_rooted(stream)? || utils::is_destroyed(c, stream)? == Some(true) {
        return Ok(());
    }
    let mut error = error;
    if !c.truthy_rooted(error)? && utils::is_finished(c, stream, None)? != Some(true) {
        error = errors::abort_error(c, None)?;
    }
    match destruction(c, stream)? {
        Destruction::ServerRequest => {
            let null = c.null();
            put(c, stream, "socket", null)?;
            call_method(c, stream, "destroy", &[error])?;
        }
        Destruction::Abort(request) => {
            call_method(c, request, "abort", &[])?;
        }
        Destruction::Destroy => {
            call_method(c, stream, "destroy", &[error])?;
        }
        Destruction::Close => {
            call_method(c, stream, "close", &[])?;
        }
        Destruction::Emit if c.truthy_rooted(error)? => tick(c, Tick::EmitErrorCloseLegacy, &[stream, error])?,
        Destruction::Emit => tick(c, Tick::EmitCloseLegacy, &[stream])?,
    }
    let destroyed = get(c, stream, "destroyed")?;
    if !c.truthy_rooted(destroyed)? {
        let key = intrinsics(c)?.slot(Slot::IsDestroyed);
        let yes = c.boolean(true);
        c.set_property_rooted(stream, key, yes, stream)?;
    }
    Ok(())
}

/// `destroyer(stream, err)`, as `stream.destroy` exports it.
pub(crate) fn destroyer_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, error) = (arg(c, args, 0), arg(c, args, 1));
    destroyer(c, stream, error)?;
    Ok(c.undefined())
}

pub(crate) fn emit_close_legacy_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let stream = arg(c, args, 0);
    emit(c, stream, "close", &[])?;
    Ok(c.undefined())
}

pub(crate) fn emit_error_close_legacy_tick(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (stream, error) = (arg(c, args, 0), arg(c, args, 1));
    emit(c, stream, "error", &[error])?;
    tick(c, Tick::EmitCloseLegacy, &[stream])?;
    Ok(c.undefined())
}

/// `this.destroy(err)`: a stream's `captureRejectionSymbol` handler.
pub(crate) fn capture_rejection(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let error = arg(c, args, 0);
    call_method(c, this, "destroy", &[error])?;
    Ok(c.undefined())
}
