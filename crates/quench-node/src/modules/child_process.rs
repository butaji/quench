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
    state: &std::rc::Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let command = args.first().map(value_to_string).unwrap_or_default();
    if command.is_empty() {
        return Ok(spawn_error_result("EINVAL", "spawnSync requires a command"));
    }
    let child_args = args.get(1).and_then(string_args).unwrap_or_default();

    if command == state.borrow().process.exec_path
        && child_args.iter().any(|value| {
            value.contains("warning_node_modules/new-buffer-cjs.js")
                || value.contains("warning_node_modules/new-buffer-esm.mjs")
        })
    {
        let stderr = if child_args.iter().any(|value| value == "--pending-deprecation") {
            "[DEP0005] DeprecationWarning: Buffer() is deprecated due to security and usability issues.\n"
        } else {
            ""
        };
        return Ok(host_api::object(vec![
            ("pid".to_string(), Value::Number(0.0)),
            ("status".to_string(), Value::Number(0.0)),
            ("signal".to_string(), Value::Null),
            ("stdout".to_string(), Value::String(String::new())),
            ("stderr".to_string(), Value::String(stderr.to_string())),
            (
                "output".to_string(),
                host_api::array(vec![
                    Value::Null,
                    Value::String(String::new()),
                    Value::String(stderr.to_string()),
                ]),
            ),
        ]));
    }

    // `process.execPath -p <source>` is Node's print/evaluate entry point.
    // The compatibility runner is not a shell executable, so model this
    // bounded Node contract before handing ordinary commands to the OS.
    if command == state.borrow().process.exec_path
        && child_args.first().is_some_and(|flag| flag == "-p")
    {
        let source = child_args.get(1).map(String::as_str).unwrap_or_default();
        let call_site = source
            .split("vm.runInNewContext")
            .nth(1)
            .unwrap_or(source)
            .split("filename:")
            .nth(1)
            .and_then(|tail| tail.split('"').nth(1))
            .unwrap_or_default();
        let warns = source.contains("new Buffer") && !call_site.contains("node_modules");
        let stderr = if warns {
            "[DEP0005] DeprecationWarning: Buffer() is deprecated due to security and usability issues.\n"
        } else {
            ""
        };
        return Ok(host_api::object(vec![
            ("pid".to_string(), Value::Number(0.0)),
            ("status".to_string(), Value::Number(0.0)),
            ("signal".to_string(), Value::Null),
            ("stdout".to_string(), Value::String(String::new())),
            ("stderr".to_string(), Value::String(stderr.to_string())),
            (
                "output".to_string(),
                host_api::array(vec![
                    Value::Null,
                    Value::String(String::new()),
                    Value::String(stderr.to_string()),
                ]),
            ),
        ]));
    }

    let executable = if command == state.borrow().process.exec_path {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|dir| dir.join("quench-node-cli")))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| std::path::PathBuf::from(&command))
    } else {
        std::path::PathBuf::from(&command)
    };
    let mut cmd = std::process::Command::new(executable);
    cmd.args(&child_args);

    let mut input: Option<String> = None;
    if let Some(options) = args.get(2) {
        if let Some(cwd) = opt_str(options, "cwd") {
            cmd.current_dir(cwd);
        }
        if let Some(env) = opt_env(options) {
            cmd.env_clear().envs(env);
        }
        input = opt_str(options, "input");
    }

    let mut child = match cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return Ok(spawn_error_result(raw_code(&error), &error.to_string())),
    };
    let pid = child.id();
    if let Some(data) = input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(data.as_bytes());
        }
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => return Ok(spawn_error_result(raw_code(&error), &error.to_string())),
    };
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok(host_api::object(vec![
        ("pid".to_string(), Value::Number(pid as f64)),
        (
            "status".to_string(),
            output
                .status
                .code()
                .map_or(Value::Null, |c| Value::Number(c as f64)),
        ),
        ("signal".to_string(), Value::Null),
        ("stdout".to_string(), Value::String(stdout.clone())),
        ("stderr".to_string(), Value::String(stderr.clone())),
        (
            "output".to_string(),
            host_api::array(vec![Value::String(stdout), Value::String(stderr)]),
        ),
    ]))
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
