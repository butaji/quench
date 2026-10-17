//! TLS handle methods resolve genuine session identity through a Core weak map.
use super::socket::{get, set};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) fn registry(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let api = super::facade::install(c)?;
    let registry_key = c.string_rooted("handleOwners");
    let descriptor = c.own_property_descriptor_rooted(api, registry_key)?;
    if c.is_object_rooted(descriptor)? {
        return get(c, descriptor, "value");
    }
    let data = c.null_object_rooted()?;
    let global = c.global_root()?;
    let weak_map = get(c, global, "WeakMap")?;
    let map = c.construct_rooted(weak_map, weak_map, &[])?;
    let prototype = get(c, weak_map, "prototype")?;
    set(c, data, "map", map)?;
    for name in ["get", "set"] {
        let function = get(c, prototype, name)?;
        set(c, data, name, function)?;
    }
    let function = c.host_function_with_data(
        crate::host::shared_vm::operation("tlsHandleVerifyError"),
        data,
    )?;
    let key = c.string_rooted("name");
    let name = c.string_rooted("verifyError");
    c.define_data_property_rooted(function, key, name, false, false, true)?;
    set(c, data, "verifyError", function)?;
    c.define_data_property_rooted(api, registry_key, data, false, false, false)?;
    Ok(data)
}
pub(crate) fn install(c: &mut Context<'_>, socket: RootId) -> Result<(), RootedError> {
    let data = registry(c)?;
    let map = get(c, data, "map")?;
    let register = get(c, data, "set")?;
    let current = get(c, socket, "_handle")?;
    // An SSL session exists before a client has a physical connection.
    let handle = if c.is_object_rooted(current)? {
        current
    } else {
        c.object_rooted()?
    };
    set(c, socket, "_handle", handle)?;
    c.call_rooted(register, map, &[handle, socket])?;
    let function = get(c, data, "verifyError")?;
    let key = c.string_rooted("verifyError");
    c.define_data_property_rooted(handle, key, function, true, true, true)?;
    c.assign_named_property_rooted(socket, "ssl", handle)
}
pub(crate) fn reusable(c: &mut Context<'_>, socket: RootId) -> Result<Option<RootId>, RootedError> {
    if super::sessions::find_owner(c, socket)?.is_none() {
        return Ok(None);
    }
    let api = super::facade::install(c)?;
    let key = c.string_rooted("handleOwners");
    let descriptor = c.own_property_descriptor_rooted(api, key)?;
    if !c.is_object_rooted(descriptor)? {
        return Ok(None);
    }
    let data = get(c, descriptor, "value")?;
    let map = get(c, data, "map")?;
    let find = get(c, data, "get")?;
    let handle = get(c, socket, "_handle")?;
    let owner = c.call_rooted(find, map, &[handle])?;
    if c.strict_equal_rooted(owner, socket)? {
        Ok(Some(handle))
    } else {
        Ok(None)
    }
}
pub(crate) fn verify_error(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let map = get(c, data, "map")?;
    let find = get(c, data, "get")?;
    let socket = c.call_rooted(find, map, &[receiver])?;
    if c.rooted_value(socket)
        .is_some_and(|value| value.is_undefined())
    {
        let error = c.type_error_rooted("Illegal invocation")?;
        return Err(c.throw(error));
    }
    let query = c.string_rooted("verifyError");
    let undefined = c.undefined();
    super::socket::info(c, undefined, &[socket, query])
}
