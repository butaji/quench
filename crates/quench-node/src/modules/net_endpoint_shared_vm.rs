//! Net endpoint overloads select TCP ports or local paths before OS effects.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn normalize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let first = args.first().copied().unwrap_or(undefined);
    let second = args.get(1).copied().unwrap_or(undefined);
    let third = args.get(2).copied().unwrap_or(undefined);
    let connecting = args
        .get(3)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_bool())
        == Some(true);
    let mut callback = third;
    let mut host = second;
    let mut options = undefined;
    let mut address = first;
    let mut explicit_path = false;
    let mut explicit_port = false;
    if context.is_callable_rooted(second)? {
        callback = second;
        host = undefined;
    }
    if context.is_object_rooted(first)? {
        options = first;
        host = get(context, options, "host")?;
        if connecting
            && context
                .rooted_value(host)
                .is_some_and(|value| value.is_undefined())
        {
            host = get(context, options, "hostname")?;
        }
        let port = get(context, options, "port")?;
        let path = get(context, options, "path")?;
        let port_key = context.string_rooted("port");
        let has_port = context.has_property_rooted(options, port_key)?;
        let port_present = context
            .rooted_value(port)
            .is_some_and(|value| !value.is_undefined());
        let port_is_scalar = context.string_text(port)?.is_some()
            || context
                .rooted_value(port)
                .is_some_and(|value| value.as_number().is_some() || value.is_null());
        if !connecting && (port_is_scalar || has_port && !port_present) {
            address = port;
            explicit_port = true;
        } else if context.truthy_rooted(path)? {
            if context.string_text(path)?.is_none() {
                if !connecting {
                    return Err(invalid_options(context, options)?);
                }
                return Err(super::util_shared_vm::argument_type_error(
                    context,
                    path,
                    "options.path",
                    "string",
                )?);
            }
            address = path;
            explicit_path = true;
            if !connecting {
                let zero = context.number(0.0);
                let numeric = context.subtract_rooted(path, zero)?;
                if context
                    .rooted_value(numeric)
                    .and_then(|value| value.as_number())
                    .is_some_and(|number| number >= 0.0)
                {
                    return Err(invalid_options(context, options)?);
                }
            }
        } else if !connecting && !has_port {
            return Err(invalid_options(context, options)?);
        } else {
            address = port;
            explicit_port = true;
        }
    }

    let original_address = address;
    if !explicit_path {
        if let Some(text) = context.string_text(address)? {
            // Number(string) >= 0 is Node's numeric-string/pipe-name distinction.
            let zero = context.number(0.0);
            let numeric = context.subtract_rooted(address, zero)?;
            let value = context
                .rooted_value(numeric)
                .and_then(|value| value.as_number())
                .unwrap_or(f64::NAN);
            if explicit_port || value >= 0.0 {
                if text.trim().is_empty() {
                    return Err(port_error(context, original_address, connecting)?);
                }
                address = numeric;
            }
        }
    }
    if context.string_text(address)?.is_none() {
        let value = context
            .rooted_value(address)
            .and_then(|value| value.as_number());
        if let Some(value) = value {
            if !value.is_finite()
                || value.fract() != 0.0
                || !(0.0..=f64::from(u16::MAX)).contains(&value)
            {
                return Err(port_error(context, original_address, connecting)?);
            }
        } else if !connecting
            && context
                .rooted_value(address)
                .is_some_and(|value| value.is_undefined() || value.is_null())
        {
            address = context.number(0.0);
        } else {
            if !connecting {
                return Err(invalid_options(context, first)?);
            }
            return Err(super::util_shared_vm::argument_type_error_with_expected(
                context,
                address,
                "options.port",
                "one of type number or string",
            )?);
        }
    }
    let plan = context.null_object_rooted()?;
    for (name, value) in [
        ("address", address),
        ("host", host),
        ("callback", callback),
        ("options", options),
    ] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(plan, key, value, true, true, true)?;
    }
    Ok(plan)
}
fn port_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    connecting: bool,
) -> Result<RootedError, RootedError> {
    let detail = super::util_shared_vm::argument_type_description(context, value)?;
    let name = if connecting { "Port" } else { "options.port" };
    let limit = u32::from(u16::MAX) + 1;
    let error = context.range_error_rooted(&format!(
        "{name} should be >= 0 and < {limit}. Received {detail}."
    ))?;
    let key = context.string_rooted("code");
    let code = context.string_rooted("ERR_SOCKET_BAD_PORT");
    context.define_data_property_rooted(error, key, code, true, true, true)?;
    Ok(context.throw(error))
}
fn invalid_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
) -> Result<RootedError, RootedError> {
    let mut missing = false;
    if context.is_object_rooted(options)? {
        let port = context.string_rooted("port");
        let path = context.string_rooted("path");
        missing = !context.has_property_rooted(options, port)?
            && !context.has_property_rooted(options, path)?;
    }
    let options = if context
        .rooted_value(options)
        .is_some_and(|value| value.as_bool().is_some())
    {
        let normalized = context.object_rooted()?;
        let key = context.string_rooted("port");
        context.define_data_property_rooted(normalized, key, options, true, true, true)?;
        normalized
    } else {
        options
    };
    let inspect = super::util_shared_vm::internal_inspect(context)?;
    let undefined = context.undefined();
    let detail = context.call_rooted(inspect, undefined, &[options])?;
    let detail = context.to_string(detail)?;
    let condition = if missing {
        "must have the property \"port\" or \"path\""
    } else {
        "is invalid"
    };
    let error = context.type_error_rooted(&format!(
        "The argument 'options' {condition}. Received {detail}"
    ))?;
    let key = context.string_rooted("code");
    let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
    context.define_data_property_rooted(error, key, code, true, true, true)?;
    Ok(context.throw(error))
}
