//! Shared-VM bindings for the existing process state, extended as APIs migrate.

use crate::host::{NodeHost, ProcessModule};
use rqj::{NativeContext, RootId, RootedError};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    context
        .host_mut()
        .state()
        .borrow_mut()
        .event_loop
        .reset_shared();
    let process = context.object_rooted()?;
    let function = context.host_function(crate::host::shared_vm::operation("uptime"))?;
    install(context, process, "uptime", function)?;
    for (name, operation) in [
        ("nextTick", "nextTick"),
        ("on", "on"),
        ("cwd", "processCwd"),
        ("umask", "processUmask"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        install(context, process, name, function)?;
    }
    let exiting = context.boolean(false);
    install(context, process, "_exiting", exiting)?;
    let (argv, exec_argv) = {
        let state = context.host_mut().state();
        let state = state.borrow();
        (state.process.argv.clone(), state.exec_argv.clone())
    };
    let env = context.object_rooted()?;
    for (name, value) in std::env::vars() {
        let key = context.string_rooted(&name);
        let value = context.string_rooted(&value);
        if !context.set_property_rooted(env, key, value, env)? {
            return Err(RootedError::host("cannot install process.env entry"));
        }
    }
    install(context, process, "env", env)?;
    for (name, values) in [("argv", argv), ("execArgv", exec_argv)] {
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
        .state()
        .borrow_mut()
        .process_module
        .replace(ProcessModule::Shared(retained));
    if let Some(ProcessModule::Shared(previous)) = previous {
        context.release_root(previous);
    }
    Ok(())
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

pub(crate) fn cwd(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let cwd = context
        .host_mut()
        .state()
        .borrow()
        .process
        .cwd
        .to_string_lossy()
        .into_owned();
    Ok(context.string_rooted(&cwd))
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
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<(), String> {
    let checkpoint = drain_checkpoint(runtime, program, state);
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
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<(), String> {
    state.borrow_mut().process.exit_code = Some(1);
    emit_exit(runtime, program, state, 1)
}

fn drain_checkpoint(
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<(), String> {
    loop {
        drain_shared_jobs(runtime, program, state)?;

        if crate::modules::fetch_shared_vm::poll(runtime, program, state)? {
            continue;
        }

        let due = state
            .borrow_mut()
            .event_loop
            .take_due_shared_timer(std::time::Instant::now());
        if let Some(timer) = due {
            let callback_result = crate::modules::timers::shared_vm::timer_callback(
                runtime,
                program,
                &timer.callback,
            );
            if callback_result.is_err() {
                state.borrow_mut().event_loop.cancel_shared_timer(timer.id);
            }
            let released = state
                .borrow_mut()
                .event_loop
                .complete_shared_timer(timer, std::time::Instant::now());
            if let Some(callback) = released {
                crate::modules::timers::shared_vm::release_runtime_callback(runtime, callback);
            }
            callback_result?;
            continue;
        }

        let immediate_cutoff = state.borrow().event_loop.shared_immediate_cutoff();
        let Some(cutoff) = immediate_cutoff else {
            let next_timer = state.borrow().event_loop.next_shared_timer_due();
            if crate::modules::fetch_shared_vm::has_pending(state) {
                let poll_interval = crate::modules::fetch_shared_vm::poll_interval();
                let wait = next_timer
                    .map(|due| due.saturating_duration_since(std::time::Instant::now()))
                    .map_or(poll_interval, |delay| delay.min(poll_interval));
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
                continue;
            }
            if let Some(due) = next_timer {
                std::thread::sleep(due.saturating_duration_since(std::time::Instant::now()));
                continue;
            }
            break;
        };
        let mut ran_immediate = false;
        loop {
            let immediate = {
                state
                    .borrow_mut()
                    .event_loop
                    .take_shared_immediate_through(cutoff)
            };
            let Some(immediate) = immediate else {
                break;
            };
            ran_immediate = true;
            if let Err(error) = invoke(runtime, program, immediate.callback) {
                let cancelled = state
                    .borrow_mut()
                    .event_loop
                    .take_shared_immediate_through(cutoff);
                if let Some(cancelled) = cancelled {
                    release_callback_roots(
                        runtime,
                        cancelled.callback.callback,
                        cancelled.callback.receiver,
                        cancelled.callback.args,
                    );
                }
                return Err(error);
            }
            drain_shared_jobs(runtime, program, state)?;
        }
        if !ran_immediate {
            let next_timer = state.borrow().event_loop.next_shared_timer_due();
            if crate::modules::fetch_shared_vm::has_pending(state) {
                let poll_interval = crate::modules::fetch_shared_vm::poll_interval();
                let wait = next_timer
                    .map(|due| due.saturating_duration_since(std::time::Instant::now()))
                    .map_or(poll_interval, |delay| delay.min(poll_interval));
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
                continue;
            }
            if let Some(due) = next_timer {
                std::thread::sleep(due.saturating_duration_since(std::time::Instant::now()));
            } else {
                break;
            }
        }
    }
    Ok(())
}

fn drain_shared_jobs(
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<(), String> {
    loop {
        loop {
            let callback = { state.borrow_mut().event_loop.take_shared_next_tick() };
            let Some(callback) = callback else {
                break;
            };
            invoke(runtime, program, callback)?;
        }
        runtime
            .run_host_jobs(program)
            .map_err(|error| runtime.format_error(program, &error))?;
        if !state.borrow().event_loop.has_shared_next_ticks() {
            return Ok(());
        }
    }
}

fn emit_exit(
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
    code: i32,
) -> Result<(), String> {
    let listeners = state.borrow_mut().event_loop.begin_shared_exit();
    if let Some(process) = shared_process_root(state) {
        set_shared_property(runtime, program, process, "_exiting", rqj::Value::TRUE)?;
    }
    let mut listeners = listeners.into_iter();
    while let Some(mut listener) = listeners.next() {
        listener
            .args
            .push(runtime.root(rqj::Value::number(code as f64)));
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
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    callback: crate::modules::event_loop::SharedCallback,
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
    runtime: &mut rqj::Runtime<NodeHost>,
    callback: crate::modules::event_loop::SharedCallback,
) {
    release_callback_roots(runtime, callback.callback, callback.receiver, callback.args);
}

fn release_callback_roots(
    runtime: &mut rqj::Runtime<NodeHost>,
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

fn shared_process_root(state: &Rc<RefCell<crate::host::HostState>>) -> Option<RootId> {
    match state.borrow().process_module.as_ref() {
        Some(ProcessModule::Shared(root)) => Some(*root),
        _ => None,
    }
}

fn set_shared_property(
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    object: RootId,
    name: &str,
    value: rqj::Value,
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
    let state = context.host_mut().state();
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = received_type(context, callback)?;
        return invalid_callback(context, &received);
    }
    if state.borrow().event_loop.shared_is_exiting() {
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
    state.borrow_mut().event_loop.queue_shared_next_tick(
        crate::modules::event_loop::SharedCallback {
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
    let event_root = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(event) = context.string_text(event_root)? else {
        let error = context.type_error_rooted(
            "The \"type\" argument must be of type string. Received an instance of Object",
        )?;
        let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
        install(context, error, "code", code)?;
        return Err(context.throw(error));
    };
    let Some(callback) = args.get(1).copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = received_type(context, callback)?;
        return invalid_callback(context, &received);
    }
    let callback = context.retain(callback)?;
    let retained_receiver = context.retain(receiver)?;
    context
        .host_mut()
        .state()
        .borrow_mut()
        .event_loop
        .add_shared_listener(
            event,
            crate::modules::event_loop::SharedCallback {
                callback,
                receiver: retained_receiver,
                args: Vec::new(),
            },
        );
    Ok(receiver)
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
