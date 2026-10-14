//! Public net reference requests project onto the owning transport endpoint.
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
#[derive(Clone, Copy)]
pub(crate) enum EndpointKind {
    Listener,
    Socket,
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
    install_handle(context, id, server, EndpointKind::Listener)
}
pub(crate) fn install_socket(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let id = context
        .rooted_value(args[1])
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("socket owner missing"))? as u64;
    install_handle(context, id, socket, EndpointKind::Socket)?;
    Ok(socket)
}
pub(crate) fn install_socket_handle(
    context: &mut NativeContext<'_, NodeHost>,
    id: u64,
    socket: RootId,
) -> Result<(), RootedError> {
    install_handle(context, id, socket, EndpointKind::Socket)
}
fn install_handle(
    context: &mut NativeContext<'_, NodeHost>,
    id: u64,
    object: RootId,
    kind: EndpointKind,
) -> Result<(), RootedError> {
    let is_socket = context.boolean(matches!(kind, EndpointKind::Socket));
    let handle = context.object_rooted()?;
    let owner_key = owner_symbol(context)?;
    context.set_property_rooted(handle, owner_key, object, handle)?;
    let id_root = context.number(id as f64);
    for (name, referenced) in [("ref", true), ("unref", false)] {
        let referenced = context.boolean(referenced);
        let data = context.array_rooted(&[id_root, handle, is_socket, referenced])?;
        let method = context.host_function_with_data(
            crate::host::shared_vm::operation("netListenerReference"),
            data,
        )?;
        let key = context.string_rooted("name");
        let name_root = context.string_rooted(name);
        context.define_data_property_rooted(method, key, name_root, false, false, true)?;
        set(context, handle, name, method)?;
    }
    let data = context.array_rooted(&[id_root, handle, is_socket])?;
    let method = context
        .host_function_with_data(crate::host::shared_vm::operation("netListenerHasRef"), data)?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("hasRef");
    context.define_data_property_rooted(method, key, name, false, false, true)?;
    set(context, handle, "hasRef", method)?;
    if matches!(kind, EndpointKind::Socket)
        && super::net_shared_vm::socket_is_tcp(&context.host_mut().shared_state().borrow().tcp, id)
    {
        for (name, operation) in [("setNoDelay", "netHandleSetNoDelay"), ("setKeepAlive", "netHandleSetKeepAlive")] {
            let method = context.host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
            let key = context.string_rooted("name");
            let name_root = context.string_rooted(name);
            context.define_data_property_rooted(method, key, name_root, false, false, true)?;
            set(context, handle, name, method)?;
        }
    }

    if matches!(kind, EndpointKind::Listener) {
        let data = context.array_rooted(&[object, handle])?;
        let method = context.host_function_with_data(crate::host::shared_vm::operation("netServerOnConnection"), data)?;
        let key = context.string_rooted("name");
        let name = context.string_rooted("onconnection");
        context.define_data_property_rooted(method, key, name, false, false, true)?;
        set(context, handle, "onconnection", method)?;
    }
    set(context, object, "_handle", handle)
}
fn owner(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
) -> Result<(u64, RootId, EndpointKind), RootedError> {
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
    let socket = get(context, data, "2")?;
    let kind = if context.truthy_rooted(socket)? {
        EndpointKind::Socket
    } else {
        EndpointKind::Listener
    };
    Ok((id, data, kind))
}
pub(crate) fn reference_handle(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, data, kind) = owner(context, receiver)?;
    let value = get(context, data, "3")?;
    let referenced = context.truthy_rooted(value)?;
    let state = context.host_mut().shared_state();
    let mut state = state.borrow_mut();
    match kind {
        EndpointKind::Listener => {
            super::net_shared_vm::listener_reference(&mut state.tcp, id, referenced)
        }
        EndpointKind::Socket => {
            super::net_shared_vm::socket_reference(&mut state.tcp, id, referenced)
        }
    }
    Ok(context.undefined())
}
pub(crate) fn handle_has_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, _, kind) = owner(context, receiver)?;
    let state = context.host_mut().shared_state();
    let value = match kind {
        EndpointKind::Listener => super::net_shared_vm::listener_has_ref(&state.borrow().tcp, id),
        EndpointKind::Socket => Some(super::net_shared_vm::socket_has_ref(&state.borrow().tcp, id).unwrap_or(false)),
    };
    Ok(match value {
        Some(value) => context.boolean(value),
        None => context.undefined(),
    })
}

pub(crate) fn reference_socket(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let referenced = context.truthy_rooted(args[1])?;
    let name = if referenced { "ref" } else { "unref" };
    let handle = get(context, socket, "_handle")?;
    if !context.truthy_rooted(handle)? {
        let once = get(context, socket, "once")?;
        let method = get(context, socket, name)?;
        let event = context.string_rooted("connect");
        context.call_rooted(once, socket, &[event, method])?;
        return Ok(socket);
    }
    let handle = get(context, socket, "_handle")?;
    let method = get(context, handle, name)?;
    if context.is_callable_rooted(method)? {
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, name)?;
        context.call_rooted(method, handle, &[])?;
    }
    Ok(socket)
}

/// Stream timeouts observe activity without independently keeping a socket alive.
pub(crate) fn unref_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let timer = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("socket timeout missing"))?;
    super::timers_shared_vm::timer_unref(context, timer, &[])
}


const DEFAULT_KEEPALIVE_INTERVAL_SECONDS: u32 = 1;
const DEFAULT_KEEPALIVE_PROBE_COUNT: u32 = 10;
fn tcp_option_owner(context: &mut NativeContext<'_, NodeHost>, receiver: RootId) -> Result<Option<u64>, RootedError> {
    let data = context.host_function_data()?;
    let handle = get(context, data, "1")?;
    if !context.strict_equal_rooted(receiver, handle)? { return Ok(None); }
    let id = get(context, data, "0")?;
    Ok(context.rooted_value(id).and_then(|value| value.as_number()).map(|id| id as u64))
}
fn option_result(context: &mut NativeContext<'_, NodeHost>, result: std::io::Result<()>) -> RootId {
    let code = result.err().map_or(0, |error| -error.raw_os_error().unwrap_or(libc::EINVAL));
    context.number(f64::from(code))
}
fn unsigned_argument(context: &NativeContext<'_, NodeHost>, root: Option<RootId>) -> Option<u32> {
    root.and_then(|root| context.rooted_value(root)).and_then(|value| value.as_number())
        .filter(|number| number.is_finite() && number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(number))
        .map(|number| number as u32)
}
pub(crate) fn set_no_delay(context: &mut NativeContext<'_, NodeHost>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else { return Ok(context.number(-f64::from(libc::EBADF))); };
    let enabled = args.first().and_then(|root| context.rooted_value(*root)).and_then(|value| value.as_bool()) == Some(true);
    let shared = context.host_mut().shared_state();
    let result = super::net_shared_vm::tcp_no_delay(&mut shared.borrow_mut().tcp, id, enabled);
    Ok(option_result(context, result))
}
pub(crate) fn set_keep_alive(context: &mut NativeContext<'_, NodeHost>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else { return Ok(context.number(-f64::from(libc::EBADF))); };
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let mask = context.number(-1.0);
    let enabled = context.arithmetic_rooted(quench_runtime::NativeArithmetic::BitwiseAnd, input, mask)?;
    let enabled = context.rooted_value(enabled).and_then(|value| value.as_number()).unwrap_or(0.0) != 0.0;
    let delay = args.get(1).and_then(|root| context.rooted_value(*root)).and_then(|value| value.as_number()).unwrap_or(0.0) as i32 as u32;
    let options = super::net_shared_vm::TcpKeepAlive {
        enabled,
        delay,
        interval: unsigned_argument(context, args.get(2).copied()).unwrap_or(DEFAULT_KEEPALIVE_INTERVAL_SECONDS),
        count: unsigned_argument(context, args.get(3).copied()).unwrap_or(DEFAULT_KEEPALIVE_PROBE_COUNT),
    };
    let shared = context.host_mut().shared_state();
    let result = super::net_shared_vm::tcp_keep_alive(&mut shared.borrow_mut().tcp, id, options);
    Ok(option_result(context, result))
}


pub(crate) fn owner_symbol(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let existing = shared.borrow().net_handle_owner_symbol;
    if let Some(symbol) = existing { return Ok(symbol); }
    let symbol = context.symbol_rooted(Some("owner_symbol"));
    let retained = context.retain(symbol)?;
    shared.borrow_mut().net_handle_owner_symbol = Some(retained);
    Ok(symbol)
}
