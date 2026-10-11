//! Shared-VM projection of the existing guest stream state machine.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    string_decoder: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(
        crate::modules::stream_source::PRELUDE,
        "node:stream/shared.js",
    )?;
    let dependencies = context.object_rooted()?;
    let replay_state = context.null_object_rooted()?;
    let replay = context.host_function_with_data(
        crate::host::shared_vm::operation("streamReplayWrite"),
        replay_state,
    )?;
    set(context, dependencies, "replayWrite", replay)?;
    let events = context.object_rooted()?;
    let global = context.global_root()?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    set(context, events, "EventEmitter", emitter)?;
    set(context, dependencies, "events", events)?;

    set(context, dependencies, "string_decoder", string_decoder)?;

    let undefined = context.undefined();
    let module = context.call_rooted(factory, undefined, &[dependencies])?;
    let key = context.string_rooted("Writable");
    let writable = context.get_property_rooted(module, key)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(writable, key)?;
    let key = context.string_rooted("write");
    let write = context.get_property_rooted(prototype, key)?;
    set(context, replay_state, "write", write)?;
    let key = context.string_rooted("Readable");
    let readable = context.get_property_rooted(module, key)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(readable, key)?;
    let getter = context.host_function(crate::host::shared_vm::operation("readableEncoding"))?;
    let name = context.string_rooted("name");
    let get_name = context.string_rooted("get");
    context.define_data_property_rooted(getter, name, get_name, false, false, true)?;
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    let key = context.string_rooted("readableEncoding");
    context.define_property_descriptor_rooted(prototype, key, descriptor)?;
    Ok(module)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install shared stream dependency {name}"
        )))
    }
}

pub(crate) fn readable_encoding(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let key = context.string_rooted("_readableState");
    let state = context.get_property_rooted(receiver, key)?;
    if !context.truthy_rooted(state)? {
        return Ok(context.null());
    }
    let key = context.string_rooted("encoding");
    context.get_property_rooted(state, key)
}

/// A queued chunk already belongs to the writable buffered total. Replay via
/// the captured intrinsic without admitting or accounting for it a second time.
pub(crate) fn replay_write(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let key = context.string_rooted("write");
    let write = context.get_property_rooted(data, key)?;
    let undefined = context.undefined();
    let values = [
        args.get(1).copied().unwrap_or(undefined),
        args.get(2).copied().unwrap_or(undefined),
        args.get(3).copied().unwrap_or(undefined),
        context.boolean(true),
    ];
    context.call_rooted(write, args[0], &values)
}
