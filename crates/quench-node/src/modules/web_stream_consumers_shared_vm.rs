//! Promise-based consumers for Web ReadableStreams.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let buffer_key = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(global, buffer_key)?;
    let decoder_key = context.string_rooted("TextDecoder");
    let decoder = context.get_property_rooted(global, decoder_key)?;
    let factory = crate::host::removed_javascript("node:stream/consumers/shared.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[buffer, decoder])
}
