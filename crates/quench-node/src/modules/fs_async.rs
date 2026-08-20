//! `fs` async operations — arguments validate synchronously (coded
//! throws like Node), the operation runs eagerly against the real
//! filesystem, and the callback is deferred onto the event loop.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::fs::{async_args, defer, err_value, parse_options, path_arg};

/// Run one async op: validate arguments synchronously (coded throws
/// like Node), execute eagerly, then defer `cb(err, result)`.
fn run(state: &Rc<RefCell<HostState>>, args: &[Value], name: &str) -> Result<Value, VmError> {
    let (leading, callback) = async_args(args)?;
    path_arg(leading.first())?;
    if matches!(name, "rename" | "copyFile") {
        path_arg(leading.get(1))?;
    }
    let options = parse_options(options_arg(name, leading))?;
    if options.signal_aborted {
        defer(state, &callback, vec![super::fs_error::abort_error()]);
        return Ok(Value::Undefined);
    }
    let op = super::fs::sync_op(name).ok_or(VmError::NotCallable)?;
    let result = op(state, None, leading);
    let error = err_value(&result);
    let value = match result {
        Ok(v) => v,
        Err(_) => Value::Undefined,
    };
    defer(state, &callback, vec![error, value]);
    Ok(Value::Undefined)
}

/// Which argument position carries `options` for this op.
fn options_arg<'a>(name: &str, leading: &'a [Value]) -> Option<&'a Value> {
    match name {
        "writeFile" | "appendFile" => leading.get(2),
        "rename" | "copyFile" | "chmod" | "access" | "truncate" | "unlink" | "rmdir" | "rm"
        | "mkdtemp" => None,
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
async_op!(access, "access");
async_op!(mkdtemp, "mkdtemp");
async_op!(readlink, "readlink");
async_op!(chmod, "chmod");
async_op!(truncate, "truncate");
async_op!(realpath, "realpath");

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
    // `fs.exists` never throws and never warns for unsupported path
    // types; it invokes the callback with `false` (Node's lenient,
    // historically-deprecated signature).
    let exists = match args.first() {
        Some(Value::String(path)) => std::path::Path::new(path).exists(),
        _ => false,
    };
    defer(state, &callback, vec![Value::Boolean(exists)]);

    Ok(Value::Undefined)
}
/// Run a fd sync op from its async wrapper: argument/type errors throw
/// synchronously (Node), while I/O/runtime errors defer to the callback.
fn run_fd_op(
    state: &Rc<RefCell<HostState>>,
    cb: &Value,
    sync: impl FnOnce(&Rc<RefCell<HostState>>, Option<&Value>, &[Value]) -> Result<Value, VmError>,
    leading: &[Value],
) -> Result<Value, VmError> {
    let result = sync(state, None, leading);
    if let Err(err) = &result {
        if is_validation_error(err) {
            return Err(err.clone());
        }
    }
    super::fs::defer(
        state,
        cb,
        vec![
            super::fs::err_value(&result),
            result.unwrap_or(Value::Undefined),
        ],
    );
    Ok(Value::Undefined)
}

/// True for synchronous argument/type errors Node throws up-front rather
/// than routing through the callback.
fn is_validation_error(err: &VmError) -> bool {
    use quench_runtime::value::Value;
    match err {
        VmError::Thrown(Value::Object(o)) => o.iter().any(|(k, v)| {
            k == "code"
                && matches!(
                    v,
                    Value::String(s) if s == "ERR_INVALID_ARG_TYPE" || s == "ERR_INVALID_ARG_VALUE"
                )
        }),
        _ => false,
    }
}

pub fn open(state: &Rc<RefCell<HostState>>, _r: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let (a, cb) = super::fs::async_args(args)?;
    run_fd_op(state, &cb, super::fs_sync::open_sync, a)
}
pub fn fstat(state: &Rc<RefCell<HostState>>, _r: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let (a, cb) = super::fs::async_args(args)?;
    run_fd_op(state, &cb, super::fs_sync::fstat_sync, a)
}
pub fn close(state: &Rc<RefCell<HostState>>, _r: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let (a, cb) = super::fs::async_args(args)?;
    let result = super::fs_sync::close_sync(state, None, a);
    if let Err(err) = &result {
        if is_validation_error(err) {
            return Err(err.clone());
        }
    }
    super::fs::defer(
        state,
        &cb,
        vec![super::fs::err_value(&result)],
    );
    Ok(Value::Undefined)
}
