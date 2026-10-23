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
const UNHANDLED_REJECTION_CLI_GUIDANCE: &str = "To terminate the node process on unhandled promise rejection, use the CLI flag `--unhandled-rejections=strict` (see https://nodejs.org/api/cli.html#cli_unhandled_rejections_mode).";

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .reset_shared();
    let process = context.object_rooted()?;
    let capture_state = context.object_rooted()?;
    let null = context.null();
    let auxiliary = context.array_rooted(&[])?;
    let report_flag = context.boolean(false);
    for (name, value) in [("captureFn", null), ("auxiliaryCallbacks", auxiliary), ("reportFlag", report_flag)] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(capture_state, key, value, true, true, true)?;
    }
    let capture_state = context.retain(capture_state)?;
    context.host_mut().shared_state().borrow_mut().process_capture_state = Some(capture_state);
    let function = context.host_function(crate::host::shared_vm::operation("uptime"))?;
    install(context, process, "uptime", function)?;
    install_exit_code(context, process)?;
    super::process_fatal_exception_route::install(context, process)?;
    super::report_configuration_shared_vm::install(context, process)?;
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
    // Property, native operation and the function name Node reports.
    for (name, operation, function_name) in [
        ("nextTick", "nextTick", "nextTick"),
        ("cwd", "processCwd", "wrappedCwd"),
        ("chdir", "processChdir", "wrappedChdir"),
        ("getuid", "processGetuid", "getuid"),
        ("geteuid", "processGeteuid", "geteuid"),
        ("getgid", "processGetgid", "getgid"),
        ("getegid", "processGetegid", "getegid"),
        ("umask", "processUmask", "wrappedUmask"),
        ("dlopen", "processDlopen", "dlopen"),
    ] {
        let operation = crate::host::shared_vm::operation(operation);
        let function = context.host_function_named(operation, function_name)?;
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
    for (name, operation) in [
        ("binding", "processBinding"),
        ("setUncaughtExceptionCaptureCallback", "processSetUncaughtExceptionCaptureCallback"),
        ("hasUncaughtExceptionCaptureCallback", "processHasUncaughtExceptionCaptureCallback"),
    ] {
        let function = context.host_constructor_named(crate::host::shared_vm::operation(operation), name)?;
        install(context, process, name, function)?;
    }
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
    let exiting = context.boolean(false);
    install(context, process, "_exiting", exiting)?;
    let shared_state = context.host_mut().shared_state();
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
    // Copied from the OS environment on the first read of `process.env`: most runs never read
    // it, and every variable costs a key and a string.
    let initializer = context.host_function(crate::host::shared_vm::operation("processEnvInitialize"))?;
    context.define_lazy_data_properties_rooted(process, &["env"], true, initializer)?;
    let load_env_file =
        context.host_function(crate::host::shared_vm::operation("processLoadEnvFile"))?;
    install(context, process, "loadEnvFile", load_env_file)?;
    install_identity(context, process)?;
    let version = format!("v{}", process_state::NODE_VERSION);
    let version = context.string_rooted(&version);
    let key = context.string_rooted("version");
    context.define_data_property_rooted(process, key, version, false, true, true)?;
    let global = context.global_root()?;
    let symbol_key = context.string_rooted("Symbol");
    let symbol = context.get_property_rooted(global, symbol_key)?;
    let tag_key = context.string_rooted("toStringTag");
    let tag = context.get_property_rooted(symbol, tag_key)?;
    let process_tag = context.string_rooted("process");
    if !context.set_property_rooted(process, tag, process_tag, process)? {
        return Err(RootedError::host("cannot set process toStringTag"));
    }
    context.scoped(|context| install_config(context, process))?;
    context.scoped(|context| {
        install_facts(context, process, "features", process_state::feature_facts())
    })?;
    context.scoped(|context| install_versions(context, process))?;
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

/// The process's arguments, executable and ids on `process`. A restored startup snapshot
/// (`Host::resume`) holds another process's, which these replace in place.
fn install_identity(context: &mut NativeContext<'_, NodeHost>, process: RootId) -> Result<(), RootedError> {
    let shared_state = context.host_mut().shared_state();
    let argv = shared_state.borrow().process_argv.clone();
    let exec_argv = shared_state.borrow().exec_argv.clone();
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
    Ok(())
}

/// `install_identity` on the installed `process` (`Host::resume`).
pub(crate) fn refresh_identity(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    install_identity(context, process)?;
    // The bootstrap's process streams describe the builder's descriptors.
    for (name, fd) in [("stdout", STDOUT_FD), ("stderr", STDERR_FD)] {
        let key = context.string_rooted(name);
        let stream = context.get_property_rooted(process, key)?;
        install_terminal(context, stream, fd)?;
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

/// Whether the stream's descriptor is a terminal, as `isTTY`.
fn install_terminal(context: &mut NativeContext<'_, NodeHost>, stream: RootId, fd: i32) -> Result<(), RootedError> {
    let tty = context.boolean(crate::modules::tty_state::is_terminal_fd(fd));
    install(context, stream, "isTTY", tty)
}

fn create_stream(
    context: &mut NativeContext<'_, NodeHost>,
    fd: i32,
) -> Result<RootId, RootedError> {
    let stream = context.object_rooted()?;
    let writable = context.boolean(true);
    install(context, stream, "writable", writable)?;
    install(context, stream, "_isStdio", writable)?;
    install_terminal(context, stream, fd)?;
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
        install_frozen(context, variables, name, value)?;
    }
    install_frozen(context, config, "variables", variables)?;
    for object in [variables, config] {
        if !context.prevent_extensions_rooted(object)? {
            return Err(RootedError::host("cannot freeze process.config"));
        }
    }
    install(context, process, "config", config)
}

/// Defines `name` as `Object.freeze` leaves a data property: enumerable, neither writable
/// nor configurable. With `prevent_extensions_rooted` after the last one, the object is
/// frozen without compiling a script to call `Object.freeze` (6 KiB of start-up malloc).
fn install_frozen(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.define_data_property_rooted(object, key, value, false, true, false)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install frozen process.config member {name}")))
    }
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
        let key = context.string_rooted(name);
        context.define_data_property_rooted(versions, key, value, false, true, true)?;
    }
    let key = context.string_rooted("versions");
    context.define_data_property_rooted(process, key, versions, false, true, true)?;
    Ok(())
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
            let number = context.to_number(value_root)?;
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

// Rust translation of original per_thread.js:exit. Property reads/calls stay
// observable: callers may replace emit/reallyExit or the public _exiting flag.
pub(crate) fn exit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    if let Some(code) = args.first() {
        set_exit_property(context, process, "exitCode", *code)?;
    }
    let key = context.string_rooted("_exiting");
    let exiting = context.get_property_rooted(process, key)?;
    if !context.truthy_rooted(exiting)? {
        let code = public_exit_code(context, process)?;
        emit_exit_event_inner(context, code, ExitEventOrigin::Explicit)?;
    }
    let code = public_exit_code(context, process)?;
    call_process_method(context, process, "reallyExit", &[code])?;
    Ok(context.undefined())
}
fn public_exit_code(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted("exitCode");
    let code = context.get_property_rooted(process, key)?;
    if context.truthy_rooted(code)? {
        Ok(code)
    } else {
        Ok(context.number(0.0))
    }
}
fn set_exit_property(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(process, key, value, process)? {
        return Ok(());
    }
    let error = context.type_error_rooted(&format!(
        "Cannot assign to read only property '{name}' of object '#<process>'"
    ))?;
    Err(context.throw(error))
}
fn call_process_method(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    let function = context.get_property_rooted(process, key)?;
    if !context.is_callable_rooted(function)? {
        let error = context.type_error_rooted(&format!("process.{name} is not a function"))?;
        return Err(context.throw(error));
    }
    context.call_rooted(function, process, args)
}
#[derive(Clone, Copy)]
enum ExitEventOrigin {
    Explicit,
    Natural,
}
/// One event algorithm, with distinct explicit and natural emission stages.
/// A replaced reallyExit or a caught listener throw leaves the event loop alive;
/// Node emits again at eventual natural shutdown in those cases.
pub(crate) fn emit_exit_event(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let code = args.first().copied().unwrap_or_else(|| context.number(0.0));
    emit_exit_event_inner(context, code, ExitEventOrigin::Natural)
}
fn emit_exit_event_inner(
    context: &mut NativeContext<'_, NodeHost>,
    code: RootId,
    origin: ExitEventOrigin,
) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    let _span = crate::modules::trace_state::EnvironmentSpan::enter(&state, "AtExit");
    if matches!(origin, ExitEventOrigin::Natural)
        && !state.borrow_mut().scheduler.begin_shared_exit()
    {
        return Ok(context.undefined());
    }
    let Some(process) = state.borrow().process_module else {
        return Ok(context.undefined());
    };
    let exiting = context.boolean(true);
    set_exit_property(context, process, "_exiting", exiting)?;
    let event = context.string_rooted("exit");
    call_process_method(context, process, "emit", &[event, code])?;
    Ok(context.undefined())
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
        Ok(value) => context
            .rooted_value(value)
            .and_then(Value::as_number)
            .unwrap_or(0.0) as i32,
        Err(error) => {
            if error.error.is_execution_terminated() {
                return Err(error);
            }
            if let Some(exception) = error.exception {
                context.release_root(exception);
            }
            0
        }
    };
    context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .request_really_exit(code);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .begin_shared_exit();
    context.execution_interrupt_handle().request_termination();
    context.check_execution_interrupt()?;
    Ok(context.undefined())
}

fn exit_code_error(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let codes = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "internal/process/per_thread.js",
        "internal/errors.js",
        "codes",
    )?;
    let key = context.string_rooted(name);
    let constructor = context.get_property_rooted(codes, key)?;
    let error = context.construct_rooted(constructor, constructor, args)?;
    Err(context.throw(error))
}
fn invalid_exit_code_type(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    let code = context.string_rooted("code");
    let number = context.string_rooted("number");
    exit_code_error(context, "ERR_INVALID_ARG_TYPE", &[code, number, value])
}
fn exit_code_range_error(
    context: &mut NativeContext<'_, NodeHost>,
    number: f64,
) -> Result<RootId, RootedError> {
    let code = context.string_rooted("code");
    let range = if number.is_finite() && number.fract() == 0.0 {
        format!(">= -{MAX_SAFE_EXIT_CODE:.0} && <= {MAX_SAFE_EXIT_CODE:.0}")
    } else {
        "an integer".to_owned()
    };
    let range = context.string_rooted(&range);
    let value = context.number(number);
    exit_code_error(context, "ERR_OUT_OF_RANGE", &[code, range, value])
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
    // Writable.write(chunk[, encoding][, callback]).
    let (encoding, callback) = match args.get(1).copied() {
        Some(value) if context.is_callable_rooted(value)? => (None, Some(value)),
        encoding => (encoding, args.get(2).copied()),
    };
    // A falsy encoding selects the stream's default.
    let encoding = match encoding {
        Some(value) if context.truthy_rooted(value)? => Some(value),
        _ => None,
    };
    let callback = match callback {
        Some(value) if context.is_callable_rooted(value)? => Some(value),
        _ => None,
    };
    let bytes = if context.string_text(chunk)?.is_some() {
        crate::modules::buffer_shared_vm::encode_bytes(context, chunk, encoding)?
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
    let worker = context.host_mut().shared_state().borrow().messaging.context.as_ref().map(|worker| worker.control.clone());
    if let Some(control) = worker {
        // A worker's stdio reaches its parent's `worker.stdout`/`worker.stderr`.
        control.write_output(fd == STDERR_FD, bytes);
    } else if fd == STDERR_FD {
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
    // Stdio writes complete synchronously; Writable reports that on the next tick.
    if let Some(callback) = callback {
        let undefined = context.undefined();
        let null = context.null();
        super::shared_event_loop::queue_next_tick(context, callback, undefined, &[null])?;
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
    crate::host::startup_snapshot::refuse_process_state(context, "process.cwd()")?;
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
    let error = errno_exception(context, errno, syscall, None)?;
    Ok(context.throw(error))
}

/// Node's C++ `ErrnoException`: `"<code>, <strerror>"`, then ` '<path>'` when a path is
/// given, with `errno` (positive), `code`, `path` and `syscall` set in that order.
pub(crate) fn errno_exception(
    context: &mut NativeContext<'_, NodeHost>,
    errno: i32,
    syscall: &str,
    path: Option<&str>,
) -> Result<RootId, RootedError> {
    let code = super::util_system_error_shared_vm::system_error_code(-f64::from(errno))
        .unwrap_or("UNKNOWN");
    // libc's message is copied before any other host operation can overwrite it.
    let description = unsafe {
        std::ffi::CStr::from_ptr(libc::strerror(errno))
            .to_string_lossy()
            .into_owned()
    };
    let message = match path {
        Some(path) => format!("{code}, {description} '{path}'"),
        None => format!("{code}, {description}"),
    };
    let error = context.error_rooted(&message)?;
    let errno = context.number(f64::from(errno));
    install(context, error, "errno", errno)?;
    set_text(context, error, "code", code)?;
    if let Some(path) = path {
        set_text(context, error, "path", path)?;
    }
    set_text(context, error, "syscall", syscall)?;
    Ok(error)
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

fn capture_state(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.host_mut().shared_state().borrow().process_capture_state
        .ok_or_else(|| RootedError::host("process capture state is not initialized"))
}

pub(crate) fn set_uncaught_exception_capture_callback(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return Err(uncaught_capture_type_error(context, None)?);
    };
    let is_null = context.rooted_value(callback).is_some_and(Value::is_null);
    if !is_null && !context.is_callable_rooted(callback)? {
        return Err(uncaught_capture_type_error(context, Some(callback))?);
    }
    let state = capture_state(context)?;
    if !is_null {
        let current_key = context.string_rooted("captureFn");
        let current = context.get_property_rooted(state, current_key)?;
        if context.rooted_value(current).is_some_and(|value| !value.is_null()) {
            let error = context.error_rooted(
                "`process.setupUncaughtExceptionCapture()` was called while a capture callback was already active",
            )?;
            super::node_errors_shared_vm::decorate_error(context, error, "ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET")?;
            return Err(context.throw(error));
        }
    }
    context.assign_named_property_rooted(state, "captureFn", callback)?;
    let process = context.host_mut().shared_state().borrow().process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    use super::event_constructor_shared_vm::get;
    if is_null {
        let auxiliary = get(context, state, "auxiliaryCallbacks")?;
        let length = get(context, auxiliary, "length")?;
        let zero = context.number(0.0);
        if context.strict_equal_rooted(length, zero)? {
            let report = get(context, process, "report")?;
            let flag = get(context, state, "reportFlag")?;
            context.assign_named_property_rooted(report, "reportOnUncaughtException", flag)?;
        }
    } else {
        let report = get(context, process, "report")?;
        let flag = get(context, report, "reportOnUncaughtException")?;
        let yes = context.boolean(true);
        let enabled = context.strict_equal_rooted(flag, yes)?;
        let enabled = context.boolean(enabled);
        context.assign_named_property_rooted(state, "reportFlag", enabled)?;
        let report = get(context, process, "report")?;
        let no = context.boolean(false);
        context.assign_named_property_rooted(report, "reportOnUncaughtException", no)?;
    }
    Ok(context.undefined())
}

pub(crate) fn has_uncaught_exception_capture_callback(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = capture_state(context)?;
    let key = context.string_rooted("captureFn");
    let callback = context.get_property_rooted(state, key)?;
    let has_callback = context.rooted_value(callback).is_some_and(|value| !value.is_null());
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
    let value = value.unwrap_or_else(|| context.undefined());
    super::util_shared_vm::argument_type_error(context, value, "fn", "function or null")
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
    // The run is over: what it touched of the code is not needed for exit, whose own code
    // would otherwise add its pages on top of the run's (a small script's peak).
    crate::host::resident_pages::release_read_only_pages();
    let process_control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    // Top-level execution has returned. Native polling and process lifecycle
    // events run outside that JavaScript resource; queued jobs restore their
    // own captured identity and stores while they execute.
    let native_state = runtime.host_mut().shared_state();
    let _native_context = super::async_hooks_shared_vm::enter_context(
        &native_state, super::async_hooks_state::NATIVE_ASYNC_ID,
    );
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
        let exit = finish_requested_exit(runtime, program);
        return if process_control.fatal_exception_handler_failed() || process_control.fatal_uncaught_requested() { checkpoint.and(exit) } else { exit };
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
    let callback = crate::modules::shared_event_loop::SharedCallback { execution: None,
        callback: operation,
        receiver: process,
        args: vec![event, code],
    };
    let result = call_shared_callback(runtime, program, &callback);
    runtime.release_root(operation);
    runtime.release_root(event);
    result
}

// Match execution.js fatal policy: an exception while already exiting does
// not overwrite the chosen code or emit another fatal exit event.
pub(crate) fn finish_after_uncaught_error(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    error: &quench_runtime::JsError,
) -> Result<bool, String> {
    let control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    let routed = emit_uncaught_exception(runtime, program, error);
    if control.requested_exit_code().is_some() {
        let exit = finish_requested_exit(runtime, program);
        if super::worker_threads_shared_vm::runtime_is_child(runtime) {
            exit?;
            return Ok(true);
        }
        if control.fatal_uncaught_requested() { return routed.and(exit.map(|_| false)); }
        if control.fatal_exception_handler_failed() {
            return routed.and(exit).map(|_| true);
        }
        if let Err(error) = exit {
            control.fail_fatal_exception_handler();
            return Err(error);
        }
        return Ok(true);
    }
    if matches!(&routed, Ok(true)) {
        finish_execution(runtime, program)?;
        return Ok(true);
    }
    let handler_error = routed.err();
    let state = runtime.host_mut().shared_state();
    cleanup_environment(runtime, &state);
    state.borrow_mut().scheduler.begin_shared_exit();
    cleanup_after_exit(runtime);
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
        cleanup_after_exit(runtime);
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
    let operation = runtime.host_function(crate::host::shared_vm::operation("processDispatchFatalException"))
        .map_err(|error| rooted_error_message(runtime, program, error))?;
    let receiver = runtime.root(Value::UNDEFINED);
    let from_promise = runtime.root(if origin == "unhandledRejection" { Value::TRUE } else { Value::FALSE });
    let result = runtime.call_rooted(operation, receiver, &[exception, from_promise]);
    runtime.release_root(operation);
    runtime.release_root(receiver);
    runtime.release_root(from_promise);
    let result = result.map_err(|error| rooted_error_message(runtime, program, error))?;
    let handled = runtime.rooted_value(result).and_then(Value::as_bool) == Some(true);
    runtime.release_root(result);
    Ok(handled)
}

fn drain_checkpoint(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<(), String> {
    let shared_state = runtime.host_mut().shared_state();
    loop {
        drain_shared_jobs(runtime, program, &shared_state)?;
        if crate::modules::worker_threads_shared_vm::poll(runtime,program)? { continue; }
        if crate::modules::process_stdin_shared_vm::poll(runtime)? {
            continue;
        }
        if poll_process_signals(runtime, program, &shared_state)? {
            continue;
        }

        if crate::modules::pipe_binding::poll(runtime, program)? { continue; }
        if crate::modules::child_process_ipc_shared_vm::poll(runtime, program)? {
            continue;
        }
        if crate::modules::child_process_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }
        #[cfg(quench_node_cares)]
        if crate::modules::cares_channel_shared_vm::poll(runtime, program)? {
            continue;
        }
        if crate::modules::cares_binding::poll(runtime, program)? {
            continue;
        }
        if crate::modules::fetch_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }
        let udp_progress=crate::modules::udp_binding::poll(runtime,&shared_state)?;
        let http_progress = crate::modules::http_shared_vm::poll(runtime, program, &shared_state)?;
        // Each turn of the loop is a collection safepoint, once its network writes have gone
        // out: its callbacks and jobs may reach no back edge (a promise-driven stream), so a
        // due collection would otherwise wait for guest code that loops. Nothing runs here,
        // and every value is a root.
        runtime.collect_if_due(program).map_err(|error| error.to_string())?;
        if http_progress || udp_progress {
            continue;
        }
        if crate::modules::napi::poll(runtime, program)? {
            continue;
        }
        if crate::modules::worker_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }
        #[cfg(not(quench_node_cares))]
        if crate::modules::dns_shared_vm::poll(runtime, program, &shared_state)? {
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
                #[cfg(quench_node_cares)]
                let poll_interval = shared_state.borrow().cares.next_timeout().map_or(poll_interval, |timeout| poll_interval.min(timeout));
                let wait = next_timer
                    .map(|due| due.saturating_duration_since(std::time::Instant::now()))
                    .map_or(poll_interval, |delay| delay.min(poll_interval));
                if !wait.is_zero() {
                    crate::modules::net_shared_vm::wait_ready(&shared_state.borrow().tcp, wait);
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
                #[cfg(quench_node_cares)]
                let poll_interval = shared_state.borrow().cares.next_timeout().map_or(poll_interval, |timeout| poll_interval.min(timeout));
                let wait = next_timer
                    .map(|due| due.saturating_duration_since(std::time::Instant::now()))
                    .map_or(poll_interval, |delay| delay.min(poll_interval));
                if !wait.is_zero() {
                    crate::modules::net_shared_vm::wait_ready(&shared_state.borrow().tcp, wait);
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
    if crate::modules::worker_threads_shared_vm::has_work(&shared_state.borrow().workers) { return true; }
    #[cfg(quench_node_cares)]
    if shared_state.borrow().cares.has_work() { return true; }
    #[cfg(not(quench_node_cares))]
    if crate::modules::dns_shared_vm::has_work(shared_state) { return true; }
    crate::modules::pipe_binding::has_work(&shared_state.borrow())
        || shared_state.borrow().udp.has_work() || shared_state.borrow().dns.has_work()
        || crate::modules::child_process_ipc_shared_vm::has_work(&shared_state.borrow())
        || crate::modules::process_stdin_shared_vm::has_work(&shared_state.borrow().stdin)
        || crate::modules::fetch_shared_vm::has_pending(shared_state)
        || crate::modules::child_process_shared_vm::has_pending(shared_state)
        || crate::modules::http_shared_vm::has_work(shared_state)
        || crate::modules::napi::has_work()
        || crate::modules::worker_shared_vm::has_work(shared_state)
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
    call_shared_callback(runtime, program, callback)
}

pub(super) fn invoke_async_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callback: crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), String> {
    let result = call_shared_callback(runtime, program, &callback);
    release_callback(runtime, callback);
    result
}

fn call_shared_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callback: &crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), String> {
    let resolved = if let Some(key)=callback.execution.as_ref().and_then(|e|e.optional_callback) {
        let resolver=runtime.host_function(crate::host::shared_vm::operation("nativeOptionalCallbackSlot")).map_err(|e|e.to_string())?;
        let result=runtime.call_rooted(resolver,callback.receiver,&[key]);
        runtime.release_root(resolver);
        match result { Ok(value)=>{
            if runtime.rooted_value(value).is_some_and(|v|v.is_undefined()) {runtime.release_root(value);return Ok(())} Some(value)
        },Err(error)=>return route_callback_failure(runtime,program,error) }
    }else{None};
    let actual_callback=resolved.unwrap_or(callback.callback);
    let shared = runtime.host_mut().shared_state();
    let scope = callback.execution.as_ref().map(|execution|
        super::async_hooks_shared_vm::enter_resource_context(&shared, execution.id, execution.resource));
    let before = match &callback.execution {
        Some(execution) if execution.id != super::domain_trampoline::LEGACY_CALLBACK_ASYNC_ID => super::async_hook_registry_shared_vm::emit_runtime(runtime, super::async_hook_registry_shared_vm::HookKind::Before, execution.id),
        Some(_) | None => Ok(()),
    };
    let called = before.and_then(|()| {
        if callback.execution.as_ref().is_some_and(|execution| execution.id == super::domain_trampoline::LEGACY_CALLBACK_ASYNC_ID) {
            let dispatch = runtime.host_function(crate::host::shared_vm::operation("domainTrampolineCall"))?;
            let mut arguments = Vec::with_capacity(callback.args.len() + 1);
            arguments.push(actual_callback);
            arguments.extend_from_slice(&callback.args);
            let result = runtime.call_rooted(dispatch, callback.receiver, &arguments);
            runtime.release_root(dispatch);
            result
        } else {
            runtime.call_rooted(actual_callback, callback.receiver, &callback.args)
        }
    });
    let result = match called {
        Ok(value) => { runtime.release_root(value); Ok(()) }
        Err(mut error) => {
            if error.exception.is_none() {
                error.exception = error.error.thrown_value().map(|value| runtime.root(value));
            }
            // Node's uncaught handler observes the failing resource. Only
            // after that handler completes does the after hook/pop run.
            route_callback_failure(runtime, program, error)
        }
    };
    let result = result.and_then(|()| match &callback.execution {
        Some(execution) if execution.id != super::domain_trampoline::LEGACY_CALLBACK_ASYNC_ID => match super::async_hook_registry_shared_vm::emit_runtime(runtime, super::async_hook_registry_shared_vm::HookKind::After, execution.id) {
            Ok(()) => Ok(()),
            Err(error) => route_callback_failure(runtime, program, error),
        },
        Some(_) | None => Ok(()),
    });
    drop(scope);
    if let Some(root)=resolved {runtime.release_root(root);}
    result
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
        Ok(false) => { super::worker_threads_shared_vm::record_callback_exception(runtime,exception); Err(format_uncaught_error(runtime, program, &mut failure)) },
        Err(handler_error) => Err(handler_error),
    };
    runtime.release_root(exception);
    result
}

/// The text Node prints for an uncaught `error`: its own fatal-exception enhancer
/// (`internal/errors`' `fatalExceptionStackEnhancers.afterInspector`), which inspects the
/// error with its own properties (an `ERR_DLOPEN_FAILED` error's `code`) and falls back to
/// its `stack`. `None` when the enhancer cannot be reached or gives no string.
fn fatal_error_text(runtime: &mut quench_runtime::Runtime<NodeHost>, error: RootId) -> Option<String> {
    let mut roots = Vec::new();
    let text = (|| {
        let global = runtime.global_root().ok()?;
        roots.push(global);
        let require = runtime.get_named_property_rooted(global, crate::host::shared_vm::commonjs::BOOTSTRAP_REQUIRE).ok()?;
        roots.push(require);
        let specifier = runtime.string_rooted("internal/errors");
        roots.push(specifier);
        let errors = runtime.call_rooted(require, global, &[specifier]).ok()?;
        roots.push(errors);
        let enhancers = runtime.get_named_property_rooted(errors, "fatalExceptionStackEnhancers").ok()?;
        roots.push(enhancers);
        let after = runtime.get_named_property_rooted(enhancers, "afterInspector").ok()?;
        roots.push(after);
        let text = runtime.call_rooted(after, enhancers, &[error]).ok()?;
        roots.push(text);
        runtime.string_text_rooted(text).ok().flatten()
    })();
    for root in roots {
        runtime.release_root(root);
    }
    text
}

/// Build fatal output only after exception handlers decline it. Source text is
/// the compiler snapshot; a guest can rewrite or remove its file after loading.
pub(crate) fn format_uncaught_error(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    error: &mut RootedError,
) -> String {
    // A worker hands its fatal exception to the parent's 'error' event.
    crate::modules::worker_shared_vm::capture_fatal(runtime, error.exception);
    let stack = error.exception.and_then(|exception| {
        if !runtime.is_error_rooted(exception).ok()? {
            return None;
        }
        if let Some(text) = fatal_error_text(runtime, exception) {
            return Some(text);
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
    let process_control = shared_state.borrow().process_control.clone();
    loop {
        if process_control.requested_exit_code().is_some() {
            return Ok(());
        }
        loop {
            let callback = { shared_state.borrow_mut().scheduler.take_shared_next_tick() };
            let Some(callback) = callback else {
                break;
            };
            runtime.with_native_stack_frame("process.processTicksAndRejections", |runtime| {
                invoke_async_callback(runtime, program, callback)
            })?;
            if process_control.requested_exit_code().is_some() {
                return Ok(());
            }
        }
        run_host_jobs_with_uncaught(runtime, program)?;
        // An async callback can wrap the host's exit completion in a rejected
        // promise. Termination wins over further ticks/rejection notification;
        // dispatching that synthetic rejection would replace the requested code.
        if process_control.requested_exit_code().is_some() {
            return Ok(());
        }
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
            super::crypto_key_material_shared_vm::prune(runtime);
            #[cfg(quench_openssl_provider)]
            super::crypto_signature_shared_vm::prune(runtime);
            super::crypto_secret_job_shared_vm::prune(runtime);
            #[cfg(quench_openssl_provider)]
            super::crypto_hash_shared_vm::prune(runtime);
            super::zlib_native_shared_vm::prune(runtime);
            super::fs_watch_shared_vm::prune(runtime);
            super::tls_shared_vm::contexts::prune(runtime);
            #[cfg(quench_icu)]
            super::icu_binding_shared_vm::prune(runtime);
            super::net_reference_shared_vm::prune(runtime);
            super::contextify_shared_vm::prune(runtime);
            let epoch = runtime.collection_epoch();
            let prune_promises = {
                let shared = runtime.host_mut().shared_state();
                let state = shared.borrow();
                // The prune records the epoch it ran at.
                !state.async_hooks.promise_resources.is_empty()
                    && state.async_hooks.promise_resources_pruned_epoch != epoch
            };
            if prune_promises {
                let prune = runtime.host_function(crate::host::shared_vm::operation("asyncPromisePrune"))
                    .map_err(|error| rooted_error_message(runtime, program, error))?;
                let result = runtime.call_rooted(prune, prune, &[]);
                runtime.release_root(prune);
                let result = result.map_err(|error| rooted_error_message(runtime, program, error))?;
                runtime.release_root(result);
            }
            super::fs_stat_watch_shared_vm::prune(runtime);
            return Ok(());
        }
        dispatch_promise_rejection_batch(runtime, program, unhandled)?;
    }
}

/// Event-loop source delivery through a host operation that runs one pending completion
/// per call and returns whether it delivered one. Each delivery runs as its own callback:
/// its microtasks drain and its exception routes as uncaught. `true` when any ran.
pub(crate) fn deliver_each(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    operation: &'static str,
) -> Result<bool, String> {
    let operation = runtime
        .host_function(crate::host::shared_vm::operation(operation))
        .map_err(|error| error.to_string())?;
    let receiver = runtime.root(quench_runtime::Value::UNDEFINED);
    let mut ran = false;
    let result = loop {
        let delivered = match runtime.call_rooted(operation, receiver, &[]) {
            Ok(result) => {
                let delivered = runtime.rooted_value(result).and_then(quench_runtime::Value::as_bool) == Some(true);
                runtime.release_root(result);
                if !delivered {
                    break Ok(ran);
                }
                run_host_jobs_with_uncaught(runtime, program)
            }
            Err(error) => route_callback_failure(runtime, program, error)
                .and_then(|()| run_host_jobs_with_uncaught(runtime, program)),
        };
        ran = true;
        if let Err(error) = delivered {
            break Err(error);
        }
    };
    runtime.release_root(operation);
    runtime.release_root(receiver);
    result
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
            Ok(false) => { super::worker_threads_shared_vm::record_callback_exception(runtime,exception); Err(format_uncaught_error(runtime, program, &mut failure)) },
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
                    WarningPayload::Error { stack: None, id: Some(id) },
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
    if let Some(exception)=error.exception { super::worker_threads_shared_vm::record_callback_exception(runtime,exception); }
    let message = runtime.format_error(program, &error.error);
    if let Some(exception) = error.exception {
        runtime.release_root(exception);
    }
    message
}

fn call_process_event(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    event_name: &str,
    args: &[RootId],
) -> Result<Option<RootId>, String> {
    let Some(process) = shared_process_root(runtime) else {
        return Ok(None);
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
    Ok(Some(emitted))
}

fn emit_process_event(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    event_name: &str,
    args: &[RootId],
) -> Result<bool, String> {
    let Some(emitted) = call_process_event(runtime, program, event_name, args)? else { return Ok(false); };
    let handled = runtime.truthy_rooted(emitted).map_err(|error| runtime.format_error(program, &error));
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
        WarningPayload::Message,
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
        WarningPayload::Error { stack, id: None },
    )
}

enum WarningPayload<'a> {
    Message,
    Error { stack: Option<&'a str>, id: Option<u64> },
}

fn schedule_rejection_warning(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    name: &str,
    message: &str,
    payload_kind: WarningPayload<'_>,
) -> Result<(), String> {
    let Some(process) = shared_process_root(runtime) else { return Ok(()); };
    let emit_warning = get_rooted_property(runtime, program, process, "emitWarning")?;
    let mut roots = vec![emit_warning];
    let result = (|| {
        let object_warning = matches!(payload_kind, WarningPayload::Error { .. });
        let payload = if let WarningPayload::Error { stack, id } = payload_kind {
            let warning = runtime.error_rooted(message)
                .map_err(|error| rooted_error_message(runtime, program, error))?;
            roots.push(warning);
            set_rooted_text_property(runtime, program, warning, "name", name)?;
            if let Some(stack) = stack {
                set_rooted_text_property(runtime, program, warning, "stack", stack)?;
            }
            if let Some(id) = id {
                set_shared_property(runtime, program, warning, "id", Value::number(id as f64))?;
            }
            warning
        } else {
            let message = runtime.string_rooted(message);
            roots.push(message);
            message
        };
        let args = if object_warning { vec![payload] } else {
            let name = runtime.string_rooted(name);
            roots.push(name);
            vec![payload, name]
        };
        let result = runtime.call_rooted(emit_warning, process, &args)
            .map_err(|error| rooted_error_message(runtime, program, error))?;
        runtime.release_root(result);
        Ok(())
    })();
    for root in roots { runtime.release_root(root); }
    result
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

// Exit listeners may submit native work. Accept it, then abandon it without
// executing guest callbacks. Teardown also runs when an exit listener throws.
fn cleanup_native_work(runtime: &mut quench_runtime::Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    shared.borrow_mut().native_tasks.shutdown();
    super::crypto_secret_job_shared_vm::cleanup(runtime);
    super::crypto_key_material_shared_vm::cleanup(runtime);
    #[cfg(quench_openssl_provider)]
    super::crypto_signature_shared_vm::cleanup(runtime);
    #[cfg(quench_openssl_provider)]
    super::crypto_hash_shared_vm::cleanup(runtime);
    #[cfg(quench_node_cares)]
    super::cares_channel_shared_vm::cleanup(runtime);
    super::cares_binding::cleanup(runtime);
    super::performance_binding_shared_vm::cleanup(runtime);
    super::pipe_binding::cleanup(runtime);
    super::stream_binding::cleanup_requests(runtime);
    super::udp_binding::cleanup(runtime);
    super::native_async_wrap::cleanup(runtime);
    let callbacks = shared.borrow_mut().scheduler.take_all_shared_immediates();
    for immediate in callbacks {
        let callback = immediate.callback;
        if let Some(execution) = callback.execution {
            // No destroy dispatch is scheduled during final cancellation.
            super::async_hooks_shared_vm::clear_context(runtime, &shared, execution.id);
            if let Some(key)=execution.optional_callback {runtime.release_root(key);}
            runtime.release_root(execution.resource);
        }
        release_callback_roots(runtime, callback.callback, callback.receiver, callback.args);
    }
}

// Builtin native objects remain usable throughout synchronous exit listeners.
fn emit_exit(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    code: i32,
) -> Result<(), String> {
    let result = emit_exit_inner(runtime, program, code);
    cleanup_after_exit(runtime);
    result
}
fn cleanup_after_exit(runtime: &mut quench_runtime::Runtime<NodeHost>) {
    super::tls_shared_vm::contexts::cleanup(runtime);
    cleanup_native_work(runtime);
    super::zlib_native_shared_vm::cleanup(runtime);
    #[cfg(quench_icu)]
    super::icu_binding_shared_vm::cleanup(runtime);
}

fn emit_exit_inner(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    code: i32,
) -> Result<(), String> {
    // Never enter a terminated VM to re-emit an event already run synchronously.
    if runtime
        .host_mut()
        .shared_state()
        .borrow()
        .scheduler
        .shared_is_exiting()
    {
        return Ok(());
    }
    let function = runtime
        .host_function(crate::host::shared_vm::operation("processEmitExitEvent"))
        .map_err(|error| rooted_error_message(runtime, program, error))?;
    let value = runtime.root(Value::number(code as f64));
    let result = runtime.call_rooted(function, function, &[value]);
    runtime.release_root(function);
    runtime.release_root(value);
    match result {
        Ok(result) => {
            runtime.release_root(result);
            Ok(())
        }
        Err(error) => Err(rooted_error_message(runtime, program, error)),
    }
}

pub(super) fn release_callback(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    callback: crate::modules::shared_event_loop::SharedCallback,
) {
    if let Some(execution) = callback.execution {
        if execution.lifetime==super::shared_event_loop::CallbackLifetime::Operation {super::async_hooks_shared_vm::complete_runtime_context(runtime, execution.id);super::native_async_wrap::detach(runtime,execution.id);}
        if let Some(key)=execution.optional_callback {runtime.release_root(key);}
        runtime.release_root(execution.resource);
    }
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
            if let Some(exception)=error.exception { super::worker_threads_shared_vm::record_callback_exception(runtime,exception); }
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

    let undefined = context.undefined();
    super::shared_event_loop::queue_next_tick(context, callback, undefined, &args[1..])?;
    Ok(context.undefined())
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
        // Listeners receive the signal's name and number (src/signal_wrap.cc).
        let mut args = vec![event, signal];
        #[cfg(target_os = "linux")]
        if let Some(number) = crate::modules::process_signals_state::signal_number(&name) {
            args.push(runtime.root(quench_runtime::Value::number(f64::from(number))));
        }
        state.borrow_mut().scheduler.queue_shared_next_tick(
            crate::modules::shared_event_loop::SharedCallback { execution: None,
                callback,
                receiver,
                args,
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

pub(crate) fn start_listening_signal(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    if let Some(name) = context.string_text(value)? {
        let result = context.host_mut().shared_state().borrow_mut().process_signals.watch(&name);
        if let Err(error) = result { return Err(signal_start_error(context, error)?); }
    }
    Ok(context.undefined())
}

pub(crate) fn stop_listening_signal(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let event = args.first().copied().unwrap_or_else(|| context.undefined());
    if let Some(name) = context.string_text(event)? {
        let process = context.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))?;
        let count = super::event_constructor_shared_vm::get(context, process, "listenerCount")?;
        let count = context.call_rooted(count, process, &[event])?;
        let zero = context.number(0.0);
        if context.strict_equal_rooted(count, zero)? {
            context.host_mut().shared_state().borrow_mut().process_signals.unwatch(Some(&name));
        }
    }
    Ok(context.undefined())
}

/// Native startup state and the configured SDK own the warning listener together.
pub(crate) fn initialize_warning_policy(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    use super::event_constructor_shared_vm::get;
    let state = context.host_mut().shared_state();
    let (process, options) = {
        let state = state.borrow();
        (state.process_module.ok_or_else(|| RootedError::host("process is not initialized"))?, state.cli_options.clone())
    };
    for (name, value) in [
        ("noDeprecation", !options.enabled("--deprecation")),
        ("noProcessWarnings", !options.enabled("--warnings")),
        ("traceProcessWarnings", options.enabled("--trace-warnings")),
        ("traceDeprecation", options.enabled("--trace-deprecation")),
        ("throwDeprecation", options.enabled("--throw-deprecation")),
    ] {
        if value {
            let key = context.string_rooted(name);
            let yes = context.boolean(true);
            context.define_data_property_rooted(process, key, yes, false, true, true)?;
        }
    }
    let exposed = options.enabled("--expose-internals");
    if exposed {
        let emit_warning = warning_export(context, "emitWarning")?;
        install(context, process, "emitWarning", emit_warning)?;
    } else {
        // Node's warning module is loaded on the first read of `process.emitWarning`.
        let initializer = context.host_function(crate::host::shared_vm::operation("processEmitWarningInitialize"))?;
        context.define_lazy_data_properties_rooted(process, &["emitWarning"], true, initializer)?;
    }
    // The bootstrap reads the environment before any script can change `process.env`.
    let disabled = environment(context).into_iter().any(|(name, value)| name == "NODE_NO_WARNINGS" && value == "1");
    if options.enabled("--warnings") && !disabled {
        // Node's listener, or, unless a script can reach Node's own to compare, a listener of
        // its name and length that loads Node's on the first warning: most runs never warn.
        let callback = if options.enabled("--expose-internals") {
            warning_export(context, "onWarning")?
        } else {
            let factory = context.evaluate_script_rooted(
                "(load) => function onWarning(warning) { return load()(warning); }",
                "node:internal/process/warning-listener.js",
            )?;
            let load = context.host_function(crate::host::shared_vm::operation("processWarningListener"))?;
            let undefined = context.undefined();
            context.call_rooted(factory, undefined, &[load])?
        };
        let on = get(context, process, "on")?;
        let event = context.string_rooted("warning");
        context.call_rooted(on, process, &[event, callback])?;
    }
    if options.enabled("--pending-deprecation") {
        let binding = get(context, process, "binding")?;
        let deprecate = crate::host::shared_vm::commonjs::node_library_export(
            context, "internal/process/pre_execution.js", "internal/util.js", "deprecate",
        )?;
        let message = context.string_rooted("process.binding() is deprecated. Please use public APIs instead.");
        let code = context.string_rooted("DEP0111");
        let undefined = context.undefined();
        let binding = context.call_rooted(deprecate, undefined, &[binding, message, code])?;
        install(context, process, "binding", binding)?;
    }
    Ok(())
}

fn warning_export(context: &mut NativeContext<'_, NodeHost>, name: &str) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_export(context, "internal/process/pre_execution.js", "internal/process/warning.js", name)
}

/// The environment `process.env` starts from: a worker's own, else the OS environment.
pub(crate) fn environment(context: &mut NativeContext<'_, NodeHost>) -> Vec<(String, String)> {
    let worker_env = context.host_mut().shared_state().borrow().messaging.context.as_ref().map(|worker| worker.env.clone());
    worker_env.unwrap_or_else(|| std::env::vars().collect())
}

/// `process.env`: the environment as Node's bootstrap reads it, less an IPC channel's own
/// variables.
pub(crate) fn initialize_env(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    crate::host::startup_snapshot::refuse_process_state(context, "process.env")?;
    let process = context.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))?;
    let env = context.object_rooted()?;
    for (name, value) in environment(context) {
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
        // One entry's handles at a time, however large the environment.
        context.scoped(|context| {
            let key = context.string_rooted(&name);
            let value = context.string_rooted(&value);
            if !context.set_property_rooted(env, key, value, env)? {
                return Err(RootedError::host("cannot install process.env entry"));
            }
            Ok(())
        })?;
    }
    let factory = context.evaluate_script_rooted(ENV_PROXY_SOURCE, "node:process/env.js")?;
    let set_env = context.host_function(crate::host::shared_vm::operation("processSetEnv"))?;
    let delete_env = context.host_function(crate::host::shared_vm::operation("processDeleteEnv"))?;
    let undefined = context.undefined();
    let env = context.call_rooted(factory, undefined, &[env, set_env, delete_env])?;
    let key = context.string_rooted("env");
    context.define_data_property_rooted(process, key, env, true, true, true)?;
    Ok(context.undefined())
}

/// Node's `process.env` over the copied environment: a stored value becomes a string and
/// sets the OS variable, a deleted key unsets it, and keys take only plain data descriptors.
const ENV_PROXY_SOURCE: &str = r#"(target, setEnv, deleteEnv) => new Proxy(target, {
  set(target, key, value) {
    if (typeof key === "symbol" || typeof value === "symbol") {
      throw new TypeError("Cannot convert a Symbol value to a string");
    }
    if (key === "") return true;
    if (typeof value !== "string" && typeof value !== "number" &&
        typeof value !== "boolean" &&
        globalThis.process.execArgv.includes("--pending-deprecation")) {
      globalThis.process.emitWarning(
        "Assigning any value other than a string, number, or boolean to a process.env property is deprecated. Please make sure to convert the value to a string before setting process.env with it.",
        { type: "DeprecationWarning", code: "DEP0104" }
      );
    }
    const text = String(value);
    const result = Reflect.set(target, key, text, target);
    if (result && typeof key !== "symbol") setEnv(String(key), text);
    return result;
  },
  defineProperty(target, key, descriptor) {
    const invalid = (message) => {
      const error = new TypeError(message);
      error.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
      return error;
    };
    if (typeof key === "symbol") {
      throw invalid("'process.env' does not accept symbol properties");
    }
    if (key === "") return true;
    if ("get" in descriptor || "set" in descriptor) {
      throw invalid("'process.env' does not accept an accessor(getter/setter) descriptor");
    }
    if (descriptor.configurable !== true || descriptor.writable !== true || descriptor.enumerable !== true) {
      throw invalid("'process.env' only accepts a configurable, writable, and enumerable data descriptor");
    }
    if (typeof descriptor.value === "symbol") {
      throw new TypeError("Cannot convert a Symbol value to a string");
    }
    const value = String(descriptor.value);
    const result = Reflect.defineProperty(target, key, {
      value,
      configurable: true,
      writable: true,
      enumerable: true
    });
    if (result) setEnv(String(key), value);
    return result;
  },
  deleteProperty(target, key) {
    const result = Reflect.deleteProperty(target, key);
    if (result && key !== "" && typeof key !== "symbol") deleteEnv(String(key));
    return result;
  }
})"#;

/// Defines `process.emitWarning` (`process_warning_shared_vm`).
pub(crate) fn initialize_emit_warning(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let process = context.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))?;
    let emit_warning = crate::modules::process_warning_shared_vm::emit_warning(context)?;
    let key = context.string_rooted("emitWarning");
    context.define_data_property_rooted(process, key, emit_warning, true, true, true)?;
    Ok(context.undefined())
}

/// The default `warning` listener's implementation: Node's `onWarning`
/// (`process_warning_shared_vm`).
pub(crate) fn warning_listener(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    crate::modules::process_warning_shared_vm::on_warning(context)
}
