//! Shared-VM projection of the canonical Console class.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let constructor = context.evaluate_script_rooted(
        crate::modules::console_source::CONSOLE_CLASS,
        "node:console/Console.js",
    )?;
    let prototype = get(context, constructor, "prototype")?;
    let trace = context.host_function(crate::host::shared_vm::operation("consoleTrace"))?;
    set(context, prototype, "trace", trace)?;
    let module = context.construct_rooted(constructor, constructor, &[])?;
    let name = context.string_rooted("Console");
    if context.set_property_rooted(module, name, constructor, module)? {
        Ok(module)
    } else {
        Err(RootedError::host("cannot install console.Console"))
    }
}

pub(crate) fn trace(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let output = get(context, receiver, "_stderr")?;
    let formatter = get(context, receiver, "_format")?;
    let args = context.array_rooted(args)?;
    let message = context.call_rooted(formatter, receiver, &[output, args])?;
    let message = context.to_string(message)?;

    let global = context.global_root()?;
    let error = get(context, global, "Error")?;
    let capture_stack = get(context, error, "captureStackTrace")?;
    let target = context.object_rooted()?;
    let trace = get(context, receiver, "trace")?;
    context.call_rooted(capture_stack, error, &[target, trace])?;
    let stack = get(context, target, "stack")?;
    let stack = context.to_string(stack)?;
    let stack = stack.lines().skip(1).collect::<Vec<_>>().join("\n");
    let text = if stack.is_empty() {
        format!("Trace: {message}")
    } else {
        format!("Trace: {message}\n{stack}")
    };

    let error = get(context, receiver, "error")?;
    let text = context.string_rooted(&text);
    context.call_rooted(error, receiver, &[text])
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let name = context.string_rooted(name);
    context.get_property_rooted(object, name)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let property_name = name;
    let name = context.string_rooted(property_name);
    if context.set_property_rooted(object, name, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install Console.{property_name}"
        )))
    }
}
