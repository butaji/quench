//! Shared-VM projection of the canonical Console class.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(
        crate::modules::console_source::CONSOLE_CLASS,
        "node:console/Console.js",
    )?;
    let noop = context.host_function(crate::host::shared_vm::operation("consoleNoop"))?;
    let initial_state = context.null_object_rooted()?;
    let global = context.global_root()?;
    let map = get(context, global, "Map")?;
    for (name, value) in [("noop", noop), ("Map", map)] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(initial_state, key, value, true, true, true)?;
    }
    let initialize = context.host_function_with_data(
        crate::host::shared_vm::operation("consoleInitializeWrite"),
        initial_state,
    )?;
    let write =
        context.host_function_with_data(crate::host::shared_vm::operation("consoleWrite"), noop)?;
    // `util` is built on the first output that needs formatting.
    let load_util = context.host_function(crate::host::shared_vm::operation("consoleLoadUtil"))?;
    let undefined = context.undefined();
    let constructor = context.call_rooted(factory, undefined, &[initialize, write, load_util])?;
    let prototype = get(context, constructor, "prototype")?;
    super::console_timing_shared_vm::install(context, prototype)?;
    let trace = context.host_function(crate::host::shared_vm::operation("consoleTrace"))?;
    set(context, prototype, "trace", trace)?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let stdout = get(context, process, "stdout")?;
    let stderr = get(context, process, "stderr")?;
    let module = context.construct_rooted(constructor, constructor, &[stdout, stderr])?;
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

pub(crate) fn noop(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}

/// The console instance owns the ignore policy and its two stream callbacks.
pub(crate) fn initialize_write(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let console = args.first().copied().unwrap_or(undefined);
    let options = args.get(1).copied().unwrap_or(undefined);
    let mut ignore = args.get(2).copied().unwrap_or(undefined);
    if context.is_object_rooted(options)? {
        ignore = get(context, options, "ignoreErrors")?;
    }
    let ignore = if context
        .rooted_value(ignore)
        .is_some_and(|value| value.is_undefined())
    {
        true
    } else {
        context.truthy_rooted(ignore)?
    };
    let value = context.boolean(ignore);
    let key = context.string_rooted("_ignoreErrors");
    context.define_data_property_rooted(console, key, value, true, false, true)?;
    let initial_state = context.host_function_data()?;
    let noop = get(context, initial_state, "noop")?;
    let map = get(context, initial_state, "Map")?;
    let times = context.construct_rooted(map, map, &[])?;
    let key = context.string_rooted("_times");
    context.define_data_property_rooted(console, key, times, true, true, true)?;
    for (stdout, name) in [
        (true, "_stdoutErrorHandler"),
        (false, "_stderrErrorHandler"),
    ] {
        let stdout = context.boolean(stdout);
        let data = context.array_rooted(&[console, stdout, noop])?;
        let callback = context.host_function_with_data(
            crate::host::shared_vm::operation("consoleWriteError"),
            data,
        )?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(console, key, callback, true, false, true)?;
    }
    Ok(undefined)
}

fn method(
    context: &mut NativeContext<'_, NodeHost>,
    target: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let operation = get(context, target, name)?;
    context.call_rooted(operation, target, args)
}

fn add_error_guard(
    context: &mut NativeContext<'_, NodeHost>,
    stream: RootId,
    noop: RootId,
    event: RootId,
) -> Result<(), RootedError> {
    let count = method(context, stream, "listenerCount", &[event])?;
    let zero = context.number(0.0);
    if context.strict_equal_rooted(count, zero)? {
        method(context, stream, "once", &[event, noop])?;
    }
    Ok(())
}

pub(crate) fn write_error(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let error = args.first().copied().unwrap_or_else(|| context.undefined());
    if context
        .rooted_value(error)
        .is_some_and(|value| value.is_null())
    {
        return Ok(context.undefined());
    }
    let data = context.host_function_data()?;
    let console = get(context, data, "0")?;
    let stdout = get(context, data, "1")?;
    let stdout = context.truthy_rooted(stdout)?;
    let stream = get(context, console, if stdout { "_stdout" } else { "_stderr" })?;
    let state = get(context, stream, "_writableState")?;
    let emitted = get(context, state, "errorEmitted")?;
    if !context.truthy_rooted(emitted)? {
        let noop = get(context, data, "2")?;
        let event = context.string_rooted("error");
        add_error_guard(context, stream, noop, event)?;
    }
    Ok(context.undefined())
}

fn stack_overflow(
    context: &mut NativeContext<'_, NodeHost>,
    error: &RootedError,
) -> Result<bool, RootedError> {
    let Some(exception) = error.exception else {
        return Ok(false);
    };
    if !context.truthy_rooted(exception)? {
        return Ok(false);
    }
    let name = get(context, exception, "name")?;
    if context.string_text(name)?.as_deref() != Some("RangeError") {
        return Ok(false);
    }
    let message = get(context, exception, "message")?;
    Ok(context.string_text(message)?.as_deref() == Some("Maximum call stack size exceeded"))
}

pub(crate) fn write(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let console = args.first().copied().unwrap_or(undefined);
    let stdout = args.get(1).copied().unwrap_or(undefined);
    let stdout = context.truthy_rooted(stdout)?;
    let text = args.get(2).copied().unwrap_or(undefined);
    let text = context.to_string(text)?;
    let text = context.string_rooted(&format!("{text}\n"));
    let ignore = get(context, console, "_ignoreErrors")?;
    let no = context.boolean(false);
    let stream = get(context, console, if stdout { "_stdout" } else { "_stderr" })?;
    let handler = get(
        context,
        console,
        if stdout {
            "_stdoutErrorHandler"
        } else {
            "_stderrErrorHandler"
        },
    )?;
    if context.strict_equal_rooted(ignore, no)? {
        return method(context, stream, "write", &[text]);
    }
    let noop = context.host_function_data()?;
    let event = context.string_rooted("error");
    let result = (|| {
        add_error_guard(context, stream, noop, event)?;
        method(context, stream, "write", &[text, handler])
    })();
    let result = match result {
        Ok(_) => Ok(undefined),
        Err(error) => match stack_overflow(context, &error) {
            Ok(true) => Err(error),
            Ok(false) => {
                if let Some(root) = error.exception {
                    context.release_root(root);
                }
                Ok(undefined)
            }
            Err(replacement) => {
                if let Some(root) = error.exception {
                    context.release_root(root);
                }
                Err(replacement)
            }
        },
    };
    // A cleanup throw takes precedence over a write throw, as in finally.
    if let Err(cleanup) = method(context, stream, "removeListener", &[event, noop]) {
        if let Err(error) = result {
            if let Some(root) = error.exception {
                context.release_root(root);
            }
        }
        return Err(cleanup);
    }
    result
}

pub(crate) fn load_util(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::util_module(context)
}
