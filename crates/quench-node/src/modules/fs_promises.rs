//! `fs.promises` — promise-returning variants. Operations reuse the sync
//! facts and return settled promises; VM promise jobs preserve ordering.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef, RealmId};
use quench_runtime::value::{PromiseData, PromiseState, Value};

use crate::host::HostState;

fn run(state: &Rc<RefCell<HostState>>, args: &[Value], name: &str) -> Result<Value, VmError> {
    let op = super::fs::sync_op(name).ok_or(VmError::NotCallable)?;
    let resource =
        crate::modules::async_hooks::new_resource(state, &[Value::String("FSREQPROMISE".into())])?;
    let result = op(state, None, args);
    crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;

    // Filesystem promises complete at an I/O boundary.  The host operation is
    // intentionally performed eagerly (the shared sync implementation is the
    // semantic source), but resolution is deferred through the check phase so
    // nextTick aborts and other same-turn state changes are observable before
    // the completion is committed.
    let promise = Rc::new(PromiseData::new(PromiseState::Pending));
    let error = super::fs::err_value(&result);
    let value = match result {
        Ok(value) => value,
        Err(_) => Value::Undefined,
    };
    let signal = promise_signal(name, args);
    let context = host_api::object(vec![
        ("\0fs-promise".into(), Value::Promise(Rc::clone(&promise))),
        ("\0fs-value".into(), value),
        ("\0fs-error".into(), error),
        ("\0fs-signal".into(), signal),
    ]);
    let callback = host_api::bound_capability_with_arguments(
        HostCapabilityRef {
            realm: RealmId::ROOT,
            kind: HostCapabilityKind::Custom(crate::registry::SPEC_TIMERS_PROMISE_FINISH.cap),
        },
        vec![context],
    );
    crate::modules::timers::set_immediate(state, &[callback])?;
    Ok(Value::Promise(promise))
}

fn promise_signal(name: &str, args: &[Value]) -> Value {
    let index = match name {
        "writeFile" | "appendFile" => 2,
        _ => 1,
    };
    let Some(options) = args.get(index) else {
        return Value::Undefined;
    };
    let signal = match options {
        Value::Object(_) | Value::ObjectAlias(_) | Value::Proxy(_) => {
            quench_runtime::execute::get_property(options, "signal")
        }
        _ => Value::Undefined,
    };
    matches!(
        signal,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Proxy(_)
    )
    .then_some(signal)
    .unwrap_or(Value::Undefined)
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
