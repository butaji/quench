//! Scratch integration draft: retain Node's exact async util.aborted policy.
//! This is a declaration projection, not a hand-written policy body. OXC owns
//! its boundary; source byte offsets are never inferred from marker strings.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn original_aborted(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    context.with_startup_intrinsics(|context| {
        let commonjs = crate::host::shared_vm::commonjs::node_primordials(context)?
            .ok_or_else(|| RootedError::host("configured Node primordials missing"))?;
        let promise_resolve = get(context, commonjs, "PromiseResolve")?;
        let promise_with_resolvers = get(context, commonjs, "PromiseWithResolvers")?;
        let webidl = crate::host::shared_vm::commonjs::node_library_module(
            context,
            "internal/abort_controller.js",
            "internal/webidl.js",
        )?;
        // Existing load adoption installs the original WebIDL interface converter
        // backed by the native signalStates WeakMap. Do not substitute instanceof.
        let converters = get(context, webidl, "converters")?;
        let validators = crate::host::shared_vm::commonjs::node_library_module(
            context,
            "internal/abort_controller.js",
            "internal/validators.js",
        )?;
        let validate_object = get(context, validators, "validateObject")?;
        let allow_objects = get(context, validators, "kValidateObjectAllowObjects")?;
        let data = context
            .host_mut()
            .shared_state()
            .borrow()
            .event_targets_api
            .ok_or_else(|| RootedError::host("EventTarget API missing"))?;
        let weak_key = get(context, data, "weakKey")?;
        let resist_key = get(context, data, "resistKey")?;
        bind_original(
            context,
            "aborted",
            true,
            &[
                ("converters", converters),
                ("validateObject", validate_object),
                ("kValidateObjectAllowObjects", allow_objects),
                ("PromiseResolve", promise_resolve),
                ("PromiseWithResolvers", promise_with_resolvers),
                ("kWeakHandler", weak_key),
                ("kResistStopPropagation", resist_key),
            ],
        )
    })
}

/// Load an unchanged configured SDK declaration; OXC owns source boundaries.
/// Binding names are the original lexical imports supplied by native providers.
pub(crate) fn bind_original(
    _: &mut NativeContext<'_, NodeHost>,
    _: &str,
    _: bool,
    _: &[(&str, RootId)],
) -> Result<RootId, RootedError> {
    crate::host::removed_javascript("node:internal/abort_controller.js")
}
