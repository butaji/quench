//! Captured configured Node filesystem policy, executed as guest code on Quench.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

#[derive(Default)]
pub(crate) struct State {
    utilities: Option<RootId>,
}
const EXPORTS: &[&str] = &[
    "BigIntStats",
    "getStatFsFromBinding",
    "getOptions",
    "stringToFlags",
    "validateReadFileBufferOptions",
    "getReadFileBuffer",
    "getReadFileBufferByteLengthName",
    "validatePosition",
    "validateOffsetLengthRead",
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
        "validateObject",
        "validateInteger",
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
