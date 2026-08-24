//! Small compatibility namespaces whose state is supplied by the host layer.
use crate::host::HostState;
use quench_runtime::{execute::VmError, value::Value};
use std::cell::RefCell;
use std::rc::Rc;

fn load(source: &str) -> Result<Value, VmError> {
    let wrapped = format!("(function(module){{{source};return module.exports;}})");
    let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
    })?;
    let module = quench_runtime::host_api::object(vec![(
        "exports".to_string(),
        quench_runtime::host_api::object(vec![]),
    )]);
    quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::call_value(&factory, &Value::Undefined, &[module.clone()])
    })?;
    quench_runtime::execute::get_property_result(&module, "exports")
}

pub fn cluster(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("cluster.js"))
}
pub fn domain(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("domain.js"))
}
pub fn diagnostics_channel(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("diagnostics_channel.js"))
}
pub fn v8(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("v8.js"))
}
pub fn inspector(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("inspector.js"))
}
pub fn repl(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("repl.js"))
}
pub fn wasi(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("wasi.js"))
}
pub fn worker_threads(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("worker_threads.js"))
}
pub fn async_hooks(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    load(include_str!("async_hooks.js"))
}

pub fn sea_is_sea(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(false))
}
