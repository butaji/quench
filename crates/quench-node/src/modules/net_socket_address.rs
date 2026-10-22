//! Public endpoint views follow original handle methods and Node's lazy caches.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}

pub(crate) fn peer(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let handle = get(context, socket, "_handle")?;
    let unavailable = if !context.truthy_rooted(handle)? {
        true
    } else {
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, "getpeername")?;
        if !context.truthy_rooted(method)? {
            true
        } else {
            let connecting = get(context, socket, "connecting")?;
            context.truthy_rooted(connecting)?
        }
    };
    if unavailable {
        let cached = get(context, socket, "_peername")?;
        return if context.truthy_rooted(cached)? {
            Ok(cached)
        } else {
            context.object_rooted()
        };
    }
    let cached = get(context, socket, "_peername")?;
    if !context.truthy_rooted(cached)? {
        let out = context.object_rooted()?;
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, "getpeername")?;
        let status = context.call_rooted(method, handle, &[out])?;
        if context.truthy_rooted(status)? {
            return Ok(out);
        }
        set(context, socket, "_peername", out)?;
    }
    get(context, socket, "_peername")
}

pub(crate) fn local(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let handle = get(context, socket, "_handle")?;
    let unavailable = if !context.truthy_rooted(handle)? {
        true
    } else {
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, "getsockname")?;
        !context.truthy_rooted(method)?
    };
    if unavailable {
        return context.object_rooted();
    }
    let cached = get(context, socket, "_sockname")?;
    if !context.truthy_rooted(cached)? {
        let out = context.object_rooted()?;
        set(context, socket, "_sockname", out)?;
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, "getsockname")?;
        let out = get(context, socket, "_sockname")?;
        context.call_rooted(method, handle, &[out])?;
    }
    get(context, socket, "_sockname")
}

pub(crate) fn view(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let data = context.host_function_data()?;
    let method = get(context, data, "method")?;
    let original = context.get_property_rooted(socket, method)?;
    let endpoint = context.call_rooted(original, socket, &[])?;
    let field = get(context, data, "field")?;
    context.get_property_rooted(endpoint, field)
}

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    constructor: RootId,
) -> Result<(), RootedError> {
    let prototype = get(context, constructor, "prototype")?;
    let undefined = context.undefined();
    let factory = context.evaluate_script_rooted(
        quench_js_check::checked_js!("operation=>function(){'use strict';return operation(this);}"),
        "node:net/endpoint-method.js",
    )?;
    for (name, operation) in [
        ("_getpeername", "netSocketPeerName"),
        ("_getsockname", "netSocketLocalName"),
    ] {
        let native = context.host_function(crate::host::shared_vm::operation(operation))?;
        let method = context.call_rooted(factory, undefined, &[native])?;
        set(context, prototype, name, method)?;
    }
    for (prefix, method) in [("remote", "_getpeername"), ("local", "_getsockname")] {
        for (suffix, field) in [
            ("Address", "address"),
            ("Family", "family"),
            ("Port", "port"),
        ] {
            let name = format!("{prefix}{suffix}");
            let data = context.null_object_rooted()?;
            let value = context.string_rooted(method);
            set(context, data, "method", value)?;
            let value = context.string_rooted(field);
            set(context, data, "field", value)?;
            let operation = context.host_function_with_data(
                crate::host::shared_vm::operation("netSocketEndpointView"),
                data,
            )?;
            let getter = context.call_rooted(factory, undefined, &[operation])?;
            let key = context.string_rooted("name");
            let value = context.string_rooted(&name);
            context.define_data_property_rooted(getter, key, value, false, false, true)?;
            let descriptor = context.null_object_rooted()?;
            set(context, descriptor, "get", getter)?;
            let yes = context.boolean(true);
            set(context, descriptor, "enumerable", yes)?;
            let no = context.boolean(false);
            set(context, descriptor, "configurable", no)?;
            let key = context.string_rooted(&name);
            context.define_property_descriptor_rooted(prototype, key, descriptor)?;
        }
    }
    Ok(())
}
