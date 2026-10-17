//! Captured configured Node filesystem policy, executed as guest code on Quench.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

#[derive(Default)]
pub(crate) struct State {
    utilities: Option<RootId>,
    vfs_state: Option<RootId>,
}
const EXPORTS: &[&str] = &[
    "BigIntStats",
    "getStatFsFromBinding",
    "getOptions",
    "copyObject",
    "stringToFlags",
    "validateReadFileBufferOptions",
    "getReadFileBuffer",
    "getReadFileBufferByteLengthName",
    "validatePosition",
    "validateOffsetLengthRead",
    "validateOffsetLengthWrite",
    "validateStringAfterArrayBufferView",
    "handleErrorFromBinding",
];
fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

/// Called only for the configured library module, before its exports reach callers.
pub(crate) fn adopt(
    context: &mut NativeContext<'_, NodeHost>,
    exports: RootId,
) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    if shared.borrow().fs_policy.utilities.is_some() {
        return Ok(());
    }
    let utilities = context.null_object_rooted()?;
    for &name in EXPORTS {
        let value = get(context, exports, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(utilities, key, value, false, false, false)?;
    }
    let buffer_module = crate::host::shared_vm::commonjs::buffer_module(context)?;
    let buffer = get(context, buffer_module, "Buffer")?;
    let key = context.string_rooted("Buffer");
    context.define_data_property_rooted(utilities, key, buffer, false, false, false)?;
    for name in [
        "parseFileMode",
        "validateBuffer",
        "validateBoolean",
        "isInt32",
        "validateObject",
        "validateInteger",
        "validateEncoding",
        "kValidateObjectAllowNullable",
    ] {
        let value = crate::host::shared_vm::commonjs::node_library_export(
            context,
            "fs.js",
            "internal/validators.js",
            name,
        )?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(utilities, key, value, false, false, false)?;
    }
    let is_view = crate::host::shared_vm::commonjs::node_library_export(
        context, "fs.js", "internal/util/types.js", "isArrayBufferView",
    )?;
    let key = context.string_rooted("isArrayBufferView");
    context.define_data_property_rooted(utilities, key, is_view, false, false, false)?;
    let codes = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/errors.js",
        "codes",
    )?;
    let constructor = get(context, codes, "ERR_INVALID_ARG_VALUE")?;
    let key = context.string_rooted("ERR_INVALID_ARG_VALUE");
    context.define_data_property_rooted(utilities, key, constructor, false, false, false)?;
    let utilities = context.retain(utilities)?;
    let vfs_state = get(context, exports, "vfsState")?;
    let vfs_state = context.retain(vfs_state)?;
    shared.borrow_mut().fs_policy.vfs_state = Some(vfs_state);
    shared.borrow_mut().fs_policy.utilities = Some(utilities);
    let constructor = get(context, utilities, "BigIntStats")?;
    super::fs_shared_vm::stat::types::adopt_bigint_stats(context, constructor)
}

pub(super) fn cached(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
) -> Result<Option<RootId>, RootedError> {
    let utilities = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs_policy
        .utilities;
    utilities
        .map(|utilities| get(context, utilities, name))
        .transpose()
}

pub(super) fn export(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
) -> Result<RootId, RootedError> {
    if let Some(value) = cached(context, name)? {
        return Ok(value);
    }
    context.with_startup_intrinsics(|context| {
        crate::host::shared_vm::commonjs::node_library_export(
            context,
            "fs.js",
            "internal/fs/utils.js",
            "getOptions",
        )
    })?;
    cached(context, name)?
        .ok_or_else(|| RootedError::host("Node filesystem policy capture missing"))
}

pub(super) fn call(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = export(context, name)?;
    let undefined = context.undefined();
    context.call_rooted(function, undefined, args)
}

/// Construct the configured Node error class, preserving its public prototype.
pub(super) fn invalid_value(
    context: &mut NativeContext<'_, NodeHost>,
    name: RootId,
    value: RootId,
    reason: RootId,
) -> Result<RootedError, RootedError> {
    let constructor = export(context, "ERR_INVALID_ARG_VALUE")?;
    let error = context.construct_rooted(constructor, constructor, &[name, value, reason])?;
    Ok(context.throw(error))
}

/// Project the configured SDK fs.js dispatch literally: only null handlers
/// skip dispatch; direct property lookup preserves inherited/accessor behavior.
/// Only an undefined handler result selects the native fallback.
pub(super) fn vfs_handlers(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<Option<RootId>, RootedError> {
    let state = context.host_mut().shared_state().borrow().fs_policy.vfs_state;
    let Some(state) = state else { return Ok(None); };
    let handlers = get(context, state, "handlers")?;
    Ok((!context.rooted_value(handlers).is_some_and(|v| v.is_null())).then_some(handlers))
}

pub(super) fn vfs_call(
    context: &mut NativeContext<'_, NodeHost>,
    handlers: RootId,
    name: &str,
    args: &[RootId],
) -> Result<Option<RootId>, RootedError> {
    let function = get(context, handlers, name)?;
    let result = context.call_rooted(function, handlers, args)?;
    Ok((!context.rooted_value(result).is_some_and(|v| v.is_undefined())).then_some(result))
}

pub(super) fn vfs_read(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    args: &[RootId],
) -> Result<Option<RootId>, RootedError> {
    let Some(handlers) = vfs_handlers(context)? else { return Ok(None); };
    vfs_call(context, handlers, name, args)
}

pub(crate) fn vfs_exists_sync(
    context: &mut NativeContext<'_, NodeHost>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args.first().copied().unwrap_or_else(|| context.undefined());
    if let Some(result) = vfs_read(context, "existsSync", &[path])? { return Ok(result); }
    let original = context.host_function_data()?;
    context.call_rooted(original, this, args)
}

/// Install only the public SDK boundaries that dispatch before validation.
/// Fixed arity comes from both original SDK declarations (fs.js mkdirSync and
/// realpathSync). The retained fallback keeps all native host behavior.
pub(super) fn wrap_vfs_pair(
    context: &mut NativeContext<'_, NodeHost>,
    original: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let name_root = context.string_rooted(name);
    let data = context.array_rooted(&[original, name_root])?;
    let wrapper = context.host_function_with_data(
        crate::host::shared_vm::operation("fsVfsPair"), data,
    )?;
    let key = context.string_rooted("name");
    context.define_data_property_rooted(wrapper, key, name_root, false, false, true)?;
    Ok(wrapper)
}

pub(crate) fn vfs_pair(
    context: &mut NativeContext<'_, NodeHost>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let original = get(context, data, "0")?;
    let name_root = get(context, data, "1")?;
    let name = context.to_string(name_root)?;
    let path = args.first().copied().unwrap_or_else(|| context.undefined());
    let options = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let forwarded = [path, options];
    if let Some(value) = vfs_read(context, &name, &forwarded)? {
        // mkdir's SDK handler returns a presence envelope even when its public
        // result is undefined. realpath returns its actual public result.
        return if name == "mkdirSync" { get(context, value, "result") } else { Ok(value) };
    }
    context.call_rooted(original, this, &forwarded)
}
