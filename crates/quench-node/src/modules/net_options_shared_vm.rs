//! Native net option validation before any transport effect.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::net::IpAddr;

/// The Server constructor accepts nullish defaults, objects, and a listener function.
pub(crate) fn server_options(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| context.undefined());
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
        || context.is_object_rooted(options)?
        || context.is_callable_rooted(options)?
    {
        return Ok(options);
    }
    Err(super::util_shared_vm::argument_type_error(
        context, options, "options", "object",
    )?)
}

pub(crate) fn socket_options(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| context.undefined());
    let for_connect = args
        .get(1)
        .copied()
        .map(|value| context.truthy_rooted(value))
        .transpose()?
        .unwrap_or(false);
    if for_connect {
        if !context.is_object_rooted(options)? || context.is_callable_rooted(options)? {
            return context.object_rooted();
        }
        let local_address = get(context, options, "localAddress")?;
        if context.truthy_rooted(local_address)? {
            let valid = context
                .string_text(local_address)?
                .as_deref()
                .and_then(super::net_shared_vm::parse_ip)
                .is_some();
            if !valid {
                let received = context.to_string(local_address)?;
                let error =
                    context.type_error_rooted(&format!("Invalid IP address: {received}"))?;
                super::node_errors_shared_vm::decorate_type_error(
                    context,
                    error,
                    "ERR_INVALID_IP_ADDRESS",
                )?;
                return Err(context.throw(error));
            }
        }
        let local_port = get(context, options, "localPort")?;
        if !context
            .rooted_value(local_port)
            .is_some_and(|value| value.is_undefined() || value.is_null())
        {
            let Some(number) = context
                .rooted_value(local_port)
                .and_then(|value| value.as_number())
            else {
                let error = super::util_shared_vm::argument_type_error(
                    context,
                    local_port,
                    "options.localPort",
                    "number",
                )?;
                return Err(validator_error(context, error, "ERR_INVALID_ARG_TYPE")?);
            };
            if !number.is_finite() || number.fract() != 0.0 || !(0.0..=65535.0).contains(&number)
            {
                let received = super::util_shared_vm::range_error_received(context, local_port)?;
                let error = context.range_error_rooted(&format!(
                    "The value of \"options.localPort\" is out of range. It must be >= 0 && <= 65535. Received {received}"
                ))?;
                super::node_errors_shared_vm::decorate_range_error(
                    context,
                    error,
                    "ERR_OUT_OF_RANGE",
                )?;
                return Err(context.throw(error));
            }
        }
        return Ok(options);
    }
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_undefined() || value.is_null())
    {
        return context.object_rooted();
    }
    if context.is_object_rooted(options)? {
        let fd = get(context, options, "fd")?;
        if !context
            .rooted_value(fd)
            .is_some_and(|value| value.is_undefined())
        {
            let Some(number) = context.rooted_value(fd).and_then(|value| value.as_number()) else {
                let error = super::util_shared_vm::argument_type_error(
                    context,
                    fd,
                    "options.fd",
                    "number",
                )?;
                return Err(validator_error(context, error, "ERR_INVALID_ARG_TYPE")?);
            };
            if !number.is_finite()
                || number.fract() != 0.0
                || number < 0.0
                || number > i32::MAX as f64
            {
                let received = super::util_shared_vm::range_error_received(context, fd)?;
                let error = context.range_error_rooted(&format!(
                    "The value of \"options.fd\" is out of range. It must be >= 0 && <= {}. Received {received}",
                    i32::MAX,
                ))?;
                super::node_errors_shared_vm::decorate_range_error(
                    context,
                    error,
                    "ERR_OUT_OF_RANGE",
                )?;
                super::node_errors_shared_vm::hide_range_error(context, error, "ERR_OUT_OF_RANGE")?;
                return Err(context.throw(error));
            }
        }
    }
    let object_mode = get(context, options, "objectMode")?;
    if context.truthy_rooted(object_mode)? {
        let value = get(context, options, "objectMode")?;
        return Err(invalid_property_value(
            context,
            "options.objectMode",
            value,
            "is not supported",
        )?);
    }
    let readable = get(context, options, "readableObjectMode")?;
    let unsupported = if context.truthy_rooted(readable)? {
        true
    } else {
        let writable = get(context, options, "writableObjectMode")?;
        context.truthy_rooted(writable)?
    };
    if unsupported {
        let readable = get(context, options, "readableObjectMode")?;
        let name = if context.truthy_rooted(readable)? {
            "options.readableObjectMode"
        } else {
            "options.writableObjectMode"
        };
        let readable = get(context, options, "readableObjectMode")?;
        let value = if context.truthy_rooted(readable)? {
            readable
        } else {
            get(context, options, "writableObjectMode")?
        };
        return Err(invalid_property_value(
            context,
            name,
            value,
            "is not supported",
        )?);
    }
    Ok(options)
}

/// Return the optional source endpoint requested by `net.connect(options)`.
/// Passing it through the shared transport path keeps the same binding
/// behavior for net, tls, and https clients.
pub(super) fn connect_source(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
) -> Result<(Option<IpAddr>, u16), RootedError> {
    if !context.is_object_rooted(options)? {
        return Ok((None, 0));
    }
    let address = get(context, options, "localAddress")?;
    let address = if context.truthy_rooted(address)? {
        context
            .string_text(address)?
            .as_deref()
            .and_then(super::net_shared_vm::parse_ip)
    } else {
        None
    };
    let port = get(context, options, "localPort")?;
    let port = context
        .rooted_value(port)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as u16;
    Ok((address, port))
}

pub(crate) fn missing_endpoint(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootedError, RootedError> {
    let error = context
        .type_error_rooted("The \"options\" or \"port\" or \"path\" argument must be specified")?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_MISSING_ARGS")?;
    Ok(context.throw(error))
}

pub(crate) fn validate_host(
    context: &mut NativeContext<'_, NodeHost>,
    host: RootId,
) -> Result<RootId, RootedError> {
    if !context.truthy_rooted(host)? {
        return Ok(context.string_rooted("localhost"));
    }
    let Some(text) = context.string_text(host)? else {
        let error =
            super::util_shared_vm::argument_type_error(context, host, "options.host", "string")?;
        return Err(validator_error(context, error, "ERR_INVALID_ARG_TYPE")?);
    };
    if text.contains('\0') {
        let error = invalid_property_value(
            context,
            "options.host",
            host,
            "must be a string without null bytes",
        )?;
        return Err(validator_error(context, error, "ERR_INVALID_ARG_VALUE")?);
    }
    Ok(host)
}

pub(crate) fn validate_lookup(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
    host: RootId,
) -> Result<(), RootedError> {
    if !context.is_object_rooted(options)? {
        return Ok(());
    }
    if context
        .string_text(host)?
        .as_deref()
        .and_then(super::net_shared_vm::parse_ip)
        .is_some()
    {
        return Ok(());
    }
    let lookup = get(context, options, "lookup")?;
    if context
        .rooted_value(lookup)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(());
    }
    let lookup = get(context, options, "lookup")?;
    if context.is_callable_rooted(lookup)? {
        return Ok(());
    }
    let error =
        super::util_shared_vm::argument_type_error(context, lookup, "options.lookup", "function")?;
    Err(validator_error(context, error, "ERR_INVALID_ARG_TYPE")?)
}

pub(super) fn validator_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootedError,
    code: &'static str,
) -> Result<RootedError, RootedError> {
    if let Some(exception) = error.exception {
        super::node_errors_shared_vm::hide_type_error(context, exception, code)?;
    }
    Ok(error)
}

const MIN_VALID_ATTEMPT_TIMEOUT: i32 = 1;

/// Node's positive signed-32-bit timeout validator, shared by the setter and connect.
pub(super) fn attempt_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<u64, RootedError> {
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        let error = super::util_shared_vm::argument_type_error(context, value, name, "number")?;
        return Err(validator_error(context, error, "ERR_INVALID_ARG_TYPE")?);
    };
    let range = if !number.is_finite() || number.fract() != 0.0 {
        Some("an integer".to_owned())
    } else if !(f64::from(MIN_VALID_ATTEMPT_TIMEOUT)..=f64::from(i32::MAX)).contains(&number) {
        Some(format!(">= {MIN_VALID_ATTEMPT_TIMEOUT} && <= {}", i32::MAX))
    } else {
        None
    };
    if let Some(range) = range {
        let received = super::util_shared_vm::range_error_received(context, value)?;
        let error = context.range_error_rooted(&format!(
            "The value of \"{name}\" is out of range. It must be {range}. Received {received}"
        ))?;
        super::node_errors_shared_vm::decorate_range_error(context, error, "ERR_OUT_OF_RANGE")?;
        super::node_errors_shared_vm::hide_range_error(context, error, "ERR_OUT_OF_RANGE")?;
        return Err(context.throw(error));
    }
    Ok(super::net_config::normalize_auto_select_family_attempt_timeout(number as u64))
}

/// Read the invocation settings once, in Node's destructuring order, before lookup.
pub(super) fn connect_family_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
) -> Result<bool, RootedError> {
    let has_options = context.is_object_rooted(options)?;
    let timeout = if has_options {
        get(context, options, "autoSelectFamilyAttemptTimeout")?
    } else {
        context.undefined()
    };
    let auto = if has_options {
        get(context, options, "autoSelectFamily")?
    } else {
        context.undefined()
    };
    let auto = if context
        .rooted_value(auto)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        context
            .host_mut()
            .shared_state()
            .borrow()
            .net_auto_select_family
    } else if let Some(value) = context.rooted_value(auto).and_then(|value| value.as_bool()) {
        value
    } else {
        let error = super::util_shared_vm::argument_type_error(
            context,
            auto,
            "options.autoSelectFamily",
            "boolean",
        )?;
        return Err(validator_error(context, error, "ERR_INVALID_ARG_TYPE")?);
    };
    if !context
        .rooted_value(timeout)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        attempt_timeout(context, timeout, "options.autoSelectFamilyAttemptTimeout")?;
    }
    Ok(auto)
}

fn invalid_property_value(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    value: RootId,
    reason: &str,
) -> Result<RootedError, RootedError> {
    let inspect = super::util_shared_vm::internal_inspect(context)?;
    let undefined = context.undefined();
    let detail = context.call_rooted(inspect, undefined, &[value])?;
    let detail = context.to_string(detail)?;
    let error = context.type_error_rooted(&format!(
        "The property '{name}' {reason}. Received {detail}"
    ))?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_INVALID_ARG_VALUE")?;
    Ok(context.throw(error))
}
