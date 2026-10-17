//! Dynamic C++ fatal-exception boundary, separated from the replaceable default.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
type Context<'a> = NativeContext<'a, NodeHost>;
fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))
}
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn set(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    if c.set_property_rooted(object, key, value, object)? {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!(
        "Cannot assign to read only property '{name}' of object '#<process>'"
    ))?;
    Err(c.throw(error))
}
fn discard(c: &mut Context<'_>, error: RootedError) -> Result<(), RootedError> {
    if error.error.is_execution_terminated() {
        return Err(error);
    }
    if let Some(exception) = error.exception {
        c.release_root(exception);
    }
    Ok(())
}
pub(crate) fn install(c: &mut Context<'_>, process: RootId) -> Result<(), RootedError> {
    let name = if super::worker_threads_shared_vm::is_child(c) {
        "workerOnGlobalUncaughtException"
    } else {
        ""
    };
    let function = c.host_function_named(
        crate::host::shared_vm::operation("processDefaultFatalException"),
        name,
    )?;
    let key = c.string_rooted("_fatalException");
    c.define_data_property_rooted(process, key, function, true, true, true)?;
    Ok(())
}
/// Every invocation observes the current public getter and callable. Exact false
/// is the sole unhandled return; JavaScript falsy values remain handled.
pub(crate) fn dispatch(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let process = process(c)?;
    let exception = args.first().copied().unwrap_or_else(|| c.undefined());
    let from_promise = args.get(1).copied().unwrap_or_else(|| c.boolean(false));
    let hook = match get(c, process, "_fatalException") {
        Ok(hook) => hook,
        Err(error) => {
            discard(c, error)?;
            return Ok(c.boolean(true));
        }
    };
    let control = c.host_mut().shared_state().borrow().process_control.clone();
    if !c.is_callable_rooted(hook)? {
        control.request_fatal_uncaught(
            super::node_protocol_constants::ExitCode::InvalidFatalExceptionMonkeyPatching as i32,
        );
        return Ok(c.boolean(false));
    }
    let value = match c.call_rooted(hook, process, &[exception, from_promise]) {
        Ok(value) => value,
        Err(error) => {
            if !error.error.is_execution_terminated() && control.requested_exit_code().is_none() {
                control.fail_fatal_exception_handler();
            }
            return Err(error);
        }
    };
    let handled = c.rooted_value(value).and_then(Value::as_bool) != Some(false);
    if !handled && control.requested_exit_code().is_none() {
        let code = control
            .exit_code()
            .unwrap_or(super::node_protocol_constants::ExitCode::GenericUserError as i32);
        control.request_fatal_uncaught(code);
    }
    Ok(c.boolean(handled))
}
fn main_default(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let exception = args.first().copied().unwrap_or_else(|| c.undefined());
    let from_promise = args.get(1).copied().unwrap_or_else(|| c.boolean(false));
    let origin = if c.truthy_rooted(from_promise)? {
        "unhandledRejection"
    } else {
        "uncaughtException"
    };
    if super::process_exception_route::route(c, exception, origin)? {
        return Ok(c.boolean(true));
    }
    // The original default emits exit itself; a user replacement returning
    // false bypasses this block and therefore emits no exit event.
    let process = process(c)?;
    let result = (|| {
        let exiting = get(c, process, "_exiting")?;
        if !c.truthy_rooted(exiting)? {
            let yes = c.boolean(true);
            set(c, process, "_exiting", yes)?;
            let code =
                c.number(super::node_protocol_constants::ExitCode::GenericUserError as i32 as f64);
            set(c, process, "exitCode", code)?;
            let emit = get(c, process, "emit")?;
            let event = c.string_rooted("exit");
            c.call_rooted(emit, process, &[event, code])?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        discard(c, error)?;
    }
    Ok(c.boolean(false))
}

/// A Worker captures the canonical default, rather than consulting the public
/// hook recursively. Replacing the public hook bypasses this serialization.
pub(crate) fn default_handler(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if !super::worker_threads_shared_vm::is_child(c) {
        return main_default(c, receiver, args);
    }
    let original = args.first().copied().unwrap_or_else(|| c.undefined());
    let mut exception = original;
    let mut handler_threw = false;
    match main_default(c, receiver, args) {
        Ok(value) if c.truthy_rooted(value)? => return Ok(c.boolean(true)),
        Ok(_) => {}
        Err(error) => {
            if error.error.is_execution_terminated() {
                return Err(error);
            }
            exception = error
                .exception
                .ok_or_else(|| RootedError::host("Worker fatal handler lost its thrown value"))?;
            handler_threw = true;
        }
    }
    let process = process(c)?;
    let setup = (|| {
        let exiting = get(c, process, "_exiting")?;
        if !c.truthy_rooted(exiting)? {
            let yes = c.boolean(true);
            set(c, process, "_exiting", yes)?;
            let code =
                c.number(super::node_protocol_constants::ExitCode::GenericUserError as i32 as f64);
            set(c, process, "exitCode", code)?;
            if !handler_threw {
                let emit = get(c, process, "emit")?;
                let event = c.string_rooted("exit");
                let code = get(c, process, "exitCode")?;
                c.call_rooted(emit, process, &[event, code])?;
            }
        }
        Ok(())
    })();
    if let Err(error) = setup {
        discard(c, error)?;
    }
    super::worker_threads_shared_vm::report_fatal_error(c, exception)?;
    let exit = get(c, process, "exit")?;
    c.call_rooted(exit, process, &[])
}
