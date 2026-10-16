//! The configured Node SDK owns the Console constructor and global namespace.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let console =
        crate::host::shared_vm::commonjs::node_library_module(context, "console.js", "console.js")?;
    let constructor_key = context.string_rooted("Console");
    let constructor = context.get_property_rooted(console, constructor_key)?;
    let prototype_key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(constructor, prototype_key)?;
    // Node's global console boundary is variadic and names aliases by their
    // namespace keys. Instance methods keep the SDK's original metadata.
    let name = context.string_rooted("name");
    let length = context.string_rooted("length");
    let variadic = context.number(0.0);
    for key in context.own_non_index_keys_rooted(prototype, true)? {
        if context.string_text(key)?.is_none() {
            continue;
        }
        let method = context.get_property_rooted(console, key)?;
        if !context.is_callable_rooted(method)? {
            continue;
        }
        context.define_data_property_rooted(method, name, key, false, false, true)?;
        context.define_data_property_rooted(method, length, variadic, false, false, true)?;
    }
    Ok(console)
}
