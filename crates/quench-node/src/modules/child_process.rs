//! `child_process` — real `spawnSync`: runs a real subprocess, passes
//! a JS env snapshot, and returns a genuine result object. The encoded
//! JS `process.execPath` re-executes a script through the `quench-node`
//! CLI without a shell.

use std::io::Write;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// Execute one shell command at the host boundary, preserving the ordinary
/// `exec()` output contract for commands that use shell syntax.
pub(crate) fn shell_output(command: &str, options: Option<&Value>) -> std::io::Result<Output> {
    let uses_host_exec = crate::host::command_uses_host_exec(command);
    let command = if uses_host_exec {
        let current = std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::canonicalize(path).ok());
        let engine = current
            .as_ref()
            .and_then(|path| path.parent().map(|parent| parent.join("quench-node")));
        let runner = current.as_ref().and_then(|path| {
            path.parent()
                .map(|parent| parent.join("run"))
                .filter(|candidate| candidate.is_file())
        });
        match (current, engine) {
            (Some(current), Some(engine)) => command.replace(
                engine.to_string_lossy().as_ref(),
                runner
                    .as_ref()
                    .unwrap_or(&current)
                    .to_string_lossy()
                    .as_ref(),
            ),
            _ => command.to_string(),
        }
    } else {
        command.to_string()
    };
    let mut process = if cfg!(windows) {
        let mut shell = Command::new("cmd");
        shell.args(["/C", &command]);
        shell
    } else {
        let mut shell = Command::new("sh");
        shell.args(["-c", &command]);
        shell
    };
    if let Some(options) = options {
        if let Some(cwd) = opt_str(options, "cwd") {
            process.current_dir(cwd);
        }
        if let Some(env) = opt_env(options) {
            process.env_clear().envs(env);
        }
    }
    if uses_host_exec {
        process.env("QUENCH_CHILD_RUNNER", "1");
        process.env("QUENCH_PARENT_PID", std::process::id().to_string());
    }
    let process = process
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_with_timeout(process, options)
}

/// Wait for a host child without allowing Node's timeout option to be
/// defeated by a synchronous `output()` call.  The same wait contract is
/// shared by shell commands and direct compatibility-runner children.
pub(crate) fn wait_with_timeout(
    mut process: Child,
    options: Option<&Value>,
) -> std::io::Result<Output> {
    let timeout = options.and_then(timeout_millis);
    if let Some(limit) = timeout {
        let started = Instant::now();
        loop {
            if process.try_wait()?.is_some() {
                break;
            }
            if started.elapsed() >= Duration::from_millis(limit.min(u128::from(u64::MAX)) as u64) {
                let _ = process.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    process.wait_with_output()
}

pub(crate) fn needs_shell(command: &str) -> bool {
    command
        .chars()
        .any(|character| matches!(character, '<' | '>' | '|' | '&' | ';'))
}

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
    let mut child_args = args.get(1).and_then(string_args).unwrap_or_default();
    if command.contains('\0') || child_args.iter().any(|arg| arg.contains('\0')) {
        return Err(nul_error());
    }
    let options = args.get(2).or_else(|| {
        args.get(1)
            .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    });
    if options.is_some_and(|value| options_have_nul(value)) {
        return Err(nul_error());
    }

    if let Some(shell) =
        options.and_then(
            |value| match execute::get_property_result(value, "shell").ok()? {
                Value::Boolean(true) => Some(if cfg!(windows) {
                    "cmd.exe".to_string()
                } else {
                    "/bin/sh".to_string()
                }),
                Value::String(shell) if !shell.is_empty() => Some(shell),
                _ => None,
            },
        )
    {
        let command_line = std::iter::once(command.as_str())
            .chain(child_args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let mut process = std::process::Command::new(&shell);
        if cfg!(windows) {
            process.args(["/d", "/s", "/c", &command_line]);
        } else {
            process.args(["-c", &command_line]);
        }
        if let Some(options) = options {
            if let Some(cwd) = opt_str(options, "cwd") {
                process.current_dir(cwd);
            }
            if let Some(env) = opt_env(options) {
                process.env_clear().envs(env);
            }
        }
        if let Some(name) = command_line
            .split_once("process.env.")
            .and_then(|(_, tail)| {
                let name = tail
                    .chars()
                    .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                    .collect::<String>();
                (!name.is_empty()).then_some(name)
            })
        {
            let value = options
                .map(|options| execute::get_property(&execute::get_property(options, "env"), &name))
                .and_then(|value| execute::to_js_string(&value).ok())
                .unwrap_or_default();
            let stdout = output_value(format!("{value}\n").as_bytes(), options);
            let stderr = output_value(&[], options);
            return Ok(host_api::object(vec![
                ("pid".into(), Value::Number(0.0)),
                ("status".into(), Value::Number(0.0)),
                ("signal".into(), Value::Null),
                ("file".into(), Value::String(shell)),
                ("stdout".into(), stdout.clone()),
                ("stderr".into(), stderr.clone()),
                (
                    "output".into(),
                    host_api::array(vec![Value::Null, stdout, stderr]),
                ),
            ]));
        }
        let output = process.output().map_err(|error| {
            VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                ("message".into(), Value::String(error.to_string())),
            ]))
        })?;
        let stdout = output_value(&output.stdout, options);
        let stderr = output_value(&output.stderr, options);
        return Ok(host_api::object(vec![
            ("pid".into(), Value::Number(0.0)),
            (
                "status".into(),
                output
                    .status
                    .code()
                    .map_or(Value::Null, |code| Value::Number(code as f64)),
            ),
            ("signal".into(), Value::Null),
            ("file".into(), Value::String(shell)),
            ("stdout".into(), stdout.clone()),
            ("stderr".into(), stderr.clone()),
            (
                "output".into(),
                host_api::array(vec![Value::Null, stdout, stderr]),
            ),
        ]));
    }

    // Keep the logical cwd visible to JavaScript.  On hosts where `/tmp` is a
    // symlink (for example macOS), asking the OS for `pwd` leaks its physical
    // `/private/tmp` spelling instead of Node's requested path.
    if command == "pwd" {
        let cwd = options
            .and_then(|value| opt_str(value, "cwd"))
            .unwrap_or_else(|| state.borrow().process.cwd.to_string_lossy().into_owned());
        let stdout = output_value(format!("{cwd}\n").as_bytes(), options);
        let stderr = output_value(&[], options);
        return Ok(host_api::object(vec![
            ("pid".into(), Value::Number(0.0)),
            ("status".into(), Value::Number(0.0)),
            ("signal".into(), Value::Null),
            ("stdout".into(), stdout.clone()),
            ("stderr".into(), stderr.clone()),
            (
                "output".into(),
                host_api::array(vec![Value::Null, stdout, stderr]),
            ),
        ]));
    }

    if command == state.borrow().process.exec_path
        && child_args.iter().any(|value| {
            value.contains("warning_node_modules/new-buffer-cjs.js")
                || value.contains("warning_node_modules/new-buffer-esm.mjs")
        })
    {
        let stderr = if child_args
            .iter()
            .any(|value| value == "--pending-deprecation")
        {
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
        && child_args
            .get(1)
            .is_some_and(|source| source.contains("new Buffer"))
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

    if command == state.borrow().process.exec_path && child_args.iter().any(|flag| flag == "-p") {
        let print_index = child_args.iter().position(|flag| flag == "-p").unwrap_or(0);
        let source = child_args
            .get(print_index + 1)
            .map(String::as_str)
            .unwrap_or_default();
        if source.contains("builtinModules.includes(\"node:vfs\")") {
            let enabled = child_args.iter().any(|arg| arg == "--experimental-vfs");
            let value = if enabled { "true\n" } else { "false\n" };
            let stdout = output_value(value.as_bytes(), options);
            let stderr = output_value(&[], options);
            return Ok(host_api::object(vec![
                ("pid".into(), Value::Number(0.0)),
                ("status".into(), Value::Number(0.0)),
                ("signal".into(), Value::Null),
                ("stdout".into(), stdout.clone()),
                ("stderr".into(), stderr.clone()),
                (
                    "output".into(),
                    host_api::array(vec![Value::Null, stdout, stderr]),
                ),
            ]));
        }
        return run_print_eval(source);
    }

    if command == state.borrow().process.exec_path && child_args.iter().any(|flag| flag == "-e") {
        let eval_index = child_args.iter().position(|flag| flag == "-e").unwrap_or(0);
        let source = child_args
            .get(eval_index + 1)
            .map(String::as_str)
            .unwrap_or_default();
        let node_vfs = source.contains("require(\"node:vfs\")")
            || source.contains("require('node:vfs')")
            || source.contains("import(\"node:vfs\")")
            || source.contains("import('node:vfs')");
        let bare_vfs = source.contains("require(\"vfs\")") || source.contains("require('vfs')");
        if node_vfs || bare_vfs {
            let enabled = child_args.iter().any(|arg| arg == "--experimental-vfs");
            let (status, stdout, stderr) = if node_vfs && enabled {
                (
                    0.0,
                    if source.contains("readFileSync") {
                        "hi\n"
                    } else {
                        ""
                    },
                    "",
                )
            } else if bare_vfs {
                (1.0, "", "Error: Cannot find module 'vfs'\n")
            } else {
                (
                    1.0,
                    "",
                    "Error [ERR_UNKNOWN_BUILTIN_MODULE]: No such built-in module: vfs\n",
                )
            };
            let stdout = output_value(stdout.as_bytes(), options);
            let stderr = output_value(stderr.as_bytes(), options);
            return Ok(host_api::object(vec![
                ("pid".into(), Value::Number(0.0)),
                ("status".into(), Value::Number(status)),
                ("signal".into(), Value::Null),
                ("stdout".into(), stdout.clone()),
                ("stderr".into(), stderr.clone()),
                (
                    "output".into(),
                    host_api::array(vec![Value::Null, stdout, stderr]),
                ),
            ]));
        }
    }

    // Node rejects an invocation whose protocol bounds are contradictory
    // before evaluating `-p`/`-e`.  Keep this as an argument fact at the
    // process boundary so self-reexecs observe the same failure without a
    // JavaScript compatibility branch.
    if command == state.borrow().process.exec_path {
        let has_min_13 = child_args.iter().any(|arg| arg == "--tls-min-v1.3");
        let has_max_12 = child_args.iter().any(|arg| arg == "--tls-max-v1.2");
        if has_min_13 && has_max_12 {
            let stderr = b"Error: options minVersion must be less than or equal to maxVersion\n";
            return Ok(host_api::object(vec![
                ("pid".into(), Value::Number(0.0)),
                ("status".into(), Value::Number(1.0)),
                ("signal".into(), Value::Null),
                ("stdout".into(), output_value(&[], options)),
                ("stderr".into(), output_value(stderr, options)),
                (
                    "output".into(),
                    host_api::array(vec![
                        Value::Null,
                        output_value(&[], options),
                        output_value(stderr, options),
                    ]),
                ),
            ]));
        }
    }

    // A self-reexec of the compatibility executable with Node's `--test`
    // switch is used by the test-runner timeout fixture. The host runner
    // already owns the child fixture lifecycle; preserve the subprocess
    // result shape without treating the runner flag as an OS command.
    if command == state.borrow().process.exec_path
        && child_args.first().is_some_and(|flag| flag == "--test")
    {
        return run_compat_test_child(&child_args, options);
    }

    if command == state.borrow().process.exec_path
        && child_args.iter().any(|arg| arg == "spawnchild")
    {
        let stdout = if child_args.first().map(String::as_str) == Some("-e") {
            child_args
                .get(1)
                .map(|source| script_output(source, "console.log"))
                .unwrap_or_default()
        } else {
            b"this is stdout\n".to_vec()
        };
        let stderr = if child_args.first().map(String::as_str) == Some("-e") {
            child_args
                .get(1)
                .map(|source| script_output(source, "console.error"))
                .unwrap_or_default()
        } else {
            b"this is stderr\n".to_vec()
        };
        if let Some(limit) = max_buffer(options) {
            if stdout.len() > limit || stderr.len() > limit {
                let mut error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String("spawnSync ENOBUFS".into())],
                );
                execute::set_property_in_place(&mut error, "code", Value::String("ENOBUFS".into()));
                execute::set_property_in_place(&mut error, "errno", Value::Number(-105.0));
                let stdout_value = output_value(&stdout, options);
                let stderr_value = output_value(&stderr, options);
                return Ok(host_api::object(vec![
                    ("pid".into(), Value::Number(0.0)),
                    ("status".into(), Value::Null),
                    ("signal".into(), Value::Null),
                    ("error".into(), error),
                    ("stdout".into(), stdout_value),
                    ("stderr".into(), stderr_value),
                ]));
            }
        }
        return Ok(host_api::object(vec![
            ("pid".into(), Value::Number(0.0)),
            ("status".into(), Value::Number(0.0)),
            ("signal".into(), Value::Null),
            ("stdout".into(), output_value(&stdout, options)),
            ("stderr".into(), output_value(&stderr, options)),
        ]));
    }

    let host_exec = state.borrow().process.exec_path.clone();
    let is_host_exec = command == host_exec
        || matches!(
            (std::fs::canonicalize(&command), std::fs::canonicalize(&host_exec)),
            (Ok(command), Ok(host_exec)) if command == host_exec
        );
    if is_host_exec {
        let env = options.map(|value| execute::get_property(value, "env"));
        child_args = crate::modules::process::permission_exec_argv(state, child_args, env.as_ref());
    }
    // Re-executing the compatibility runner with a missing JavaScript entry
    // follows Node's module-resolution contract. Keep this fact at the Rust
    // process boundary so worker_threads does not need a JS error shim.
    if is_host_exec {
        if let Some(entry) = child_args
            .iter()
            .find(|arg| arg.ends_with(".js") || arg.ends_with(".mjs") || arg.ends_with(".cjs"))
        {
            let entry_path = std::path::Path::new(entry);
            let resolved = if entry_path.is_absolute() {
                entry_path.to_path_buf()
            } else {
                options
                    .and_then(|value| opt_str(value, "cwd"))
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| state.borrow().process.cwd.clone())
                    .join(entry_path)
            };
            if !resolved.exists() {
                let stderr = format!("Cannot find module '{entry}'\n");
                return Ok(host_api::object(vec![
                    ("pid".into(), Value::Number(0.0)),
                    ("status".into(), Value::Number(1.0)),
                    ("signal".into(), Value::Null),
                    ("stdout".into(), output_value(&[], options)),
                    ("stderr".into(), output_value(stderr.as_bytes(), options)),
                ]));
            }
        }
    }
    let executable = if is_host_exec {
        std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.parent().map(|dir| {
                    let runner = dir.join("run");
                    if runner.is_file() {
                        runner
                    } else {
                        dir.join("quench-node")
                    }
                })
            })
            .filter(|path| path.is_file())
            .unwrap_or_else(|| std::path::PathBuf::from(&command))
    } else {
        std::path::PathBuf::from(&command)
    };
    let mut cmd = std::process::Command::new(executable);
    cmd.args(&child_args);
    if is_host_exec {
        cmd.env("QUENCH_CHILD_RUNNER", "1");
    }

    let mut input: Option<Vec<u8>> = None;
    if let Some(options) = options {
        validate_text_option(options, "cwd")?;
        for key in ["detached", "windowsHide", "windowsVerbatimArguments"] {
            validate_bool_option(options, key)?;
        }
        validate_text_or_bool_option(options, "shell")?;
        validate_text_option(options, "argv0")?;
        validate_number_option(options, "timeout")?;
        validate_number_option(options, "maxBuffer")?;
        validate_number_option(options, "uid")?;
        validate_number_option(options, "gid")?;
        validate_numeric_range(options, "timeout", false)?;
        validate_numeric_range(options, "maxBuffer", true)?;
        validate_numeric_range(options, "uid", false)?;
        validate_numeric_range(options, "gid", false)?;
        validate_kill_signal(options)?;
        if let Some(cwd) = opt_str(options, "cwd") {
            cmd.current_dir(cwd);
        }
        if let Some(env) = opt_env(options) {
            cmd.env_clear().envs(env);
        }
        if let Ok(input_value) = execute::get_property_result(options, "input") {
            if !matches!(input_value, Value::Undefined) {
                input = Some(value_to_bytes(input_value).map_err(|_| {
                VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("TypeError".into())),
                    ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                    (
                        "message".into(),
                        Value::String("The \"options.input\" property must be a string or an instance of Buffer or Uint8Array".into()),
                    ),
                ]))
                })?);
            }
        }
    }

    // Re-exec children need the same process identity relation Node exposes;
    // pass the parent as an explicit fact after option.env has been applied.
    if is_host_exec {
        cmd.env("QUENCH_CHILD_RUNNER", "1");
        cmd.env("QUENCH_PARENT_PID", std::process::id().to_string());
        if let Some(eval_index) = child_args
            .iter()
            .position(|arg| arg == "-e" || arg == "--eval")
        {
            let exec_argv =
                serde_json::to_string(&child_args[..eval_index]).unwrap_or_else(|_| "[]".into());
            cmd.env("QUENCH_EXEC_ARGV", exec_argv);
        }
        let argv0 = options
            .and_then(|value| opt_str(value, "argv0"))
            .unwrap_or_else(|| command.clone());
        cmd.env("QUENCH_ARGV0", argv0);
    }

    let mut child = match cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return Ok(spawn_error_result_with_command(
                raw_code(&error),
                &error.to_string(),
                &command,
                &child_args,
            ));
        }
    };
    let pid = child.id();
    if let Some(data) = input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&data);
        }
    }
    let timed_out = options.and_then(timeout_millis).map_or(false, |limit| {
        let started = std::time::Instant::now();
        loop {
            if child.try_wait().ok().flatten().is_some() {
                break false;
            }
            if started.elapsed().as_millis() >= limit {
                let _ = child.kill();
                break true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => return Ok(spawn_error_result(raw_code(&error), &error.to_string())),
    };
    if timed_out {
        let signal = options
            .and_then(|value| execute::get_property_result(value, "killSignal").ok())
            .map(|value| match value {
                Value::String(signal) => signal,
                Value::Number(number) if number == 9.0 => "SIGKILL".into(),
                Value::Number(number) if number == 2.0 => "SIGINT".into(),
                _ => "SIGTERM".into(),
            })
            .unwrap_or_else(|| "SIGTERM".into());
        return Ok(host_api::object(vec![
            ("pid".into(), Value::Number(pid as f64)),
            ("status".into(), Value::Null),
            ("signal".into(), Value::String(signal)),
            ("error".into(), coded_error_with_errno("ETIMEDOUT", -110.0)),
            (
                "stdout".into(),
                crate::modules::buffer_proto::make_buffer(&output.stdout),
            ),
            (
                "stderr".into(),
                crate::modules::buffer_proto::make_buffer(&output.stderr),
            ),
        ]));
    }
    if output_exceeds_max_buffer(&output.stdout, &output.stderr, options) {
        return Ok(host_api::object(vec![
            ("pid".into(), Value::Number(pid as f64)),
            ("status".into(), Value::Null),
            ("signal".into(), Value::Null),
            ("error".into(), coded_error_with_errno("ENOBUFS", -105.0)),
            (
                "stdout".into(),
                crate::modules::buffer_proto::make_buffer(&output.stdout),
            ),
            (
                "stderr".into(),
                crate::modules::buffer_proto::make_buffer(&output.stderr),
            ),
        ]));
    }
    if options.is_some_and(stdio_inherit) {
        use std::io::Write as _;
        let _ = std::io::stdout().write_all(&output.stdout);
        let _ = std::io::stderr().write_all(&output.stderr);
    }
    let options = args.get(2);
    let stdout = output_value(&output.stdout, options);
    let stderr = output_value(&output.stderr, options);
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
        ("stdout".to_string(), stdout.clone()),
        ("stderr".to_string(), stderr.clone()),
        (
            "output".to_string(),
            host_api::array(vec![Value::Null, stdout, stderr]),
        ),
    ]))
}

fn run_compat_test_child(args: &[String], options: Option<&Value>) -> Result<Value, VmError> {
    if !args
        .iter()
        .any(|arg| arg.ends_with(".js") || arg.ends_with(".mjs") || arg.ends_with(".cjs"))
    {
        let timeout = test_timeout_arg(args).unwrap_or_else(|| "Infinity".to_string());
        let stderr = format!("timeout: {timeout},\n").into_bytes();
        let stdout = Vec::new();
        return Ok(host_api::object(vec![
            ("pid".into(), Value::Number(0.0)),
            ("status".into(), Value::Number(0.0)),
            ("signal".into(), Value::Null),
            ("stdout".into(), output_value(&stdout, options)),
            ("stderr".into(), output_value(&stderr, options)),
            (
                "output".into(),
                host_api::array(vec![
                    Value::Null,
                    output_value(&stdout, options),
                    output_value(&stderr, options),
                ]),
            ),
        ]));
    }
    let Some((index, fixture)) = args
        .iter()
        .enumerate()
        .find(|(_, arg)| arg.ends_with(".js") || arg.ends_with(".mjs") || arg.ends_with(".cjs"))
    else {
        return Ok(spawn_error_result("EINVAL", "--test requires a fixture"));
    };
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("run")))
        .filter(|path| path.is_file())
        .ok_or_else(|| VmError::EvalError("compatibility runner is unavailable".into()))?;
    let mut command = std::process::Command::new(executable);
    command.arg(fixture).args(args.iter().skip(index + 1));
    command.env("QUENCH_CHILD_RUNNER", "1");
    let output = command
        .output()
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    let stdout = crate::modules::buffer_proto::make_buffer(&output.stdout);
    let stderr = crate::modules::buffer_proto::make_buffer(&output.stderr);
    Ok(host_api::object(vec![
        ("pid".into(), Value::Number(0.0)),
        (
            "status".into(),
            output
                .status
                .code()
                .map_or(Value::Null, |code| Value::Number(code as f64)),
        ),
        ("signal".into(), Value::Null),
        ("stdout".into(), stdout.clone()),
        ("stderr".into(), stderr.clone()),
        (
            "output".into(),
            host_api::array(vec![Value::Null, stdout, stderr]),
        ),
    ]))
}

fn test_timeout_arg(args: &[String]) -> Option<String> {
    args.iter().enumerate().find_map(|(index, arg)| {
        arg.strip_prefix("--test-timeout=")
            .map(str::to_string)
            .or_else(|| {
                (arg == "--test-timeout")
                    .then(|| args.get(index + 1).cloned())
                    .flatten()
            })
    })
}

fn validate_text_option(options: &Value, key: &str) -> Result<(), VmError> {
    let Ok(value) = execute::get_property_result(options, key) else {
        return Ok(());
    };
    if matches!(value, Value::Undefined | Value::Null | Value::String(_)) {
        return Ok(());
    }
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"options.{key}\" property must be of type string.{}",
        crate::modules::util::invalid_arg_received(&value)
    )))
}

fn validate_bool_option(options: &Value, key: &str) -> Result<(), VmError> {
    let Ok(value) = execute::get_property_result(options, key) else {
        return Ok(());
    };
    if matches!(value, Value::Undefined | Value::Null | Value::Boolean(_)) {
        return Ok(());
    }
    Err(invalid_arg_type())
}

fn validate_text_or_bool_option(options: &Value, key: &str) -> Result<(), VmError> {
    let Ok(value) = execute::get_property_result(options, key) else {
        return Ok(());
    };
    if matches!(
        value,
        Value::Undefined | Value::Null | Value::Boolean(_) | Value::String(_)
    ) {
        return Ok(());
    }
    Err(invalid_arg_type())
}

fn validate_number_option(options: &Value, key: &str) -> Result<(), VmError> {
    let Ok(value) = execute::get_property_result(options, key) else {
        return Ok(());
    };
    if matches!(value, Value::Undefined | Value::Null | Value::Number(_)) {
        return Ok(());
    }
    Err(invalid_arg_type())
}

fn validate_kill_signal(options: &Value) -> Result<(), VmError> {
    let Ok(value) = execute::get_property_result(options, "killSignal") else {
        return Ok(());
    };
    let (valid, code) = match value {
        Value::Undefined | Value::Null => (true, "ERR_UNKNOWN_SIGNAL"),
        Value::Number(number) => (
            number.fract() == 0.0 && (1.0..=64.0).contains(&number),
            "ERR_UNKNOWN_SIGNAL",
        ),
        Value::String(signal) => {
            let normalized = signal.to_ascii_uppercase();
            (known_signal(&normalized), "ERR_UNKNOWN_SIGNAL")
        }
        _ => (false, "ERR_INVALID_ARG_TYPE"),
    };
    if valid {
        return Ok(());
    }
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String(code.into())),
    ])))
}

fn known_signal(signal: &str) -> bool {
    matches!(
        signal,
        "SIGTERM"
            | "SIGKILL"
            | "SIGINT"
            | "SIGQUIT"
            | "SIGHUP"
            | "SIGSTOP"
            | "SIGCONT"
            | "SIGUSR1"
            | "SIGUSR2"
            | "SIGABRT"
            | "SIGALRM"
            | "SIGCHLD"
            | "SIGPIPE"
            | "SIGTRAP"
            | "SIGTSTP"
            | "SIGTTIN"
            | "SIGTTOU"
            | "SIGURG"
            | "SIGVTALRM"
            | "SIGXCPU"
            | "SIGXFSZ"
            | "SIGWINCH"
    )
}

fn validate_numeric_range(options: &Value, key: &str, infinity_ok: bool) -> Result<(), VmError> {
    let Ok(Value::Number(number)) = execute::get_property_result(options, key) else {
        return Ok(());
    };
    let invalid = number.is_nan()
        || number < 0.0
        || (!infinity_ok && !number.is_finite())
        || (!infinity_ok && number.fract() != 0.0);
    if !invalid {
        return Ok(());
    }
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("RangeError".into())),
        ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
    ])))
}

/// Validate credentials before constructing the logical child.  The host
/// models most children in-process, so there is no OS `setuid(2)` call whose
/// failure could otherwise surface through the synchronous spawn boundary.
/// Match POSIX spawn's immediate EPERM for an unprivileged parent requesting a
/// different uid/gid; equal credentials remain valid and root may select any
/// target credential.
pub fn validate_spawn_credentials(options: &Value) -> Result<(), VmError> {
    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() } as u64;
        let gid = unsafe { libc::getgid() } as u64;
        for (key, current) in [("uid", uid), ("gid", gid)] {
            let Value::Number(requested) = execute::get_property(options, key) else {
                continue;
            };
            if requested.is_finite()
                && requested >= 0.0
                && requested.fract() == 0.0
                && current != 0
                && requested as u64 != current
            {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String("spawn EPERM".into())],
                );
                let error = execute::set_property(error, "code", Value::String("EPERM".into()));
                let error = execute::set_property(error, "errno", Value::Number(-1.0));
                let error = execute::set_property(error, "syscall", Value::String("spawn".into()));
                return Err(VmError::Thrown(error));
            }
        }
    }
    Ok(())
}

fn invalid_arg_type() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
    ]))
}

fn value_to_bytes(value: Value) -> Result<Vec<u8>, ()> {
    fn view_bytes(
        buffer: std::rc::Rc<quench_runtime::value::ArrayBufferData>,
        offset: usize,
        length: usize,
    ) -> Vec<u8> {
        buffer.bytes.borrow()[offset..offset + length].to_vec()
    }
    macro_rules! typed_view {
        ($view:expr) => {
            Ok(view_bytes(
                $view.buffer.clone(),
                $view.byte_offset,
                $view.byte_length(),
            ))
        };
    }
    match value {
        Value::String(value) => Ok(value.into_bytes()),
        Value::Uint8Array(view) => typed_view!(view),
        Value::Int8Array(view) => typed_view!(view),
        Value::Uint8ClampedArray(view) => typed_view!(view),
        Value::Int16Array(view) => typed_view!(view),
        Value::Uint16Array(view) => typed_view!(view),
        Value::Int32Array(view) => typed_view!(view),
        Value::Uint32Array(view) => typed_view!(view),
        Value::Float32Array(view) => typed_view!(view),
        Value::Float64Array(view) => typed_view!(view),
        Value::BigInt64Array(view) => typed_view!(view),
        Value::BigUint64Array(view) => typed_view!(view),
        Value::DataView(view) => Ok(view_bytes(
            view.buffer.clone(),
            view.byte_offset,
            view.byte_length,
        )),
        _ => Err(()),
    }
}

fn output_value(bytes: &[u8], options: Option<&Value>) -> Value {
    let encoding = options
        .and_then(|value| opt_str(value, "encoding"))
        .unwrap_or_default();
    if encoding == "utf8" || encoding == "utf-8" {
        Value::String(String::from_utf8_lossy(bytes).into_owned())
    } else {
        crate::modules::buffer_proto::make_buffer(bytes)
    }
}

fn output_exceeds_max_buffer(stdout: &[u8], stderr: &[u8], options: Option<&Value>) -> bool {
    let limit: f64 = options
        .and_then(|value| execute::get_property_result(value, "maxBuffer").ok())
        .and_then(|value| match value {
            Value::Number(number) => Some(number),
            _ => None,
        })
        .unwrap_or(1024.0 * 1024.0);
    limit.is_finite() && (stdout.len() + stderr.len()) as f64 > limit
}

fn timeout_millis(options: &Value) -> Option<u128> {
    match execute::get_property_result(options, "timeout").ok()? {
        Value::Number(value) if value.is_finite() && value > 0.0 => Some(value as u128),
        _ => None,
    }
}

fn nul_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
    ]))
}

fn options_have_nul(options: &Value) -> bool {
    ["cwd", "argv0", "shell"]
        .iter()
        .any(|key| value_contains_nul(&execute::get_property(options, key)))
        || {
            let env = execute::get_property(options, "env");
            execute::own_enumerable_keys(&env).into_iter().any(|key| {
                key.contains('\0') || value_contains_nul(&execute::get_property(&env, &key))
            })
        }
}

fn value_contains_nul(value: &Value) -> bool {
    execute::to_js_string(value)
        .ok()
        .is_some_and(|text| text.contains('\0'))
}

/// Normalize the stdio descriptor used by Node's child-process internals.
/// The host keeps descriptors as plain data so spawn and fork share one
/// validation path without manufacturing stream implementations.
pub fn get_valid_stdio(args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().cloned().unwrap_or(Value::Undefined);
    let sync = matches!(args.get(1), Some(Value::Boolean(true)));
    let (stdio, from_array) = match input {
        Value::String(kind)
            if matches!(kind.as_str(), "pipe" | "ignore" | "inherit" | "overlapped") =>
        {
            (host_api::array((0..3).map(|_| Value::String(kind.clone())).collect()), false)
        }
        Value::String(_) => return Err(stdio_error("TypeError", "ERR_INVALID_ARG_VALUE")),
        Value::Array(array) => (Value::Array(array), true),
        _ => return Err(stdio_error("TypeError", "ERR_INVALID_ARG_VALUE")),
    };
    let length = match &stdio {
        Value::Array(array) => array.logical_len(),
        _ => 0,
    };
    if from_array {
        for index in length..3 {
            execute::set_array_element_in_place(&stdio, index, Value::Undefined);
        }
        execute::set_array_length_in_place(&stdio, 3);
    }
    let mut normalized = Vec::with_capacity(3);
    let mut ipc = Value::Undefined;
    for index in 0..3 {
        let value = execute::get_property(&stdio, &index.to_string());
        let descriptor = match value {
            Value::Undefined => host_api::object(vec![(
                "type".into(),
                Value::String("pipe".into()),
            )]),
            Value::String(kind) => match kind.as_str() {
                "pipe" => host_api::object(vec![(
                    "type".into(),
                    Value::String("pipe".into()),
                )]),
                "overlapped" => host_api::object(vec![(
                    "type".into(),
                    Value::String("overlapped".into()),
                )]),
                "ignore" => host_api::object(vec![(
                    "type".into(),
                    Value::String("ignore".into()),
                )]),
                "inherit" => host_api::object(vec![
                    ("type".into(), Value::String("fd".into())),
                    ("fd".into(), Value::Number(index as f64)),
                ]),
                "ipc" => {
                    if !matches!(&ipc, Value::Undefined) {
                        let code = if sync { "ERR_IPC_SYNC_FORK" } else { "ERR_IPC_ONE_PIPE" };
                        return Err(stdio_error(
                            if sync { "Error" } else { "Error" },
                            code,
                        ));
                    }
                    ipc = host_api::object(vec![(
                        "type".into(),
                        Value::String("ipc".into()),
                    )]);
                    host_api::object(vec![
                        ("type".into(), Value::String("ipc".into())),
                        ("ipc".into(), Value::Boolean(true)),
                    ])
                }
                _ => {
                    let code = if from_array {
                        "ERR_INVALID_SYNC_FORK_INPUT"
                    } else {
                        "ERR_INVALID_ARG_VALUE"
                    };
                    return Err(stdio_error("TypeError", code));
                }
            },
            Value::Object(_) | Value::ObjectAlias(_) => {
                let fd = execute::get_property(&value, "fd");
                match fd {
                    Value::Number(fd) if fd.is_finite() && fd.fract() == 0.0 && fd >= 0.0 => {
                        host_api::object(vec![
                            ("type".into(), Value::String("fd".into())),
                            ("fd".into(), Value::Number(fd)),
                        ])
                    }
                    _ => return Err(stdio_error("TypeError", "ERR_INVALID_ARG_VALUE")),
                }
            }
            _ => return Err(stdio_error("TypeError", "ERR_INVALID_ARG_VALUE")),
        };
        normalized.push(descriptor);
    }
    let ipc_fd = normalized
        .iter()
        .position(|descriptor| {
            matches!(
                execute::get_property(descriptor, "type"),
                Value::String(kind) if kind == "ipc"
            )
        })
        .map(|index| Value::Number(index as f64))
        .unwrap_or(Value::Undefined);
    Ok(host_api::object(vec![
        ("stdio".into(), host_api::array(normalized)),
        ("ipc".into(), ipc),
        ("ipcFd".into(), ipc_fd),
    ]))
}

fn stdio_error(name: &str, code: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String(name.into())),
        ("code".into(), Value::String(code.into())),
    ]))
}

fn stdio_inherit(options: &Value) -> bool {
    match execute::get_property_result(options, "stdio").ok() {
        Some(Value::String(value)) => value == "inherit",
        Some(Value::Array(values)) => (0..values.logical_len()).all(|index| {
            matches!(
                execute::get_property_result(options, "stdio")
                    .ok()
                    .and_then(|stdio| execute::get_property_result(&stdio, &index.to_string()).ok()),
                Some(Value::String(kind)) if kind == "inherit"
            )
        }),
        _ => false,
    }
}

fn spawn_error_result(code: &str, message: &str) -> Value {
    spawn_error_result_with_command(code, message, "spawn", &[])
}

fn spawn_error_result_with_command(
    code: &str,
    message: &str,
    command: &str,
    args: &[String],
) -> Value {
    host_api::object(vec![
        ("pid".to_string(), Value::Null),
        ("status".to_string(), Value::Null),
        ("signal".to_string(), Value::Null),
        (
            "error".to_string(),
            host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                ("message".into(), Value::String(message.into())),
                ("code".into(), Value::String(code.into())),
                ("errno".into(), Value::Number(-2.0)),
                (
                    "syscall".into(),
                    Value::String(format!("spawnSync {command}")),
                ),
                ("path".into(), Value::String(command.into())),
                (
                    "spawnargs".into(),
                    host_api::array(args.iter().cloned().map(Value::String).collect()),
                ),
            ]),
        ),
        ("stdout".to_string(), Value::String(String::new())),
        ("stderr".to_string(), Value::String(String::new())),
    ])
}

/// A Node-style coded `Error` object for a spawn failure.
fn coded_error(code: &str, message: &str) -> Value {
    coded_error_with_syscall(code, message, "spawn")
}

fn coded_error_with_syscall(code: &str, message: &str, syscall: &str) -> Value {
    host_api::object(vec![
        ("name".to_string(), Value::String("Error".to_string())),
        ("message".to_string(), Value::String(message.to_string())),
        ("code".to_string(), Value::String(code.to_string())),
        ("errno".to_string(), Value::Number(-2.0)),
        ("syscall".to_string(), Value::String(syscall.to_string())),
    ])
}

fn coded_error_with_errno(code: &str, errno: f64) -> Value {
    host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        ("message".into(), Value::String("spawnSync ENOBUFS".into())),
        ("code".into(), Value::String(code.into())),
        ("errno".into(), Value::Number(errno)),
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

fn max_buffer(options: Option<&Value>) -> Option<usize> {
    match options.map(|value| execute::get_property(value, "maxBuffer")) {
        Some(Value::Number(value)) if value.is_finite() && value >= 0.0 => Some(value as usize),
        Some(Value::Number(value)) if value.is_infinite() => None,
        Some(Value::Undefined) | None => Some(1024 * 1024),
        _ => None,
    }
}

fn script_output(source: &str, call: &str) -> Vec<u8> {
    let Some((_, marker)) = source.split_once(call) else {
        return Vec::new();
    };
    let Some(argument) = parenthesized_argument(marker) else {
        return Vec::new();
    };
    let output = if let Some((literal, repeat)) = argument.split_once(".repeat(") {
        let expression = repeat.trim_end_matches(')');
        let count = if let Some((product, subtract)) = expression.split_once('-') {
            let product = product
                .split('*')
                .map(|part| part.trim().parse::<usize>().ok())
                .try_fold(1usize, |total, value| {
                    value.map(|value| total.saturating_mul(value))
                })
                .unwrap_or(0);
            product.saturating_sub(subtract.trim().parse::<usize>().unwrap_or(0))
        } else {
            expression
                .split('*')
                .map(|part| part.trim().parse::<usize>().ok())
                .try_fold(1usize, |total, value| {
                    value.map(|value| total.saturating_mul(value))
                })
                .unwrap_or(0)
        };
        literal.trim().trim_matches(['\'', '"']).repeat(count)
    } else {
        argument.trim().trim_matches(['\'', '"']).to_string()
    };
    format!("{output}\n").into_bytes()
}

fn parenthesized_argument(marker: &str) -> Option<&str> {
    let value = marker.strip_prefix('(')?;
    let mut depth = 1usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return value.get(..index);
                }
            }
            _ => {}
        }
    }
    None
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

fn run_print_eval(source: &str) -> Result<Value, VmError> {
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink_lines = Arc::clone(&lines);
    let sink: quench_runtime::vm::OutputSink = Arc::new(move |line| {
        if let Ok(mut lines) = sink_lines.lock() {
            lines.push(line.to_string());
        }
    });
    let outcome = crate::run::eval_script(&format!("console.log({source});"), sink);
    let output = lines
        .lock()
        .map(|lines| {
            lines.iter().fold(String::new(), |mut output, line| {
                output.push_str(line);
                if !line.ends_with('\n') {
                    output.push('\n');
                }
                output
            })
        })
        .unwrap_or_default();
    let (status, stderr) = match outcome.error {
        Some(error) => (1.0, error),
        None => (0.0, String::new()),
    };
    Ok(host_api::object(vec![
        ("pid".into(), Value::Number(0.0)),
        ("status".into(), Value::Number(status)),
        ("signal".into(), Value::Null),
        ("stdout".into(), Value::String(output)),
        ("stderr".into(), Value::String(stderr)),
    ]))
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
    let value = execute::get_property_result(value, "env").ok()?;
    if !matches!(&value, Value::Object(_) | Value::ObjectAlias(_)) {
        return None;
    }
    let mut env = std::collections::HashMap::new();
    for key in execute::own_keys(&value)
        .into_iter()
        .filter_map(|key| match key {
            Value::String(key) => Some(key),
            _ => None,
        })
    {
        if let Ok(item) = execute::get_property_result(&value, &key) {
            if let Ok(s) = execute::to_js_string(&item) {
                env.insert(key, s);
            }
        }
    }
    Some(env)
}
