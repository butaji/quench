//! Rust-owned `trace_events` surface.
//!
//! The module only controls the process-owned trace writer. Event production
//! remains attached to the async-resource lifecycle, so dynamic and flag
//! based tracing share one semantic path.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute;
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
    let categories = args
        .first()
        .map(|options| execute::get_property(options, "categories"))
        .map(|categories| match categories {
            Value::Array(values) => (0..values.logical_len())
                .filter_map(|index| execute::to_js_string(&values.get(index)?).ok())
                .collect::<Vec<_>>(),
            Value::String(value) => vec![value],
            _ => Vec::new(),
        })
        .unwrap_or_default();
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
        (CATEGORIES.into(), Value::String(category_text)),
    ]);
    Ok(tracing)
}

fn receiver_categories(receiver: Option<&Value>) -> Vec<String> {
    let Value::String(value) = execute::get_property(
        receiver.unwrap_or(&Value::Undefined),
        CATEGORIES,
    ) else {
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
    Ok(Value::Undefined)
}

pub fn disable(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    crate::modules::process::trace_disable(state, &receiver_categories(receiver));
    Ok(Value::Undefined)
}

pub fn get_enabled(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Ok(Value::String(crate::modules::process::trace_categories(state)))
}
