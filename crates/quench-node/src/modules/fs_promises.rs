//! `fs.promises` — promise-returning variants. Operations reuse the sync
//! facts and return settled promises; VM promise jobs preserve ordering.

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
    Value::Promise(PromiseData::allocate(state))
}

fn run(state: &Rc<RefCell<HostState>>, args: &[Value], name: &str) -> Result<Value, VmError> {
    let op = super::fs::sync_op(name).ok_or(VmError::NotCallable)?;
    let resource =
        crate::modules::async_hooks::new_resource(state, &[Value::String("FSREQPROMISE".into())])?;
    let promise = settle(op(state, None, args));
    crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
    Ok(promise)
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
promise_op!(symlink, "symlink");
promise_op!(copy_file, "copyFile");
promise_op!(link, "link");
promise_op!(access, "access");
promise_op!(mkdtemp, "mkdtemp");
promise_op!(readlink, "readlink");
promise_op!(chmod, "chmod");
promise_op!(lchmod, "lchmod");
promise_op!(truncate, "truncate");
promise_op!(realpath, "realpath");
promise_op!(statfs, "statfs");
promise_op!(utimes, "utimes");
promise_op!(lutimes, "lutimes");
promise_op!(chown, "chown");
promise_op!(lchown, "lchown");
promise_op!(opendir, "opendir");
