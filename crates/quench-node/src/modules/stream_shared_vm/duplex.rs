//! `Duplex` (a readable with a writable half), `Transform` and `PassThrough`.

use super::errors::{self, StreamError};
use super::readable::{adopt_options, needs_new, shape_events, READABLE_EVENTS};
use super::state::{self, change};
use super::writable::{WRITABLE_EVENTS, WRITABLE_OPTIONS};
use super::{
    arg, call_method, captures, closure, define_methods, get, get_slot, intrinsics, is_function, is_nullish, next_tick,
    number, on, put, put_bool, put_slot, Class, Context, Key, Outcome, Slot,
};
use quench_runtime::{RootId, RootedError};

/// `new Duplex(options)`.
pub(crate) fn constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    if let Some(class) = needs_new(c, this, Class::Duplex)? {
        return c.construct_rooted(class, class, &[options]);
    }
    initialize(c, this, options)?;
    Ok(c.undefined())
}

fn initialize(c: &mut Context<'_>, this: RootId, options: RootId) -> Result<(), RootedError> {
    let events: Vec<&str> = ["close", "error"]
        .into_iter()
        .chain(WRITABLE_EVENTS.iter().copied().filter(|event| !matches!(*event, "close" | "error")))
        .chain(READABLE_EVENTS.iter().copied().filter(|event| !matches!(*event, "close" | "error")))
        .collect();
    shape_events(c, this, &events)?;
    let readable = state::create(c, Class::ReadableState, options, this, true)?;
    put(c, this, "_readableState", readable)?;
    let writable = state::create(c, Class::WritableState, options, this, true)?;
    put(c, this, "_writableState", writable)?;
    let no = c.boolean(false);
    if c.truthy_rooted(options)? {
        let half_open = get(c, options, "allowHalfOpen")?;
        let allow = !c.strict_equal_rooted(half_open, no)?;
        put_bool(c, this, "allowHalfOpen", allow)?;
        let readable_option = get(c, options, "readable")?;
        if c.strict_equal_rooted(readable_option, no)? {
            put(c, readable, "readable", no)?;
            change(c, readable, state::readable::ENDED | state::readable::END_EMITTED, 0)?;
        }
        let writable_option = get(c, options, "writable")?;
        if c.strict_equal_rooted(writable_option, no)? {
            put(c, writable, "writable", no)?;
            let closed = state::writable::ENDING | state::writable::ENDED | state::writable::FINISHED;
            change(c, writable, closed, 0)?;
        }
        adopt_options(c, this, options, &[("read", "_read")])?;
        adopt_options(c, this, options, WRITABLE_OPTIONS)?;
        let signal = get(c, options, "signal")?;
        if c.truthy_rooted(signal)? {
            super::utils::add_abort_signal(c, signal, this)?;
        }
    } else {
        put_bool(c, this, "allowHalfOpen", true)?;
    }
    let stream = intrinsics(c)?.class(Class::Stream);
    c.call_rooted(stream, this, &[options])?;
    super::destroy::construct_if_defined(c, this)
}

/// The Writable accessors a Duplex shares, by the same functions.
const SHARED_WRITABLE_ACCESSORS: &[&str] = &[
    "writable",
    "writableHighWaterMark",
    "writableObjectMode",
    "writableBuffer",
    "writableLength",
    "writableFinished",
    "writableCorked",
    "writableEnded",
    "writableNeedDrain",
];

pub(crate) fn build_duplex(c: &mut Context<'_>, readable: RootId, writable: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamDuplex"), "Duplex")?;
    let prototype = get(c, constructor, "prototype")?;
    let readable_prototype = get(c, readable, "prototype")?;
    c.set_prototype_rooted(prototype, readable_prototype)?;
    c.set_prototype_rooted(constructor, readable)?;
    let writable_prototype = get(c, writable, "prototype")?;
    for key in c.enumerable_own_keys_rooted(writable_prototype, false, false)? {
        let current = c.get_property_rooted(prototype, key)?;
        if !c.truthy_rooted(current)? {
            let value = c.get_property_rooted(writable_prototype, key)?;
            c.set_property_rooted(prototype, key, value, prototype)?;
        }
    }
    let destroy = get(c, writable_prototype, "destroy")?;
    put(c, prototype, "destroy", destroy)?;
    for name in SHARED_WRITABLE_ACCESSORS {
        let key = c.string_rooted(name);
        let fields = c
            .own_property_fields_rooted(writable_prototype, key)?
            .ok_or_else(|| RootedError::host("Writable accessor missing"))?;
        c.define_property_fields_rooted(prototype, key, fields)?;
    }
    super::utils::define_stream_accessors(c, prototype, super::utils::DUPLEX_ACCESSORS)?;
    Ok(constructor)
}

/// `new Transform(options)`.
pub(crate) fn transform(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    if let Some(class) = needs_new(c, this, Class::Transform)? {
        return c.construct_rooted(class, class, &[options]);
    }
    initialize_transform(c, this, options)?;
    Ok(c.undefined())
}

fn initialize_transform(c: &mut Context<'_>, this: RootId, options: RootId) -> Result<(), RootedError> {
    let mut options = options;
    if c.truthy_rooted(options)? {
        let mark = state::high_water_mark(c, false, options, "readableHighWaterMark", true)?;
        if mark == 0.0 {
            let global = c.global_root()?;
            let object = get(c, global, "Object")?;
            let copy = c.object_rooted()?;
            let copy = call_method(c, object, "assign", &[copy, options])?;
            let null = c.null();
            put(c, copy, "highWaterMark", null)?;
            let zero = c.number(0.0);
            put(c, copy, "readableHighWaterMark", zero)?;
            let writable_mark = get(c, options, "writableHighWaterMark")?;
            let writable_mark = if c.truthy_rooted(writable_mark)? { writable_mark } else { zero };
            put(c, copy, "writableHighWaterMark", writable_mark)?;
            options = copy;
        }
    }
    initialize(c, this, options)?;
    let readable = get(c, this, "_readableState")?;
    change(c, readable, 0, state::readable::SYNC)?;
    let null = c.null();
    put_slot(c, this, Slot::Callback, null)?;
    if c.truthy_rooted(options)? {
        adopt_options(c, this, options, &[("transform", "_transform"), ("flush", "_flush")])?;
    }
    let prefinish = c.host_function_named(crate::host::shared_vm::operation("streamTransformPrefinish"), "prefinish")?;
    on(c, this, "prefinish", prefinish)
}

/// The `prefinish` listener: flush unless `_final` was replaced.
pub(crate) fn transform_prefinish(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let transform = intrinsics(c)?.class(Class::Transform);
    let prototype = get(c, transform, "prototype")?;
    let original = get(c, prototype, "_final")?;
    let current = get(c, this, "_final")?;
    if !c.strict_equal_rooted(current, original)? {
        let undefined = c.undefined();
        transform_final(c, this, &[undefined])?;
    }
    Ok(c.undefined())
}

/// `Transform.prototype._final(cb)`: flush, then end the readable half.
pub(crate) fn transform_final(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    let flush = get(c, this, "_flush")?;
    let destroyed = get(c, this, "destroyed")?;
    if is_function(c, flush)? && !c.truthy_rooted(destroyed)? {
        let done = closure(c, "streamTransformFlushed", "", &[this, callback])?;
        c.call_rooted(flush, this, &[done])?;
    } else {
        end_readable_half(c, this, callback)?;
    }
    Ok(c.undefined())
}

fn end_readable_half(c: &mut Context<'_>, stream: RootId, callback: RootId) -> Result<(), RootedError> {
    let null = c.null();
    call_method(c, stream, "push", &[null])?;
    if c.truthy_rooted(callback)? {
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[])?;
    }
    Ok(())
}

/// The callback `_flush` receives.
pub(crate) fn transform_flushed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, callback] = captures(c)?;
    let (error, data) = (arg(c, args, 0), arg(c, args, 1));
    if c.truthy_rooted(error)? {
        if c.truthy_rooted(callback)? {
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[error])?;
        } else {
            call_method(c, stream, "destroy", &[error])?;
        }
        return Ok(c.undefined());
    }
    if !is_nullish(c, data) {
        call_method(c, stream, "push", &[data])?;
    }
    end_readable_half(c, stream, callback)?;
    Ok(c.undefined())
}

pub(crate) fn default_transform(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(errors::throw(c, StreamError::MethodNotImplemented("_transform()")))
}

/// `Transform.prototype._write(chunk, encoding, callback)`.
pub(crate) fn transform_write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, encoding, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let readable = get(c, this, "_readableState")?;
    let length = get(c, readable, "length")?;
    let done = closure(c, "streamTransformDone", "", &[this, callback, length])?;
    call_method(c, this, "_transform", &[chunk, encoding, done])?;
    Ok(c.undefined())
}

/// The callback `_transform` receives.
pub(crate) fn transform_done(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [stream, callback, length] = captures(c)?;
    let (error, value) = (arg(c, args, 0), arg(c, args, 1));
    let undefined = c.undefined();
    if c.truthy_rooted(error)? {
        c.call_rooted(callback, undefined, &[error])?;
        return Ok(undefined);
    }
    if !is_nullish(c, value) {
        call_method(c, stream, "push", &[value])?;
    }
    let readable = get(c, stream, "_readableState")?;
    let writable = get(c, stream, "_writableState")?;
    if state::has(c, readable, state::readable::ENDED)? {
        next_tick(c, callback, &[])?;
        return Ok(undefined);
    }
    let current = number(c, readable, "length")?;
    let ready = state::has(c, writable, state::writable::ENDED)?
        || c.to_number(length)? == current
        || current < number(c, readable, "highWaterMark")?;
    if ready {
        c.call_rooted(callback, undefined, &[])?;
    } else {
        put_slot(c, stream, Slot::Callback, callback)?;
    }
    Ok(undefined)
}

/// `Transform.prototype._read()`: release the write waiting for buffer room.
pub(crate) fn transform_read(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let callback = get_slot(c, this, Slot::Callback)?;
    if c.truthy_rooted(callback)? {
        let null = c.null();
        put_slot(c, this, Slot::Callback, null)?;
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn build_transform(c: &mut Context<'_>, duplex: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamTransform"), "Transform")?;
    let prototype = get(c, constructor, "prototype")?;
    let duplex_prototype = get(c, duplex, "prototype")?;
    c.set_prototype_rooted(prototype, duplex_prototype)?;
    c.set_prototype_rooted(constructor, duplex)?;
    define_methods(
        c,
        prototype,
        &[
            (Key::Name("_final"), "streamTransformFinal", "final"),
            (Key::Name("_transform"), "streamTransformDefault", ""),
            (Key::Name("_write"), "streamTransformWrite", ""),
            (Key::Name("_read"), "streamTransformRead", ""),
        ],
    )?;
    Ok(constructor)
}

/// `new PassThrough(options)`.
pub(crate) fn pass_through(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    if let Some(class) = needs_new(c, this, Class::PassThrough)? {
        return c.construct_rooted(class, class, &[options]);
    }
    initialize_transform(c, this, options)?;
    Ok(c.undefined())
}

/// `PassThrough.prototype._transform(chunk, encoding, cb)`: `cb(null, chunk)`.
pub(crate) fn pass_through_transform(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (chunk, callback) = (arg(c, args, 0), arg(c, args, 2));
    let null = c.null();
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[null, chunk])?;
    Ok(undefined)
}

pub(crate) fn build_pass_through(c: &mut Context<'_>, transform: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamPassThrough"), "PassThrough")?;
    let prototype = get(c, constructor, "prototype")?;
    let transform_prototype = get(c, transform, "prototype")?;
    c.set_prototype_rooted(prototype, transform_prototype)?;
    c.set_prototype_rooted(constructor, transform)?;
    define_methods(c, prototype, &[(Key::Name("_transform"), "streamPassThroughTransform", "")])?;
    Ok(constructor)
}

/// `new DuplexSide(options)`: one end of a `duplexPair`.
pub(crate) fn duplex_side(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    initialize(c, this, options)?;
    let null = c.null();
    put_slot(c, this, Slot::PairCallback, null)?;
    put_slot(c, this, Slot::OtherSide, null)?;
    Ok(c.undefined())
}

/// `DuplexSide.prototype._read()`: release the other side's waiting write.
pub(crate) fn side_read(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let callback = get_slot(c, this, Slot::PairCallback)?;
    if c.truthy_rooted(callback)? {
        let null = c.null();
        put_slot(c, this, Slot::PairCallback, null)?;
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[])?;
    }
    Ok(c.undefined())
}

/// `DuplexSide.prototype._write(chunk, encoding, callback)`: hand the chunk to the other
/// side, which calls back once its reader takes more.
pub(crate) fn side_write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, callback) = (arg(c, args, 0), arg(c, args, 2));
    if number(c, chunk, "length")? == 0.0 {
        next_tick(c, callback, &[])?;
    } else {
        let other = get_slot(c, this, Slot::OtherSide)?;
        call_method(c, other, "push", &[chunk])?;
        put_slot(c, other, Slot::PairCallback, callback)?;
    }
    Ok(c.undefined())
}

/// `DuplexSide.prototype._final(callback)`: end the other side's readable half.
pub(crate) fn side_final(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    let other = get_slot(c, this, Slot::OtherSide)?;
    on(c, other, "end", callback)?;
    let null = c.null();
    call_method(c, other, "push", &[null])?;
    Ok(c.undefined())
}

pub(crate) fn build_duplex_side(c: &mut Context<'_>, duplex: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamDuplexSide"), "DuplexSide")?;
    let prototype = get(c, constructor, "prototype")?;
    let duplex_prototype = get(c, duplex, "prototype")?;
    c.set_prototype_rooted(prototype, duplex_prototype)?;
    c.set_prototype_rooted(constructor, duplex)?;
    for (name, operation) in [("_read", "streamDuplexSideRead"), ("_write", "streamDuplexSideWrite"), ("_final", "streamDuplexSideFinal")] {
        let method = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, method, true, false, true)?;
    }
    Ok(constructor)
}

/// `duplexPair(options)`: two duplexes, each writing into the other's readable half.
pub(crate) fn duplex_pair(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    let side = intrinsics(c)?.class(Class::DuplexSide);
    let first = c.construct_rooted(side, side, &[options])?;
    let second = c.construct_rooted(side, side, &[options])?;
    put_slot(c, first, Slot::OtherSide, second)?;
    put_slot(c, second, Slot::OtherSide, first)?;
    c.array_rooted(&[first, second])
}
