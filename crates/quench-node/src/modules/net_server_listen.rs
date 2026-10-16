//! net.Server listen generations and DNS callbacks share the public Node lifecycle.
use super::event_constructor_shared_vm::get;
use super::net_shared_vm;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

type Context<'a> = NativeContext<'a, NodeHost>;

fn once(context: &mut Context<'_>, server: RootId, event: &str, callback: RootId) -> Result<(), RootedError> {
    let method = get(context, server, "once")?;
    let event = context.string_rooted(event);
    context.call_rooted(method, server, &[event, callback])?;
    Ok(())
}
fn emit(context: &mut Context<'_>, server: RootId, event: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let method = get(context, server, "emit")?;
    let event = context.string_rooted(event);
    let mut values = vec![event];
    values.extend_from_slice(args);
    context.call_rooted(method, server, &values)
}
fn advance(context: &mut Context<'_>, server: RootId) -> Result<RootId, RootedError> {
    let previous = get(context, server, "_listeningId")?;
    let generation = context.numeric_update_rooted(previous, false)?;
    context.assign_named_property_rooted(server, "_listeningId", generation)?;
    Ok(generation)
}
fn original_number(context: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let number = context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        get(context, global, "Number")
    })?;
    let undefined = context.undefined();
    context.call_rooted(number, undefined, &[value])
}
fn positional_backlog(context: &mut Context<'_>, args: &[RootId]) -> Result<RootId, RootedError> {
    for index in [1, 2] {
        let value = args.get(index).copied().unwrap_or_else(|| context.boolean(false));
        let number = original_number(context, value)?;
        if context.rooted_value(number).and_then(|value| value.as_number()).is_some_and(|number| number > 0.0) {
            return Ok(number);
        }
    }
    Ok(context.undefined())
}

pub(crate) fn install(context: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    let getter = context.host_constructor_named(crate::host::shared_vm::operation("netServerListening"), "get")?;
    let descriptor = context.null_object_rooted()?;
    let key = context.string_rooted("get");
    context.define_data_property_rooted(descriptor, key, getter, true, true, true)?;
    for name in ["enumerable", "configurable"] {
        let key = context.string_rooted(name);
        let yes = context.boolean(true);
        context.define_data_property_rooted(descriptor, key, yes, true, true, true)?;
    }
    let key = context.string_rooted("listening");
    context.define_property_descriptor_rooted(prototype, key, descriptor)?;
    Ok(())
}

pub(crate) fn listening(context: &mut Context<'_>, server: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let handle = get(context, server, "_handle")?;
    let active = context.truthy_rooted(handle)?;
    Ok(context.boolean(active))
}

pub(crate) fn listen(context: &mut Context<'_>, server: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let handle = get(context, server, "_handle")?;
    if context.truthy_rooted(handle)? {
        let error = context.error_rooted("Listen method has been called more than once without closing.")?;
        super::node_errors_shared_vm::decorate_error(context, error, "ERR_SERVER_ALREADY_LISTEN")?;
        return Err(context.throw(error));
    }
    let undefined = context.undefined();
    let first = args.first().copied().unwrap_or(undefined);
    let first = if context.is_callable_rooted(first)? { context.number(0.0) } else { first };
    let second = args.get(1).copied().unwrap_or(undefined);
    let host = if context.string_text(second)?.is_some() { second } else { undefined };
    let callback = args.last().copied().filter(|root| context.is_callable_rooted(*root).unwrap_or(false)).unwrap_or(undefined);
    let no = context.boolean(false);
    let plan = super::net_endpoint_shared_vm::normalize(context, undefined, &[first, host, callback, no])?;
    if context.is_callable_rooted(callback)? { once(context, server, "listening", callback)?; }
    let fallback = positional_backlog(context, args)?;
    let options = get(context, plan, "options")?;
    let reuse_port = if context.is_object_rooted(options)? {
        let requested = get(context, options, "reusePort")?;
        context.truthy_rooted(requested)?
    } else { false };
    let mut backlog = if context.is_object_rooted(options)? { get(context, options, "backlog")? } else { undefined };
    if !context.truthy_rooted(backlog)? { backlog = fallback; }
    if !context.truthy_rooted(backlog)? { backlog = context.number(f64::from(net_shared_vm::DEFAULT_TCP_LISTEN_BACKLOG)); }
    let generation = advance(context, server)?;
    let address = get(context, plan, "address")?;
    #[cfg(unix)]
    if let Some(path) = context.string_text(address)? {
        return net_shared_vm::listen_local_server(context, server, &path);
    }
    #[cfg(not(unix))]
    if context.string_text(address)?.is_some() {
        return Err(RootedError::host("invalid TCP port"));
    }
    let port = context.rooted_value(address).and_then(|value| value.as_number()).unwrap_or(0.0) as u16;
    let host = get(context, plan, "host")?;
    if context.truthy_rooted(host)? {
        let data = context.null_object_rooted()?;
        let port = context.number(f64::from(port));
        let reuse_port = context.boolean(reuse_port);
        for (name, value) in [("server", server), ("generation", generation), ("backlog", backlog), ("port", port), ("reusePort", reuse_port)] {
            let key = context.string_rooted(name);
            context.define_data_property_rooted(data, key, value, true, true, true)?;
        }
        let callback = context.host_function_named_with_data(crate::host::shared_vm::operation("netServerLookupCompleted"), "", data)?;
        let dns = crate::host::shared_vm::commonjs::dns_module(context)?;
        let lookup = get(context, dns, "lookup")?;
        let options = context.object_rooted()?;
        let key = context.string_rooted("all");
        let yes = context.boolean(true);
        context.define_data_property_rooted(options, key, yes, true, true, true)?;
        context.call_rooted(lookup, dns, &[host, options, callback])?;
        return Ok(server);
    }
    let backlog = net_shared_vm::convert_listen_backlog(context, backlog)?;
    let address = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port);
    let shared = context.host_mut().shared_state();
    let listener = net_shared_vm::listen_with_reuse_port(&mut shared.borrow_mut().tcp, address, reuse_port, backlog);
    let listener = match listener {
        Err(_) => net_shared_vm::listen_with_reuse_port(&mut shared.borrow_mut().tcp, SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port), reuse_port, backlog),
        result => result,
    }.map_err(RootedError::host)?;
    net_shared_vm::register_server(context, server, listener)
}

pub(crate) fn lookup_completed(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let server = get(context, data, "server")?;
    let generation = get(context, data, "generation")?;
    let current = get(context, server, "_listeningId")?;
    if !context.strict_equal_rooted(generation, current)? { return Ok(context.undefined()); }
    let error = args.first().copied().unwrap_or_else(|| context.undefined());
    if context.truthy_rooted(error)? { return emit(context, server, "error", &[error]); }
    let addresses = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let iterator = context.iterator_rooted(addresses)?;
    let mut first = None;
    let selected = loop {
        let Some(address) = context.iterator_step_rooted(&iterator)? else { break first; };
        first.get_or_insert(address);
        let text = match get(context, address, "address") {
            Ok(text) => text,
            Err(error) => { let _ = context.iterator_close_rooted(&iterator); return Err(error); }
        };
        let link_local = context.string_text(text)?.and_then(|text| net_shared_vm::parse_ip(&text)).is_some_and(|address| matches!(address, IpAddr::V6(address) if address.is_unicast_link_local()));
        if !link_local { context.iterator_close_rooted(&iterator)?; break Some(address); }
    };
    let selected = selected.unwrap_or_else(|| context.undefined());
    let address = get(context, selected, "address")?;
    let address = context.string_text(address)?.ok_or_else(|| RootedError::host("DNS listen address is not a string"))?;
    let port = get(context, data, "port")?;
    let port = context.rooted_value(port).and_then(|value| value.as_number()).unwrap_or(0.0) as u16;
    if net_shared_vm::parse_ip(&address).is_none() {
        return Err(RootedError::host("DNS listen address is not an IP literal"));
    }
    let address = (address.as_str(), port).to_socket_addrs()
        .map_err(|error| RootedError::host(error.to_string()))?
        .next().ok_or_else(|| RootedError::host("DNS listen address is unavailable"))?;
    let reuse = get(context, data, "reusePort")?;
    let reuse = context.truthy_rooted(reuse)?;
    let backlog = get(context, data, "backlog")?;
    let backlog = net_shared_vm::convert_listen_backlog(context, backlog)?;
    let shared = context.host_mut().shared_state();
    let listener = net_shared_vm::listen_with_reuse_port(&mut shared.borrow_mut().tcp, address, reuse, backlog).map_err(RootedError::host)?;
    net_shared_vm::register_server(context, server, listener)
}

pub(crate) fn close(context: &mut Context<'_>, server: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    advance(context, server)?;
    let handle = get(context, server, "_handle")?;
    let active = context.truthy_rooted(handle)?;
    if let Some(callback) = args.first().copied().filter(|root| context.is_callable_rooted(*root).unwrap_or(false)) {
        let callback = if active { callback } else {
            context.host_constructor_named_with_data(crate::host::shared_vm::operation("netServerCloseCallback"), "close", callback)?
        };
        once(context, server, "close", callback)?;
    }
    let registered = if let Some(id) = net_shared_vm::optional_server_id(context, server)? {
        let shared = context.host_mut().shared_state();
        let mut state = shared.borrow_mut();
        if let Some(listener) = state.net_servers.get_mut(&id).map(|server| {
            server.closing = true;
            server.listening_pending = false;
            server.listener
        }) {
            net_shared_vm::close_listener(&mut state.tcp, listener);
            true
        } else { false }
    } else { false };
    if !registered {
        let callback = context.host_function(crate::host::shared_vm::operation("netServerClosePending"))?;
        let undefined = context.undefined();
        super::shared_event_loop::queue_next_tick(context, callback, undefined, &[server])?;
    }
    let null = context.null();
    context.assign_named_property_rooted(server, "_handle", null)?;
    Ok(server)
}

pub(crate) fn close_pending(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    emit(context, args[0], "close", &[])
}

pub(crate) fn close_callback(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let callback = context.host_function_data()?;
    let error = context.error_rooted("Server is not running.")?;
    super::node_errors_shared_vm::decorate_error(context, error, "ERR_SERVER_NOT_RUNNING")?;
    let undefined = context.undefined();
    context.call_rooted(callback, undefined, &[error])?;
    Ok(context.undefined())
}
