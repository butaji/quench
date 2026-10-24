//! Net lookup callbacks retain endpoint ownership before requesting OS connect.
use super::event_constructor_shared_vm::get;
use super::net_shared_vm;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const SINGLE_CALLBACK: &str = quench_js_check::checked_js!(
    r#"native => function emitLookup(err, ip, addressType) { 'use strict'; return native(err, ip, addressType); }"#
);
const MULTIPLE_CALLBACK: &str = quench_js_check::checked_js!(
    r#"native => function emitLookup(err, addresses) { 'use strict'; return native(err, addresses); }"#
);

const IPV4_FAMILY: f64 = super::net_config::IPV4_FAMILY as f64;
const IPV6_FAMILY: f64 = super::net_config::IPV6_FAMILY as f64;

fn is_ip_family(family: Option<f64>) -> bool {
    family.is_some_and(|value| value == IPV4_FAMILY || value == IPV6_FAMILY)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

pub(super) fn start(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    host: RootId,
    port: RootId,
    options: RootId,
) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    let id =
        net_shared_vm::reserve_lookup(&mut state.borrow_mut().tcp).map_err(RootedError::host)?;
    net_shared_vm::register_socket(context, socket, id)?;
    let undefined = context.undefined();
    let has_options = context.is_object_rooted(options)?;
    let family = if has_options {
        get(context, options, "family")?
    } else {
        undefined
    };
    let family = match context.string_text(family)?.as_deref() {
        Some("IPv4") => context.number(IPV4_FAMILY),
        Some("IPv6") => context.number(IPV6_FAMILY),
        _ => family,
    };
    let family_number = context
        .rooted_value(family)
        .and_then(|value| value.as_number());
    let local_address = if has_options {
        get(context, options, "localAddress")?
    } else {
        undefined
    };
    let auto = super::net_options_shared_vm::connect_family_options(context, options)?;
    let all = !is_ip_family(family_number) && !context.truthy_rooted(local_address)? && auto;
    let dns_options = context.object_rooted()?;
    set(context, dns_options, "family", family)?;
    let hints = if has_options {
        get(context, options, "hints")?
    } else {
        undefined
    };
    let mut hints = if context.truthy_rooted(hints)? {
        hints
    } else {
        context.number(0.0)
    };
    if !is_ip_family(family_number)
        && context
            .rooted_value(hints)
            .and_then(|value| value.as_number())
            == Some(0.0)
    {
        #[cfg(unix)]
        {
            hints = context.number(f64::from(libc::AI_ADDRCONFIG));
        }
    }
    set(context, dns_options, "hints", hints)?;
    if all {
        let yes = context.boolean(true);
        set(context, dns_options, "all", yes)?;
    }
    let lookup = if has_options {
        get(context, options, "lookup")?
    } else {
        undefined
    };
    let lookup = if context.truthy_rooted(lookup)? {
        lookup
    } else {
        let dns = crate::host::shared_vm::commonjs::dns_module(context)?;
        get(context, dns, "lookup")?
    };
    let owner = context.object_rooted()?;
    let id_root = context.number(id as f64);
    let all_root = context.boolean(all);
    for (name, value) in [
        ("id", id_root),
        ("socket", socket),
        ("host", host),
        ("port", port),
        ("options", options),
        ("all", all_root),
    ] {
        set(context, owner, name, value)?;
    }
    let callback = context
        .host_function_with_data(crate::host::shared_vm::operation("netLookupResult"), owner)?;
    let factory = context.evaluate_script_rooted(
        if all {
            MULTIPLE_CALLBACK
        } else {
            SINGLE_CALLBACK
        },
        "node:net/lookup-callback.js",
    )?;
    let callback = context.call_rooted(factory, undefined, &[callback])?;
    let name = context.string_rooted("_host");
    context.set_property_rooted(socket, name, host, socket)?;
    context.call_rooted(lookup, undefined, &[host, dns_options, callback])?;
    Ok(socket)
}

fn active(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    id: u64,
) -> Result<bool, RootedError> {
    let state = context.host_mut().shared_state();
    if !net_shared_vm::is_looking_up(&state.borrow().tcp, id) {
        return Ok(false);
    }
    let connecting = get(context, socket, "connecting")?;
    context.truthy_rooted(connecting)
}
fn emit_lookup(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    error: RootId,
    ip: RootId,
    family: RootId,
    host: RootId,
) -> Result<(), RootedError> {
    let emit = get(context, socket, "emit")?;
    let event = context.string_rooted("lookup");
    context.call_rooted(emit, socket, &[event, error, ip, family, host])?;
    Ok(())
}
fn fail(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    error: RootId,
) -> Result<RootId, RootedError> {
    let callback = context.host_function(crate::host::shared_vm::operation("netLookupError"))?;
    let undefined = context.undefined();
    super::shared_event_loop::queue_next_tick(context, callback, undefined, &[socket, error])?;
    Ok(undefined)
}
fn detail(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<String, RootedError> {
    if context.is_symbol_rooted(value)? {
        let inspect = super::util_shared_vm::internal_inspect(context)?;
        let undefined = context.undefined();
        let text = context.call_rooted(inspect, undefined, &[value])?;
        context.to_string(text)
    } else {
        context.to_string(value)
    }
}
fn invalid_ip(
    context: &mut NativeContext<'_, NodeHost>,
    ip: RootId,
) -> Result<RootId, RootedError> {
    let ip = if context.is_object_rooted(ip)? && !context.is_callable_rooted(ip)? {
        let inspect = super::util_shared_vm::internal_inspect(context)?;
        let options = context.object_rooted()?;
        let depth = context.number(0.0);
        set(context, options, "depth", depth)?;
        let undefined = context.undefined();
        let text = context.call_rooted(inspect, undefined, &[ip, options])?;
        context.to_string(text)?
    } else {
        detail(context, ip)?
    };
    let error = context.type_error_rooted(&format!("Invalid IP address: {ip}"))?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_INVALID_IP_ADDRESS")?;
    Ok(error)
}
fn invalid_family(
    context: &mut NativeContext<'_, NodeHost>,
    owner: RootId,
    family: RootId,
) -> Result<RootId, RootedError> {
    let options = get(context, owner, "options")?;
    let (host, port) = if context.is_object_rooted(options)? {
        (
            get(context, options, "host")?,
            get(context, options, "port")?,
        )
    } else {
        (get(context, owner, "host")?, get(context, owner, "port")?)
    };
    let family_text = detail(context, family)?;
    let host_text = detail(context, host)?;
    let port_text = detail(context, port)?;
    let error = context.range_error_rooted(&format!(
        "Invalid address family: {family_text} {host_text}:{port_text}"
    ))?;
    super::node_errors_shared_vm::decorate_range_error(
        context,
        error,
        "ERR_INVALID_ADDRESS_FAMILY",
    )?;
    set(context, error, "host", host)?;
    set(context, error, "port", port)?;
    Ok(error)
}

pub(crate) fn result(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let owner = context.host_function_data()?;
    let socket = get(context, owner, "socket")?;
    let host = get(context, owner, "host")?;
    let id_root = get(context, owner, "id")?;
    let id = context
        .rooted_value(id_root)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("lookup owner missing"))? as u64;
    let all_root = get(context, owner, "all")?;
    let all = context.truthy_rooted(all_root)?;
    let undefined = context.undefined();
    let error = args.first().copied().unwrap_or(undefined);
    let ip = args.get(1).copied().unwrap_or(undefined);
    let family = args.get(2).copied().unwrap_or(undefined);
    if all {
        if !active(context, socket, id)? {
            return Ok(undefined);
        }
        if context.truthy_rooted(error)? {
            emit_lookup(context, socket, error, undefined, undefined, host)?;
            return fail(context, socket, error);
        }
    } else {
        emit_lookup(context, socket, error, ip, family, host)?;
        if !active(context, socket, id)? {
            return Ok(undefined);
        }
        if context.truthy_rooted(error)? {
            return fail(context, socket, error);
        }
    }
    let mut destinations = Vec::new();
    let mut first = None;
    if all {
        let length = get(context, ip, "length")?;
        let zero = context.number(0.0);
        let length = context.subtract_rooted(length, zero)?;
        let length = context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .unwrap_or(0.0)
            .max(0.0) as usize;
        for index in 0..length {
            let address = get(context, ip, &index.to_string())?;
            let value = get(context, address, "address")?;
            let family = get(context, address, "family")?;
            first.get_or_insert((value, family));
            emit_lookup(context, socket, error, value, family, host)?;
            if !active(context, socket, id)? {
                return Ok(undefined);
            }
            if let Some(text) = context
                .string_text(value)?
                .filter(|text| net_shared_vm::parse_ip(text).is_some())
            {
                let family_number = context
                    .rooted_value(family)
                    .and_then(|value| value.as_number());
                if is_ip_family(family_number) && !destinations.iter().any(|(old, _)| old == &text)
                {
                    destinations.push((text, family_number.expect("validated family")));
                }
            }
        }
    } else {
        first = Some((ip, family));
        if let Some(text) = context
            .string_text(ip)?
            .filter(|text| net_shared_vm::parse_ip(text).is_some())
        {
            let family_number = context
                .rooted_value(family)
                .and_then(|value| value.as_number());
            if is_ip_family(family_number) {
                destinations.push((text, family_number.expect("validated family")));
            }
        }
    }
    if destinations.is_empty() {
        let (ip, family) = if let Some(first) = first {
            first
        } else {
            let first = get(context, ip, "0")?;
            (
                get(context, first, "address")?,
                get(context, first, "family")?,
            )
        };
        let valid_ip = context
            .string_text(ip)?
            .is_some_and(|text| net_shared_vm::parse_ip(&text).is_some());
        let error = if valid_ip {
            invalid_family(context, owner, family)?
        } else {
            invalid_ip(context, ip)?
        };
        return fail(context, socket, error);
    }
    // Group preference is determined by the first valid callback result.
    let preferred = destinations[0].1;
    let mut preferred_group = destinations
        .iter()
        .filter(|(_, family)| *family == preferred);
    let mut other_group = destinations
        .iter()
        .filter(|(_, family)| *family != preferred);
    let mut addresses = Vec::new();
    loop {
        let first = preferred_group.next();
        let second = other_group.next();
        if first.is_none() && second.is_none() {
            break;
        }
        if let Some((text, _)) = first {
            addresses.push(text.clone());
        }
        if let Some((text, _)) = second {
            addresses.push(text.clone());
        }
    }
    let port = get(context, owner, "port")?;
    let port = context
        .rooted_value(port)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("lookup port missing"))? as u16;
    let options = get(context, owner, "options")?;
    let (local_address, local_port) =
        super::net_options_shared_vm::connect_source(context, options)?;
    let state = context.host_mut().shared_state();
    net_shared_vm::connect_resolved(
        &mut state.borrow_mut().tcp,
        id,
        addresses,
        port,
        local_address,
        local_port,
    )
    .map_err(RootedError::host)?;
    Ok(undefined)
}

pub(crate) fn error(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let error = args[1];
    let destroy = get(context, socket, "destroy")?;
    context.call_rooted(destroy, socket, &[error])?;
    Ok(context.undefined())
}

pub(crate) fn get_default(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let enabled = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_auto_select_family;
    Ok(context.boolean(enabled))
}
pub(crate) fn set_default(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(enabled) = context
        .rooted_value(value)
        .and_then(|value| value.as_bool())
    else {
        let error = super::util_shared_vm::argument_type_error(context, value, "value", "boolean")?;
        return Err(super::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    };
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_auto_select_family = enabled;
    Ok(context.undefined())
}
