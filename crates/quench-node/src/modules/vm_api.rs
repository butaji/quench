//! Node's `vm` API adapters.
//!
//! This module owns only Node-facing validation and capability wiring. The
//! JavaScript evaluator, realms, and execution state are exclusively owned by
//! `quench-runtime::vm`; keeping that distinction visible prevents a second
//! VM from growing in the host crate.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;

fn invalid_context() -> VmError {
    let error = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String("context must be an object".into())],
        ),
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    );
    VmError::Thrown(error)
}

fn invalid_contextified() -> VmError {
    let error = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(
                "The \"contextifiedObject\" argument must be an vm.Context".into(),
            )],
        ),
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    );
    VmError::Thrown(error)
}

fn invalid_option_property(name: &str, value: &Value) -> VmError {
    let received = match value {
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Boolean(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("'{value}'"),
        _ => "an object".into(),
    };
    let error = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(format!(
                "The \"options.{name}\" property must be of type string. Received {received}"
            ))],
        ),
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    );
    VmError::Thrown(error)
}

fn invalid_options_argument() -> VmError {
    invalid_options_argument_value(&Value::Undefined)
}

fn invalid_options_argument_value(value: &Value) -> VmError {
    let received = match value {
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("type string ('{value}')"),
        _ => "an object".to_string(),
    };
    let error = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(format!(
                "The \"options\" argument must be of type object. Received {received}"
            ))],
        ),
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    );
    VmError::Thrown(error)
}

fn out_of_range_option(name: &str) -> VmError {
    let error = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String(format!(
                "The \"options.{name}\" is out of range."
            ))],
        ),
        "code",
        Value::String("ERR_OUT_OF_RANGE".into()),
    );
    VmError::Thrown(error)
}

fn validate_run_options(options: Option<&Value>, allow_filename: bool) -> Result<(), VmError> {
    let Some(options) = options else {
        return Ok(());
    };
    // The third argument of the legacy run* APIs may be a filename string.
    // Keep that shorthand observable while validating object-form options.
    if allow_filename && matches!(options, Value::String(_)) {
        return Ok(());
    }
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(invalid_options_argument_value(options));
    }
    let timeout = execute::get_property(options, "timeout");
    if !matches!(timeout, Value::Undefined) {
        match timeout {
            Value::Number(value) if value.is_finite() && value.fract() == 0.0 && value > 0.0 => {}
            Value::Number(_) => return Err(out_of_range_option("timeout")),
            other => return Err(invalid_option_property("timeout", &other)),
        }
    }
    for name in ["displayErrors", "breakOnSigint"] {
        let value = execute::get_property(options, name);
        if !matches!(value, Value::Undefined | Value::Boolean(_)) {
            return Err(invalid_option_property(name, &value));
        }
    }
    for name in ["contextName", "contextOrigin"] {
        let value = execute::get_property(options, name);
        if !matches!(value, Value::Undefined | Value::String(_)) {
            return Err(invalid_option_property(name, &value));
        }
    }
    Ok(())
}

pub fn run_in_new_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    if let Some(context) = args.get(1) {
        if !matches!(
            context,
            Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
        ) {
            return Err(invalid_context());
        }
    }
    validate_run_options(args.get(2), true)?;
    let filename = match args.get(2) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(options) => execute::get_property_result(options, "filename")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            }),
        None => None,
    };
    let global = quench_runtime::vm::current_global_object();
    let source = if matches!(
        execute::get_property(&global, "__quench_domain_promises_patched"),
        Value::Boolean(true)
    ) {
        format!(
            "{}\n{}",
            crate::modules::domain::PROMISE_BRIDGE_SOURCE,
            source
        )
    } else {
        source
    };
    quench_runtime::vm::execute_script_in_sandbox(&source, args.get(1), filename.as_deref())
}

pub fn construct_run_in_new_context(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    run_in_new_context(state, None, args)
}

pub fn create_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args.get(1) {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(invalid_options_argument_value(options));
        }
        for name in ["name", "origin"] {
            let value = execute::get_property(options, name);
            if !matches!(value, Value::Undefined | Value::String(_)) {
                return Err(invalid_option_property(name, &value));
            }
        }
    }
    let context = args
        .first()
        .cloned()
        .unwrap_or_else(|| Value::object(vec![]));
    if !matches!(
        context,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        return Err(invalid_context());
    }
    quench_runtime::vm::create_script_context(context)
}

pub fn is_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if !matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        return Err(invalid_context());
    }
    Ok(Value::Boolean(quench_runtime::vm::is_script_context(value)))
}

pub fn run_in_context(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    let context = args.get(1).ok_or_else(invalid_context)?;
    if !matches!(
        context,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        return Err(invalid_context());
    }
    if !quench_runtime::vm::is_script_context(context) {
        return Err(invalid_contextified());
    }
    validate_run_options(args.get(2), true)?;
    let filename = args.get(2).and_then(|options| match options {
        Value::String(value) => Some(value.clone()),
        _ => execute::get_property_result(options, "filename")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            }),
    });
    mark_foreign_result(quench_runtime::vm::execute_script_in_sandbox(
        &source,
        Some(context),
        filename.as_deref(),
    ))
}

fn mark_foreign_result(result: Result<Value, VmError>) -> Result<Value, VmError> {
    result.map(|value| {
        if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
            execute::set_property(value, "\0vm:foreign_context", Value::Boolean(true))
        } else {
            value
        }
    }).map_err(|error| match error {
        VmError::Thrown(value) => VmError::Thrown(execute::set_property(
            value,
            "\0vm:foreign_context",
            Value::Boolean(true),
        )),
        error => error,
    })
}

pub fn run_in_this_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    validate_run_options(args.get(1), true)?;
    let filename = match args.get(1) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(options) => execute::get_property_result(options, "filename")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            }),
        None => None,
    };
    quench_runtime::vm::execute_script_in_current_context(&source, filename.as_deref())
}

pub fn construct_script(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let source = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    if let Some(options) = args.get(1) {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(invalid_options_argument_value(options));
        }
        for name in ["lineOffset", "columnOffset"] {
            let value = execute::get_property(options, name);
            if matches!(value, Value::Undefined) {
                continue;
            }
            match value {
                Value::Number(number)
                    if number.is_finite()
                        && number.fract() == 0.0
                        && (0.0..=u32::MAX as f64).contains(&number) => {}
                Value::Number(_) => return Err(out_of_range_option(name)),
                other => return Err(invalid_option_property(name, &other)),
            }
        }
        let filename = execute::get_property(options, "filename");
        if !matches!(filename, Value::Undefined | Value::String(_)) {
            return Err(invalid_option_property("filename", &filename));
        }
        let produce = execute::get_property(options, "produceCachedData");
        if !matches!(produce, Value::Undefined | Value::Boolean(_)) {
            return Err(invalid_option_property("produceCachedData", &produce));
        }
        let cached = execute::get_property(options, "cachedData");
        if !matches!(
            cached,
            Value::Undefined
                | Value::ArrayBuffer(_)
                | Value::Uint8Array(_)
                | Value::Uint8ClampedArray(_)
                | Value::Uint16Array(_)
                | Value::Uint32Array(_)
                | Value::Int8Array(_)
                | Value::Int16Array(_)
                | Value::Int32Array(_)
                | Value::Float32Array(_)
                | Value::Float64Array(_)
                | Value::DataView(_)
        ) {
            return Err(invalid_options_argument());
        }
    }
    let source_map_url = script_source_map_url(&source);
    let filename = args.get(1).and_then(|options| {
        execute::get_property_result(options, "filename")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
    });
    let script = crate::host::namespace_object_from_pairs(vec![
        (
            "\0vm_script_source".to_string(),
            Value::String(source.clone()),
        ),
        (
            "runInContext".to_string(),
            crate::host::capability(crate::registry::SPEC_VM_SCRIPT_RUN_IN_CONTEXT),
        ),
        (
            "runInNewContext".to_string(),
            crate::host::capability(crate::registry::SPEC_VM_SCRIPT_RUN_IN_NEW_CONTEXT),
        ),
        (
            "createCachedData".to_string(),
            crate::host::capability(crate::registry::SPEC_VM_SCRIPT_CREATE_CACHED_DATA),
        ),
        (
            "runInThisContext".to_string(),
            crate::host::capability(crate::registry::SPEC_VM_SCRIPT_RUN_IN_THIS_CONTEXT),
        ),
        (
            "sourceMapURL".to_string(),
            source_map_url
                .map(Value::String)
                .unwrap_or(Value::Undefined),
        ),
        ("cachedDataProduced".to_string(), Value::Boolean(false)),
        ("cachedDataRejected".to_string(), Value::Boolean(false)),
        (
            "\0vm_script_filename".to_string(),
            filename.map(Value::String).unwrap_or(Value::Undefined),
        ),
    ]);
    Ok(script)
}

fn script_source(receiver: Option<&Value>) -> Result<String, VmError> {
    let Some(receiver) = receiver else {
        return Err(VmError::NotCallable);
    };
    execute::get_property_result(receiver, "\0vm_script_source")
        .and_then(|value| execute::to_js_string(&value))
}

fn script_filename(args: &[Value]) -> Option<String> {
    args.get(1).and_then(|options| {
        execute::get_property_result(options, "filename")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
    })
}

fn script_source_map_url(source: &str) -> Option<String> {
    source
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("//# sourceMappingURL="))
        .map(str::to_owned)
}

pub fn script_run_in_context(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = script_source(receiver)?;
    let context = args.first().ok_or_else(invalid_context)?;
    if !matches!(
        context,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        return Err(invalid_context());
    }
    if !quench_runtime::vm::is_script_context(context) {
        return Err(invalid_contextified());
    }
    validate_run_options(args.get(1), false)?;
    mark_foreign_result(quench_runtime::vm::execute_script_in_sandbox(
        &source,
        Some(context),
        script_filename(args).as_deref(),
    ))
}

pub fn script_run_in_new_context(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = script_source(receiver)?;
    validate_run_options(args.get(1), false)?;
    let context = args
        .first()
        .cloned()
        .unwrap_or_else(|| Value::object(Vec::new()));
    mark_foreign_result(quench_runtime::vm::execute_script_in_sandbox(
        &source,
        Some(&context),
        script_filename(args).as_deref(),
    ))
}

pub fn script_run_in_this_context(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = script_source(receiver)?;
    validate_run_options(args.first(), false)?;
    let filename = args
        .first()
        .and_then(|options| {
            execute::get_property_result(options, "filename")
                .ok()
                .and_then(|value| match value {
                    Value::String(value) => Some(value),
                    _ => None,
                })
        })
        .or_else(|| {
            receiver.and_then(|script| {
                match execute::get_property(script, "\0vm_script_filename") {
                    Value::String(value) => Some(value),
                    _ => None,
                }
            })
        });
    quench_runtime::vm::execute_script_in_current_context(&source, filename.as_deref())
}

pub fn script_create_cached_data(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let bytes = quench_runtime::host_api::bytes(script_source(receiver)?.as_bytes());
    let global = quench_runtime::vm::current_global_object();
    let buffer = execute::get_property(&global, "Buffer");
    let from = execute::get_property(&buffer, "from");
    if quench_runtime::is_callable(&from) {
        execute::call(&from, &buffer, &[bytes])
    } else {
        Ok(bytes)
    }
}

pub fn build() -> Value {
    let run = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_NEW_CONTEXT);
    let create_context = crate::host::capability(crate::registry::SPEC_VM_CREATE_CONTEXT);
    let is_context = crate::host::capability(crate::registry::SPEC_VM_IS_CONTEXT);
    let run_in_context = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_CONTEXT);
    let run_in_this_context = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_THIS_CONTEXT);
    let source_text_module = crate::host::capability(crate::registry::SPEC_VM_SOURCE_TEXT_MODULE);
    let module = crate::host::capability(crate::registry::SPEC_VM_MODULE);
    let compile_function = crate::host::capability(crate::registry::SPEC_VM_COMPILE_FUNCTION);
    let synthetic_module = crate::host::capability(crate::registry::SPEC_VM_SYNTHETIC_MODULE);
    let synthetic_prototype = crate::host::namespace_object_from_pairs(vec![(
        "setExport".into(),
        crate::host::capability(crate::registry::SPEC_VM_SYNTHETIC_SET_EXPORT),
    )]);
    let _ = quench_runtime::execute::set_callable_property(
        &synthetic_module,
        "prototype",
        synthetic_prototype,
    );
    crate::host::namespace_object(vec![
        ("runInNewContext", run.clone()),
        ("runInContext", run_in_context),
        ("runInThisContext", run_in_this_context),
        ("createContext", create_context),
        ("isContext", is_context),
        (
            "Script",
            crate::host::capability(crate::registry::SPEC_VM_SCRIPT),
        ),
        ("SourceTextModule", source_text_module),
        ("Module", module),
        ("compileFunction", compile_function),
        ("SyntheticModule", synthetic_module),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
