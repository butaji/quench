//! Socket pull requests and native read interest retain the original handle hooks.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

fn method(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = get(context, receiver, name)?;
    context.call_rooted(function, receiver, args)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(receiver, key, value, receiver)?;
    Ok(())
}
fn read_error(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    status: RootId,
) -> Result<(), RootedError> {
    let status_number = context.to_number(status)?;
    let text = super::util_system_error_shared_vm::system_error_name(status_number);
    let code = context.string_rooted(&text);
    let error = context.error_rooted(&format!("read {text}"))?;
    set(context, error, "code", code)?;
    set(context, error, "errno", status)?;
    let syscall = context.string_rooted("read");
    set(context, error, "syscall", syscall)?;
    method(context, socket, "destroy", &[error])?;
    Ok(())
}

pub(crate) fn read(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let connecting = get(context, socket, "connecting")?;
    let pending_tls = super::tls_shared_vm::sessions::find_owner(context, socket)?.is_some()
        && super::tls_shared_vm::socket::physical_owner(context, socket)?.is_none();
    let waiting = if context.truthy_rooted(connecting)? || pending_tls {
        true
    } else {
        let handle = get(context, socket, "_handle")?;
        !context.truthy_rooted(handle)?
    };
    if waiting {
        let data = context.array_rooted(args)?;
        let callback = context.host_function_with_data(
            crate::host::shared_vm::operation("netSocketReadAfterConnect"),
            data,
        )?;
        let connect = context.string_rooted("connect");
        method(context, socket, "once", &[connect, callback])?;
    } else {
        let handle = get(context, socket, "_handle")?;
        let reading = get(context, handle, "reading")?;
        if !context.truthy_rooted(reading)? {
            let handle = get(context, socket, "_handle")?;
            let yes = context.boolean(true);
            set(context, handle, "reading", yes)?;
            let handle = get(context, socket, "_handle")?;
            let status = method(context, handle, "readStart", &[])?;
            if context.truthy_rooted(status)? {
                read_error(context, socket, status)?;
            }
        }
    }
    Ok(context.undefined())
}
pub(crate) fn read_after_connect(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let socket = get(context, data, "0")?;
    let amount = get(context, data, "1")?;
    method(context, socket, "_read", &[amount])
}
pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let handle = get(context, socket, "_handle")?;
    if context.truthy_rooted(handle)? {
        let paused = get(context, args[1], "pauseOnCreate")?;
        if context.truthy_rooted(paused)? {
            let handle = get(context, socket, "_handle")?;
            let no = context.boolean(false);
            set(context, handle, "reading", no)?;
            let handle = get(context, socket, "_handle")?;
            method(context, handle, "readStop", &[])?;
            set(context, socket, "readableFlowing", no)?;
        } else {
            let manual = get(context, args[1], "manualStart")?;
            if !context.truthy_rooted(manual)? {
                let zero = context.number(0.0);
                method(context, socket, "read", &[zero])?;
            }
        }
    }
    Ok(context.undefined())
}

pub(crate) fn connected(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<(), RootedError> {
    let ready = context.string_rooted("ready");
    method(context, socket, "emit", &[ready])?;
    let readable = get(context, socket, "readable")?;
    if context.truthy_rooted(readable)? {
        let paused = method(context, socket, "isPaused", &[])?;
        if !context.truthy_rooted(paused)? {
            let zero = context.number(0.0);
            method(context, socket, "read", &[zero])?;
        }
    }
    Ok(())
}

pub(crate) fn delivered(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    result: RootId,
) -> Result<(), RootedError> {
    if !context.truthy_rooted(result)? {
        let handle = get(context, socket, "_handle")?;
        if !context.truthy_rooted(handle)? {
            return Ok(());
        }
        let no = context.boolean(false);
        set(context, handle, "reading", no)?;
        let destroyed = get(context, socket, "destroyed")?;
        if !context.truthy_rooted(destroyed)? {
            let status = method(context, handle, "readStop", &[])?;
            if context.truthy_rooted(status)? {
                read_error(context, socket, status)?;
            }
        }
    }
    Ok(())
}

/// TLS owns one mutable reading view; its original TCP handle forwards to it.
pub(crate) fn redirect_parent_reading(
    context: &mut NativeContext<'_, NodeHost>,
    parent: RootId,
    handle: RootId,
) -> Result<(), RootedError> {
    let getter = context.host_function_with_data(
        crate::host::shared_vm::operation("netParentReadingGet"),
        handle,
    )?;
    let setter = context.host_function_with_data(
        crate::host::shared_vm::operation("netParentReadingSet"),
        handle,
    )?;
    let name = context.string_rooted("name");
    let value = context.string_rooted("get");
    context.define_data_property_rooted(getter, name, value, false, false, true)?;
    let value = context.string_rooted("set");
    context.define_data_property_rooted(setter, name, value, false, false, true)?;
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    set(context, descriptor, "set", setter)?;
    let key = context.string_rooted("reading");
    context.define_property_descriptor_rooted(parent, key, descriptor)?;
    Ok(())
}
pub(crate) fn parent_reading_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = context.host_function_data()?;
    get(context, handle, "reading")
}
pub(crate) fn parent_reading_set(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = context.host_function_data()?;
    set(context, handle, "reading", args[0])?;
    Ok(context.undefined())
}
