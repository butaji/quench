//! `fs` async operations — arguments validate synchronously (coded
//! throws like Node), the operation runs eagerly against the real
//! filesystem, and the callback is deferred onto the event loop.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::fs::{async_args, defer, err_value, parse_mkdir_options, parse_options, path_arg};

fn eval_function(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}

fn defer_result(
    state: &Rc<RefCell<HostState>>,
    callback: &Value,
    args: Vec<Value>,
    signal: Option<Value>,
) {
    let Some(signal) = signal else {
        defer(state, callback, args);
        return;
    };
    // AbortSignal can transition between the synchronous syscall and the
    // deferred callback (notably when callers abort immediately after
    // `writeFile`). Keep that observable decision at the event-loop edge.
    let Ok(factory) = eval_function(
        "(cb, signal, abort) => (...args) => signal.aborted ? cb(abort) : cb(...args)",
    ) else {
        defer(state, callback, args);
        return;
    };
    let Ok(wrapped) = quench_runtime::execute::call(
        &factory,
        &Value::Undefined,
        &[callback.clone(), signal, super::fs_error::abort_error()],
    ) else {
        defer(state, callback, args);
        return;
    };
    defer(state, &wrapped, args);
}

/// Run one async op: validate arguments synchronously (coded throws
/// like Node), execute eagerly, then defer `cb(err, result)`.
fn run(state: &Rc<RefCell<HostState>>, args: &[Value], name: &str) -> Result<Value, VmError> {
    // Overloaded truncate(path, len?, callback) validates `len` before the
    // callback slot, but the path itself still wins when it is malformed.
    if name == "truncate" {
        path_arg(args.first())?;
        super::fs_sync::truncate_length(args.get(1))?;
    }
    let (leading, callback) = async_args(args)?;
    if matches!(name, "readFile" | "writeFile" | "appendFile")
        && matches!(leading.first(), Some(Value::Number(_)))
    {
        super::fs::descriptor_arg(leading.first())?;
    } else if name == "copyFile" {
        super::fs_sync::copy_path_arg(leading.first(), "src")?;
    } else {
        path_arg(leading.first())?;
    }
    if matches!(name, "rename" | "copyFile" | "link" | "symlink") {
        if name == "copyFile" {
            super::fs_sync::copy_path_arg(leading.get(1), "dest")?;
        } else {
            path_arg(leading.get(1))?;
        }
    }
    if name == "copyFile" {
        super::fs_sync::copy_mode(leading.get(2))?;
    }
    if matches!(name, "chown" | "lchown") {
        super::fs_sync::owner_id(leading.get(1), "uid")?;
        super::fs_sync::owner_id(leading.get(2), "gid")?;
    }
    if name == "chmod" {
        super::fs::chmod_mode(leading.get(1))?;
    }
    if name == "lchmod" {
        super::fs::chmod_mode(leading.get(1))?;
    }
    let options = if name == "mkdir" {
        parse_mkdir_options(options_arg(name, leading))?
    } else {
        parse_options(options_arg(name, leading))?
    };
    let signal = match options_arg(name, leading) {
        Some(value @ (Value::Object(_) | Value::ObjectAlias(_) | Value::Proxy(_))) => {
            let signal = quench_runtime::execute::get_property(value, "signal");
            matches!(
                signal,
                Value::Object(_) | Value::ObjectAlias(_) | Value::Proxy(_)
            )
            .then_some(signal)
        }
        _ => None,
    };
    if matches!(name, "writeFile" | "appendFile") {
        super::fs_sync::validate_data(
            leading.get(1).unwrap_or(&Value::Undefined),
            options.encoding.as_deref(),
        )?;
    }
    if options.signal_aborted {
        defer(state, &callback, vec![super::fs_error::abort_error()]);
        return Ok(Value::Undefined);
    }
    let first_created = (name == "mkdir" && options.recursive)
        .then(|| first_missing_path(leading.first()))
        .flatten();
    let result = if name == "readFile" {
        super::fs_sync::read_file_async(state, None, leading)
    } else {
        let op = super::fs::sync_op(name).ok_or(VmError::NotCallable)?;
        op(state, None, leading)
    };
    if matches!(name, "appendFile" | "writeFile") && options.flush && result.is_ok() {
        // Node reports completion only after the requested asynchronous fsync.
        // Route through the public module so test mocks observe the call.
        let (fd, owned) = if matches!(leading.first(), Some(Value::Number(_))) {
            let fd = super::fs::descriptor_arg(leading.first())?;
            (fd, false)
        } else {
            let path = path_arg(leading.first())?;
            let fd = super::fs::open_sync(
                state,
                None,
                &[Value::String(path), Value::String("r".into())],
            )?;
            let fd = match fd {
                Value::Number(number) => number as i32,
                _ => unreachable!("openSync returns a numeric descriptor"),
            };
            (fd, true)
        };
        let global = quench_runtime::vm::current_global_object();
        let fs_module = quench_runtime::execute::get_property(&global, "__nodeFs");
        let fs_module = if matches!(fs_module, Value::Undefined) {
            state
                .borrow()
                .module_cache
                .get("fs")
                .cloned()
                .unwrap_or_else(super::fs::build)
        } else {
            fs_module
        };
        let fsync = quench_runtime::execute::get_property(&fs_module, "fsync");
        let flush_result = quench_runtime::execute::call(
            &fsync,
            &fs_module,
            &[Value::Number(fd as f64), callback.clone()],
        );
        if owned {
            let _ = super::fs::close_sync(state, None, &[Value::Number(fd as f64)]);
        }
        if flush_result.is_ok() {
            return Ok(Value::Undefined);
        }
        let error = super::fs::err_value(&flush_result);
        super::fs::defer(state, &callback, vec![error]);
        return Ok(Value::Undefined);
    }
    let error = err_value(&result);
    let value = match result {
        Ok(v) => v,
        Err(_) => Value::Undefined,
    };
    let value = first_created.map(Value::String).unwrap_or(value);
    defer_result(state, &callback, vec![error, value], signal);
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
        "rename" | "copyFile" | "link" | "symlink" | "chmod" | "lchmod" | "access" | "truncate"
        | "unlink" | "chown" | "lchown" | "utimes" | "lutimes" => None,
        // Both removal APIs accept an options object before the callback.
        "rmdir" | "rm" => leading.get(1),
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
async_op!(link, "link");
async_op!(symlink, "symlink");
async_op!(chown, "chown");
async_op!(lchown, "lchown");
async_op!(utimes, "utimes");
async_op!(lutimes, "lutimes");
async_op!(mkdtemp, "mkdtemp");
async_op!(readlink, "readlink");
async_op!(chmod, "chmod");
async_op!(lchmod, "lchmod");
async_op!(truncate, "truncate");
async_op!(realpath, "realpath");

pub fn realpath_native(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    run(state, args, "realpathNative")
}

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
