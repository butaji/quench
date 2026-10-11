//! execFile output collection and its single terminal callback transition.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const DEFAULT_MAX_BUFFER: f64 = (1024 * 1024) as f64;
use super::util_shared_vm::MAX_SAFE_INTEGER;

pub(crate) fn function(
    context: &mut NativeContext<'_, NodeHost>,
    spawn: RootId,
    buffer: RootId,
) -> Result<RootId, RootedError> {
    let data = context.array_rooted(&[spawn, buffer])?;
    context.host_function_with_data(crate::host::shared_vm::operation("execFile"), data)
}

pub(crate) fn exec_file(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let file = args.first().copied().unwrap_or(undefined);
    let mut arguments = args.get(1).copied().unwrap_or(undefined);
    let mut options = args.get(2).copied().unwrap_or(undefined);
    let mut callback = args.get(3).copied().unwrap_or(undefined);
    if context.is_callable_rooted(arguments)? {
        callback = arguments;
        arguments = undefined;
        options = undefined;
    } else if !nullish(context, arguments)
        && context.is_object_rooted(arguments)?
        && !context.is_array_rooted(arguments)?
    {
        callback = options;
        options = arguments;
        arguments = undefined;
    }
    if context.is_callable_rooted(options)? {
        callback = options;
        options = undefined;
    }
    if !nullish(context, options)
        && (!context.is_object_rooted(options)? || context.is_array_rooted(options)?)
    {
        return Err(super::util_shared_vm::argument_type_error(
            context, options, "options", "object",
        )?);
    }
    if !nullish(context, callback) && !context.is_callable_rooted(callback)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, callback, "callback", "function",
        )?);
    }
    if nullish(context, arguments) {
        arguments = context.array_rooted(&[])?;
    } else if context.is_array_rooted(arguments)? {
        let length = get(context, arguments, "length")?;
        let length = number(context, length).unwrap_or(0.0) as usize;
        let values = (0..length)
            .map(|index| get(context, arguments, &index.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        arguments = context.array_rooted(&values)?;
    }
    let source_options = options;
    options = context.null_object_rooted()?;
    let utf8 = context.string_rooted("utf8");
    let zero = context.number(0.0);
    let max = context.number(DEFAULT_MAX_BUFFER);
    let sigterm = context.string_rooted("SIGTERM");
    for (name, value) in [
        ("encoding", utf8),
        ("timeout", zero),
        ("maxBuffer", max),
        ("killSignal", sigterm),
    ] {
        set(context, options, name, value)?;
    }
    if !nullish(context, source_options) {
        for key in context.enumerable_own_keys_rooted(source_options, true, true)? {
            let value = context.get_property_rooted(source_options, key)?;
            context.set_property_rooted(options, key, value, options)?;
        }
    }
    let timeout = get(context, options, "timeout")?;
    let max_buffer = get(context, options, "maxBuffer")?;
    validate_limit(context, timeout, "timeout", true)?;
    validate_limit(context, max_buffer, "options.maxBuffer", false)?;
    let kill_signal = get(context, options, "killSignal")?;
    // execFile owns the timeout transition; the underlying spawn has none.
    set(context, options, "timeout", zero)?;
    let data = context.host_function_data()?;
    let spawn = get(context, data, "0")?;
    let buffer = get(context, data, "1")?;
    let signal = get(context, options, "signal")?;
    if !context
        .rooted_value(signal)
        .is_some_and(|value| value.is_undefined())
    {
        let valid = context.is_object_rooted(signal)? && !context.is_callable_rooted(signal)?;
        let key = context.string_rooted("aborted");
        if !valid || !context.has_property_rooted(signal, key)? {
            return Err(super::util_shared_vm::argument_type_error(
                context,
                signal,
                "options.signal",
                "AbortSignal",
            )?);
        }
    }
    let child = context.call_rooted(spawn, undefined, &[file, arguments, options])?;
    let mut encoding = get(context, options, "encoding")?;
    let is_encoding = method(context, buffer, "isEncoding", &[encoding])?;
    if !context.truthy_rooted(is_encoding)? {
        encoding = context.null();
    }
    let state = context.null_object_rooted()?;
    let stdout = context.array_rooted(&[])?;
    let stderr = context.array_rooted(&[])?;
    for (name, value) in [
        ("callback", callback),
        ("Buffer", buffer),
        ("stdout", stdout),
        ("stderr", stderr),
        ("file", file),
        ("arguments", arguments),
        ("child", child),
        ("maxBuffer", max_buffer),
        ("killSignal", kill_signal),
    ] {
        set(context, state, name, value)?;
    }
    for event in ["stdout", "stderr", "error", "close"] {
        let kind = context.string_rooted(event);
        let data = context.array_rooted(&[state, kind])?;
        let handler = context
            .host_function_with_data(crate::host::shared_vm::operation("execFileEvent"), data)?;
        let (emitter, name) = if matches!(event, "stdout" | "stderr") {
            (get(context, child, event)?, "data")
        } else {
            (child, event)
        };
        if !nullish(context, emitter) {
            if matches!(event, "stdout" | "stderr") && !nullish(context, encoding) {
                method(context, emitter, "setEncoding", &[encoding])?;
            }
            let name = context.string_rooted(name);
            method(context, emitter, "on", &[name, handler])?;
        }
    }
    if number(context, timeout).is_some_and(|timeout| timeout > 0.0) {
        let kind = context.string_rooted("timeout");
        let data = context.array_rooted(&[state, kind])?;
        let handler = context
            .host_function_with_data(crate::host::shared_vm::operation("execFileEvent"), data)?;
        let global = context.global_root()?;
        let timer = method(context, global, "setTimeout", &[handler, timeout])?;
        set(context, state, "timer", timer)?;
    }
    Ok(child)
}

pub(crate) fn event(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let state = get(context, data, "0")?;
    let kind = get(context, data, "1")?;
    let kind = context.to_string(kind)?;
    let undefined = context.undefined();
    let value = args.first().copied().unwrap_or(undefined);
    let done = get(context, state, "done")?;
    if context.truthy_rooted(done)? {
        return Ok(undefined);
    }
    if kind == "timeout" {
        kill(context, state)?;
        return Ok(undefined);
    }
    if matches!(kind.as_str(), "stdout" | "stderr") {
        let stopped = get(context, state, "stopped")?;
        if context.truthy_rooted(stopped)? {
            return Ok(undefined);
        }
        let chunks = get(context, state, &kind)?;
        let child = get(context, state, "child")?;
        let emitter = get(context, child, &kind)?;
        let encoding = get(context, emitter, "readableEncoding")?;
        let length = if context.truthy_rooted(encoding)? {
            let buffer = get(context, state, "Buffer")?;
            method(context, buffer, "byteLength", &[value, encoding])?
        } else {
            get(context, value, "length")?
        };
        let count_name = format!("{kind}Length");
        let prior = get(context, state, &count_name)?;
        let prior = number(context, prior).unwrap_or(0.0);
        let length = number(context, length).unwrap_or(0.0);
        let count = context.number(prior + length);
        set(context, state, &count_name, count)?;
        let limit = get(context, state, "maxBuffer")?;
        let limit = number(context, limit).unwrap_or_else(|| {
            if context
                .rooted_value(limit)
                .is_some_and(|value| value.is_undefined())
            {
                f64::INFINITY
            } else {
                0.0
            }
        });
        let chunk = if prior + length > limit {
            let start = context.number(0.0);
            let end = context.number((limit - prior).max(0.0));
            method(context, value, "slice", &[start, end])?
        } else {
            value
        };
        method(context, chunks, "push", &[chunk])?;
        if prior + length > limit {
            let error = context.range_error_rooted(&format!("{kind} maxBuffer length exceeded"))?;
            let code = context.string_rooted("ERR_CHILD_PROCESS_STDIO_MAXBUFFER");
            set(context, error, "code", code)?;
            set(context, state, "error", error)?;
            kill(context, state)?;
        }
        return Ok(undefined);
    }
    let done = context.boolean(true);
    set(context, state, "done", done)?;
    let timer = get(context, state, "timer")?;
    if !nullish(context, timer) {
        let global = context.global_root()?;
        method(context, global, "clearTimeout", &[timer])?;
    }
    let callback = get(context, state, "callback")?;
    if nullish(context, callback) {
        return Ok(undefined);
    }
    let buffer = get(context, state, "Buffer")?;
    let child = get(context, state, "child")?;
    let mut output = Vec::with_capacity(2);
    for stream in ["stdout", "stderr"] {
        let chunks = get(context, state, stream)?;
        let emitter = get(context, child, stream)?;
        let encoding = get(context, emitter, "readableEncoding")?;
        output.push(if context.truthy_rooted(encoding)? {
            let empty = context.string_rooted("");
            method(context, chunks, "join", &[empty])?
        } else {
            method(context, buffer, "concat", &[chunks])?
        });
    }
    let file = get(context, state, "file")?;
    let mut command = context.to_string(file)?;
    let arguments = get(context, state, "arguments")?;
    let length = get(context, arguments, "length")?;
    if number(context, length).unwrap_or(0.0) > 0.0 {
        let separator = context.string_rooted(" ");
        let joined = method(context, arguments, "join", &[separator])?;
        command.push(' ');
        command.push_str(&context.to_string(joined)?);
    }
    let signal = args.get(1).copied().unwrap_or(undefined);
    let pending_error = get(context, state, "error")?;
    let error = if kind == "error" {
        value
    } else if !nullish(context, pending_error) {
        pending_error
    } else if number(context, value) == Some(0.0)
        && context
            .rooted_value(signal)
            .is_some_and(|value| value.is_null())
    {
        context.null()
    } else {
        let stderr = context.to_string(output[1])?;
        let error = context.error_rooted(&format!("Command failed: {command}\n{stderr}"))?;
        let child = get(context, state, "child")?;
        let killed = get(context, child, "killed")?;
        let code = if let Some(number) = number(context, value).filter(|number| *number < 0.0) {
            context.string_rooted(&super::util_system_error_shared_vm::system_error_name(
                number,
            ))
        } else {
            value
        };
        set(context, error, "code", code)?;
        set(context, error, "signal", signal)?;
        set(context, error, "killed", killed)?;
        error
    };
    if !nullish(context, error) {
        let command = context.string_rooted(&command);
        set(context, error, "cmd", command)?;
    }
    context.call_rooted(callback, undefined, &[error, output[0], output[1]])?;
    Ok(undefined)
}

fn validate_limit(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
    integer: bool,
) -> Result<(), RootedError> {
    if nullish(context, value) {
        return Ok(());
    }
    let Some(number) = number(context, value) else {
        return Err(super::util_shared_vm::argument_type_error(
            context, value, name, "number",
        )?);
    };
    if number.is_nan()
        || number < 0.0
        || (integer && (number.fract() != 0.0 || number > MAX_SAFE_INTEGER))
    {
        let range = if integer && (!number.is_finite() || number.fract() != 0.0) {
            "an integer"
        } else if integer {
            ">= 0 && <= 9007199254740991"
        } else {
            ">= 0"
        };
        let received = super::util_shared_vm::range_error_received(context, value)?;
        let error = context.range_error_rooted(&format!(
            "The value of \"{name}\" is out of range. It must be {range}. Received {received}"
        ))?;
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    Ok(())
}

fn kill(context: &mut NativeContext<'_, NodeHost>, state: RootId) -> Result<(), RootedError> {
    let stopped = context.boolean(true);
    set(context, state, "stopped", stopped)?;
    let child = get(context, state, "child")?;
    for stream in ["stdout", "stderr"] {
        let emitter = get(context, child, stream)?;
        if !nullish(context, emitter) {
            method(context, emitter, "destroy", &[])?;
        }
    }
    let signal = get(context, state, "killSignal")?;
    method(context, child, "kill", &[signal])?;
    Ok(())
}

fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}

fn number(context: &NativeContext<'_, NodeHost>, value: RootId) -> Option<f64> {
    context
        .rooted_value(value)
        .and_then(|value| value.as_number())
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}

fn method(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = get(context, object, name)?;
    context.call_rooted(function, object, args)
}
