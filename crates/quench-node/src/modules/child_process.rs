//! `child_process` — real `spawnSync`: runs a real subprocess, passes
//! a JS env snapshot, and returns a genuine result object. The encoded
//! JS `process.execPath` re-executes a script through the `quench-node`
//! CLI without a shell.

use std::io::Write;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// `child_process.spawnSync(command[, args][, options])`. Returns a
/// result object with `pid`, `status`, `signal`, `stdout`, `stderr`.
/// A spawn failure yields `status: null` plus a coded `error`, never a
/// throw (Node's `throwOnError` default for `spawnSync`).
pub fn spawn_sync(
    _state: &std::rc::Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let command = args.first().map(value_to_string).unwrap_or_default();
    if command.is_empty() {
        return Ok(spawn_error_result("EINVAL", "spawnSync requires a command"));
    }
    let child_args = args.get(1).and_then(string_args).unwrap_or_default();
    let mut cmd = std::process::Command::new(&command);
    cmd.args(&child_args);
    let input = apply_options(&mut cmd, args.get(2));
    let mut child = match spawn_piped(cmd) {
        Ok(child) => child,
        Err(error) => return Ok(spawn_error_result(raw_code(&error), &error.to_string())),
    };
    let pid = child.id();
    pipe_input(&mut child, input.as_deref());
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => return Ok(spawn_error_result(raw_code(&error), &error.to_string())),
    };
    Ok(spawn_result_object(
        pid,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn spawn_piped(mut cmd: std::process::Command) -> std::io::Result<std::process::Child> {
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
}

fn apply_options(cmd: &mut std::process::Command, options: Option<&Value>) -> Option<String> {
    let Some(options) = options else {
        return None;
    };
    if let Some(cwd) = opt_str(options, "cwd") {
        cmd.current_dir(cwd);
    }
    if let Some(env) = opt_env(options) {
        cmd.env_clear().envs(env);
    }
    opt_str(options, "input")
}

fn pipe_input(child: &mut std::process::Child, data: Option<&str>) {
    let Some(data) = data else { return };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(data.as_bytes());
    }
}

fn spawn_result_object(pid: u32, status: Option<i32>, stdout: String, stderr: String) -> Value {
    let status_value = status.map_or(Value::Null, |c| Value::Number(c as f64));
    host_api::object(vec![
        ("pid".to_string(), Value::Number(pid as f64)),
        ("status".to_string(), status_value),
        ("signal".to_string(), Value::Null),
        ("stdout".to_string(), Value::String(stdout.clone())),
        ("stderr".to_string(), Value::String(stderr.clone())),
        (
            "output".to_string(),
            host_api::array(vec![Value::String(stdout), Value::String(stderr)]),
        ),
    ])
}

fn spawn_error_result(code: &str, message: &str) -> Value {
    host_api::object(vec![
        ("pid".to_string(), Value::Null),
        ("status".to_string(), Value::Null),
        ("signal".to_string(), Value::Null),
        ("error".to_string(), coded_error(code, message)),
        ("stdout".to_string(), Value::String(String::new())),
        ("stderr".to_string(), Value::String(String::new())),
    ])
}

/// A Node-style coded `Error` object for a spawn failure.
fn coded_error(code: &str, message: &str) -> Value {
    host_api::object(vec![
        ("name".to_string(), Value::String("Error".to_string())),
        ("message".to_string(), Value::String(message.to_string())),
        ("code".to_string(), Value::String(code.to_string())),
        ("errno".to_string(), Value::Number(-2.0)),
        ("syscall".to_string(), Value::String("spawn".to_string())),
    ])
}

fn raw_code(error: &std::io::Error) -> &'static str {
    if let Some(raw) = error.raw_os_error() {
        return code_name(raw);
    }
    use std::io::ErrorKind::*;
    match error.kind() {
        NotFound => "ENOENT",
        PermissionDenied => "EACCES",
        _ => "EIO",
    }
}

#[cfg(unix)]
fn code_name(raw: i32) -> &'static str {
    match raw {
        1 => "EPERM",
        2 => "ENOENT",
        13 => "EACCES",
        22 => "EINVAL",
        _ => "EIO",
    }
}

#[cfg(not(unix))]
fn code_name(_raw: i32) -> &'static str {
    "EIO"
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

fn string_args(value: &Value) -> Option<Vec<String>> {
    let Value::Array(array) = value else {
        return None;
    };
    let len = array.logical_len();
    let mut out = Vec::new();
    for i in 0..len {
        if let Ok(item) = execute::get_property_result(value, &i.to_string()) {
            if let Ok(s) = execute::to_js_string(&item) {
                out.push(s);
            }
        }
    }
    Some(out)
}

fn opt_str(value: &Value, key: &str) -> Option<String> {
    execute::get_property_result(value, key)
        .ok()
        .and_then(|v| match v {
            Value::Undefined => None,
            other => execute::to_js_string(&other).ok(),
        })
}

/// Snapshot a JS `env` object into a clean env map (empty when the
/// object has no own enumerable keys, e.g. a deleted `process.env`).
fn opt_env(value: &Value) -> Option<std::collections::HashMap<String, String>> {
    if !matches!(value, Value::Object(_)) {
        return None;
    }
    let mut env = std::collections::HashMap::new();
    for key in execute::own_enumerable_keys(value) {
        if let Ok(item) = execute::get_property_result(value, &key) {
            if let Ok(s) = execute::to_js_string(&item) {
                env.insert(key, s);
            }
        }
    }
    Some(env)
}
/// Execute a shell command and return Node's synchronous stdout contract.
pub fn exec_sync(
    _state: &std::rc::Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let command = args.first().map(value_to_string).unwrap_or_default();
    if command.is_empty() {
        return Ok(Value::String(String::new()));
    }
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", &command]);
    let input = apply_options(&mut cmd, args.get(1));
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| execute::type_error("failed to spawn command"))?;
    pipe_input(&mut child, input.as_deref());
    let output = child
        .wait_with_output()
        .map_err(|_| execute::type_error("failed waiting for command"))?;
    if !output.status.success() {
        return Err(execute::type_error("command failed"));
    }
    Ok(Value::String(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

/// Execute a command asynchronously, invoking the supplied callback eagerly.
pub fn exec(
    _state: &std::rc::Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let command = args.first().map(value_to_string).unwrap_or_default();
    let callback = args
        .iter()
        .rev()
        .find(|v| quench_runtime::is_callable(v))
        .cloned();
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", &command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let result = exec_output(&mut cmd);
    if let Some(cb) = callback {
        execute::call(
            &cb,
            &Value::Undefined,
            &[result.0, Value::String(result.1), Value::String(result.2)],
        )?;
    }
    Ok(Value::Undefined)
}

fn exec_output(cmd: &mut std::process::Command) -> (Value, String, String) {
    match cmd.output() {
        Ok(output) => {
            let out = String::from_utf8_lossy(&output.stdout).into_owned();
            let err = String::from_utf8_lossy(&output.stderr).into_owned();
            (
                if output.status.success() {
                    Value::Null
                } else {
                    coded_error("1", "command failed")
                },
                out,
                err,
            )
        }
        Err(error) => (
            coded_error(raw_code(&error), &error.to_string()),
            String::new(),
            String::new(),
        ),
    }
}

/// Execute a file directly (without a shell), with optional callback.
pub fn exec_file(
    _state: &std::rc::Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let file = args.first().map(value_to_string).unwrap_or_default();
    let file_args = args.get(1).and_then(string_args).unwrap_or_default();
    let callback = args
        .iter()
        .rev()
        .find(|v| quench_runtime::is_callable(v))
        .cloned();
    let result = match std::process::Command::new(&file).args(file_args).output() {
        Ok(output) => (
            if output.status.success() {
                Value::Null
            } else {
                coded_error("1", "command failed")
            },
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ),
        Err(error) => (
            coded_error(raw_code(&error), &error.to_string()),
            String::new(),
            String::new(),
        ),
    };
    if let Some(cb) = callback {
        execute::call(
            &cb,
            &Value::Undefined,
            &[result.0, Value::String(result.1), Value::String(result.2)],
        )?;
    }
    Ok(Value::Undefined)
}
/// Synchronous direct-file variant.
pub fn exec_file_sync(
    _state: &std::rc::Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let file = args.first().map(value_to_string).unwrap_or_default();
    let file_args = args.get(1).and_then(string_args).unwrap_or_default();
    let output = std::process::Command::new(file)
        .args(file_args)
        .output()
        .map_err(|_| execute::type_error("failed to execute file"))?;
    if !output.status.success() {
        return Err(execute::type_error("file command failed"));
    }
    Ok(Value::String(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}
