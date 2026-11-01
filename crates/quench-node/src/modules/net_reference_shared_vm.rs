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
    let handle = create_handle(context, id, kind, Some(object))?;
    // Public net sockets are backed directly by Quench's TCP transport rather
    // than a tcp_wrap constructor. Give the first TCP handle for an endpoint
    // the same AsyncWrap identity so socket callbacks inherit ALS state and
    // expose a stable getAsyncId() value. Accepted handles that already got
    // initialized with their listener trigger keep that original resource.
    if matches!(kind, EndpointKind::Socket)
        && super::net_shared_vm::socket_is_tcp(&context.host_mut().shared_state().borrow().tcp, id)
        && super::native_async_wrap::tcp_resource(&context.host_mut().shared_state(), id).is_none()
    {
        super::native_async_wrap::initialize_tcp(context, handle, id, false)?;
    }
    set(context, object, "_handle", handle)
}

/// An accepted native endpoint has no Socket owner until admission succeeds.
pub(crate) fn create_socket_handle(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context
        .rooted_value(args[0])
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("accepted endpoint missing"))? as u64;
    let parent = if let Some(parent) = args.get(1) {
        super::native_async_wrap::view(context, *parent)?
    } else {
        super::native_async_wrap::View::NotNative
    };
    let shared = context.host_mut().shared_state();
    let _trigger = if let super::native_async_wrap::View::Active(parent) = parent {
        Some(super::async_hooks_shared_vm::enter_default_trigger(
            &shared,
            parent.id as f64,
        ))
    } else {
        None
    };
    let handle = create_handle(context, id, EndpointKind::Socket, None)?;
    if _trigger.is_some() {
        super::native_async_wrap::initialize_tcp(context, handle, id, false)?;
    }
    Ok(handle)
}

fn create_handle(
    context: &mut NativeContext<'_, NodeHost>,
    id: u64,
    kind: EndpointKind,
    object: Option<RootId>,
) -> Result<RootId, RootedError> {
    let is_socket = context.boolean(matches!(kind, EndpointKind::Socket));
    let reusable = if matches!(kind, EndpointKind::Socket) {
        if let Some(object) = object {
            super::tls_shared_vm::handle::reusable(context, object)?
        } else {
            None
        }
    } else {
        None
    };
    let handle = if let Some(handle) = reusable {
        handle
    } else {
        context.object_rooted()?
    };
    if let Some(object) = object {
        let owner_key = owner_symbol(context)?;
        context.set_property_rooted(handle, owner_key, object, handle)?;
    }
    let id_root = context.number(id as f64);
    if matches!(kind, EndpointKind::Socket) {
        let shared = context.host_mut().shared_state();
        let first_handle = !shared
            .borrow()
            .net_handle_identities
            .iter()
            .any(|(_, existing)| *existing == id);
        if first_handle {
            super::net_shared_vm::stop_read(&mut shared.borrow_mut().tcp, id);
        } else {
            // A TLS wrapper inherits its original endpoint's existing request.
            let active = super::net_shared_vm::read_active(&shared.borrow().tcp, id);
            let reading = context.boolean(active);
            set(context, handle, "reading", reading)?;
        }
    }
    register_identity(context, handle, id)?;
    let tcp_listener = matches!(kind, EndpointKind::Listener)
        && super::net_shared_vm::listener_is_tcp(
            &context.host_mut().shared_state().borrow().tcp,
            id,
        );
    if tcp_listener
        || (matches!(kind, EndpointKind::Socket)
            && super::net_shared_vm::socket_is_tcp(
                &context.host_mut().shared_state().borrow().tcp,
                id,
            ))
    {
        let prototype = super::tcp_binding::prototype(context)?;
        context.set_prototype_rooted(handle, prototype)?;
    }
    // Keep ref/unref inherited from TCPWrap's native prototype. Defining
    // endpoint-specific own methods here bypasses prototype overrides used by
    // Node internals and by applications instrumenting TCPWrap. The native
    // methods resolve the stable handle identity and route to the same
    // transport reference state for native and public net.Socket endpoints.
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
        for (name, operation) in [
            ("setNoDelay", "netHandleSetNoDelay"),
            ("setKeepAlive", "netHandleSetKeepAlive"),
            ("setTypeOfService", "netHandleSetTypeOfService"),
            ("getTypeOfService", "netHandleGetTypeOfService"),
            ("getsockname", "netHandleGetSockName"),
            ("getpeername", "netHandleGetPeerName"),
        ] {
            let method = context
                .host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
            let key = context.string_rooted("name");
            let name_root = context.string_rooted(name);
            context.define_data_property_rooted(method, key, name_root, false, false, true)?;
            set(context, handle, name, method)?;
        }
    }

    if matches!(kind, EndpointKind::Socket) {
        for (name, operation) in [
            ("readStart", "netHandleReadStart"),
            ("readStop", "netHandleReadStop"),
            ("hasPendingRead", "netHandleHasPendingRead"),
        ] {
            let method = context
                .host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
            let key = context.string_rooted("name");
            let name_root = context.string_rooted(name);
            context.define_data_property_rooted(method, key, name_root, false, false, true)?;
            set(context, handle, name, method)?;
        }
        let method = context
            .host_function_with_data(crate::host::shared_vm::operation("netHandleClose"), data)?;
        let key = context.string_rooted("name");
        let name = context.string_rooted("close");
        context.define_data_property_rooted(method, key, name, false, false, true)?;
        set(context, handle, "close", method)?;
    }
    if matches!(kind, EndpointKind::Listener) {
        let method = on_connection(context)?;
        set(context, handle, "onconnection", method)?;
    }
    Ok(handle)
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
        EndpointKind::Socket => {
            Some(super::net_shared_vm::socket_has_ref(&state.borrow().tcp, id).unwrap_or(false))
        }
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
fn tcp_option_owner(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
) -> Result<Option<u64>, RootedError> {
    let data = context.host_function_data()?;
    let handle = get(context, data, "1")?;
    if !context.strict_equal_rooted(receiver, handle)? {
        return Ok(None);
    }
    let id = get(context, data, "0")?;
    Ok(context
        .rooted_value(id)
        .and_then(|value| value.as_number())
        .map(|id| id as u64))
}
fn option_result(context: &mut NativeContext<'_, NodeHost>, result: std::io::Result<()>) -> RootId {
    let code = result
        .err()
        .map_or(0, |error| -error.raw_os_error().unwrap_or(libc::EINVAL));
    context.number(f64::from(code))
}
fn unsigned_argument(context: &NativeContext<'_, NodeHost>, root: Option<RootId>) -> Option<u32> {
    root.and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|number| {
            number.is_finite()
                && number.fract() == 0.0
                && (0.0..=f64::from(u32::MAX)).contains(number)
        })
        .map(|number| number as u32)
}
pub(crate) fn set_no_delay(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else {
        return Ok(context.number(-f64::from(libc::EBADF)));
    };
    let enabled = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        == Some(true);
    let shared = context.host_mut().shared_state();
    let result = super::net_shared_vm::tcp_no_delay(&mut shared.borrow_mut().tcp, id, enabled);
    Ok(option_result(context, result))
}
pub(crate) fn set_keep_alive(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else {
        return Ok(context.number(-f64::from(libc::EBADF)));
    };
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let mask = context.number(super::net_socket_options::SIGNED_INTEGER_MASK);
    let enabled =
        context.arithmetic_rooted(quench_runtime::NativeArithmetic::BitwiseAnd, input, mask)?;
    let enabled = context
        .rooted_value(enabled)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0)
        != 0.0;
    let delay = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as i32 as u32;
    let options = super::net_shared_vm::TcpKeepAlive {
        enabled,
        delay,
        interval: unsigned_argument(context, args.get(2).copied())
            .unwrap_or(DEFAULT_KEEPALIVE_INTERVAL_SECONDS),
        count: unsigned_argument(context, args.get(3).copied())
            .unwrap_or(DEFAULT_KEEPALIVE_PROBE_COUNT),
    };
    let shared = context.host_mut().shared_state();
    let result = super::net_shared_vm::tcp_keep_alive(&mut shared.borrow_mut().tcp, id, options);
    Ok(option_result(context, result))
}

pub(crate) fn set_type_of_service(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else {
        return Ok(context.number(-f64::from(libc::EBADF)));
    };
    let tos = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && value.fract() == 0.0 && (0.0..=255.0).contains(value))
        .unwrap_or(0.0) as u8;
    let shared = context.host_mut().shared_state();
    let result =
        super::net_shared_vm::tcp_set_type_of_service(&mut shared.borrow_mut().tcp, id, tos);
    Ok(option_result(context, result))
}

pub(crate) fn get_type_of_service(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else {
        return Ok(context.number(0.0));
    };
    let shared = context.host_mut().shared_state();
    let value =
        super::net_shared_vm::tcp_get_type_of_service(&shared.borrow().tcp, id).unwrap_or(0);
    Ok(context.number(f64::from(value)))
}

pub(crate) fn owner_symbol(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let existing = shared.borrow().net_handle_owner_symbol;
    if let Some(symbol) = existing {
        return Ok(symbol);
    }
    let symbol = context.symbol_rooted(Some("owner_symbol"));
    let retained = context.retain(symbol)?;
    shared.borrow_mut().net_handle_owner_symbol = Some(retained);
    Ok(symbol)
}

pub(crate) fn register_identity(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
    id: u64,
) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().net_handle_identities);
    let mut live = Vec::with_capacity(entries.len() + 1);
    for (identity, id) in entries {
        if context.resolve_host_weak_root(identity).is_some() {
            live.push((identity, id));
        } else {
            context.release_host_weak_root(identity);
        }
    }
    live.push((context.host_weak_root(handle)?, id));
    shared.borrow_mut().net_handle_identities = live;
    Ok(())
}

pub(crate) fn optional_handle_id(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<Option<u64>, RootedError> {
    let entries = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_handle_identities
        .clone();
    for (identity, id) in entries {
        if let Some(original) = context.resolve_host_weak_root(identity) {
            if context.strict_equal_rooted(original, handle)? {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}
pub(crate) fn handle_id(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<u64, RootedError> {
    optional_handle_id(context, handle)?
        .ok_or_else(|| RootedError::host("native endpoint identity missing"))
}

pub(crate) fn get_sock_name(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    socket_name(context, receiver, args, false)
}
pub(crate) fn get_peer_name(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    socket_name(context, receiver, args, true)
}
fn socket_name(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
    peer: bool,
) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else {
        return Ok(context.number(-f64::from(libc::EBADF)));
    };
    let shared = context.host_mut().shared_state();
    let address = super::net_shared_vm::socket_address(&shared.borrow().tcp, id, peer);
    match address {
        Ok(address) => {
            let out = args.first().copied().unwrap_or_else(|| context.undefined());
            let value = context.string_rooted(&address.ip().to_string());
            set(context, out, "address", value)?;
            let value = context.string_rooted(if address.is_ipv4() { "IPv4" } else { "IPv6" });
            set(context, out, "family", value)?;
            let value = context.number(f64::from(address.port()));
            set(context, out, "port", value)?;
            Ok(context.number(0.0))
        }
        Err(error) => Ok(option_result(context, Err(error))),
    }
}
pub(crate) fn close(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, _, _) = owner(context, receiver)?;
    let shared = context.host_mut().shared_state();
    if !super::net_shared_vm::socket_is_open(&shared.borrow().tcp, id) {
        return Ok(context.undefined());
    }
    let key = onclose_symbol(context)?;
    if let Some(callback) = args.first().copied() {
        if context.is_callable_rooted(callback)? {
            context.set_property_rooted(receiver, key, callback, receiver)?;
        }
    }
    let handle = context.retain(receiver)?;
    shared
        .borrow_mut()
        .net_handle_close_callbacks
        .insert(id, handle);
    super::stream_binding::release_reader(context, id);
    super::net_shared_vm::close_socket(&mut shared.borrow_mut().tcp, id);
    Ok(context.undefined())
}

pub(crate) fn onclose_symbol(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if let Some(symbol) = shared.borrow().net_handle_onclose_symbol {
        return Ok(symbol);
    }
    let symbol = context.symbol_rooted(Some("handle_onclose"));
    let retained = context.retain(symbol)?;
    shared.borrow_mut().net_handle_onclose_symbol = Some(retained);
    Ok(symbol)
}

fn on_connection(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if let Some(callback) = shared.borrow().net_server_on_connection {
        return Ok(callback);
    }
    let operation =
        context.host_function(crate::host::shared_vm::operation("netServerOnConnection"))?;
    let factory = context.evaluate_script_rooted(quench_js_check::checked_js!(
        "operation=>function onconnection(err,clientHandle){'use strict';return operation(this,err,clientHandle);}"
    ), "node:net/onconnection.js")?;
    let undefined = context.undefined();
    let callback = context.call_rooted(factory, undefined, &[operation])?;
    let retained = context.retain(callback)?;
    shared.borrow_mut().net_server_on_connection = Some(retained);
    Ok(callback)
}

pub(crate) fn prune(runtime: &mut quench_runtime::Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().net_handle_identities);
    let mut live = Vec::with_capacity(entries.len());
    let mut dead = Vec::new();
    for (identity, id) in entries {
        if runtime.host_weak_root_is_live(identity) {
            live.push((identity, id));
        } else {
            runtime.release_host_weak_root(identity);
            dead.push(id);
        }
    }
    for id in dead {
        if !live.iter().any(|(_, existing)| *existing == id) {
            super::net_shared_vm::native_tcp_close(&mut shared.borrow_mut().tcp, id);
            super::native_async_wrap::collect_tcp(runtime, id);
        }
    }
    shared.borrow_mut().net_handle_identities = live;
}

/// Resolve the original mutable callback only after physical close completes.
pub(crate) fn close_complete(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let key = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_handle_onclose_symbol
        .ok_or_else(|| RootedError::host("close callback key missing"))?;
    let id = handle_id(context, receiver)?;
    let result = (|| {
        if context.has_property_rooted(receiver, key)? {
            let callback = context.get_property_rooted(receiver, key)?;
            if context.is_callable_rooted(callback)? {
                return context.call_rooted(callback, receiver, &[]);
            }
        }
        Ok(context.undefined())
    })();
    super::stream_binding::release_reader(context, id);
    let socket = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_sockets
        .remove(&id);
    if let Some(socket) = socket {
        context.release_root(socket.root);
    }
    result
}

pub(crate) fn read_start(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else {
        return Ok(context.number(-f64::from(libc::EBADF)));
    };
    let shared = context.host_mut().shared_state();
    let result = super::net_shared_vm::start_read(&mut shared.borrow_mut().tcp, id);
    if result.is_ok() {
        super::stream_binding::retain_reader(context, id, receiver)?;
    }
    Ok(option_result(context, result))
}
pub(crate) fn read_stop(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = tcp_option_owner(context, receiver)? else {
        return Ok(context.number(-f64::from(libc::EBADF)));
    };
    super::net_shared_vm::stop_read(&mut context.host_mut().shared_state().borrow_mut().tcp, id);
    super::stream_binding::release_reader(context, id);
    Ok(context.number(0.0))
}

pub(crate) fn has_pending_read(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let pending = tcp_option_owner(context, receiver)?.is_some_and(|id| {
        super::net_shared_vm::socket_has_pending_read(
            &context.host_mut().shared_state().borrow().tcp,
            id,
        )
    });
    Ok(context.boolean(pending))
}
