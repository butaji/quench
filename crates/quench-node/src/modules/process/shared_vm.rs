//! Shared-VM bindings for the existing process state, extended as APIs migrate.

use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, PromiseRejectionEvent, RootId, RootedError, Value};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

struct CallbackFailure {
    exception: Option<RootId>,
    message: String,
}

const STDOUT_FD: i32 = 1;
const STDERR_FD: i32 = 2;
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
        ("umask", "processUmask"),
        ("emitWarning", "processEmitWarning"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        install(context, process, name, function)?;
    }
    let on = context.host_function(crate::host::shared_vm::operation("on"))?;
    install(context, process, "on", on)?;
    install(context, process, "addListener", on)?;
    let once = context.host_function(crate::host::shared_vm::operation("processOnce"))?;
    install(context, process, "once", once)?;
    let emit = context.host_function(crate::host::shared_vm::operation("processEmit"))?;
    install(context, process, "emit", emit)?;
    let exiting = context.boolean(false);
    install(context, process, "_exiting", exiting)?;
    let shared_state = context.host_mut().shared_state();
    let argv = shared_state.borrow().process_argv.clone();
    let exec_argv = shared_state.borrow().exec_argv.clone();
    let env = context.object_rooted()?;
    for (name, value) in std::env::vars() {
        let key = context.string_rooted(&name);
        let value = context.string_rooted(&value);
        if !context.set_property_rooted(env, key, value, env)? {
            return Err(RootedError::host("cannot install process.env entry"));
        }
    }
    install(context, process, "env", env)?;
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
    set_text(
        context,
        process,
        "arch",
        crate::modules::process::architecture(),
    )?;
    set_text(
        context,
        process,
        "platform",
        &crate::modules::process::platform(),
    )?;
    let pid = context.number(std::process::id() as f64);
    install(context, process, "pid", pid)?;
    let version = format!("v{}", crate::modules::process::NODE_VERSION);
    set_text(context, process, "version", &version)?;
    install_config(context, process)?;
    install_facts(
        context,
        process,
        "features",
        crate::modules::process::feature_facts(),
    )?;
    install_versions(context, process)?;
    let global = context.global_root()?;
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
    let tty = context.boolean(crate::modules::tty::is_terminal_fd(fd));
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
    for (name, fact) in crate::modules::process::config_variable_facts() {
        let value = fact_value(context, *fact)?;
        install(context, variables, name, value)?;
    }
    install(context, config, "variables", variables)?;
    install(context, process, "config", config)
}

fn install_facts(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    property: &str,
    facts: &[(&'static str, crate::modules::process::ProcessFact)],
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
    fact: crate::modules::process::ProcessFact,
) -> Result<RootId, RootedError> {
    match fact {
        crate::modules::process::ProcessFact::Boolean(value) => Ok(context.boolean(value)),
        crate::modules::process::ProcessFact::Number(value) => Ok(context.number(value)),
        crate::modules::process::ProcessFact::String(value) => Ok(context.string_rooted(value)),
        crate::modules::process::ProcessFact::EmptyArray => context.array_rooted(&[]),
    }
}

fn install_versions(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
) -> Result<(), RootedError> {
    let versions = context.object_rooted()?;
    for (name, version) in crate::modules::process::version_facts() {
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
    let seconds = context.host_mut().state().borrow().process.uptime();
    Ok(context.number(seconds))
}

pub(crate) fn exit_code_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let code = context.host_mut().state().borrow().process.exit_code;
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
        context.host_mut().state().borrow_mut().process.exit_code = None;
        return Ok(context.undefined());
    };
    let value = context
        .rooted_value(value_root)
        .ok_or_else(|| RootedError::host("invalid process.exitCode value root"))?;
    if value.is_undefined() || value.is_null() {
        context.host_mut().state().borrow_mut().process.exit_code = None;
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
        return invalid_exit_code_type(context);
    };
    if !number.is_finite() || number.fract() != 0.0 || number.abs() > MAX_SAFE_EXIT_CODE {
        return exit_code_range_error(context, number);
    }
    context.host_mut().state().borrow_mut().process.exit_code = Some(number as i64 as i32);
    Ok(context.undefined())
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
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted("The \"code\" argument must be of type number")?;
    Err(throw_with_code(context, error, "ERR_INVALID_ARG_TYPE"))
}

fn exit_code_range_error(
    context: &mut NativeContext<'_, NodeHost>,
    number: f64,
) -> Result<RootId, RootedError> {
    let message = format!(
        "The value of \"code\" is out of range. It must be a safe integer. Received {number}"
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
    let text = context.to_string(chunk)?;
    if fd == STDERR_FD {
        std::io::stderr()
            .lock()
            .write_all(text.as_bytes())
            .map_err(|error| RootedError::host(error.to_string()))?;
    } else {
        let output = context.host_mut().state().borrow().output.clone();
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
    match crate::modules::process::change_directory_cwd(&cwd, &directory) {
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
        let current = context.host_mut().state().borrow().process.umask;
        return Ok(context.number(current as f64));
    };
    let value = context
        .rooted_value(mask)
        .ok_or_else(|| RootedError::host("invalid process.umask argument root"))?;
    let parsed = if let Some(number) = value.as_number() {
        (number.is_finite() && number >= 0.0 && number.fract() == 0.0).then_some(number as u32)
    } else if let Some(text) = context.string_text(mask)? {
        u32::from_str_radix(&text, 8).ok()
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
    let previous = {
        let state = context.host_mut().state();
        let mut state = state.borrow_mut();
        state.process.update_umask(parsed)
    };
    Ok(context.number(previous as f64))
}

/// Run shared-host nextTick callbacks before VM jobs, then emit process exit
/// without restarting the VM or invalidating callback roots.
pub(crate) fn finish_execution(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<(), String> {
    let checkpoint = drain_checkpoint(runtime, program, state);
    let shared_state = runtime.host_mut().shared_state();
    crate::modules::http::shared_vm::cleanup(runtime, &shared_state);
    let exit_code = match checkpoint {
        Ok(()) => state.borrow().process.exit_code.unwrap_or(0),
        Err(_) => {
            state.borrow_mut().process.exit_code = Some(1);
            1
        }
    };
    let exit = emit_exit(runtime, program, state, exit_code);

    checkpoint.and(exit)
}

pub(crate) fn finish_after_uncaught_error(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
    error: &quench_runtime_next::JsError,
) -> Result<bool, String> {
    let routed = emit_uncaught_exception(runtime, program, error);
    if matches!(&routed, Ok(true)) {
        finish_execution(runtime, program, state)?;
        return Ok(true);
    }
    let handler_error = routed.err();
    state.borrow_mut().process.exit_code = Some(1);
    let shared_state = runtime.host_mut().shared_state();
    crate::modules::http::shared_vm::cleanup(runtime, &shared_state);
    emit_exit(runtime, program, state, 1)?;
    handler_error.map_or(Ok(false), Err)
}

fn emit_uncaught_exception(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    error: &quench_runtime_next::JsError,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
        .and_then(quench_runtime_next::Value::as_bool)
        .ok_or_else(|| "process.emit returned a non-boolean result".to_owned());
    runtime.release_root(emitted);
    match handled {
        Ok(handled) => Ok(handled),
        Err(message) => Err(message),
    }
}

fn drain_checkpoint(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<(), String> {
    let shared_state = runtime.host_mut().shared_state();
    loop {
        drain_shared_jobs(runtime, program, state, &shared_state)?;

        if crate::modules::fetch_shared_vm::poll(runtime, program, &shared_state)? {
            continue;
        }
        if crate::modules::http::shared_vm::poll(runtime, program, state, &shared_state)? {
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
                    .min(crate::modules::net::shared_vm::poll_interval());
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
            if let Err(error) = drain_shared_jobs(runtime, program, state, &shared_state) {
                release_pending_shared_immediates(runtime, &shared_state);
                return Err(error);
            }
        }
        if !ran_immediate {
            let next_timer = shared_state.borrow().scheduler.next_shared_timer_due();
            if has_referenced_shared_work(&shared_state) {
                let poll_interval = crate::modules::fetch_shared_vm::poll_interval()
                    .min(crate::modules::net::shared_vm::poll_interval());
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
        || crate::modules::http::shared_vm::has_work(shared_state)
        || shared_state.borrow().scheduler.has_refed_shared_timers()
        || shared_state
            .borrow()
            .scheduler
            .has_refed_shared_immediates()
}

fn settle_immediate_reference(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    callback: &crate::modules::shared_event_loop::SharedCallback,
) -> Result<(), String> {
    match call_shared_callback(runtime, program, callback) {
        Ok(()) => Ok(()),
        Err(failure) => route_callback_failure(runtime, program, failure),
    }
}

fn invoke_async_callback(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
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
            dispatch_promise_rejection_batch(runtime, program, state, handled)?;
        }
        let unhandled = runtime.take_promise_rejection_unhandled_events();
        if unhandled.is_empty() {
            return Ok(());
        }
        dispatch_promise_rejection_batch(runtime, program, state, unhandled)?;
    }
}

pub(crate) fn run_host_jobs_with_uncaught(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
    events: Vec<PromiseRejectionEvent>,
) -> Result<(), String> {
    let mut events = events.into_iter();
    while let Some(event) = events.next() {
        let result = dispatch_promise_rejection(runtime, program, state, event);
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
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
        } => dispatch_unhandled_rejection(runtime, program, state, id, promise, reason),
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
    id: u64,
    promise: RootId,
    reason: RootId,
) -> Result<(), String> {
    let mode = state.borrow().process.unhandled_rejection_mode;
    match mode {
        crate::modules::process::UnhandledRejectionMode::None => {
            emit_process_event(runtime, program, "unhandledRejection", &[reason, promise])?;
        }
        crate::modules::process::UnhandledRejectionMode::Warn => {
            emit_process_event(runtime, program, "unhandledRejection", &[reason, promise])?;
            schedule_unhandled_rejection_warnings(runtime, program, id, reason)?;
        }
        crate::modules::process::UnhandledRejectionMode::Throw => {
            let handled =
                emit_process_event(runtime, program, "unhandledRejection", &[reason, promise])?;
            if !handled {
                route_unhandled_rejection(runtime, program, state, reason)?;
            }
        }
        crate::modules::process::UnhandledRejectionMode::Strict => {
            let (exception, owned_exception) =
                unhandled_rejection_exception(runtime, program, reason)?;
            let handled =
                route_uncaught_exception_value(runtime, program, exception, "unhandledRejection");
            if let Some(exception) = owned_exception {
                runtime.release_root(exception);
            }
            if !handled? {
                return terminate_unhandled_rejection(state);
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
    reason: RootId,
) -> Result<(), String> {
    let (exception, owned_exception) = unhandled_rejection_exception(runtime, program, reason)?;
    let handled = route_uncaught_exception_value(runtime, program, exception, "unhandledRejection");
    if let Some(exception) = owned_exception {
        runtime.release_root(exception);
    }
    if !handled? {
        terminate_unhandled_rejection(state)?;
    }
    Ok(())
}

fn terminate_unhandled_rejection(
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<(), String> {
    state.borrow_mut().process.exit_code = Some(1);
    Err("unhandled Promise rejection was not handled".into())
}

fn unhandled_rejection_exception(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    value: RootId,
) -> Result<String, String> {
    runtime
        .detail_string_rooted(value)
        .map_err(|error| rooted_error_message(runtime, program, error))
}

fn own_stack_string(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    object: RootId,
    name: &str,
) -> Result<RootId, String> {
    let key = runtime.string_rooted(name);
    let property = runtime.get_property_rooted(object, key);
    runtime.release_root(key);
    property.map_err(|error| rooted_error_message(runtime, program, error))
}

fn set_rooted_text_property(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    error: RootedError,
) -> String {
    let message = runtime.format_error(program, &error.error);
    if let Some(exception) = error.exception {
        runtime.release_root(exception);
    }
    message
}

fn emit_process_event(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    _state: &Rc<RefCell<crate::host::HostState>>,
    code: i32,
) -> Result<(), String> {
    let shared_state = runtime.host_mut().shared_state();
    let listeners = shared_state.borrow_mut().scheduler.begin_shared_exit();
    if let Some(process) = shared_process_root(runtime) {
        let event_count = shared_state.borrow().scheduler.shared_listener_count();
        let setup = set_shared_property(
            runtime,
            program,
            process,
            "_eventsCount",
            quench_runtime_next::Value::number(event_count as f64),
        )
        .and_then(|()| {
            set_shared_property(
                runtime,
                program,
                process,
                "_exiting",
                quench_runtime_next::Value::TRUE,
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
            .push(runtime.root(quench_runtime_next::Value::number(code as f64)));
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
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
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    callback: crate::modules::shared_event_loop::SharedCallback,
) {
    release_callback_roots(runtime, callback.callback, callback.receiver, callback.args);
}

fn release_callback_roots(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
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

fn shared_process_root(runtime: &mut quench_runtime_next::Runtime<NodeHost>) -> Option<RootId> {
    runtime.host_mut().shared_state().borrow().process_module
}

fn set_shared_property(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    object: RootId,
    name: &str,
    value: quench_runtime_next::Value,
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

    let shared_state = context.host_mut().shared_state();
    let (listeners, suppress_stderr) = {
        let shared = shared_state.borrow();
        (
            shared.scheduler.shared_listener_roots(
                &crate::modules::shared_event_loop::SharedEventKey::String("warning".to_owned()),
            ),
            shared
                .exec_argv
                .iter()
                .any(|argument| argument == "--no-warnings"),
        )
    };
    for (callback, receiver) in listeners {
        let callback = context.retain(callback)?;
        let receiver = context.retain(receiver)?;
        let warning_argument = context.retain(warning)?;
        shared_state.borrow_mut().scheduler.queue_shared_next_tick(
            crate::modules::shared_event_loop::SharedCallback {
                callback,
                receiver,
                args: vec![warning_argument],
            },
        );
    }
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
    value: RootId,
) -> Result<String, RootedError> {
    let value = context
        .rooted_value(value)
        .ok_or_else(|| RootedError::host("invalid process callback root"))?;
    Ok(if value.is_undefined() {
        "undefined".to_owned()
    } else if value.is_null() {
        "null".to_owned()
    } else if let Some(boolean) = value.as_bool() {
        format!("type boolean ({boolean})")
    } else if let Some(number) = value.as_number() {
        format!("type number ({number})")
    } else {
        "an instance of Object".to_owned()
    })
}

#[cfg(test)]
#[path = "shared_vm_tests.rs"]
mod tests;
