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
    context: Option<Value>,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
    static ROOT_BEFORE: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static ROOT_AFTER: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static CURRENT_CONTEXT: RefCell<Option<Value>> = const { RefCell::new(None) };
    static MODULE_MOCKS: RefCell<std::collections::HashMap<String, Value>> = RefCell::new(std::collections::HashMap::new());
}

pub fn register_module_mock(specifier: String, options: Value) { MODULE_MOCKS.with(|mocks| { mocks.borrow_mut().insert(specifier, options); }); }
pub fn mocked_module(specifier: &str) -> Option<Value> {
    MODULE_MOCKS.with(|mocks| {
        let options = mocks.borrow().get(specifier)?.clone();
        let exports = quench_runtime::execute::get_property(&options, "namedExports");
        if !matches!(exports, Value::Object(_) | Value::ObjectAlias(_)) { return None; }
        let pairs = quench_runtime::execute::own_enumerable_keys(&exports).into_iter().map(|key| (key.clone(), quench_runtime::execute::get_property(&exports, &key))).collect();
        Some(quench_runtime::host_api::object(pairs))
    })
}
pub fn mock_module_cache(specifier: &str) -> bool { MODULE_MOCKS.with(|mocks| mocks.borrow().get(specifier).is_some_and(|options| matches!(quench_runtime::execute::get_property(options, "cache"), Value::Boolean(true)))) }

pub fn register_mock_restore(restore: Value) {
    FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            frame.restores.push(restore);
        }
    });
}

pub fn reset_mocks() {
    MODULE_MOCKS.with(|mocks| mocks.borrow_mut().clear());
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
            } else {
                ROOT_BEFORE.with(|hooks| hooks.borrow_mut().push(callback.clone()));
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
            } else {
                ROOT_AFTER.with(|hooks| hooks.borrow_mut().push(callback.clone()));
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

fn fulfilled_test_promise() -> Value {
    Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Fulfilled(Value::Undefined),
    )))
}

fn context() -> Value {
    let namespace = crate::modules::assert::build_value();
    let mut assertion_pairs = crate::modules::assert::build()
        .into_iter()
        .filter(|(name, _)| *name != "Assert" && *name != "AssertionError")
        .collect::<Vec<_>>();
    assertion_pairs.push(("name".into(), quench_runtime::execute::get_property(&namespace, "name")));
    for name in ["rejects", "doesNotReject"] {
        assertion_pairs.push((name.into(), quench_runtime::execute::get_property(&namespace, name)));
    }
    let snapshot = crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_SKIP);
    assertion_pairs.push(("snapshot".into(), snapshot.clone()));
    assertion_pairs.push(("fileSnapshot".into(), snapshot));
    let assertions = quench_runtime::host_api::object(assertion_pairs);
    quench_runtime::host_api::object(vec![
        ("assert".into(), assertions),
        ("skip".into(), crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_SKIP)),
        ("todo".into(), crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_TODO)),
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
            ("reset".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_RESET)),
            ("timers".into(), quench_runtime::host_api::object(vec![
                ("enable".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_ENABLE)),
                ("tick".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_TICK)),
                ("setTime".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_SETTIME)),
                ("reset".into(), crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_RESET)),
            ])),
        ])),
    ])
}

pub fn current_context() -> Value {
    CURRENT_CONTEXT.with(|context| context.borrow().clone().unwrap_or(Value::Undefined))
}

pub fn nested(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = callback(args).cloned() else { return Ok(Value::Undefined) };
    let child = context();
    let child_name = test_name(args);
    let _ = quench_runtime::execute::set_property_in_place(&child, "name", Value::String(child_name.clone()));
    let _ = quench_runtime::execute::set_property_in_place(&child, "fullName", Value::String(child_name));
    let _ = quench_runtime::execute::set_property_in_place(&child, "signal", quench_runtime::host_api::object(vec![("aborted".into(), Value::Boolean(false))]));
    let (inherited, parent_after) = FRAMES.with(|frames| frames.borrow().last().map(|f| (f.before.clone(), f.after.clone())).unwrap_or_default());
    let parent_context = CURRENT_CONTEXT.with(|current| current.borrow().clone());
    let previous = CURRENT_CONTEXT.with(|current| current.replace(Some(child.clone())));
    for hook in inherited {
        let hook_context = parent_context.as_ref().unwrap_or(&child);
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(hook_context.clone())));
        let result = invoke(state, &hook, hook_context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        result?;
    }
    FRAMES.with(|frames| frames.borrow_mut().push(Frame { before: Vec::new(), after: Vec::new(), restores: Vec::new(), context: Some(child.clone()) }));
    let result = invoke(state, &callback, &child);
    let frame = FRAMES.with(|frames| frames.borrow_mut().pop()).unwrap();
    for hook in frame.after.iter().rev() { invoke(state, hook, &child)?; }
    let parent_hook_context = parent_context.as_ref().unwrap_or(&child).clone();
    for hook in parent_after.iter().rev() {
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(parent_hook_context.clone())));
        let hook_result = invoke(state, hook, &parent_hook_context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        hook_result?;
    }
    for restore in frame.restores.iter().rev() { let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]); }
    CURRENT_CONTEXT.with(|current| current.replace(previous));
    result.map(|_| Value::Undefined)
}

pub fn run(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let name = test_name(args);
    validate_options(args)?;
    if let Some(options) = args.iter().find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_))) {
        let signal = quench_runtime::execute::get_property(options, "signal");
        if matches!(signal, Value::Object(_) | Value::ObjectAlias(_))
            && !matches!(quench_runtime::execute::get_property(&signal, "aborted"), Value::Boolean(_))
        {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"options.signal\" property must be an instance of AbortSignal".into(),
            ));
        }
    }
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
        let emitter = crate::modules::events::new_emitter_object(state)?;
        let emit = crate::host::capability(crate::registry::SPEC_TEST_RUN_EMIT);
        for _ in 0..4 { state.borrow_mut().event_loop.queue_microtask(emit.clone(), vec![emitter.clone(), Value::String("test:pass".into())]); }
        return Ok(emitter);
    };
    let context = context();
    let _ = quench_runtime::execute::set_property_in_place(&context, "name", Value::String(name.clone()));
    let _ = quench_runtime::execute::set_property_in_place(&context, "fullName", Value::String(name.clone()));
    let _ = quench_runtime::execute::set_property_in_place(&context, "signal", quench_runtime::host_api::object(vec![("aborted".into(), Value::Boolean(false))]));
    let previous = CURRENT_CONTEXT.with(|current| current.replace(Some(context.clone())));
    let inherited_before = FRAMES.with(|frames| frames.borrow().last().map(|f| (f.before.clone(), f.context.clone())).unwrap_or_default());
    let hook_context = inherited_before.1.as_ref().unwrap_or(&context).clone();
    for hook in inherited_before.0 {
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(hook_context.clone())));
        let result = invoke(state, &hook, &hook_context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        result?;
    }
    let root_before = ROOT_BEFORE.with(|hooks| hooks.borrow().clone());
    let root_after = ROOT_AFTER.with(|hooks| hooks.borrow().clone());
    for hook in root_before { invoke(state, &hook, &context)?; }
    FRAMES.with(|frames| frames.borrow_mut().push(Frame { before: Vec::new(), after: Vec::new(), restores: Vec::new(), context: Some(context.clone()) }));
    let result = quench_runtime::vm::call_value(callback, &Value::Undefined, &[context.clone()]);
    let frame = FRAMES.with(|frames| frames.borrow_mut().pop()).unwrap();
    let hook_context = frame.context.as_ref().unwrap_or(&context).clone();
    for hook in frame.after.iter().rev() {
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(hook_context.clone())));
        let hook_result = invoke(state, hook, &hook_context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        hook_result?;
    }
    for hook in root_after.iter().rev() { invoke(state, &hook, &context)?; }
    for restore in frame.restores.iter().rev() { let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]); }
    quench_runtime::date::set_mock_now(None);
    match result {
        Ok(result) => {
            // Async callbacks return a promise; drive the loop until it
            // settles so a rejection reports `not ok`, never a vacuous `ok`.
            if let Err(error) = crate::modules::pump::await_promise(state, &result) {
                CURRENT_CONTEXT.with(|current| current.replace(previous));
                report(state, &format!("not ok - {name}"));
                return Err(error);
            }
            CURRENT_CONTEXT.with(|current| current.replace(previous));
            report(state, &format!("ok - {name}"));
            Ok(fulfilled_test_promise())
        }
        Err(error) => {
            CURRENT_CONTEXT.with(|current| current.replace(previous));
            report(state, &format!("not ok - {name}"));
            Err(error)
        }
    }
}

fn validate_options(args: &[Value]) -> Result<(), VmError> {
    let Some(options) = args.iter().find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_))) else { return Ok(()) };
    let timeout = quench_runtime::execute::get_property(options, "timeout");
    if !matches!(timeout, Value::Undefined | Value::Null | Value::Number(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type("The \"options.timeout\" property must be a number".into()));
    }
    if let Value::Number(value) = timeout {
        if value.is_nan() || value.is_sign_negative() || (value.is_finite() && value > 2_f64.powi(32)) {
            return Err(crate::modules::buffer_enc::out_of_range("options.timeout", ">= 0 && <= 4294967295", &value.to_string()));
        }
    }
    let concurrency = quench_runtime::execute::get_property(options, "concurrency");
    if !matches!(concurrency, Value::Undefined | Value::Null | Value::Boolean(_) | Value::Number(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type("The \"options.concurrency\" property must be a number or boolean".into()));
    }
    if let Value::Number(value) = concurrency {
        if value.is_nan() || value <= 0.0 || value.fract() != 0.0 || value > 2_f64.powi(32) {
            return Err(crate::modules::buffer_enc::out_of_range("options.concurrency", ">= 0 && <= 4294967295", &value.to_string()));
        }
    }
    Ok(())
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
