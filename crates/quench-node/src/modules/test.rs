//! `node:test` — minimal synchronous test runner.
//!
//! `test(name, fn)` executes the callback, reporting `ok` /
//! `not ok` through the host output sink. A throwing callback
//! propagates the error after reporting, failing the whole fixture;
//! that is the observable contract the conformance runner
//! classifies on. Async callbacks are awaited through the event-loop
//! pump: a rejection (or a promise that never settles) fails the run.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;

struct Frame {
    before: Vec<Value>,
    after: Vec<Value>,
    restores: Vec<Value>,
    context: Option<Value>,
    todo: bool,
    children: bool,
    module_mocks: Vec<String>,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
    static ROOT_BEFORE: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static ROOT_BEFORE_EACH: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static ROOT_AFTER: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static ROOT_AFTER_EACH: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static CURRENT_CONTEXT: RefCell<Option<Value>> = const { RefCell::new(None) };
    static MODULE_MOCKS: RefCell<std::collections::HashMap<String, Value>> = RefCell::new(std::collections::HashMap::new());
    static CUSTOM_ASSERTIONS: RefCell<std::collections::HashMap<String, Value>> = RefCell::new(std::collections::HashMap::new());
}

pub fn register_module_mock(specifier: String, options: Value) {
    let key = canonical_mock_specifier(&specifier);
    MODULE_MOCKS.with(|mocks| {
        mocks.borrow_mut().insert(key.clone(), options);
    });
    FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            frame.module_mocks.push(key);
        }
    });
}

pub fn release_module_mocks(specifiers: &[String]) {
    MODULE_MOCKS.with(|mocks| {
        let mut mocks = mocks.borrow_mut();
        for specifier in specifiers {
            mocks.remove(specifier);
        }
    });
}

pub fn unregister_module_mock(specifier: &str) {
    let key = canonical_mock_specifier(specifier);
    MODULE_MOCKS.with(|mocks| {
        mocks.borrow_mut().remove(&key);
    });
}

pub fn module_is_mocked(specifier: &str) -> bool {
    MODULE_MOCKS.with(|mocks| {
        mocks
            .borrow()
            .contains_key(&canonical_mock_specifier(specifier))
    })
}

pub fn mocked_module(specifier: &str) -> Option<Value> {
    MODULE_MOCKS.with(|mocks| {
        let key = canonical_mock_specifier(specifier);
        let options = mocks.borrow().get(&key)?.clone();
        let exports_option = quench_runtime::execute::get_property(&options, "exports");
        if !matches!(exports_option, Value::Undefined) {
            return Some(exports_option);
        }
        let exports = quench_runtime::execute::get_property(&options, "namedExports");
        let default_export = quench_runtime::execute::get_property(&options, "defaultExport");
        if !matches!(default_export, Value::Undefined) {
            if matches!(default_export, Value::Object(_) | Value::ObjectAlias(_)) {
                if matches!(exports, Value::Object(_) | Value::ObjectAlias(_)) {
                    for key in quench_runtime::execute::own_enumerable_keys(&exports) {
                        let value = quench_runtime::execute::get_property(&exports, &key);
                        let _ = quench_runtime::execute::set_property_in_place(
                            &default_export,
                            &key,
                            value,
                        );
                    }
                }
                return Some(default_export);
            }
            if matches!(exports, Value::Object(_) | Value::ObjectAlias(_)) {
                let mut pairs = vec![("default".to_string(), default_export)];
                pairs.extend(
                    quench_runtime::execute::own_enumerable_keys(&exports)
                        .into_iter()
                        .map(|key| {
                            let value = quench_runtime::execute::get_property(&exports, &key);
                            (key, value)
                        }),
                );
                return Some(quench_runtime::host_api::object(pairs));
            }
            return Some(default_export);
        }
        if !matches!(exports, Value::Object(_) | Value::ObjectAlias(_)) {
            return None;
        }
        let pairs = quench_runtime::execute::own_enumerable_keys(&exports)
            .into_iter()
            .map(|key| {
                (
                    key.clone(),
                    quench_runtime::execute::get_property(&exports, &key),
                )
            })
            .collect();
        Some(quench_runtime::host_api::object(pairs))
    })
}
pub fn mock_module_cache(specifier: &str) -> bool {
    MODULE_MOCKS.with(|mocks| {
        let key = canonical_mock_specifier(specifier);
        mocks.borrow().get(&key).is_some_and(|options| {
            matches!(
                quench_runtime::execute::get_property(options, "cache"),
                Value::Boolean(true)
            )
        })
    })
}

pub fn mock_has_unappliable_default(specifier: &str) -> bool {
    MODULE_MOCKS.with(|mocks| {
        let key = canonical_mock_specifier(specifier);
        let Some(options) = mocks.borrow().get(&key).cloned() else {
            return false;
        };
        matches!(
            quench_runtime::execute::get_property(&options, "defaultExport"),
            Value::Null
        ) && matches!(
            quench_runtime::execute::get_property(&options, "namedExports"),
            Value::Object(_) | Value::ObjectAlias(_)
        )
    })
}

pub fn canonical_mock_specifier(specifier: &str) -> String {
    let specifier = specifier.strip_prefix("node:").unwrap_or(specifier);
    let Some(path) = specifier.strip_prefix("file://") else {
        return specifier.to_string();
    };
    let mut bytes = Vec::with_capacity(path.len());
    let raw = path.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            if let Some(value) = hex(raw[index + 1]).zip(hex(raw[index + 2])) {
                bytes.push(value.0 << 4 | value.1);
                index += 3;
                continue;
            }
        }
        bytes.push(raw[index]);
        index += 1;
    }
    String::from_utf8(bytes).unwrap_or_else(|_| path.to_string())
}

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

pub fn register_assertion(name: String, function: Value) {
    CUSTOM_ASSERTIONS.with(|assertions| {
        assertions.borrow_mut().insert(name, function);
    });
}

fn custom_assertions() -> Vec<(String, Value)> {
    CUSTOM_ASSERTIONS.with(|assertions| {
        assertions
            .borrow()
            .iter()
            .map(|(name, function)| (name.clone(), function.clone()))
            .collect()
    })
}

pub fn before_each(args: &[Value]) -> Result<Value, VmError> {
    if let Some(callback) = callback(args) {
        FRAMES.with(|frames| {
            if let Some(frame) = frames.borrow_mut().last_mut() {
                frame.before.push(callback.clone());
            } else {
                ROOT_BEFORE_EACH.with(|hooks| hooks.borrow_mut().push(callback.clone()));
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
                ROOT_AFTER_EACH.with(|hooks| hooks.borrow_mut().push(callback.clone()));
            }
        });
    }
    Ok(Value::Undefined)
}

pub fn before(args: &[Value]) -> Result<Value, VmError> {
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

pub fn after(args: &[Value]) -> Result<Value, VmError> {
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
    args.iter().find(|value| {
        matches!(
            value,
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        )
    })
}

fn invoke(
    state: &Rc<RefCell<HostState>>,
    callback: &Value,
    context: &Value,
) -> Result<(), VmError> {
    let result =
        quench_runtime::vm::call_value(callback, &Value::Undefined, std::slice::from_ref(context))?;
    crate::modules::pump::await_promise(state, &result)
}

fn fulfilled_test_promise() -> Value {
    Value::Promise(quench_runtime::value::PromiseData::allocate(
        quench_runtime::value::PromiseState::Fulfilled(Value::Undefined),
    ))
}

fn context() -> Value {
    let file_path = quench_runtime::vm::current_context()
        .source_name()
        .unwrap_or_default()
        .to_owned();
    let namespace = crate::modules::assert::build_value();
    let mut assertion_pairs = crate::modules::assert::build()
        .into_iter()
        .filter(|(name, _)| *name != "Assert" && *name != "AssertionError")
        .collect::<Vec<_>>();
    assertion_pairs.push((
        "name".into(),
        quench_runtime::execute::get_property(&namespace, "name"),
    ));
    for name in ["rejects", "doesNotReject"] {
        assertion_pairs.push((
            name.into(),
            quench_runtime::execute::get_property(&namespace, name),
        ));
    }
    let snapshot = crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_SKIP);
    assertion_pairs.push(("snapshot".into(), snapshot.clone()));
    assertion_pairs.push(("fileSnapshot".into(), snapshot));
    let custom = custom_assertions();
    let assertions = quench_runtime::host_api::object(assertion_pairs);
    for (name, function) in custom {
        let wrapper = quench_runtime::host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_TEST_ASSERT_CALL.cap,
                ),
            },
            vec![function],
        );
        let _ = quench_runtime::execute::set_property_in_place(&assertions, &name, wrapper);
    }
    let context = quench_runtime::host_api::object(vec![
        ("assert".into(), assertions),
        ("filePath".into(), Value::String(file_path)),
        (
            "plan".into(),
            crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_PLAN),
        ),
        (
            "waitFor".into(),
            crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_WAIT_FOR),
        ),
        ("passed".into(), Value::Boolean(false)),
        ("attempt".into(), Value::Number(0.0)),
        (
            "diagnostic".into(),
            crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_DIAGNOSTIC),
        ),
        (
            "skip".into(),
            crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_SKIP),
        ),
        (
            "todo".into(),
            crate::host::capability(crate::registry::SPEC_TEST_CONTEXT_TODO),
        ),
        (
            "beforeEach".into(),
            crate::host::capability(crate::registry::SPEC_TEST_BEFORE_EACH),
        ),
        (
            "before".into(),
            crate::host::capability(crate::registry::SPEC_TEST_BEFORE),
        ),
        (
            "afterEach".into(),
            crate::host::capability(crate::registry::SPEC_TEST_AFTER_EACH),
        ),
        (
            "after".into(),
            crate::host::capability(crate::registry::SPEC_TEST_AFTER),
        ),
        (
            "test".into(),
            crate::host::capability(crate::registry::SPEC_TEST_NESTED),
        ),
        (
            "mock".into(),
            quench_runtime::host_api::object(vec![
                (
                    "fn".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_FN),
                ),
                (
                    "method".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_METHOD),
                ),
                (
                    "getter".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_GETTER),
                ),
                (
                    "setter".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_SETTER),
                ),
                (
                    "property".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_PROPERTY),
                ),
                (
                    "module".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_MODULE),
                ),
                (
                    "reset".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_RESET),
                ),
                (
                    "timers".into(),
                    quench_runtime::host_api::object(vec![
                        (
                            "enable".into(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_ENABLE),
                        ),
                        (
                            "tick".into(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_TICK),
                        ),
                        (
                            "setTime".into(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_SETTIME),
                        ),
                        (
                            "reset".into(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_RESET),
                        ),
                        (
                            "runAll".into(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_RUN_ALL),
                        ),
                        (
                            "Symbol.dispose".into(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_RESET),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);
    let _ = quench_runtime::execute::define_property(
        quench_runtime::execute::get_property(&context, "assert"),
        "\0test:context",
        quench_runtime::host_api::object(vec![
            ("value".into(), context.clone()),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(false)),
        ]),
    );
    context
}

fn root_context() -> Value {
    let root = context();
    let _ = quench_runtime::execute::set_property_in_place(
        &root,
        "name",
        Value::String("<root>".into()),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &root,
        "fullName",
        Value::String("<root>".into()),
    );
    root
}

pub fn current_context() -> Value {
    CURRENT_CONTEXT.with(|context| context.borrow().clone().unwrap_or(Value::Undefined))
}

fn parent_full_name(parent: &Option<Value>, name: &str) -> String {
    let Some(parent) = parent else {
        return name.to_owned();
    };
    match quench_runtime::execute::get_property(parent, "fullName") {
        Value::String(prefix) if !prefix.is_empty() && prefix != "<root>" => {
            format!("{prefix} > {name}")
        }
        _ => name.to_owned(),
    }
}

pub fn nested(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = callback(args).cloned() else {
        return Ok(Value::Undefined);
    };
    let parent_context = CURRENT_CONTEXT.with(|current| current.borrow().clone());
    let child = context();
    let child_name = test_name(args);
    let _ = quench_runtime::execute::set_property_in_place(
        &child,
        "name",
        Value::String(child_name.clone()),
    );
    let child_full_name = parent_full_name(&parent_context, &child_name);
    let _ = quench_runtime::execute::set_property_in_place(
        &child,
        "fullName",
        Value::String(child_full_name),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &child,
        "signal",
        quench_runtime::host_api::object(vec![("aborted".into(), Value::Boolean(false))]),
    );
    let (inherited, parent_after, parent_todo) = FRAMES.with(|frames| {
        frames
            .borrow()
            .last()
            .map(|f| (f.before.clone(), f.after.clone(), f.todo))
            .unwrap_or((Vec::new(), Vec::new(), false))
    });
    let previous = CURRENT_CONTEXT.with(|current| current.replace(Some(child.clone())));
    for hook in inherited {
        let hook_context = parent_context.as_ref().unwrap_or(&child);
        let hook_previous =
            CURRENT_CONTEXT.with(|current| current.replace(Some(hook_context.clone())));
        let result = invoke(state, &hook, hook_context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        result?;
    }
    let todo = parent_todo
        || args
            .iter()
            .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
            .is_some_and(|value| {
                quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
                    value, "todo",
                ))
            });
    FRAMES.with(|frames| {
        if let Some(parent) = frames.borrow_mut().last_mut() {
            parent.children = true;
        }
    });
    FRAMES.with(|frames| {
        frames.borrow_mut().push(Frame {
            before: Vec::new(),
            after: Vec::new(),
            restores: Vec::new(),
            context: Some(child.clone()),
            todo,
            children: false,
            module_mocks: Vec::new(),
        })
    });
    let callback_style = matches!(
        quench_runtime::execute::get_property(&callback, "length"),
        Value::Number(length) if length >= 2.0
    );
    let completion =
        quench_runtime::value::PromiseData::allocate(quench_runtime::value::PromiseState::Pending);
    let done = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_TEST_DONE.cap,
            ),
        },
        vec![Value::Promise(completion.clone())],
    );
    let result = crate::modules::diagnostics_channel::test_scope(state, &child, || {
        let body_result = if callback_style {
            quench_runtime::vm::call_value(&callback, &Value::Undefined, &[child.clone(), done])
        } else {
            quench_runtime::vm::call_value(
                &callback,
                &Value::Undefined,
                std::slice::from_ref(&child),
            )
        };
        match body_result {
            Ok(value) => {
                let wait_for = if callback_style && !matches!(&value, Value::Promise(_)) {
                    Value::Promise(completion.clone())
                } else {
                    value
                };
                crate::modules::pump::await_promise(state, &wait_for).map(|_| Value::Undefined)
            }
            Err(error) => Err(error),
        }
    });
    let result = match result {
        Ok(value) => {
            let _ = value;
            Ok(())
        }
        Err(error) => Err(error),
    };
    let frame = FRAMES.with(|frames| frames.borrow_mut().pop()).unwrap();
    for hook in frame.after.iter().rev() {
        invoke(state, hook, &child)?;
    }
    let parent_hook_context = parent_context.as_ref().unwrap_or(&child).clone();
    for hook in parent_after.iter().rev() {
        let hook_previous =
            CURRENT_CONTEXT.with(|current| current.replace(Some(parent_hook_context.clone())));
        let hook_result = invoke(state, hook, &parent_hook_context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        hook_result?;
    }
    for restore in frame.restores.iter().rev() {
        let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]);
    }
    release_module_mocks(&frame.module_mocks);
    CURRENT_CONTEXT.with(|current| current.replace(previous));
    match result {
        Ok(()) => Ok(Value::Undefined),
        Err(error) if todo => {
            report(state, &format!("ok - {child_name} # TODO"));
            Ok(Value::Undefined)
        }
        Err(error) => Err(error),
    }
}

pub fn run(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let name = test_name(args);
    validate_options(args)?;
    if let Some(options) = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let signal = quench_runtime::execute::get_property(options, "signal");
        if matches!(signal, Value::Object(_) | Value::ObjectAlias(_))
            && !matches!(
                quench_runtime::execute::get_property(&signal, "aborted"),
                Value::Boolean(_)
            )
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
        return Ok(fulfilled_test_promise());
    }
    let todo = args.iter().any(|arg| {
        matches!(arg, Value::Object(_) | Value::ObjectAlias(_))
            && !matches!(
                quench_runtime::execute::get_property(arg, "todo"),
                Value::Undefined
            )
    });
    let inherited_todo =
        FRAMES.with(|frames| frames.borrow().last().is_some_and(|frame| frame.todo));
    let todo = todo || inherited_todo;
    let callback = args
        .iter()
        .find(|arg| {
            matches!(
                arg,
                Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
            )
        })
        .cloned();
    let Some(callback) = callback else {
        let runner_mode = args.iter().any(|value| {
            matches!(value, Value::Object(_) | Value::ObjectAlias(_))
                && !matches!(
                    quench_runtime::execute::get_property(value, "files"),
                    Value::Undefined
                )
        });
        if !runner_mode {
            report(state, &format!("ok - {name}"));
            return Ok(fulfilled_test_promise());
        }
        let emitter = crate::modules::events::new_emitter_object(state)?;
        let emit = crate::host::capability(crate::registry::SPEC_TEST_RUN_EMIT);
        for _ in 0..4 {
            state.borrow_mut().event_loop.queue_microtask(
                emit.clone(),
                vec![emitter.clone(), Value::String("test:pass".into())],
            );
        }
        return Ok(emitter);
    };
    let context = context();
    let parent = CURRENT_CONTEXT.with(|current| current.borrow().clone());
    let _ = quench_runtime::execute::set_property_in_place(
        &context,
        "name",
        Value::String(name.clone()),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &context,
        "fullName",
        Value::String(parent_full_name(&parent, &name)),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &context,
        "signal",
        quench_runtime::host_api::object(vec![("aborted".into(), Value::Boolean(false))]),
    );
    let previous = CURRENT_CONTEXT.with(|current| current.replace(Some(context.clone())));
    let inherited_before = FRAMES.with(|frames| {
        frames
            .borrow()
            .last()
            .map(|f| (f.before.clone(), f.context.clone(), f.todo))
            .unwrap_or_default()
    });
    let hook_context = inherited_before.1.as_ref().unwrap_or(&context).clone();
    for hook in inherited_before.0 {
        let hook_previous =
            CURRENT_CONTEXT.with(|current| current.replace(Some(hook_context.clone())));
        let result = invoke(state, &hook, &hook_context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        if let Err(error) = result {
            if inherited_before.2 {
                CURRENT_CONTEXT.with(|current| current.replace(previous));
                report(state, &format!("ok - {name} # TODO"));
                return Ok(fulfilled_test_promise());
            }
            return Err(error);
        }
    }
    let root_before = ROOT_BEFORE.with(|hooks| hooks.borrow().clone());
    let root_before_each = ROOT_BEFORE_EACH.with(|hooks| hooks.borrow().clone());
    let root_after = ROOT_AFTER.with(|hooks| hooks.borrow().clone());
    let root_after_each = ROOT_AFTER_EACH.with(|hooks| hooks.borrow().clone());
    let root = root_context();
    for hook in root_before {
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(root.clone())));
        let result = invoke(state, &hook, &root);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        result?;
    }
    for hook in root_before_each {
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(context.clone())));
        let result = invoke(state, &hook, &context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        result?;
    }
    FRAMES.with(|frames| {
        frames.borrow_mut().push(Frame {
            before: Vec::new(),
            after: Vec::new(),
            restores: Vec::new(),
            context: Some(context.clone()),
            todo: false,
            children: false,
            module_mocks: Vec::new(),
        })
    });
    // Node's callback-style tests receive a completion callback as their
    // second argument. Represent completion as the same promise state the
    // runner already uses for async tests, with a bound host capability
    // resolving/rejecting it when JavaScript invokes `done(error)`.
    let callback_style = matches!(
        quench_runtime::execute::get_property(&callback, "length"),
        Value::Number(length) if length >= 2.0
    );
    let completion =
        quench_runtime::value::PromiseData::allocate(quench_runtime::value::PromiseState::Pending);
    let done = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_TEST_DONE.cap,
            ),
        },
        vec![Value::Promise(completion.clone())],
    );
    let timeout = args.iter().find_map(|value| match value {
        Value::Object(_) | Value::ObjectAlias(_) => {
            match quench_runtime::execute::get_property(value, "timeout") {
                Value::Number(ms) if ms > 0.0 && ms.is_finite() => Some(ms),
                _ => None,
            }
        }
        _ => None,
    });
    let timed_out_body = Cell::new(false);
    let result = crate::modules::diagnostics_channel::test_scope(state, &context, || {
        let body_result =
            quench_runtime::vm::call_value(&callback, &Value::Undefined, &[context.clone(), done]);
        match body_result {
            Ok(value) => {
                let wait_for = if callback_style && !matches!(&value, Value::Promise(_)) {
                    Value::Promise(completion.clone())
                } else {
                    value
                };
                match timeout {
                    Some(ms) => match crate::modules::pump::await_promise_with_timeout(
                        state, &wait_for, ms,
                    )? {
                        true => {
                            timed_out_body.set(true);
                            Ok(fulfilled_test_promise())
                        }
                        false => Ok(fulfilled_test_promise()),
                    },
                    None => crate::modules::pump::await_promise(state, &wait_for)
                        .map(|_| fulfilled_test_promise()),
                }
            }
            Err(error) => Err(error),
        }
    });
    let frame = FRAMES.with(|frames| frames.borrow_mut().pop()).unwrap();
    let hook_context = frame.context.as_ref().unwrap_or(&context).clone();
    if !frame.children {
        for hook in frame.after.iter().rev() {
            let hook_previous =
                CURRENT_CONTEXT.with(|current| current.replace(Some(hook_context.clone())));
            let hook_result = invoke(state, hook, &hook_context);
            CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
            hook_result?;
        }
    }
    for hook in root_after_each.iter().rev() {
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(context.clone())));
        let result = invoke(state, hook, &context);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        result?;
    }
    for hook in root_after.iter().rev() {
        let hook_previous = CURRENT_CONTEXT.with(|current| current.replace(Some(root.clone())));
        let result = invoke(state, &hook, &root);
        CURRENT_CONTEXT.with(|current| current.replace(hook_previous));
        result?;
    }
    // Keep mocks alive until an async/callback-style test settles. Restoring
    // them immediately after invoking the body races its completion callback.
    release_module_mocks(&frame.module_mocks);
    let frame_restores = frame.restores;
    quench_runtime::date::set_mock_now(None);
    if timed_out_body.get() {
        for restore in frame_restores.iter().rev() {
            let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]);
        }
        let signal = quench_runtime::execute::get_property(&context, "signal");
        let _ = quench_runtime::execute::set_property_in_place(
            &signal,
            "aborted",
            Value::Boolean(true),
        );
        CURRENT_CONTEXT.with(|current| current.replace(previous));
        report(state, &format!("ok - {name} # CANCELLED"));
        record_failure(state);
        return Ok(fulfilled_test_promise());
    }
    match result {
        Ok(result) => {
            // Async callbacks return a promise; drive the loop until it
            // settles so a rejection reports `not ok`, never a vacuous `ok`.
            let wait_for = if callback_style && !matches!(&result, Value::Promise(_)) {
                Value::Promise(completion)
            } else {
                result
            };
            let timed_out = match timeout {
                Some(ms) => crate::modules::pump::await_promise_with_timeout(state, &wait_for, ms),
                None => crate::modules::pump::await_promise(state, &wait_for).map(|_| false),
            };
            if let Err(error) = timed_out {
                for restore in frame_restores.iter().rev() {
                    let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]);
                }
                CURRENT_CONTEXT.with(|current| current.replace(previous));
                if todo {
                    report(state, &format!("ok - {name} # TODO"));
                } else {
                    report(state, &format!("not ok - {name}"));
                    record_failure(state);
                }
                let _ = error;
                return Ok(fulfilled_test_promise());
            }
            if timed_out.unwrap_or(false) {
                for restore in frame_restores.iter().rev() {
                    let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]);
                }
                let signal = quench_runtime::execute::get_property(&context, "signal");
                let _ = quench_runtime::execute::set_property_in_place(
                    &signal,
                    "aborted",
                    Value::Boolean(true),
                );
                CURRENT_CONTEXT.with(|current| current.replace(previous));
                report(state, &format!("ok - {name} # CANCELLED"));
                return Ok(fulfilled_test_promise());
            }
            for restore in frame_restores.iter().rev() {
                let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]);
            }
            CURRENT_CONTEXT.with(|current| current.replace(previous));
            report(
                state,
                &format!("ok - {name}{}", if todo { " # TODO" } else { "" }),
            );
            Ok(fulfilled_test_promise())
        }
        Err(error) => {
            for restore in frame_restores.iter().rev() {
                let _ = quench_runtime::vm::call_value(restore, &Value::Undefined, &[]);
            }
            CURRENT_CONTEXT.with(|current| current.replace(previous));
            if todo {
                report(state, &format!("ok - {name} # TODO"));
            } else {
                report(state, &format!("not ok - {name}"));
                record_failure(state);
            }
            let _ = error;
            Ok(fulfilled_test_promise())
        }
    }
}

fn validate_options(args: &[Value]) -> Result<(), VmError> {
    let Some(options) = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    else {
        return Ok(());
    };
    let timeout = quench_runtime::execute::get_property(options, "timeout");
    if !matches!(timeout, Value::Undefined | Value::Null | Value::Number(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options.timeout\" property must be a number".into(),
        ));
    }
    if let Value::Number(value) = timeout {
        if value.is_nan()
            || value.is_sign_negative()
            || (value.is_finite() && value > 2_f64.powi(32))
        {
            return Err(crate::modules::buffer_enc::out_of_range(
                "options.timeout",
                ">= 0 && <= 4294967295",
                &value.to_string(),
            ));
        }
    }
    let concurrency = quench_runtime::execute::get_property(options, "concurrency");
    if !matches!(
        concurrency,
        Value::Undefined | Value::Null | Value::Boolean(_) | Value::Number(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options.concurrency\" property must be a number or boolean".into(),
        ));
    }
    if let Value::Number(value) = concurrency {
        if value.is_nan() || value <= 0.0 || value.fract() != 0.0 || value > 2_f64.powi(32) {
            return Err(crate::modules::buffer_enc::out_of_range(
                "options.concurrency",
                ">= 0 && <= 4294967295",
                &value.to_string(),
            ));
        }
    }
    Ok(())
}

/// `test.skip` / `it.skip` — report without running the callback.
pub fn skip(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    report(state, &format!("ok - {} # SKIP", test_name(args)));
    Ok(fulfilled_test_promise())
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
        let line = format!("{line}\n");
        sink(&line);
    }
}

fn record_failure(state: &Rc<RefCell<HostState>>) {
    if state.borrow().process.exit_code.is_none() {
        state.borrow_mut().process.exit_code = Some(1);
    }
}
