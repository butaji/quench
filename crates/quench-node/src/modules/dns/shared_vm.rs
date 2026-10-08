//! Shared-VM `dns.lookup` projection over the operating system resolver.

use crate::host::NodeHost;
use crate::modules::shared_event_loop::SharedCallback;
use quench_runtime_next::{NativeContext, RootId, RootedError, Value};
use std::net::{IpAddr, ToSocketAddrs};

const DNS_LOOKUP_PORT: u16 = 0;
const ANY_FAMILY: u8 = 0;
const IPV4_FAMILY: u8 = 4;
const IPV6_FAMILY: u8 = 6;
const DNS_SYSCALL: &str = "getaddrinfo";
const NOT_FOUND_CODE: &str = "ENOTFOUND";

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let lookup = context.host_function(crate::host::shared_vm::operation("dnsLookup"))?;
    set(context, module, "lookup", lookup)?;
    Ok(module)
}

/// Node's callback form of lookup. OS resolution happens here; the callback is
/// retained by the host event loop and runs in the next immediate phase.
pub(crate) fn lookup(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(hostname_root) = args.first().copied() else {
        return throw_type(context, "The \"hostname\" argument must be of type string");
    };
    let Some(hostname) = context.string_text(hostname_root)? else {
        return throw_type(context, "The \"hostname\" argument must be of type string");
    };
    if hostname.is_empty() || hostname.contains('\0') {
        return throw_invalid_value(
            context,
            "The argument 'hostname' must be a string without null bytes.",
        );
    }

    let (family, all, callback) = lookup_arguments(context, args)?;
    let resolved = resolve(&hostname, family);
    let callback_args = match resolved {
        Ok(addresses) if all => success_all(context, addresses)?,
        Ok(addresses) => {
            let first = addresses
                .into_iter()
                .next()
                .expect("successful resolution has at least one address");
            vec![
                context.null(),
                context.string_rooted(&first.to_string()),
                context.number(f64::from(family_of(first))),
            ]
        }
        Err(()) if all => vec![not_found_error(context, &hostname)?],
        Err(()) => vec![
            not_found_error(context, &hostname)?,
            context.undefined(),
            context.undefined(),
        ],
    };

    queue_callback(context, callback, callback_args)?;
    Ok(context.undefined())
}

fn lookup_arguments(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<(u8, bool, RootId), RootedError> {
    let (options, callback) = match args.get(1).copied() {
        Some(callback) if context.is_callable_rooted(callback)? => (None, Some(callback)),
        Some(options) => (Some(options), args.get(2).copied()),
        None => (None, None),
    };
    let Some(callback) = callback else {
        let error = invalid_type_error(
            context,
            "The \"callback\" argument must be of type function",
        )?;
        return Err(context.throw(error));
    };
    if !context.is_callable_rooted(callback)? {
        let error = invalid_type_error(
            context,
            "The \"callback\" argument must be of type function",
        )?;
        return Err(context.throw(error));
    }

    let Some(options) = options else {
        return Ok((ANY_FAMILY, false, callback));
    };
    let option_value = context
        .rooted_value(options)
        .ok_or_else(|| RootedError::host("invalid dns.lookup options root"))?;
    if option_value.is_undefined() || option_value.is_null() {
        return Ok((ANY_FAMILY, false, callback));
    }
    if let Some(number) = option_value.as_number() {
        let family = parse_family(context, number)?;
        return Ok((family, false, callback));
    }
    if !option_value.is_heap() || context.string_text(options)?.is_some() {
        let error = invalid_type_error(
            context,
            "The \"options\" argument must be of type object or number",
        )?;
        return Err(context.throw(error));
    }

    let family_key = context.string_rooted("family");
    let family_value = context.get_property_rooted(options, family_key)?;
    let family_value = context
        .rooted_value(family_value)
        .unwrap_or(Value::UNDEFINED);
    let family = if family_value.is_undefined() {
        ANY_FAMILY
    } else if let Some(number) = family_value.as_number() {
        parse_family(context, number)?
    } else {
        let error = invalid_type_error(
            context,
            "The \"options.family\" property must be of type number",
        )?;
        return Err(context.throw(error));
    };

    let all_key = context.string_rooted("all");
    let all_value = context.get_property_rooted(options, all_key)?;
    let all_value = context.rooted_value(all_value).unwrap_or(Value::UNDEFINED);
    let all = match all_value.as_bool() {
        Some(all) => all,
        None if all_value.is_undefined() => false,
        None => {
            let error = invalid_type_error(
                context,
                "The \"options.all\" property must be of type boolean",
            )?;
            return Err(context.throw(error));
        }
    };
    Ok((family, all, callback))
}

fn parse_family(context: &mut NativeContext<'_, NodeHost>, family: f64) -> Result<u8, RootedError> {
    if !family.is_finite() || family.fract() != 0.0 {
        let error = invalid_value_error(
            context,
            "The property 'options.family' must be one of: 0, 4, 6",
        )?;
        return Err(context.throw(error));
    }
    let family = family as i32;
    match family {
        0 => Ok(ANY_FAMILY),
        4 => Ok(IPV4_FAMILY),
        6 => Ok(IPV6_FAMILY),
        _ => {
            let error = invalid_value_error(
                context,
                "The property 'options.family' must be one of: 0, 4, 6",
            )?;
            Err(context.throw(error))
        }
    }
}

fn resolve(hostname: &str, family: u8) -> Result<Vec<IpAddr>, ()> {
    (hostname, DNS_LOOKUP_PORT)
        .to_socket_addrs()
        .map_err(|_| ())
        .map(|addresses| {
            addresses
                .filter_map(|address| {
                    let ip = address.ip();
                    let address_family = family_of(ip);
                    (family == ANY_FAMILY || family == address_family).then_some(ip)
                })
                .collect::<Vec<IpAddr>>()
        })
        .and_then(|addresses| (!addresses.is_empty()).then_some(addresses).ok_or(()))
}

fn family_of(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => IPV4_FAMILY,
        IpAddr::V6(_) => IPV6_FAMILY,
    }
}

fn success_all(
    context: &mut NativeContext<'_, NodeHost>,
    addresses: Vec<IpAddr>,
) -> Result<Vec<RootId>, RootedError> {
    let mut values = Vec::with_capacity(addresses.len());
    for address in addresses {
        let item = context.object_rooted()?;
        set_str(context, item, "address", &address.to_string())?;
        set_number(context, item, "family", f64::from(family_of(address)))?;
        values.push(item);
    }
    let array = context.array_rooted(&values)?;
    Ok(vec![context.null(), array])
}

fn not_found_error(
    context: &mut NativeContext<'_, NodeHost>,
    hostname: &str,
) -> Result<RootId, RootedError> {
    let error = context.error_rooted(&format!("{DNS_SYSCALL} ENOTFOUND {hostname}"))?;
    set_str(context, error, "code", NOT_FOUND_CODE)?;
    set_str(context, error, "syscall", DNS_SYSCALL)?;
    set_str(context, error, "hostname", hostname)?;
    Ok(error)
}

fn queue_callback(
    context: &mut NativeContext<'_, NodeHost>,
    callback: RootId,
    args: Vec<RootId>,
) -> Result<(), RootedError> {
    let state = context.host_mut().shared_state();
    let id = state
        .borrow_mut()
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("shared immediate identifier space exhausted"))?;
    let callback = context.retain(callback)?;
    let undefined = context.undefined();
    let receiver = context.retain(undefined)?;
    let args = args
        .into_iter()
        .map(|argument| context.retain(argument))
        .collect::<Result<Vec<_>, _>>()?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_immediate(
            id,
            crate::modules::shared_event_loop::SharedImmediateOwner::Host,
            SharedCallback {
                callback,
                receiver,
                args,
            },
        );
    Ok(())
}

fn invalid_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(message)?;
    set_str(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(error)
}

fn invalid_value_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(message)?;
    set_str(context, error, "code", "ERR_INVALID_ARG_VALUE")?;
    Ok(error)
}

fn throw_type<T>(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<T, RootedError> {
    let error = invalid_type_error(context, message)?;
    Err(context.throw(error))
}

fn throw_invalid_value<T>(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<T, RootedError> {
    let error = invalid_value_error(context, message)?;
    Err(context.throw(error))
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install dns.{name}")))
    }
}

fn set_str(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set_number(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: f64,
) -> Result<(), RootedError> {
    let value = context.number(value);
    set(context, object, name, value)
}
