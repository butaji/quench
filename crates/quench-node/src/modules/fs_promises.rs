//! `fs.promises` — promise-returning variants. Each op runs the
//! sync implementation and returns an already-settled promise;
//! validation and I/O failures surface as rejections.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::{PromiseData, PromiseState, Value};

use crate::host::HostState;

fn settle(result: Result<Value, VmError>) -> Value {
    let state = match result {
        Ok(value) => PromiseState::Fulfilled(value),
        Err(VmError::Thrown(error)) => PromiseState::Rejected(error),
        Err(_) => PromiseState::Rejected(Value::String("I/O error".to_string())),
    };
    Value::Promise(Rc::new(PromiseData::new(state)))
}

fn run(state: &Rc<RefCell<HostState>>, args: &[Value], name: &str) -> Result<Value, VmError> {
    let op = super::fs::sync_op(name).ok_or(VmError::NotCallable)?;
    Ok(settle(op(state, None, args)))
}

macro_rules! promise_op {
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

promise_op!(read_file, "readFile");
promise_op!(write_file, "writeFile");
promise_op!(append_file, "appendFile");
promise_op!(stat, "stat");
promise_op!(statfs, "statfs");
promise_op!(lstat, "lstat");
promise_op!(readdir, "readdir");
promise_op!(mkdir, "mkdir");
promise_op!(unlink, "unlink");
promise_op!(rmdir, "rmdir");
promise_op!(rm, "rm");
promise_op!(rename, "rename");
promise_op!(copy_file, "copyFile");

/// Promise `access` validates mode instead of inheriting the callback
/// API's permissive coercion. Invalid modes reject with Node-coded errors.
pub fn access(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match args.get(1) {
        Some(Value::Number(mode)) if mode.is_finite() && *mode >= 0.0 && *mode <= 7.0 => {}
        Some(Value::Number(mode)) => {
            return Ok(settle(Err(crate::modules::buffer_enc::out_of_range(
                "mode", ">= 0 && <= 7", &mode.to_string(),
            ))));
        }
        Some(_) => {
            return Ok(settle(Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"mode\" argument must be of type number.".to_string(),
            ))));
        }
        None => {}
    }
    run(state, args, "access")
}
promise_op!(mkdtemp, "mkdtemp");
promise_op!(readlink, "readlink");
promise_op!(chmod, "chmod");
promise_op!(truncate, "truncate");
promise_op!(realpath, "realpath");
promise_op!(chown, "chown");
promise_op!(utimes, "utimes");
promise_op!(link, "link");
promise_op!(symlink, "symlink");
promise_op!(lutimes, "lutimes");
promise_op!(lchown, "lchown");
promise_op!(lchmod, "lchmod");

const FD_PROP: &str = "\0quench:fs:filehandle:fd";

fn fd(receiver: Option<&Value>) -> Result<i32, VmError> {
    match receiver.map(|v| quench_runtime::vm::get_property(v, FD_PROP)) {
        Some(Value::Number(n)) if n.is_finite() => Ok(n as i32),
        _ => Err(crate::modules::buffer_enc::invalid_arg_value("Invalid file descriptor".into())),
    }
}

fn file_handle(fd: i32) -> Result<Value, VmError> {
    let mut obj = crate::host::namespace_object_from_pairs(vec![(FD_PROP.to_string(), Value::Number(fd as f64))]);
    for (name, cap) in [
        ("stat", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_STAT)),
        ("close", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_CLOSE)),
        ("truncate", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_TRUNCATE)),
        ("datasync", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_DATASYNC)),
        ("sync", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_SYNC)),
        ("write", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_WRITE)),
        ("read", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_READ)),
        ("chmod", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_CHMOD)),
        ("chown", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_CHOWN)),
        ("utimes", crate::host::capability(crate::registry::SPEC_FSP_FILEHANDLE_UTIMES)),
    ] {
        let desc = quench_runtime::host_api::object(vec![
            ("value".to_string(), cap), ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(false)), ("configurable".to_string(), Value::Boolean(true)),
        ]);
        obj = quench_runtime::execute::define_property(obj, name, desc)?;
    }
    Ok(obj)
}

pub fn open(_state: &Rc<RefCell<HostState>>, _r: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let path = super::fs::path_arg(args.first())?;
    let flag = match args.get(1) { Some(Value::String(s)) => Some(s.as_str()), None | Some(Value::Undefined) => None, _ => return Err(crate::modules::buffer_enc::invalid_arg_type("The flags argument must be a string".into())) };
    Ok(settle(super::fs::fd_open(&path, flag).and_then(file_handle)))
}

pub fn filehandle_stat(_state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(settle(fd(receiver).and_then(super::fs::fd_stat)))
}

pub fn filehandle_close(_state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(settle(fd(receiver).and_then(super::fs::fd_close)))
}
pub fn filehandle_truncate(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { let n = match a.first() { Some(Value::Number(n)) if *n >= 0.0 => *n as u64, _ => 0 }; Ok(settle(fd(r).and_then(|x| super::fs::fd_truncate(x,n)))) }
fn num_arg(args: &[Value], index: usize, default: usize) -> usize {
    match args.get(index) { Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize, _ => default }
}

pub fn filehandle_write(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value]) -> Result<Value, VmError> {
    let fd = fd(r)?;
    let data = match a.first() { Some(Value::Uint8Array(v)) => v.buffer.bytes.borrow()[v.byte_offset..v.byte_offset + v.length].to_vec(), _ => return Ok(settle(Err(crate::modules::buffer_enc::invalid_arg_type("The \"buffer\" argument must be an instance of Buffer or Uint8Array".into())))) };
    let offset = num_arg(a, 1, 0);
    let length = num_arg(a, 2, data.len().saturating_sub(offset));
    let position = match a.get(3) { Some(Value::Number(n)) if *n >= 0.0 => Some(*n as u64), _ => None };
    let result = super::fs::fd_write(fd, &data, offset.min(data.len()), length, position).map(|n| {
        quench_runtime::host_api::object(vec![("bytesWritten".into(), Value::Number(n as f64)), ("buffer".into(), a[0].clone())])
    });
    Ok(settle(result))
}

pub fn filehandle_read(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value]) -> Result<Value, VmError> {
    let fd = fd(r)?;
    let buffer = match a.first() { Some(Value::Uint8Array(v)) => v.clone(), _ => return Ok(settle(Err(crate::modules::buffer_enc::invalid_arg_type("The \"buffer\" argument must be an instance of Buffer or Uint8Array".into())))) };
    let offset = num_arg(a, 1, 0).min(buffer.length);
    let length = num_arg(a, 2, buffer.length.saturating_sub(offset));
    let position = match a.get(3) { Some(Value::Number(n)) if *n >= 0.0 => Some(*n as u64), _ => None };
    let mut bytes = buffer.buffer.bytes.borrow_mut();
    let result = super::fs::fd_read(fd, &mut bytes, buffer.byte_offset + offset, length, position).map(|n| {
        quench_runtime::host_api::object(vec![("bytesRead".into(), Value::Number(n as f64)), ("buffer".into(), Value::Uint8Array(buffer.clone()))])
    });
    drop(bytes);
    Ok(settle(result))
}
pub fn filehandle_chmod(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value]) -> Result<Value, VmError> {
    let mode = match a.first() { Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as u32, _ => return Ok(settle(Err(crate::modules::buffer_enc::invalid_arg_type("The \"mode\" argument must be a number".into())))) };
    Ok(settle(fd(r).and_then(|x| super::fs::fd_chmod(x, mode))))
}
pub fn filehandle_chown(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value]) -> Result<Value, VmError> {
    let parse = |v: Option<&Value>| match v { Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => Ok(*n as u32), _ => Err(crate::modules::buffer_enc::invalid_arg_type("uid and gid must be numbers".into())) };
    let uid = match parse(a.first()) { Ok(v) => v, Err(e) => return Ok(settle(Err(e))) };
    let gid = match parse(a.get(1)) { Ok(v) => v, Err(e) => return Ok(settle(Err(e))) };
    Ok(settle(fd(r).and_then(|x| super::fs::fd_chown(x, uid, gid))))
}
pub fn filehandle_utimes(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value]) -> Result<Value, VmError> {
    let atime = match super::fs_sync::utimes_time(a.first(), "atime") { Ok(v) => v, Err(e) => return Ok(settle(Err(e))) };
    let mtime = match super::fs_sync::utimes_time(a.get(1), "mtime") { Ok(v) => v, Err(e) => return Ok(settle(Err(e))) };
    Ok(settle(fd(r).and_then(|x| super::fs::fd_utimes(x, atime, mtime))))
}
pub fn filehandle_datasync(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, _a: &[Value]) -> Result<Value, VmError> { Ok(settle(fd(r).map(|_| Value::Undefined))) }
pub fn filehandle_sync(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, _a: &[Value]) -> Result<Value, VmError> { Ok(settle(fd(r).map(|_| Value::Undefined))) }
