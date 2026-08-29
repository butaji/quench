//! `fs` async operations — arguments validate synchronously (coded
//! throws like Node), the operation runs eagerly against the real
//! filesystem, and the callback is deferred onto the event loop.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::fs::{async_args, defer, err_value, parse_mkdir_options, parse_options, path_arg};

/// Run one async op: validate arguments synchronously (coded throws
/// like Node), execute eagerly, then defer `cb(err, result)`.
fn run(state: &Rc<RefCell<HostState>>, args: &[Value], name: &str) -> Result<Value, VmError> {
    let (leading, callback) = async_args(args)?;
    if name == "readFile" && matches!(leading.first(), Some(Value::Number(_))) {
        super::fs::descriptor_arg(leading.first())?;
    } else {
        path_arg(leading.first())?;
    }
    if matches!(name, "rename" | "copyFile") {
        path_arg(leading.get(1))?;
    }
    let options = if name == "mkdir" {
        parse_mkdir_options(options_arg(name, leading))?
    } else {
        parse_options(options_arg(name, leading))?
    };
    if options.signal_aborted {
        defer(state, &callback, vec![super::fs_error::abort_error()]);
        return Ok(Value::Undefined);
    }
    let first_created = (name == "mkdir" && options.recursive)
        .then(|| first_missing_path(leading.first()))
        .flatten();
    let op = super::fs::sync_op(name).ok_or(VmError::NotCallable)?;
    let result = op(state, None, leading);
    let error = err_value(&result);
    let value = match result {
        Ok(v) => v,
        Err(_) => Value::Undefined,
    };
    let value = first_created.map(Value::String).unwrap_or(value);
    defer(state, &callback, vec![error, value]);
    Ok(Value::Undefined)
}

fn first_missing_path(value: Option<&Value>) -> Option<String> {
    let mut candidate = match value? {
        Value::String(path) => std::path::PathBuf::from(path),
        _ => return None,
    };
    if candidate.exists() {
        return None;
    }
    while candidate
        .parent()
        .is_some_and(|parent| !parent.exists() && parent != std::path::Path::new(""))
    {
        candidate = candidate.parent()?.to_path_buf();
    }
    Some(candidate.to_string_lossy().into_owned())
}

/// Which argument position carries `options` for this op.
fn options_arg<'a>(name: &str, leading: &'a [Value]) -> Option<&'a Value> {
    match name {
        "writeFile" | "appendFile" => leading.get(2),
        "rename" | "copyFile" | "chmod" | "access" | "truncate" | "unlink" | "rmdir" | "rm" => None,
        _ => leading.get(1),
    }
}

macro_rules! async_op {
    ($func:ident, $name:literal) => {
        pub fn $func(
            state: &Rc<RefCell<HostState>>,
            _r: Option<&Value>,
            args: &[Value],
        ) -> Result<Value, VmError> {
            run(state, args, $name)
        }
    };
}

async_op!(read_file, "readFile");
async_op!(write_file, "writeFile");
async_op!(append_file, "appendFile");
async_op!(stat, "stat");
async_op!(lstat, "lstat");
async_op!(readdir, "readdir");
async_op!(mkdir, "mkdir");
async_op!(unlink, "unlink");
async_op!(rmdir, "rmdir");
async_op!(rm, "rm");
async_op!(rename, "rename");
async_op!(copy_file, "copyFile");
async_op!(mkdtemp, "mkdtemp");
async_op!(readlink, "readlink");
async_op!(chmod, "chmod");
async_op!(truncate, "truncate");
async_op!(realpath, "realpath");

pub fn access(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (leading, callback) = super::fs::async_args(args)?;
    super::fs::path_arg(leading.first())?;
    super::fs_sync::access_mode(leading)?;
    let op = super::fs::sync_op("access").ok_or(VmError::NotCallable)?;
    let result = op(state, None, leading);
    let error = super::fs::err_value(&result);
    super::fs::defer_with_resource(state, &callback, vec![error], "FSREQCALLBACK")?;
    Ok(Value::Undefined)
}

/// `fs.exists(path, callback)` — never errors; the callback receives
/// a single boolean. Invalid path types yield `false` (no throw),
/// matching Node. A missing/non-function callback throws
/// synchronously (`ERR_INVALID_ARG_TYPE`).
pub fn exists(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = super::fs::require_callback(args.get(1))?;
    let exists = match args.first() {
        Some(Value::String(path)) => std::path::Path::new(path).exists(),
        _ => false,
    };
    defer(state, &callback, vec![Value::Boolean(exists)]);
    Ok(Value::Undefined)
}
