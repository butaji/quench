//! `tty` module — pure Rust `isatty`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::ops::Builtin;
use quench_runtime::value::Value;

pub fn isatty(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = args.first().map(value_to_i32).unwrap_or(0);
    Ok(Value::Boolean(is_terminal_fd(fd)))
}

pub(crate) mod shared_vm;

/// Return whether this process currently has a terminal attached to `fd`.
/// Both runtime entry paths use this OS fact as the authority for tty.isatty.
pub(crate) fn is_terminal_fd(fd: i32) -> bool {
    #[cfg(unix)]
    {
        if fd < 0 {
            return false;
        }
        unsafe { libc_isatty(fd) }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        false
    }
}

pub fn value_to_i32(value: &Value) -> i32 {
    match value {
        Value::Number(n) => *n as i32,
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) != 0 }
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![(
        "isatty",
        crate::host::capability(crate::registry::SPEC_TTY_ISATTY),
    )])
    .unwrap_or_else(|_| Value::Undefined)
}

/// Construct a real object for `tty.ReadStream`/`tty.WriteStream`.
/// The object carries the observable stream fields used by util's color gate.
pub fn stream_construct(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::object(vec![
        (
            "fd".into(),
            args.first().cloned().unwrap_or(Value::Undefined),
        ),
        ("isTTY".into(), Value::Boolean(false)),
        ("columns".into(), Value::Undefined),
        ("rows".into(), Value::Undefined),
        ("write".into(), Value::Builtin(Builtin::Object)),
        ("getColorDepth".into(), Value::Builtin(Builtin::Object)),
        ("hasColors".into(), Value::Builtin(Builtin::Object)),
        ("getWindowSize".into(), Value::Builtin(Builtin::Object)),
    ]))
}
