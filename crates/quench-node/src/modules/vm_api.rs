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

pub fn run_in_new_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    let filename = args.get(2).and_then(|options| {
        execute::get_property_result(options, "filename")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
    });
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
    let context = args
        .first()
        .cloned()
        .unwrap_or_else(|| Value::object(vec![]));
    quench_runtime::vm::create_script_context(context)
}

pub fn is_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    Ok(Value::Boolean(quench_runtime::vm::is_script_context(value)))
}

pub fn run_in_context(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    run_in_new_context(state, receiver, args)
}

pub fn build() -> Value {
    let run = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_NEW_CONTEXT);
    let create_context = crate::host::capability(crate::registry::SPEC_VM_CREATE_CONTEXT);
    let is_context = crate::host::capability(crate::registry::SPEC_VM_IS_CONTEXT);
    let run_in_context = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_CONTEXT);
    let source_text_module = crate::host::capability(crate::registry::SPEC_VM_SOURCE_TEXT_MODULE);
    crate::host::namespace_object(vec![
        ("runInNewContext", run.clone()),
        ("runInContext", run_in_context),
        ("createContext", create_context),
        ("isContext", is_context),
        ("SourceTextModule", source_text_module),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
