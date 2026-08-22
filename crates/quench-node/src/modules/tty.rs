//! `tty` module — pure Rust `isatty`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

pub fn isatty(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => *value as i32,
        _ => return Ok(Value::Boolean(false)),
    };
    Ok(Value::Boolean(platform_isatty(fd)))
}

#[cfg(unix)]
fn platform_isatty(fd: i32) -> bool {
    unsafe {
        extern "C" {
            fn isatty(fd: i32) -> i32;
        }
        isatty(fd) != 0
    }
}

#[cfg(not(unix))]
fn platform_isatty(_fd: i32) -> bool {
    false
}

pub fn value_to_i32(value: &Value) -> i32 {
    match value {
        Value::Number(n) => *n as i32,
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "isatty",
            crate::host::capability(crate::registry::SPEC_TTY_ISATTY),
        ),
        (
            "ReadStream",
            crate::host::capability(crate::registry::SPEC_TTY_READSTREAM),
        ),
        (
            "WriteStream",
            crate::host::capability(crate::registry::SPEC_TTY_WRITESTREAM),
        ),
        (
            "Socket",
            crate::host::capability(crate::registry::SPEC_TTY_WRITESTREAM),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}

pub fn read_stream(
    _state: &Rc<RefCell<crate::host::HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = args.first().map(value_to_i32).unwrap_or(0);
    Ok(crate::host::namespace_object_from_pairs(vec![
        ("fd".into(), Value::Number(fd as f64)),
        ("isTTY".into(), Value::Boolean(false)),
    ]))
}

pub fn write_stream(
    state: &Rc<RefCell<crate::host::HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    read_stream(state, receiver, args)
}
