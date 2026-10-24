//! Captured configured Node filesystem policy, executed as guest code on Quench.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

#[derive(Default)]
pub(crate) struct State {
    utilities: Option<RootId>,
    vfs_state: Option<RootId>,
    exists_type_warning_emitted: bool,
}
const EXPORTS: &[&str] = &[
    "BigIntStats",
    "vfsState",
    "getValidatedPath",
    "toUnixTimestamp",
    "getStatFsFromBinding",
    "getOptions",
    "kWriteFileMaxChunkSize",
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
    // fs.js destructures this scalar when its SDK dependency is initialized.
    // Later mutations to the exported constants object cannot change policy.
    let constants = get(context, exports, "constants")?;
    let maximum_user_id = get(context, constants, "kMaxUserId")?;
    let key = context.string_rooted("kMaxUserId");
    context.define_data_property_rooted(utilities, key, maximum_user_id, false, false, false)?;
    let buffer_module = crate::host::shared_vm::commonjs::buffer_module(context)?;
    let buffer = get(context, buffer_module, "Buffer")?;
    let key = context.string_rooted("Buffer");
    context.define_data_property_rooted(utilities, key, buffer, false, false, false)?;
    for name in [
        "parseFileMode",
        "validateFunction",
        "validateBuffer",
        "validateBoolean",
        "validateAbortSignal",
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
        context,
        "fs.js",
        "internal/util/types.js",
        "isArrayBufferView",
    )?;
    let key = context.string_rooted("isArrayBufferView");
    context.define_data_property_rooted(utilities, key, is_view, false, false, false)?;
    let assert = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "fs.js",
        "internal/assert.js",
    )?;
    let key = context.string_rooted("assert");
    context.define_data_property_rooted(utilities, key, assert, false, false, false)?;
    let iterable = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/streams/utils.js",
        "isIterable",
    )?;
    let key = context.string_rooted("isIterable");
    context.define_data_property_rooted(utilities, key, iterable, false, false, false)?;
    let abort = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/errors.js",
        "AbortError",
    )?;
    let key = context.string_rooted("AbortError");
    context.define_data_property_rooted(utilities, key, abort, false, false, false)?;
    let codes = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/errors.js",
        "codes",
    )?;
    let constructor = get(context, codes, "ERR_INVALID_ARG_VALUE")?;
    let key = context.string_rooted("ERR_INVALID_ARG_VALUE");
    context.define_data_property_rooted(utilities, key, constructor, false, false, false)?;
    let commonjs = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/util.js",
        "kEmptyObject",
    )?;
    let key = context.string_rooted("kEmptyObject");
    context.define_data_property_rooted(utilities, key, commonjs, false, false, false)?;
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

fn install_vfs_callbacks(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    if cached(context, "vfsVoid")?.is_some_and(|root| {
        !context
            .rooted_value(root)
            .is_some_and(|value| value.is_undefined())
    }) {
        return Ok(());
    }
    let utilities = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs_policy
        .utilities
        .ok_or_else(|| RootedError::host("Node filesystem policy capture missing"))?;
    let library_root = context.host_mut().node_library_root.clone();
    let filename = library_root.join("lib/fs.js");
    let source = std::fs::read_to_string(&filename)
        .or_else(|error| {
            crate::node_sources::source(&filename, &library_root)
                .map(str::to_owned)
                .ok_or(error)
        })
        .map_err(|error| RootedError::host(error.to_string()))?;
    let source =
        crate::host::shared_vm::policy_projection::project(&source, &["vfsVoid", "vfsResult"])
            .map_err(RootedError::host)?;
    // Only wrapper plumbing is generated. Both policy declarations and their
    // transitive declaration dependencies retain the configured SDK source.
    let factory = context.evaluate_script_rooted(
        &format!("(primordials, module) => {{\n{source}\nreturn module.exports;\n}}"),
        &filename.to_string_lossy(),
    )?;
    let primordials = crate::host::shared_vm::commonjs::node_primordials(context)?
        .ok_or_else(|| RootedError::host("Node VFS policy requires SDK primordials"))?;
    let module = context.object_rooted()?;
    let undefined = context.undefined();
    let helpers = context.call_rooted(factory, undefined, &[primordials, module])?;
    for name in ["vfsVoid", "vfsResult"] {
        let helper = get(context, helpers, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(utilities, key, helper, false, false, false)?;
    }
    Ok(())
}

pub(super) fn export(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
) -> Result<RootId, RootedError> {
    if matches!(name, "vfsVoid" | "vfsResult") {
        install_vfs_callbacks(context)?;
    }
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
    let state = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs_policy
        .vfs_state;
    let Some(state) = state else {
        return Ok(None);
    };
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
    Ok((!context
        .rooted_value(result)
        .is_some_and(|v| v.is_undefined()))
    .then_some(result))
}

pub(super) fn vfs_read(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    args: &[RootId],
) -> Result<Option<RootId>, RootedError> {
    let Some(handlers) = vfs_handlers(context)? else {
        return Ok(None);
    };
    vfs_call(context, handlers, name, args)
}

pub(crate) fn vfs_exists_sync(
    context: &mut NativeContext<'_, NodeHost>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args.first().copied().unwrap_or_else(|| context.undefined());
    if let Some(result) = vfs_read(context, "existsSync", &[path])? {
        return Ok(result);
    }
    let path = match call(context, "getValidatedPath", &[path]) {
        Ok(path) => path,
        Err(error) => {
            let Some(exception) = error.exception else {
                return Err(error);
            };
            let code = if context
                .rooted_value(exception)
                .is_some_and(|value| value.is_null() || value.is_undefined())
            {
                context.undefined()
            } else {
                get(context, exception, "code")?
            };
            let argument_type = context.string_rooted("ERR_INVALID_ARG_TYPE");
            let shared = context.host_mut().shared_state();
            let emitted = shared.borrow().fs_policy.exists_type_warning_emitted;
            if !emitted && context.strict_equal_rooted(code, argument_type)? {
                let process = shared
                    .borrow()
                    .process_module
                    .ok_or_else(|| RootedError::host("Node process is not initialized"))?;
                let emit = get(context, process, "emitWarning")?;
                let message = context
                    .string_rooted("Passing invalid argument types to fs.existsSync is deprecated");
                let category = context.string_rooted("DeprecationWarning");
                let code = context.string_rooted("DEP0187");
                context.call_rooted(emit, process, &[message, category, code])?;
                shared.borrow_mut().fs_policy.exists_type_warning_emitted = true;
            }
            return Ok(context.boolean(false));
        }
    };
    let original = context.host_function_data()?;
    context.call_rooted(original, this, &[path])
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
    let wrapper =
        context.host_function_with_data(crate::host::shared_vm::operation("fsVfsPair"), data)?;
    let key = context.string_rooted("name");
    context.define_data_property_rooted(wrapper, key, name_root, false, false, true)?;
    if name == "realpathSync" {
        // Keep the native realpath entry point's distinct syscall/error
        // behavior while still routing it through the VFS boundary.
        let native = get(context, original, "native")?;
        let native_data = context.array_rooted(&[native, name_root])?;
        let native_wrapper = context
            .host_function_with_data(crate::host::shared_vm::operation("fsVfsPair"), native_data)?;
        let key = context.string_rooted("name");
        context.define_data_property_rooted(native_wrapper, key, name_root, false, false, true)?;
        let key = context.string_rooted("native");
        context.define_data_property_rooted(wrapper, key, native_wrapper, false, false, true)?;
    }
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
        return if name == "mkdirSync" {
            get(context, value, "result")
        } else {
            Ok(value)
        };
    }
    context.call_rooted(original, this, &forwarded)
}
