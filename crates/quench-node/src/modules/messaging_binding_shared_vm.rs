//! Native messaging primitives consumed by Node's `internal/worker` library.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(context, "\0binding:messaging", build)
}

fn build(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    // Node's per-context messaging binding re-exports the DOM exception
    // constructors used by internal consumers such as Web Crypto's
    // getRandomValues() quota check.
    let global = context.global_root()?;
    for name in ["DOMException", "QuotaExceededError"] {
        let key = context.string_rooted(name);
        let constructor = context.get_property_rooted(global, key)?;
        context.define_data_property_rooted(binding, key, constructor, true, true, true)?;
    }
    for (name, operation) in [
        ("structuredClone", "messagingStructuredClone"),
        ("markAsUncloneable", "markAsUncloneable"),
        ("markAsUntransferable", "markAsUntransferable"),
        ("isMarkedAsUntransferable", "isMarkedAsUntransferable"),
    ] {
        let function =
            context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(binding, key, function, true, true, true)?;
    }
    let setter = context.host_function_with_data(
        crate::host::shared_vm::operation("messagingSetDeserializer"),
        binding,
    )?;
    let key = context.string_rooted("setDeserializerCreateObjectFunction");
    context.define_data_property_rooted(binding, key, setter, true, true, true)?;
    Ok(binding)
}

pub(crate) fn set_deserializer(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let binding = context.host_function_data()?;
    let key = context.string_rooted("deserializerCreateObjectFunction");
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    context.set_property_rooted(binding, key, value, binding)?;
    Ok(context.undefined())
}
