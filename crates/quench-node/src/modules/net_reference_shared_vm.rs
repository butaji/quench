//! Server reference requests project onto the listener's single reference bit.
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
pub(crate) fn reference_server(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args[0];
    let referenced = context.truthy_rooted(args[1])?;
    let unref = context.boolean(!referenced);
    set(context, server, "_unref", unref)?;
    let handle = get(context, server, "_handle")?;
    if context.truthy_rooted(handle)? {
        // The public guard and call each read the handle, as Node does.
        let handle = get(context, server, "_handle")?;
        let method = get(context, handle, if referenced { "ref" } else { "unref" })?;
        context.call_rooted(method, handle, &[])?;
    }
    Ok(server)
}
pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    id: u64,
    server: RootId,
) -> Result<(), RootedError> {
    let unref = get(context, server, "_unref")?;
    let referenced = !context.truthy_rooted(unref)?;
    super::net_shared_vm::listener_reference(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        id,
        referenced,
    );
    let handle = context.object_rooted()?;
    let id_root = context.number(id as f64);
    for (name, referenced) in [("ref", true), ("unref", false)] {
        let referenced = context.boolean(referenced);
        let data = context.array_rooted(&[id_root, handle, referenced])?;
        let method = context.host_function_with_data(
            crate::host::shared_vm::operation("netListenerReference"),
            data,
        )?;
        let key = context.string_rooted("name");
        let name_root = context.string_rooted(name);
        context.define_data_property_rooted(method, key, name_root, false, false, true)?;
        set(context, handle, name, method)?;
    }
    let data = context.array_rooted(&[id_root, handle])?;
    let method = context
        .host_function_with_data(crate::host::shared_vm::operation("netListenerHasRef"), data)?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("hasRef");
    context.define_data_property_rooted(method, key, name, false, false, true)?;
    set(context, handle, "hasRef", method)?;
    set(context, server, "_handle", handle)
}
fn owner(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
) -> Result<(u64, RootId), RootedError> {
    let data = context.host_function_data()?;
    let handle = get(context, data, "1")?;
    if !context.strict_equal_rooted(receiver, handle)? {
        let error = context.type_error_rooted("Illegal invocation")?;
        return Err(context.throw(error));
    }
    let id = get(context, data, "0")?;
    let id = context
        .rooted_value(id)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("listener owner missing"))? as u64;
    Ok((id, data))
}
pub(crate) fn reference_handle(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, data) = owner(context, receiver)?;
    let value = get(context, data, "2")?;
    let referenced = context.truthy_rooted(value)?;
    super::net_shared_vm::listener_reference(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        id,
        referenced,
    );
    Ok(context.undefined())
}
pub(crate) fn handle_has_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, _) = owner(context, receiver)?;
    let value =
        super::net_shared_vm::listener_has_ref(&context.host_mut().shared_state().borrow().tcp, id);
    Ok(match value {
        Some(value) => context.boolean(value),
        None => context.undefined(),
    })
}
