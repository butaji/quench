//! Synchronous child process execution backed by the host operating system.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::process::{Command, Stdio};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(spawnSync, Buffer) => ({ spawnSync: (command, args, options) => {
  const result = spawnSync(command, args, options);
  result.stdout = Buffer.from(result.stdout);
  result.stderr = Buffer.from(result.stderr);
  return result;
} })"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(global, key)?;
    let spawn_sync = context.host_function(crate::host::shared_vm::operation(
        "childProcessSpawnSync",
    ))?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:child_process/shared.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[spawn_sync, buffer])
}

pub(crate) fn spawn_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(command_arg) = args.first().copied() else {
        let error = context.type_error_rooted("The \"file\" argument must be of type string")?;
        return Err(context.throw(error));
    };
    let command = context.to_string(command_arg)?;
    let arguments = args
        .get(1)
        .copied()
        .filter(|root| {
            !context
                .rooted_value(*root)
                .is_some_and(|value| value.is_undefined() || value.is_null())
        })
        .map(|root| string_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let options = args.get(2).copied().filter(|root| {
        !context
            .rooted_value(*root)
            .is_some_and(|value| value.is_undefined() || value.is_null())
    });
    let cwd = options
        .map(|root| property_string(context, root, "cwd"))
        .transpose()?
        .flatten();

    let mut process = Command::new(&command);
    process.args(arguments);
    process.stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        process.current_dir(cwd);
    }
    let env = options
        .map(|root| property(context, root, "env"))
        .transpose()?;
    if let Some(env) = env.filter(|root| context.is_object_rooted(*root).unwrap_or(false)) {
        process.env_clear();
        for key in object_keys(context, env)? {
            let name = context.to_string(key)?;
            let value = property_string(context, env, &name)?.unwrap_or_default();
            process.env(name, value);
        }
    }

    let output = match process.spawn() {
        Ok(child) => {
            let pid = child.id();
            match child.wait_with_output() {
                Ok(output) => make_result(
                    context,
                    Some(pid),
                    output.status.code(),
                    signal_name(&output.status),
                    &output.stdout,
                    &output.stderr,
                    None,
                )?,
                Err(error) => make_result(
                    context,
                    Some(pid),
                    None,
                    None,
                    &[],
                    &[],
                    Some(("EIO", error.to_string())),
                )?,
            }
        }
        Err(error) => {
            let code = error.raw_os_error().map_or("UNKNOWN", |code| match code {
                2 => "ENOENT",
                13 => "EACCES",
                _ => "UNKNOWN",
            });
            make_result(context, None, None, None, &[], &[], Some((code, error.to_string())))?
        }
    };
    Ok(output)
}

fn make_result(
    context: &mut NativeContext<'_, NodeHost>,
    pid: Option<u32>,
    status: Option<i32>,
    signal: Option<&str>,
    stdout: &[u8],
    stderr: &[u8],
    error: Option<(&str, String)>,
) -> Result<RootId, RootedError> {
    let result = context.object_rooted()?;
    let pid = match pid {
        Some(pid) => context.number(pid as f64),
        None => context.null(),
    };
    set(context, result, "pid", pid)?;
    let status = match status {
        Some(status) => context.number(status as f64),
        None => context.null(),
    };
    set(context, result, "status", status)?;
    let signal = match signal {
        Some(signal) => context.string_rooted(signal),
        None => context.null(),
    };
    set(context, result, "signal", signal)?;
    let stdout = bytes_array(context, stdout)?;
    let stderr = bytes_array(context, stderr)?;
    set(context, result, "stdout", stdout)?;
    set(context, result, "stderr", stderr)?;
    let error_value = if let Some((code, message)) = error {
        let value = context.object_rooted()?;
        set_string(context, value, "code", code)?;
        set_string(context, value, "message", &message)?;
        value
    } else {
        context.null()
    };
    set(context, result, "error", error_value)?;
    Ok(result)
}

fn bytes_array(context: &mut NativeContext<'_, NodeHost>, bytes: &[u8]) -> Result<RootId, RootedError> {
    let values = bytes
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

fn string_array(context: &mut NativeContext<'_, NodeHost>, array: RootId) -> Result<Vec<String>, RootedError> {
    let length = property(context, array, "length")?;
    let length = context.rooted_value(length).and_then(|value| value.as_number()).unwrap_or_default().max(0.0) as usize;
    (0..length)
        .map(|index| {
            let key = context.string_rooted(&index.to_string());
            let value = context.get_property_rooted(array, key)?;
            context.to_string(value)
        })
        .collect()
}

fn object_keys(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<Vec<RootId>, RootedError> {
    let global = context.global_root()?;
    let object = property(context, global, "Object")?;
    let keys = property(context, object, "keys")?;
    let array = context.call_rooted(keys, object, &[value])?;
    let length = property(context, array, "length")?;
    let length = context.rooted_value(length).and_then(|value| value.as_number()).unwrap_or_default().max(0.0) as usize;
    (0..length)
        .map(|index| {
            let key = context.string_rooted(&index.to_string());
            context.get_property_rooted(array, key)
        })
        .collect()
}

fn property_string(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let value = property(context, object, name)?;
    if context.rooted_value(value).is_some_and(|value| value.is_undefined() || value.is_null()) {
        Ok(None)
    } else {
        context.string_text(value)
    }
}

fn property(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn set_string(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str, value: &str) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? { Ok(()) } else { Err(RootedError::host(format!("cannot set child_process result {name}"))) }
}

#[cfg(unix)]
fn signal_name(status: &std::process::ExitStatus) -> Option<&'static str> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| match signal { 9 => "SIGKILL", 15 => "SIGTERM", 2 => "SIGINT", _ => "SIGTERM" })
}

#[cfg(not(unix))]
fn signal_name(_: &std::process::ExitStatus) -> Option<&'static str> { None }
