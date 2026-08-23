//! Small, deterministic compatibility surfaces for modules whose orchestration is
//! provided by the embedding host rather than the JS VM.
use crate::host::HostState;
use quench_runtime::{execute::VmError, value::Value};
use std::cell::RefCell;
use std::rc::Rc;

fn load(source: &str) -> Result<Value, VmError> {
    let wrapped = format!("(function(module){{{source};return module.exports;}})");
    let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
        .map_err(|e| VmError::EvalError(e.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut regs = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(program.ops(), &mut regs, &context)
    })?;
    let module = quench_runtime::execute::define_property(
        quench_runtime::host_api::object(vec![]),
        "exports",
        quench_runtime::host_api::object(vec![
            ("value".to_string(), quench_runtime::host_api::object(vec![])),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]),
    )?;
    let exports = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::call_value(&factory, &Value::Undefined, &[module.clone()])
    })?;
    Ok(exports)
}

fn empty_namespace() -> Value {
    crate::host::namespace_object_from_pairs(vec![])
}

pub fn cluster(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("cluster.js"))
        .or_else(|_| Ok(empty_namespace()))
}
pub fn domain(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("domain.js"))
        .or_else(|_| Ok(empty_namespace()))
}
pub fn diagnostics_channel(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("diagnostics_channel.js"))
        .or_else(|_| Ok(empty_namespace()))
}
pub fn v8(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("v8.js")).or_else(|_| Ok(empty_namespace()))
}
pub fn inspector(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("inspector.js"))
        .or_else(|_| Ok(empty_namespace()))
}
pub fn repl(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("repl.js")).or_else(|_| Ok(empty_namespace()))
}
pub fn wasi(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("wasi.js")).or_else(|_| Ok(empty_namespace()))
}
pub fn worker_threads(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("worker_threads.js")).or_else(|_| {
        Ok(crate::host::namespace_object_from_pairs(vec![
            ("isMainThread".to_string(), Value::Boolean(true)),
            (
                "Worker".to_string(),
                crate::host::capability(crate::registry::NodeSpec::new(
                    "worker_threads:Worker",
                    0x1900,
                )),
            ),
        ]))
    })
}
/// SEA is intentionally unavailable outside a Bun/Node single executable.
/// Keep the documented predicate callable while never claiming SEA support.
pub fn sea_is_sea(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(false))
}
