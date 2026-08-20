//! `node:test` — minimal synchronous test runner.
//!
//! `test(name, fn)` executes the callback, reporting `ok` /
//! `not ok` through the host output sink. A throwing callback
//! propagates the error after reporting, failing the whole fixture;
//! that is the observable contract the conformance runner
//! classifies on. Async callbacks are awaited through the event-loop
//! pump: a rejection (or a promise that never settles) fails the run.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn run(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let name = test_name(args);
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
    let context = quench_runtime::host_api::object(vec![(
        "assert".to_string(),
        crate::modules::assert::build_value(),
    )]);
    match quench_runtime::vm::call_value(callback, &Value::Undefined, &[context]) {
        Ok(result) => {
            // Async callbacks return a promise; drive the loop until it
            // settles so a rejection reports `not ok`, never a vacuous `ok`.
            if let Err(error) = crate::modules::pump::await_promise(state, &result) {
                report(state, &format!("not ok - {name}"));
                return Err(error);
            }
            report(state, &format!("ok - {name}"));
            Ok(Value::Undefined)
        }
        Err(error) => {
            report(state, &format!("not ok - {name}"));
            Err(error)
        }
    }
}

/// `test.skip` / `it.skip` — report without running the callback.
pub fn skip(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    report(state, &format!("ok - {} # SKIP", test_name(args)));
    Ok(Value::Undefined)
}

fn test_name(args: &[Value]) -> String {
    args.iter()
        .find_map(|arg| match arg {
            Value::String(name) => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "<anonymous>".to_string())
}

fn report(state: &Rc<RefCell<HostState>>, line: &str) {
    if let Some(sink) = &state.borrow().output {
        sink(line);
    }
}
