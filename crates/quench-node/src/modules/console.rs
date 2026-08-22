//! `console` module — pure Rust log/info/warn/error/debug/trace.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::util::inspect;

/// Build the `console` namespace object.
pub fn build() -> Vec<(String, Value)> {
    vec![
        (
            "log".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_LOG),
        ),
        (
            "info".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_INFO),
        ),
        (
            "warn".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_WARN),
        ),
        (
            "error".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_ERROR),
        ),
        (
            "debug".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_DEBUG),
        ),
        (
            "trace".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_TRACE),
        ),
        (
            "dir".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_LOG),
        ),
    ]
}

pub fn build_value() -> Value {
    quench_runtime::host_api::object(build())
}

pub fn log(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    is_error: bool,
) -> Result<Value, quench_runtime::execute::VmError> {
    let line = format_args(args);
    let state = state.borrow();
    if let Some(sink) = &state.output {
        sink(&line);
    }
    if is_error {
        eprintln!("{line}");
    }
    Ok(Value::Undefined)
}

pub fn info(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, false)
}
pub fn warn(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, true)
}
pub fn error(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, true)
}
pub fn debug(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, false)
}

pub fn trace(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let line = "Trace".to_string();
    let state = state.borrow();
    if let Some(sink) = &state.output {
        sink(&line);
    }
    Ok(Value::Undefined)
}

fn format_args(args: &[Value]) -> String {
    if args.is_empty() {
        return String::new();
    }
    if let Value::String(template) = &args[0] {
        crate::modules::util::format_template(template, args)
    } else {
        let mut out = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&inspect(arg));
        }
        out
    }
}
