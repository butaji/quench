//! Rust-owned `trace_events` surface.
//!
//! The module only controls the process-owned trace writer. Event production
//! remains attached to the async-resource lifecycle, so dynamic and flag
//! based tracing share one semantic path.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

const CATEGORIES: &str = "\0quench:trace-events:categories";

pub fn build() -> Value {
    crate::host::namespace_object_from_pairs(vec![
        (
            "createTracing".into(),
            crate::host::capability(crate::registry::SPEC_TRACE_EVENTS_CREATE_TRACING),
        ),
        (
            "getEnabledCategories".into(),
            crate::host::capability(crate::registry::SPEC_TRACE_EVENTS_GET_ENABLED),
        ),
    ])
}

pub fn create_tracing(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    // `createTracing()` is a data constructor, but its input contract is
    // observable: options must be an object and categories must be a
    // non-empty array of strings.  Do the validation at this boundary so a
    // malformed declaration cannot silently create an inert tracer.
    let Some(options) = args.first() else {
        return Err(invalid_arg_type("options", &Value::Undefined));
    };
    if !matches!(options, Value::Object(_) | Value::Proxy(_)) {
        return Err(invalid_arg_type("options", options));
    }
    let categories_value = execute::get_property(options, "categories");
    let Value::Array(values) = categories_value else {
        return Err(invalid_arg_type("options.categories", &categories_value));
    };
    let mut categories = Vec::with_capacity(values.logical_len());
    for index in 0..values.logical_len() {
        let value = values.get(index).unwrap_or(Value::Undefined);
        let Value::String(category) = value else {
            return Err(invalid_arg_type("options.categories", &value));
        };
        categories.push(category);
    }
    if categories.is_empty() {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "message".into(),
                Value::String("At least one category is required".into()),
            ),
            (
                "code".into(),
                Value::String("ERR_TRACE_EVENTS_CATEGORY_REQUIRED".into()),
            ),
        ])));
    }
    let category_text = categories.join(",");
    let tracing = host_api::object(vec![
        (
            "enable".into(),
            crate::host::capability(crate::registry::SPEC_TRACE_EVENTS_ENABLE),
        ),
        (
            "disable".into(),
            crate::host::capability(crate::registry::SPEC_TRACE_EVENTS_DISABLE),
        ),
        ("categories".into(), Value::String(category_text.clone())),
        ("enabled".into(), Value::Boolean(false)),
        (CATEGORIES.into(), Value::String(category_text)),
    ]);
    Ok(tracing)
}

fn receiver_categories(receiver: Option<&Value>) -> Vec<String> {
    let Value::String(value) =
        execute::get_property(receiver.unwrap_or(&Value::Undefined), CATEGORIES)
    else {
        return Vec::new();
    };
    value
        .split(',')
        .filter(|category| !category.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn enable(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    crate::modules::process::trace_enable(state, &receiver_categories(receiver));
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, "enabled", Value::Boolean(true));
    }
    Ok(Value::Undefined)
}

pub fn disable(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    crate::modules::process::trace_disable(state, &receiver_categories(receiver));
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, "enabled", Value::Boolean(false));
    }
    Ok(Value::Undefined)
}

pub fn get_enabled(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let categories = crate::modules::process::trace_categories(state);
    Ok(if categories.is_empty() {
        Value::Undefined
    } else {
        Value::String(categories)
    })
}

pub fn category_enabled(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(category)) = args.first() else {
        return Err(invalid_arg_type(
            "category",
            &args.first().cloned().unwrap_or(Value::Undefined),
        ));
    };
    Ok(Value::Boolean(
        crate::modules::process::trace_category_enabled(state, category),
    ))
}

pub fn category_buffer(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(category)) = args.first() else {
        return Err(invalid_arg_type(
            "category",
            &args.first().cloned().unwrap_or(Value::Undefined),
        ));
    };
    Ok(crate::modules::process::trace_category_buffer(
        state, category,
    ))
}

pub fn trace(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let phase = match args.first() {
        Some(Value::Number(value)) if value.is_finite() => *value as u8,
        Some(Value::String(value)) => value.as_bytes().first().copied().unwrap_or_default(),
        _ => return Ok(Value::Undefined),
    };
    let Some(Value::String(category)) = args.get(1) else {
        return Ok(Value::Undefined);
    };
    let Some(Value::String(name)) = args.get(2) else {
        return Ok(Value::Undefined);
    };
    let id = args.get(3).and_then(|value| match value {
        Value::Number(value) if value.is_finite() && *value >= 0.0 => Some(*value as u64),
        _ => None,
    });
    crate::modules::process::trace_binding_event(state, phase, category, name, id, args.get(4));
    Ok(Value::Undefined)
}

fn invalid_arg_type(name: &str, value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "message".into(),
            Value::String(format!(
                "The \"{name}\" argument must be an object.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
    ]))
}
