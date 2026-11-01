//! Admission and connection counts follow the original net.Server callback.
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
fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
    event: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let method = get(context, server, "emit")?;
    let event = context.string_rooted(event);
    let mut arguments = Vec::with_capacity(args.len() + 1);
    arguments.push(event);
    arguments.extend_from_slice(args);
    context.call_rooted(method, server, &arguments)
}
fn close_handle(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<RootId, RootedError> {
    let close = get(context, handle, "close")?;
    context.call_rooted(close, handle, &[])
}
fn drop_connection(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
    handle: RootId,
) -> Result<RootId, RootedError> {
    let local = get(context, handle, "getsockname")?;
    let has_names = if context.truthy_rooted(local)? {
        true
    } else {
        let peer = get(context, handle, "getpeername")?;
        context.truthy_rooted(peer)?
    };
    if has_names {
        let data = context.null_object_rooted()?;
        for (method, prefix) in [("getsockname", "local"), ("getpeername", "remote")] {
            let original = get(context, handle, method)?;
            if context.truthy_rooted(original)? {
                let info = context.null_object_rooted()?;
                let original = get(context, handle, method)?;
                context.call_rooted(original, handle, &[info])?;
                for (source, suffix) in [
                    ("address", "Address"),
                    ("port", "Port"),
                    ("family", "Family"),
                ] {
                    let value = get(context, info, source)?;
                    set(context, data, &format!("{prefix}{suffix}"), value)?;
                }
            }
        }
        emit(context, server, "drop", &[data])?;
    } else {
        emit(context, server, "drop", &[])?;
    }
    close_handle(context, handle)?;
    Ok(context.undefined())
}

pub(crate) fn on_connection(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let receiver = args[0];
    let error = args[1];
    let handle = args[2];
    let owner_key = super::net_reference_shared_vm::owner_symbol(context)?;
    let server = context.get_property_rooted(receiver, owner_key)?;
    if context.truthy_rooted(error)? {
        let errno = context
            .rooted_value(error)
            .and_then(|value| value.as_number())
            .unwrap_or(0.0);
        let code = super::util_system_error_shared_vm::system_error_name(errno);
        let exception = context.error_rooted(&format!("accept {code}"))?;
        let code = context.string_rooted(&code);
        set(context, exception, "code", code)?;
        set(context, exception, "errno", error)?;
        let syscall = context.string_rooted("accept");
        set(context, exception, "syscall", syscall)?;
        emit(context, server, "error", &[exception])?;
        return Ok(context.undefined());
    }
    let maximum = get(context, server, "maxConnections")?;
    if !context
        .rooted_value(maximum)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        let count = get(context, server, "_connections")?;
        let maximum = get(context, server, "maxConnections")?;
        if context.greater_equal_rooted(count, maximum)? {
            return drop_connection(context, server, handle);
        }
    }
    let block = get(context, server, "blockList")?;
    if context.truthy_rooted(block)? {
        let peer = get(context, handle, "getpeername")?;
        if context.is_callable_rooted(peer)? {
            let info = context.null_object_rooted()?;
            let peer = get(context, handle, "getpeername")?;
            context.call_rooted(peer, handle, &[info])?;
            let address = get(context, info, "address")?;
            let undefined = context.undefined();
            let family = super::net_shared_vm::is_ip(context, undefined, &[address])?;
            if context.truthy_rooted(family)? {
                let block = get(context, server, "blockList")?;
                let check = get(context, block, "check")?;
                let address = get(context, info, "address")?;
                let family = context.to_string(family)?;
                let family = context.string_rooted(&format!("ipv{family}"));
                let blocked = context.call_rooted(check, block, &[address, family])?;
                if context.truthy_rooted(blocked)? {
                    close_handle(context, handle)?;
                    return Ok(context.undefined());
                }
            }
        }
    }
    let undefined = context.undefined();
    let options = super::net_socket_options::accepted_options(context, undefined, &[server])?;
    set(context, options, "handle", handle)?;
    let yes = context.boolean(true);
    set(context, options, "readable", yes)?;
    set(context, options, "writable", yes)?;
    let constructor = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_socket_constructor
        .ok_or_else(|| RootedError::host("net.Socket constructor missing"))?;
    let socket = context.construct_rooted(constructor, constructor, &[options])?;
    let id = super::net_reference_shared_vm::handle_id(context, handle)?;
    let id_value = context.number(id as f64);
    set(context, socket, "__quenchNetSocketId", id_value)?;
    context.set_property_rooted(handle, owner_key, socket, handle)?;
    let retained = context.retain(socket)?;
    let shared = context.host_mut().shared_state();
    shared.borrow_mut().net_sockets.insert(
        id,
        crate::host::node_host::NetSocket {
            root: retained,
            encoding: None,
            parent_server: None,
            write_callbacks: Default::default(),
        },
    );
    // Socket construction can request reads before public ownership is tagged.
    // Transfer that read interest without retaining a second raw callback owner.
    super::stream_binding::release_reader(context, id);
    super::net_socket_options::apply_accepted(context, server, socket, handle)?;
    let count = get(context, server, "_connections")?;
    let count = context.numeric_update_rooted(count, false)?;
    set(context, server, "_connections", count)?;
    let server_id = get(context, server, "__quenchNetServerId")?;
    let server_id = context
        .rooted_value(server_id)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);
    if let Some(server_id) = server_id {
        let mut state = shared.borrow_mut();
        if let Some(endpoint) = state.net_sockets.get_mut(&id) {
            endpoint.parent_server = Some(server_id);
        }
        if let Some(owner) = state.net_servers.get_mut(&server_id) {
            owner.connections.insert(id);
        }
    }
    set(context, socket, "server", server)?;
    set(context, socket, "_server", server)?;
    emit(context, server, "connection", &[socket])?;
    let message = context.object_rooted()?;
    let key = context.string_rooted("socket");
    context.define_data_property_rooted(message, key, socket, true, true, true)?;
    crate::modules::diagnostics_channel_shared_vm::publish_named(
        context,
        "net.server.socket",
        message,
    )?;
    Ok(context.undefined())
}

pub(crate) fn get_connections(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args[0];
    let count = get(context, server, "_connections")?;
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("netServerConnectionsResult"),
        args[1],
    )?;
    let null = context.null();
    let undefined = context.undefined();
    super::shared_event_loop::queue_next_tick(context, callback, undefined, &[null, count])?;
    Ok(server)
}
pub(crate) fn connections_result(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = context.host_function_data()?;
    let undefined = context.undefined();
    context.call_rooted(callback, undefined, args)
}

/// Match Socket._destroy's synchronous decrement after its original callback.
pub(crate) fn socket_destroyed(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    id: Option<u64>,
) -> Result<(), RootedError> {
    let server = get(context, socket, "_server")?;
    if context.truthy_rooted(server)? {
        let server = get(context, socket, "_server")?;
        let count = get(context, server, "_connections")?;
        let count = context.numeric_update_rooted(count, true)?;
        set(context, server, "_connections", count)?;
        if let Some(id) = id {
            let shared = context.host_mut().shared_state();
            for owner in shared.borrow_mut().net_servers.values_mut() {
                owner.connections.remove(&id);
            }
        }
        let server = get(context, socket, "_server")?;
        let drained = get(context, server, "_emitCloseIfDrained")?;
        if context.truthy_rooted(drained)? {
            let server = get(context, socket, "_server")?;
            let drained = get(context, server, "_emitCloseIfDrained")?;
            context.call_rooted(drained, server, &[])?;
        }
    }
    Ok(())
}
