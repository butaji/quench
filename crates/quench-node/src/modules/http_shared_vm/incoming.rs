//! `IncomingMessage` (lib/_http_incoming.js): a `Readable` of a message's body that the
//! parser pushes into, with the head's fields and its headers folded lazily from
//! `rawHeaders`.

use super::{captured, current, function, get_slot, put_slot, Slot};
use crate::modules::http_protocol::NON_REPEATABLE_HEADERS;
use crate::modules::stream_shared_vm::{
    arg, call_method, emit, get, is_nullish, is_string, is_undefined, listener_count, next_tick, number, put,
    put_bool, truthy_member, Context, Outcome,
};
use quench_runtime::{RootId, RootedError};

crate::modules::host_members::members!(Member, "httpIncoming", [
    Construct,
    Connection,
    SetConnection,
    Headers,
    SetHeaders,
    HeadersDistinct,
    SetHeadersDistinct,
    Trailers,
    SetTrailers,
    TrailersDistinct,
    SetTrailersDistinct,
    SetTimeout,
    Read,
    Destroy,
    AddHeaderLines,
    AddHeaderLine,
    AddHeaderLineDistinct,
    Dump,
    /// `_destroy`'s wait for the socket it destroyed to finish.
    SocketFinished,
    /// `onError(self, error, cb)` on the next tick.
    DestroyDone,
]);

pub(crate) fn build(c: &mut Context<'_>) -> Outcome {
    let stream_module = crate::modules::stream_shared_vm::module(c)?;
    let readable = get(c, stream_module, "Readable")?;
    let class = super::constructor(c, Member::Construct, "IncomingMessage")?;
    super::inherit(c, class, readable)?;
    let prototype = get(c, class, "prototype")?;
    super::accessors(c, prototype, &[
        ("connection", Member::Connection, Some(Member::SetConnection)),
        ("headers", Member::Headers, Some(Member::SetHeaders)),
        ("headersDistinct", Member::HeadersDistinct, Some(Member::SetHeadersDistinct)),
        ("trailers", Member::Trailers, Some(Member::SetTrailers)),
        ("trailersDistinct", Member::TrailersDistinct, Some(Member::SetTrailersDistinct)),
    ])?;
    super::methods(c, prototype, &[
        ("setTimeout", Member::SetTimeout),
        ("_read", Member::Read),
        ("_destroy", Member::Destroy),
        ("_addHeaderLines", Member::AddHeaderLines),
        ("_addHeaderLine", Member::AddHeaderLine),
        ("_addHeaderLineDistinct", Member::AddHeaderLineDistinct),
        ("_dump", Member::Dump),
    ])?;
    Ok(class)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::Construct => {
            let socket = at(0);
            initialize(c, this, socket)?;
            Ok(c.undefined())
        }
        Member::Connection => get(c, this, "socket"),
        Member::SetConnection => {
            let value = at(0);
            put(c, this, "socket", value)?;
            Ok(c.undefined())
        }
        Member::Headers => folded(c, this, Fold::Headers),
        Member::HeadersDistinct => folded(c, this, Fold::HeadersDistinct),
        Member::Trailers => folded(c, this, Fold::Trailers),
        Member::TrailersDistinct => folded(c, this, Fold::TrailersDistinct),
        Member::SetHeaders => set_folded(c, this, Fold::Headers, args),
        Member::SetHeadersDistinct => set_folded(c, this, Fold::HeadersDistinct, args),
        Member::SetTrailers => set_folded(c, this, Fold::Trailers, args),
        Member::SetTrailersDistinct => set_folded(c, this, Fold::TrailersDistinct, args),
        Member::SetTimeout => {
            let (msecs, callback) = (at(0), at(1));
            if c.truthy_rooted(callback)? {
                crate::modules::stream_shared_vm::on(c, this, "timeout", callback)?;
            }
            let socket = get(c, this, "socket")?;
            call_method(c, socket, "setTimeout", &[msecs])?;
            Ok(this)
        }
        Member::Read => {
            if !truthy_member(c, this, "_consuming")? {
                let state = get(c, this, "_readableState")?;
                put_bool(c, state, "readingMore", false)?;
                put_bool(c, this, "_consuming", true)?;
            }
            let socket = get(c, this, "socket")?;
            if !is_nullish(c, socket) && truthy_member(c, socket, "readable")? {
                read_start(c, socket)?;
            }
            Ok(c.undefined())
        }
        Member::Destroy => {
            let (error, callback) = (at(0), at(1));
            destroy(c, this, error, callback)?;
            Ok(c.undefined())
        }
        Member::AddHeaderLines => {
            let (headers, count) = (at(0), at(1));
            add_header_lines(c, this, headers, count)?;
            Ok(c.undefined())
        }
        Member::AddHeaderLine => {
            let (field, value, destination) = (at(0), at(1), at(2));
            add_header_line(c, this, field, value, destination)?;
            Ok(c.undefined())
        }
        Member::AddHeaderLineDistinct => {
            let (field, value, destination) = (at(0), at(1), at(2));
            add_header_line_distinct(c, field, value, destination)?;
            Ok(c.undefined())
        }
        Member::Dump => {
            dump(c, this)?;
            Ok(c.undefined())
        }
        Member::SocketFinished => {
            let [message, error, callback, cleanup] = captured(c)?;
            let mut failure = at(0);
            if crate::modules::stream_shared_vm::errors::has_code(c, failure, "ERR_STREAM_PREMATURE_CLOSE")? {
                failure = c.null();
            }
            let cleanup = get(c, cleanup, "0")?;
            let undefined = c.undefined();
            c.call_rooted(cleanup, undefined, &[])?;
            let error = if c.truthy_rooted(failure)? { failure } else { error };
            let done = function(c, Member::DestroyDone, "onError", &[message, error, callback])?;
            next_tick(c, done, &[])?;
            Ok(c.undefined())
        }
        Member::DestroyDone => {
            let [message, error, callback] = captured(c)?;
            let undefined = c.undefined();
            if listener_count(c, message, "error")? == 0.0 {
                c.call_rooted(callback, undefined, &[])?;
            } else {
                c.call_rooted(callback, undefined, &[error])?;
            }
            Ok(c.undefined())
        }
    }
}

/// `IncomingMessage.call(this, socket)`: the fields Node's constructor assigns, in order.
pub(crate) fn initialize(c: &mut Context<'_>, this: RootId, socket: RootId) -> Result<(), RootedError> {
    let stream_module = crate::modules::stream_shared_vm::module(c)?;
    let readable = get(c, stream_module, "Readable")?;
    let options = if c.truthy_rooted(socket)? {
        let options = c.object_rooted()?;
        let mark = get(c, socket, "readableHighWaterMark")?;
        put(c, options, "highWaterMark", mark)?;
        options
    } else {
        c.undefined()
    };
    c.call_rooted(readable, this, &[options])?;
    let state = get(c, this, "_readableState")?;
    put_bool(c, state, "readingMore", true)?;
    put(c, this, "socket", socket)?;
    let null = c.null();
    for name in ["httpVersionMajor", "httpVersionMinor", "httpVersion"] {
        put(c, this, name, null)?;
    }
    put_bool(c, this, "complete", false)?;
    let zero = c.number(0.0);
    put_slot(c, this, Slot::Headers, null)?;
    put_slot(c, this, Slot::HeadersCount, zero)?;
    let raw = c.array_rooted(&[])?;
    put(c, this, "rawHeaders", raw)?;
    put_slot(c, this, Slot::Trailers, null)?;
    put_slot(c, this, Slot::TrailersCount, zero)?;
    let raw = c.array_rooted(&[])?;
    put(c, this, "rawTrailers", raw)?;
    put_bool(c, this, "joinDuplicateHeaders", false)?;
    put_bool(c, this, "aborted", false)?;
    put(c, this, "upgrade", null)?;
    let empty = c.string_rooted("");
    put(c, this, "url", empty)?;
    put(c, this, "method", null)?;
    put(c, this, "statusCode", null)?;
    put(c, this, "statusMessage", null)?;
    put(c, this, "client", socket)?;
    put_bool(c, this, "_consuming", false)?;
    put_bool(c, this, "_dumped", false)
}

/// `readStart(socket)`: let a socket the parser paused flow again.
pub(crate) fn read_start(c: &mut Context<'_>, socket: RootId) -> Result<(), RootedError> {
    if !is_nullish(c, socket) && !truthy_member(c, socket, "_paused")? && truthy_member(c, socket, "readable")? {
        call_method(c, socket, "resume", &[])?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Fold {
    Headers,
    HeadersDistinct,
    Trailers,
    TrailersDistinct,
}

impl Fold {
    fn slot(self) -> Slot {
        match self {
            Self::Headers => Slot::Headers,
            Self::HeadersDistinct => Slot::HeadersDistinct,
            Self::Trailers => Slot::Trailers,
            Self::TrailersDistinct => Slot::TrailersDistinct,
        }
    }

    fn source(self) -> (&'static str, Slot) {
        match self {
            Self::Headers | Self::HeadersDistinct => ("rawHeaders", Slot::HeadersCount),
            Self::Trailers | Self::TrailersDistinct => ("rawTrailers", Slot::TrailersCount),
        }
    }

    fn distinct(self) -> bool {
        matches!(self, Self::HeadersDistinct | Self::TrailersDistinct)
    }
}

fn set_folded(c: &mut Context<'_>, this: RootId, fold: Fold, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    put_slot(c, this, fold.slot(), value)?;
    Ok(c.undefined())
}

/// The `headers`-family getter: the raw lines folded once into the cached object.
fn folded(c: &mut Context<'_>, this: RootId, fold: Fold) -> Outcome {
    let cached = get_slot(c, this, fold.slot())?;
    if c.truthy_rooted(cached)? {
        return Ok(cached);
    }
    let destination = c.object_rooted()?;
    put_slot(c, this, fold.slot(), destination)?;
    let (source, count) = fold.source();
    let raw = get(c, this, source)?;
    let count = get_slot(c, this, count)?;
    let count = c.to_number(count)? as usize;
    let lines = c.array_like_values_rooted(raw)?;
    for pair in lines.chunks(2).take(count.div_ceil(2)) {
        let field = pair[0];
        let value = pair.get(1).copied().unwrap_or_else(|| c.undefined());
        if fold.distinct() {
            add_header_line_distinct(c, field, value, destination)?;
        } else {
            add_header_line(c, this, field, value, destination)?;
        }
    }
    Ok(destination)
}

pub(crate) fn add_header_lines(c: &mut Context<'_>, this: RootId, headers: RootId, count: RootId) -> Result<(), RootedError> {
    if is_nullish(c, headers) || number(c, headers, "length")? == 0.0 {
        return Ok(());
    }
    let (raw, count_slot, destination) = if truthy_member(c, this, "complete")? {
        ("rawTrailers", Slot::TrailersCount, Slot::Trailers)
    } else {
        ("rawHeaders", Slot::HeadersCount, Slot::Headers)
    };
    put(c, this, raw, headers)?;
    put_slot(c, this, count_slot, count)?;
    let destination = get_slot(c, this, destination)?;
    if c.truthy_rooted(destination)? {
        let count = c.to_number(count)? as usize;
        let lines = c.array_like_values_rooted(headers)?;
        for pair in lines.chunks(2).take(count.div_ceil(2)) {
            let value = pair.get(1).copied().unwrap_or_else(|| c.undefined());
            add_header_line(c, this, pair[0], value, destination)?;
        }
    }
    Ok(())
}

/// How repeats of a field combine (`matchKnownFields`).
enum Repeat {
    /// Only the first is kept (or joined, with `joinDuplicateHeaders`).
    First,
    /// `set-cookie`: an array of every value.
    Array,
    /// Joined with the separator.
    Join(&'static str),
}

fn repeat_of(field: &str) -> Repeat {
    const SET_COOKIE: &str = "set-cookie";
    const COOKIE: &str = "cookie";
    if field == SET_COOKIE {
        Repeat::Array
    } else if field == COOKIE {
        Repeat::Join("; ")
    } else if field == "content-length" || NON_REPEATABLE_HEADERS.contains(&field) {
        Repeat::First
    } else {
        Repeat::Join(", ")
    }
}

/// `_addHeaderLine(field, value, dest)`.
fn add_header_line(c: &mut Context<'_>, this: RootId, field: RootId, value: RootId, destination: RootId) -> Result<(), RootedError> {
    let field = c.to_string(field)?.to_lowercase();
    let current = get(c, destination, &field)?;
    match repeat_of(&field) {
        Repeat::Array => {
            if is_undefined(c, current) {
                let values = c.array_rooted(&[value])?;
                put(c, destination, &field, values)
            } else {
                call_method(c, current, "push", &[value]).map(drop)
            }
        }
        Repeat::Join(separator) => {
            if is_string(c, current) {
                let joined = join(c, current, separator, value)?;
                put(c, destination, &field, joined)
            } else {
                put(c, destination, &field, value)
            }
        }
        Repeat::First => {
            if truthy_member(c, this, "joinDuplicateHeaders")? && !is_undefined(c, current) {
                let joined = join(c, current, ", ", value)?;
                put(c, destination, &field, joined)
            } else if is_undefined(c, current) {
                put(c, destination, &field, value)
            } else {
                Ok(())
            }
        }
    }
}

fn join(c: &mut Context<'_>, current: RootId, separator: &str, value: RootId) -> Outcome {
    let mut units = c.to_string_code_units(current)?;
    units.extend(separator.encode_utf16());
    units.extend(c.to_string_code_units(value)?);
    Ok(c.string_units_rooted(&units))
}

/// `_addHeaderLineDistinct(field, value, dest)`.
fn add_header_line_distinct(c: &mut Context<'_>, field: RootId, value: RootId, destination: RootId) -> Result<(), RootedError> {
    let field = c.to_string(field)?.to_lowercase();
    let current = get(c, destination, &field)?;
    if c.truthy_rooted(current)? {
        call_method(c, current, "push", &[value]).map(drop)
    } else {
        let values = c.array_rooted(&[value])?;
        put(c, destination, &field, values)
    }
}

pub(crate) fn dump(c: &mut Context<'_>, this: RootId) -> Result<(), RootedError> {
    if truthy_member(c, this, "_dumped")? {
        return Ok(());
    }
    put_bool(c, this, "_dumped", true)?;
    let data = c.string_rooted("data");
    call_method(c, this, "removeAllListeners", &[data])?;
    call_method(c, this, "resume", &[]).map(drop)
}

fn destroy(c: &mut Context<'_>, this: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    if !truthy_member(c, this, "readableEnded")? || !truthy_member(c, this, "complete")? {
        put_bool(c, this, "aborted", true)?;
        emit(c, this, "aborted", &[])?;
    }
    let socket = get(c, this, "socket")?;
    if !is_nullish(c, socket) && !truthy_member(c, socket, "destroyed")? && truthy_member(c, this, "aborted")? {
        call_method(c, socket, "destroy", &[error])?;
        let cell = c.array_rooted(&[])?;
        let finished = function(c, Member::SocketFinished, "", &[this, error, callback, cell])?;
        let undefined = c.undefined();
        let cleanup = crate::modules::stream_shared_vm::eos::eos(c, socket, undefined, finished)?;
        put(c, cell, "0", cleanup)
    } else {
        let done = function(c, Member::DestroyDone, "onError", &[this, error, callback])?;
        next_tick(c, done, &[])
    }
}
