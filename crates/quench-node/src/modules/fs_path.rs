//! Node path validation and raw bytes at the filesystem boundary.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};
use std::path::PathBuf;

pub(super) fn validate(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let value = super::url_shared_vm::to_path_if_file_url(context, undefined, &[value])?;
    let has_null = if let Some(text) = context.string_text(value)? {
        text.as_bytes().contains(&0)
    } else if context.has_builtin_brand_rooted(value, BuiltinBrand::Uint8Array)? {
        // Node's validator invokes the intrinsic typed-array operation, which also
        // rejects detached and out-of-bounds views before crossing the host boundary.
        let includes = context.with_startup_intrinsics(|context| {
            let global = context.global_root()?;
            let key = context.string_rooted("Uint8Array");
            let constructor = context.get_property_rooted(global, key)?;
            let key = context.string_rooted("prototype");
            let prototype = context.get_property_rooted(constructor, key)?;
            let key = context.string_rooted("includes");
            context.get_property_rooted(prototype, key)
        })?;
        let zero = context.number(0.0);
        let included = context.call_rooted(includes, value, &[zero])?;
        context.truthy_rooted(included)?
    } else {
        let error = super::util_shared_vm::argument_type_error_with_expected(
            context,
            value,
            name,
            "of type string or an instance of Buffer or URL",
        )?;
        return Err(super::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    };
    if has_null {
        let error = super::util_shared_vm::argument_value_error(
            context,
            value,
            name,
            "must be a string, Uint8Array, or URL without null bytes",
        )?;
        return Err(super::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_VALUE",
        )?);
    }
    Ok(value)
}

fn bytes(context: &mut NativeContext<'_, NodeHost>, path: RootId) -> Result<Vec<u8>, RootedError> {
    if let Some(text) = context.string_text(path)? {
        return Ok(text.into_bytes());
    }
    context
        .view_bytes_rooted(path)
        .ok_or_else(|| RootedError::host("filesystem path lost its backing bytes"))
}

/// Snapshot after option getters, when the native operation consumes the path.
pub(super) fn native_path(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<PathBuf, RootedError> {
    let bytes = bytes(context, value)?;
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(String::from_utf8_lossy(&bytes).into_owned());
    Ok(path)
}

pub(super) fn resolve(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<PathBuf, RootedError> {
    let path = native_path(context, value)?;
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
    Ok(cwd.resolve_io_path(&path))
}

/// System errors display UTF-8 replacement characters without public conversion hooks.
pub(super) fn display(
    context: &mut NativeContext<'_, NodeHost>,
    path: RootId,
) -> Result<String, RootedError> {
    Ok(String::from_utf8_lossy(&bytes(context, path)?).into_owned())
}
