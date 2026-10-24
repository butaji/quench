//! Shared-VM `dns.lookup` projection over the operating system resolver.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::net::IpAddr;

use super::net_config::{ANY_FAMILY, IPV4_FAMILY, IPV6_FAMILY};

#[cfg(not(quench_node_cares))]
pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(
        crate::polyfills::bootstrap::dns::JS,
        "node:dns/bootstrap.js",
    )?;
    let module = context.object_rooted()?;
    let lookup = context.host_function(crate::host::shared_vm::operation("dnsLookup"))?;
    set(context, module, "lookup", lookup)?;
    let promises = context.object_rooted()?;
    let promised = context.host_function(crate::host::shared_vm::operation("dnsPromiseLookup"))?;
    set(context, promises, "lookup", promised)?;
    set(context, module, "promises", promises)?;
    let service = context.host_function(crate::host::shared_vm::operation("dnsLookupService"))?;
    set(context, module, "lookupService", service)?;
    let promised_service =
        context.host_function(crate::host::shared_vm::operation("dnsPromiseLookupService"))?;
    set(context, promises, "lookupService", promised_service)?;
    context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let key = context.string_rooted("Symbol");
        let symbol = context.get_property_rooted(global, key)?;
        let key = context.string_rooted("for");
        let symbol_for = context.get_property_rooted(symbol, key)?;
        let name = context.string_rooted("nodejs.util.promisify.custom");
        let custom = context.call_rooted(symbol_for, symbol, &[name])?;
        context.define_data_property_rooted(service, custom, promised_service, false, false, true)
    })?;
    for (name, operation) in [
        ("getDefaultResultOrder", "dnsGetDefaultOrder"),
        ("setDefaultResultOrder", "dnsSetDefaultOrder"),
    ] {
        let method = context.host_function(crate::host::shared_vm::operation(operation))?;
        set(context, module, name, method)?;
        set(context, promises, name, method)?;
    }
    context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let key = context.string_rooted("Symbol");
        let symbol = context.get_property_rooted(global, key)?;
        let key = context.string_rooted("for");
        let symbol_for = context.get_property_rooted(symbol, key)?;
        let name = context.string_rooted("nodejs.util.promisify.custom");
        let custom = context.call_rooted(symbol_for, symbol, &[name])?;
        context.define_data_property_rooted(lookup, custom, promised, false, false, true)
    })?;
    // The bootstrap DNS adapter owns the Node-shaped resolver methods and
    // permission checks. Keep the native host-backed lookup above, while
    // exposing the resolver surface even when c-ares is not linked.
    let global = context.global_root()?;
    let adapter_key = context.string_rooted("\0quench:dns_module");
    let adapter = context.get_property_rooted(global, adapter_key)?;
    if context.is_object_rooted(adapter)? {
        for name in [
            "resolve",
            "resolveAny",
            "resolve4",
            "resolve6",
            "resolveMx",
            "resolveNs",
            "resolveCaa",
            "resolveCname",
            "resolveNaptr",
            "resolvePtr",
            "resolveSoa",
            "resolveSrv",
            "resolveTlsa",
            "resolveTxt",
            "reverse",
            "Resolver",
            "getServers",
            "setServers",
        ] {
            let key = context.string_rooted(name);
            let value = context.get_property_rooted(adapter, key)?;
            if !context
                .rooted_value(value)
                .is_some_and(|value| value.is_undefined())
            {
                set(context, module, name, value)?;
            }
        }
        let adapter_promises_key = context.string_rooted("promises");
        let adapter_promises = context.get_property_rooted(adapter, adapter_promises_key)?;
        for name in [
            "resolve",
            "resolve4",
            "resolve6",
            "resolveAny",
            "resolveMx",
            "resolveNs",
            "resolveCaa",
            "resolveCname",
            "resolveNaptr",
            "resolvePtr",
            "resolveSoa",
            "resolveSrv",
            "resolveTlsa",
            "resolveTxt",
            "reverse",
            "Resolver",
            "getServers",
            "setServers",
        ] {
            let key = context.string_rooted(name);
            let value = context.get_property_rooted(adapter_promises, key)?;
            if !context
                .rooted_value(value)
                .is_some_and(|value| value.is_undefined())
            {
                set(context, promises, name, value)?;
            }
        }
    }
    Ok(module)
}

/// Native lookup policy: numeric results use nextTick; OS completions use poll dispatch.
pub(crate) fn lookup(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    lookup_impl(context, args, None)
}
fn lookup_impl(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    capability: Option<RootId>,
) -> Result<RootId, RootedError> {
    let hostname_root = args.first().copied().unwrap_or_else(|| context.undefined());
    let name = context.string_rooted("hostname");
    validate(
        context,
        "validateStringWithoutNullBytes",
        &[hostname_root, name],
    )?;
    let hostname = context
        .string_text(hostname_root)?
        .expect("SDK validated hostname string");
    if hostname.is_empty() {
        invalid_argument_value(
            context,
            "hostname",
            hostname_root,
            Some("must be a non-empty string"),
        )?;
    }

    let (family, all, callback) = lookup_arguments(context, args)?;
    let default_order = default_order(context)?;
    let mut order = match default_order.as_str() {
        "ipv4first" => 1,
        "ipv6first" => 2,
        _ => 0,
    };
    let mut hints = 0;
    if let Some(options) = args.get(1).copied() {
        if context
            .rooted_value(options)
            .is_some_and(|value| value.is_heap())
            && context.string_text(options)?.is_none()
            && !context.is_callable_rooted(options)?
        {
            for name in ["verbatim", "order", "hints"] {
                let key = context.string_rooted(name);
                let value = context.get_property_rooted(options, key)?;
                if context
                    .rooted_value(value)
                    .is_some_and(|v| v.is_undefined() || v.is_null())
                {
                    continue;
                }
                match name {
                    "verbatim" => {
                        let name = context.string_rooted("options.verbatim");
                        validate(context, "validateBoolean", &[value, name])?;
                        order = if context
                            .rooted_value(value)
                            .and_then(Value::as_bool)
                            .expect("SDK validated boolean")
                        {
                            0
                        } else {
                            1
                        };
                    }
                    "order" => {
                        order = match validate_order(context, value, "options.order")?.as_str() {
                            "ipv4first" => 1,
                            "ipv6first" => 2,
                            _ => 0,
                        };
                    }
                    "hints" => {
                        let name = context.string_rooted("options.hints");
                        validate(context, "validateNumber", &[value, name])?;
                        let number = context
                            .rooted_value(value)
                            .and_then(Value::as_number)
                            .expect("SDK validated number");
                        let uint32_modulus = u32::MAX as f64 + 1.0;
                        hints = if !number.is_finite() || number == 0.0 {
                            0
                        } else {
                            number.trunc().rem_euclid(uint32_modulus) as u32 as i32
                        };
                        if hints & !(libc::AI_ADDRCONFIG | libc::AI_ALL | libc::AI_V4MAPPED) != 0 {
                            let hints_root = context.number((hints as u32) as f64);
                            invalid_argument_value(context, "hints", hints_root, None)?;
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    if let Ok(address) = hostname.parse::<IpAddr>() {
        let callback_args = if all {
            success_all(context, vec![address])?
        } else {
            vec![
                context.null(),
                context.string_rooted(&hostname),
                context.number(f64::from(family_of(address))),
            ]
        };
        let undefined = context.undefined();
        crate::modules::shared_event_loop::queue_next_tick(
            context,
            callback,
            undefined,
            &callback_args,
        )?;
        return Ok(context.undefined());
    }
    if !network_permission_granted(context, &hostname)? {
        let error = context.error_rooted(&format!("getaddrinfo ERR_ACCESS_DENIED {hostname}"))?;
        set_str(context, error, "code", "ERR_ACCESS_DENIED")?;
        set_str(context, error, "syscall", "getaddrinfo")?;
        set_str(context, error, "hostname", &hostname)?;
        let undefined = context.undefined();
        crate::modules::shared_event_loop::queue_next_tick(context, callback, undefined, &[error])?;
        return Ok(context.undefined());
    }
    let binding = crate::host::shared_vm::commonjs::cares_binding(context)?;
    let key = context.string_rooted("getaddrinfo");
    let operation = context.get_property_rooted(binding, key)?;
    let key = context.string_rooted("GetAddrInfoReqWrap");
    let constructor = context.get_property_rooted(binding, key)?;
    let request = context.construct_rooted(constructor, constructor, &[])?;
    let callback = if let Some(capability) = capability {
        for name in ["resolve", "reject"] {
            let key = context.string_rooted(name);
            let value = context.get_property_rooted(capability, key)?;
            set(context, request, name, value)?;
        }
        context.host_function_with_data(
            crate::host::shared_vm::operation("dnsPromiseComplete"),
            request,
        )?
    } else {
        callback
    };
    let family_root = context.number(family as f64);
    set(context, request, "family", family_root)?;
    set(context, request, "callback", callback)?;
    set_str(context, request, "hostname", &hostname)?;
    let all_root = context.boolean(all);
    set(context, request, "all", all_root)?;
    let complete = context.host_function(crate::host::shared_vm::operation("dnsLookupComplete"))?;
    set(context, request, "oncomplete", complete)?;
    let family = context.number(family as f64);
    let hints = context.number(hints as f64);
    let order = context.number(order as f64);
    let hostname_root = context.string_rooted(&hostname);
    let status = context.call_rooted(
        operation,
        binding,
        &[request, hostname_root, family, hints, order],
    )?;
    if context
        .rooted_value(status)
        .and_then(Value::as_number)
        .is_some_and(|status| status != 0.0)
    {
        let addresses = context.null();
        crate::modules::shared_event_loop::queue_next_tick(
            context,
            complete,
            request,
            &[status, addresses],
        )?;
    }

    Ok(context.undefined())
}

fn network_permission_granted(
    context: &mut NativeContext<'_, NodeHost>,
    hostname: &str,
) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let process_key = context.string_rooted("process");
    let process = context.get_property_rooted(global, process_key)?;
    let permission_key = context.string_rooted("permission");
    let permission = context.get_property_rooted(process, permission_key)?;
    if !context.is_object_rooted(permission)? {
        return Ok(true);
    }
    let has_key = context.string_rooted("has");
    let has = context.get_property_rooted(permission, has_key)?;
    let scope = context.string_rooted("net");
    let resource = context.string_rooted(hostname);
    let result = context.call_rooted(has, permission, &[scope, resource])?;
    let args_key = context.string_rooted("execArgv");
    let args = context.get_property_rooted(process, args_key)?;
    if context.is_object_rooted(args)? {
        let includes_key = context.string_rooted("includes");
        let includes = context.get_property_rooted(args, includes_key)?;
        let audit_flag = context.string_rooted("--permission-audit");
        let audit = context.call_rooted(includes, args, &[audit_flag])?;
        if context.truthy_rooted(audit)? {
            return Ok(true);
        }
    }
    Ok(context.truthy_rooted(result)?)
}

fn lookup_arguments(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<(u8, bool, RootId), RootedError> {
    let undefined = context.undefined();
    let second = args.get(1).copied().unwrap_or(undefined);
    let second_is_callback = context.is_callable_rooted(second)?;
    let callback = if second_is_callback {
        second
    } else {
        args.get(2).copied().unwrap_or(undefined)
    };
    let name = context.string_rooted("callback");
    validate(context, "validateFunction", &[callback, name])?;
    if second_is_callback {
        return Ok((ANY_FAMILY, false, callback));
    }
    let value = context.rooted_value(second).unwrap_or(Value::UNDEFINED);
    if value.is_null() || value.is_undefined() {
        return Ok((ANY_FAMILY, false, callback));
    }
    if value.as_number().is_some() {
        return Ok((validated_family(context, second)?, false, callback));
    }
    if !value.is_heap() || context.string_text(second)?.is_some() {
        let name = context.string_rooted("options");
        validate(context, "validateObject", &[second, name])?;
    }
    let key = context.string_rooted("family");
    let family_root = context.get_property_rooted(second, key)?;
    let value = context
        .rooted_value(family_root)
        .unwrap_or(Value::UNDEFINED);
    let family = if value.is_null() || value.is_undefined() {
        ANY_FAMILY
    } else {
        match context.string_text(family_root)?.as_deref() {
            Some("IPv4") => IPV4_FAMILY,
            Some("IPv6") => IPV6_FAMILY,
            _ => validated_family(context, family_root)?,
        }
    };
    let key = context.string_rooted("all");
    let all_root = context.get_property_rooted(second, key)?;
    let value = context.rooted_value(all_root).unwrap_or(Value::UNDEFINED);
    let all = if value.is_null() || value.is_undefined() {
        false
    } else {
        let name = context.string_rooted("options.all");
        validate(context, "validateBoolean", &[all_root, name])?;
        value.as_bool().expect("SDK validated boolean")
    };
    Ok((family, all, callback))
}
fn validated_family(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<u8, RootedError> {
    let name = context.string_rooted("family");
    let values = [
        context.number(ANY_FAMILY as f64),
        context.number(IPV4_FAMILY as f64),
        context.number(IPV6_FAMILY as f64),
    ];
    let allowed = context.array_rooted(&values)?;
    validate(context, "validateOneOf", &[value, name, allowed])?;
    Ok(context
        .rooted_value(value)
        .and_then(Value::as_number)
        .expect("SDK validated family") as u8)
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

pub(crate) fn complete(
    context: &mut NativeContext<'_, NodeHost>,
    request: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let status = args.first().copied().unwrap_or_else(|| context.undefined());
    let addresses = args.get(1).copied().unwrap_or_else(|| context.undefined());
    complete_lookup(context, request, status, addresses)?;
    Ok(context.undefined())
}
fn complete_lookup(
    context: &mut NativeContext<'_, NodeHost>,
    request: RootId,
    status: RootId,
    addresses: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted("callback");
    let callback = context.get_property_rooted(request, key)?;
    let key = context.string_rooted("hostname");
    let hostname = context.get_property_rooted(request, key)?;
    let hostname = context.string_text(hostname)?.unwrap_or_default();
    let key = context.string_rooted("all");
    let all = context.get_property_rooted(request, key)?;
    let all = context
        .rooted_value(all)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let status_number = context
        .rooted_value(status)
        .and_then(Value::as_number)
        .unwrap_or(0.0);
    if status_number != 0.0 {
        let native_code =
            crate::modules::util_system_error_shared_vm::system_error_name(status_number);
        let code = if matches!(native_code.as_str(), "EAI_NODATA" | "EAI_NONAME") {
            "ENOTFOUND"
        } else {
            native_code.as_str()
        };
        let error = context.error_rooted(&format!("getaddrinfo {code} {hostname}"))?;
        set_str(context, error, "code", code)?;
        set_str(context, error, "syscall", "getaddrinfo")?;
        set_str(context, error, "hostname", &hostname)?;
        set(context, error, "errno", status)?;
        let undefined = context.undefined();
        context.call_rooted(callback, undefined, &[error])?;
        return Ok(());
    }
    let key = context.string_rooted("family");
    let requested_family = context.get_property_rooted(request, key)?;
    let requested_family = context
        .rooted_value(requested_family)
        .and_then(Value::as_number)
        .unwrap_or(0.0);
    let key = context.string_rooted("length");
    let length = context.get_property_rooted(addresses, key)?;
    let length = context
        .rooted_value(length)
        .and_then(Value::as_number)
        .unwrap_or(0.0) as usize;
    let mut result_args = vec![context.null()];
    if all {
        for index in 0..length {
            let key = context.string_rooted(&index.to_string());
            let address = context.get_property_rooted(addresses, key)?;
            let family = if requested_family != 0.0 {
                requested_family
            } else {
                context
                    .string_text(address)?
                    .and_then(|text| text.parse::<IpAddr>().ok())
                    .map_or(0.0, |address| family_of(address) as f64)
            };
            let item = context.object_rooted()?;
            set(context, item, "address", address)?;
            set_number(context, item, "family", family)?;
            context.set_property_rooted(addresses, key, item, addresses)?;
        }
        result_args.push(addresses);
    } else {
        let key = context.string_rooted("0");
        let address = context.get_property_rooted(addresses, key)?;
        let family = if requested_family != 0.0 {
            requested_family
        } else {
            context
                .string_text(address)?
                .and_then(|text| text.parse::<IpAddr>().ok())
                .map_or(0.0, |address| family_of(address) as f64)
        };
        result_args.push(address);
        result_args.push(context.number(family));
    }
    let undefined = context.undefined();
    context.call_rooted(callback, undefined, &result_args)?;
    Ok(())
}
pub(crate) fn promise_lookup(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let capability = promise_capability(context)?;
    let key = context.string_rooted("promise");
    let promise = context.get_property_rooted(capability, key)?;
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("dnsPromiseComplete"),
        capability,
    )?;
    let undefined = context.undefined();
    let hostname = args.first().copied().unwrap_or(undefined);
    let options = args.get(1).copied().unwrap_or(undefined);
    lookup_impl(context, &[hostname, options, callback], Some(capability))?;
    Ok(promise)
}
pub(crate) fn promise_complete(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let capability = context.host_function_data()?;
    let undefined = context.undefined();
    let error = args.first().copied().unwrap_or(undefined);
    let failed = context
        .rooted_value(error)
        .is_some_and(|value| !value.is_null() && !value.is_undefined());
    let key = context.string_rooted(if failed { "reject" } else { "resolve" });
    let callback = context.get_property_rooted(capability, key)?;
    let result = if failed {
        error
    } else if let Some(family) = args.get(2).copied() {
        let result = context.object_rooted()?;
        let address = args.get(1).copied().unwrap_or(undefined);
        set(context, result, "address", address)?;
        set(context, result, "family", family)?;
        result
    } else {
        args.get(1).copied().unwrap_or(undefined)
    };
    context.call_rooted(callback, capability, &[result])
}
// The pinned SDK validators own enum validation, received-value formatting, and errors.
fn validate(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    args: &[RootId],
) -> Result<(), RootedError> {
    let validator = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "dns.js",
        "internal/validators.js",
        name,
    )?;
    let undefined = context.undefined();
    context.call_rooted(validator, undefined, args)?;
    Ok(())
}
fn validate_order(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<String, RootedError> {
    let name = context.string_rooted(name);
    let values = [
        context.string_rooted("verbatim"),
        context.string_rooted("ipv4first"),
        context.string_rooted("ipv6first"),
    ];
    let allowed = context.array_rooted(&values)?;
    validate(context, "validateOneOf", &[value, name, allowed])?;
    Ok(context
        .string_text(value)?
        .expect("SDK validated string enum"))
}
fn default_order(context: &mut NativeContext<'_, NodeHost>) -> Result<String, RootedError> {
    let state = context.host_mut().shared_state();
    if let Some(order) = state.borrow().dns.default_order.clone() {
        return Ok(order);
    }
    let initial = state
        .borrow()
        .cli_options
        .string_value("--dns-result-order")
        .unwrap_or("verbatim")
        .to_owned();
    let initial_root = context.string_rooted(&initial);
    let initial = validate_order(context, initial_root, "--dns-result-order")?;
    state.borrow_mut().dns.default_order = Some(initial.clone());
    Ok(initial)
}
pub(crate) fn get_default_order(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let order = default_order(context)?;
    Ok(context.string_rooted(&order))
}
pub(crate) fn set_default_order(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let order = validate_order(context, value, "dnsOrder")?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .dns
        .default_order = Some(order);
    Ok(context.undefined())
}
fn invalid_argument_value(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    value: RootId,
    reason: Option<&str>,
) -> Result<(), RootedError> {
    let codes = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "dns.js",
        "internal/errors.js",
        "codes",
    )?;
    let key = context.string_rooted("ERR_INVALID_ARG_VALUE");
    let constructor = context.get_property_rooted(codes, key)?;
    let name = context.string_rooted(name);
    let mut args = vec![name, value];
    if let Some(reason) = reason {
        args.push(context.string_rooted(reason));
    }
    let error = context.construct_rooted(constructor, constructor, &args)?;
    Err(context.throw(error))
}

fn promise_capability(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let key = context.string_rooted("Promise");
        let promise = context.get_property_rooted(global, key)?;
        let key = context.string_rooted("withResolvers");
        let factory = context.get_property_rooted(promise, key)?;
        context.call_rooted(factory, promise, &[])
    })
}
pub(crate) fn lookup_service(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    service_impl(context, args, None)
}
pub(crate) fn promise_lookup_service(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let capability = promise_capability(context)?;
    service_impl(context, args, Some(capability))?;
    let key = context.string_rooted("promise");
    context.get_property_rooted(capability, key)
}
fn service_error(
    context: &mut NativeContext<'_, NodeHost>,
    status: RootId,
    address: RootId,
) -> Result<RootId, RootedError> {
    let constructor = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "dns.js",
        "internal/errors.js",
        "DNSException",
    )?;
    let syscall = context.string_rooted("getnameinfo");
    context.construct_rooted(constructor, constructor, &[status, syscall, address])
}
fn service_impl(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    capability: Option<RootId>,
) -> Result<RootId, RootedError> {
    let expected = if capability.is_some() { 2 } else { 3 };
    if args.len() != expected {
        let codes = crate::host::shared_vm::commonjs::node_library_export(
            context,
            "dns.js",
            "internal/errors.js",
            "codes",
        )?;
        let key = context.string_rooted("ERR_MISSING_ARGS");
        let constructor = context.get_property_rooted(codes, key)?;
        let address = context.string_rooted("address");
        let port = context.string_rooted("port");
        let callback = context.string_rooted("callback");
        let names = [address, port, callback];
        let error = context.construct_rooted(constructor, constructor, &names[..expected])?;
        return Err(context.throw(error));
    }
    let address = args[0];
    let port = args[1];
    let name = context.string_rooted("address");
    validate(context, "validateString", &[address, name])?;
    if context
        .string_text(address)?
        .and_then(|text| text.parse::<IpAddr>().ok())
        .is_none()
    {
        invalid_argument_value(context, "address", address, None)?;
    }
    validate(context, "validatePort", &[port])?;
    if capability.is_none() {
        let name = context.string_rooted("callback");
        validate(context, "validateFunction", &[args[2], name])?;
    }
    let port = context.to_number(port)? + 0.0;
    let port = context.number(port);
    let address_text = context
        .string_text(address)?
        .expect("SDK validated address string");
    if !network_permission_granted(context, &address_text)? {
        let error =
            context.error_rooted(&format!("getnameinfo ERR_ACCESS_DENIED {address_text}"))?;
        set_str(context, error, "code", "ERR_ACCESS_DENIED")?;
        set_str(context, error, "syscall", "getnameinfo")?;
        set_str(context, error, "hostname", &address_text)?;
        if let Some(capability) = capability {
            let reject_key = context.string_rooted("reject");
            let reject = context.get_property_rooted(capability, reject_key)?;
            context.call_rooted(reject, capability, &[error])?;
        } else {
            let callback = args[2];
            let undefined = context.undefined();
            crate::modules::shared_event_loop::queue_next_tick(
                context,
                callback,
                undefined,
                &[error],
            )?;
        }
        return Ok(context.undefined());
    }
    let binding = crate::host::shared_vm::commonjs::cares_binding(context)?;
    let key = context.string_rooted("GetNameInfoReqWrap");
    let constructor = context.get_property_rooted(binding, key)?;
    let request = context.construct_rooted(constructor, constructor, &[])?;
    set(context, request, "hostname", address)?;
    set(context, request, "port", port)?;
    if let Some(capability) = capability {
        for name in ["resolve", "reject"] {
            let key = context.string_rooted(name);
            let value = context.get_property_rooted(capability, key)?;
            set(context, request, name, value)?;
        }
    } else {
        set(context, request, "callback", args[2])?;
    }
    let promise_mode = context.boolean(capability.is_some());
    let complete = context.host_function_with_data(
        crate::host::shared_vm::operation("dnsServiceComplete"),
        promise_mode,
    )?;
    set(context, request, "oncomplete", complete)?;
    let key = context.string_rooted("getnameinfo");
    let operation = context.get_property_rooted(binding, key)?;
    let status = context.call_rooted(operation, binding, &[request, address, port])?;
    if context
        .rooted_value(status)
        .and_then(Value::as_number)
        .is_some_and(|status| status != 0.0)
    {
        let error = service_error(context, status, address)?;
        if capability.is_some() {
            let key = context.string_rooted("reject");
            let reject = context.get_property_rooted(request, key)?;
            context.call_rooted(reject, request, &[error])?;
        } else {
            return Err(context.throw(error));
        }
    }
    Ok(request)
}
pub(crate) fn service_complete(
    context: &mut NativeContext<'_, NodeHost>,
    request: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mode = context.host_function_data()?;
    let promise_mode = context
        .rooted_value(mode)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let undefined = context.undefined();
    let status = args.first().copied().unwrap_or(undefined);
    let failed = context
        .rooted_value(status)
        .and_then(Value::as_number)
        .is_some_and(|status| status != 0.0);
    let hostname = args.get(1).copied().unwrap_or(undefined);
    let service = args.get(2).copied().unwrap_or(undefined);
    if failed {
        // Original SDK promises onlookupservice reads this.host; callback policy reads this.hostname.
        let key = context.string_rooted(if promise_mode { "host" } else { "hostname" });
        let address = context.get_property_rooted(request, key)?;
        let error = service_error(context, status, address)?;
        let key = context.string_rooted(if promise_mode { "reject" } else { "callback" });
        let callback = context.get_property_rooted(request, key)?;
        context.call_rooted(callback, request, &[error])?;
    } else if promise_mode {
        let result = context.object_rooted()?;
        set(context, result, "hostname", hostname)?;
        set(context, result, "service", service)?;
        let key = context.string_rooted("resolve");
        let resolve = context.get_property_rooted(request, key)?;
        context.call_rooted(resolve, request, &[result])?;
    } else {
        let key = context.string_rooted("callback");
        let callback = context.get_property_rooted(request, key)?;
        let null = context.null();
        context.call_rooted(callback, request, &[null, hostname, service])?;
    }
    Ok(context.undefined())
}
