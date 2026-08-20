//! `tty` module — pure Rust `isatty`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

pub fn isatty(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = args.first().map(value_to_i32).unwrap_or(0);
    let result = match fd {
        0 => atty_stdout(),
        1 => atty_stdout(),
        2 => atty_stderr(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

pub fn value_to_i32(value: &Value) -> i32 {
    match value {
        Value::Number(n) => *n as i32,
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(unix)]
fn atty_stdout() -> bool {
    unsafe { libc_isatty(1) }
}
#[cfg(unix)]
fn atty_stderr() -> bool {
    unsafe { libc_isatty(2) }
}
#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) != 0 }
}

#[cfg(not(unix))]
fn atty_stdout() -> bool {
    false
}
#[cfg(not(unix))]
fn atty_stderr() -> bool {
    false
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        ("isatty", crate::host::capability(crate::registry::SPEC_TTY_ISATTY)),
        ("ReadStream", crate::host::capability(crate::registry::NodeSpec::new("tty:ReadStream", 0x1401))),
        ("WriteStream", crate::host::capability(crate::registry::NodeSpec::new("tty:WriteStream", 0x1402))),
        ("Socket", crate::host::capability(crate::registry::NodeSpec::new("tty:WriteStream", 0x1402))),
    ]).unwrap_or_else(|_| Value::Undefined)
}

pub fn read_stream(_state: &Rc<RefCell<crate::host::HostState>>, _receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let fd = args.first().map(value_to_i32).unwrap_or(0);
    Ok(crate::host::namespace_object_from_pairs(vec![
        ("fd".into(), Value::Number(fd as f64)), ("isTTY".into(), Value::Boolean(false)),
    ]))
}

pub fn write_stream(state: &Rc<RefCell<crate::host::HostState>>, receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    read_stream(state, receiver, args)
}
