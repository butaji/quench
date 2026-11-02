//! Legacy public binding policy over the configured SDK/native binding registry.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
include!("node_legacy_bindings.rs");

fn name(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<(String, RootId), RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let string = context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let key = context.string_rooted("String");
        context.get_property_rooted(global, key)
    })?;
    let undefined = context.undefined();
    let value = context.call_rooted(string, undefined, &[value])?;
    let text = context
        .string_text(value)?
        .ok_or_else(|| RootedError::host("String conversion did not return a string"))?;
    Ok((text, value))
}

/// A fresh legacy wrapper object: `util`'s type predicates, the public `util.types`
/// functions; `natives` maps builtin ids to their JavaScript source, and no Quench builtin
/// has any.
fn legacy_wrapper(context: &mut NativeContext<'_, NodeHost>, name: &str) -> Result<RootId, RootedError> {
    let result = context.object_rooted()?;
    if name == UTIL_BINDING {
        let util = super::util_shared_vm::module(context)?;
        let key = context.string_rooted("types");
        let types = context.get_property_rooted(util, key)?;
        for predicate in LEGACY_UTIL_TYPES {
            let key = context.string_rooted(predicate);
            let function = context.get_property_rooted(types, key)?;
            context.define_data_property_rooted(result, key, function, true, true, true)?;
        }
    }
    Ok(result)
}

pub(crate) fn binding(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let permission_enabled = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        state.cli_options.enabled("--permission") || state.cli_options.enabled("--permission-audit")
    };
    if permission_enabled {
        let error = context.error_rooted("process.binding")?;
        for (name, value) in [
            ("code", "ERR_ACCESS_DENIED"),
            ("permission", ""),
            ("resource", ""),
        ] {
            let key = context.string_rooted(name);
            let value = context.string_rooted(value);
            context.define_data_property_rooted(error, key, value, true, true, true)?;
        }
        return Err(context.throw(error));
    }
    let (name, value) = name(context, args)?;
    if RUNTIME_DEPRECATED_BINDINGS.contains(&name.as_str()) {
        let process = context
            .host_mut()
            .shared_state()
            .borrow()
            .process_module
            .ok_or_else(|| RootedError::host("process binding owner unavailable"))?;
        let key = context.string_rooted("emitWarning");
        let emit = context.get_property_rooted(process, key)?;
        let message = context.string_rooted(&format!(
            "Access to process.binding('{name}') is deprecated."
        ));
        let kind = context.string_rooted("DeprecationWarning");
        let code = context.string_rooted("DEP0111");
        context.call_rooted(emit, process, &[message, kind, code])?;
    } else if LEGACY_WRAPPER_BINDINGS.contains(&name.as_str()) {
        return legacy_wrapper(context, &name);
    } else if !PROCESS_BINDING_ALLOW_LIST.contains(&name.as_str()) {
        let error = context.error_rooted(&format!("No such module: {name}"))?;
        return Err(context.throw(error));
    }
    let undefined = context.undefined();
    super::trace_shared_vm::internal_binding(context, undefined, &[value])
}
