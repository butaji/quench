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

struct Frame {
    before: Vec<Value>,
    after: Vec<Value>,
    restores: Vec<Value>,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

pub fn register_mock_restore(restore: Value) {
    FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            frame.restores.push(restore);
        }
    });
}

pub fn reset_mocks() {
    FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow().last() {
            for restore in frame.restores.iter().rev() {
                let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]);
            }
        }
    });
}

pub fn before_each(args: &[Value]) -> Result<Value, VmError> {
    if let Some(callback) = callback(args) {
        FRAMES.with(|frames| {
            if let Some(frame) = frames.borrow_mut().last_mut() {
                frame.before.push(callback.clone());
            }
        });
    }
    Ok(Value::Undefined)
}

pub fn after_each(args: &[Value]) -> Result<Value, VmError> {
    if let Some(callback) = callback(args) {
        FRAMES.with(|frames| {
            if let Some(frame) = frames.borrow_mut().last_mut() {
                frame.after.push(callback.clone());
            }
        });
    }
    Ok(Value::Undefined)
}

fn callback(args: &[Value]) -> Option<&Value> {
    args.iter().find(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)))
}

fn invoke(state: &Rc<RefCell<HostState>>, callback: &Value, context: &Value) -> Result<(), VmError> {
    let result = quench_runtime::vm::call_value(callback, &Value::Undefined, std::slice::from_ref(context))?;
    crate::modules::pump::await_promise(state, &result)
}

fn context() -> Value {
    quench_runtime::host_api::object(vec![
        ("assert".into(), crate::modules::assert::build_value()),
        ("beforeEach".into(), crate::host::capability(crate::registry::SPEC_TEST_BEFORE_EACH)),
        ("afterEach".into(), crate::host::capability(crate::registry::SPEC_TEST_AFTER_EACH)),
        ("test".into(), crate::host::capability(crate::registry::SPEC_TEST_NESTED)),
        ("mock".into(), quench_runtime::host_api::object(vec![
            ("fn".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_FN)),
            ("method".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_METHOD)),
            ("getter".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_GETTER)),
            ("setter".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_SETTER)),
            ("property".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_PROPERTY)),
            ("module".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_MODULE)),
            ("timers".into(), quench_runtime::host_api::object(vec![
                ("enable".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_ENABLE)),
                ("tick".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_TICK)),
                ("setTime".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_SETTIME)),
                ("reset".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_RESET)),
            ])),
        ])),
    ])
}

pub fn nested(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = callback(args).cloned() else { return Ok(Value::Undefined) };
    let child = context();
    let (inherited, parent_after) = FRAMES.with(|frames| frames.borrow().last().map(|f| (f.before.clone(), f.after.clone())).unwrap_or_default());
    for hook in inherited { invoke(state, &hook, &child)?; }
    FRAMES.with(|frames| frames.borrow_mut().push(Frame { before: Vec::new(), after: Vec::new(), restores: Vec::new() }));
    let result = invoke(state, &callback, &child);
    let frame = FRAMES.with(|frames| frames.borrow_mut().pop()).unwrap();
    for hook in frame.after.iter().rev() { invoke(state, hook, &child)?; }
    for hook in parent_after.iter().rev() { invoke(state, hook, &child)?; }
    for restore in frame.restores.iter().rev() { let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]); }
    result.map(|_| Value::Undefined)
}

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
    let context = context();
    FRAMES.with(|frames| frames.borrow_mut().push(Frame { before: Vec::new(), after: Vec::new(), restores: Vec::new() }));
    let result = quench_runtime::vm::call_value(callback, &Value::Undefined, &[context.clone()]);
    let frame = FRAMES.with(|frames| frames.borrow_mut().pop()).unwrap();
    for restore in frame.restores.iter().rev() { let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]); }
    quench_runtime::date::set_mock_now(None);
    match result {
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
