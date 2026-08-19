//! `node:test` — minimal synchronous test runner.
//!
//! `test(name, fn)` executes the callback immediately, reporting
//! `ok` / `not ok` through the host output sink. A throwing callback
//! propagates the error after reporting, failing the whole fixture;
//! that is the observable contract the conformance runner
//! classifies on.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn run(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let name = args
        .iter()
        .find_map(|arg| match arg {
            Value::String(name) => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "<anonymous>".to_string());
    // `test(name, { skip: ... }, fn)` — a truthy `skip` option skips the run.
    let skipped = args.iter().any(|arg| {
        matches!(arg, Value::Object(_) | Value::ObjectAlias(_))
            && quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
                arg, "skip",
            ))
    });
    if skipped {
        report(state, &format!("ok - {name} # SKIP"));
        return Ok(Value::Undefined);
    }
    let callback = args.iter().find(|arg| {
        matches!(
            arg,
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        )
    });
    let Some(callback) = callback else {
        return Ok(Value::Undefined);
    };
    match quench_runtime::vm::call_value(callback, &Value::Undefined, &[]) {
        Ok(_) => {
            report(state, &format!("ok - {name}"));
            Ok(Value::Undefined)
        }
        Err(error) => {
            report(state, &format!("not ok - {name}"));
            Err(error)
        }
    }
}

fn report(state: &Rc<RefCell<HostState>>, line: &str) {
    if let Some(sink) = &state.borrow().output {
        sink(line);
    }
}
