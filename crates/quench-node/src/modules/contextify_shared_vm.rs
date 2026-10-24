//! Native Script ownership for the configured SDK's internal VM policy.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{CompiledScript, HostWeakRoot, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

#[derive(Default)]
pub(crate) struct State {
    scripts: Vec<(HostWeakRoot, CompiledScript)>,
}

fn unsupported(context: &mut Context<'_>, feature: &str) -> Result<RootedError, RootedError> {
    let error = context.error_rooted(&format!("Native VM {feature} is not implemented"))?;
    super::node_errors_shared_vm::decorate_error(context, error, "ERR_NOT_IMPLEMENTED")?;
    Ok(context.throw(error))
}

fn defined(context: &Context<'_>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| !value.is_undefined())
}

fn global_symbol(context: &mut Context<'_>, description: &str) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let symbol = get(context, global, "Symbol")?;
    let for_method = get(context, symbol, "for")?;
    let description = context.string_rooted(description);
    context.call_rooted(for_method, symbol, &[description])
}

pub(crate) fn binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(
        context,
        "\0binding:contextify",
        |context| {
            let binding = context.object_rooted()?;
            let constructor = context.host_constructor_named(
                crate::host::shared_vm::operation("contextifyScript"),
                "ContextifyScript",
            )?;
            let prototype = get(context, constructor, "prototype")?;
            let run = context.host_function_named(
                crate::host::shared_vm::operation("contextifyRunInContext"),
                "runInContext",
            )?;
            let key = context.string_rooted("runInContext");
            context.define_data_property_rooted(prototype, key, run, true, true, true)?;
            let key = context.string_rooted("ContextifyScript");
            context.define_data_property_rooted(binding, key, constructor, true, true, true)?;
            let compile = context.host_function_named(
                crate::host::shared_vm::operation("contextifyCompileFunction"),
                "compileFunction",
            )?;
            let key = context.string_rooted("compileFunction");
            context.define_data_property_rooted(binding, key, compile, true, true, true)?;
            Ok(binding)
        },
    )
}

fn argument(context: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index)
        .copied()
        .unwrap_or_else(|| context.undefined())
}

fn check_compilation_options(
    context: &mut Context<'_>,
    args: &[RootId],
) -> Result<(), RootedError> {
    for index in [2, 3] {
        let value = argument(context, args, index);
        if defined(context, value) && context.to_number(value)? != 0.0 {
            return Err(unsupported(context, "source offsets")?);
        }
    }
    let cached = argument(context, args, 4);
    let produce = argument(context, args, 5);
    if defined(context, cached) || context.truthy_rooted(produce)? {
        return Err(unsupported(context, "cached bytecode")?);
    }
    let parsing_context = argument(context, args, 6);
    if defined(context, parsing_context) {
        return Err(unsupported(context, "parsing contexts")?);
    }
    Ok(())
}

/// Each wrapper owns genuine compiled code; its weak identity never keeps the
/// wrapper or a realm alive. No source text is re-evaluated by runInContext.
pub(crate) fn script(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    check_compilation_options(context, args)?;
    if !context.is_object_rooted(receiver)? {
        let error = context.type_error_rooted("ContextifyScript requires construction")?;
        return Err(context.throw(error));
    }
    let source = argument(context, args, 0);
    let filename = argument(context, args, 1);
    let source = context.to_string(source)?;
    let filename = context.to_string(filename)?;
    let global = context.global_root()?;
    let compiled = context.compile_script_in_realm_with_origin(global, &source, &filename, 0, 0)?;
    // Node's public vm.runInContext accepts the ContextifyScript object made
    // by internal/vm. Quench's public vm wrapper executes source text, so keep
    // the source and filename under shared-registry symbols for that bridge.
    let source_key = global_symbol(context, "quench.internal.contextifyScript.source")?;
    let source_value = context.string_rooted(&source);
    context.define_data_property_rooted(receiver, source_key, source_value, false, false, false)?;
    let filename_key = global_symbol(context, "quench.internal.contextifyScript.filename")?;
    let filename_value = context.string_rooted(&filename);
    context.define_data_property_rooted(
        receiver,
        filename_key,
        filename_value,
        false,
        false,
        false,
    )?;
    registered_script(context, None);
    let identity = context.host_weak_root(receiver)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .vm_scripts
        .scripts
        .push((identity, compiled));
    Ok(receiver)
}

fn registered_script(
    context: &mut Context<'_>,
    receiver: Option<RootId>,
) -> Option<CompiledScript> {
    let shared = context.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().vm_scripts.scripts);
    let mut live = Vec::with_capacity(entries.len());
    let mut selected = None;
    for (identity, script) in entries {
        if let Some(original) = context.resolve_host_weak_root(identity) {
            if receiver.is_some_and(|receiver| {
                context.rooted_value(original) == context.rooted_value(receiver)
            }) {
                selected = Some(script.clone());
            }
            live.push((identity, script));
        } else {
            context.release_host_weak_root(identity);
        }
    }
    shared.borrow_mut().vm_scripts.scripts = live;
    selected
}

pub(crate) fn run_in_context(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(script) = registered_script(context, Some(receiver)) else {
        let error = context.type_error_rooted("Invalid ContextifyScript receiver")?;
        return Err(context.throw(error));
    };
    let sandbox = argument(context, args, 0);
    let timeout = argument(context, args, 1);
    if context.to_number(timeout)? != -1.0 {
        return Err(unsupported(context, "execution timeouts")?);
    }
    for index in [3, 4] {
        let value = argument(context, args, index);
        if context.truthy_rooted(value)? {
            return Err(unsupported(context, "execution interrupts")?);
        }
    }
    if context
        .rooted_value(sandbox)
        .is_some_and(|value| value.is_null())
    {
        let global = context.global_root()?;
        return context.evaluate_compiled_script_in_realm_rooted(global, &script);
    }
    let key = global_symbol(context, "quench.internal.contextify.realmGlobal")?;
    let run = context.get_property_rooted(sandbox, key)?;
    if !context.is_callable_rooted(run)? {
        let error = context.type_error_rooted("Invalid contextified sandbox")?;
        return Err(context.throw(error));
    }
    let display_errors = argument(context, args, 2);
    let break_on_sigint = argument(context, args, 3);
    context.call_rooted(
        run,
        sandbox,
        &[receiver, timeout, display_errors, break_on_sigint],
    )
}

/// Function bodies are genuine compiler input in the global lexical context,
/// rather than closures in an API shim's CommonJS environment.
pub(crate) fn compile_function(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    check_compilation_options(context, args)?;
    let extensions = argument(context, args, 7);
    if defined(context, extensions) {
        let length = get(context, extensions, "length")?;
        if context.to_number(length)? != 0.0 {
            return Err(unsupported(context, "context extensions")?);
        }
    }
    let source = argument(context, args, 0);
    let filename = argument(context, args, 1);
    let source = context.to_string(source)?;
    let filename = context.to_string(filename)?;
    let params = argument(context, args, 8);
    let mut parameters = Vec::new();
    if defined(context, params) {
        let length = get(context, params, "length")?;
        let length = context.to_number(length)? as usize;
        for index in 0..length {
            let parameter = get(context, params, &index.to_string())?;
            parameters.push(context.to_string(parameter)?);
        }
    }
    let source = format!("(function ({}) {{\n{source}\n}})", parameters.join(", "));
    let global = context.global_root()?;
    let function = context.evaluate_specialized_script_in_realm_with_origin_rooted(
        global, &source, &filename, 1, 0,
    )?;
    let result = context.object_rooted()?;
    let key = context.string_rooted("function");
    context.define_data_property_rooted(result, key, function, true, true, true)?;
    Ok(result)
}

pub(crate) fn prune(runtime: &mut quench_runtime::Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().vm_scripts.scripts);
    let mut live = Vec::with_capacity(entries.len());
    for (identity, script) in entries {
        if runtime.host_weak_root_is_live(identity) {
            live.push((identity, script));
        } else {
            runtime.release_host_weak_root(identity);
        }
    }
    shared.borrow_mut().vm_scripts.scripts = live;
}
