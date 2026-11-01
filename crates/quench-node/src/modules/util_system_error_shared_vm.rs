//! Target libuv protocol facts shared by Node utility and host I/O errors.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

include!(concat!(env!("OUT_DIR"), "/node_uv_errors.rs"));
type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn system_error_number(name: &str) -> Option<i32> {
    UV_ERRORS.iter().find(|row| row.1 == name).map(|row| row.0)
}
pub(crate) fn system_error_code(number: f64) -> Option<&'static str> {
    UV_ERRORS
        .iter()
        .find(|row| f64::from(row.0) == number)
        .map(|row| row.1)
}
pub(crate) fn system_error_name(number: f64) -> String {
    system_error_code(number)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Unknown system error {number}"))
}
// libuv's UDP protocol flag, also consumed by the native datagram transport.
pub(crate) const UV_UDP_REUSEADDR: i32 = 4;

pub(crate) fn uv_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(context, "\0binding:uv", |context| {
        let binding = context.object_rooted()?;
        let warning_state = context.null_object_rooted()?;
        let errname = context.host_function_named_with_data(
            crate::host::shared_vm::operation("uvErrName"),
            "errname",
            warning_state,
        )?;
        let key = context.string_rooted("errname");
        context.define_data_property_rooted(binding, key, errname, true, true, true)?;
        for &(code, name, _) in UV_ERRORS {
            let key = context.string_rooted(&format!("UV_{name}"));
            let value = context.number(f64::from(code));
            context.define_data_property_rooted(binding, key, value, false, true, false)?;
        }
        method(context, binding, "getErrorMap", "uvGetErrorMap")?;
        method(context, binding, "getErrorMessage", "uvGetErrorMessage")?;
        Ok(binding)
    })
}
fn method(
    c: &mut Context<'_>,
    binding: RootId,
    name: &str,
    operation: &str,
) -> Result<(), RootedError> {
    let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
    let key = c.string_rooted(name);
    c.define_data_property_rooted(binding, key, function, true, true, true)?;
    Ok(())
}
fn number(c: &mut Context<'_>, args: &[RootId]) -> Result<f64, RootedError> {
    let missing = c.undefined();
    c.to_number(args.first().copied().unwrap_or(missing))
}
pub(crate) fn errname(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    pending_errname_warning(c)?;
    let value = number(c, args)?;
    Ok(c.string_rooted(&system_error_name(value)))
}
pub(crate) fn error_message(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = number(c, args)?;
    let message = UV_ERRORS
        .iter()
        .find(|row| f64::from(row.0) == value)
        .map(|row| row.2.to_owned())
        .unwrap_or_else(|| format!("Unknown system error {value}"));
    Ok(c.string_rooted(&message))
}
pub(crate) fn error_map(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    c.with_startup_intrinsics(|c| {
        let mut entries = Vec::with_capacity(UV_ERRORS.len());
        for &(code, name, message) in UV_ERRORS {
            let code = c.number(f64::from(code));
            let name = c.string_rooted(name);
            let message = c.string_rooted(message);
            let pair = c.array_rooted(&[name, message])?;
            entries.push(c.array_rooted(&[code, pair])?);
        }
        let entries = c.array_rooted(&entries)?;
        let global = c.global_root()?;
        let key = c.string_rooted("Map");
        let constructor = c.get_property_rooted(global, key)?;
        c.construct_rooted(constructor, constructor, &[entries])
    })
}

// The SDK uv.cc emits once per environment, before validating the error number.
fn pending_errname_warning(c: &mut Context<'_>) -> Result<(), RootedError> {
    use super::event_constructor_shared_vm::get;
    let state = c.host_mut().shared_state();
    if !state.borrow().cli_options.enabled("--pending-deprecation") {
        return Ok(());
    }
    let data = c.host_function_data()?;
    let warned = get(c, data, "warned")?;
    if c.rooted_value(warned)
        .is_some_and(|value| value.as_bool() == Some(true))
    {
        return Ok(());
    }
    let key = c.string_rooted("warned");
    let yes = c.boolean(true);
    c.define_data_property_rooted(data, key, yes, true, false, false)?;
    let process = state
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    let emit = get(c, process, "emitWarning")?;
    if !c.is_callable_rooted(emit)? {
        return Ok(());
    }
    let message = c.string_rooted("Directly calling process.binding('uv').errname(<val>) is being deprecated. Please make sure to use util.getSystemErrorName() instead.");
    let kind = c.string_rooted("DeprecationWarning");
    let code = c.string_rooted("DEP0119");
    c.call_rooted(emit, process, &[message, kind, code])?;
    Ok(())
}
