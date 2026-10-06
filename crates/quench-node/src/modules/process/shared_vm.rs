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
    for (name, operation) in [("nextTick", "nextTick"), ("on", "on")] {
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
    let global = context.global_root()?;
    install(context, global, "process", process)?;
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
        while let Some(callback) = state.borrow_mut().event_loop.take_shared_next_tick() {
            invoke(runtime, program, callback)?;
        }
        runtime
            .run_host_jobs(program)
            .map_err(|error| runtime.format_error(program, &error))?;
        if !state.borrow().event_loop.has_shared_next_ticks() {
            break;
        }
    }
    Ok(())
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
