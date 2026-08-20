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
promise_op!(lstat, "lstat");
promise_op!(readdir, "readdir");
promise_op!(mkdir, "mkdir");
promise_op!(unlink, "unlink");
promise_op!(rmdir, "rmdir");
promise_op!(rm, "rm");
promise_op!(rename, "rename");
promise_op!(copy_file, "copyFile");
promise_op!(access, "access");
promise_op!(mkdtemp, "mkdtemp");
promise_op!(readlink, "readlink");
promise_op!(chmod, "chmod");
promise_op!(truncate, "truncate");
promise_op!(realpath, "realpath");

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
pub fn filehandle_datasync(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, _a: &[Value]) -> Result<Value, VmError> { Ok(settle(fd(r).map(|_| Value::Undefined))) }
pub fn filehandle_sync(_s: &Rc<RefCell<HostState>>, r: Option<&Value>, _a: &[Value]) -> Result<Value, VmError> { Ok(settle(fd(r).map(|_| Value::Undefined))) }
