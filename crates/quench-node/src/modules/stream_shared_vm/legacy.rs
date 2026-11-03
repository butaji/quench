//! `internal/streams/legacy`: the `Stream` base class and its old-style `pipe`.

use super::{
    arg, call_method, captures, closure, define_methods, emit, get, intrinsics, listener_count, on,
    put, put_bool, remove_listener, Class, Context, Key, Outcome,
};
use quench_runtime::{RootId, RootedError};

/// `Stream(opts)`: `EventEmitter.call(this, opts)`.
pub(crate) fn constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    let emitter = intrinsics(c)?.class(Class::Emitter);
    c.call_rooted(emitter, this, &[options])?;
    Ok(c.undefined())
}

pub(crate) fn build(c: &mut Context<'_>, emitter: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation!("streamLegacy"), "Stream")?;
    let prototype = get(c, constructor, "prototype")?;
    let emitter_prototype = get(c, emitter, "prototype")?;
    c.set_prototype_rooted(prototype, emitter_prototype)?;
    c.set_prototype_rooted(constructor, emitter)?;
    define_methods(
        c,
        prototype,
        &[
            (Key::Name("pipe"), "streamLegacyPipe", ""),
            (Key::Name("eventNames"), "streamLegacyEventNames", "eventNames"),
        ],
    )?;
    Ok(constructor)
}

/// The listeners one legacy `pipe` installs, by name and operation.
const LISTENERS: &[(&str, &str)] = &[
    ("ondata", "streamLegacyOnData"),
    ("ondrain", "streamLegacyOnDrain"),
    ("onend", "streamLegacyOnEnd"),
    ("onclose", "streamLegacyOnClose"),
    ("onerror", "streamLegacyOnError"),
    ("cleanup", "streamLegacyCleanup"),
];

/// `Stream.prototype.pipe(dest, options)`.
pub(crate) fn pipe(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (dest, options) = (arg(c, args, 0), arg(c, args, 1));
    let record = c.null_object_rooted()?;
    put(c, record, "source", this)?;
    put(c, record, "dest", dest)?;
    put_bool(c, record, "didOnEnd", false)?;
    for &(name, operation) in LISTENERS {
        let listener = closure(c, operation, name, &[record])?;
        put(c, record, name, listener)?;
    }
    let listener = get(c, record, "ondata")?;
    on(c, this, "data", listener)?;
    let listener = get(c, record, "ondrain")?;
    on(c, dest, "drain", listener)?;
    let stdio = get(c, dest, "_isStdio")?;
    let ends = if c.truthy_rooted(options)? {
        let end = get(c, options, "end")?;
        let no = c.boolean(false);
        !c.strict_equal_rooted(end, no)?
    } else {
        true
    };
    if !c.truthy_rooted(stdio)? && ends {
        let listener = get(c, record, "onend")?;
        on(c, this, "end", listener)?;
        let listener = get(c, record, "onclose")?;
        on(c, this, "close", listener)?;
    }
    let listener = get(c, record, "onerror")?;
    let error = c.string_rooted("error");
    prepend_listener(c, this, error, listener)?;
    prepend_listener(c, dest, error, listener)?;
    let cleanup = get(c, record, "cleanup")?;
    on(c, this, "end", cleanup)?;
    on(c, this, "close", cleanup)?;
    on(c, dest, "close", cleanup)?;
    emit(c, dest, "pipe", &[this])?;
    Ok(dest)
}

pub(crate) fn on_data(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let chunk = arg(c, args, 0);
    let source = get(c, record, "source")?;
    let dest = get(c, record, "dest")?;
    let writable = get(c, dest, "writable")?;
    if c.truthy_rooted(writable)? {
        let written = call_method(c, dest, "write", &[chunk])?;
        let no = c.boolean(false);
        let pause = get(c, source, "pause")?;
        if c.strict_equal_rooted(written, no)? && c.truthy_rooted(pause)? {
            c.call_rooted(pause, source, &[])?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn on_drain(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let source = get(c, record, "source")?;
    let readable = get(c, source, "readable")?;
    let resume = get(c, source, "resume")?;
    if c.truthy_rooted(readable)? && c.truthy_rooted(resume)? {
        c.call_rooted(resume, source, &[])?;
    }
    Ok(c.undefined())
}

/// Whether the pipe already ended its destination; marks it ended.
fn end_once(c: &mut Context<'_>, record: RootId) -> Result<bool, RootedError> {
    let ended = get(c, record, "didOnEnd")?;
    if c.truthy_rooted(ended)? {
        return Ok(false);
    }
    put_bool(c, record, "didOnEnd", true)?;
    Ok(true)
}

pub(crate) fn on_end(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    if end_once(c, record)? {
        let dest = get(c, record, "dest")?;
        call_method(c, dest, "end", &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn on_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    if end_once(c, record)? {
        let dest = get(c, record, "dest")?;
        let destroy = get(c, dest, "destroy")?;
        if c.is_callable_rooted(destroy)? {
            c.call_rooted(destroy, dest, &[])?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn on_error(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    let error = arg(c, args, 0);
    cleanup_now(c, record)?;
    if listener_count(c, this, "error")? == 0.0 {
        emit(c, this, "error", &[error])?;
    }
    Ok(c.undefined())
}

pub(crate) fn cleanup(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [record] = captures(c)?;
    cleanup_now(c, record)?;
    Ok(c.undefined())
}

fn cleanup_now(c: &mut Context<'_>, record: RootId) -> Result<(), RootedError> {
    let source = get(c, record, "source")?;
    let dest = get(c, record, "dest")?;
    let cleanup = get(c, record, "cleanup")?;
    for (target, event, name) in [
        (source, "data", "ondata"),
        (dest, "drain", "ondrain"),
        (source, "end", "onend"),
        (source, "close", "onclose"),
        (source, "error", "onerror"),
        (dest, "error", "onerror"),
    ] {
        let listener = get(c, record, name)?;
        remove_listener(c, target, event, listener)?;
    }
    remove_listener(c, source, "end", cleanup)?;
    remove_listener(c, source, "close", cleanup)?;
    remove_listener(c, dest, "close", cleanup)
}

/// `Stream.prototype.eventNames()`: the events with at least one listener.
pub(crate) fn event_names(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let events = get(c, this, "_events")?;
    let mut names = Vec::new();
    for key in c.own_keys_rooted(events)? {
        let value = c.get_property_rooted(events, key)?;
        let listed = if c.is_callable_rooted(value)? {
            true
        } else {
            c.is_array_rooted(value)? && super::number(c, value, "length")? > 0.0
        };
        if listed {
            names.push(key);
        }
    }
    c.array_rooted(&names)
}

/// `prependListener(emitter, event, fn)`, for emitters with or without `prependListener`.
pub(crate) fn prepend_listener(c: &mut Context<'_>, emitter: RootId, event: RootId, listener: RootId) -> Result<(), RootedError> {
    let prepend = get(c, emitter, "prependListener")?;
    if c.is_callable_rooted(prepend)? {
        c.call_rooted(prepend, emitter, &[event, listener])?;
        return Ok(());
    }
    let events = get(c, emitter, "_events")?;
    let existing = if c.truthy_rooted(events)? { c.get_property_rooted(events, event)? } else { c.undefined() };
    if !c.truthy_rooted(existing)? {
        call_method(c, emitter, "on", &[event, listener])?;
    } else if c.is_array_rooted(existing)? {
        call_method(c, existing, "unshift", &[listener])?;
    } else {
        let list = c.array_rooted(&[listener, existing])?;
        c.set_property_rooted(events, event, list, events)?;
    }
    Ok(())
}

pub(crate) fn prepend_listener_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (emitter, event, listener) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    prepend_listener(c, emitter, event, listener)?;
    Ok(c.undefined())
}
