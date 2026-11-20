//! `node:sea` (lib/sea.js) for an executable that is never a single-executable application:
//! `isSea()` is false and every asset accessor fails as Node's does outside one.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

const NOT_IN_SEA: &str = "ERR_NOT_IN_SINGLE_EXECUTABLE_APPLICATION";
const NOT_IN_SEA_MESSAGE: &str = "Operation cannot be invoked when not in a single-executable application";

/// lib/sea.js's exports in its order.
const EXPORTS: [(&str, &str); 5] = [
    ("isSea", "seaIsSea"),
    ("getAsset", "seaGetAsset"),
    ("getRawAsset", "seaGetRawAsset"),
    ("getAssetAsBlob", "seaGetAssetAsBlob"),
    ("getAssetKeys", "seaGetAssetKeys"),
];

pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    for (name, operation_name) in EXPORTS {
        let function = c.host_function_named(operation(operation_name), name)?;
        c.assign_named_property_rooted(module, name, function)?;
    }
    Ok(module)
}

fn not_in_sea(c: &mut Context<'_>) -> Result<RootedError, RootedError> {
    let error = c.error_rooted(NOT_IN_SEA_MESSAGE)?;
    super::node_errors_shared_vm::decorate_error(c, error, NOT_IN_SEA)?;
    Ok(c.throw(error))
}

fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.string_text(value)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, value, name, "string")?);
    }
    Ok(())
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

pub(crate) fn is_sea(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.boolean(false))
}

/// `getRawAsset(key)` and `getAssetAsBlob(key, options)`: the key is validated before the
/// application is.
pub(crate) fn get_raw_asset(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let key = argument(c, args, 0);
    validate_string(c, key, "key")?;
    Err(not_in_sea(c)?)
}

/// `getAsset(key, encoding)` validates its encoding first.
pub(crate) fn get_asset(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let encoding = argument(c, args, 1);
    if !c.rooted_value(encoding).is_some_and(|value| value.is_undefined()) {
        validate_string(c, encoding, "encoding")?;
    }
    get_raw_asset(c, this, args)
}

pub(crate) fn get_asset_keys(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Err(not_in_sea(c)?)
}
