//! Small, deterministic compatibility surfaces for modules whose orchestration is
//! provided by the embedding host rather than the JS VM.
use std::cell::RefCell;
use std::rc::Rc;
use quench_runtime::{execute::VmError, value::Value};
use crate::host::HostState;

fn load(source: &str) -> Result<Value, VmError> {
    let wrapped = format!("(function(module){{{source};return module.exports;}})");
    let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
        .map_err(|e| VmError::EvalError(e.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut regs = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(program.ops(), &mut regs, &context)
    })?;
    let module = quench_runtime::host_api::object(vec![(
        "exports".to_string(),
        quench_runtime::host_api::object(vec![]),
    )]);
    quench_runtime::vm::call_value(&factory, &Value::Undefined, &[module.clone()])?;
    quench_runtime::execute::get_property_result(&module, "exports")
}

pub fn cluster(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("cluster.js"))
}
pub fn domain(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("domain.js"))
}
pub fn v8(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("v8.js"))
}
pub fn worker_threads(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
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
