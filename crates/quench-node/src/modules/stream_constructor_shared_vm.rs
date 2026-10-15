//! Callable stream constructors initialize their actual receiver. The legacy
//! stream machine supplies Readable/Writable state; Rust owns the shared tails
//! used by both guest subclasses and public native constructors.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
use super::event_constructor_shared_vm::get;

#[derive(Clone, Copy)]
enum Family {
    Duplex,
    Transform,
    PassThrough,
}

fn set(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

pub(crate) fn prepare(c: &mut Context<'_>, dependencies: RootId) -> Result<(), RootedError> {
    for (name, operation) in [
        ("initializeDuplex", "streamInitializeDuplex"),
        ("initializeTransform", "streamInitializeTransform"),
        ("initializePassThrough", "streamInitializePassThrough"),
    ] {
        let function =
            c.host_function_with_data(crate::host::shared_vm::operation(operation), dependencies)?;
        set(c, dependencies, name, function)?;
    }
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let assign = get(c, object, "assign")?;
    set(c, dependencies, "assignOptions", assign)
}

pub(crate) fn install(
    c: &mut Context<'_>,
    module: RootId,
    dependencies: RootId,
) -> Result<(), RootedError> {
    let readable = get(c, module, "Readable")?;
    set(c, dependencies, "readableConstructor", readable)?;
    let mut parent = readable;
    for (name, operation) in [
        ("Duplex", "streamDuplexConstruct"),
        ("Transform", "streamTransformConstruct"),
        ("PassThrough", "streamPassThroughConstruct"),
    ] {
        let original = get(c, module, name)?;
        let prototype = get(c, original, "prototype")?;
        let data = c.null_object_rooted()?;
        set(c, data, "dependencies", dependencies)?;
        let constructor =
            c.host_constructor_with_data(crate::host::shared_vm::operation(operation), data)?;
        set(c, data, "constructor", constructor)?;
        let key = c.string_rooted("name");
        let text = c.string_rooted(name);
        c.define_data_property_rooted(constructor, key, text, false, false, true)?;
        let key = c.string_rooted("prototype");
        c.define_data_property_rooted(constructor, key, prototype, true, false, false)?;
        for key in c.own_keys_rooted(original)? {
            if c.string_text(key)?.is_some_and(|key| {
                matches!(
                    key.as_str(),
                    "name" | "length" | "prototype" | "caller" | "arguments"
                )
            }) {
                continue;
            }
            let descriptor = c.own_property_descriptor_rooted(original, key)?;
            c.define_property_descriptor_rooted(constructor, key, descriptor)?;
        }
        let key = c.string_rooted("constructor");
        c.define_data_property_rooted(prototype, key, constructor, true, false, true)?;
        c.set_prototype_rooted(constructor, parent)?;
        set(c, module, name, constructor)?;
        parent = constructor;
        if name == "PassThrough" {
            let transform = c.host_constructor(crate::host::shared_vm::operation(
                "streamPassThroughTransform",
            ))?;
            let key = c.string_rooted("name");
            let empty = c.string_rooted("");
            c.define_data_property_rooted(transform, key, empty, false, false, true)?;
            let key = c.string_rooted("_transform");
            c.define_data_property_rooted(prototype, key, transform, true, true, true)?;
        }
    }
    Ok(())
}

fn options(c: &mut Context<'_>, value: Option<RootId>) -> Result<RootId, RootedError> {
    let value = value.unwrap_or_else(|| c.undefined());
    if c.truthy_rooted(value)? {
        Ok(value)
    } else {
        c.object_rooted()
    }
}

fn construct(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    family: Family,
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, "constructor")?;
    if !c.instance_of_rooted(receiver, constructor)? {
        return c.construct_rooted(constructor, constructor, args);
    }
    let dependencies = get(c, data, "dependencies")?;
    let original = args.first().copied().unwrap_or_else(|| c.undefined());
    let normalized = options(c, Some(original))?;
    let target = c.object_rooted()?;
    let marker = c.object_rooted()?;
    let yes = c.boolean(true);
    set(c, marker, "__quenchCompatConstruct", yes)?;
    let assign = get(c, dependencies, "assignOptions")?;
    let undefined = c.undefined();
    let readable_options = c.call_rooted(assign, undefined, &[target, normalized, marker])?;
    let readable = get(c, dependencies, "readableConstructor")?;
    c.call_rooted(readable, receiver, &[readable_options])?;
    duplex_tail(c, dependencies, receiver, normalized)?;
    if matches!(family, Family::Transform | Family::PassThrough) {
        transform_tail(c, receiver, original)?;
    }
    if matches!(family, Family::PassThrough) {
        pass_through_tail(c, receiver)?;
    }
    Ok(c.undefined())
}

pub(crate) fn duplex(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    construct(c, receiver, args, Family::Duplex)
}
pub(crate) fn transform(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    construct(c, receiver, args, Family::Transform)
}
pub(crate) fn pass_through(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    construct(c, receiver, args, Family::PassThrough)
}

pub(crate) fn initialize_duplex(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let dependencies = c.host_function_data()?;
    let stream = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("Duplex receiver missing"))?;
    let options = options(c, args.get(1).copied())?;
    duplex_tail(c, dependencies, stream, options)?;
    Ok(c.undefined())
}

fn duplex_tail(
    c: &mut Context<'_>,
    dependencies: RootId,
    stream: RootId,
    options: RootId,
) -> Result<(), RootedError> {
    let yes = c.boolean(true);
    set(c, stream, "_isDuplex", yes)?;
    for name in ["initializeWritableState", "initializeConstruct"] {
        let initializer = get(c, dependencies, name)?;
        let undefined = c.undefined();
        c.call_rooted(initializer, undefined, &[stream, options])?;
    }
    let half_open = get(c, options, "allowHalfOpen")?;
    let allowed = c.boolean(c.rooted_value(half_open).and_then(|v| v.as_bool()) != Some(false));
    set(c, stream, "allowHalfOpen", allowed)?;
    for (option, state, fields) in [
        ("readable", "_readableState", ["ended", "endEmitted"]),
        ("writable", "_writableState", ["ended", "finished"]),
    ] {
        let value = get(c, options, option)?;
        if c.rooted_value(value).and_then(|v| v.as_bool()) == Some(false) {
            let no = c.boolean(false);
            set(c, stream, option, no)?;
            let state = get(c, stream, state)?;
            for field in fields {
                let yes = c.boolean(true);
                set(c, state, field, yes)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn initialize_transform(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let stream = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("Transform receiver missing"))?;
    let options = args.get(1).copied().unwrap_or_else(|| c.undefined());
    transform_tail(c, stream, options)?;
    Ok(c.undefined())
}

fn transform_tail(c: &mut Context<'_>, stream: RootId, options: RootId) -> Result<(), RootedError> {
    let yes = c.boolean(true);
    set(c, stream, "_isTransform", yes)?;
    let null = c.null();
    set(c, stream, "_transformBackpressure", null)?;
    super::stream_shared_vm::apply_transform_options(c, stream, &[stream, options])?;
    if c.truthy_rooted(options)? {
        let final_method = get(c, options, "final")?;
        if c.truthy_rooted(final_method)? {
            let final_method = get(c, options, "final")?;
            set(c, stream, "_final", final_method)?;
        }
    }
    let callback = c.host_function_with_data(
        crate::host::shared_vm::operation("streamTransformPrefinish"),
        stream,
    )?;
    let once = get(c, stream, "once")?;
    let event = c.string_rooted("prefinish");
    c.call_rooted(once, stream, &[event, callback])?;
    Ok(())
}

pub(crate) fn prefinish(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let stream = c.host_function_data()?;
    let end = c.host_function_with_data(
        crate::host::shared_vm::operation("streamTransformFlushed"),
        stream,
    )?;
    let flush = get(c, stream, "_flush")?;
    if c.truthy_rooted(flush)? {
        let flush = get(c, stream, "_flush")?;
        c.call_rooted(flush, stream, &[end])?;
    } else {
        let undefined = c.undefined();
        c.call_rooted(end, undefined, &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn flushed(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let stream = c.host_function_data()?;
    let error = args.first().copied().unwrap_or_else(|| c.undefined());
    if c.truthy_rooted(error)? {
        let destroy = get(c, stream, "destroy")?;
        c.call_rooted(destroy, stream, &[error])?;
        return Ok(c.undefined());
    }
    if let Some(data) = args.get(1).copied() {
        if !c
            .rooted_value(data)
            .is_some_and(|v| v.is_null() || v.is_undefined())
        {
            let push = get(c, stream, "push")?;
            c.call_rooted(push, stream, &[data])?;
        }
    }
    let push = get(c, stream, "push")?;
    let null = c.null();
    c.call_rooted(push, stream, &[null])?;
    Ok(c.undefined())
}

fn pass_through_tail(c: &mut Context<'_>, stream: RootId) -> Result<(), RootedError> {
    let yes = c.boolean(true);
    set(c, stream, "_passThrough", yes)?;
    let no = c.boolean(false);
    set(c, stream, "_passThroughRead", no)
}

pub(crate) fn initialize_pass_through(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let stream = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("PassThrough receiver missing"))?;
    pass_through_tail(c, stream)?;
    Ok(c.undefined())
}

pub(crate) fn pass_through_transform(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let chunk = args.first().copied().unwrap_or_else(|| c.undefined());
    let callback = args.get(2).copied().unwrap_or_else(|| c.undefined());
    let undefined = c.undefined();
    let null = c.null();
    c.call_rooted(callback, undefined, &[null, chunk])?;
    Ok(undefined)
}
