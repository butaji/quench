//! Shared-VM projection of the existing guest stream state machine.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    string_decoder: RootId,
) -> Result<RootId, RootedError> {
    let factory =
        context.evaluate_script_rooted(crate::modules::stream::PRELUDE, "node:stream/shared.js")?;
    let dependencies = context.object_rooted()?;
    let events = context.object_rooted()?;
    let global = context.global_root()?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    set(context, events, "EventEmitter", emitter)?;
    set(context, dependencies, "events", events)?;

    set(context, dependencies, "string_decoder", string_decoder)?;

    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[dependencies])
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
