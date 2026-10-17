//! Server response status policy over Node's original mutable status-code map.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) enum Reason {
    WriteHead(RootId),
    Existing(RootId),
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn set(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    if !c.set_property_rooted(object, key, value, object)? {
        let error = c.type_error_rooted(&format!(
            "Cannot assign to read only property '{name}' of object '#<ServerResponse>'"
        ))?;
        return Err(c.throw(error));
    }
    Ok(())
}

pub(crate) fn codes(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let object = c.object_rooted()?;
    for &(status, reason) in super::http_protocol::STATUS_CODES {
        let key = c.string_rooted(&status.to_string());
        let value = c.string_rooted(reason);
        c.define_data_property_rooted(object, key, value, true, true, true)?;
    }
    Ok(object)
}

pub(crate) fn prepare(
    c: &mut Context<'_>,
    response: RootId,
    original: RootId,
    reason: Reason,
) -> Result<u16, RootedError> {
    let zero = c.number(0.0);
    let integer = c.bitwise_or_rooted(original, zero)?;
    let integer = c
        .rooted_value(integer)
        .and_then(|v| v.as_number())
        .ok_or_else(|| RootedError::host("HTTP status integer missing"))?;
    let Some(status) = super::http_protocol::valid_status_number(integer) else {
        let formatters = super::util_shared_vm::internal_formatters(c)?;
        let format = get(c, formatters, "format")?;
        let template = c.string_rooted("Invalid status code: %s");
        let undefined = c.undefined();
        let message = c.call_rooted(format, undefined, &[template, original])?;
        let error = c.range_error_rooted("")?;
        set(c, error, "message", message)?;
        super::node_errors_shared_vm::decorate_range_error(
            c,
            error,
            "ERR_HTTP_INVALID_STATUS_CODE",
        )?;
        return Err(c.throw(error));
    };
    let (current, explicit) = match reason {
        Reason::WriteHead(value) if c.string_code_units(value)?.is_some() => (value, true),
        Reason::WriteHead(_) => (get(c, response, "statusMessage")?, false),
        Reason::Existing(value) => (value, false),
    };
    if explicit {
        set(c, response, "statusMessage", current)?;
    } else if !c.truthy_rooted(current)? {
        // The native callbacks retain this original object, even if the public
        // module export is replaced. Mutating its entries remains observable.
        let codes = c.host_function_data()?;
        let key = c.number(f64::from(status));
        let value = c.get_property_rooted(codes, key)?;
        let value = if c.truthy_rooted(value)? {
            value
        } else {
            c.string_rooted("unknown")
        };
        set(c, response, "statusMessage", value)?;
    }
    let value = c.number(f64::from(status));
    set(c, response, "statusCode", value)?;
    Ok(status)
}

/// Node's default status-line cache is observable through getters and map mutation.
pub(crate) fn encode(
    c: &mut Context<'_>,
    response: RootId,
    status: u16,
) -> Result<Vec<u8>, RootedError> {
    let value = get(c, response, "statusMessage")?;
    let codes = c.host_function_data()?;
    let key = c.number(f64::from(status));
    let default = c.get_property_rooted(codes, key)?;
    if c.strict_equal_rooted(value, default)? {
        let state = c.host_mut().shared_state();
        if let Some(cached) = state.borrow().http.status_reasons.get(&status) {
            return Ok(cached.clone());
        }
        let value = get(c, response, "statusMessage")?;
        let bytes = c
            .to_string_code_units(value)?
            .into_iter()
            .map(|unit| unit as u8)
            .collect::<Vec<_>>();
        state
            .borrow_mut()
            .http
            .status_reasons
            .insert(status, bytes.clone());
        return Ok(bytes);
    }
    let value = get(c, response, "statusMessage")?;
    let units = c.to_string_code_units(value)?;
    if !units
        .iter()
        .all(|&unit| super::http_protocol::valid_header_unit(unit, false))
    {
        let error = c.type_error_rooted("Invalid character in statusMessage")?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_CHAR")?;
        return Err(c.throw(error));
    }
    let value = get(c, response, "statusMessage")?;
    Ok(c.to_string_code_units(value)?
        .into_iter()
        .map(|unit| unit as u8)
        .collect())
}
