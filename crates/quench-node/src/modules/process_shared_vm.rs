//! Shared-VM bindings for the existing process state, extended as APIs migrate.

use crate::host::node_host::NodeHost;
use crate::modules::process_state::{self, ProcessFact, UnhandledRejectionMode};
use quench_runtime::{NativeContext, PromiseRejectionEvent, RootId, RootedError, Value};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

struct CallbackFailure {
    exception: Option<RootId>,
    message: String,
}

const STDOUT_FD: i32 = 1;
const STDERR_FD: i32 = 2;
static HRTIME_ORIGIN: OnceLock<Instant> = OnceLock::new();
const MAX_SAFE_EXIT_CODE: f64 = 9_007_199_254_740_991.0;
const HOST_WARNING_STACK_OPTION: &str = "\0quench:process-warning-stack";
const HOST_WARNING_ID_OPTION: &str = "\0quench:process-warning-id";
const UNHANDLED_REJECTION_CLI_GUIDANCE: &str = "To terminate the node process on unhandled promise rejection, use the CLI flag `--unhandled-rejections=strict` (see https://nodejs.org/api/cli.html#cli_unhandled_rejections_mode).";

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .reset_shared();
    let process = context.object_rooted()?;
    let function = context.host_function(crate::host::shared_vm::operation("uptime"))?;
    install(context, process, "uptime", function)?;
    install_exit_code(context, process)?;
    let exit = context.host_function(crate::host::shared_vm::operation("processExit"))?;
    install(context, process, "exit", exit)?;
    let stdout = create_stream(context, STDOUT_FD)?;
    install(context, process, "stdout", stdout)?;
    let stderr = create_stream(context, STDERR_FD)?;
    install(context, process, "stderr", stderr)?;
    let event_count = context.number(0.0);
    install(context, process, "_eventsCount", event_count)?;
    for (name, operation) in [
        ("nextTick", "nextTick"),
        ("cwd", "processCwd"),
        ("chdir", "processChdir"),
        ("getuid", "processGetuid"),
        ("geteuid", "processGeteuid"),
        ("getgid", "processGetgid"),
        ("getegid", "processGetegid"),
        ("umask", "processUmask"),
        ("emitWarning", "processEmitWarning"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        install(context, process, name, function)?;
    }
    #[cfg(unix)]
    for (name, operation) in [
        ("setuid", "processSetuid"),
        ("seteuid", "processSeteuid"),
        ("setgid", "processSetgid"),
        ("setegid", "processSetegid"),
        ("setgroups", "processSetgroups"),
        ("initgroups", "processInitgroups"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        install(context, process, name, function)?;
    }
    let kill = context.host_function(crate::host::shared_vm::operation("processKillNative"))?;
    install(context, process, "_kill", kill)?;
    let raw_debug = context.host_function(crate::host::shared_vm::operation("processRawDebug"))?;
    install(context, process, "_rawDebug", raw_debug)?;
    let hrtime_raw = context.host_function(crate::host::shared_vm::operation("processHrtimeNow"))?;
    let hrtime_factory = context.evaluate_script_rooted(
        "(raw) => { const hrtime = (previous) => { const [seconds, nanoseconds] = raw(); if (previous === undefined) return [seconds, nanoseconds]; if (!Array.isArray(previous)) { const received = previous === null ? 'null' : typeof previous === 'number' ? 'type number (' + previous + ')' : typeof previous; const error = new TypeError('The \\\"time\\\" argument must be an instance of Array. Received ' + received); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } if (previous.length !== 2) { const error = new RangeError('The value of \\\"time\\\" is out of range. It must be 2. Received ' + previous.length); error.code = 'ERR_OUT_OF_RANGE'; throw error; } let sec = seconds - previous[0]; let nsec = nanoseconds - previous[1]; if (nsec < 0) { sec -= 1; nsec += 1000000000; } return [sec, nsec]; }; hrtime.bigint = () => { const [seconds, nanoseconds] = raw(); return BigInt(seconds) * 1000000000n + BigInt(nanoseconds); }; return hrtime; }",
        "node:process/shared-hrtime.js",
    )?;
    let undefined = context.undefined();
    let hrtime = context.call_rooted(hrtime_factory, undefined, &[hrtime_raw])?;
    install(context, process, "hrtime", hrtime)?;
    let on = context.host_function(crate::host::shared_vm::operation("on"))?;
    install(context, process, "on", on)?;
    install(context, process, "addListener", on)?;
    let remove_listener =
        context.host_function(crate::host::shared_vm::operation("processRemoveListener"))?;
    install(context, process, "removeListener", remove_listener)?;
    install(context, process, "off", remove_listener)?;
    let remove_all_listeners = context
        .host_function(crate::host::shared_vm::operation("processRemoveAllListeners"))?;
    install(context, process, "removeAllListeners", remove_all_listeners)?;
    let once = context.host_function(crate::host::shared_vm::operation("processOnce"))?;
    install(context, process, "once", once)?;
    let emit = context.host_function(crate::host::shared_vm::operation("processEmit"))?;
    install(context, process, "emit", emit)?;
    let exiting = context.boolean(false);
    install(context, process, "_exiting", exiting)?;
    let shared_state = context.host_mut().shared_state();
    let argv = shared_state.borrow().process_argv.clone();
    let exec_argv = shared_state.borrow().exec_argv.clone();
    let title = exec_argv
        .iter()
        .find_map(|argument| argument.strip_prefix("--title="));
    if let Some(title) = title {
        let global = context.global_root()?;
        let key = context.string_rooted("__quench_cli_title");
        let value = context.string_rooted(title);
        if !context.set_property_rooted(global, key, value, global)? {
            return Err(RootedError::host("cannot install CLI process title"));
        }
        set_text(context, process, "title", title)?;
    }
    let env = context.object_rooted()?;
    for (name, value) in std::env::vars() {
        let key = context.string_rooted(&name);
        let value = context.string_rooted(&value);
        if !context.set_property_rooted(env, key, value, env)? {
            return Err(RootedError::host("cannot install process.env entry"));
        }
    }
    install(context, process, "env", env)?;
    let load_env_file = context.host_function(crate::host::shared_vm::operation("processLoadEnvFile"))?;
    install(context, process, "loadEnvFile", load_env_file)?;
    for (name, values) in [
        ("argv", argv.as_slice()),
        ("execArgv", exec_argv.as_slice()),
    ] {
        let values = values
            .iter()
            .map(|value| context.string_rooted(value))
            .collect::<Vec<_>>();
        let values = context.array_rooted(&values)?;
        install(context, process, name, values)?;
    }
    set_text(context, process, "arch", process_state::architecture())?;
    set_text(context, process, "platform", &process_state::platform())?;
    let executable = std::env::current_exe()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let node_executable = executable
        .parent()
        .map(|parent| parent.join(if cfg!(windows) { "quench-node.exe" } else { "quench-node" }))
        .filter(|path| path.is_file())
        .unwrap_or(executable);
    set_text(context, process, "execPath", &node_executable.to_string_lossy())?;
    let pid = context.number(std::process::id() as f64);
    install(context, process, "pid", pid)?;
    #[cfg(unix)]
    let ppid = unsafe { libc::getppid() };
    #[cfg(not(unix))]
    let ppid = 0;
    let ppid = context.number(ppid as f64);
    install(context, process, "ppid", ppid)?;
    let version = format!("v{}", process_state::NODE_VERSION);
    set_text(context, process, "version", &version)?;
    let global = context.global_root()?;
    let symbol_key = context.string_rooted("Symbol");
    let symbol = context.get_property_rooted(global, symbol_key)?;
    let tag_key = context.string_rooted("toStringTag");
    let tag = context.get_property_rooted(symbol, tag_key)?;
    let process_tag = context.string_rooted("process");
    if !context.set_property_rooted(process, tag, process_tag, process)? {
        return Err(RootedError::host("cannot set process toStringTag"));
    }
    install_config(context, process)?;
    install_facts(context, process, "features", process_state::feature_facts())?;
    install_versions(context, process)?;
    install(context, global, "global", global)?;
    define_global_process(context, global, process)?;
    let retained = context.retain(process)?;
    let previous = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .process_module
        .replace(retained);
    if let Some(previous) = previous {
        context.release_root(previous);
    }
    Ok(())
}

fn install_exit_code(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
) -> Result<(), RootedError> {
    let getter = context.host_function(crate::host::shared_vm::operation("processExitCodeGet"))?;
    let setter = context.host_function(crate::host::shared_vm::operation("processExitCodeSet"))?;
    let descriptor = context.object_rooted()?;
    install(context, descriptor, "get", getter)?;
    install(context, descriptor, "set", setter)?;
    let enumerable = context.boolean(true);
    install(context, descriptor, "enumerable", enumerable)?;
    let configurable = context.boolean(false);
    install(context, descriptor, "configurable", configurable)?;

    let global = context.global_root()?;
    let object_key = context.string_rooted("Object");
    let object = context.get_property_rooted(global, object_key)?;
    let define_key = context.string_rooted("defineProperty");
    let define = context.get_property_rooted(object, define_key)?;
    let property = context.string_rooted("exitCode");
    context.call_rooted(define, object, &[process, property, descriptor])?;
    Ok(())
}

fn create_stream(
    context: &mut NativeContext<'_, NodeHost>,
    fd: i32,
) -> Result<RootId, RootedError> {
    let stream = context.object_rooted()?;
    let writable = context.boolean(true);
    install(context, stream, "writable", writable)?;
    let tty = context.boolean(crate::modules::tty_state::is_terminal_fd(fd));
    install(context, stream, "isTTY", tty)?;
    let fd_value = context.number(fd as f64);
    install(context, stream, "fd", fd_value)?;
    let data = context.number(fd as f64);
    let write = context.host_function_with_data(
        crate::host::shared_vm::operation("processStreamWrite"),
        data,
    )?;
    install(context, stream, "write", write)?;
    Ok(stream)
}

fn define_global_process(
    context: &mut NativeContext<'_, NodeHost>,
    global: RootId,
    process: RootId,
) -> Result<(), RootedError> {
    let object_key = context.string_rooted("Object");
    let object = context.get_property_rooted(global, object_key)?;
    let define_key = context.string_rooted("defineProperty");
    let define = context.get_property_rooted(object, define_key)?;
    let name = context.string_rooted("process");
    let descriptor = context.object_rooted()?;
    let writable = context.boolean(true);
    let enumerable = context.boolean(false);
    let configurable = context.boolean(true);
    for (key, value) in [
        ("value", process),
        ("writable", writable),
        ("enumerable", enumerable),
        ("configurable", configurable),
    ] {
        let key = context.string_rooted(key);
        if !context.set_property_rooted(descriptor, key, value, descriptor)? {
            return Err(RootedError::host("cannot define global process descriptor"));
        }
    }
    context.call_rooted(define, object, &[global, name, descriptor])?;
    Ok(())
}

fn install_config(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
) -> Result<(), RootedError> {
    let config = context.object_rooted()?;
    let variables = context.object_rooted()?;
    for (name, fact) in process_state::config_variable_facts() {
        let value = fact_value(context, *fact)?;
        install(context, variables, name, value)?;
    }
    install(context, config, "variables", variables)?;
    let freeze = context.evaluate_script_rooted(
        "(value) => Object.freeze(value)",
        "node:process/freeze-config.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(freeze, undefined, &[variables])?;
    context.call_rooted(freeze, undefined, &[config])?;
    install(context, process, "config", config)
}

fn install_facts(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    property: &str,
    facts: &[(&'static str, ProcessFact)],
) -> Result<(), RootedError> {
    let object = context.object_rooted()?;
    for (name, fact) in facts {
        let value = fact_value(context, *fact)?;
        install(context, object, name, value)?;
    }
    install(context, process, property, object)
}

fn fact_value(
    context: &mut NativeContext<'_, NodeHost>,
    fact: ProcessFact,
) -> Result<RootId, RootedError> {
    match fact {
        ProcessFact::Boolean(value) => Ok(context.boolean(value)),
        ProcessFact::Number(value) => Ok(context.number(value)),
        ProcessFact::String(value) => Ok(context.string_rooted(value)),
        ProcessFact::StringArray(values) => {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)
        }
    }
}

fn install_versions(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
) -> Result<(), RootedError> {
    let versions = context.object_rooted()?;
    for (name, version) in process_state::version_facts() {
        let value = context.string_rooted(version);
        install(context, versions, name, value)?;
    }
    install(context, process, "versions", versions)
}

fn set_text(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    property: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    install(context, object, property, value)
}

fn install(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install process binding {name}"
        )))
    }
}

pub(crate) fn uptime(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let seconds = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .uptime();
    Ok(context.number(seconds))
}

pub(crate) fn exit_code_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let code = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .exit_code();
    let value = match code {
        Some(code) => context.number(code as f64),
        None => context.undefined(),
    };
    Ok(value)
}

pub(crate) fn exit_code_set(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(value_root) = args.first().copied() else {
        context
            .host_mut()
            .shared_state()
            .borrow()
            .process_control
            .set_exit_code(None);
        return Ok(context.undefined());
    };
    let value = context
        .rooted_value(value_root)
        .ok_or_else(|| RootedError::host("invalid process.exitCode value root"))?;
    if value.is_undefined() || value.is_null() {
        context
            .host_mut()
            .shared_state()
            .borrow()
            .process_control
            .set_exit_code(None);
        return Ok(context.undefined());
    }

    let number = if let Some(number) = value.as_number() {
        Some(number)
    } else if let Some(string) = context.string_text(value_root)? {
        if string.is_empty() {
            None
        } else {
            let number = parse_exit_code_string(&string);
            (!number.is_nan()).then_some(number)
        }
    } else {
        None
    };
    let Some(number) = number else {
        return invalid_exit_code_type(context, value_root);
    };
    if !number.is_finite() || number.fract() != 0.0 || number.abs() > MAX_SAFE_EXIT_CODE {
        return exit_code_range_error(context, number);
    }
    context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .set_exit_code(Some(number as i64 as i32));
    Ok(context.undefined())
}

pub(crate) fn exit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let process_control = context.host_mut().shared_state().borrow().process_control.clone();
    let code = match args.first().copied() {
        None => process_control.exit_code().unwrap_or(0),
        Some(value_root) => {
            let value = context
                .rooted_value(value_root)
                .ok_or_else(|| RootedError::host("invalid process.exit code value root"))?;
            let number = if let Some(number) = value.as_number() {
                Some(number)
            } else if let Some(string) = context.string_text(value_root)? {
                let number = parse_exit_code_string(&string);
                (!number.is_nan()).then_some(number)
            } else {
                None
            };
            let Some(number) = number else {
                return invalid_exit_code_type(context, value_root);
            };
            if !number.is_finite() || number.fract() != 0.0 || number.abs() > MAX_SAFE_EXIT_CODE {
                return exit_code_range_error(context, number);
            }
            number as i64 as i32
        }
    };
    let emit_abort = !process_control.exit_emitting();
    process_control.request_exit(code);
    if emit_abort {
        Err(RootedError::host("process.exit"))
    } else {
        Ok(context.undefined())
    }
}

fn parse_exit_code_string(value: &str) -> f64 {
    let value = value.trim();
    if value.is_empty() {
        return 0.0;
    }
    match value {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ => value.parse().unwrap_or(f64::NAN),
    }
}

fn invalid_exit_code_type(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    let received = context.evaluate_script_rooted(
        "(value) => value === null ? 'null' : value === undefined ? 'undefined' : typeof value === 'string' ? `type string ('${value}')` : typeof value === 'number' ? `type number (${String(value)})` : typeof value === 'boolean' ? `type boolean (${value})` : typeof value === 'bigint' ? `type bigint (${String(value)}n)` : Array.isArray(value) ? 'an instance of Array' : 'an instance of Object'",
        "node:process/exit-code-error.js",
    )?;
    let undefined = context.undefined();
    let received_value = context.call_rooted(received, undefined, &[value]);
    context.release_root(received);
    let received_value = received_value?;
    let received = context
        .string_text(received_value)?
        .unwrap_or_else(|| "an unknown value".to_owned());
    context.release_root(received_value);
    let error = context.type_error_rooted(&format!(
        "The \"code\" argument must be of type number. Received {received}"
    ))?;
    Err(throw_with_code(context, error, "ERR_INVALID_ARG_TYPE"))
}

fn exit_code_range_error(
    context: &mut NativeContext<'_, NodeHost>,
    number: f64,
) -> Result<RootId, RootedError> {
    let received = if number.is_nan() {
        "NaN".to_owned()
    } else if number == f64::INFINITY {
        "Infinity".to_owned()
    } else if number == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        number.to_string()
    };
    let message = format!("The value of \"code\" is out of range. It must be a safe integer. Received {received}");
    let error = context.range_error_rooted(&message)?;
    Err(throw_with_code(context, error, "ERR_OUT_OF_RANGE"))
}

fn throw_with_code(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &str,
) -> RootedError {
    let key = context.string_rooted("code");
    let value = context.string_rooted(code);
    let _ = context.set_property_rooted(error, key, value, error);
    context.throw(error)
}

pub(crate) fn stream_write(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd_root = context.host_function_data()?;
    let fd = context
        .rooted_value(fd_root)
        .and_then(|value| value.as_number())
        .filter(|fd| *fd == STDOUT_FD as f64 || *fd == STDERR_FD as f64)
        .ok_or_else(|| RootedError::host("invalid process stream descriptor"))? as i32;
    let Some(chunk) = args.first().copied() else {
        return invalid_stream_chunk(context);
    };
    let text = context.to_string(chunk)?;
    if fd == STDERR_FD {
        std::io::stderr()
            .lock()
            .write_all(text.as_bytes())
            .map_err(|error| RootedError::host(error.to_string()))?;
    } else {
        let output = context.host_mut().shared_state().borrow().output.clone();
        if let Some(output) = output {
            output(&text);
        } else {
            std::io::stdout()
                .lock()
                .write_all(text.as_bytes())
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    Ok(context.boolean(true))
}

fn invalid_stream_chunk(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(
        "The \"chunk\" argument must be of type string or an instance of Buffer",
    )?;
    Err(throw_with_code(context, error, "ERR_INVALID_ARG_TYPE"))
}

pub(crate) fn cwd(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let cwd = context
        .host_mut()
        .shared_state()
        .borrow()
        .cwd
        .path()
        .to_string_lossy()
        .into_owned();
    Ok(context.string_rooted(&cwd))
}

pub(crate) fn getuid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(unix)]
    let id = unsafe { libc::getuid() };
    #[cfg(not(unix))]
    let id = 0u32;
    Ok(context.number(id as f64))
}

pub(crate) fn geteuid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(unix)]
    let id = unsafe { libc::geteuid() };
    #[cfg(not(unix))]
    let id = 0u32;
    Ok(context.number(id as f64))
}

pub(crate) fn getgid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(unix)]
    let id = unsafe { libc::getgid() };
    #[cfg(not(unix))]
    let id = 0u32;
    Ok(context.number(id as f64))
}

pub(crate) fn getegid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(unix)]
    let id = unsafe { libc::getegid() };
    #[cfg(not(unix))]
    let id = 0u32;
    Ok(context.number(id as f64))
}

#[derive(Clone, Copy)]
enum CredentialKind {
    Uid,
    Euid,
    Gid,
    Egid,
}

pub(crate) fn setuid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    set_credential(context, args.first().copied(), CredentialKind::Uid)
}

pub(crate) fn seteuid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    set_credential(context, args.first().copied(), CredentialKind::Euid)
}

pub(crate) fn setgid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    set_credential(context, args.first().copied(), CredentialKind::Gid)
}

pub(crate) fn setegid(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    set_credential(context, args.first().copied(), CredentialKind::Egid)
}

pub(crate) fn setgroups(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(groups) = args.first().copied() else {
        return Err(groups_type_error(context, None)?);
    };
    if !is_array(context, groups)? {
        return Err(groups_type_error(context, Some(groups))?);
    }
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(groups, length_key)?;
    let length = context
        .rooted_value(length)
        .and_then(Value::as_number)
        .unwrap_or_default() as usize;
    let mut group_ids = Vec::with_capacity(length);
    for index in 0..length {
        let key = context.string_rooted(&index.to_string());
        let value = context.get_property_rooted(groups, key)?;
        group_ids.push(parse_group_id(context, value, Some(index))?);
    }
    apply_groups(context, &group_ids)
}

pub(crate) fn initgroups(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(user) = args.first().copied() else {
        return Err(user_argument_type_error(context, None)?);
    };
    let user_value = context
        .rooted_value(user)
        .ok_or_else(|| RootedError::host("invalid process initgroups user root"))?;
    if user_value.as_number().is_none() && context.string_text(user)?.is_none() {
        return Err(user_argument_type_error(context, Some(user))?);
    }
    let Some(extra_group) = args.get(1).copied() else {
        return Err(extra_group_type_error(context, None)?);
    };
    let group_id = parse_group_id(context, extra_group, None)?;
    let user_name = credential_user_name(context, user)?;
    let result = initgroups_syscall(&user_name, group_id);
    if result == 0 {
        return Ok(context.undefined());
    }
    Err(throw_os_process_error(context, "initgroups")?)
}

fn is_array(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let array_key = context.string_rooted("Array");
    let array = context.get_property_rooted(global, array_key)?;
    let is_array_key = context.string_rooted("isArray");
    let is_array = context.get_property_rooted(array, is_array_key)?;
    let result = context.call_rooted(is_array, array, &[value])?;
    Ok(context
        .rooted_value(result)
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

fn parse_group_id(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    index: Option<usize>,
) -> Result<u32, RootedError> {
    let rooted = context
        .rooted_value(value)
        .ok_or_else(|| RootedError::host("invalid process group argument"))?;
    let label = index.map_or_else(|| "extraGroup".to_owned(), |i| format!("groups[{i}]"));
    if let Some(number) = rooted.as_number() {
        if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > u32::MAX as f64
        {
            let error = context.range_error_rooted(&format!(
                "The value of \"{label}\" is out of range. It must be >= 0 and <= {}. Received {number}",
                u32::MAX
            ))?;
            return Err(throw_with_code(context, error, "ERR_OUT_OF_RANGE"));
        }
        return Ok(number as u32);
    }
    if let Some(name) = context.string_text(value)? {
        return match credential_id_by_name(&name, CredentialKind::Gid) {
            Some(id) => Ok(id),
            None => Err(unknown_credential(context, "Group", &name)?),
        };
    }
    Err(group_argument_type_error(context, value, index)?)
}

fn credential_user_name(
    context: &mut NativeContext<'_, NodeHost>,
    user: RootId,
) -> Result<std::ffi::CString, RootedError> {
    let rooted = context
        .rooted_value(user)
        .ok_or_else(|| RootedError::host("invalid process initgroups user root"))?;
    let name = if let Some(name) = context.string_text(user)? {
        let c_name = std::ffi::CString::new(name.as_str()).ok();
        let exists = c_name.as_ref().is_some_and(credential_name_exists);
        if exists {
            c_name
        } else {
            return Err(unknown_credential(context, "User", &name)?);
        }
    } else if let Some(number) = rooted.as_number() {
        if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > u32::MAX as f64
        {
            let error = context.range_error_rooted(&format!(
                "The value of \"user\" is out of range. It must be >= 0 and <= {}. Received {number}",
                u32::MAX
            ))?;
            return Err(throw_with_code(context, error, "ERR_OUT_OF_RANGE"));
        }
        let Some(name) = credential_name_by_uid(number as u32) else {
            return Err(unknown_credential(context, "User", &number.to_string())?);
        };
        Some(name)
    } else {
        return Err(credential_type_error(context, Some(user))?);
    };
    name.ok_or_else(|| RootedError::host("invalid process initgroups user name"))
}

fn apply_groups(
    context: &mut NativeContext<'_, NodeHost>,
    groups: &[u32],
) -> Result<RootId, RootedError> {
    let result = setgroups_syscall(groups);
    if result == 0 {
        return Ok(context.undefined());
    }
    Err(throw_os_process_error(context, "setgroups")?)
}

#[cfg(unix)]
fn initgroups_syscall(user: &std::ffi::CString, group: u32) -> i32 {
    unsafe { libc::initgroups(user.as_ptr(), group as libc::gid_t) }
}

#[cfg(not(unix))]
fn initgroups_syscall(_: &std::ffi::CString, _: u32) -> i32 {
    libc::ENOSYS
}

#[cfg(unix)]
fn setgroups_syscall(groups: &[u32]) -> i32 {
    let groups = groups
        .iter()
        .copied()
        .map(|group| group as libc::gid_t)
        .collect::<Vec<_>>();
    unsafe { libc::setgroups(groups.len(), groups.as_ptr()) }
}

#[cfg(not(unix))]
fn setgroups_syscall(_: &[u32]) -> i32 {
    libc::ENOSYS
}

#[cfg(unix)]
fn credential_name_exists(name: &std::ffi::CString) -> bool {
    unsafe { !libc::getpwnam(name.as_ptr()).is_null() }
}

#[cfg(not(unix))]
fn credential_name_exists(_: &std::ffi::CString) -> bool {
    false
}

#[cfg(unix)]
fn credential_name_by_uid(uid: u32) -> Option<std::ffi::CString> {
    let entry = unsafe { libc::getpwuid(uid as libc::uid_t) };
    (!entry.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr((*entry).pw_name) }.to_owned())
}

#[cfg(not(unix))]
fn credential_name_by_uid(_: u32) -> Option<std::ffi::CString> {
    None
}

fn throw_os_process_error(
    context: &mut NativeContext<'_, NodeHost>,
    syscall: &str,
) -> Result<RootedError, RootedError> {
    let errno = std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EINVAL);
    let code = errno_name(errno);
    let message = unsafe {
        std::ffi::CStr::from_ptr(libc::strerror(errno))
            .to_string_lossy()
            .into_owned()
    };
    let error = context.error_rooted(&format!("{code}, {message}"))?;
    set_text(context, error, "code", code)?;
    set_text(context, error, "syscall", syscall)?;
    Ok(context.throw(error))
}

fn groups_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> Result<RootedError, RootedError> {
    let received = value.map_or_else(
        || Ok("undefined".to_owned()),
        |value| received_type(context, value),
    )?;
    throw_process_error(
        context,
        &format!("The \"groups\" argument must be an instance of Array. Received {received}"),
        "ERR_INVALID_ARG_TYPE",
        true,
    )
}

fn user_argument_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> Result<RootedError, RootedError> {
    let received = value.map_or_else(
        || Ok("undefined".to_owned()),
        |value| received_type(context, value),
    )?;
    throw_process_error(
        context,
        &format!("The \"user\" argument must be one of type number or string. Received {received}"),
        "ERR_INVALID_ARG_TYPE",
        true,
    )
}

fn extra_group_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> Result<RootedError, RootedError> {
    let received = value.map_or_else(
        || Ok("undefined".to_owned()),
        |value| received_type(context, value),
    )?;
    throw_process_error(
        context,
        &format!("The \"extraGroup\" argument must be one of type number or string. Received {received}"),
        "ERR_INVALID_ARG_TYPE",
        true,
    )
}

fn group_argument_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    index: Option<usize>,
) -> Result<RootedError, RootedError> {
    let received = received_type(context, value)?;
    let label = index.map_or_else(|| "extraGroup".to_owned(), |i| format!("groups[{i}]"));
    throw_process_error(
        context,
        &format!("The \"{label}\" argument must be one of type number or string. Received {received}"),
        "ERR_INVALID_ARG_TYPE",
        true,
    )
}

fn unknown_credential(
    context: &mut NativeContext<'_, NodeHost>,
    kind: &str,
    name: &str,
) -> Result<RootedError, RootedError> {
    throw_process_error(
        context,
        &format!("{kind} identifier does not exist: {name}"),
        "ERR_UNKNOWN_CREDENTIAL",
        false,
    )
}

fn set_credential(
    context: &mut NativeContext<'_, NodeHost>,
    value: Option<RootId>,
    kind: CredentialKind,
) -> Result<RootId, RootedError> {
    let Some(value) = value else {
        return Err(credential_type_error(context, None)?);
    };
    let id = parse_credential_id(context, value, kind)?;
    apply_credential(context, id, kind)
}

fn parse_credential_id(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    kind: CredentialKind,
) -> Result<u32, RootedError> {
    let rooted = context
        .rooted_value(value)
        .ok_or_else(|| RootedError::host("invalid process credential argument"))?;
    if let Some(number) = rooted.as_number() {
        if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > u32::MAX as f64
        {
            let error = context.range_error_rooted(&format!(
                "The value of \"id\" is out of range. It must be >= 0 and <= {}. Received {number}",
                u32::MAX
            ))?;
            return Err(throw_with_code(context, error, "ERR_OUT_OF_RANGE"));
        }
        return Ok(number as u32);
    } else if let Some(name) = context.string_text(value)? {
        let Some(id) = credential_id_by_name(&name, kind) else {
            let noun = match kind {
                CredentialKind::Uid | CredentialKind::Euid => "User",
                CredentialKind::Gid | CredentialKind::Egid => "Group",
            };
            return Err(throw_process_error(
                context,
                &format!("{noun} identifier does not exist: {name}"),
                "ERR_UNKNOWN_CREDENTIAL",
                false,
            )?);
        };
        return Ok(id);
    } else {
        return Err(credential_type_error(context, Some(value))?);
    }
}

fn apply_credential(
    context: &mut NativeContext<'_, NodeHost>,
    id: u32,
    kind: CredentialKind,
) -> Result<RootId, RootedError> {
    let errno = credential_syscall(id, kind);
    if errno == 0 {
        return Ok(context.undefined());
    }
    let code = errno_name(errno);
    let message = unsafe {
        std::ffi::CStr::from_ptr(libc::strerror(errno))
            .to_string_lossy()
            .into_owned()
    };
    Err(throw_process_error(
        context,
        &format!("{code}, {message}"),
        code,
        false,
    )?)
}

fn credential_syscall(id: u32, kind: CredentialKind) -> i32 {
    #[cfg(unix)]
    {
        let result = unsafe {
            match kind {
                CredentialKind::Uid => libc::setuid(id),
                CredentialKind::Euid => libc::seteuid(id),
                CredentialKind::Gid => libc::setgid(id),
                CredentialKind::Egid => libc::setegid(id),
            }
        };
        if result == 0 {
            0
        } else {
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EINVAL)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (id, kind);
        libc::ENOSYS
    }
}

fn credential_id_by_name(name: &str, kind: CredentialKind) -> Option<u32> {
    #[cfg(unix)]
    {
        let name = std::ffi::CString::new(name).ok()?;
        return unsafe {
            match kind {
                CredentialKind::Uid | CredentialKind::Euid => {
                    libc::getpwnam(name.as_ptr()).as_ref().map(|entry| entry.pw_uid)
                }
                CredentialKind::Gid | CredentialKind::Egid => {
                    libc::getgrnam(name.as_ptr()).as_ref().map(|entry| entry.gr_gid)
                }
            }
        };
    }
    #[cfg(not(unix))]
    {
        let _ = (name, kind);
        None
    }
}

fn credential_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> Result<RootedError, RootedError> {
    let received = match value {
        None => "undefined".to_owned(),
        Some(value) => received_type(context, value)?,
    };
    throw_process_error(
        context,
        &format!(
            "The \"id\" argument must be one of type number or string. Received {received}"
        ),
        "ERR_INVALID_ARG_TYPE",
        true,
    )
}

fn throw_process_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
    code: &str,
    type_error: bool,
) -> Result<RootedError, RootedError> {
    let error = if type_error {
        context.type_error_rooted(message)?
    } else {
        context.error_rooted(message)?
    };
    let code = context.string_rooted(code);
    install(context, error, "code", code)?;
    Ok(context.throw(error))
}

fn errno_name(errno: i32) -> &'static str {
    match errno {
        libc::EPERM => "EPERM",
        libc::EACCES => "EACCES",
        libc::EINVAL => "EINVAL",
        libc::ENOSYS => "ENOSYS",
        _ => "UNKNOWN",
    }
}

pub(crate) fn hrtime_now(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let elapsed = HRTIME_ORIGIN.get_or_init(Instant::now).elapsed();
    let seconds = context.number(elapsed.as_secs() as f64);
    let nanoseconds = context.number(elapsed.subsec_nanos() as f64);
    context.array_rooted(&[seconds, nanoseconds])
}

pub(crate) fn chdir(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(directory) = args.first().copied() else {
        return Err(chdir_type_error(context)?);
    };
    let Some(directory) = context.string_text(directory)? else {
        return Err(chdir_type_error(context)?);
    };
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
    match process_state::change_directory_cwd(&cwd, &directory) {
        Ok(()) => Ok(context.undefined()),
        Err(error) => {
            let exception = context.error_rooted(&error.message)?;
            set_text(context, exception, "code", error.code)?;
            set_text(context, exception, "syscall", "chdir")?;
            set_text(context, exception, "path", &error.path)?;
            set_text(context, exception, "dest", &error.destination)?;
            let errno = context.number(error.errno as f64);
            install(context, exception, "errno", errno)?;
            Err(context.throw(exception))
        }
    }
}

fn chdir_type_error(context: &mut NativeContext<'_, NodeHost>) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted("The \"directory\" argument must be of type string")?;
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    install(context, error, "code", code)?;
    Ok(context.throw(error))
}

pub(crate) fn umask(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(mask) = args.first().copied() else {
        let current = context
            .host_mut()
            .shared_state()
            .borrow()
            .process_control
            .umask();
        return Ok(context.number(current as f64));
    };
    let value = context
        .rooted_value(mask)
        .ok_or_else(|| RootedError::host("invalid process.umask argument root"))?;
    let parsed = if let Some(number) = value.as_number() {
        (number.is_finite() && number >= 0.0 && number.fract() == 0.0).then_some(number as u32)
    } else if let Some(text) = context.string_text(mask)? {
        match u32::from_str_radix(&text, 8) {
            Ok(parsed) => Some(parsed),
            Err(_) => {
                let error = context.type_error_rooted(&format!(
                    "The \"mask\" argument is invalid. Received {text}"
                ))?;
                return Err(throw_with_code(context, error, "ERR_INVALID_ARG_VALUE"));
            }
        }
    } else {
        None
    };
    let Some(parsed) = parsed else {
        let error =
            context.type_error_rooted("The \"mask\" argument must be of type number or string")?;
        let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
        let property = context.string_rooted("code");
        if !context.set_property_rooted(error, property, code, error)? {
            return Err(RootedError::host("cannot set process.umask error code"));
        }
        return Err(context.throw(error));
    };
    let previous = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .update_umask(parsed);
    Ok(context.number(previous as f64))
}

pub(crate) fn kill_native(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let pid = integer_argument(context, args.first().copied(), "pid")?;
    let signal = integer_argument(context, args.get(1).copied(), "signal")?;
    #[cfg(unix)]
    let error = if unsafe { libc::kill(pid, signal) } == 0 {
        0
    } else {
        std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EINVAL)
    };
    #[cfg(not(unix))]
    let error = libc::ENOSYS;
    Ok(context.number(error as f64))
}

pub(crate) fn raw_debug(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let message = format_raw_debug(context, args)?;
    let mut stderr = std::io::stderr().lock();
    stderr
        .write_all(message.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
        .map_err(|error| RootedError::host(error.to_string()))?;
    Ok(context.undefined())
}

pub(crate) fn load_env_file(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path_text = match args.first().copied() {
        None => ".env".to_owned(),
        Some(path) => match context.string_text(path)? {
            Some(path) => path,
            None => {
                return Err(throw_process_error(
                    context,
                    "The \"path\" argument must be of type string",
                    "ERR_INVALID_ARG_TYPE",
                    true,
                )?);
            }
        },
    };
    let cwd = context.host_mut().shared_state().borrow().cwd.path();
    let requested = std::path::Path::new(&path_text);
    let path = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };
    let contents = std::fs::read_to_string(&path).map_err(|error| {
        dotenv_file_error(context, &path_text, error)
    })?;
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process.loadEnvFile called without process root"))?;
    let env_key = context.string_rooted("env");
    let env = context.get_property_rooted(process, env_key)?;
    for (key_text, value_text) in parse_env_file(&contents) {
        if std::env::var_os(&key_text).is_some() {
            continue;
        }
        let key = context.string_rooted(&key_text);
        let existing = context.get_property_rooted(env, key)?;
        if !context
            .rooted_value(existing)
            .is_some_and(Value::is_undefined)
        {
            continue;
        }
        let value = context.string_rooted(&value_text);
        if !context.set_property_rooted(env, key, value, env)? {
            return Err(RootedError::host("cannot set process.env variable"));
        }
        // SAFETY: this synchronous Node API mirrors Node's process.env update;
        // the environment is observed only at host process boundaries.
        unsafe { std::env::set_var(&key_text, &value_text) };
    }
    Ok(context.undefined())
}

fn parse_env_file(contents: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    let mut lines = contents.lines().peekable();
    while let Some(line) = lines.next() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
        {
            continue;
        }
        let raw_value = raw_value.trim_start();
        let value = if let Some(quote @ ('\'' | '"' | '`')) = raw_value.chars().next() {
            let mut quoted = raw_value[quote.len_utf8()..].to_owned();
            while !quoted.contains(quote) {
                let Some(next) = lines.next() else { break };
                quoted.push('\n');
                quoted.push_str(next);
            }
            let value = quoted.split_once(quote).map_or(quoted.as_str(), |(value, _)| value);
            if quote == '\'' {
                value.to_owned()
            } else {
                value.replace("\\n", "\n").replace("\\r", "\r")
            }
        } else {
            let value = raw_value
                .char_indices()
                .find(|(index, character)| {
                    *character == '#' && (*index == 0 || raw_value[..*index].chars().next_back().is_some_and(char::is_whitespace))
                })
                .map_or(raw_value, |(index, _)| &raw_value[..index]);
            value.trim_end().to_owned()
        };
        values.push((key.to_owned(), value));
    }
    values
}

fn dotenv_file_error(
    context: &mut NativeContext<'_, NodeHost>,
    path: &str,
    error: std::io::Error,
) -> RootedError {
    let details = crate::modules::fs_error_details::error_details("open", Some(path), &error);
    let error_root = match context.error_rooted(&details.message) {
        Ok(error) => error,
        Err(error) => return error,
    };
    if let Err(error) = set_text(context, error_root, "code", details.code) {
        return error;
    }
    if let Err(error) = set_text(context, error_root, "syscall", &details.syscall) {
        return error;
    }
    if let Some(path) = details.path.as_deref() {
        if let Err(error) = set_text(context, error_root, "path", path) {
            return error;
        }
    }
    let errno = context.number(details.errno as f64);
    let errno_key = context.string_rooted("errno");
    if !matches!(
        context.set_property_rooted(error_root, errno_key, errno, error_root),
        Ok(true)
    ) {
        return RootedError::host("cannot set process.loadEnvFile errno");
    }
    context.throw(error_root)
}

fn format_raw_debug(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<String, RootedError> {
    let Some((&first, rest)) = args.split_first() else {
        return Ok(String::new());
    };
    let format = context.to_string(first)?;
    let mut output = String::with_capacity(format.len());
    let mut rest = rest.iter().copied();
    let mut chars = format.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            output.push(character);
            continue;
        }
        let Some(specifier) = chars.next() else {
            output.push('%');
            break;
        };
        if specifier == '%' {
            output.push('%');
            continue;
        }
        if matches!(specifier, 's' | 'd' | 'i' | 'f' | 'j' | 'o' | 'O') {
            if let Some(value) = rest.next() {
                output.push_str(&context.to_string(value)?);
            } else {
                output.push('%');
                output.push(specifier);
            }
        } else {
            output.push('%');
            output.push(specifier);
        }
    }
    for value in rest {
        output.push(' ');
        output.push_str(&context.to_string(value)?);
    }
    Ok(output)
}

fn integer_argument(
    context: &mut NativeContext<'_, NodeHost>,
    value: Option<RootId>,
    name: &str,
) -> Result<i32, RootedError> {
    let Some(value) = value else {
        return Err(RootedError::host(format!("missing process.kill {name}")));
    };
    if let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    {
        return Ok(number as i32);
    }
    if let Some(text) = context.string_text(value)? {
        if let Ok(number) = text.parse::<i32>() {
            return Ok(number);
        }
    }
    Err(RootedError::host(format!("invalid process.kill {name}")))
}

/// Run shared-host nextTick callbacks before VM jobs, then emit process exit
/// without restarting the VM or invalidating callback roots.
pub(crate) fn finish_execution(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<(), String> {
    let process_control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    let checkpoint = drain_checkpoint(runtime, program);
    let shared_state = runtime.host_mut().shared_state();
    crate::modules::http_shared_vm::cleanup(runtime, &shared_state);
    let exit_code = match checkpoint {
        Ok(()) => process_control.exit_code().unwrap_or(0),
        Err(_) => {
            process_control.set_exit_code(Some(1));
            1
        }
    };
    let exit = emit_exit(runtime, program, exit_code);

    checkpoint.and(exit)
}

pub(crate) fn finish_after_uncaught_error(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    error: &quench_runtime::JsError,
) -> Result<bool, String> {
    let process_control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    let routed = emit_uncaught_exception(runtime, program, error);
    if matches!(&routed, Ok(true)) {
        finish_execution(runtime, program)?;
        return Ok(true);
    }
    let handler_error = routed.err();
    process_control.set_exit_code(Some(1));
    let shared_state = runtime.host_mut().shared_state();
    crate::modules::http_shared_vm::cleanup(runtime, &shared_state);
    emit_exit(runtime, program, 1)?;
    handler_error.map_or(Ok(false), Err)
}

pub(crate) fn finish_requested_exit(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<(), String> {
    let process_control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    let shared_state = runtime.host_mut().shared_state();
    crate::modules::http_shared_vm::cleanup(runtime, &shared_state);
    emit_exit(runtime, program, process_control.exit_code().unwrap_or(0))
}

fn emit_uncaught_exception(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    error: &quench_runtime::JsError,
) -> Result<bool, String> {
    let Some(value) = error.thrown_value() else {
        return Ok(false);
    };
    let exception = runtime.root(value);
    let handled = route_uncaught_exception_value(runtime, program, exception, "uncaughtException");
    runtime.release_root(exception);
    handled
}

pub(crate) fn route_uncaught_exception_value(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    exception: RootId,
    origin: &str,
) -> Result<bool, String> {
    let Some(process) = shared_process_root(runtime) else {
        return Ok(false);
    };
    let origin = runtime.string_rooted(origin);
    let event = runtime.string_rooted("uncaughtException");
    let emit_name = runtime.string_rooted("emit");
    let emit = runtime.get_property_rooted(process, emit_name);
    runtime.release_root(emit_name);
    let emit = match emit {
        Ok(emit) => emit,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            runtime.release_root(event);
            runtime.release_root(origin);
            return Err(message);
        }
    };
    let emitted = runtime.call_rooted(emit, process, &[event, exception, origin]);
    runtime.release_root(emit);
    runtime.release_root(event);
    runtime.release_root(origin);
    let emitted = match emitted {
        Ok(emitted) => emitted,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            return Err(message);
        }
    };
    let handled = runtime
        .rooted_value(emitted)
        .and_then(quench_runtime::Value::as_bool)
        .ok_or_else(|| "process.emit returned a non-boolean result".to_owned());
    runtime.release_root(emitted);
    match handled {
        Ok(handled) => Ok(handled),
        Err(message) => Err(message),
    }
}

fn drain_checkpoint(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<(), String> {
    let shared_state = runtime.host_mut().shared_state();
    loop {
        drain_shared_jobs(runtime, program, &shared_state)?;

        if crate::modules::fetch_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }
        if crate::modules::http_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }

        let can_run_timers = has_referenced_shared_work(&shared_state);
        let due = if can_run_timers {
            shared_state
                .borrow_mut()
                .scheduler
                .take_due_shared_timer(std::time::Instant::now())
        } else {
            None
        };
        if let Some(timer) = due {
            let callback_result = invoke_timer_callback(runtime, program, &timer.callback);
            if callback_result.is_err() {
                shared_state
                    .borrow_mut()
                    .scheduler
                    .cancel_shared_timer(timer.id);
            }
            let released = shared_state
                .borrow_mut()
                .scheduler
                .complete_shared_timer(timer, std::time::Instant::now());
            if let Some(callback) = released {
                release_callback(runtime, callback);
            }
            callback_result?;
            continue;
        }

        if !has_referenced_shared_work(&shared_state) {
            release_unreferenced_shared_callbacks(runtime, &shared_state);
            break;
        }
        let immediate_cutoff = shared_state.borrow().scheduler.shared_immediate_cutoff();
        let Some(cutoff) = immediate_cutoff else {
            let next_timer = shared_state.borrow().scheduler.next_shared_timer_due();
            if has_referenced_shared_work(&shared_state) {
                let poll_interval = crate::modules::fetch_shared_vm::poll_interval()
                    .min(crate::modules::net_shared_vm::poll_interval());
                let wait = next_timer
                    .map(|due| due.saturating_duration_since(std::time::Instant::now()))
                    .map_or(poll_interval, |delay| delay.min(poll_interval));
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
                continue;
            }
            release_unreferenced_shared_callbacks(runtime, &shared_state);
            break;
        };
        let mut ran_immediate = false;
        loop {
            let immediate = {
                shared_state
                    .borrow_mut()
                    .scheduler
                    .take_shared_immediate_through(cutoff)
            };
            let Some(immediate) = immediate else {
                break;
            };
            ran_immediate = true;
            if let Err(error) = settle_immediate_reference(
                runtime,
                program,
                &shared_state,
                immediate.owner,
                immediate.callback.receiver,
            ) {
                release_callback(runtime, immediate.callback);
                release_pending_shared_immediates(runtime, &shared_state);
                return Err(error);
            }
            if let Err(error) = invoke_async_callback(runtime, program, immediate.callback) {
                release_pending_shared_immediates(runtime, &shared_state);
                return Err(error);
            }
            if let Err(error) = drain_shared_jobs(runtime, program, &shared_state) {
                release_pending_shared_immediates(runtime, &shared_state);
                return Err(error);
            }
        }
        if !ran_immediate {
            let next_timer = shared_state.borrow().scheduler.next_shared_timer_due();
            if has_referenced_shared_work(&shared_state) {
                let poll_interval = crate::modules::fetch_shared_vm::poll_interval()
                    .min(crate::modules::net_shared_vm::poll_interval());
                let wait = next_timer
                    .map(|due| due.saturating_duration_since(std::time::Instant::now()))
                    .map_or(poll_interval, |delay| delay.min(poll_interval));
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
                continue;
            }
            release_unreferenced_shared_callbacks(runtime, &shared_state);
            break;
        }
    }
    Ok(())
}

fn has_referenced_shared_work(shared_state: &Rc<RefCell<crate::host::SharedNodeState>>) -> bool {
    crate::modules::fetch_shared_vm::has_pending(shared_state)
        || crate::modules::http_shared_vm::has_work(shared_state)
        || !shared_state.borrow().net_sockets.is_empty()
        || !shared_state.borrow().net_servers.is_empty()
        || shared_state.borrow().scheduler.has_refed_shared_timers()
        || shared_state
            .borrow()
            .scheduler
            .has_refed_shared_immediates()
}

fn settle_immediate_reference(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    owner: crate::modules::shared_event_loop::SharedImmediateOwner,
    handle: RootId,
) -> Result<(), String> {
    match owner {
        crate::modules::shared_event_loop::SharedImmediateOwner::Host => Ok(()),
        crate::modules::shared_event_loop::SharedImmediateOwner::Guest => {
            let refed_key = shared_state
                .borrow()
                .timer_handle_api
                .map(|api| api.refed_key)
                .ok_or_else(|| "shared Immediate handle state is unavailable".to_owned())?;
            let refed = runtime
                .get_property_rooted(handle, refed_key)
                .map_err(|error| runtime.format_error(program, &error.error))?;
            let was_refed = runtime.truthy_rooted(refed);
            runtime.release_root(refed);
            if was_refed.map_err(|error| runtime.format_error(program, &error))? {
                shared_state
                    .borrow_mut()
                    .scheduler
                    .transition_shared_immediate_ref(false);
            }
            let settled = runtime.root(Value::NULL);
            let result = runtime.set_property_rooted(handle, refed_key, settled, handle);
            runtime.release_root(settled);
            match result {
                Ok(true) => Ok(()),
                Ok(false) => Err("cannot settle Immediate reference state".to_owned()),
                Err(error) => Err(runtime.format_error(program, &error.error)),
            }
        }
    }
}

fn release_pending_shared_immediates(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
) {
    let immediates = shared_state
        .borrow_mut()
        .scheduler
        .take_all_shared_immediates();
    for immediate in immediates {
        release_callback(runtime, immediate.callback);
    }
}

fn release_unreferenced_shared_callbacks(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
) {
    let callbacks = {
        let mut host = shared_state.borrow_mut();
        let immediates = host.scheduler.discard_unreferenced_shared_immediates();
        let timers = host.scheduler.discard_unreferenced_shared_timers();
        immediates.into_iter().chain(timers).collect::<Vec<_>>()
    };
    for callback in callbacks {
        release_callback(runtime, callback);
    }
}

fn invoke_timer_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callback: &crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), String> {
    match call_shared_callback(runtime, program, callback) {
        Ok(()) => Ok(()),
        Err(failure) => route_callback_failure(runtime, program, failure),
    }
}

fn invoke_async_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callback: crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), String> {
    let result = call_shared_callback(runtime, program, &callback);
    release_callback(runtime, callback);
    match result {
        Ok(()) => Ok(()),
        Err(failure) => route_callback_failure(runtime, program, failure),
    }
}

fn call_shared_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callback: &crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), CallbackFailure> {
    match runtime.call_rooted(callback.callback, callback.receiver, &callback.args) {
        Ok(result) => {
            runtime.release_root(result);
            Ok(())
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            let exception = error
                .exception
                .or_else(|| error.error.thrown_value().map(|value| runtime.root(value)));
            Err(CallbackFailure { exception, message })
        }
    }
}

fn route_callback_failure(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    failure: CallbackFailure,
) -> Result<(), String> {
    let Some(exception) = failure.exception else {
        return Err(failure.message);
    };
    let routed = route_uncaught_exception_value(runtime, program, exception, "uncaughtException");
    runtime.release_root(exception);
    match routed {
        Ok(true) => Ok(()),
        Ok(false) => Err(failure.message),
        Err(handler_error) => Err(handler_error),
    }
}

fn drain_shared_jobs(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
) -> Result<(), String> {
    loop {
        loop {
            let callback = { shared_state.borrow_mut().scheduler.take_shared_next_tick() };
            let Some(callback) = callback else {
                break;
            };
            invoke_async_callback(runtime, program, callback)?;
        }
        run_host_jobs_with_uncaught(runtime, program)?;
        if shared_state.borrow().scheduler.has_shared_next_ticks() {
            continue;
        }
        loop {
            let handled = runtime.take_promise_rejection_handled_events();
            if handled.is_empty() {
                break;
            }
            dispatch_promise_rejection_batch(runtime, program, handled)?;
        }
        let unhandled = runtime.take_promise_rejection_unhandled_events();
        if unhandled.is_empty() {
            return Ok(());
        }
        dispatch_promise_rejection_batch(runtime, program, unhandled)?;
    }
}

pub(crate) fn run_host_jobs_with_uncaught(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<(), String> {
    loop {
        let error = match runtime.run_host_jobs(program) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        let message = runtime.format_error(program, &error);
        let Some(value) = error.thrown_value() else {
            return Err(message);
        };
        let exception = runtime.root(value);
        let routed =
            route_uncaught_exception_value(runtime, program, exception, "uncaughtException");
        runtime.release_root(exception);
        match routed {
            Ok(true) => {}
            Ok(false) => return Err(message),
            Err(handler_error) => return Err(handler_error),
        }
    }
}

fn dispatch_promise_rejection_batch(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    events: Vec<PromiseRejectionEvent>,
) -> Result<(), String> {
    let mut events = events.into_iter();
    while let Some(event) = events.next() {
        let result = dispatch_promise_rejection(runtime, program, event);
        release_promise_rejection_event(runtime, event);
        if let Err(error) = result {
            for event in events {
                release_promise_rejection_event(runtime, event);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn release_promise_rejection_event(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    event: PromiseRejectionEvent,
) {
    match event {
        PromiseRejectionEvent::Unhandled {
            promise, reason, ..
        } => {
            runtime.release_root(promise);
            runtime.release_root(reason);
        }
        PromiseRejectionEvent::Handled { promise, .. } => {
            runtime.release_root(promise);
        }
    }
}

fn dispatch_promise_rejection(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    event: PromiseRejectionEvent,
) -> Result<(), String> {
    if let PromiseRejectionEvent::Unhandled { promise, .. } = event {
        runtime
            .mark_promise_rejection_reported(promise)
            .map_err(|error| rooted_error_message(runtime, program, error))?;
    }
    match event {
        PromiseRejectionEvent::Unhandled {
            id,
            promise,
            reason,
        } => dispatch_unhandled_rejection(runtime, program, id, promise, reason),
        PromiseRejectionEvent::Handled { id, promise } => {
            let handled = emit_process_event(runtime, program, "rejectionHandled", &[promise])?;
            if !handled {
                let message =
                    format!("Promise rejection was handled asynchronously (rejection id: {id})");
                schedule_rejection_warning(
                    runtime,
                    program,
                    "PromiseRejectionHandledWarning",
                    &message,
                    None,
                    Some(id),
                )?;
            }
            Ok(())
        }
    }
}

fn dispatch_unhandled_rejection(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    id: u64,
    promise: RootId,
    reason: RootId,
) -> Result<(), String> {
    let mode = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .unhandled_rejection_mode;
    match mode {
        UnhandledRejectionMode::None => {
            emit_process_event(runtime, program, "unhandledRejection", &[reason, promise])?;
        }
        UnhandledRejectionMode::Warn => {
            emit_process_event(runtime, program, "unhandledRejection", &[reason, promise])?;
            schedule_unhandled_rejection_warnings(runtime, program, id, reason)?;
        }
        UnhandledRejectionMode::Throw => {
            let handled =
                emit_process_event(runtime, program, "unhandledRejection", &[reason, promise])?;
            if !handled {
                route_unhandled_rejection(runtime, program, reason)?;
            }
        }
        UnhandledRejectionMode::Strict => {
            let (exception, owned_exception) =
                unhandled_rejection_exception(runtime, program, reason)?;
            let handled =
                route_uncaught_exception_value(runtime, program, exception, "unhandledRejection");
            if let Some(exception) = owned_exception {
                runtime.release_root(exception);
            }
            if !handled? {
                return terminate_unhandled_rejection(runtime);
            }
            let handled =
                emit_process_event(runtime, program, "unhandledRejection", &[reason, promise])?;
            if !handled {
                schedule_unhandled_rejection_warnings(runtime, program, id, reason)?;
            }
        }
    }
    Ok(())
}

fn route_unhandled_rejection(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    reason: RootId,
) -> Result<(), String> {
    let (exception, owned_exception) = unhandled_rejection_exception(runtime, program, reason)?;
    let handled = route_uncaught_exception_value(runtime, program, exception, "unhandledRejection");
    if let Some(exception) = owned_exception {
        runtime.release_root(exception);
    }
    if !handled? {
        terminate_unhandled_rejection(runtime)?;
    }
    Ok(())
}

fn terminate_unhandled_rejection(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
) -> Result<(), String> {
    let control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    control.set_exit_code(Some(1));
    Err("unhandled Promise rejection was not handled".into())
}

fn unhandled_rejection_exception(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    reason: RootId,
) -> Result<(RootId, Option<RootId>), String> {
    let is_error = runtime
        .is_error_rooted(reason)
        .map_err(|error| rooted_error_message(runtime, program, error))?;
    if is_error
        || runtime
            .has_own_property_rooted(reason, "stack")
            .map_err(|error| rooted_error_message(runtime, program, error))?
    {
        return Ok((reason, None));
    }

    let rejected = detail_string_rooted(runtime, program, reason)?;
    let message = format!(
        "This error originated either by throwing inside of an async function without a catch block, or by rejecting a promise which was not handled with .catch(). The promise rejected with the reason \"{rejected}\"."
    );
    let mut temporary_roots = Vec::new();
    let result = (|| {
        let global = runtime
            .global_root()
            .map_err(|error| runtime.format_error(program, &error))?;
        temporary_roots.push(global);
        let constructor = get_rooted_property(runtime, program, global, "Error")?;
        temporary_roots.push(constructor);
        let message = runtime.string_rooted(&message);
        temporary_roots.push(message);
        let exception = runtime
            .construct_rooted(constructor, constructor, &[message])
            .map_err(|error| rooted_error_message(runtime, program, error))?;
        temporary_roots.push(exception);
        set_rooted_text_property(
            runtime,
            program,
            exception,
            "name",
            "UnhandledPromiseRejection",
        )?;
        set_rooted_text_property(
            runtime,
            program,
            exception,
            "code",
            "ERR_UNHANDLED_REJECTION",
        )?;
        let value = runtime
            .rooted_value(exception)
            .ok_or_else(|| "unhandled rejection Error root expired".to_owned())?;
        Ok(runtime.root(value))
    })();
    for root in temporary_roots {
        runtime.release_root(root);
    }
    result.map(|exception| (exception, Some(exception)))
}

fn detail_string_rooted(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    value: RootId,
) -> Result<String, String> {
    runtime
        .detail_string_rooted(value)
        .map_err(|error| rooted_error_message(runtime, program, error))
}

fn own_stack_string(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    reason: RootId,
) -> Result<Option<String>, String> {
    let is_error = runtime
        .is_error_rooted(reason)
        .map_err(|error| rooted_error_message(runtime, program, error))?;
    let has_stack = is_error
        || runtime
            .has_own_property_rooted(reason, "stack")
            .map_err(|error| rooted_error_message(runtime, program, error))?;
    if !has_stack {
        return Ok(None);
    }
    let stack = get_rooted_property(runtime, program, reason, "stack")?;
    let text = runtime
        .string_text_rooted(stack)
        .map_err(|error| rooted_error_message(runtime, program, error));
    runtime.release_root(stack);
    text
}

fn get_rooted_property(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    object: RootId,
    name: &str,
) -> Result<RootId, String> {
    let key = runtime.string_rooted(name);
    let property = runtime.get_property_rooted(object, key);
    runtime.release_root(key);
    property.map_err(|error| rooted_error_message(runtime, program, error))
}

fn set_rooted_text_property(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    object: RootId,
    name: &str,
    text: &str,
) -> Result<(), String> {
    let key = runtime.string_rooted(name);
    let value = runtime.string_rooted(text);
    let result = runtime.set_property_rooted(object, key, value, object);
    runtime.release_root(key);
    runtime.release_root(value);
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("cannot set rejection Error.{name}")),
        Err(error) => Err(rooted_error_message(runtime, program, error)),
    }
}

fn rooted_error_message(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    error: RootedError,
) -> String {
    let message = runtime.format_error(program, &error.error);
    if let Some(exception) = error.exception {
        runtime.release_root(exception);
    }
    message
}

fn emit_process_event(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    event_name: &str,
    args: &[RootId],
) -> Result<bool, String> {
    let Some(process) = shared_process_root(runtime) else {
        return Ok(false);
    };
    let event = runtime.string_rooted(event_name);
    let emit = get_rooted_property(runtime, program, process, "emit");
    let emit = match emit {
        Ok(emit) => emit,
        Err(error) => {
            runtime.release_root(event);
            return Err(error);
        }
    };
    let mut event_args = Vec::with_capacity(args.len() + 1);
    event_args.push(event);
    event_args.extend_from_slice(args);
    let emitted = runtime.call_rooted(emit, process, &event_args);
    runtime.release_root(emit);
    runtime.release_root(event);
    let emitted = emitted.map_err(|error| rooted_error_message(runtime, program, error))?;
    let handled = runtime
        .rooted_value(emitted)
        .and_then(Value::as_bool)
        .ok_or_else(|| "process.emit returned a non-boolean result".to_owned());
    runtime.release_root(emitted);
    handled
}

fn schedule_unhandled_rejection_warnings(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    id: u64,
    reason: RootId,
) -> Result<(), String> {
    let stack = match own_stack_string(runtime, program, reason) {
        Ok(stack) => stack,
        Err(_) => None,
    };
    let message = match &stack {
        Some(stack) => stack.clone(),
        None => detail_string_rooted(runtime, program, reason)?,
    };
    schedule_unhandled_rejection_warnings_with_message(
        runtime,
        program,
        id,
        &message,
        stack.as_deref(),
    )
}

fn schedule_unhandled_rejection_warnings_with_message(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    id: u64,
    message: &str,
    stack: Option<&str>,
) -> Result<(), String> {
    schedule_rejection_warning(
        runtime,
        program,
        "UnhandledPromiseRejectionWarning",
        message,
        stack,
        None,
    )?;
    schedule_unhandled_rejection_note(runtime, program, id, stack)
}

fn schedule_unhandled_rejection_note(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    id: u64,
    stack: Option<&str>,
) -> Result<(), String> {
    let message = format!(
        "Unhandled promise rejection. This error originated either by throwing inside of an async function without a catch block, or by rejecting a promise which was not handled with .catch(). {UNHANDLED_REJECTION_CLI_GUIDANCE} (rejection id: {id})"
    );
    schedule_rejection_warning(
        runtime,
        program,
        "UnhandledPromiseRejectionWarning",
        &message,
        stack,
        None,
    )
}

fn schedule_rejection_warning(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    name: &str,
    message: &str,
    stack: Option<&str>,
    warning_id: Option<u64>,
) -> Result<(), String> {
    let Some(process) = shared_process_root(runtime) else {
        return Ok(());
    };
    let emit_warning = get_rooted_property(runtime, program, process, "emitWarning")?;
    let message = runtime.string_rooted(message);
    let options = match runtime.object_rooted() {
        Ok(options) => options,
        Err(error) => {
            runtime.release_root(emit_warning);
            runtime.release_root(message);
            return Err(rooted_error_message(runtime, program, error));
        }
    };
    let type_key = runtime.string_rooted("type");
    let type_value = runtime.string_rooted(name);
    let stack_key = runtime.string_rooted(HOST_WARNING_STACK_OPTION);
    let stack_value = stack.map(|stack| runtime.string_rooted(stack));
    let id_key = runtime.string_rooted(HOST_WARNING_ID_OPTION);
    let id_value = warning_id.map(|id| runtime.root(Value::number(id as f64)));
    let set_type = runtime.set_property_rooted(options, type_key, type_value, options);
    runtime.release_root(type_key);
    runtime.release_root(type_value);
    let set_stack = stack_value.map(|stack| {
        let result = runtime.set_property_rooted(options, stack_key, stack, options);
        runtime.release_root(stack);
        result
    });
    runtime.release_root(stack_key);
    let set_id = id_value.map(|id| {
        let result = runtime.set_property_rooted(options, id_key, id, options);
        runtime.release_root(id);
        result
    });
    runtime.release_root(id_key);
    if let Some(set_stack) = set_stack {
        match set_stack {
            Ok(true) => {}
            Ok(false) => {
                runtime.release_root(emit_warning);
                runtime.release_root(message);
                runtime.release_root(options);
                return Err("cannot set process.emitWarning stack".into());
            }
            Err(error) => {
                runtime.release_root(emit_warning);
                runtime.release_root(message);
                runtime.release_root(options);
                return Err(rooted_error_message(runtime, program, error));
            }
        }
    }
    if !matches!(set_type, Ok(true)) {
        runtime.release_root(emit_warning);
        runtime.release_root(message);
        runtime.release_root(options);
        return match set_type {
            Ok(false) => Err("cannot set process.emitWarning type".into()),
            Err(error) => Err(rooted_error_message(runtime, program, error)),
            Ok(true) => unreachable!(),
        };
    }
    if !matches!(set_id, None | Some(Ok(true))) {
        runtime.release_root(emit_warning);
        runtime.release_root(message);
        runtime.release_root(options);
        return match set_id.expect("checked present") {
            Ok(false) => Err("cannot set process.emitWarning id".into()),
            Err(error) => Err(rooted_error_message(runtime, program, error)),
            Ok(true) => unreachable!(),
        };
    }
    let result = runtime.call_rooted(emit_warning, process, &[message, options]);
    runtime.release_root(emit_warning);
    runtime.release_root(message);
    runtime.release_root(options);
    result
        .map(|result| {
            runtime.release_root(result);
        })
        .map_err(|error| rooted_error_message(runtime, program, error))
}

fn emit_exit(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    code: i32,
) -> Result<(), String> {
    let shared_state = runtime.host_mut().shared_state();
    shared_state
        .borrow()
        .process_control
        .begin_exit_emission();
    let listeners = shared_state.borrow_mut().scheduler.begin_shared_exit();
    if let Some(process) = shared_process_root(runtime) {
        let event_count = shared_state.borrow().scheduler.shared_listener_count();
        let setup = set_shared_property(
            runtime,
            program,
            process,
            "_eventsCount",
            quench_runtime::Value::number(event_count as f64),
        )
        .and_then(|()| {
            set_shared_property(
                runtime,
                program,
                process,
                "_exiting",
                quench_runtime::Value::TRUE,
            )
        });
        if let Err(error) = setup {
            for callback in listeners {
                release_callback(runtime, callback);
            }
            return Err(error);
        }
    }
    let mut listeners = listeners.into_iter();
    while let Some(mut listener) = listeners.next() {
        listener
            .args
            .push(runtime.root(quench_runtime::Value::number(code as f64)));
        if let Err(error) = invoke(runtime, program, listener) {
            for callback in listeners {
                release_callback(runtime, callback);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn invoke(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callback: crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), String> {
    let result = runtime.call_rooted(callback.callback, callback.receiver, &callback.args);
    release_callback_roots(runtime, callback.callback, callback.receiver, callback.args);
    match result {
        Ok(value) => {
            runtime.release_root(value);
            Ok(())
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    }
}

fn release_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    callback: crate::modules::shared_event_loop::SharedCallback,
) {
    release_callback_roots(runtime, callback.callback, callback.receiver, callback.args);
}

fn release_callback_roots(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    callback: RootId,
    receiver: RootId,
    args: Vec<RootId>,
) {
    runtime.release_root(callback);
    runtime.release_root(receiver);
    for argument in args {
        runtime.release_root(argument);
    }
}

fn shared_process_root(runtime: &mut quench_runtime::Runtime<NodeHost>) -> Option<RootId> {
    runtime.host_mut().shared_state().borrow().process_module
}

fn set_shared_property(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    object: RootId,
    name: &str,
    value: quench_runtime::Value,
) -> Result<(), String> {
    let key = runtime.string_rooted(name);
    let value = runtime.root(value);
    let updated = runtime.set_property_rooted(object, key, value, object);
    runtime.release_root(key);
    runtime.release_root(value);
    match updated {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("cannot update process.{name}")),
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    }
}

pub(crate) fn next_tick(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = received_type(context, callback)?;
        return invalid_callback(context, &received);
    }
    if state.borrow().scheduler.shared_is_exiting() {
        return Ok(context.undefined());
    }

    let callback = context.retain(callback)?;
    let undefined = context.undefined();
    let receiver = context.retain(undefined)?;
    let retained_args = args[1..]
        .iter()
        .copied()
        .map(|argument| context.retain(argument))
        .collect::<Result<Vec<_>, _>>()?;
    state.borrow_mut().scheduler.queue_shared_next_tick(
        crate::modules::shared_event_loop::SharedCallback {
            callback,
            receiver,
            args: retained_args,
        },
    );
    Ok(context.undefined())
}

pub(crate) fn on(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    register_process_listener(context, receiver, args, false)
}

pub(crate) fn once(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    register_process_listener(context, receiver, args, true)
}

pub(crate) fn remove_listener(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let event_root = args.first().copied().unwrap_or_else(|| context.undefined());
    let event = process_event_name(context, event_root)?;
    let Some(callback) = args.get(1).copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = received_type(context, callback)?;
        return invalid_callback(context, &received);
    }
    let callback_value = context
        .rooted_value(callback)
        .ok_or_else(|| RootedError::host("invalid process listener callback root"))?;
    let snapshots = context
        .host_mut()
        .shared_state()
        .borrow()
        .scheduler
        .shared_listener_snapshots(&event);
    let listener = snapshots
        .into_iter()
        .rev()
        .find(|listener| context.rooted_value(listener.callback) == Some(callback_value));
    let Some(listener) = listener else {
        return Ok(receiver);
    };
    let removed = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .remove_shared_listener(&event, listener.id);
    let Some((callback, event_root, event_removed)) = removed else {
        return Ok(receiver);
    };
    release_context_callback(context, callback);
    if let Some(event_root) = event_root {
        context.release_root(event_root);
    }
    if event_removed {
        update_process_event_count(context, receiver)?;
    }
    Ok(receiver)
}

pub(crate) fn remove_all_listeners(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let event = match args.first().copied() {
        Some(value)
            if !context
                .rooted_value(value)
                .is_some_and(quench_runtime::Value::is_undefined) =>
        {
            Some(process_event_name(context, value)?)
        }
        _ => None,
    };
    let (callbacks, event_roots) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .remove_all_shared_listeners(event.as_ref());
    for callback in callbacks {
        release_context_callback(context, callback);
    }
    for root in event_roots {
        context.release_root(root);
    }
    update_process_event_count(context, receiver)?;
    Ok(receiver)
}

fn register_process_listener(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
    once: bool,
) -> Result<RootId, RootedError> {
    let event_root = args.first().copied().unwrap_or_else(|| context.undefined());
    let event = process_event_name(context, event_root)?;
    let Some(callback) = args.get(1).copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = received_type(context, callback)?;
        return invalid_callback(context, &received);
    }
    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .reserve_shared_listener_id()
        .ok_or_else(|| RootedError::host("process listener identifier space exhausted"))?;
    let callback = context.retain(callback)?;
    let retained_receiver = context.retain(receiver)?;
    let event = retain_process_event_name(context, event, event_root)?;
    let (new_event, duplicate_root) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .add_shared_listener(
            event,
            id,
            crate::modules::shared_event_loop::SharedCallback {
                callback,
                receiver: retained_receiver,
                args: Vec::new(),
            },
            once,
        );
    if let Some(root) = duplicate_root {
        context.release_root(root);
    }
    if new_event {
        let count = context
            .host_mut()
            .shared_state()
            .borrow()
            .scheduler
            .shared_listener_count();
        let count = context.number(count as f64);
        install(context, receiver, "_eventsCount", count)?;
    }
    Ok(receiver)
}

pub(crate) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let event_root = args.first().copied().unwrap_or_else(|| context.undefined());
    let event = process_event_name(context, event_root)?;
    let snapshots = context
        .host_mut()
        .shared_state()
        .borrow()
        .scheduler
        .shared_listener_snapshots(&event);
    let had_listeners = !snapshots.is_empty();
    let mut listeners = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        let callback = match context.retain(snapshot.callback) {
            Ok(callback) => callback,
            Err(error) => {
                release_listener_snapshots(context, listeners);
                return Err(error);
            }
        };
        let retained_receiver = match context.retain(snapshot.receiver) {
            Ok(receiver) => receiver,
            Err(error) => {
                context.release_root(callback);
                release_listener_snapshots(context, listeners);
                return Err(error);
            }
        };
        listeners.push((snapshot, callback, retained_receiver));
    }
    let event_args = args.get(1..).unwrap_or_default();
    let mut listeners = listeners.into_iter();
    while let Some((listener, callback, retained_receiver)) = listeners.next() {
        if listener.once {
            let removed = context
                .host_mut()
                .shared_state()
                .borrow_mut()
                .scheduler
                .take_shared_once_listener(&event, listener.id);
            let Some((stored, event_root, event_removed)) = removed else {
                context.release_root(callback);
                context.release_root(retained_receiver);
                continue;
            };
            release_context_callback(context, stored);
            if let Some(event_root) = event_root {
                context.release_root(event_root);
            }
            if event_removed {
                if let Err(error) = update_process_event_count(context, receiver) {
                    context.release_root(callback);
                    context.release_root(retained_receiver);
                    release_listener_snapshots(context, listeners);
                    return Err(error);
                }
            }
        }
        let result = context.call_rooted(callback, retained_receiver, event_args);
        context.release_root(callback);
        context.release_root(retained_receiver);
        match result {
            Ok(result) => {
                context.release_root(result);
            }
            Err(error) => {
                release_listener_snapshots(context, listeners);
                return Err(error);
            }
        }
    }
    Ok(context.boolean(had_listeners))
}

fn release_listener_snapshots(
    context: &mut NativeContext<'_, NodeHost>,
    listeners: impl IntoIterator<
        Item = (
            crate::modules::shared_event_loop::SharedListenerSnapshot,
            RootId,
            RootId,
        ),
    >,
) {
    for (_, callback, receiver) in listeners {
        context.release_root(callback);
        context.release_root(receiver);
    }
}

fn release_context_callback(
    context: &mut NativeContext<'_, NodeHost>,
    callback: crate::modules::shared_event_loop::SharedCallback,
) {
    context.release_root(callback.callback);
    context.release_root(callback.receiver);
    for argument in callback.args {
        context.release_root(argument);
    }
}

fn update_process_event_count(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
) -> Result<(), RootedError> {
    let count = context
        .host_mut()
        .shared_state()
        .borrow()
        .scheduler
        .shared_listener_count();
    let count = context.number(count as f64);
    install(context, process, "_eventsCount", count)
}

fn process_event_name(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<crate::modules::shared_event_loop::SharedEventKey, RootedError> {
    if let Some(event) = context.string_text(value)? {
        return Ok(crate::modules::shared_event_loop::SharedEventKey::String(
            event,
        ));
    }
    if context.is_symbol_rooted(value)? {
        let identity = context
            .rooted_value(value)
            .ok_or_else(|| RootedError::host("invalid process event symbol root"))?;
        return Ok(crate::modules::shared_event_loop::SharedEventKey::Symbol {
            identity,
            root: value,
        });
    }
    let received = received_type(context, value)?;
    let error = context.type_error_rooted(&format!(
        "The \"type\" argument must be of type string. Received {received}"
    ))?;
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    install(context, error, "code", code)?;
    Err(context.throw(error))
}

fn retain_process_event_name(
    context: &mut NativeContext<'_, NodeHost>,
    event: crate::modules::shared_event_loop::SharedEventKey,
    source: RootId,
) -> Result<crate::modules::shared_event_loop::SharedEventKey, RootedError> {
    match event {
        crate::modules::shared_event_loop::SharedEventKey::String(name) => Ok(
            crate::modules::shared_event_loop::SharedEventKey::String(name),
        ),
        crate::modules::shared_event_loop::SharedEventKey::Symbol { identity, .. } => {
            let root = context.retain(source)?;
            Ok(crate::modules::shared_event_loop::SharedEventKey::Symbol { identity, root })
        }
    }
}

pub(crate) fn emit_warning(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let message_root = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(message) = context.string_text(message_root)? else {
        let received = received_type(context, message_root)?;
        let error = context.type_error_rooted(&format!(
            "The \"warning\" argument must be of type string. Received {received}"
        ))?;
        return Err(throw_with_code(context, error, "ERR_INVALID_ARG_TYPE"));
    };

    let options = args.get(1).copied();
    let options_value = options
        .map(|options| context.rooted_value(options))
        .flatten()
        .ok_or_else(|| RootedError::host("invalid warning options root"));
    let (name, option_code, detail, host_stack, rejection_id) = match (options, options_value) {
        (Some(options), Ok(value)) if !value.is_undefined() && !value.is_null() => (
            string_property(context, options, "type")?
                .or(string_property(context, options, "name")?)
                .or(context.string_text(options)?)
                .unwrap_or_else(|| "Warning".into()),
            string_property(context, options, "code")?,
            string_property(context, options, "detail")?,
            string_property(context, options, HOST_WARNING_STACK_OPTION)?,
            number_property(context, options, HOST_WARNING_ID_OPTION)?
                .filter(|id| id.is_finite() && *id >= 1.0 && id.fract() == 0.0)
                .map(|id| id as u64),
        ),
        (Some(_), Err(error)) => return Err(error),
        _ => ("Warning".into(), None, None, None, None),
    };
    let code = match args.get(2).copied() {
        Some(code) => context.string_text(code)?.or(option_code),
        None => option_code,
    };

    let warning = context.error_rooted(&message)?;
    let name_value = context.string_rooted(&name);
    install(context, warning, "name", name_value)?;
    if let Some(code) = &code {
        let code_value = context.string_rooted(code);
        install(context, warning, "code", code_value)?;
    }
    if let Some(detail) = &detail {
        let detail_value = context.string_rooted(detail);
        install(context, warning, "detail", detail_value)?;
    }
    if let Some(stack) = host_stack {
        let stack_value = context.string_rooted(&stack);
        install(context, warning, "stack", stack_value)?;
    }
    if let Some(id) = rejection_id {
        let id_value = context.number(id as f64);
        install(context, warning, "id", id_value)?;
    }

    let callback =
        context.host_function(crate::host::shared_vm::operation("processDispatchWarning"))?;
    let callback = context.retain(callback)?;
    let undefined = context.undefined();
    let receiver = context.retain(undefined)?;
    let warning_argument = context.retain(warning)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(crate::modules::shared_event_loop::SharedCallback {
            callback,
            receiver,
            args: vec![warning_argument],
        });
    Ok(context.undefined())
}

pub(crate) fn dispatch_warning(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(warning) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let name = string_property(context, warning, "name")?.unwrap_or_else(|| "Warning".into());
    let message = string_property(context, warning, "message")?.unwrap_or_default();
    let code = string_property(context, warning, "code")?;
    let detail = string_property(context, warning, "detail")?;

    let suppress_stderr = context
        .host_mut()
        .shared_state()
        .borrow()
        .exec_argv
        .iter()
        .any(|argument| argument == "--no-warnings");
    if !suppress_stderr {
        let code = code.map_or_else(String::new, |code| format!(" [{code}]"));
        let mut output = format!("(node:{}){code} {name}: {message}\n", std::process::id());
        if let Some(detail) = detail {
            output.push_str(&detail);
            output.push('\n');
        }
        std::io::stderr()
            .lock()
            .write_all(output.as_bytes())
            .map_err(|error| RootedError::host(error.to_string()))?;
    }
    let global = context.global_root()?;
    let process_key = context.string_rooted("process");
    let process = context.get_property_rooted(global, process_key);
    context.release_root(global);
    context.release_root(process_key);
    let process = process?;
    let event = context.string_rooted("warning");
    let emitted = emit(context, process, &[event, warning]);
    context.release_root(event);
    context.release_root(process);
    let emitted = emitted?;
    context.release_root(emitted);
    Ok(context.undefined())
}

fn string_property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<String>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    context.string_text(value)
}

fn number_property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<f64>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    let number = context
        .rooted_value(value)
        .and_then(|value| value.as_number());
    context.release_root(value);
    context.release_root(key);
    Ok(number)
}

fn invalid_callback(
    context: &mut NativeContext<'_, NodeHost>,
    received: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"callback\" argument must be of type function. Received {received}"
    ))?;
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    install(context, error, "code", code)?;
    Err(context.throw(error))
}

fn received_type(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
) -> Result<String, RootedError> {
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("invalid process callback root"))?;
    if value.is_undefined() {
        return Ok("undefined".to_owned());
    }
    if value.is_null() {
        return Ok("null".to_owned());
    }
    if let Some(boolean) = value.as_bool() {
        return Ok(format!("type boolean ({boolean})"));
    }
    if let Some(number) = value.as_number() {
        return Ok(format!("type number ({number})"));
    }
    if context.is_callable_rooted(root)? {
        return Ok("function ".to_owned());
    }
    if is_array(context, root)? {
        return Ok("an instance of Array".to_owned());
    }
    Ok("an instance of Object".to_owned())
}

#[cfg(test)]
#[path = "process/shared_vm_tests.rs"]
mod tests;
