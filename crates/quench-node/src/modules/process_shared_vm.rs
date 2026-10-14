//! Shared-VM bindings for the existing process state, extended as APIs migrate.

use crate::host::node_host::NodeHost;
use crate::modules::process_state::{self, ProcessFact, UnhandledRejectionMode};
use quench_runtime::{NativeArithmetic, NativeContext, PromiseRejectionEvent, RootId, RootedError, Value};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

pub(crate) fn collect_garbage(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.collect()?;
    Ok(context.undefined())
}

const STDOUT_FD: i32 = 1;
const STDERR_FD: i32 = 2;
static HRTIME_ORIGIN: OnceLock<Instant> = OnceLock::new();
const MAX_SAFE_EXIT_CODE: f64 = 9_007_199_254_740_991.0;
const HOST_WARNING_STACK_OPTION: &str = "\0quench:process-warning-stack";
const HOST_WARNING_ID_OPTION: &str = "\0quench:process-warning-id";
const HOST_CAPTURE_CALLBACK_PROPERTY: &str = "\0quench:process-capture-callback";
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
    let really_exit = context.host_function(crate::host::shared_vm::operation("reallyExit"))?;
    install(context, process, "reallyExit", really_exit)?;
    let stdout = create_stream(context, STDOUT_FD)?;
    install(context, process, "stdout", stdout)?;
    crate::modules::process_stdin_shared_vm::install(context, process)?;
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
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        install(context, process, name, function)?;
    }
    #[cfg(unix)]
    {
        let function =
            context.host_function(crate::host::shared_vm::operation("processGetgroups"))?;
        let key = context.string_rooted("name");
        let name = context.string_rooted("getgroups");
        context.define_data_property_rooted(function, key, name, false, false, true)?;
        install(context, process, "getgroups", function)?;
    }
    let error_instance = context.evaluate_script_rooted(
        "((Error) => value => value instanceof Error)(Error)",
        "node:process/warning-instance.js",
    )?;
    let emit_warning = context.host_function_with_data(
        crate::host::shared_vm::operation("processEmitWarning"),
        error_instance,
    )?;
    install(context, process, "emitWarning", emit_warning)?;
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
    #[cfg(target_os = "linux")]
    {
        let signals = super::os_shared_vm::signal_constants(context)?;
        let data = context.array_rooted(&[process, signals])?;
        let operation = context
            .host_function_with_data(crate::host::shared_vm::operation("processKill"), data)?;
        let factory = context.evaluate_script_rooted(
            "operation => function kill(pid, signal) { return operation(pid, signal); }",
            "node:process/shared-kill.js",
        )?;
        let undefined = context.undefined();
        let kill = context.call_rooted(factory, undefined, &[operation])?;
        install(context, process, "kill", kill)?;
    }

    let raw_debug = context.host_function(crate::host::shared_vm::operation("processRawDebug"))?;
    install(context, process, "_rawDebug", raw_debug)?;
    let set_capture = context.host_function(crate::host::shared_vm::operation(
        "processSetUncaughtExceptionCaptureCallback",
    ))?;
    install(
        context,
        process,
        "setUncaughtExceptionCaptureCallback",
        set_capture,
    )?;
    let has_capture = context.host_function(crate::host::shared_vm::operation(
        "processHasUncaughtExceptionCaptureCallback",
    ))?;
    install(
        context,
        process,
        "hasUncaughtExceptionCaptureCallback",
        has_capture,
    )?;
    let active_handles =
        context.host_function(crate::host::shared_vm::operation("processGetActiveHandles"))?;
    install(context, process, "_getActiveHandles", active_handles)?;
    let active_network_resources = context.host_function(crate::host::shared_vm::operation(
        "processGetActiveNetworkResources",
    ))?;
    install(
        context,
        process,
        "__quenchGetActiveNetworkResources",
        active_network_resources,
    )?;
    let active_timer_resources = context.host_function(crate::host::shared_vm::operation("processGetActiveTimerResources"))?;
    install(context, process, "__quenchGetActiveTimerResources", active_timer_resources)?;
    let hrtime_raw =
        context.host_function(crate::host::shared_vm::operation("processHrtimeNow"))?;
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
    let remove_all_listeners = context.host_function(crate::host::shared_vm::operation(
        "processRemoveAllListeners",
    ))?;
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
    if let Some(pair) = exec_argv.windows(2).rev().find(|pair| {
        matches!(
            pair[0].as_str(),
            "-e" | "--eval" | "-p" | "--print" | "-pe" | "-ep"
        )
    }) {
        set_text(context, process, "_eval", &pair[1])?;
    }
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
    super::child_process_ipc_shared_vm::initialize(context)?;
    let env = context.object_rooted()?;
    for (name, value) in std::env::vars() {
        if context
            .host_mut()
            .shared_state()
            .borrow()
            .process_ipc
            .channel
            .is_some()
            && matches!(
                name.as_str(),
                "NODE_CHANNEL_FD" | "NODE_CHANNEL_SERIALIZATION_MODE"
            )
        {
            continue;
        }
        let key = context.string_rooted(&name);
        let value = context.string_rooted(&value);
        if !context.set_property_rooted(env, key, value, env)? {
            return Err(RootedError::host("cannot install process.env entry"));
        }
    }
    install(context, process, "env", env)?;
    let load_env_file =
        context.host_function(crate::host::shared_vm::operation("processLoadEnvFile"))?;
    install(context, process, "loadEnvFile", load_env_file)?;
    let set_env = context.host_function(crate::host::shared_vm::operation("processSetEnv"))?;
    install(context, process, "__quenchSetEnv", set_env)?;
    let delete_env =
        context.host_function(crate::host::shared_vm::operation("processDeleteEnv"))?;
    install(context, process, "__quenchDeleteEnv", delete_env)?;
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
    let executable =
        std::env::current_exe().map_err(|error| RootedError::host(error.to_string()))?;
    let node_executable = crate::shared_run::executable_path()
        .map_err(|error| RootedError::host(error.to_string()))?;
    set_text(
        context,
        process,
        "execPath",
        &node_executable.to_string_lossy(),
    )?;
    let host_argv0 = std::env::args_os().next().map(std::path::PathBuf::from);
    let argv0_is_runtime = host_argv0
        .as_deref()
        .and_then(|argv0| std::fs::canonicalize(argv0).ok())
        .is_some_and(|argv0| argv0 == executable);
    let argv0 = if argv0_is_runtime {
        node_executable.clone()
    } else {
        host_argv0.unwrap_or_else(|| node_executable.clone())
    };
    set_text(context, process, "argv0", &argv0.to_string_lossy())?;
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
    install(context, stream, "_isStdio", writable)?;
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
        ProcessFact::SharedOpenSsl => Ok(context.boolean(process_state::shared_openssl())),
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
    let process_control = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
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

pub(crate) fn really_exit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let mask = context.number(-1.0);
    // Node's native Int32Value boundary exits with zero when coercion throws.
    let code = match context.arithmetic_rooted(NativeArithmetic::BitwiseAnd, value, mask) {
        Ok(value) => context.rooted_value(value).and_then(Value::as_number).unwrap_or(0.0) as i32,
        Err(error) => {
            if let Some(exception) = error.exception { context.release_root(exception); }
            0
        }
    };
    context.host_mut().shared_state().borrow().process_control.request_really_exit(code);
    Err(RootedError::host("process.exit"))
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
    let message = format!(
        "The value of \"code\" is out of range. It must be a safe integer. Received {received}"
    );
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
    let bytes = if let Some(text) = context.string_text(chunk)? {
        text.into_bytes()
    } else if context
        .has_builtin_brand_rooted(chunk, quench_runtime::BuiltinBrand::ArrayBufferView)?
    {
        context
            .view_bytes_rooted(chunk)
            .ok_or_else(|| RootedError::host("process stream byte view unavailable"))?
    } else {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            context,
            chunk,
            "chunk",
            "of type string or an instance of Buffer, TypedArray, or DataView",
        )?);
    };
    if fd == STDERR_FD {
        std::io::stderr()
            .lock()
            .write_all(&bytes)
            .map_err(|error| RootedError::host(error.to_string()))?;
    } else {
        let output = context.host_mut().shared_state().borrow().output.clone();
        if let Some(output) = output {
            // Preserve the embedding API's existing text-only sink contract.
            output(&String::from_utf8_lossy(&bytes));
        } else {
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&bytes)
                .and_then(|()| stdout.flush())
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
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
    match cwd.query_path() {
        Ok(path) => Ok(context.string_rooted(&path.to_string_lossy())),
        Err(error) => {
            let (code, errno) = super::fs_error_codes::code_for(&error);
            let message = if code == "ENOENT" {
                "ENOENT: process.cwd failed with error no such file or directory, the current working directory was likely removed without changing the working directory, uv_cwd".to_owned()
            } else {
                format!("{code}: {}, uv_cwd", super::fs_error_codes::strerror(code))
            };
            let error = context.error_rooted(&message)?;
            set_text(context, error, "code", code)?;
            set_text(context, error, "syscall", "uv_cwd")?;
            let errno = context.number(errno as f64);
            install(context, error, "errno", errno)?;
            Err(context.throw(error))
        }
    }
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
    if let Err(error) = initgroups_syscall(&user_name, group_id) {
        return Err(throw_os_process_error(context, "initgroups", error)?);
    }
    Ok(context.undefined())
}

fn is_array(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<bool, RootedError> {
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
    if let Err(error) = setgroups_syscall(groups) {
        return Err(throw_os_process_error(context, "setgroups", error)?);
    }
    Ok(context.undefined())
}

#[cfg(unix)]
fn initgroups_syscall(user: &std::ffi::CString, group: u32) -> std::io::Result<()> {
    let group = group
        .try_into()
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    let result = unsafe { libc::initgroups(user.as_ptr(), group) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn initgroups_syscall(_: &std::ffi::CString, _: u32) -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(libc::ENOSYS))
}

#[cfg(unix)]
fn setgroups_syscall(groups: &[u32]) -> std::io::Result<()> {
    let groups = groups
        .iter()
        .copied()
        .map(|group| {
            libc::gid_t::try_from(group)
                .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let count = groups
        .len()
        .try_into()
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    let result = unsafe { libc::setgroups(count, groups.as_ptr()) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn setgroups_syscall(_: &[u32]) -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(libc::ENOSYS))
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
    error: std::io::Error,
) -> Result<RootedError, RootedError> {
    let errno = error.raw_os_error().unwrap_or(libc::EINVAL);
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
        &format!(
            "The \"extraGroup\" argument must be one of type number or string. Received {received}"
        ),
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
        &format!(
            "The \"{label}\" argument must be one of type number or string. Received {received}"
        ),
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
                CredentialKind::Uid | CredentialKind::Euid => libc::getpwnam(name.as_ptr())
                    .as_ref()
                    .map(|entry| entry.pw_uid),
                CredentialKind::Gid | CredentialKind::Egid => libc::getgrnam(name.as_ptr())
                    .as_ref()
                    .map(|entry| entry.gr_gid),
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
        &format!("The \"id\" argument must be one of type number or string. Received {received}"),
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
    #[cfg(target_os = "linux")]
    let (pid, signal) = {
        let undefined = context.undefined();
        let zero = context.number(0.0);
        let pid = args.first().copied().unwrap_or(undefined);
        let signal = args.get(1).copied().unwrap_or(undefined);
        let pid = context.bitwise_or_rooted(pid, zero)?;
        let signal = context.bitwise_or_rooted(signal, zero)?;
        let pid = context
            .rooted_value(pid)
            .and_then(|value| value.as_number())
            .expect("bitwise number") as i32;
        let signal = context
            .rooted_value(signal)
            .and_then(|value| value.as_number())
            .expect("bitwise number") as i32;
        (pid, signal)
    };
    #[cfg(not(target_os = "linux"))]
    let (pid, signal) = (
        integer_argument(context, args.first().copied(), "pid")?,
        integer_argument(context, args.get(1).copied(), "signal")?,
    );
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
    #[cfg(target_os = "linux")]
    let error = -error;
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

pub(crate) fn set_uncaught_exception_capture_callback(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return Err(uncaught_capture_type_error(context, None)?);
    };
    let current_key = context.string_rooted(HOST_CAPTURE_CALLBACK_PROPERTY);
    let current = context.get_property_rooted(process, current_key)?;
    if context.is_callable_rooted(current)? {
        let error = context.error_rooted(
            "setupUncaughtExceptionCapture() called while a capture callback is already set",
        )?;
        return Err(throw_with_code(
            context,
            error,
            "ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET",
        ));
    }
    let is_null = context.rooted_value(callback).is_some_and(Value::is_null);
    if !is_null && !context.is_callable_rooted(callback)? {
        return Err(uncaught_capture_type_error(context, Some(callback))?);
    }
    let value = if is_null {
        context.undefined()
    } else {
        callback
    };
    if !context.define_data_property_rooted(process, current_key, value, true, false, true)? {
        return Err(RootedError::host(
            "cannot set process uncaught-exception capture callback",
        ));
    }
    Ok(context.undefined())
}

pub(crate) fn has_uncaught_exception_capture_callback(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(HOST_CAPTURE_CALLBACK_PROPERTY);
    let callback = context.get_property_rooted(process, key)?;
    let has_callback = context.is_callable_rooted(callback)?;
    Ok(context.boolean(has_callback))
}

pub(crate) fn get_active_handles(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let state = shared.borrow();
    let handles = state
        .net_sockets
        .values()
        .map(|socket| socket.root)
        .chain(state.net_servers.values().map(|server| server.root))
        .collect::<Vec<_>>();
    context.array_rooted(&handles)
}

pub(crate) fn get_active_network_resources(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let state = shared.borrow();
    let active_sockets = state
        .net_sockets
        .iter()
        .filter(|(id, socket)| {
            crate::modules::net_shared_vm::socket_has_ref(&state.tcp, **id) == Some(true)
                && (socket.parent_server.is_none()
                    || crate::modules::net_shared_vm::connection_delivery_started(&state.tcp, **id))
        })
        .count()
        + state
            .net_closing_sockets
            .keys()
            .filter(|id| {
                crate::modules::net_shared_vm::socket_has_ref(&state.tcp, **id) == Some(true)
            })
            .count();
    let active_servers = state
        .net_servers
        .keys()
        .filter(|id| {
            crate::modules::net_shared_vm::listener_has_ref(&state.tcp, **id) == Some(true)
        })
        .count();
    let mut resources = Vec::with_capacity(active_sockets + active_servers);
    for _ in 0..active_sockets {
        resources.push(context.string_rooted("TCPSocketWrap"));
    }
    for _ in 0..active_servers {
        resources.push(context.string_rooted("TCPServerWrap"));
    }
    context.array_rooted(&resources)
}

fn uncaught_capture_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> Result<RootedError, RootedError> {
    let received = value.map_or_else(
        || Ok("undefined".to_owned()),
        |value| received_type(context, value),
    )?;
    throw_process_error(
        context,
        &format!("The \"fn\" argument must be of type function or null. Received {received}"),
        "ERR_INVALID_ARG_TYPE",
        true,
    )
}

pub(crate) fn set_env(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(key_root) = args.first().copied() else {
        return Err(RootedError::host("process.env setter is missing a key"));
    };
    let Some(value_root) = args.get(1).copied() else {
        return Err(RootedError::host("process.env setter is missing a value"));
    };
    let key = context.string_text(key_root)?.unwrap_or_default();
    let value = context.string_text(value_root)?.unwrap_or_default();
    if key.contains('=') || key.contains('\0') || value.contains('\0') {
        let error = context.type_error_rooted("Invalid process.env key or value")?;
        return Err(context.throw(error));
    }
    // SAFETY: the shared VM serializes host environment reads and writes for
    // this process, matching Node's process.env behavior.
    unsafe { std::env::set_var(key, value) };
    Ok(context.undefined())
}

pub(crate) fn delete_env(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(key_root) = args.first().copied() else {
        return Err(RootedError::host("process.env deleter is missing a key"));
    };
    let key = context.string_text(key_root)?.unwrap_or_default();
    if key.contains('=') || key.contains('\0') {
        let error = context.type_error_rooted("Invalid process.env key")?;
        return Err(context.throw(error));
    }
    // SAFETY: see `set_env`; this is the matching process.env deletion path.
    unsafe { std::env::remove_var(key) };
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
    let contents = std::fs::read_to_string(&path)
        .map_err(|error| dotenv_file_error(context, &path_text, error))?;
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process.loadEnvFile called without process root"))?;
    let env_key = context.string_rooted("env");
    let env = context.get_property_rooted(process, env_key)?;
    for (key_text, value_text) in crate::modules::util_env::parse_env_file(&contents) {
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

#[cfg(not(target_os = "linux"))]
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
    let checkpoint = (|| loop {
        drain_checkpoint(runtime, program)?;
        if process_control.requested_exit_code().is_some() {
            return Ok(());
        }
        let state = runtime.host_mut().shared_state();
        let span = crate::modules::trace_state::EnvironmentSpan::enter(&state, "BeforeExit");
        let code = runtime.root(Value::number(
            process_control.exit_code().unwrap_or(0) as f64
        ));
        let emitted = emit_before_exit(runtime, program, code);
        runtime.release_root(code);
        drop(span);
        emitted?;
        drain_shared_jobs(runtime, program, &state)?;
        if process_control.requested_exit_code().is_some() || !has_referenced_shared_work(&state) {
            return Ok(());
        }
    })();
    if process_control.requested_exit_code().is_some() {
        return finish_requested_exit(runtime, program);
    }
    let shared_state = runtime.host_mut().shared_state();
    cleanup_environment(runtime, &shared_state);
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

fn emit_before_exit(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    code: RootId,
) -> Result<(), String> {
    let Some(process) = shared_process_root(runtime) else {
        return Ok(());
    };
    let key = runtime.string_rooted("emit");
    let operation = runtime.get_property_rooted(process, key);
    runtime.release_root(key);
    let operation = match operation {
        Ok(operation) => operation,
        Err(failure) => return route_callback_failure(runtime, program, failure),
    };
    let event = runtime.string_rooted("beforeExit");
    let callback = crate::modules::shared_event_loop::SharedCallback {
        callback: operation,
        receiver: process,
        args: vec![event, code],
    };
    let result = call_shared_callback(runtime, &callback);
    runtime.release_root(operation);
    runtime.release_root(event);
    match result {
        Ok(()) => Ok(()),
        Err(failure) => route_callback_failure(runtime, program, failure),
    }
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
    cleanup_environment(runtime, &shared_state);
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
    cleanup_environment(runtime, &shared_state);
    if process_control.exit_without_event() {
        super::tls_shared_vm::contexts::cleanup(runtime);
        super::buffer_shared_vm::cleanup(runtime);
        return Ok(());
    }
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
    let capture_key = runtime.string_rooted(HOST_CAPTURE_CALLBACK_PROPERTY);
    let capture = runtime.get_property_rooted(process, capture_key);
    runtime.release_root(capture_key);
    let capture = match capture {
        Ok(capture) => capture,
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            return Err(message);
        }
    };
    let has_capture = runtime
        .rooted_value(capture)
        .is_some_and(|value| !value.is_undefined() && !value.is_null());
    if has_capture {
        let captured = runtime.call_rooted(capture, process, &[exception]);
        runtime.release_root(capture);
        return match captured {
            Ok(result) => {
                runtime.release_root(result);
                Ok(true)
            }
            Err(error) => {
                let message = runtime.format_error(program, &error.error);
                if let Some(exception) = error.exception {
                    runtime.release_root(exception);
                }
                Err(message)
            }
        };
    }
    runtime.release_root(capture);
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
        if crate::modules::process_stdin_shared_vm::poll(runtime)? {
            continue;
        }
        if poll_process_signals(runtime, program, &shared_state)? {
            continue;
        }

        if crate::modules::child_process_ipc_shared_vm::poll(runtime, program)? {
            continue;
        }
        if crate::modules::child_process_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }
        if crate::modules::fetch_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }
        if crate::modules::http_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }

        if has_referenced_shared_work(&shared_state)
            && super::fs_watch_shared_vm::poll(runtime, program)?
        {
            continue;
        }
        let can_run_timers = has_referenced_shared_work(&shared_state);
        if can_run_timers && super::fs_stat_watch_shared_vm::poll(runtime, program)? {
            continue;
        }
        let due = if can_run_timers {
            shared_state
                .borrow_mut()
                .scheduler
                .take_due_shared_timer(std::time::Instant::now())
        } else {
            None
        };
        if let Some(mut timer) = due {
            let _span =
                crate::modules::trace_state::EnvironmentSpan::enter(&shared_state, "RunTimers");
            let api = shared_state.borrow().timer_handle_api.expect("queued timer handle API");
            let (primitive_id, mut callback_result) = match runtime.get_property_rooted(timer.callback.receiver, api.async_id_key) {
                Ok(id) => (Some(id), invoke_timer_callback(runtime, program, &timer.callback)),
                Err(error) => (None, route_callback_failure(runtime, program, error)),
            };

            let repeated = runtime.call_rooted(api.repeat_delay, timer.callback.receiver, &[]);
            match repeated {
                Ok(repeated) => {
                    timer.interval = runtime.rooted_value(repeated).and_then(|value| value.as_number())
                        .map(|value| std::time::Duration::from_millis(value.max(0.0).trunc() as u64));
                    runtime.release_root(repeated);
                }
                Err(error) => {
                    callback_result = route_callback_failure(runtime, program, error);
                    shared_state.borrow_mut().scheduler.cancel_shared_timer(timer.id);
                }
            }
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
                let settled = mark_timer_destroyed(runtime, callback.receiver).and_then(|()| {
                    let undefined = runtime.root(Value::UNDEFINED);
                    let args = [callback.receiver, primitive_id.unwrap_or(undefined)];
                    let args = if primitive_id.is_some() { &args[..] } else { &args[..1] };
                    let result = runtime.call_rooted(api.primitive_cleanup, undefined, args);
                    runtime.release_root(undefined);
                    result.map(|result| { runtime.release_root(result); })
                });
                release_callback(runtime, callback);
                if let Err(error) = settled {
                    if let Some(id) = primitive_id { runtime.release_root(id); }
                    route_callback_failure(runtime, program, error)?;
                    callback_result?;
                    continue;
                }
            }
            if let Some(id) = primitive_id { runtime.release_root(id); }
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
                    .min(crate::modules::net_shared_vm::poll_interval())
                    .min(crate::modules::child_process_shared_vm::poll_interval());
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
        let _check_span =
            crate::modules::trace_state::EnvironmentSpan::enter(&shared_state, "CheckImmediate");
        let mut ran_immediate = run_and_clear_native_immediates(runtime, program, &shared_state)?;
        let cutoff = shared_state
            .borrow()
            .scheduler
            .shared_immediate_cutoff()
            .unwrap_or(cutoff);

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
                    .min(crate::modules::net_shared_vm::poll_interval())
                    .min(crate::modules::child_process_shared_vm::poll_interval());
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

fn run_and_clear_native_immediates(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    state: &Rc<RefCell<crate::host::SharedNodeState>>,
) -> Result<bool, String> {
    let _span =
        crate::modules::trace_state::EnvironmentSpan::enter(state, "RunAndClearNativeImmediates");
    let mut ran = false;
    loop {
        let immediate = state.borrow_mut().scheduler.take_shared_native_immediate();
        let Some(immediate) = immediate else {
            return Ok(ran);
        };
        ran = true;
        if let Err(error) = invoke_async_callback(runtime, program, immediate.callback)
            .and_then(|()| drain_shared_jobs(runtime, program, state))
        {
            release_pending_shared_immediates(runtime, state);
            return Err(error);
        }
    }
}

fn has_referenced_shared_work(shared_state: &Rc<RefCell<crate::host::SharedNodeState>>) -> bool {
    crate::modules::child_process_ipc_shared_vm::has_work(&shared_state.borrow())
        || crate::modules::process_stdin_shared_vm::has_work(&shared_state.borrow().stdin)
        || crate::modules::fetch_shared_vm::has_pending(shared_state)
        || crate::modules::child_process_shared_vm::has_pending(shared_state)
        || crate::modules::http_shared_vm::has_work(shared_state)
        || super::fs_watch_shared_vm::has_work(&shared_state.borrow().fs_watch)
        || super::fs_stat_watch_shared_vm::has_work(&shared_state.borrow().fs_stat_watch)
        || shared_state.borrow().scheduler.has_refed_shared_timers()
        || shared_state
            .borrow()
            .scheduler
            .has_refed_shared_immediates()
}

fn mark_timer_destroyed(runtime: &mut quench_runtime::Runtime<NodeHost>, handle: RootId) -> Result<(), RootedError> {
    let key = runtime.string_rooted(crate::modules::timers_shared_vm::TIMER_DESTROYED);
    let destroyed = runtime.root(Value::TRUE);
    let result = runtime.set_property_rooted(handle, key, destroyed, handle);
    runtime.release_root(key);
    runtime.release_root(destroyed);
    result.map(|_| ())
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
            mark_timer_destroyed(runtime, handle).map_err(|error| runtime.format_error(program, &error.error))?;
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
    match call_shared_callback(runtime, callback) {
        Ok(()) => Ok(()),
        Err(failure) => route_callback_failure(runtime, program, failure),
    }
}

fn invoke_async_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callback: crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), String> {
    let result = call_shared_callback(runtime, &callback);
    release_callback(runtime, callback);
    match result {
        Ok(()) => Ok(()),
        Err(failure) => route_callback_failure(runtime, program, failure),
    }
}

fn call_shared_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    callback: &crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), RootedError> {
    match runtime.call_rooted(callback.callback, callback.receiver, &callback.args) {
        Ok(result) => {
            runtime.release_root(result);
            Ok(())
        }
        Err(mut error) => {
            if error.exception.is_none() {
                error.exception = error.error.thrown_value().map(|value| runtime.root(value));
            }
            Err(error)
        }
    }
}

pub(super) fn route_callback_failure(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    mut failure: RootedError,
) -> Result<(), String> {
    if runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .requested_exit_code()
        .is_some()
    {
        if let Some(exception) = failure.exception {
            runtime.release_root(exception);
        }
        return Err("process.exit".to_owned());
    }
    let Some(exception) = failure.exception else {
        return Err(runtime.format_rooted_error(program, &mut failure));
    };
    let routed = route_uncaught_exception_value(runtime, program, exception, "uncaughtException");
    let result = match routed {
        Ok(true) => Ok(()),
        Ok(false) => Err(format_uncaught_error(runtime, program, &mut failure)),
        Err(handler_error) => Err(handler_error),
    };
    runtime.release_root(exception);
    result
}

/// Build fatal output only after exception handlers decline it. Source text is
/// the compiler snapshot; a guest can rewrite or remove its file after loading.
pub(crate) fn format_uncaught_error(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    error: &mut RootedError,
) -> String {
    let stack = error.exception.and_then(|exception| {
        if !runtime.is_error_rooted(exception).ok()? {
            return None;
        }
        let key = runtime.string_rooted("stack");
        let value = runtime.get_property_rooted(exception, key);
        runtime.release_root(key);
        match value {
            Ok(value) => {
                let text = runtime.string_text_rooted(value).ok().flatten();
                runtime.release_root(value);
                text
            }
            Err(error) => {
                if let Some(exception) = error.exception {
                    runtime.release_root(exception);
                }
                None
            }
        }
    });
    let body = stack.unwrap_or_else(|| runtime.format_rooted_error(program, error));
    let mut output = String::new();
    if let Some((file, line, column, source)) = error.error.source_location() {
        output.push_str(&format!("{file}:{line}\n"));
        if let Some(source) = source {
            output.push_str(source);
            output.push('\n');
            let mut remaining = column.saturating_sub(1) as usize;
            for character in source.chars() {
                if remaining == 0 {
                    break;
                }
                if character == '\t' {
                    output.push('\t');
                } else {
                    output.extend(std::iter::repeat_n(
                        ' ',
                        character.len_utf16().min(remaining),
                    ));
                }
                remaining = remaining.saturating_sub(character.len_utf16());
            }
            output.push_str("^\n");
        }
        output.push('\n');
    }
    output.push_str(&body);
    output.push_str(&format!("\n\nNode.js v{}", process_state::NODE_VERSION));
    output
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
            runtime.with_native_stack_frame("process.processTicksAndRejections", |runtime| {
                invoke_async_callback(runtime, program, callback)
            })?;
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
            runtime.clear_kept_objects();
            crate::modules::zlib_stream_shared_vm::prune(runtime);
            super::fs_watch_shared_vm::prune(runtime);
            super::tls_shared_vm::contexts::prune(runtime);
            super::net_reference_shared_vm::prune(runtime);
            super::fs_stat_watch_shared_vm::prune(runtime);
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
        let Some(value) = error.thrown_value() else {
            return Err(runtime.format_error(program, &error));
        };
        let exception = runtime.root(value);
        let mut failure = RootedError {
            error,
            exception: Some(exception),
        };
        let routed =
            route_uncaught_exception_value(runtime, program, exception, "uncaughtException");
        let result = match routed {
            Ok(true) => Ok(()),
            Ok(false) => Err(format_uncaught_error(runtime, program, &mut failure)),
            Err(handler_error) => Err(handler_error),
        };
        runtime.release_root(exception);
        result?;
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
            route_unhandled_rejection(runtime, program, reason)?;
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
    let routed = route_uncaught_exception_value(runtime, program, exception, "unhandledRejection");
    let result = match routed {
        Ok(true) => Ok(()),
        Ok(false) => terminate_unhandled_rejection(runtime, program, exception),
        Err(handler_error) => Err(handler_error),
    };
    if let Some(exception) = owned_exception {
        runtime.release_root(exception);
    }
    result
}

fn terminate_unhandled_rejection(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    exception: RootId,
) -> Result<(), String> {
    let control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    control.set_exit_code(Some(1));
    // The caller owns the exception until fatal formatting has finished.
    let mut failure = RootedError::host("unhandled Promise rejection");
    failure.exception = Some(exception);
    Err(format_uncaught_error(runtime, program, &mut failure))
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
        let exception = runtime
            .error_rooted(&message)
            .map_err(|error| rooted_error_message(runtime, program, error))?;
        temporary_roots.push(exception);
        set_rooted_text_property(
            runtime,
            program,
            exception,
            "code",
            "ERR_UNHANDLED_REJECTION",
        )?;
        set_rooted_text_property(
            runtime,
            program,
            exception,
            "name",
            "UnhandledPromiseRejection",
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

fn cleanup_environment(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    state: &Rc<RefCell<crate::host::SharedNodeState>>,
) {
    let _span = crate::modules::trace_state::EnvironmentSpan::enter(state, "RunCleanup");
    crate::modules::http_shared_vm::cleanup(runtime, state);
    super::fs_watch_shared_vm::cleanup(runtime);
    super::fs_stat_watch_shared_vm::cleanup(runtime);
}

// Builtin native objects remain usable throughout synchronous exit listeners.
fn emit_exit(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    code: i32,
) -> Result<(), String> {
    let result = emit_exit_inner(runtime, program, code);
    super::tls_shared_vm::contexts::cleanup(runtime);
    super::buffer_shared_vm::cleanup(runtime);
    result
}

fn emit_exit_inner(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    code: i32,
) -> Result<(), String> {
    let shared_state = runtime.host_mut().shared_state();
    let _span = crate::modules::trace_state::EnvironmentSpan::enter(&shared_state, "AtExit");
    shared_state.borrow().process_control.begin_exit_emission();
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
        unwatch_process_signal(context, Some(&event));
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
    unwatch_process_signal(context, event.as_ref());
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
    if let crate::modules::shared_event_loop::SharedEventKey::String(name) = &event {
        let result = context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .process_signals
            .watch(name);
        if let Err(error) = result {
            return Err(signal_start_error(context, error)?);
        }
    }
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
                unwatch_process_signal(context, Some(&event));
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
    let undefined = context.undefined();
    let payload = args.first().copied().unwrap_or(undefined);
    let options = args.get(1).copied().unwrap_or(undefined);
    let mut kind = options;
    let mut code = args.get(2).copied().unwrap_or(undefined);
    let mut detail = None;
    let mut host_stack = None;
    let mut rejection_id = None;
    let global = context.global_root()?;
    let process_key = context.string_rooted("process");
    let process = context.get_property_rooted(global, process_key)?;
    let no_deprecation_key = context.string_rooted("noDeprecation");
    let no_deprecation = context.get_property_rooted(process, no_deprecation_key)?;
    if context.truthy_rooted(no_deprecation)?
        && context.string_text(kind)?.as_deref() == Some("DeprecationWarning")
    {
        return Ok(undefined);
    }
    if context.is_object_rooted(options)?
        && !context.is_array_rooted(options)?
        && !context.is_callable_rooted(options)?
    {
        // Keep the upstream property access order, including the ignored stack
        // constructor option. Host stack/id options are private rejection plumbing.
        let ctor_key = context.string_rooted("ctor");
        context.get_property_rooted(options, ctor_key)?;
        let code_key = context.string_rooted("code");
        code = context.get_property_rooted(options, code_key)?;
        let detail_key = context.string_rooted("detail");
        let detail_value = context.get_property_rooted(options, detail_key)?;
        if context.string_text(detail_value)?.is_some() {
            // Upstream reads the detail getter again after checking its type.
            let detail_value = context.get_property_rooted(options, detail_key)?;
            detail = Some(detail_value);
        }
        let kind_key = context.string_rooted("type");
        kind = context.get_property_rooted(options, kind_key)?;
        if !context.truthy_rooted(kind)? {
            kind = context.string_rooted("Warning");
        }
        host_stack = string_property(context, options, HOST_WARNING_STACK_OPTION)?;
        rejection_id = number_property(context, options, HOST_WARNING_ID_OPTION)?
            .filter(|id| id.is_finite() && *id >= 1.0 && id.fract() == 0.0);
    } else if context.is_callable_rooted(options)? {
        kind = context.string_rooted("Warning");
        code = undefined;
    }
    if !context
        .rooted_value(kind)
        .is_some_and(|value| value.is_undefined())
        && context.string_text(kind)?.is_none()
    {
        return Err(super::util_shared_vm::argument_type_error(
            context, kind, "type", "string",
        )?);
    }
    if context.is_callable_rooted(code)? {
        code = undefined;
    } else if !context
        .rooted_value(code)
        .is_some_and(|value| value.is_undefined())
        && context.string_text(code)?.is_none()
    {
        return Err(super::util_shared_vm::argument_type_error(
            context, code, "code", "string",
        )?);
    }
    let warning = if let Some(message) = context.string_text(payload)? {
        let warning = context.error_rooted(&message)?;
        let name = context
            .string_text(kind)?
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Warning".into());
        let name = context.string_rooted(&name);
        install(context, warning, "name", name)?;
        if !context
            .rooted_value(code)
            .is_some_and(|value| value.is_undefined())
        {
            install(context, warning, "code", code)?;
        }
        if let Some(detail) = detail {
            install(context, warning, "detail", detail)?;
        }
        if let Some(stack) = host_stack {
            let stack = context.string_rooted(&stack);
            install(context, warning, "stack", stack)?;
        }
        if let Some(id) = rejection_id {
            let id = context.number(id);
            install(context, warning, "id", id)?;
        }
        warning
    } else {
        // The syntax bridge performs ordinary instanceof, including public
        // Symbol.hasInstance and Error prototype changes; policy stays native.
        let check = context.host_function_data()?;
        let result = context.call_rooted(check, undefined, &[payload])?;
        if !context.truthy_rooted(result)? {
            return Err(super::util_shared_vm::argument_type_error_with_expected(
                context,
                payload,
                "warning",
                "of type string or an instance of Error",
            )?);
        }
        payload
    };
    if string_property(context, warning, "name")?.as_deref() == Some("DeprecationWarning") {
        let no_deprecation = context.get_property_rooted(process, no_deprecation_key)?;
        if context.truthy_rooted(no_deprecation)? {
            return Ok(undefined);
        }
        let throw_key = context.string_rooted("throwDeprecation");
        let should_throw = context.get_property_rooted(process, throw_key)?;
        if context.truthy_rooted(should_throw)? {
            let callback =
                context.host_function(crate::host::shared_vm::operation("processThrowWarning"))?;
            super::shared_event_loop::queue_next_tick(context, callback, undefined, &[warning])?;
            return Ok(undefined);
        }
    }
    queue_warning(context, warning)?;
    Ok(undefined)
}

pub(crate) fn throw_warning(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let warning = args.first().copied().unwrap_or_else(|| context.undefined());
    Err(context.throw(warning))
}

fn queue_warning(
    context: &mut NativeContext<'_, NodeHost>,
    warning: RootId,
) -> Result<(), RootedError> {
    let callback =
        context.host_function(crate::host::shared_vm::operation("processDispatchWarning"))?;
    let undefined = context.undefined();
    crate::modules::shared_event_loop::queue_next_tick(context, callback, undefined, &[warning])
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

fn unwatch_process_signal(
    context: &mut NativeContext<'_, NodeHost>,
    event: Option<&crate::modules::shared_event_loop::SharedEventKey>,
) {
    let name = match event {
        Some(crate::modules::shared_event_loop::SharedEventKey::String(name)) => {
            Some(name.as_str())
        }
        Some(_) => return,
        None => None,
    };
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .process_signals
        .unwatch(name);
}

fn signal_start_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: std::io::Error,
) -> Result<RootedError, RootedError> {
    let Some(errno) = error.raw_os_error() else {
        return Ok(RootedError::host(error.to_string()));
    };
    let code = super::util_system_error_shared_vm::system_error_name(-f64::from(errno));
    let error = context.error_rooted(&format!("uv_signal_start {code}"))?;
    let syscall = context.string_rooted("uv_signal_start");
    install(context, error, "syscall", syscall)?;
    let number = context.number(-f64::from(errno));
    install(context, error, "errno", number)?;
    Ok(throw_with_code(context, error, &code))
}

fn poll_process_signals(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    state: &Rc<RefCell<crate::host::SharedNodeState>>,
) -> Result<bool, String> {
    let pending = state
        .borrow_mut()
        .process_signals
        .take_pending()
        .map_err(|error| error.to_string())?;
    if pending.is_empty() {
        return Ok(false);
    }
    let Some(process) = shared_process_root(runtime) else {
        return Ok(false);
    };
    for name in pending {
        let callback = get_rooted_property(runtime, program, process, "emit")?;
        let receiver = runtime.root(
            runtime
                .rooted_value(process)
                .ok_or("process root is unavailable")?,
        );
        let event = runtime.string_rooted(&name);
        let signal = runtime.string_rooted(&name);
        state.borrow_mut().scheduler.queue_shared_next_tick(
            crate::modules::shared_event_loop::SharedCallback {
                callback,
                receiver,
                args: vec![event, signal],
            },
        );
    }
    Ok(true)
}

/// Node's public kill overloads and mutable _kill delegation. PID validation
/// follows the shared bitwise/abstract equality operations, including hooks.
pub(crate) fn kill(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let key = context.string_rooted("0");
    let process = context.get_property_rooted(data, key)?;
    let key = context.string_rooted("1");
    let signals = context.get_property_rooted(data, key)?;
    let undefined = context.undefined();
    let pid = args.first().copied().unwrap_or(undefined);
    let zero = context.number(0.0);
    let integer = context.bitwise_or_rooted(pid, zero)?;
    if !context.equal_rooted(pid, integer)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, pid, "pid", "number",
        )?);
    }
    let signal = args.get(1).copied().unwrap_or(undefined);
    let integer_signal = context.bitwise_or_rooted(signal, zero)?;
    enum KillSignal {
        Number(RootId),
        Name(RootId),
    }
    let signal = if context.strict_equal_rooted(signal, integer_signal)? {
        KillSignal::Number(signal)
    } else {
        let signal = if context.truthy_rooted(signal)? {
            signal
        } else {
            context.string_rooted("SIGTERM")
        };
        let number = context.get_property_rooted(signals, signal)?;
        if !context.truthy_rooted(number)? {
            let message = if context.is_object_rooted(signal)? {
                // `util` is built on this error path, not for every process start.
                let util = crate::host::shared_vm::commonjs::util_module(context)?;
                let key = context.string_rooted("inspect");
                let inspect = context.get_property_rooted(util, key)?;
                let options = context.null_object_rooted()?;
                let depth = context.number(0.0);
                install(context, options, "depth", depth)?;
                let text = context.call_rooted(inspect, undefined, &[signal, options])?;
                context.to_string(text)?
            } else {
                context.to_string(signal)?
            };
            let error = context.type_error_rooted(&format!("Unknown signal: {message}"))?;
            return Err(throw_with_code(context, error, "ERR_UNKNOWN_SIGNAL"));
        }
        KillSignal::Name(signal)
    };
    let key = context.string_rooted("_kill");
    let operation = context.get_property_rooted(process, key)?;
    let number = match signal {
        KillSignal::Number(number) => number,
        KillSignal::Name(name) => context.get_property_rooted(signals, name)?,
    };
    let returned = context.call_rooted(operation, process, &[pid, number])?;
    if context.truthy_rooted(returned)? {
        let errno = context
            .rooted_value(returned)
            .and_then(|value| value.as_number())
            .ok_or_else(|| RootedError::host("process._kill returned a non-numeric error"))?;
        let code = super::util_system_error_shared_vm::system_error_name(errno);
        let error = context.error_rooted(&format!("kill {code}"))?;
        let syscall = context.string_rooted("kill");
        install(context, error, "syscall", syscall)?;
        install(context, error, "errno", returned)?;
        return Err(throw_with_code(context, error, &code));
    }
    Ok(context.boolean(true))
}

pub(crate) fn getgroups(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(unix)]
    {
        let query = || {
            let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
            if count < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut groups = vec![0 as libc::gid_t; count as usize];
            let count = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
            if count < 0 {
                return Err(std::io::Error::last_os_error());
            }
            groups.truncate(count as usize);
            let effective = unsafe { libc::getegid() };
            if !groups.contains(&effective) {
                groups.push(effective);
            }
            Ok(groups)
        };
        match query() {
            Ok(groups) => {
                let values = groups
                    .into_iter()
                    .map(|group| context.number(f64::from(group)))
                    .collect::<Vec<_>>();
                context.array_rooted(&values)
            }
            Err(error) => {
                let errno = error.raw_os_error().unwrap_or(libc::EIO);
                let code = super::util_system_error_shared_vm::system_error_name(-f64::from(errno));
                let error = context.error_rooted(&format!("getgroups {code}"))?;
                let number = context.number(-f64::from(errno));
                install(context, error, "errno", number)?;
                let code = context.string_rooted(&code);
                install(context, error, "code", code)?;
                let syscall = context.string_rooted("getgroups");
                install(context, error, "syscall", syscall)?;
                Err(context.throw(error))
            }
        }
    }
    #[cfg(not(unix))]
    context.array_rooted(&[])
}
