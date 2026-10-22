//! Fork policy projects the shared spawn path; it never launches a second host.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
fn invalid(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
    expected: &str,
) -> Result<RootedError, RootedError> {
    super::util_shared_vm::argument_type_error_with_expected(context, value, name, expected)
}
fn null_check(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(), RootedError> {
    if context
        .string_text(value)?
        .is_some_and(|value| value.contains('\0'))
    {
        let text = context.to_string(value)?;
        let error = context.type_error_rooted(&format!(
            "The argument '{name}' must be a string without null bytes. Received {}",
            text
        ))?;
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    Ok(())
}
fn array(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<Vec<RootId>, RootedError> {
    if !context.is_array_rooted(value)? {
        return Err(invalid(context, value, name, "an instance of Array")?);
    }
    let length = get(context, value, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default() as usize;
    (0..length)
        .map(|index| get(context, value, &index.to_string()))
        .collect()
}

pub(crate) fn plan(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let module_path = args.first().copied().unwrap_or_else(|| context.undefined());
    let module_path = if context.string_text(module_path)?.is_some() {
        module_path
    } else if {
        let is_buffer = context.host_function_data()?;
        let undefined = context.undefined();
        let result = context.call_rooted(is_buffer, undefined, &[module_path])?;
        context.truthy_rooted(result)?
    } {
        let bytes = context
            .view_bytes_rooted(module_path)
            .ok_or_else(|| RootedError::host("fork Buffer has no backing bytes"))?;
        context.string_rooted(&String::from_utf8_lossy(&bytes))
    } else if context.is_object_rooted(module_path)?
        && super::url_shared_vm::url_argument_text(context, module_path)?.is_some()
    {
        let undefined = context.undefined();
        super::url_shared_vm::file_url_to_path(context, undefined, &[module_path])?
    } else {
        return Err(invalid(
            context,
            module_path,
            "modulePath",
            "of type string or an instance of Buffer or URL",
        )?);
    };
    null_check(context, module_path, "modulePath")?;
    let arguments = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let mut options = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let arguments = if nullish(context, arguments) {
        Vec::new()
    } else if context.is_object_rooted(arguments)? && !context.is_array_rooted(arguments)? {
        options = arguments;
        Vec::new()
    } else {
        array(context, arguments, "args")?
    };
    if !nullish(context, options)
        && (!context.is_object_rooted(options)?
            || context.is_array_rooted(options)?
            || context.is_callable_rooted(options)?)
    {
        return Err(invalid(context, options, "options", "of type object")?);
    }
    let copied = context.object_rooted()?;
    let null = context.null();
    context.set_prototype_rooted(copied, null)?;
    if !nullish(context, options) {
        for key in context.enumerable_own_keys_rooted(options, true, true)? {
            let value = context.get_property_rooted(options, key)?;
            context.define_data_property_rooted(copied, key, value, true, true, true)?;
        }
    }
    options = copied;
    let shell = context.boolean(false);
    set(context, options, "shell", shell)?;
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("fork has no process"))?;
    let mut file = get(context, options, "execPath")?;
    if !context.truthy_rooted(file)? {
        file = get(context, process, "execPath")?;
        set(context, options, "execPath", file)?;
    }
    null_check(context, file, "options.execPath")?;
    let process_exec_argv = get(context, process, "execArgv")?;
    let mut exec_argv = get(context, options, "execArgv")?;
    if !context.truthy_rooted(exec_argv)? {
        exec_argv = process_exec_argv;
    }
    let mut invocation = array(context, exec_argv, "options.execArgv")?;
    for (index, value) in invocation.iter().copied().enumerate() {
        null_check(context, value, &format!("options.execArgv[{index}]"))?;
    }
    if context.strict_equal_rooted(exec_argv, process_exec_argv)? {
        let eval = get(context, process, "_eval")?;
        if !nullish(context, eval) {
            let mut matching = None;
            for (index, value) in invocation.iter().copied().enumerate() {
                if context.strict_equal_rooted(value, eval)? {
                    matching = Some(index);
                }
            }
            if let Some(index) = matching.filter(|index| *index > 0) {
                invocation.drain(index - 1..=index);
            }
        }
    }
    invocation.push(module_path);
    invocation.extend(arguments);
    let arguments = context.array_rooted(&invocation)?;
    let stdio = get(context, options, "stdio")?;
    let stdio = if let Some(value) = context.string_text(stdio)? {
        let mode = match value.as_str() {
            "pipe" | "overlapped" | "inherit" | "ignore" => value,
            _ => {
                let error = context.type_error_rooted(&format!(
                    "The argument 'stdio' is invalid. Received '{value}'"
                ))?;
                let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
                set(context, error, "code", code)?;
                return Err(context.throw(error));
            }
        };
        fork_stdio(context, &mode)?
    } else if !context.is_array_rooted(stdio)? {
        let silent = get(context, options, "silent")?;
        let mode = if context.truthy_rooted(silent)? {
            "pipe"
        } else {
            "inherit"
        };
        fork_stdio(context, mode)?
    } else {
        if !array(context, stdio, "options.stdio")?
            .iter()
            .any(|value| context.string_text(*value).ok().flatten().as_deref() == Some("ipc"))
        {
            let error = context.error_rooted(
                "Forked processes must have an IPC channel, missing value 'ipc' in options.stdio",
            )?;
            let code = context.string_rooted("ERR_CHILD_PROCESS_IPC_REQUIRED");
            set(context, error, "code", code)?;
            return Err(context.throw(error));
        }
        stdio
    };
    set(context, options, "stdio", stdio)?;
    let plan = context.object_rooted()?;
    set(context, plan, "file", file)?;
    set(context, plan, "args", arguments)?;
    set(context, plan, "options", options)?;
    Ok(plan)
}
fn fork_stdio(
    context: &mut NativeContext<'_, NodeHost>,
    mode: &str,
) -> Result<RootId, RootedError> {
    let mode = context.string_rooted(mode);
    let ipc = context.string_rooted("ipc");
    context.array_rooted(&[mode, mode, mode, ipc])
}
